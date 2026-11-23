//! Background daemon loop wiring `sye-core`, `sye-platform`, and on-demand `sye-ui` subprocesses.

use std::path::Path;
use std::process::{Command, ExitCode};
use std::sync::mpsc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use sye_core::config::{BreakType, Config};
use sye_core::ipc::{IpcCommand, IpcResponse};
use sye_core::scheduler::{OverlayOutcome, Scheduler, SchedulerAction, SchedulerEvent};
use sye_core::session::SessionState;
use sye_core::traits::{IdleProvider, Notifier, OverlayLauncher};
use sye_platform::{
    AppPaths, IpcError, IpcServer, OsAudioPlayer, OsNotifier, SubprocessOverlayLauncher,
    SystemClock, SystemDndProvider, SystemIdleProvider, TrayEvent, TrayHandle,
};

const POLL_INTERVAL: Duration = Duration::from_millis(25);

struct DaemonHost<'a> {
    headless: bool,
    session_file: &'a Path,
    notifier: OsNotifier,
    audio: OsAudioPlayer,
    overlay_launcher: Option<SubprocessOverlayLauncher>,
    tray: Option<TrayHandle>,
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
                            Some(Instant::now() + Duration::from_secs(req.duration_secs));
                    } else if let Some(launcher) = self.overlay_launcher.as_mut() {
                        let _ = launcher.launch(&req);
                    }
                }
                SchedulerAction::CloseOverlay => {
                    self.headless_break_deadline = None;
                    if let Some(launcher) = self.overlay_launcher.as_mut() {
                        let _ = launcher.close();
                    }
                }
                SchedulerAction::UpdateTray(snapshot) => {
                    if let Some(handle) = self.tray.as_mut() {
                        let _ = handle.update(&snapshot);
                    }
                }
                SchedulerAction::PersistSession(session) => {
                    let _ = session.save_to_path(self.session_file);
                }
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
            self.overlay_launcher
                .as_mut()
                .and_then(|launcher| launcher.poll_outcome().ok().flatten())
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

/// Runs the single-instance background daemon until `Quit`.
pub(crate) fn run_daemon(paths: AppPaths, headless: bool) -> ExitCode {
    let server = match IpcServer::bind(&paths.socket_path) {
        Ok(srv) => srv,
        Err(IpcError::AlreadyRunning) => {
            println!("Safe Your Eyes is already running");
            return ExitCode::SUCCESS;
        }
        Err(err) => {
            eprintln!("Failed to bind IPC socket: {err}");
            return ExitCode::FAILURE;
        }
    };
    if let Err(err) = server.set_accept_nonblocking() {
        eprintln!("Failed to configure non-blocking IPC listener: {err}");
        return ExitCode::FAILURE;
    }

    let (config, load_err) = Config::load_or_default(&paths.config_file);
    if let Some(err) = load_err {
        eprintln!("Warning: falling back to default config ({err})");
    }
    if !headless {
        let _ = sye_platform::sync_system_autostart(config.autostart, Some(&paths.config_file));
    }

    let seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0x5EED_CAFE, |d| d.as_nanos() as u64);

    let persist_state = config.persist_state;
    let mut scheduler = Scheduler::new(config, SystemClock::new(), seed);
    if persist_state
        && let Ok(raw) = std::fs::read_to_string(&paths.session_file)
        && let Ok(session) = SessionState::from_toml_str(&raw)
    {
        scheduler.restore_session(&session);
    }

    let idle_provider = SystemIdleProvider::new();
    let dnd_provider = SystemDndProvider::new();
    let (tray_tx, tray_rx) = mpsc::channel::<TrayEvent>();
    let mut host = DaemonHost {
        headless,
        session_file: &paths.session_file,
        notifier: OsNotifier::new(),
        audio: OsAudioPlayer::new(),
        overlay_launcher: (!headless)
            .then(|| SubprocessOverlayLauncher::current_exe().ok())
            .flatten(),
        tray: (!headless)
            .then(|| TrayHandle::try_new(&scheduler.tray_snapshot(), tray_tx).ok())
            .flatten(),
        headless_break_deadline: None,
    };

    let mut last_policy_check = Instant::now();
    let mut running = true;

    host.execute(scheduler.handle_event(SchedulerEvent::Start));

    while running {
        loop {
            let mut should_quit = false;
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
                    deferred_actions
                        .extend(scheduler.handle_event(SchedulerEvent::TakeBreak { kind }));
                    IpcResponse::Ok {
                        message: Some("Break triggered".to_owned()),
                    }
                }
                IpcCommand::OpenSettings => {
                    open_settings = true;
                    IpcResponse::Ok {
                        message: Some("Opening settings".to_owned()),
                    }
                }
                IpcCommand::Quit => {
                    should_quit = true;
                    deferred_actions.extend(scheduler.handle_event(SchedulerEvent::Quit));
                    IpcResponse::Ok {
                        message: Some("Quitting".to_owned()),
                    }
                }
            });

            if !deferred_actions.is_empty() {
                host.execute(deferred_actions);
            }

            if open_settings && !headless {
                spawn_settings_subprocess(&paths.config_file);
            }

            if should_quit {
                running = false;
                break;
            }

            match accepted {
                Ok(true) => continue,
                Ok(false) | Err(_) => break,
            }
        }

        if !running {
            break;
        }

        while let Ok(event) = tray_rx.try_recv() {
            let actions = match event {
                TrayEvent::Enable => scheduler.handle_event(SchedulerEvent::Enable),
                TrayEvent::Disable { minutes } => scheduler.handle_event(SchedulerEvent::Disable {
                    duration_secs: disable_minutes_to_secs(minutes),
                }),
                TrayEvent::TakeBreak { kind } => {
                    scheduler.handle_event(SchedulerEvent::TakeBreak { kind })
                }
                TrayEvent::OpenSettings => {
                    if !headless {
                        spawn_settings_subprocess(&paths.config_file);
                    }
                    Vec::new()
                }
                TrayEvent::Quit => {
                    running = false;
                    scheduler.handle_event(SchedulerEvent::Quit)
                }
            };
            host.execute(actions);
        }

        if !running {
            break;
        }

        if let Some(outcome) = host.poll_overlay_outcome() {
            host.execute(scheduler.handle_event(SchedulerEvent::OverlayFinished(outcome)));
        }

        if !headless && last_policy_check.elapsed() >= Duration::from_secs(1) {
            last_policy_check = Instant::now();
            if let Ok(idle) = idle_provider.idle_duration() {
                host.execute(scheduler.handle_event(SchedulerEvent::IdleObserved(idle)));
            }
            let _ = &dnd_provider;
        }

        let tick_actions = scheduler.tick();
        if !tick_actions.is_empty() {
            host.execute(tick_actions);
        }

        std::thread::sleep(POLL_INTERVAL);
    }

    host.close_overlay();

    ExitCode::SUCCESS
}

fn spawn_settings_subprocess(config_file: &Path) {
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    let _ = Command::new(exe)
        .arg("--internal-settings")
        .arg("--config")
        .arg(config_file)
        .spawn();
}

/// Formats a human-readable `--status` line for CLI users.
pub(crate) fn format_human_status(snapshot: &sye_core::scheduler::StatusSnapshot) -> String {
    let enabled_label = if snapshot.enabled {
        "enabled"
    } else {
        "disabled"
    };
    let next_label = match (&snapshot.next_break_type, &snapshot.next_break_name) {
        (Some(kind), Some(name)) => {
            let tier = match kind {
                BreakType::Short => "short",
                BreakType::Long => "long",
            };
            format!("{tier} ({name}) in {}s", snapshot.seconds_until_next_break)
        }
        _ => "none".to_owned(),
    };
    format!(
        "Safe Your Eyes: {:?} ({enabled_label}) | Next break: {next_label} | Consecutive skips: {}",
        snapshot.state, snapshot.consecutive_skips
    )
}
