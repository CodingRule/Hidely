# Changelog

All notable changes to Hidely are listed here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and versions follow
[Semantic Versioning](https://semver.org/).

## [0.2.0] - 2026-11-23

### Added

- Privacy sessions with Start and End. Ending a session makes every window Hidely changed
  capturable again and reports any it could not restore.
- Meeting profiles: named sets of apps, with create, rename, duplicate, delete and select.
  Starting a session shows which apps it will affect and which are not running.
- During a session with a profile, windows those apps open later are protected as well.
- Per-window status: Unprotected, Applying, Protection applied, Unsupported or Error, shown
  with words and a distinct icon, with the reason for failures.
- Shortcuts can be changed. A combination that is taken is refused and the previous one
  keeps working. Restore defaults from the window or the tray.
- Tray menu: status line, start or end session, make everything capturable again.
- Optional local diagnostics report, shown in full before saving.
- The mouse pointer stays a plain arrow while it is over a protected window, so its shape
  cannot reveal the window in a capture.
- `hidely-cli watch` runs a session from a terminal.
- `Hidely.exe --toggle-session` and `--end-session` control the running app from a script.
- Test page: a harmless test window to protect, and a clear statement of what the preview
  does and does not verify.
- [Compatibility record](docs/COMPATIBILITY.md) of tested capture apps and measured behaviour.

### Changed

- "Protection applied" is only reported after Windows confirms the setting by reading it back.
- A window that an app already excludes from capture itself is left alone and never
  "restored" by Hidely.
- Recovery records now include the process start time, so a reused window handle or process
  id can never cause an unrelated window to be changed.
- Hide and Show are now Protect and Unprotect. The hide/show shortcut now starts and ends a
  session.
- The README no longer lists capture apps that have not been tested.

## [0.1.2] - 2026-11-22

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

[0.2.0]: https://github.com/CodingRule/Hidely/releases/tag/v0.2.0
[0.1.2]: https://github.com/CodingRule/Hidely/releases/tag/v0.1.2
[0.1.1]: https://github.com/CodingRule/Hidely/releases/tag/v0.1.1
[0.1.0]: https://github.com/CodingRule/Hidely/releases/tag/v0.1.0
