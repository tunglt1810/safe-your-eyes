# Safe Your Eyes Specification

## 1. Purpose and Scope

Safe Your Eyes is a desktop application that protects your eyes during computer use.
The application runs on macOS, Windows, and Linux.
The codebase uses Rust under the GPL-2.0-only license.
The break behavior follows SafeEyes.

## 2. Architecture

The Cargo workspace contains four crates and compiles into one binary named `safe-your-eyes`.

- `sye-core`: Pure Rust library with `BreakQueue`, `Scheduler`, automatic policies, `Config`, `SessionState`, a PCM WAV chime synthesizer, and the local IPC protocol.
- `sye-platform`: Operating system adapters for the system tray, idle time, `DoNotDisturb`, desktop notifications, audio playback, local IPC, and autostart.
- `sye-ui`: On-demand native graphical interface built with Native Shell (`longbridge/nsh` v0.6.1) for the break overlay and the Settings window.
- `sye-cli`: Single binary entry point that runs the background daemon, CLI commands, and internal UI subprocesses.

### 2.1 Process and Memory Design

1. The daemon runs on `std::thread` and `std::sync::mpsc` without `tokio`.
2. In the `Waiting` state, the daemon creates no graphics context and uses 0% GPU.
3. On macOS and Windows, `PlatformEventLoop` pumps main-thread operating system events without a window or GPU context.
4. On macOS, `PlatformEventLoop` forwards application reopen requests as `TrayEvent::Reopen`.
5. On macOS, `PlatformEventLoop` detects Login Item launches (`keyAELaunchedAsLogInItem`).
6. On macOS, `PlatformEventLoop` yields foreground activation rights to the break overlay and Settings child processes.
7. The daemon updates `TrayHandle` on each loop iteration when the tray snapshot changes.
8. On macOS, `TrayHandle` runs a timer in `NSEventTrackingRunLoopMode` to update the countdown while the menu stays open.
9. When the daemon needs to show a break overlay or the Settings window, the daemon resolves the executable symlink and starts a child process of `safe-your-eyes` with `--internal-overlay` or `--internal-settings`.
10. The daemon ignores `OpenSettings` and `TrayEvent::Reopen` during `PreBreak`, `Break`, or when the tray menu is locked.
11. When the child process exits, the operating system releases all graphics memory.

### 2.2 Safety and Build Rules

- The workspace forbids `unsafe` code by default (`unsafe_code = "forbid"`).
- Platform FFI code isolates required `unsafe` blocks inside `sye-platform` on Windows and macOS.
- Clippy denies `unwrap_used`, `expect_used`, `panic`, `todo`, and `unimplemented`.
- The release profile sets `opt-level = "z"`, `lto = "fat"`, `codegen-units = 1`, `panic = "abort"`, and `strip = "symbols"`.
- Linux builds require `libgtk-4-dev` and `pkg-config` for `nsh-platform-linux`.
- Linux tray and notification adapters use pure Rust `ksni` and `zbus`.

## 3. Terminology

Each concept uses one name across this specification and the source code.

| Concept | Rust Identifier | Definition |
|---|---|---|
| Break tier | `BreakType` | `Short` break or `Long` break. |
| Exercise entry | `BreakSpec` | Configuration entry with a name and optional interval or duration override. |
| Resolved break | `ResolvedBreak` | Break with final positive interval and duration values. |
| Staged break | `QueuedBreak` | Next break in the queue together with its wait time `wait_secs`. |
| Break queue | `BreakQueue` | Dual-tier queue with a 4:1 short-to-long ratio and random or sequential order. |
| Scheduler | `Scheduler` | Pure state machine that controls break cycles and policies. |
| Scheduler state | `SchedulerState` | One of seven states: `Start`, `Waiting`, `PreBreak`, `Break`, `Stopped`, `Quit`, `Resting`. |
| Scheduler event | `SchedulerEvent` | Discrete input passed to `Scheduler::handle_event`. |
| Scheduler action | `SchedulerAction` | Pure side-effect command returned by `Scheduler`. |
| Overlay request | `OverlayRequest` | Data sent to `sye-ui` to show a break overlay. |
| Overlay outcome | `OverlayOutcome` | Terminal result from the break overlay: `Completed`, `Skipped`, `Postponed`, `Closed`. |
| Strict break | `strict_break` | Option that hides the Skip control and locks tray controls during breaks. |
| Postpone | `Postpone` | Action that delays the current break by `postpone_duration_secs` without advancing `BreakQueue`. |
| Skip | `Skip` | Action that ends the current break early, advances `BreakQueue`, and omits the post-break chime. |
| Shortcut lock | `shortcut_disable_secs` | Initial seconds of a break that block `Skip` and `Postpone`. |
| Smart Pause | `SmartPausePolicy` | Policy that enters `Resting` on idle, credits short idle time back, or resets the cycle. |
| Do Not Disturb | `DoNotDisturbPolicy` | Policy that blocks breaks on battery, fullscreen windows, or excluded window titles. |
| Skip limit | `SkipLimitPolicy` | Policy that counts consecutive skips or postpones and locks both actions at `max_skips`. |
| Theme mode | `ThemeMode` | Persisted Settings window theme: `System`, `Light`, or `Dark`. |
| Configuration | `Config` | TOML configuration structure with `schema_version` and validation. |
| Session state | `SessionState` | Saved queue progress stored in `session.toml`. |
| Chime kind | `ChimeKind` | Synthesized WAV chime type: `PreBreak` or `PostBreak`. |
| IPC message | `IpcCommand`, `IpcResponse` | Single-line JSON messages exchanged between the CLI and the daemon. |
| Status snapshot | `StatusSnapshot` | Status payload for `--status --json` and the system tray. |
| Clock | `Clock` | Trait that supplies monotonic and Unix wall time to `Scheduler`. |
| Platform event loop | `PlatformEventLoop` | Main-thread operating system event loop adapter for the system tray and reopen events. |

## 4. Break Queue (`BreakQueue`)

`BreakQueue` manages two break tiers: `Short` and `Long`.

- `Short` breaks default to a 900-second (15-minute) interval, a 15-second duration, and 13 exercises.
- `Long` breaks default to a 4500-second (75-minute) interval, a 60-second duration, and 6 exercises.
- The default cadence runs four `Short` breaks before one `Long` break.

`BreakQueue` supports two ordering modes:

1. When `random_order` is `true`, `BreakQueue` shuffles each cycle with Fisher-Yates using an internal `SplitMix64` generator.
2. When `random_order` is `false`, `BreakQueue` steps through each list in configuration order and wraps at the end.

Each `BreakSpec` can override `interval_secs` or `duration_secs` for that exercise.
`BreakQueue` ignores any `BreakSpec` with an empty name or a zero resolved value.
When both `short_breaks` and `long_breaks` are empty, `BreakQueue::from_config` returns `None` and `Scheduler` stays in `Stopped`.

## 5. Scheduler State Machine (`Scheduler`)

`Scheduler` operates across seven states:

- `Start`: Initial state before the first timer starts.
- `Waiting`: Daemon counts down to the pre-break warning time.
- `PreBreak`: Daemon shows a pre-break notification for `pre_break_warning_secs` (default 10 seconds).
- `Break`: Daemon shows the break overlay for the exercise duration.
- `Stopped`: Daemon pauses by user request or because no valid exercises exist.
- `Resting`: Daemon pauses because the user is idle or the computer suspended.
- `Quit`: Terminal state that ignores all subsequent events.

`Scheduler` reads time through the `Clock` trait (`now_monotonic` and `now_wall_unix`).
Every event handler runs synchronously and returns a list of `SchedulerAction` values for the host loop.

### 5.1 Manual Breaks (`TakeBreak`)

1. `TakeBreak` runs only when `Scheduler` is in `Waiting`.
2. `TakeBreak` skips `PreBreak` and enters `Break` directly.
3. The user can request the next scheduled break (`None`), a `Short` break, or a `Long` break.

### 5.2 Timed and Indefinite Pause (`Disable` and `Enable`)

1. `Disable` with a positive duration moves `Scheduler` to `Stopped` and resumes `Waiting` when the timer expires.
2. `Disable` with no duration or `0` keeps `Scheduler` in `Stopped` until `Enable` or a daemon restart.
3. When `Scheduler` is in `PreBreak` or `Break`, `Disable` emits `DismissNotification` or `CloseOverlay` before entering `Stopped`.

### 5.3 Natural Rest and Suspend Reset (`Resting`)

1. On entry to `Resting`, `Scheduler` records monotonic time, wall time, and remaining countdown seconds.
2. On exit from `Resting`, `Scheduler` computes elapsed rest as the larger of wall delta and monotonic delta.
3. When elapsed rest exceeds the next `Long` break duration, `Scheduler` calls `BreakQueue::skip_long_break()` and starts a new wait cycle.
4. Otherwise, `Scheduler` resumes the saved countdown.

## 6. Break Protections

- `strict_break`: When `true`, `Scheduler` rejects `Skipped`, the overlay hides the Skip button, and the tray locks controls during `PreBreak` and `Break`.
- `allow_postpone`: When `true`, the user can delay the break by `postpone_duration_secs` (default 300 seconds) without advancing `BreakQueue`.
- `Skip`: When permitted, `Scheduler` advances `BreakQueue`, closes the overlay, and skips the post-break chime.
- `shortcut_disable_secs`: During the first seconds of `Break` (default 2 seconds), `Scheduler` and the overlay reject `Skipped` and `Postponed` to prevent accidental key presses.

## 7. Automatic Policies

### 7.1 `SmartPause`

1. In `Waiting`, when OS idle time reaches `idle_threshold_secs` (default 300 seconds), `SmartPausePolicy` returns `PauseResting`.
2. When user activity resumes during `Resting`, the policy calculates total idle duration.
3. When idle duration is less than `short_break_interval_secs`, the policy returns `ResumeCreditBack` to shift the next break forward.
4. When idle duration is greater than or equal to `short_break_interval_secs`, the policy returns `ResumeResetCycle`.
5. When `postpone_if_active` is `true` and the user acts during the final 2 seconds before `Break`, `Scheduler` stays in `PreBreak`, dismisses any active pre-break banner once, and defers `Break` by 2 seconds without repeating the pre-break notification or chime.

### 7.2 `DoNotDisturb`

`Scheduler` evaluates `DoNotDisturbPolicy` before entering `PreBreak` and again before entering `Break`.
When `DoNotDisturbPolicy` blocks a break, `Scheduler` skips the current break, dismisses any active pre-break notification, advances `BreakQueue`, and starts the next wait cycle.

`DoNotDisturbPolicy` evaluates rules in fixed priority order:

1. If `enabled` is `false`, return `Allow`.
2. If `while_on_battery` is `true` and the battery is discharging, block with `OnBattery`.
3. If the active window matches `skip_break_windows` (case-insensitive), block with `SkipWindow`.
4. If the active window matches `take_break_windows`, return `Allow` even in fullscreen mode.
5. If the active window is fullscreen, block with `Fullscreen`.
6. If the operating system reports Do Not Disturb (`system_inhibited`), block with `SystemInhibited`.
7. Otherwise, return `Allow`.

### 7.3 `LimitConsecutiveSkipping`

1. `limit_consecutive_skips.enabled` defaults to `false` and `max_skips` defaults to `2`.
2. Each accepted `Skip` or `Postpone` increments `consecutive_skips` by 1.
3. When `enabled` is `true` and `consecutive_skips >= max_skips`, `Scheduler` disables both `Skip` and `Postpone` on the next break.
4. Completing a break resets `consecutive_skips` to 0.
5. When `enabled` is `true` and `consecutive_skips > 0`, the overlay shows a status note with the skip count.

## 8. Configuration and Session Persistence

The application stores configuration in `config.toml` with `schema_version = 1`.
`Config::validate` validates:

- `schema_version` equals 1.
- Break intervals, break durations, `postpone_duration_secs`, and `smart_pause.idle_threshold_secs` are at least 1.
- `pre_break_warning_secs` is less than `short_break_interval_secs`.
- `fade_in_ms` allows `0` to disable the overlay fade-in animation.
- `audible_alert.volume` is between 0 and 100 (`audible_alert.pre_break` defaults to `false` and `audible_alert.post_break` defaults to `true`).
- `limit_consecutive_skips.max_skips` is at least 1 when enabled.
- Every `BreakSpec` has a non-empty name and positive overrides.

When `config.toml` is missing or invalid, `Config::load_or_default` returns `Config::default()` so the daemon still starts.
`Config` stores `theme` (`system`, `light`, or `dark`) with default `system`.
When `config.toml` omits `theme`, `Config` defaults `theme` to `system` without increasing `schema_version`.
When `persist_state` is `true`, the daemon writes `SessionState` to `session.toml` using `BreakQueue::persisted_short_index()` and `BreakQueue::persisted_long_index()` so restarts resume at the staged break.
When `autostart` is `true`, `sye-platform` registers a per-user operating system login item:

- On macOS `.app` bundles with the default configuration path, `sye-platform` registers `SMAppService.mainApp` (`ServiceManagement.framework`). In this mode, `sye-platform` also deletes any legacy `~/Library/LaunchAgents/io.github.safeyoureyes.SafeYourEyes.plist` file.
- On macOS with a custom `--config` path, `sye-platform` writes `~/Library/LaunchAgents/io.github.safeyoureyes.SafeYourEyes.plist` with `--background` and `AssociatedBundleIdentifiers`.
- On Linux, `sye-platform` writes `~/.config/autostart/safe-your-eyes.desktop` with `--background`.
- On Windows, `sye-platform` writes `%APPDATA%\Microsoft\Windows\Start Menu\Programs\Startup\safe-your-eyes.cmd` with `--background`.

When `autostart` is `false`, `sye-platform` unregisters or deletes that login item.
On macOS `.app` bundles with the default configuration path, `query_system_autostart` reads the `System Settings` login item state.
When the user changes `Launch at login (startup)`, the Settings window updates the login item immediately.
When the user cancels or closes the Settings window without `Save`, or when the daemon stops while the Settings window is open, the application restores the saved login item state.

## 9. Audio Chime, Notifications, and Local IPC

### 9.1 WAV Chime Synthesizer and Pre-Break Notifications

`sye-core` synthesizes 16-bit mono PCM RIFF/WAVE bytes at 22050 Hz for `PreBreak` and `PostBreak`.
Each chime uses a smooth attack and exponential decay envelope to prevent clicks.
Volume clamps to 0 through 100, and volume 0 produces a valid silent WAV buffer.
`sye-platform` plays the buffer without blocking via `pw-play`, `paplay`, or `aplay` on Linux, `afplay` on macOS, and `PlaySoundW` on Windows.
On macOS `.app` bundles, `OsNotifier` requests alert permission once on daemon startup and sends pre-break banners through `UserNotifications.framework` (`UNUserNotificationCenter`).
On Linux and Windows, `OsNotifier` sends pre-break banners through `notify-rust`.

### 9.2 Local IPC Protocol

The CLI and the daemon communicate over a local socket (Unix domain socket on Linux and macOS, named pipe on Windows).
Each message is one JSON line ended by `\n` (NDJSON) with a 16384-byte maximum length.
Supported commands are `Status`, `Enable`, `Disable`, `TakeBreak`, `OpenSettings`, and `Quit`.
The daemon accepts connections in non-blocking mode so `Scheduler::tick()` never stalls behind IPC polling.

## 10. Cozy Design System and Native UI (`sye-ui`)

`sye-ui` renders native desktop controls through `longbridge/nsh` (AppKit on macOS, Win32 and Direct2D on Windows, GTK 4 on Linux).
The visual tokens, contrast pairs, component patterns, and motion presets follow the [Cozy Design System](https://github.com/tunglt1810/cozy) (`v0.2.0`: `tokens/cozy.tokens.json`, `css/effects.css`, `docs/motion.md`, `docs/patterns.md`) together with Astryx layout rules (`meta/astryx`).
All visual tokens and motion presets live in `crates/sye-ui/src/theme.rs` using `AdaptiveColor` for Light, Dark, and System modes.
All reusable controls live in `crates/sye-ui/src/ui.rs`.

### 10.1 Cozy Color Tokens

The palette avoids pure `#FFFFFF` and `#000000` to reduce eye strain.
Primary coral buttons pair with dark espresso text to keep WCAG AAA contrast (7.55:1 to 7.64:1).
All four status colors (including v0.2.0 light `warning` `#B87A12`) reach at least 3:1 on `bg-body` and `bg-surface`.
Status banners and destructive buttons pair `warning-muted` / `error-muted` fills with `on-warning-muted` / `on-error-muted` text from `tunglt1810/cozy`.

| Token | Light Mode | Dark Mode | Role |
|---|---|---|---|
| `BACKGROUND_BODY` | `#FAEBDA` | `#18140E` | Main canvas (`bg-body`) |
| `BACKGROUND_SURFACE` | `#FBF1E1` | `#231D16` | Unified Form Card, inputs, secondary buttons, and sticky toolbar (`bg-surface`) |
| `BACKGROUND_MUTED` | `#F3DFC8` | `#30261D` | Quiet emphasis surface (`bg-muted`) |
| `TRACK` | `#E6D2BC` | `#30271E` | Switch and progress tracks (`track`) |
| `SIDEBAR_BG` / `SIDEBAR_ACTIVE` | `#C3CBB9` / `#F8E8D3` | `#29332A` / `#404A41` | Floating Sage Sidebar and active indicator pill (`nav-bg`, `nav-active`) |
| `SIDEBAR_TEXT` / `SIDEBAR_TEXT_MUTED` | `#403B26` / `#55503A` | `#FFFEF7` / `#B8C4B6` | Sidebar labels and version caption (`nav-text`, `nav-text-muted`) |
| `ACCENT` / `ON_ACCENT` | `#EF8F63` / `#261101` | `#EE8F64` / `#18140E` | Primary CTA fill and WCAG AAA text (`accent`, `on-accent`) |
| `FOCUS_RING` | `#A84A24` | `#F2A57E` | Input focus outline (`focus-ring`) |
| `TEXT_PRIMARY` / `TEXT_SECONDARY` | `#261101` / `#5B4C3B` | `#FFFEF7` / `#C2B7B1` | Primary headings, labels, and supporting text |
| `TEXT_DISABLED` | `#968574` | `#70655C` | Placeholders, input hover border, and disabled controls |
| `WARNING` / `ERROR` | `#B87A12` / `#C8433B` | `#F5C156` / `#F27B70` | Status accents and banner outlines (v0.2.0, >= 3:1 contrast) |
| `WARNING_SOFT` / `ON_WARNING_MUTED` | `#FAE1B5` / `#6B4408` | `#443A26` / `#FCE09B` | Warning status banners |
| `ERROR_SOFT` / `ON_ERROR_MUTED` | `#F9CFC8` / `#7E211C` | `#472826` / `#F9BEB8` | Error banners and destructive actions |
| `HEADING_HIGHLIGHT` | `#FAD3B9` | `#7D4737` | Heading highlighter stroke (`mark-peach`) |
| `KPI_SAGE` | `#E2DEC4` / `#7EAB9A` | `#31392C` / `#5E8C7A` | Short Break summary card |
| `KPI_PEACH` | `#FBCBB4` / `#F48A65` | `#432F28` / `#E5866B` | Long Break summary card |
| `KPI_BUTTER` | `#FAE1B5` / `#FAC76A` | `#443A26` / `#D9A74E` | Exercises summary card |
| `KPI_MIST` | `#D2D7DA` / `#A2B3C1` | `#2B333D` / `#6D8296` | Skip Limit summary card |

### 10.2 Shape, Motion, and Screens

- Concentric desktop border radii keep inner corners parallel to native window corners: `8px` (elements and sidebar navigation items), `12px` (Floating Sage Sidebar, Form Card, and Pastel KPI cards), `28px` (overlay card), and `9999px` (pill controls).
- Motion follows `tunglt1810/cozy` v0.2.0.
- `COZY_SPRING` (`stiffness: 420`, `damping: 32`, `mass: 0.9`) moves the sliding sidebar indicator and the centered switch knob (`3pt` symmetrical inset, `18pt` horizontal travel).
- `COZY_EASE` (`cubic-bezier(0.22, 1, 0.36, 1)`) drives tab panel entrances (`160ms`), overlay dialog scale-in (`0.98 -> 1.0`, `200ms`), and highlighter sweeps (`380ms`).
- Cozy Circular Theme Reveal uses an `nsh` `screen_transition()` WGSL shader (`480ms`, `cubic-bezier(0.4, 0, 0.2, 1)`) that expands a circle from the theme button center to the farthest window corner.
- The reveal runs only when the resolved Light or Dark appearance changes and `reduce_motion()` is `false`.
- Pressable pills scale to `0.97` on press.
- All animations stop when the operating system enables Reduce Motion (`reduce_motion()`).
- The break overlay opens a borderless `WindowFrameStyle::None` window at `WindowLevel::AboveDock` across `Workspaces::All`.
- On macOS, the break overlay uses `ActivationPolicy::Accessory`.
- On macOS, the break overlay covers the full bounds of the primary display.
- On Linux and Windows, the break overlay uses `WindowMode::FullScreen` on the primary display.
- On additional displays, the break overlay opens secondary `WindowKind::Popup` windows that display the same Cozy break card and `Postpone (Space)` / `Skip (Esc)` controls.
- The break overlay focuses the primary window on launch.
- The break overlay uses dark-roast colors (`#18140E` canvas and `#231D16` card).
- The break overlay fades in over `fade_in_ms` with a `0.98 -> 1.0` scale-in.
- The break overlay shows the embedded app icon, exercise title, `MM:SS` countdown, animated progress bar, shortcut lock notice, `Postpone (Space)`, and `Skip (Esc)`.
- The Settings window uses `TitleBarStyle::Hidden` (`36pt` height) so the titlebar matches the active Cozy theme.
- On macOS, the Floating Sage Sidebar places the window traffic lights inside its top-left area.
- On Windows and Linux, the Settings window reserves reactive top-right clearance for native window controls.
- The Settings window focuses itself on launch to appear in front of the active application.
- The Settings window opens directly in the saved `Config.theme` mode with zero flash.
- The Floating Sage Sidebar has a matching `1px` border and switches between `Timing`, `Policies`, `Exercises`, and `About & Support`.
- The fixed page header shows the active screen title (`Timing`, `Policies`, `Exercises`, or `About & Support`) with the animated highlighter bar on the left, the top-right Cozy theme toggle pill (`Theme: System`, `Theme: Light`, `Theme: Dark` with vector SVG icon) on the right, and four Pastel KPI summary cards below.
- The unified rounded Form Card contains the scrollable form and the clipped sticky toolbar (`Save`, `Reset`, `Cancel`).
- The About & Support tab shows the **Buy Me a Coffee** link and QR code for `https://buymeacoffee.com/bbeeezzzzz`.
- Headless UI tests validate both screens in CI using `nsh_testing::Headless`.

## 11. CLI Contract (`safe-your-eyes`)

Global options:

- `--config <PATH>`: Override the TOML configuration file path.
- `--socket <PATH>`: Override the local IPC socket or pipe name.
- `--headless`: Run the daemon without the system tray or GUI subprocesses.
- `--background`: Start the daemon silently in the system tray without opening the Settings window.
- The argument parser ignores macOS LaunchServices `-psn_*` arguments with a non-empty suffix.

Modes and control flags:

- No control flag: Start the single-instance daemon. When the user omits `--headless` and `--background` and macOS did not launch the process as a Login Item (`keyAELaunchedAsLogInItem`), the daemon opens the Settings window after startup and keeps the system tray active. When the Settings window closes after saving changes, the daemon reloads `config.toml` immediately. If a daemon already listens on `--socket`, the CLI sends `IpcCommand::OpenSettings` (unless `--headless` or `--background` is present), prints `Safe Your Eyes is already running` to stdout, and exits with code 0.
- `--status`: Print human-readable daemon status to stdout and exit with code 0. If no daemon is running, print `Safe Your Eyes is not running` to stderr and exit with code 1.
- `--status --json`: Print JSON with `state`, `enabled`, `next_break_type`, `next_break_name`, `seconds_until_next_break`, and `consecutive_skips`.
- `-e`, `--enable`: Resume the running daemon.
- `-d`, `--disable [MINUTES]`: Pause the running daemon for `MINUTES`, or until restart if omitted or `0`.
- `-t`, `--take-break`: Start the next scheduled break immediately.
- `-b`, `--short-break`: Start an immediate `Short` break. Combining `-b` with `-l` prints `Cannot combine -b/--short-break with -l/--long-break` to stderr and exits with code 1.
- `-l`, `--long-break`: Start an immediate `Long` break.
- `-s`, `--settings`: Open the Settings window.
- `-q`, `--quit`: Stop the running daemon cleanly.
- `-V`, `--version`: Print `safe-your-eyes <version>` and exit with code 0.
- `-h`, `--help`: Print usage information and exit with code 0.
- `--print-default-config`: Print the default TOML configuration to stdout and exit with code 0.
- `--validate-config`: Validate `--config <PATH>` and exit with code 0 when valid or code 1 on error.
- `--internal-overlay <JSON_PAYLOAD>` and `--internal-settings --config <PATH>`: Internal subprocess entry points started by the daemon.
