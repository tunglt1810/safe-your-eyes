# Safe Your Eyes

[![CI](https://github.com/tunglt1810/safe-your-eyes/actions/workflows/ci.yml/badge.svg)](https://github.com/tunglt1810/safe-your-eyes/actions/workflows/ci.yml)
[![VirusTotal Scan](https://img.shields.io/badge/VirusTotal-SHA256%20Scanned-394EFF?logo=virustotal&logoColor=white)](https://github.com/tunglt1810/safe-your-eyes/actions/workflows/ci.yml)
[![License: GPL v2](https://img.shields.io/badge/License-GPL_v2-blue.svg)](LICENSE)

AI makes us work faster than ever—so it is easy to get carried away and strain your eyes.
Safe Your Eyes is a lightweight, cross-platform Rust desktop app that reminds you to pause, close your eyes to open your mind, spark sharper creative ideas, and put AI to even better use.
Inspired by [SafeEyes](https://github.com/slgobinath/safeeyes), built with native controls via [Native Shell (`longbridge/nsh`)](https://github.com/longbridge/nsh), and styled with the [Cozy Design System](https://github.com/tunglt1810/cozy).

![Close your eyes, open your mind — Safe Your Eyes workday time-lapse](docs/assets/storyline.gif)

Read the 7-part workday story and state diagram in [docs/STORYLINE.md](docs/STORYLINE.md). Illustrated with [tinycharts](https://github.com/tunglt1810/tinycharts).

## Features

- **Gentle break cadence**: Default 15-minute / 15-second short breaks and 75-minute / 60-second long breaks (4:1 ratio), with a 10-second pre-break notice, synthesized PCM WAV chimes, and a 2-second key lock to stop accidental skips.
- **Smart workday policies**: Smart Pause on idle, Do Not Disturb (fullscreen apps, battery power, window rules), strict break mode, consecutive skip limits (default 2/2), optional session persistence across restarts, and user-level OS login autostart toggle.
- **Lightweight single binary (`safe-your-eyes`)**: Bundles the background daemon, CLI client, and on-demand GUI subprocesses with no `tokio` runtime, keeping GPU usage at 0% while waiting in the system tray.
- **Native Cozy UI**: Light and Dark themes from the [Cozy Design System](https://github.com/tunglt1810/cozy), featuring a sage sidebar, pastel summary cards, high-contrast coral actions, and reduced-motion support on macOS, Windows, and Linux.

## UI Preview

Open the native Cozy Settings window anytime with `safe-your-eyes --settings` or from the system tray:

![Safe Your Eyes Cozy UI preview](docs/assets/ui-demo.gif)

## Install

### macOS (Homebrew Cask)

```bash
brew install --cask tunglt1810/tap/safe-your-eyes
```

### Prebuilt Binaries (Windows, macOS, Linux — x86_64 & ARM64)

Download the latest release binary from [GitHub Releases](https://github.com/tunglt1810/safe-your-eyes/releases). No administrator privileges or `sudo` required.

## Prerequisites

- Rust 1.97 or newer (Rust 2024 edition)
- **Linux**: `libgtk-4-dev` and `pkg-config` (`sudo apt-get install libgtk-4-dev pkg-config`)
- **macOS**: Xcode Command Line Tools (macOS 14 or newer)
- **Windows**: MSVC build tools (Windows 10 1809 or newer)

## Build and Test

```bash
# Run unit tests and headless NSH UI tests
cargo test --workspace

# Build the debug binary
cargo build --bin safe-your-eyes

# Run all 72 opaque-box E2E tests (Python 3.11+ standard library only)
python3 e2e/run_e2e.py --bin target/debug/safe-your-eyes --tier all

# Build the size-optimized release binary
cargo build --release --bin safe-your-eyes
```

## Usage

```bash
# Start the background daemon
safe-your-eyes

# Check status (human-readable or JSON)
safe-your-eyes --status
safe-your-eyes --status --json

# Pause for 30 minutes, then resume
safe-your-eyes --disable 30
safe-your-eyes --enable

# Start a break immediately
safe-your-eyes --take-break
safe-your-eyes --short-break
safe-your-eyes --long-break

# Open the Cozy Settings window
safe-your-eyes --settings

# Print or validate configuration
safe-your-eyes --print-default-config
safe-your-eyes --validate-config --config /path/to/config.toml

# Stop the daemon cleanly
safe-your-eyes --quit
```

## Specification

Read the full product, architecture, Cozy design, and CLI specification in [docs/specs/SPEC.md](docs/specs/SPEC.md).

## Code Signing Policy

Free code signing provided by [SignPath.io](https://signpath.io/), certificate by [SignPath Foundation](https://signpath.org/).

- **Maintainers & Roles**: Committers, Reviewers, and Approvers: [@tunglt1810](https://github.com/tunglt1810).
- **Privacy Policy**: This program will not transfer any information to other networked systems unless specifically requested by the user or the person installing or operating it.
- **Project Website**: [https://tunglt1810.github.io/safe-your-eyes/](https://tunglt1810.github.io/safe-your-eyes/)

## Support

If Safe Your Eyes helps protect your vision, you can support the author here:

- **Buy Me a Coffee**: [https://buymeacoffee.com/bbeeezzzzz](https://buymeacoffee.com/bbeeezzzzz)

## License

GPL-2.0-only. See [LICENSE](LICENSE).
