# Changelog

All notable changes to Safe Your Eyes are recorded in this file.

## [0.5.2] - 2026-11-28

### Fixed

- Toggling `autostart` in `config.toml` no longer resets the active countdown or interrupts an ongoing break.
- Atomic writes for `config.toml`, `session.toml`, and `update.toml` now use unique per-process temporary filenames and remove temporary files on error.

### Changed

- Consolidate shared Cozy tokens, RLE icon decoding, executable path resolution, and cross-platform helper code across crates, release automation, and E2E scripts.

## [0.5.1] - 2026-11-27

### Changed

- Pre-break banner: apply the Cozy Design System palette, spacing, rounded shadow, and `Config.theme` (`system`, `light`, or `dark`) on macOS and Windows.
- Pre-break banner: animate the progress bar continuously instead of stepping once per second, and step once per second when Reduce Motion is on.

### Fixed

- Daemon upgrades: hand off a stale daemon cleanly, wait for the new daemon before opening Settings, and defer idle config reloads during active breaks.
- Reload `config.toml` locale changes reliably across the tray menu and desktop notifications.
- macOS: remove faint rectangular corner halos around the rounded pre-break banner.
- Windows: repaint only the progress bar track when its fill width changes.

## [0.5.0] - 2026-11-27

### Added

- Multi-language support: the app now supports English and Vietnamese (`language` setting: `system`, `en`, or `vi`) across the Settings window, break overlay, pre-break banner, and system tray menu.
- Microsoft Store build (`--features msstore`): Windows MSIX Store packages disable in-app update checks and binary replacement so Microsoft Store manages updates.

## [0.4.0] - 2026-11-27

### Added

- Windows: release builds now make a Microsoft Store (MSIX) bundle for x64 and ARM64. The Store build uses the Windows Startup Apps setting for autostart. The Store build needs Windows 10 version 2004 or later.
- Do Not Disturb: the app detects game sessions and does not show breaks while you play. The `do_not_disturb.while_gaming` setting controls this and is on by default.
- The pre-break banner shows a progress bar and a remaining-time badge. The banner color changes when the break is near.

### Changed

- Autostart is on by default for new installs.
- Screen readers announce the countdown in whole minutes, so the label does not change every second.

### Fixed

- macOS: dragging the Settings window no longer makes the window jump.
- Linux: the app restarts correctly after a self-update.
- A broken `config.toml` no longer changes the system autostart setting.
