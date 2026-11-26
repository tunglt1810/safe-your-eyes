use std::fmt::Write as _;
use std::path::Path;
use std::process::{Child, Command, ExitCode, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use sye_core::config::{BreakType, Config};
use sye_core::ipc::{IpcCommand, IpcResponse};
use sye_core::scheduler::{
    OverlayOutcome, Scheduler, SchedulerAction, SchedulerEvent, SchedulerState,
};
use sye_core::session::SessionState;
use sye_core::traits::{DndProvider, IdleProvider, Notifier, OverlayLauncher};
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
const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LoopControl {
    Continue,
    Quit,
    Restart,
}

struct DaemonHost<'a> {
    headless: bool,
    config_file: &'a Path,
    session_file: &'a Path,
    update_cache_file: &'a Path,
    socket_path: &'a Path,
    event_loop: Option<&'a PlatformEventLoop>,
    notifier: OsNotifier,
    audio: OsAudioPlayer,
    overlay_launcher: Option<SubprocessOverlayLauncher>,
    settings_child: Option<Child>,
    settings_exited: bool,
    tray: Option<TrayHandle>,
    available_update: Option<UpdateInfo>,
    update_in_progress: bool,
    update_apply_rx: Option<mpsc::Receiver<Result<(), String>>>,
    headless_break_deadline: Option<Instant>,
}

impl DaemonHost<'_> {
    fn execute(&mut self, actions: Vec<SchedulerAction>) {
        for action in actions {
            match action {
                SchedulerAction::NotifyPreBreak {
                    break_spec,
                    warning_secs,
                } => {
                    if !self.headless {
                        let _ = self.notifier.show_pre_break(
                            break_spec.break_type,
                            &break_spec.name,
                            warning_secs,
                        );
                    }
                }
                SchedulerAction::DismissNotification => {
                    if !self.headless {
                        let _ = self.notifier.dismiss();
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
                        if let Err(err) = launcher.launch(&req) {
                            eprintln!("Warning: failed to launch break overlay: {err}");
                        }
                    }
                }
                SchedulerAction::CloseOverlay => {
                    self.headless_break_deadline = None;
                    if let Some(launcher) = self.overlay_launcher.as_mut()
                        && let Err(err) = launcher.close()
                    {
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

    fn reload_cached_update(&mut self) {
        let (cache, _) = UpdateCacheState::load_or_default(self.update_cache_file);
        self.available_update = cache.available_update(CURRENT_VERSION);
        self.sync_tray_update();
    }

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
                let _ = self.notifier.show_notice(
                    "Safe Your Eyes — Updating",
                    &format!("Downloading and verifying v{}...", info.latest_version),
                );
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
                let _ = self.notifier.show_notice(
                    &format!("Safe Your Eyes v{} available", info.latest_version),
                    &format!("Run: {HOMEBREW_UPGRADE_COMMAND}"),
                );
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
                let _ = self.notifier.show_notice(
                    "Safe Your Eyes — Update failed",
                    "Opening the download page instead.",
                );
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
                Ok(outcome) => outcome,
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

    fn close_settings(&mut self, saved_autostart: bool) {
        if let Some(mut child) = self.settings_child.take() {
            let _ = child.kill();
            let _ = child.wait();
            if !self.headless {
                let _ =
                    sye_platform::sync_system_autostart(saved_autostart, Some(self.config_file));
            }
        }
    }

    fn close_overlay(&mut self) {
        if let Some(launcher) = self.overlay_launcher.as_mut() {
            let _ = launcher.close();
        }
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
    let server = match IpcServer::bind(&paths.socket_path) {
        Ok(srv) => srv,
        Err(IpcError::AlreadyRunning) => {
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

    let (mut config, load_err) = Config::load_or_default(&paths.config_file);
    if let Some(err) = load_err {
        eprintln!("Warning: falling back to default config ({err})");
    }
    if !headless {
        if let Some(sys_autostart) = sye_platform::query_system_autostart(Some(&paths.config_file))
        {
            config.autostart = sys_autostart;
        } else if let Err(err) =
            sye_platform::sync_system_autostart(config.autostart, Some(&paths.config_file))
        {
            eprintln!("Warning: failed to synchronize autostart entry: {err}");
        }
    }

    let persist_state = config.persist_state;
    let mut scheduler = Scheduler::new(config, SystemClock::new(), rng_seed_from_clock());
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
        match TrayHandle::try_new(&scheduler.tray_snapshot(), tray_tx) {
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
        OsNotifier::new()
    };
    if !headless {
        sye_platform::cleanup_current_stale_old_executable();
    }

    let (cached_update, _) = if headless {
        (UpdateCacheState::default(), None)
    } else {
        UpdateCacheState::load_or_default(&paths.update_cache_file)
    };
    let initial_available_update = cached_update.available_update(CURRENT_VERSION);

    let mut host = DaemonHost {
        headless,
        config_file: &paths.config_file,
        session_file: &paths.session_file,
        update_cache_file: &paths.update_cache_file,
        socket_path: &paths.socket_path,
        event_loop: app_loop.as_ref(),
        notifier,
        audio: OsAudioPlayer::new(),
        overlay_launcher: (!headless)
            .then(|| SubprocessOverlayLauncher::current_exe().ok())
            .flatten(),
        settings_child: None,
        settings_exited: false,
        tray,
        available_update: initial_available_update,
        update_in_progress: false,
        update_apply_rx: None,
        headless_break_deadline: None,
    };
    host.sync_tray_update();

    let mut last_policy_check = Instant::now();
    let mut update_check_rx: Option<mpsc::Receiver<Option<UpdateInfo>>> = None;
    let mut next_update_check_at = compute_next_update_check(&cached_update);
    let mut restart_after_exit = false;

    host.execute(scheduler.handle_event(SchedulerEvent::Start));
    let launched_at_login = app_loop
        .as_ref()
        .is_some_and(PlatformEventLoop::was_launched_as_login_item);
    let mut restart_on_settings_close =
        should_open_settings_on_startup(headless, background, launched_at_login);
    if restart_on_settings_close {
        host.open_settings();
        restart_on_settings_close = host.settings_child.is_some();
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

        if restart_on_settings_close
            && !matches!(
                scheduler.state(),
                SchedulerState::Waiting | SchedulerState::Resting
            )
        {
            restart_on_settings_close = false;
        }
        let settings_exited = host.take_settings_exited();
        if settings_exited {
            host.reload_cached_update();
        }
        if settings_exited
            && let Ok(updated_cfg) = Config::load_from_path(&paths.config_file)
            && updated_cfg != *scheduler.config()
        {
            if !headless
                && updated_cfg.autostart != scheduler.config().autostart
                && let Err(err) = sye_platform::sync_system_autostart(
                    updated_cfg.autostart,
                    Some(&paths.config_file),
                )
            {
                eprintln!("Warning: failed to synchronize autostart entry: {err}");
            }
            host.execute(scheduler.handle_event(SchedulerEvent::ReloadConfig(updated_cfg)));
        }
        if settings_exited && std::mem::take(&mut restart_on_settings_close) {
            host.execute(scheduler.handle_event(SchedulerEvent::RestartCountdown));
        }

        if !headless {
            if let Some(rx) = update_check_rx.as_ref() {
                match rx.try_recv() {
                    Ok(found) => {
                        update_check_rx = None;
                        host.available_update = found;
                        host.sync_tray_update();
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

            if host.poll_update_apply() {
                restart_after_exit = true;
                break;
            }
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
            let dnd_ctx = if scheduler.config().do_not_disturb.enabled {
                dnd_provider.query_dnd_context().ok()
            } else {
                None
            };
            scheduler.tick_with_policies(current_idle, dnd_ctx.as_ref())
        } else {
            scheduler.tick()
        };
        if !tick_actions.is_empty() {
            host.execute(tick_actions);
        }
        host.refresh_tray(&scheduler.tray_snapshot());

        if let Some(ref event_loop) = app_loop {
            event_loop.pump_events(POLL_INTERVAL);
        } else {
            std::thread::sleep(POLL_INTERVAL);
        }
    }

    host.close_overlay();
    host.close_settings(scheduler.config().autostart);
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
            IpcCommand::Status => IpcResponse::Status(scheduler.status_snapshot()),
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
                if can_open_settings(scheduler.state(), scheduler.tray_snapshot().menu_locked) {
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
                if can_open_settings(scheduler.state(), scheduler.tray_snapshot().menu_locked) {
                    host.open_settings();
                }
                Vec::new()
            }
            TrayEvent::UpdateApp => {
                if can_open_settings(scheduler.state(), scheduler.tray_snapshot().menu_locked) {
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

const fn can_open_settings(state: SchedulerState, menu_locked: bool) -> bool {
    !menu_locked && !matches!(state, SchedulerState::PreBreak | SchedulerState::Break)
}

fn spawn_settings_subprocess(config_file: &Path, socket_path: &Path) -> std::io::Result<Child> {
    let exe = std::env::current_exe()?;
    let resolved = exe.canonicalize().unwrap_or(exe);
    let mut cmd = Command::new(resolved);
    cmd.arg("--internal-settings")
        .arg("--config")
        .arg(config_file)
        .arg("--socket")
        .arg(socket_path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    sye_platform::hide_child_console(&mut cmd).spawn()
}

fn spawn_restarted_daemon(paths: &AppPaths, headless: bool) -> std::io::Result<Child> {
    let exe = std::env::current_exe()?;
    let resolved = exe.canonicalize().unwrap_or(exe);
    let mut cmd = Command::new(resolved);
    cmd.arg("--background")
        .arg("--config")
        .arg(&paths.config_file)
        .arg("--socket")
        .arg(&paths.socket_path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if headless {
        cmd.arg("--headless");
    }
    sye_platform::hide_child_console(&mut cmd).spawn()
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
    fn settings_guards_block_pre_break_break_and_locked_menu() {
        assert!(can_open_settings(SchedulerState::Waiting, false));
        assert!(can_open_settings(SchedulerState::Stopped, false));
        assert!(can_open_settings(SchedulerState::Resting, false));
        assert!(!can_open_settings(SchedulerState::PreBreak, false));
        assert!(!can_open_settings(SchedulerState::Break, false));
        assert!(!can_open_settings(SchedulerState::Waiting, true));
    }
}
