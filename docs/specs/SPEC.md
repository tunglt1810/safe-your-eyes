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
3. When the daemon needs to show a break overlay or the Settings window, the daemon starts a child process of `safe-your-eyes` with `--internal-overlay` or `--internal-settings`.
4. When the child process exits, the operating system releases all graphics memory.

### 2.2 Safety and Build Rules

- The workspace forbids `unsafe` code by default (`unsafe_code = "forbid"`).
- Platform FFI code isolates any required `unsafe` blocks inside `sye-platform` on Windows.
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
| Configuration | `Config` | TOML configuration structure with `schema_version` and validation. |
| Session state | `SessionState` | Saved queue progress stored in `session.toml`. |
| Chime kind | `ChimeKind` | Synthesized WAV chime type: `PreBreak` or `PostBreak`. |
| IPC message | `IpcCommand`, `IpcResponse` | Single-line JSON messages exchanged between the CLI and the daemon. |
| Status snapshot | `StatusSnapshot` | Status payload for `--status --json` and the system tray. |
| Clock | `Clock` | Trait that supplies monotonic and Unix wall time to `Scheduler`. |

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

1. In `Waiting`, when OS idle time reaches `idle_threshold_secs` (default 5 seconds), `SmartPausePolicy` returns `PauseResting`.
2. When user activity resumes during `Resting`, the policy calculates total idle duration.
3. When idle duration is less than `short_break_interval_secs`, the policy returns `ResumeCreditBack` to shift the next break forward.
4. When idle duration is greater than or equal to `short_break_interval_secs`, the policy returns `ResumeResetCycle`.
5. When `postpone_if_active` is `true` and the user acts during the final 2 seconds before `Break`, the policy defers the break by 2 seconds.

### 7.2 `DoNotDisturb`

`DoNotDisturbPolicy` evaluates rules in fixed priority order:

1. If `enabled` is `false`, return `Allow`.
2. If `while_on_battery` is `true` and the battery is discharging, block with `OnBattery`.
3. If the active window matches `skip_break_windows` (case-insensitive), block with `SkipWindow`.
4. If the active window matches `take_break_windows`, return `Allow` even in fullscreen mode.
5. If the active window is fullscreen, block with `Fullscreen`.
6. If the operating system reports Do Not Disturb (`system_inhibited`), block with `SystemInhibited`.
7. Otherwise, return `Allow`.

### 7.3 `LimitConsecutiveSkipping`

1. Each accepted `Skip` or `Postpone` increments `consecutive_skips` by 1.
2. When `enabled` is `true` and `consecutive_skips >= max_skips` (default 2), `Scheduler` disables both `Skip` and `Postpone` on the next break.
3. Completing a break resets `consecutive_skips` to 0.
4. When `enabled` is `true` and `consecutive_skips > 0`, the overlay shows a status note with the skip count.

## 8. Configuration and Session Persistence

The application stores configuration in `config.toml` with `schema_version = 1`.
`Config::validate` validates:

- `schema_version` equals 1.
- Break intervals, break durations, `postpone_duration_secs`, and `smart_pause.idle_threshold_secs` are at least 1.
- `pre_break_warning_secs` is less than `short_break_interval_secs`.
- `audible_alert.volume` is between 0 and 100.
- `limit_consecutive_skips.max_skips` is at least 1 when enabled.
- Every `BreakSpec` has a non-empty name and positive overrides.

When `config.toml` is missing or invalid, `Config::load_or_default` returns `Config::default()` so the daemon still starts.
When `persist_state` is `true`, the daemon writes `SessionState` to `session.toml` using `BreakQueue::persisted_short_index()` and `BreakQueue::persisted_long_index()` so restarts resume at the staged break.

## 9. Audio Chime and Local IPC

### 9.1 WAV Chime Synthesizer

`sye-core` synthesizes 16-bit mono PCM RIFF/WAVE bytes at 22050 Hz for `PreBreak` and `PostBreak`.
Each chime uses a smooth attack and exponential decay envelope to prevent clicks.
Volume clamps to 0 through 100, and volume 0 produces a valid silent WAV buffer.
`sye-platform` plays the buffer without blocking via `pw-play`, `paplay`, or `aplay` on Linux, `afplay` on macOS, and `PlaySoundW` on Windows.

### 9.2 Local IPC Protocol

The CLI and the daemon communicate over a local socket (Unix domain socket on Linux and macOS, named pipe on Windows).
Each message is one JSON line ended by `\n` (NDJSON) with a 16384-byte maximum length.
Supported commands are `Status`, `Enable`, `Disable`, `TakeBreak`, `OpenSettings`, and `Quit`.
The daemon accepts connections in non-blocking mode so `Scheduler::tick()` never stalls behind IPC polling.

## 10. Cozy Design System and Native UI (`sye-ui`)

`sye-ui` renders native desktop controls through `longbridge/nsh` (AppKit on macOS, Win32 and Direct2D on Windows, GTK 4 on Linux).
The visual tokens, contrast pairs, and motion presets follow the [Cozy Design System](https://github.com/tunglt1810/cozy) (`tokens/cozy.tokens.json`, `css/effects.css`, `docs/motion.md`) together with Astryx layout rules (`meta/astryx`).
All visual tokens and motion presets live in `crates/sye-ui/src/theme.rs` using `AdaptiveColor` for Light, Dark, and System modes.
All reusable controls live in `crates/sye-ui/src/ui.rs`.

### 10.1 Cozy Color Tokens

The palette avoids pure `#FFFFFF` and `#000000` to reduce eye strain.
Primary coral buttons pair with dark espresso text to keep WCAG AAA contrast (7.55:1 to 7.64:1).
Status banners and destructive buttons pair `warning-muted` / `error-muted` fills with `on-warning-muted` / `on-error-muted` text from `tunglt1810/cozy`.

| Token | Light Mode | Dark Mode | Role |
|---|---|---|---|
| `BACKGROUND_BODY` | `#FAEBDA` | `#18140E` | Main canvas (`bg-body`) |
| `BACKGROUND_SURFACE` | `#FBF1E1` | `#231D16` | Unified Form Card, panels, and sticky toolbar (`bg-surface`) |
| `BACKGROUND_MUTED` | `#F3DFC8` | `#30261D` | Inputs, switch tracks, segmented track, and secondary buttons (`bg-muted`) |
| `SIDEBAR_BG` / `SIDEBAR_ACTIVE` | `#C3CBB9` / `#F8E8D3` | `#29332A` / `#404A41` | Floating Sage Sidebar and active indicator pill (`nav-bg`, `nav-active`) |
| `SIDEBAR_TEXT` / `SIDEBAR_TEXT_MUTED` | `#403B26` / `#55503A` | `#FFFEF7` / `#B8C4B6` | Sidebar labels and version caption (`nav-text`, `nav-text-muted`) |
| `ACCENT` / `ON_ACCENT` | `#EF8F63` / `#261101` | `#EE8F64` / `#18140E` | Primary CTA fill and WCAG AAA text (`accent`, `on-accent`) |
| `FOCUS_RING` | `#A84A24` | `#F2A57E` | Input focus outline (`focus-ring`) |
| `TEXT_PRIMARY` / `TEXT_SECONDARY` | `#261101` / `#5B4C3B` | `#FFFEF7` / `#C2B7B1` | Primary headings, labels, and supporting text |
| `TEXT_DISABLED` | `#968574` | `#70655C` | Placeholders and disabled controls |
| `WARNING_SOFT` / `ON_WARNING_MUTED` | `#FAE1B5` / `#6B4408` | `#443A26` / `#FCE09B` | Warning status banners |
| `ERROR_SOFT` / `ON_ERROR_MUTED` | `#F9CFC8` / `#7E211C` | `#472826` / `#F9BEB8` | Error banners and destructive actions |
| `HEADING_HIGHLIGHT` | `#FAD3B9` | `#7D4737` | Heading highlighter stroke (`mark-peach`) |
| `KPI_SAGE` | `#E2DEC4` / `#7EAB9A` | `#31392C` / `#5E8C7A` | Short Break summary card |
| `KPI_PEACH` | `#FBCBB4` / `#F48A65` | `#432F28` / `#E5866B` | Long Break summary card |
| `KPI_BUTTER` | `#FAE1B5` / `#FAC76A` | `#443A26` / `#D9A74E` | Exercises summary card |
| `KPI_MIST` | `#D2D7DA` / `#A2B3C1` | `#2B333D` / `#6D8296` | Skip Limit summary card |

### 10.2 Shape, Motion, and Screens

- Border radii follow `tunglt1810/cozy`: `12px` (elements), `14px` (sidebar nav items), `18px` (Form Card), `20px` (Pastel KPI cards), `22px` (Floating Sage Sidebar), `28px` (overlay card), and `9999px` (pill controls).
- Motion follows `tunglt1810/cozy`: `COZY_SPRING` (`stiffness: 420`, `damping: 32`, `mass: 0.9`) drives the sliding sidebar indicator, theme `SegmentedControl` thumb, and switch thumb; `COZY_EASE` (`cubic-bezier(0.22, 1, 0.36, 1)`) drives tab panel entrances (`160ms`), highlighter sweeps (`380ms`), and theme color transitions (`200ms`); pressable pills scale to `0.97` on press.
- All animations stop when the operating system enables Reduce Motion (`reduce_motion()`).
- **Break Overlay**: Uses restful dark-roast tones (`#18140E` canvas, `#231D16` card), displays the embedded app icon, fades in over `fade_in_ms`, and shows the exercise title, `MM:SS` countdown, animated progress bar, shortcut lock notice, `Postpone (Space)`, and `Skip (Esc)`.
- **Settings Window**: Uses the Floating Sage Sidebar (`Timing`, `Policies`, `Exercises`, `About & Support`, with embedded app icon and version footer), fixed page header with four Pastel KPI summary cards, and a unified rounded Form Card containing the scrollable form and clipped sticky toolbar (`Save`, `Reset`, `Cancel`). The About & Support tab provides a Light/Dark/System `SegmentedControl` and a **Buy Me a Coffee** link with QR code for `https://buymeacoffee.com/bbeeezzzzz`.
- Headless UI tests validate both screens in CI using `nsh_testing::Headless`.

## 11. CLI Contract (`safe-your-eyes`)

Global options:

- `--config <PATH>`: Override the TOML configuration file path.
- `--socket <PATH>`: Override the local IPC socket or pipe name.
- `--headless`: Run the daemon without the system tray or GUI subprocesses.

Modes and control flags:

- No control flag: Start the single-instance daemon. If a daemon already listens on `--socket`, print `Safe Your Eyes is already running` to stdout and exit with code 0.
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
