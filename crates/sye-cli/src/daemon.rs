use std::fmt::Write as _;
use std::path::Path;
use std::process::{Child, Command, ExitCode, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use sye_core::Locale;
use sye_core::config::{BreakType, Config};
use sye_core::ipc::{IpcCommand, IpcResponse};
use sye_core::scheduler::{
    OverlayOutcome, Scheduler, SchedulerAction, SchedulerEvent, SchedulerState,
};
use sye_core::session::SessionState;
use sye_core::traits::DndQueryOptions;
#[cfg(not(feature = "msstore"))]
use sye_core::{
    HOMEBREW_UPGRADE_COMMAND, PUBLIC_DOWNLOAD_URL, UPDATE_CHECK_INTERVAL_SECS,
    UPDATE_INITIAL_DELAY_SECS, UpdateCacheState, UpdateInfo, UpdateStrategy,
};
use sye_platform::{
    AppPaths, IpcClient, IpcError, IpcServer, OsAudioPlayer, OsNotifier, PlatformEventLoop,
    SubprocessOverlayLauncher, SystemClock, SystemDndProvider, SystemIdleProvider, TrayEvent,
    TrayHandle,
};

const POLL_INTERVAL: Duration = Duration::from_millis(25);
const CONFIG_POLL_INTERVAL_ACTIVE: Duration = Duration::from_millis(500);
const CONFIG_POLL_INTERVAL_IDLE: Duration = Duration::from_secs(1);
const DAEMON_HANDOFF_TIMEOUT: Duration = Duration::from_millis(2500);
const DAEMON_HANDOFF_POLL_INTERVAL: Duration = Duration::from_millis(50);
const DAEMON_HANDOFF_IPC_TIMEOUT: Duration = Duration::from_millis(250);
pub(crate) const HANDOFF_ENV_VAR: &str = "SYE_HANDOFF_IN_PROGRESS";
pub(crate) const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");
#[cfg(not(feature = "msstore"))]
const UPDATE_CACHE_POLL_INTERVAL: Duration = Duration::from_millis(500);
// The macOS and Windows banners have no auto-hide, so the daemon closes update notices itself.
#[cfg(not(feature = "msstore"))]
const UPDATE_NOTICE_DURATION: Duration = Duration::from_secs(8);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LoopControl {
    Continue,
    Quit,
    Restart,
}

struct DaemonHost<'a> {
    headless: bool,
    locale: Locale,
    config_file: &'a Path,
    session_file: &'a Path,
    socket_path: &'a Path,
    event_loop: Option<&'a PlatformEventLoop>,
    notifier: OsNotifier,
    audio: OsAudioPlayer,
    overlay_launcher: Option<SubprocessOverlayLauncher>,
    overlay_launched: bool,
    overlay_launch_failed: bool,
    overlay_awaiting_shown: bool,
    pending_notification_dismiss: bool,
    settings_child: Option<Child>,
    settings_exited: bool,
    tray: Option<TrayHandle>,
    #[cfg(not(feature = "msstore"))]
    available_update: Option<UpdateInfo>,
    #[cfg(not(feature = "msstore"))]
    update_in_progress: bool,
    #[cfg(not(feature = "msstore"))]
    update_apply_rx: Option<mpsc::Receiver<Result<(), String>>>,
    headless_break_deadline: Option<Instant>,
    #[cfg(not(feature = "msstore"))]
    update_notice_dismiss_at: Option<Instant>,
}

impl DaemonHost<'_> {
    fn execute(&mut self, actions: Vec<SchedulerAction>) {
        for action in actions {
            match action {
                SchedulerAction::NotifyPreBreak {
                    break_spec,
                    warning_secs,
                    total_secs,
                } => {
                    if !self.headless {
                        self.pending_notification_dismiss = false;
                        let _ = self.notifier.show_pre_break(
                            break_spec.break_type,
                            &break_spec.name,
                            warning_secs,
                            total_secs,
                        );
                    }
                }
                SchedulerAction::DismissNotification => {
                    if !self.headless {
                        if self.overlay_awaiting_shown {
                            self.pending_notification_dismiss = true;
                        } else {
                            self.pending_notification_dismiss = false;
                            let _ = self.notifier.dismiss();
                        }
                    }
                }
                SchedulerAction::PlayChime { kind, volume } => {
                    if !self.headless {
                        let _ = self.audio.play_chime(kind, volume);
                    }
                }
                SchedulerAction::ShowOverlay(req) => {
                    if self.headless {
                        self.headless_break_deadline =
                            Instant::now().checked_add(Duration::from_secs(req.duration_secs));
                    } else if let Some(launcher) = self.overlay_launcher.as_mut() {
                        if let Some(el) = self.event_loop {
                            el.prepare_child_activation();
                        }
                        match launcher.launch(&req) {
                            Ok(()) => {
                                self.overlay_launched = true;
                                self.overlay_launch_failed = false;
                                self.overlay_awaiting_shown = true;
                            }
                            Err(err) => {
                                self.mark_overlay_launch_failed();
                                eprintln!("Warning: failed to launch break overlay: {err}");
                            }
                        }
                    } else {
                        self.mark_overlay_launch_failed();
                        eprintln!("Warning: break overlay launcher is unavailable");
                    }
                }
                SchedulerAction::CloseOverlay => {
                    self.headless_break_deadline = None;
                    if let Err(err) = self.close_overlay_inner() {
                        eprintln!("Warning: failed to close break overlay: {err}");
                    }
                }
                SchedulerAction::UpdateTray(snapshot) => {
                    self.refresh_tray(&snapshot);
                }
                SchedulerAction::PersistSession(session) => {
                    if let Err(err) = session.save_to_path(self.session_file) {
                        eprintln!("Warning: failed to persist session state: {err}");
                    }
                }
            }
        }
    }

    fn refresh_tray(&mut self, snapshot: &sye_core::scheduler::TraySnapshot) {
        if let Some(handle) = self.tray.as_mut() {
            let _ = handle.update(snapshot);
        }
    }

    #[cfg(not(feature = "msstore"))]
    fn sync_tray_update(&mut self) {
        if let Some(handle) = self.tray.as_mut() {
            handle.set_available_update(
                self.available_update
                    .as_ref()
                    .map(|info| info.latest_version.as_str()),
                self.update_in_progress,
            );
        }
    }

    fn set_locale(&mut self, locale: Locale) {
        if self.locale == locale {
            return;
        }
        self.locale = locale;
        self.notifier.set_locale(locale);
        if let Some(handle) = self.tray.as_mut() {
            handle.set_locale(locale);
        }
    }

    #[cfg(not(feature = "msstore"))]
    fn show_update_notice(&mut self, summary: &str, body: &str) {
        if self.notifier.show_notice(summary, body).is_ok() {
            self.update_notice_dismiss_at = Some(Instant::now() + UPDATE_NOTICE_DURATION);
        }
    }

    #[cfg(not(feature = "msstore"))]
    fn expire_update_notice(&mut self, state: SchedulerState) {
        let Some(deadline) = self.update_notice_dismiss_at else {
            return;
        };
        if state == SchedulerState::PreBreak {
            self.update_notice_dismiss_at = None;
        } else if !self.update_in_progress && Instant::now() >= deadline {
            self.update_notice_dismiss_at = None;
            let _ = self.notifier.dismiss();
        }
    }

    #[cfg(not(feature = "msstore"))]
    fn apply_cached_update(&mut self, cache: &UpdateCacheState) {
        self.available_update = cache.available_update(CURRENT_VERSION);
        self.sync_tray_update();
    }

    #[cfg(not(feature = "msstore"))]
    fn trigger_tray_update(&mut self) {
        if self.headless || self.update_in_progress {
            return;
        }
        let Some(info) = self.available_update.clone() else {
            return;
        };

        match sye_platform::detect_current_update_strategy() {
            UpdateStrategy::SelfUpdate { asset_name } => {
                self.update_in_progress = true;
                self.sync_tray_update();
                let (summary, body) = match self.locale {
                    Locale::En => (
                        "Safe Your Eyes — Updating",
                        format!("Downloading and verifying v{}...", info.latest_version),
                    ),
                    Locale::Vi => (
                        "Safe Your Eyes — Đang cập nhật",
                        format!("Đang tải và xác minh v{}...", info.latest_version),
                    ),
                };
                self.show_update_notice(summary, &body);
                let (tx, rx) = mpsc::channel();
                let version = info.latest_version;
                let spawned = std::thread::Builder::new()
                    .name("sye-self-update".to_owned())
                    .spawn(move || {
                        let res =
                            sye_platform::apply_self_update(&version, asset_name, CURRENT_VERSION)
                                .map_err(|err| err.to_string());
                        let _ = tx.send(res);
                    });
                match spawned {
                    Ok(_) => self.update_apply_rx = Some(rx),
                    Err(err) => {
                        self.update_in_progress = false;
                        self.sync_tray_update();
                        eprintln!("Warning: failed to spawn self-update thread: {err}");
                        let _ = sye_platform::open_url(&info.download_url);
                    }
                }
            }
            UpdateStrategy::HomebrewCask => {
                let (summary, body) = match self.locale {
                    Locale::En => (
                        format!("Safe Your Eyes v{} available", info.latest_version),
                        format!("Run: {HOMEBREW_UPGRADE_COMMAND}"),
                    ),
                    Locale::Vi => (
                        format!("Đã có Safe Your Eyes v{}", info.latest_version),
                        format!("Chạy: {HOMEBREW_UPGRADE_COMMAND}"),
                    ),
                };
                self.show_update_notice(&summary, &body);
                if let Err(err) = sye_platform::open_url(&info.download_url) {
                    eprintln!("Warning: failed to open download page: {err}");
                }
            }
            UpdateStrategy::ManualDownload => {
                if let Err(err) = sye_platform::open_url(&info.download_url) {
                    eprintln!("Warning: failed to open download page: {err}");
                }
            }
        }
    }

    #[cfg(not(feature = "msstore"))]
    fn poll_update_apply(&mut self) -> bool {
        let Some(rx) = self.update_apply_rx.as_ref() else {
            return false;
        };
        match rx.try_recv() {
            Ok(Ok(())) => {
                self.update_apply_rx = None;
                self.update_in_progress = false;
                true
            }
            Ok(Err(err)) => {
                self.update_apply_rx = None;
                self.update_in_progress = false;
                self.sync_tray_update();
                eprintln!("Warning: self-update failed: {err}");
                let (summary, body) = match self.locale {
                    Locale::En => (
                        "Safe Your Eyes — Update failed",
                        "Opening the download page instead.",
                    ),
                    Locale::Vi => (
                        "Safe Your Eyes — Cập nhật thất bại",
                        "Đang mở trang tải xuống thay thế.",
                    ),
                };
                self.show_update_notice(summary, body);
                let url = self
                    .available_update
                    .as_ref()
                    .map_or(PUBLIC_DOWNLOAD_URL, |info| info.download_url.as_str());
                let _ = sye_platform::open_url(url);
                false
            }
            Err(mpsc::TryRecvError::Empty) => false,
            Err(mpsc::TryRecvError::Disconnected) => {
                self.update_apply_rx = None;
                self.update_in_progress = false;
                self.sync_tray_update();
                false
            }
        }
    }

    fn dismiss_pending_notification(&mut self) {
        if std::mem::take(&mut self.pending_notification_dismiss) && !self.headless {
            let _ = self.notifier.dismiss();
        }
    }

    fn take_overlay_launched(&mut self) -> bool {
        std::mem::take(&mut self.overlay_launched)
    }

    fn take_overlay_launch_failed(&mut self) -> bool {
        std::mem::take(&mut self.overlay_launch_failed)
    }

    fn poll_overlay_shown(&mut self) -> bool {
        if self.headless {
            return false;
        }
        let shown = self
            .overlay_launcher
            .as_mut()
            .is_some_and(SubprocessOverlayLauncher::poll_shown);
        if shown {
            self.overlay_awaiting_shown = false;
            self.dismiss_pending_notification();
        }
        shown
    }

    fn poll_overlay_outcome(&mut self) -> Option<OverlayOutcome> {
        if self.headless {
            if let Some(deadline) = self.headless_break_deadline
                && Instant::now() >= deadline
            {
                self.headless_break_deadline = None;
                return Some(OverlayOutcome::Completed);
            }
            None
        } else {
            let launcher = self.overlay_launcher.as_mut()?;
            match launcher.poll_outcome() {
                Ok(outcome) => {
                    if outcome.is_some() {
                        self.overlay_awaiting_shown = false;
                        self.dismiss_pending_notification();
                    }
                    outcome
                }
                Err(err) => {
                    eprintln!("Warning: failed to poll overlay subprocess: {err}");
                    None
                }
            }
        }
    }

    fn open_settings(&mut self) {
        if self.headless {
            return;
        }
        self.reap_settings_child();
        if let Some(ref child) = self.settings_child {
            if let Some(el) = self.event_loop {
                el.activate_child(child.id());
            }
            return;
        }
        if let Some(el) = self.event_loop {
            el.prepare_child_activation();
        }
        match spawn_settings_subprocess(self.config_file, self.socket_path) {
            Ok(child) => self.settings_child = Some(child),
            Err(err) => eprintln!("Warning: failed to open settings window: {err}"),
        }
    }

    fn reap_settings_child(&mut self) {
        if let Some(child) = self.settings_child.as_mut()
            && matches!(child.try_wait(), Ok(Some(_)) | Err(_))
        {
            self.settings_child = None;
            self.settings_exited = true;
        }
    }

    fn take_settings_exited(&mut self) -> bool {
        self.reap_settings_child();
        std::mem::take(&mut self.settings_exited)
    }

    fn close_settings(&mut self) {
        if let Some(mut child) = self.settings_child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }

    fn mark_overlay_launch_failed(&mut self) {
        self.overlay_launched = false;
        self.overlay_launch_failed = true;
        self.overlay_awaiting_shown = false;
        self.dismiss_pending_notification();
    }

    fn close_overlay_inner(&mut self) -> Result<(), sye_core::traits::PlatformError> {
        self.overlay_launched = false;
        self.overlay_launch_failed = false;
        self.overlay_awaiting_shown = false;
        self.dismiss_pending_notification();
        if let Some(launcher) = self.overlay_launcher.as_mut() {
            launcher.close()?;
        }
        Ok(())
    }

    fn close_overlay(&mut self) {
        let _ = self.close_overlay_inner();
    }
}

fn sync_overlay_launch_status(host: &mut DaemonHost<'_>, scheduler: &mut Scheduler<SystemClock>) {
    if host.take_overlay_launched() {
        host.execute(scheduler.handle_event(SchedulerEvent::OverlayLaunching));
    }
    if host.take_overlay_launch_failed() {
        host.execute(
            scheduler.handle_event(SchedulerEvent::OverlayFinished(OverlayOutcome::Closed)),
        );
    }
}

fn file_fingerprint(path: &Path) -> Option<(SystemTime, u64)> {
    let meta = std::fs::metadata(path).ok()?;
    Some((meta.modified().ok()?, meta.len()))
}

fn poll_config_change(
    path: &Path,
    force_read: bool,
    last_fingerprint: &mut Option<(SystemTime, u64)>,
    current_config: &Config,
) -> Option<Config> {
    let fingerprint = file_fingerprint(path);
    if !force_read && fingerprint == *last_fingerprint {
        return None;
    }
    let changed = fingerprint != *last_fingerprint;
    *last_fingerprint = fingerprint;
    match Config::load_from_path(path) {
        Ok(updated) => (updated != *current_config).then_some(updated),
        Err(err) => {
            if changed {
                eprintln!(
                    "Warning: ignoring invalid config file {}: {err}",
                    path.display()
                );
            }
            None
        }
    }
}

#[cfg(not(feature = "msstore"))]
fn poll_update_cache_change(
    path: &Path,
    force_read: bool,
    last_fingerprint: &mut Option<(SystemTime, u64)>,
    last_cache: &mut UpdateCacheState,
) -> Option<UpdateCacheState> {
    let fingerprint = file_fingerprint(path);
    if !force_read && fingerprint == *last_fingerprint {
        return None;
    }
    let (cache, err) = UpdateCacheState::load_or_default(path);
    if err.is_some() {
        return None;
    }
    *last_fingerprint = fingerprint;
    if cache == *last_cache {
        return None;
    }
    *last_cache = cache.clone();
    Some(cache)
}

pub(crate) fn should_replace_running_daemon(
    snapshot: &sye_core::scheduler::StatusSnapshot,
    current_version: &str,
) -> bool {
    snapshot.daemon_version.as_deref() != Some(current_version)
        && !matches!(
            snapshot.state,
            SchedulerState::PreBreak | SchedulerState::Break
        )
}

pub(crate) fn query_running_daemon_status(
    socket_path: &Path,
) -> Option<sye_core::scheduler::StatusSnapshot> {
    match IpcClient::send_command_with_timeout(
        socket_path,
        &IpcCommand::Status,
        DAEMON_HANDOFF_IPC_TIMEOUT,
    ) {
        Ok(IpcResponse::Status(snapshot)) => Some(snapshot),
        _ => None,
    }
}

pub(crate) fn sync_autostart_warn(enabled: bool, config_file: &Path) {
    if let Err(err) = sye_platform::sync_system_autostart(enabled, Some(config_file)) {
        eprintln!("Warning: failed to synchronize autostart entry: {err}");
    }
}

fn bind_or_replace_daemon(
    socket_path: &Path,
    headless: bool,
    current_version: &str,
) -> Result<IpcServer, IpcError> {
    match IpcServer::bind(socket_path) {
        Ok(srv) => Ok(srv),
        Err(IpcError::AlreadyRunning) if !headless => {
            let Some(snapshot) = query_running_daemon_status(socket_path) else {
                return Err(IpcError::AlreadyRunning);
            };
            if !should_replace_running_daemon(&snapshot, current_version) {
                return Err(IpcError::AlreadyRunning);
            }
            if !matches!(
                IpcClient::send_command_with_timeout(
                    socket_path,
                    &IpcCommand::Quit,
                    DAEMON_HANDOFF_IPC_TIMEOUT,
                ),
                Ok(IpcResponse::Ok { .. })
            ) {
                return Err(IpcError::AlreadyRunning);
            }
            let deadline = Instant::now() + DAEMON_HANDOFF_TIMEOUT;
            loop {
                match IpcServer::bind(socket_path) {
                    Ok(srv) => return Ok(srv),
                    Err(IpcError::AlreadyRunning) => {
                        if Instant::now() >= deadline {
                            return Err(IpcError::AlreadyRunning);
                        }
                        std::thread::sleep(DAEMON_HANDOFF_POLL_INTERVAL);
                    }
                    Err(err) => return Err(err),
                }
            }
        }
        Err(err) => Err(err),
    }
}

fn disable_minutes_to_secs(minutes: Option<u64>) -> Option<u64> {
    minutes.filter(|&m| m > 0).map(|m| m.saturating_mul(60))
}

fn rng_seed_from_clock() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0x5EED_CAFE, |d| {
            let nanos = d.as_nanos();
            let low = u64::try_from(nanos & u128::from(u64::MAX)).unwrap_or(0);
            let high = u64::try_from(nanos >> 64).unwrap_or(0);
            low ^ high
        })
}

pub(crate) fn run_daemon(paths: &AppPaths, headless: bool, background: bool) -> ExitCode {
    let server = match bind_or_replace_daemon(&paths.socket_path, headless, CURRENT_VERSION) {
        Ok(srv) => srv,
        Err(IpcError::AlreadyRunning) => {
            if std::env::var_os(HANDOFF_ENV_VAR).is_some() {
                return ExitCode::FAILURE;
            }
            if !headless && !background {
                let _ = IpcClient::send_command(&paths.socket_path, &IpcCommand::OpenSettings);
            }
            println!("Safe Your Eyes is already running");
            return ExitCode::SUCCESS;
        }
        Err(err) => {
            eprintln!("Failed to bind IPC socket: {err}");
            return ExitCode::FAILURE;
        }
    };
    if !headless {
        sye_platform::detach_console();
    }
    if let Err(err) = server.set_accept_nonblocking() {
        eprintln!("Failed to configure non-blocking IPC listener: {err}");
        return ExitCode::FAILURE;
    }

    let is_first_run = !paths.config_file.exists();
    let (mut config, load_err) = Config::load_or_default(&paths.config_file);
    let config_loaded = load_err.is_none();
    if let Some(err) = load_err {
        eprintln!("Warning: falling back to default config ({err})");
    }
    if !headless {
        let sys_autostart = (!is_first_run)
            .then(|| sye_platform::query_system_autostart(Some(&paths.config_file)))
            .flatten();
        match plan_startup_autostart(is_first_run, config_loaded, config.autostart, sys_autostart) {
            StartupAutostartPlan::RegisterAndPersist(enabled) => {
                sync_autostart_warn(enabled, &paths.config_file);
                if let Err(err) = config.save_to_path(&paths.config_file) {
                    eprintln!("Warning: failed to write initial config: {err}");
                }
            }
            StartupAutostartPlan::AdoptSystem(sys) => {
                config.autostart = sys;
            }
            StartupAutostartPlan::SyncFromConfig(enabled) => {
                sync_autostart_warn(enabled, &paths.config_file);
            }
            StartupAutostartPlan::KeepSystem => {}
        }
    }

    let persist_state = config.persist_state;
    let system_locale = sye_platform::detect_system_locale();
    let mut scheduler = Scheduler::new(config, SystemClock::new(), rng_seed_from_clock());
    scheduler.set_system_locale(system_locale);
    let active_locale = scheduler.active_locale();
    if persist_state {
        let (session, session_err) = SessionState::load_or_default(&paths.session_file);
        if let Some(err) = session_err {
            eprintln!("Warning: ignoring unreadable session file ({err})");
        } else {
            scheduler.restore_session(&session);
        }
    }

    let idle_provider = SystemIdleProvider::new();
    let dnd_provider = SystemDndProvider::new();
    let (tray_tx, tray_rx) = mpsc::channel::<TrayEvent>();
    let app_loop = (!headless).then(|| PlatformEventLoop::new(tray_tx.clone()));
    let tray = if headless {
        None
    } else {
        match TrayHandle::try_new_with_locale(&scheduler.tray_snapshot(), active_locale, tray_tx) {
            Ok(handle) => Some(handle),
            Err(err) => {
                eprintln!("Warning: system tray unavailable: {err}");
                None
            }
        }
    };
    let notifier = if headless {
        OsNotifier::noop()
    } else {
        let notifier = OsNotifier::with_locale(active_locale);
        notifier.set_theme_mode(scheduler.config().theme);
        notifier
    };
    #[cfg(not(feature = "msstore"))]
    if !headless {
        sye_platform::cleanup_current_stale_old_executable();
    }

    #[cfg(not(feature = "msstore"))]
    let initial_update_cache_fingerprint = (!headless)
        .then(|| file_fingerprint(&paths.update_cache_file))
        .flatten();
    #[cfg(not(feature = "msstore"))]
    let (cached_update, _) = if headless {
        (UpdateCacheState::default(), None)
    } else {
        UpdateCacheState::load_or_default(&paths.update_cache_file)
    };
    #[cfg(not(feature = "msstore"))]
    let initial_available_update = cached_update.available_update(CURRENT_VERSION);

    let mut host = DaemonHost {
        headless,
        locale: active_locale,
        config_file: &paths.config_file,
        session_file: &paths.session_file,
        socket_path: &paths.socket_path,
        event_loop: app_loop.as_ref(),
        notifier,
        audio: OsAudioPlayer::new(),
        overlay_launcher: (!headless)
            .then(|| SubprocessOverlayLauncher::current_exe().ok())
            .flatten(),
        overlay_launched: false,
        overlay_launch_failed: false,
        overlay_awaiting_shown: false,
        pending_notification_dismiss: false,
        settings_child: None,
        settings_exited: false,
        tray,
        #[cfg(not(feature = "msstore"))]
        available_update: initial_available_update,
        #[cfg(not(feature = "msstore"))]
        update_in_progress: false,
        #[cfg(not(feature = "msstore"))]
        update_apply_rx: None,
        headless_break_deadline: None,
        #[cfg(not(feature = "msstore"))]
        update_notice_dismiss_at: None,
    };
    #[cfg(not(feature = "msstore"))]
    host.sync_tray_update();

    let mut last_policy_check = Instant::now();
    let mut last_config_poll = Instant::now();
    #[cfg(not(feature = "msstore"))]
    let mut last_update_cache_poll = Instant::now();
    let mut config_fingerprint = file_fingerprint(&paths.config_file);
    #[cfg(not(feature = "msstore"))]
    let mut update_cache_fingerprint = initial_update_cache_fingerprint;
    #[cfg(not(feature = "msstore"))]
    let mut last_update_cache = cached_update;
    #[cfg(not(feature = "msstore"))]
    let mut update_check_rx: Option<mpsc::Receiver<Option<UpdateInfo>>> = None;
    #[cfg(not(feature = "msstore"))]
    let mut next_update_check_at = compute_next_update_check(&last_update_cache);
    let mut restart_after_exit = false;

    host.execute(scheduler.handle_event(SchedulerEvent::Start));
    let launched_at_login = app_loop
        .as_ref()
        .is_some_and(PlatformEventLoop::was_launched_as_login_item);
    if should_open_settings_on_startup(headless, background, launched_at_login) {
        host.open_settings();
    }

    loop {
        match drain_ipc_requests(&server, &mut scheduler, &mut host) {
            LoopControl::Continue => {}
            LoopControl::Quit => break,
            LoopControl::Restart => {
                restart_after_exit = true;
                break;
            }
        }
        if !drain_tray_events(&tray_rx, &mut scheduler, &mut host) {
            break;
        }

        let settings_exited = host.take_settings_exited();
        let config_poll_interval = if host.settings_child.is_some() {
            CONFIG_POLL_INTERVAL_ACTIVE
        } else {
            CONFIG_POLL_INTERVAL_IDLE
        };
        // An outside edit must not close an active break overlay. The fingerprint stays
        // unchanged, so the edit is applied after the break.
        let defer_idle_reload = host.settings_child.is_none()
            && matches!(
                scheduler.state(),
                SchedulerState::PreBreak | SchedulerState::Break
            );
        if settings_exited
            || (!defer_idle_reload && last_config_poll.elapsed() >= config_poll_interval)
        {
            last_config_poll = Instant::now();
            let force_read = settings_exited || host.settings_child.is_some();
            if let Some(updated_cfg) = poll_config_change(
                &paths.config_file,
                force_read,
                &mut config_fingerprint,
                scheduler.config(),
            ) {
                if !headless && updated_cfg.autostart != scheduler.config().autostart {
                    sync_autostart_warn(updated_cfg.autostart, &paths.config_file);
                }
                let actions = scheduler.handle_event(SchedulerEvent::ReloadConfig(updated_cfg));
                host.set_locale(scheduler.active_locale());
                host.notifier.set_theme_mode(scheduler.config().theme);
                host.execute(actions);
            }
        }

        #[cfg(not(feature = "msstore"))]
        if !headless {
            if settings_exited || last_update_cache_poll.elapsed() >= UPDATE_CACHE_POLL_INTERVAL {
                last_update_cache_poll = Instant::now();
                let force_read = settings_exited || host.settings_child.is_some();
                if let Some(cache) = poll_update_cache_change(
                    &paths.update_cache_file,
                    force_read,
                    &mut update_cache_fingerprint,
                    &mut last_update_cache,
                ) {
                    host.apply_cached_update(&cache);
                    next_update_check_at = compute_next_update_check(&cache);
                }
            }

            if let Some(rx) = update_check_rx.as_ref() {
                match rx.try_recv() {
                    Ok(found) => {
                        update_check_rx = None;
                        host.available_update = found;
                        host.sync_tray_update();
                        update_cache_fingerprint = file_fingerprint(&paths.update_cache_file);
                        if let (cache, None) =
                            UpdateCacheState::load_or_default(&paths.update_cache_file)
                        {
                            last_update_cache = cache;
                        }
                        next_update_check_at =
                            Instant::now() + Duration::from_secs(UPDATE_CHECK_INTERVAL_SECS);
                    }
                    Err(mpsc::TryRecvError::Empty) => {}
                    Err(mpsc::TryRecvError::Disconnected) => {
                        update_check_rx = None;
                        next_update_check_at =
                            Instant::now() + Duration::from_secs(UPDATE_CHECK_INTERVAL_SECS);
                    }
                }
            } else if scheduler.config().check_for_updates && Instant::now() >= next_update_check_at
            {
                let (tx, rx) = mpsc::channel();
                let cache_path = paths.update_cache_file.clone();
                if std::thread::Builder::new()
                    .name("sye-update-check".to_owned())
                    .spawn(move || {
                        if let Ok(found) = sye_platform::check_and_cache_update(
                            &cache_path,
                            CURRENT_VERSION,
                            sye_platform::now_unix_secs(),
                        ) {
                            let _ = tx.send(found);
                        }
                    })
                    .is_ok()
                {
                    update_check_rx = Some(rx);
                } else {
                    next_update_check_at =
                        Instant::now() + Duration::from_secs(UPDATE_CHECK_INTERVAL_SECS);
                }
            }

            host.expire_update_notice(scheduler.state());
            if host.poll_update_apply() {
                host.execute(scheduler.handle_event(SchedulerEvent::Quit));
                restart_after_exit = true;
                break;
            }
        }

        sync_overlay_launch_status(&mut host, &mut scheduler);
        if host.poll_overlay_shown() {
            host.execute(scheduler.handle_event(SchedulerEvent::OverlayShown));
        }
        if let Some(outcome) = host.poll_overlay_outcome() {
            host.execute(scheduler.handle_event(SchedulerEvent::OverlayFinished(outcome)));
        }

        if !headless && last_policy_check.elapsed() >= Duration::from_secs(1) {
            last_policy_check = Instant::now();
            if let Ok(idle) = idle_provider.idle_duration() {
                host.execute(scheduler.handle_event(SchedulerEvent::IdleObserved(idle)));
            }
        }

        let tick_actions = if !headless && scheduler.needs_policy_check() {
            let current_idle = idle_provider.idle_duration().ok();
            let dnd_cfg = &scheduler.config().do_not_disturb;
            let dnd_ctx = if dnd_cfg.needs_query() {
                dnd_provider
                    .query_dnd_context_with(DndQueryOptions {
                        check_fullscreen: dnd_cfg.enabled,
                        check_gaming: dnd_cfg.while_gaming,
                    })
                    .ok()
            } else {
                None
            };
            scheduler.tick_with_policies(current_idle, dnd_ctx.as_ref())
        } else {
            scheduler.tick()
        };
        if !tick_actions.is_empty() {
            host.execute(tick_actions);
            sync_overlay_launch_status(&mut host, &mut scheduler);
        }
        host.refresh_tray(&scheduler.tray_snapshot());

        if let Some(ref event_loop) = app_loop {
            event_loop.pump_events(POLL_INTERVAL);
        } else {
            std::thread::sleep(POLL_INTERVAL);
        }
    }

    host.close_overlay();
    host.close_settings();
    drop(host);
    drop(server);
    if restart_after_exit && let Err(err) = spawn_restarted_daemon(paths, headless) {
        eprintln!("Warning: failed to restart updated daemon: {err}");
    }
    if let Some(event_loop) = app_loop {
        event_loop.finish_termination();
    }

    ExitCode::SUCCESS
}

#[cfg(not(feature = "msstore"))]
fn compute_next_update_check(cached: &UpdateCacheState) -> Instant {
    let now = sye_platform::now_unix_secs();
    if cached.is_check_due(now) {
        Instant::now() + Duration::from_secs(UPDATE_INITIAL_DELAY_SECS)
    } else {
        let elapsed = now.saturating_sub(cached.checked_at_unix_secs);
        let remaining = UPDATE_CHECK_INTERVAL_SECS
            .saturating_sub(elapsed)
            .max(UPDATE_INITIAL_DELAY_SECS);
        Instant::now() + Duration::from_secs(remaining)
    }
}

fn drain_ipc_requests(
    server: &IpcServer,
    scheduler: &mut Scheduler<SystemClock>,
    host: &mut DaemonHost<'_>,
) -> LoopControl {
    loop {
        let mut loop_control = LoopControl::Continue;
        let mut open_settings = false;
        let mut deferred_actions: Vec<SchedulerAction> = Vec::new();

        let accepted = server.accept_once(Duration::from_millis(10), |cmd| match cmd {
            IpcCommand::Status => {
                let mut snapshot = scheduler.status_snapshot();
                snapshot.daemon_version = Some(CURRENT_VERSION.to_owned());
                IpcResponse::Status(snapshot)
            }
            IpcCommand::Enable => {
                deferred_actions.extend(scheduler.handle_event(SchedulerEvent::Enable));
                IpcResponse::Ok {
                    message: Some("Enabled".to_owned()),
                }
            }
            IpcCommand::Disable { minutes } => {
                let duration_secs = disable_minutes_to_secs(minutes);
                deferred_actions
                    .extend(scheduler.handle_event(SchedulerEvent::Disable { duration_secs }));
                IpcResponse::Ok {
                    message: Some("Disabled".to_owned()),
                }
            }
            IpcCommand::TakeBreak { kind } => {
                deferred_actions.extend(scheduler.handle_event(SchedulerEvent::TakeBreak { kind }));
                IpcResponse::Ok {
                    message: Some("Break triggered".to_owned()),
                }
            }
            IpcCommand::OpenSettings => {
                if can_open_settings(scheduler.state()) {
                    open_settings = true;
                    IpcResponse::Ok {
                        message: Some("Opening settings".to_owned()),
                    }
                } else {
                    IpcResponse::Ok { message: None }
                }
            }
            IpcCommand::Restart => {
                loop_control = LoopControl::Restart;
                deferred_actions.extend(scheduler.handle_event(SchedulerEvent::Quit));
                IpcResponse::Ok {
                    message: Some("Restarting".to_owned()),
                }
            }
            IpcCommand::Quit => {
                loop_control = LoopControl::Quit;
                deferred_actions.extend(scheduler.handle_event(SchedulerEvent::Quit));
                IpcResponse::Ok {
                    message: Some("Quitting".to_owned()),
                }
            }
        });

        if !deferred_actions.is_empty() {
            host.execute(deferred_actions);
        }
        if open_settings {
            host.open_settings();
        }
        if loop_control != LoopControl::Continue {
            return loop_control;
        }

        match accepted {
            Ok(true) => {}
            Ok(false) => return LoopControl::Continue,
            Err(err) => {
                eprintln!("Warning: IPC accept error: {err}");
                return LoopControl::Continue;
            }
        }
    }
}

fn drain_tray_events(
    tray_rx: &mpsc::Receiver<TrayEvent>,
    scheduler: &mut Scheduler<SystemClock>,
    host: &mut DaemonHost<'_>,
) -> bool {
    let mut keep_running = true;
    while let Ok(event) = tray_rx.try_recv() {
        let actions = match event {
            TrayEvent::Enable => scheduler.handle_event(SchedulerEvent::Enable),
            TrayEvent::Disable { minutes } => scheduler.handle_event(SchedulerEvent::Disable {
                duration_secs: disable_minutes_to_secs(minutes),
            }),
            TrayEvent::TakeBreak { kind } => {
                scheduler.handle_event(SchedulerEvent::TakeBreak { kind })
            }
            TrayEvent::OpenSettings | TrayEvent::Reopen => {
                if can_open_settings(scheduler.state()) {
                    host.open_settings();
                }
                Vec::new()
            }
            #[cfg(not(feature = "msstore"))]
            TrayEvent::UpdateApp => {
                if can_open_settings(scheduler.state()) {
                    host.trigger_tray_update();
                }
                Vec::new()
            }
            TrayEvent::Quit => {
                keep_running = false;
                scheduler.handle_event(SchedulerEvent::Quit)
            }
        };
        host.execute(actions);
    }
    keep_running
}

const fn should_open_settings_on_startup(
    headless: bool,
    background: bool,
    launched_at_login: bool,
) -> bool {
    !headless && !background && !launched_at_login
}

const fn can_open_settings(state: SchedulerState) -> bool {
    !matches!(state, SchedulerState::PreBreak | SchedulerState::Break)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StartupAutostartPlan {
    RegisterAndPersist(bool),
    AdoptSystem(bool),
    SyncFromConfig(bool),
    KeepSystem,
}

const fn plan_startup_autostart(
    is_first_run: bool,
    config_loaded: bool,
    config_autostart: bool,
    system_autostart: Option<bool>,
) -> StartupAutostartPlan {
    if is_first_run {
        StartupAutostartPlan::RegisterAndPersist(config_autostart)
    } else if let Some(sys) = system_autostart {
        StartupAutostartPlan::AdoptSystem(sys)
    } else if config_loaded {
        StartupAutostartPlan::SyncFromConfig(config_autostart)
    } else {
        // A broken `config.toml` falls back to defaults, which must not overwrite the login item.
        StartupAutostartPlan::KeepSystem
    }
}

fn spawn_self_detached(
    config_file: &Path,
    socket_path: &Path,
    strip_deleted: bool,
    configure: impl FnOnce(&mut Command),
) -> std::io::Result<Child> {
    #[cfg(target_os = "linux")]
    let resolved = if strip_deleted {
        let exe = strip_linux_deleted_exe_suffix(std::env::current_exe()?);
        exe.canonicalize().unwrap_or(exe)
    } else {
        sye_platform::resolve_current_exe()?
    };
    #[cfg(not(target_os = "linux"))]
    let resolved = {
        let _ = strip_deleted;
        sye_platform::resolve_current_exe()?
    };
    let mut cmd = Command::new(resolved);
    configure(&mut cmd);
    cmd.arg("--config")
        .arg(config_file)
        .arg("--socket")
        .arg(socket_path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    sye_platform::hide_child_console(&mut cmd).spawn()
}

fn spawn_settings_subprocess(config_file: &Path, socket_path: &Path) -> std::io::Result<Child> {
    spawn_self_detached(config_file, socket_path, false, |cmd| {
        cmd.arg("--internal-settings").env_remove(HANDOFF_ENV_VAR);
    })
}

pub(crate) fn hand_off_to_new_daemon(paths: &AppPaths) -> bool {
    let Ok(mut child) = spawn_daemon_for_handoff(paths) else {
        return false;
    };
    // The new daemon exits when the old daemon does not quit. The caller then opens Settings
    // itself, so that the user's request is not lost.
    let deadline = Instant::now() + DAEMON_HANDOFF_TIMEOUT + 2 * DAEMON_HANDOFF_IPC_TIMEOUT;
    while Instant::now() < deadline {
        if !matches!(child.try_wait(), Ok(None)) {
            return false;
        }
        if query_running_daemon_status(&paths.socket_path)
            .is_some_and(|s| s.daemon_version.as_deref() == Some(CURRENT_VERSION))
        {
            return true;
        }
        std::thread::sleep(DAEMON_HANDOFF_POLL_INTERVAL);
    }
    true
}

fn spawn_daemon_for_handoff(paths: &AppPaths) -> std::io::Result<Child> {
    spawn_self_detached(&paths.config_file, &paths.socket_path, false, |cmd| {
        cmd.env(HANDOFF_ENV_VAR, "1");
    })
}

#[cfg(any(target_os = "linux", test))]
fn strip_linux_deleted_exe_suffix(exe: std::path::PathBuf) -> std::path::PathBuf {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        if let Some(stripped) = exe.as_os_str().as_bytes().strip_suffix(b" (deleted)") {
            return std::path::PathBuf::from(std::ffi::OsStr::from_bytes(stripped));
        }
        exe
    }
    #[cfg(not(unix))]
    {
        if let Some(stripped) = exe.to_str().and_then(|s| s.strip_suffix(" (deleted)")) {
            return std::path::PathBuf::from(stripped);
        }
        exe
    }
}

fn spawn_restarted_daemon(paths: &AppPaths, headless: bool) -> std::io::Result<Child> {
    spawn_self_detached(&paths.config_file, &paths.socket_path, true, |cmd| {
        cmd.arg("--background").env_remove(HANDOFF_ENV_VAR);
        if headless {
            cmd.arg("--headless");
        }
    })
}

#[must_use]
pub(crate) fn format_human_status(snapshot: &sye_core::scheduler::StatusSnapshot) -> String {
    let state_label = match snapshot.state {
        SchedulerState::Start => "Start",
        SchedulerState::Waiting => "Waiting",
        SchedulerState::PreBreak => "PreBreak",
        SchedulerState::Break => "Break",
        SchedulerState::Stopped => "Stopped",
        SchedulerState::Quit => "Quit",
        SchedulerState::Resting => "Resting",
    };
    let enabled_label = if snapshot.enabled {
        "enabled"
    } else {
        "disabled"
    };
    let mut out = String::with_capacity(96);
    let _ = write!(
        out,
        "Safe Your Eyes: {state_label} ({enabled_label}) | Next break: "
    );
    match (&snapshot.next_break_type, &snapshot.next_break_name) {
        (Some(kind), Some(name)) => {
            let tier = match kind {
                BreakType::Short => "short",
                BreakType::Long => "long",
            };
            let _ = write!(
                out,
                "{tier} ({name}) in {}s",
                snapshot.seconds_until_next_break
            );
        }
        _ => out.push_str("none"),
    }
    let _ = write!(out, " | Consecutive skips: {}", snapshot.consecutive_skips);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn startup_settings_suppressed_for_headless_background_or_login_item() {
        assert!(should_open_settings_on_startup(false, false, false));
        assert!(!should_open_settings_on_startup(true, false, false));
        assert!(!should_open_settings_on_startup(false, true, false));
        assert!(!should_open_settings_on_startup(false, false, true));
    }

    #[test]
    fn settings_guards_block_pre_break_and_break() {
        assert!(can_open_settings(SchedulerState::Waiting));
        assert!(can_open_settings(SchedulerState::Stopped));
        assert!(can_open_settings(SchedulerState::Resting));
        assert!(!can_open_settings(SchedulerState::PreBreak));
        assert!(!can_open_settings(SchedulerState::Break));
    }

    #[test]
    fn startup_autostart_plan_registers_first_run_and_respects_os_thereafter() {
        assert_eq!(
            plan_startup_autostart(true, true, true, Some(false)),
            StartupAutostartPlan::RegisterAndPersist(true)
        );
        assert_eq!(
            plan_startup_autostart(true, true, true, None),
            StartupAutostartPlan::RegisterAndPersist(true)
        );
        assert_eq!(
            plan_startup_autostart(false, true, true, Some(false)),
            StartupAutostartPlan::AdoptSystem(false)
        );
        assert_eq!(
            plan_startup_autostart(false, true, false, Some(true)),
            StartupAutostartPlan::AdoptSystem(true)
        );
        assert_eq!(
            plan_startup_autostart(false, true, true, None),
            StartupAutostartPlan::SyncFromConfig(true)
        );
        assert_eq!(
            plan_startup_autostart(false, true, false, None),
            StartupAutostartPlan::SyncFromConfig(false)
        );
        assert_eq!(
            plan_startup_autostart(false, false, true, None),
            StartupAutostartPlan::KeepSystem
        );
    }

    #[cfg(not(feature = "msstore"))]
    #[test]
    fn poll_update_cache_change_detects_live_updates_and_skips_partial_writes()
    -> Result<(), Box<dyn std::error::Error>> {
        let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join(".tmp")
            .join(format!("sye-cli-update-poll-{}", std::process::id()));
        std::fs::create_dir_all(&dir)?;
        let cache_path = dir.join("update.toml");

        let mut fingerprint = file_fingerprint(&cache_path);
        let mut last_cache = UpdateCacheState::default();
        assert_eq!(
            poll_update_cache_change(&cache_path, true, &mut fingerprint, &mut last_cache),
            None
        );

        let initial = UpdateCacheState {
            checked_at_unix_secs: 1_700_000_000,
            latest_version: Some("0.2.0".to_owned()),
            ..UpdateCacheState::default()
        };
        initial.save_to_path(&cache_path)?;
        assert_eq!(
            poll_update_cache_change(&cache_path, false, &mut fingerprint, &mut last_cache),
            Some(initial.clone())
        );
        assert_eq!(
            poll_update_cache_change(&cache_path, true, &mut fingerprint, &mut last_cache),
            None
        );

        std::fs::write(&cache_path, "schema_version = [broken")?;
        assert_eq!(
            poll_update_cache_change(&cache_path, true, &mut fingerprint, &mut last_cache),
            None
        );
        assert_eq!(last_cache, initial);

        let newer = UpdateCacheState {
            checked_at_unix_secs: 1_700_000_001,
            latest_version: Some("0.2.1".to_owned()),
            ..UpdateCacheState::default()
        };
        newer.save_to_path(&cache_path)?;
        assert_eq!(
            poll_update_cache_change(&cache_path, true, &mut fingerprint, &mut last_cache),
            Some(newer)
        );

        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    #[test]
    fn strip_linux_deleted_exe_suffix_handles_replaced_and_normal_paths() {
        assert_eq!(
            strip_linux_deleted_exe_suffix(std::path::PathBuf::from(
                "/usr/local/bin/safe-your-eyes (deleted)"
            )),
            std::path::PathBuf::from("/usr/local/bin/safe-your-eyes")
        );
        assert_eq!(
            strip_linux_deleted_exe_suffix(std::path::PathBuf::from(
                "/usr/local/bin/safe-your-eyes"
            )),
            std::path::PathBuf::from("/usr/local/bin/safe-your-eyes")
        );
    }

    #[test]
    fn should_replace_running_daemon_checks_version_and_break_state() {
        let base = sye_core::scheduler::StatusSnapshot {
            state: SchedulerState::Waiting,
            enabled: true,
            next_break_type: Some(BreakType::Short),
            next_break_name: Some("Blink".to_owned()),
            seconds_until_next_break: 60,
            consecutive_skips: 0,
            daemon_version: None,
        };
        assert!(should_replace_running_daemon(&base, "0.5.0"));
        assert!(should_replace_running_daemon(
            &sye_core::scheduler::StatusSnapshot {
                daemon_version: Some("0.4.0".to_owned()),
                ..base.clone()
            },
            "0.5.0"
        ));
        assert!(!should_replace_running_daemon(
            &sye_core::scheduler::StatusSnapshot {
                daemon_version: Some("0.5.0".to_owned()),
                ..base.clone()
            },
            "0.5.0"
        ));
        assert!(!should_replace_running_daemon(
            &sye_core::scheduler::StatusSnapshot {
                state: SchedulerState::PreBreak,
                ..base.clone()
            },
            "0.5.0"
        ));
        assert!(!should_replace_running_daemon(
            &sye_core::scheduler::StatusSnapshot {
                state: SchedulerState::Break,
                ..base
            },
            "0.5.0"
        ));
    }

    #[test]
    fn poll_config_change_detects_same_length_language_toggle_and_skips_partial_writes()
    -> Result<(), Box<dyn std::error::Error>> {
        let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join(".tmp")
            .join(format!("sye-cli-cfg-poll-{}", std::process::id()));
        std::fs::create_dir_all(&dir)?;
        let cfg_path = dir.join("config.toml");

        let mut current = Config {
            language: sye_core::LanguageMode::En,
            ..Config::default()
        };
        current.save_to_path(&cfg_path)?;
        let mut fingerprint = file_fingerprint(&cfg_path);
        assert_eq!(
            poll_config_change(&cfg_path, false, &mut fingerprint, &current),
            None
        );

        std::fs::write(&cfg_path, "schema_version = [broken")?;
        assert_eq!(
            poll_config_change(&cfg_path, true, &mut fingerprint, &current),
            None
        );
        assert_eq!(fingerprint, file_fingerprint(&cfg_path));

        let next = Config {
            language: sye_core::LanguageMode::Vi,
            ..current.clone()
        };
        next.save_to_path(&cfg_path)?;
        let reloaded = poll_config_change(&cfg_path, true, &mut fingerprint, &current);
        assert_eq!(reloaded, Some(next.clone()));
        current = next;
        assert_eq!(
            poll_config_change(&cfg_path, true, &mut fingerprint, &current),
            None
        );

        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    #[test]
    fn bind_or_replace_daemon_hands_off_from_older_daemon_and_keeps_same_version()
    -> Result<(), Box<dyn std::error::Error>> {
        let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join(".tmp")
            .join(format!("sye-ho-{}", std::process::id()));
        std::fs::create_dir_all(&dir)?;

        let old_sock = dir.join("old.sock");
        let old_server = IpcServer::bind(&old_sock)?;
        let old_thread = std::thread::spawn(move || -> bool {
            let mut saw_quit = false;
            let deadline = Instant::now() + Duration::from_secs(3);
            while !saw_quit && Instant::now() < deadline {
                let _ = old_server.accept_once(Duration::from_millis(200), |cmd| match cmd {
                    IpcCommand::Status => {
                        IpcResponse::Status(sye_core::scheduler::StatusSnapshot {
                            state: SchedulerState::Waiting,
                            enabled: true,
                            next_break_type: Some(BreakType::Short),
                            next_break_name: Some("Blink".to_owned()),
                            seconds_until_next_break: 60,
                            consecutive_skips: 0,
                            daemon_version: None,
                        })
                    }
                    IpcCommand::Quit => {
                        saw_quit = true;
                        IpcResponse::Ok {
                            message: Some("Quitting".to_owned()),
                        }
                    }
                    _ => IpcResponse::Ok { message: None },
                });
            }
            drop(old_server);
            saw_quit
        });

        let replaced = bind_or_replace_daemon(&old_sock, false, "0.5.0")?;
        drop(replaced);
        let Ok(saw_quit) = old_thread.join() else {
            return Err("old daemon thread panicked".into());
        };
        assert!(saw_quit);

        let same_sock = dir.join("same.sock");
        let same_server = IpcServer::bind(&same_sock)?;
        let same_thread = std::thread::spawn(move || -> bool {
            let mut saw_status = false;
            let deadline = Instant::now() + Duration::from_secs(3);
            while !saw_status && Instant::now() < deadline {
                let _ = same_server.accept_once(Duration::from_millis(200), |cmd| match cmd {
                    IpcCommand::Status => {
                        saw_status = true;
                        IpcResponse::Status(sye_core::scheduler::StatusSnapshot {
                            state: SchedulerState::Waiting,
                            enabled: true,
                            next_break_type: Some(BreakType::Short),
                            next_break_name: Some("Blink".to_owned()),
                            seconds_until_next_break: 60,
                            consecutive_skips: 0,
                            daemon_version: Some("0.5.0".to_owned()),
                        })
                    }
                    _ => IpcResponse::Ok { message: None },
                });
            }
            saw_status
        });

        let err = bind_or_replace_daemon(&same_sock, false, "0.5.0").err();
        assert_eq!(err, Some(IpcError::AlreadyRunning));
        let Ok(saw_status) = same_thread.join() else {
            return Err("same daemon thread panicked".into());
        };
        assert!(saw_status);

        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }
}
