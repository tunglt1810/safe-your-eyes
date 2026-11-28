# Project: safe-your-eyes

> A small, compact, cross-platform (macOS, Windows, Linux) eye-protection desktop application written in Rust, inspired by `slgobinath/safeeyes`, applying engineering practices from `aicodeking/leanrea` and `erkamyaman/rust-anything`.

## Architecture

4-crate Cargo workspace with a single compiled binary (`safe-your-eyes`), zero `tokio` runtime (`std::thread` + `mpsc`), and an on-demand GUI subprocess architecture that keeps the steady-state background daemon at ~2–4 MB RAM and 0% GPU:

```
┌──────────────────────────────────────────────────────────────────────┐
│  sye-cli (single binary: `safe-your-eyes`)                           │
│  - Default: lightweight background daemon (scheduler + tray + IPC)   │
│  - CLI control: --status, --enable, --disable, --take-break, -b, -l, │
│                 --settings, --quit, --version, --config, --socket    │
│  - Internal subprocess modes: --internal-overlay, --internal-settings│
└───────────────┬──────────────────────────────────┬───────────────────┘
                │                                  │
                ▼                                  ▼
┌───────────────────────────────┐  ┌───────────────────────────────────┐
│  sye-platform (OS adapters)   │  │  sye-ui (on-demand longbridge/nsh)│
│  - Tray (tray-icon + ksni)    │  │  - Cozy Design System (DESIGN.md) │
│  - Idle (X11/Wayland/mac/Win) │  │  - Break overlay + MM:SS timer,   │
│  - DND (fullscreen + battery) │  │    fade-in, 2s shortcut lock      │
│  - Notify (notify-rust/zbus)  │  │  - Floating Sage Sidebar Settings │
│  - Audio (OS WAV playback)    │  │    + Buy Me a Coffee support link │
│  - IPC (interprocess socket)  │  └─────────────────┬─────────────────┘
│  - Autostart helpers          │                    │
└───────────────┬───────────────┘                    │
                │                                    │
                ▼                                    ▼
┌──────────────────────────────────────────────────────────────────────┐
│  sye-core (pure Rust, std + serde + toml, #[forbid(unsafe_code)])    │
│  - BreakQueue (short/long 4:1, random/sequential, per-break override)│
│  - 7-state Scheduler (Start/Waiting/PreBreak/Break/Stopped/Quit/     │
│    Resting), strict mode, postpone, skip, timed disable, rest reset  │
│  - Policies: SmartPause, DoNotDisturb, LimitConsecutiveSkipping      │
│  - Config (TOML defaults, 13 short + 6 long exercises, validation)   │
│  - Session persistence, IPC JSON protocol, WAV chime synthesizer     │
│  - Platform trait interfaces (Clock, IdleProvider, DndProvider, ...) │
└──────────────────────────────────────────────────────────────────────┘
```

### Skill & Quality Rules (`leanrea` + `rust-anything` + User Rules)
1. **Docs & ADRs**: Full product, architecture, and Cozy design specification in `docs/specs/SPEC.md`, temporary implementation plans in `docs/plans/`. Vietnamese technical docs follow STE structural rules; English technical text follows ASD-STE100 Strict mode.
2. **Workspace Safety & Lints**: `[workspace.lints.rust] unsafe_code = "forbid"` (isolated in `sye-platform` only where OS FFI requires it, with mandatory `// SAFETY:` comments); `cargo clippy --all-targets -- -D warnings`; `cargo fmt --check`; zero `unwrap()`/`expect()`/`panic!()` on fallible paths (`// INFALLIBLE:` only for provably impossible states); `#[expect(lint, reason = "...")]` over `#[allow]`.
3. **Minimalism & Binary Size**: Release profile uses `opt-level = "z"`, `lto = "fat"`, `codegen-units = 1`, `panic = "abort"`, `strip = "symbols"`; `default-features = false` on heavy crates; zero `tokio`, zero `libgtk-3-dev` / `libayatana-appindicator3-dev` / `libdbus-1-dev` / `libasound2-dev` build dependencies on Linux.
4. **Temporary Files**: Always use `.tmp/` inside the repo (never OS `/tmp`), with `/.tmp` in `.gitignore`. `.agents/teamwork/` holds agent metadata only.

## Code Layout

```
.
├── .gitignore                        # M1 (includes /target, /.tmp)
├── Cargo.toml                        # M1 (workspace root + lints + release profile)
├── Cargo.lock                        # M1..M4 (kept updated for --locked builds)
├── README.md                         # M4 (updated with build/usage/architecture)
├── .github/workflows/ci.yml          # M4 (Linux/macOS/Windows matrix)
├── docs/
│   ├── assets/
│   │   └── storyline.gif             # Animated tinycharts Cozy workday storyline
│   └── specs/
│       └── SPEC.md                   # Consolidated ASD-STE100 product, architecture, and Cozy design spec
├── crates/
│   ├── sye-core/                     # M1 (pure Rust core library)
│   │   ├── Cargo.toml
│   │   └── src/
│   ├── sye-platform/                 # M2 (cross-platform OS adapters)
│   │   ├── Cargo.toml
│   │   └── src/
│   ├── sye-ui/                       # M3 (Native Shell nsh Cozy overlay + settings)
│   │   ├── Cargo.toml
│   │   └── src/
│   └── sye-cli/                      # M4 (single binary: daemon + CLI + subprocesses)
│       ├── Cargo.toml
│       └── src/
└── e2e/                              # E2E Testing Track (opaque-box test suite)
    ├── run_e2e.py                    # Test runner (exits 0 when all pass)
    └── tests/                        # Tier 1, Tier 2, Tier 3, Tier 4 test suites
```

## Feature Inventory

Every Survey feature is assigned a milestone (none unassigned).

| # | Feature | Description | Milestone | Source |
|---|---------|-------------|-----------|--------|
| F1 | Project Specs, ADRs, Glossary & Hygiene | `.gitignore` (`/.tmp`, `/target`), `docs/specs/SPEC.md`, Cargo workspace with strict lints & size-optimized release profile | M1 | survey (`explorer_survey_1`, `spec_miner_survey_3`) |
| F2 | Dual-Tier Break Queue | Short (15m/15s) & Long (75m/60s) break queues (4:1 ratio), 13 default short + 6 default long exercises, random shuffle per cycle or sequential, per-break interval/duration overrides | M1 | survey (`spec_miner_survey_2` #1–3) |
| F3 | 7-State Scheduler State Machine | Deterministic `Start`, `Waiting`, `PreBreak`, `Break`, `Stopped`, `Quit`, `Resting` transitions; 10s pre-break warning; reentrancy-safe tick/event model with trait-injected `Clock` | M1 | survey (`spec_miner_survey_2` #4–5) |
| F4 | Strict Break, Postpone, Skip & Anti-Accidental Lock | `strict_break` (disables Skip), `allow_postpone` (queue-preserving postpone by configured minutes/seconds), Skip (advances queue, skips chime), `shortcut_disable_time` (2s lock) | M1 | survey (`spec_miner_survey_2` #6–8, #15) |
| F5 | Manual Break & Timed Pause/Disable | Take break now (Any/Short/Long, bypasses pre-break); Enable/Disable/Pause for N minutes (30m/1h/2h/3h) or until restart with auto-resume | M1 | survey (`spec_miner_survey_2` #9–10) |
| F6 | Natural Rest / Suspend Cycle Reset | `is_resting` pause tracks elapsed time; if resting/sleeping `> long_break_duration`, calls `skip_long_break()` to reset cycle cleanly | M1 | survey (`spec_miner_survey_2` #11, #13) |
| F7 | Smart Pause Policy | Idle credit-back (`idle_time <= idle < short_break_interval` shifts next break forward), full reset (`idle >= short_break_interval`), optional `postpone_if_active` (2s deferral) | M1 | survey (`spec_miner_survey_2` #19) |
| F8 | Do Not Disturb Policy | Fullscreen suppression, `skip_break_windows` > `take_break_windows` precedence, `while_on_battery` check | M1 | survey (`spec_miner_survey_2` #20) |
| F9 | Limit Consecutive Skipping Policy | Tracks consecutive skips/postpones; disables Skip & Postpone at limit (`default 2`); resets on completed break; exposes status text | M1 | survey (`spec_miner_survey_2` #23) |
| F10 | TOML Config, Validation & Session Persistence | Human-readable TOML config with schema version, validation, corrupted-config fallback to defaults, reset-to-defaults, optional `session.toml` persistence | M1 | survey (`spec_miner_survey_2` #27, `spec_miner_survey_3`) |
| F11 | Pure-Rust WAV Chime Synthesizer & IPC Protocol | Synthesizes gentle pre/post-break PCM WAV chime in pure Rust (zero external audio asset bloat); typed JSON IPC `IpcCommand`/`IpcResponse` protocol | M1 | survey (`spec_miner_survey_2` #22, `spec_miner_survey_3`) |
| F12 | Cross-Platform System Tray | `tray-icon` (`ksni` pure-Rust D-Bus on Linux, native macOS/Windows): enabled/disabled/pre-break states, next-break tooltip, Enable/Disable submenu, Take-a-break submenu, Settings, Quit; graceful degrade when headless | M2 | survey (`spec_miner_survey_2` #17–18, `spec_miner_survey_3`) |
| F13 | Cross-Platform Idle & DND Providers | OS idle duration (Linux X11/Wayland, macOS, Windows), fullscreen/active-window detection, battery discharging status, all with graceful fallback | M2 | survey (`spec_miner_survey_2` #19–20, `spec_miner_survey_3`) |
| F14 | Cross-Platform Notifications & Audio Playback | `notify-rust` (`zbus` on Linux, native macOS/Windows) for pre-break alerts; non-blocking OS WAV chime playback (`pw-play`/`paplay`/`aplay`, `afplay`, `PlaySoundW`) with volume clamp & graceful fallback | M2 | survey (`spec_miner_survey_2` #21–22, `spec_miner_survey_3`) |
| F15 | Single-Instance Local IPC & Autostart | `interprocess` local socket (UDS on Linux/macOS, Named Pipes on Windows) with stale-socket recovery; OS autostart entry helper | M2 | survey (`spec_miner_survey_2` #27, #29, `spec_miner_survey_3`) |
| F16 | Multi-Monitor Fullscreen Break Overlay UI | Native Shell (`nsh`): dark fullscreen overlay per monitor, fade-in animation (`1500ms`), centered bold exercise text, `MM:SS` countdown, skip-limit label, Skip (`Esc`) & Postpone (`Space`) with 2s lock & strict mode | M3 | survey (`spec_miner_survey_2` #12–16, `spec_miner_survey_3`) |
| F17 | Compact Settings UI | Native Shell (`nsh`) Cozy Settings window: edit intervals/durations/toggles/window rules/exercises, Reset to defaults, autosave | M3 | survey (`spec_miner_survey_2` #28, `spec_miner_survey_3`) |
| F18 | Single-Binary CLI, Daemon & Subprocess Orchestration | `safe-your-eyes` binary: daemon mode, CLI flags (`--status`, `--enable`, `--disable [MIN]`, `--take-break`, `-b/--short-break`, `-l/--long-break`, `--settings`, `--quit`, `--version`, `--config`, `--socket`, `--headless`), on-demand `--internal-overlay`/`--internal-settings` subprocesses, signal handling, release size verification | M4 | survey (`spec_miner_survey_2` #29, `spec_miner_survey_3`) |
| F19 | Cross-Platform CI Matrix | `.github/workflows/ci.yml` running `fmt`, `clippy`, `test`, and release build across `ubuntu-latest`, `macos-latest`, `windows-latest` | M4 | survey (`explorer_survey_1`, `spec_miner_survey_3`) |
| F20 | Opaque-Box E2E Test Suite (Tiers 1–4) | Requirement-driven E2E harness & tests in `e2e/` covering all CLI/config/scheduler/IPC/policy behaviors + `TEST_READY.md` | E2E Track | Project Pattern |
| F21 | Final Verification & Tier 5 Hardening | Pass 100% of E2E Tiers 1–4 + white-box Tier 5 adversarial coverage hardening | M5 | Project Pattern |
| F22 | macOS AppKit Event Loop, Visible Tray & Interactive Startup/Re-open | `PlatformEventLoop` in `sye-platform` (macOS `NSApplication` + `NSApplicationDelegate` reopen/terminate + `finishLaunching()` + event pump; Windows Win32 message pump), tray error logging, `--background` flag (used by autostart), `-psn_*` ignore, interactive launch & duplicate-launch `OpenSettings` | M6 | survey (`explorer_survey_1`) |
| F23 | Seamless Themed Hidden Titlebar | `Root::main_window` with `TitleBarStyle::Hidden`, macOS sidebar-integrated `controls_offset`, reactive `title_bar()` insets (macOS left / Windows & Linux right), `.window_drag(true/false)` matching Cozy theme & `ui-demo.gif` | M6 | survey (`explorer_survey_2`, `explorer_survey_3`) |
| F24 | Concentric Window & Inner Component Borders | Harmonized concentric radii (`SIDEBAR`/`CONTAINER`/`KPI` = 12pt, `NAV_ITEM`/`ELEMENT` = 8pt), root `.p_3().gap_3()`, matching hairline border on sidebar, remove redundant `.rounded_b` on `sticky_toolbar`, tidy number-field right alignment | M6 | survey (`explorer_survey_2`) |
| F25 | Centered & Animated Toggle Switch | Fix `cozy_switch` track layout (`.flex_row().items_center().px(px(2.0))`) and thumb checked translation (`.translate_x(px(0.0)).checked(\|c\| c.translate_x(px(18.0)))`) with `COZY_SPRING` | M6 | survey (`explorer_survey_2`) |
| F26 | Dynamic Per-Screen Page Titles | Replace hardcoded `"Settings"` H1 and duplicate badge pill with reactive active tab label (`Timing`, `Policies`, `Exercises`, `About & Support`) + animated Cozy highlight bar | M6 | survey (`explorer_survey_2`) |
| F27 | Top-Right Cozy Theme Switcher & Circular Reveal Effect | Persisted `Config.theme: ThemeMode` (`system`/`light`/`dark`), top-right Cozy pill switcher with vector SVG icon in `page_header` (removing duplicate from About), `nsh` `shader` feature + `screen_transition()` WGSL 480ms `cubic-bezier(0.4,0,0.2,1)` circular reveal from button center to farthest corner with `reduce_motion()` & resolved-mode guards | M6 | survey (`explorer_survey_3`) |

## Milestones

| # | Name | Scope | Dependencies | Status |
|---|------|-------|--------------|--------|
| M1 | `foundation-and-core` | Specs, ADRs, Glossary, Cargo workspace, `sye-core` (F1–F11) + compiling crate stubs | none | DONE (50/50 tests) |
| M2 | `platform-adapters` | `sye-platform` OS adapters: tray, idle, DND, notifications, audio, IPC, autostart (F12–F15) | M1 | DONE (36/36 tests) |
| M3 | `ui-overlay-and-settings` | `sye-ui` `longbridge/nsh` Cozy break overlay + Floating Sage Sidebar Settings UI + Buy Me a Coffee link (F16–F17) | M1 | DONE (28/28 tests incl. `nsh_testing::Headless`) |
| M4 | `cli-daemon-integration` | `sye-cli` single binary, daemon loop, CLI IPC, subprocess launcher, CI matrix, README (F18–F19) | M2, M3 | DONE (3/3 CLI tests + CI) |
| M5 | `final-e2e-and-hardening` | Phase 1: 100% E2E Tiers 1–4 pass (F21) | M4, E2E Track | DONE (72/72 E2E pass) |
| E2E | `e2e-testing-track` | Independent opaque-box E2E test infra (`TEST_INFRA.md`) + Tiers 1–4 suite (`e2e/`) (F20) | none | DONE (72/72 pass) |
| M6 | `macos-v011-fixes` | macOS startup/tray/re-open + Cozy UI parity (hidden themed titlebar, concentric borders, toggle fix, per-screen titles, top-right theme switcher & circular reveal) (F22–F27) | M1–M5 | DONE (133/133 unit/headless + 72/72 E2E) |

## Interface Contracts

### 1. CLI & Process Entry Point (`safe-your-eyes`)
Used by both `sye-cli` (M4, M6) and `e2e/` (E2E Testing Track):
- **Binary name**: `safe-your-eyes` (`target/debug/safe-your-eyes` or `target/release/safe-your-eyes`)
- **Global options**:
  - `--config <PATH>`: Override config TOML file path (default: OS config dir `safe-your-eyes/config.toml`).
  - `--socket <PATH>`: Override local IPC socket/pipe name (essential for isolated E2E runs).
  - `--headless`: Run daemon without system tray or GUI overlay subprocesses (logs break events / simulates break completion via timer; ideal for headless CI & E2E).
  - `--background`: Start daemon silently in the system tray without opening the Settings window (used by OS login autostart entries).
  - macOS LaunchServices `-psn_*` arguments are silently ignored during CLI argument parsing.
- **Modes / Control Flags**:
  - *(no control flag)*: Start daemon (single-instance). On interactive launch (`!headless && !background`), opens Settings immediately while keeping tray alive. If already running on `--socket`, forwards `IpcCommand::OpenSettings` when `!headless && !background`, prints `"Safe Your Eyes is already running"` to stdout, and exits `0`.
  - `--status`: Query running daemon status; print human-readable status (or JSON with `--json`) to stdout and exit `0`. If daemon not running, print `"Safe Your Eyes is not running"` to stderr and exit `1`.
  - `--status --json`: Print structured JSON status: `{"state": "...", "enabled": true/false, "next_break_type": "short"|"long"|null, "next_break_name": "...", "seconds_until_next_break": N, "consecutive_skips": N}`.
  - `-e, --enable`: Resume/enable running daemon. Exit `0` if running, `1` if not running.
  - `-d, --disable [MINUTES]`: Pause/disable running daemon (if `MINUTES` omitted or `0`, until restart; if `> 0`, auto-resume after `MINUTES`). Exit `0` if running, `1` if not running.
  - `-t, --take-break`: Trigger next scheduled break immediately. Exit `0` if running, `1` if not running.
  - `-b, --short-break`: Trigger immediate short break. Cannot combine with `-l/--long-break` (must print `"Cannot combine -b/--short-break with -l/--long-break"` to stderr and exit `1`).
  - `-l, --long-break`: Trigger immediate long break. Cannot combine with `-b/--short-break`.
  - `-s, --settings`: Open Settings UI (or forward to daemon).
  - `-q, --quit`: Gracefully stop running daemon. Exit `0` if running, `1` if not running.
  - `-V, --version`: Print `safe-your-eyes <version>` and exit `0`.
  - `--print-default-config`: Print default TOML config to stdout and exit `0`.
  - `--validate-config`: Validate `--config <PATH>` TOML and exit `0` if valid, `1` with error message on stderr if invalid.
  - `--internal-overlay <JSON_PAYLOAD>` / `--internal-settings --config <PATH>`: Internal subprocess entry points spawned by daemon.

### 2. `sye-core` ↔ `sye-platform` & `sye-ui`
Defined in `crates/sye-core/src/`:
- `ThemeMode` (`#[serde(rename_all = "snake_case")]`, default `System`): `System | Light | Dark`.
- `Config` (TOML-serializable, `#[serde(default)]`): `theme: ThemeMode` (default `system`), `short_break_interval_secs` (900), `short_break_duration_secs` (15), `long_break_interval_secs` (4500), `long_break_duration_secs` (60), `pre_break_warning_secs` (10), `strict_break` (false), `allow_postpone` (false), `postpone_duration_secs` (300), `random_order` (true), `shortcut_disable_secs` (2), `fade_in_ms` (1500), `persist_state` (false), `smart_pause` (`enabled`, `idle_threshold_secs`, `postpone_if_active`), `do_not_disturb` (`enabled`, `skip_break_windows`, `take_break_windows`, `while_on_battery`), `audible_alert` (`pre_break`, `post_break`, `volume`), `limit_consecutive_skips` (`enabled`, `max_skips`), `short_breaks: Vec<BreakSpec>`, `long_breaks: Vec<BreakSpec>`.
- `SchedulerState`: `Start`, `Waiting`, `PreBreak`, `Break`, `Stopped`, `Quit`, `Resting`.
- `SchedulerAction`: `NotifyPreBreak { break_spec, warning_secs }`, `DismissNotification`, `PlayChime { kind: ChimeKind, volume: u8 }`, `ShowOverlay(OverlayRequest)`, `CloseOverlay`, `UpdateTray(TraySnapshot)`, `PersistSession(SessionState)`.
- `OverlayRequest`: `{ break_type, title, duration_secs, strict_break, allow_postpone, postpone_duration_secs, shortcut_disable_secs, fade_in_ms, status_note: Option<String> }`.
- `OverlayOutcome`: `Completed`, `Skipped`, `Postponed { duration_secs: u64 }`, `Closed`.
- `IpcCommand` / `IpcResponse`: JSON-line messages over local socket (`Status`, `Enable`, `Disable { minutes: Option<u64> }`, `TakeBreak { kind: Option<BreakType> }`, `OpenSettings`, `Quit`).
- Platform traits & adapters: `Clock`, `IdleProvider`, `DndProvider`, `Notifier`, `AudioPlayer`, `OverlayLauncher`, `PlatformEventLoop` (main-thread OS event loop init & pump for tray/reopen).
