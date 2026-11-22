# Changelog

All notable changes to Hidely are listed here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and versions follow
[Semantic Versioning](https://semver.org/).

## [Unreleased]

### Changed

- New look: a borderless, rounded, see-through black window with its own title bar.
- The window list is now a table with App and Status columns.

### Added

- Search box, and sidebar filters for All, Pinned and Hidden windows with live counts.
- A status pill in the title bar showing how many windows are hidden.

### Removed

- The light theme. Hidely is dark only for now.

## [0.1.1] - 2026-11-22

### Added

- Hidely's own window now appears first in the list, so it can be hidden from capture and pinned
  like any other window.

## [0.1.0] - 2026-11-22

First release.

### Added

- Tray app with a list of open windows and a Hide button for each.
- Pins, with <kbd>Win</kbd> + <kbd>Shift</kbd> + <kbd>H</kbd> to hide or show all pinned windows.
- Panic key <kbd>Win</kbd> + <kbd>Shift</kbd> + <kbd>U</kbd> to show everything.
- Tray icon that turns red while windows are hidden.
- On-screen confirmation that is itself excluded from capture.
- Automatic restore of hidden windows on quit, on crash, and on the next launch after a kill.
- `hidely-cli` with `list`, `hide`, `show` and `restore`.
- Screen-share test page.
- Windows installer and portable executable.

[0.1.1]: https://github.com/CodingRule/Hidely/releases/tag/v0.1.1
[0.1.0]: https://github.com/CodingRule/Hidely/releases/tag/v0.1.0
