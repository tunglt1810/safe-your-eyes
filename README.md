# Safe Your Eyes

[![CI](https://github.com/tunglt1810/safe-your-eyes/actions/workflows/ci.yml/badge.svg)](https://github.com/tunglt1810/safe-your-eyes/actions/workflows/ci.yml)
[![VirusTotal Scan](https://img.shields.io/badge/VirusTotal-SHA256%20Scanned-394EFF?logo=virustotal&logoColor=white)](https://github.com/tunglt1810/safe-your-eyes/actions/workflows/ci.yml)
[![License: GPL v2](https://img.shields.io/badge/License-GPL_v2-blue.svg)](LICENSE)

I built this because working with AI made me forget to look away from the screen. Hours went by and my eyes paid for it.

Safe Your Eyes is a small Rust app for Windows, macOS and Linux. Every so often it tells you to stop and close your eyes for a bit. I usually come back with a clearer head than when I left.

The idea comes from [SafeEyes](https://github.com/slgobinath/safeeyes). The native UI runs on [Native Shell (`longbridge/nsh`)](https://github.com/longbridge/nsh) and uses my [Cozy Design System](https://github.com/tunglt1810/cozy).

![Close your eyes, open your mind. A comic of one workday with Safe Your Eyes](docs/assets/storyline.gif)

The whole workday story, with a state diagram, is in [docs/STORYLINE.md](docs/STORYLINE.md). I drew the charts with [tinycharts](https://github.com/tunglt1810/tinycharts).

## What it does

- A short break of 15 seconds every 15 minutes. A long break of 60 seconds every 75 minutes. A soft chime and a 10-second notice come first. Then a 2-second key lock stops you from skipping by accident.
- Smart Pause stops the timer when you are idle. Do Not Disturb holds off for fullscreen apps and battery power, and you can add your own window rules. Strict mode and a skip limit (2 by default) are there if you need them.
- It is one binary with no `tokio`. GPU use stays at 0% while it waits in the tray.
- No admin rights. Everything lives in your user account.

## Settings window

Run `safe-your-eyes --settings` or open it from the tray.

![Safe Your Eyes Cozy UI preview](docs/assets/ui-demo.gif)

## Install

macOS:

```bash
brew install --cask tunglt1810/tap/safe-your-eyes
```

Windows, macOS and Linux (x86_64 and ARM64): download a binary from [Releases](https://github.com/tunglt1810/safe-your-eyes/releases). You don't need `sudo`.

## Usage

```bash
safe-your-eyes                    # start the daemon
safe-your-eyes --status [--json]
safe-your-eyes --disable 30       # pause for 30 minutes
safe-your-eyes --enable
safe-your-eyes --take-break       # or --short-break / --long-break
safe-your-eyes --settings
safe-your-eyes --print-default-config
safe-your-eyes --validate-config --config /path/to/config.toml
safe-your-eyes --quit
```

## Build from source

You need Rust 1.97 or newer. On Linux you also need `libgtk-4-dev` and `pkg-config`. On macOS 14 or newer, install the Xcode Command Line Tools. On Windows 10 1809 or newer, install the MSVC build tools.

```bash
cargo test --workspace
cargo build --bin safe-your-eyes
python3 e2e/run_e2e.py --bin target/debug/safe-your-eyes --tier all   # 72 E2E tests, Python 3.11+
cargo build --release --bin safe-your-eyes
```

The full spec is in [docs/specs/SPEC.md](docs/specs/SPEC.md).

## Code Signing Policy

Free code signing provided by [SignPath.io](https://signpath.io/), certificate by [SignPath Foundation](https://signpath.org/).

- **Maintainers & Roles**: Committers, Reviewers, and Approvers: [@tunglt1810](https://github.com/tunglt1810).
- **Privacy Policy**: This program does not transfer personal information or telemetry to networked systems. When automatic update checks are enabled (configurable in Settings) or when you manually check for or install an update, the program sends HTTPS requests to GitHub Releases solely to check for and download new versions.
- **Project Website**: [https://tunglt1810.github.io/safe-your-eyes/](https://tunglt1810.github.io/safe-your-eyes/)

## Support

I make this in my free time. If it saves your eyes, you can [buy me a coffee](https://buymeacoffee.com/bbeeezzzzz).

## License

GPL-2.0-only. See [LICENSE](LICENSE).

[@tunglt1810](https://github.com/tunglt1810)
