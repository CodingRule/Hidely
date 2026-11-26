# Compatibility record

Hidely asks Windows to leave a window out of screen capture. Whether a given app's capture
actually leaves it out depends on how that app captures the screen, so every row here is
something someone looked at, with the versions they used. Nothing is inferred from another
app: if a combination has not been checked, it says **Untested**.

To check a combination yourself, protect the harmless test window from the
[test page](https://codingrule.github.io/Hidely/) and look at that app's own preview. Results
and corrections are welcome as issues or pull requests.

## Capture methods

Programs capture the screen through a small number of Windows mechanisms. These rows test the
mechanism directly: a solid-colour test window was captured before protecting it, while
protected, and after restoring it, and the pixels inside its rectangle were counted. All on
Windows 11 25H2 (26200.9974) on 2026-11-23, using ffmpeg 9.0.1 as the capturing program
unless noted.

| Capture method | Capture mode | Unprotected | Protected | Restored |
|---|---|---|---|---|
| GDI `BitBlt` (ffmpeg `gdigrab`) | Region of the screen | Window visible | **Window absent** | Visible again |
| DXGI Desktop Duplication (ffmpeg `ddagrab`) | Whole monitor | Window visible | **Window absent** | Visible again |
| Windows Graphics Capture (ffmpeg `gfxcapture`) | Whole monitor | Window visible | **Window absent** | Visible again |
| Windows Graphics Capture (ffmpeg `gfxcapture`) | The protected window itself | Window visible | **No picture at all**: the capture delivered no frames | Visible again |
| `PrintWindow` with `PW_RENDERFULLCONTENT` | The protected window itself | Window visible | **Blank**: none of the window's content | Visible again |

## Capture apps

Each app was checked in the app itself, with the versions shown. An app is not marked as
working because another app that uses the same method is.

| Windows | Capture app and version | Capture mode | Result | Checked |
|---|---|---|---|---|
| 11 25H2 (26200.9974) | Google Chrome 157.0.8103.80, `getDisplayMedia` | Entire screen | Protected window absent; visible again after restoring | 2026-11-23, automated |
| 11 25H2 (26200.9974) | Microsoft Edge 158.0.4370.33, `getDisplayMedia` | Entire screen | Protected window absent; visible again after restoring | 2026-11-23, automated |
| any | Chrome or Edge, `getDisplayMedia` | Single window, or browser tab | **Untested** | |
| 11 25H2 (26200.9974) | Mozilla Firefox 161.0, `getDisplayMedia` | Entire screen | Protected window absent | 2026-11-26, by the maintainer, Hidely 0.4.0 |
| 11 25H2 (26200.9974) | Google Meet (browser not recorded) | Entire screen | Protected window absent | 2026-11-26, by the maintainer, Hidely 0.4.0 |
| 11 25H2 (26200.9974) | Zoom Workplace 7.1.9 (48550) | Entire screen | Protected window absent | 2026-11-26, by the maintainer, Hidely 0.4.0 |
| 11 25H2 (26200.9974) | Microsoft Teams 26304.1207.5384.858 | Entire screen | Protected window absent | 2026-11-26, by the maintainer, Hidely 0.4.0 |
| 11 26H2 (26300.9974) | Discord stable 679963, host 1.0.9269 | Entire screen | Protected window absent | 2026-11-24, reported by hu-ja-ja in [issue 10](https://github.com/CodingRule/Hidely/issues/10), Hidely 0.2.1 |
| 11 25H2 (26200.9974) | Discord 1.0.9270 | Entire screen | Protected window absent | 2026-11-26, by the maintainer, Hidely 0.4.0 |
| any | Discord | Single window, or Go Live on an app | **Untested** | |
| 11 25H2 (26200.9974) | OBS Studio 30.1.2 | Display capture (entire screen) | Protected window absent | 2026-11-26, by the maintainer, Hidely 0.4.0 |
| 11 25H2 (26200.9974) | Snipping Tool 11.2609.15.0 | Screenshot | Protected window absent | 2026-11-26, by the maintainer, Hidely 0.4.0 |
| 11 25H2 (26200.9974) | Print Screen key | Screenshot | Protected window absent | 2026-11-26, by the maintainer, Hidely 0.4.0 |
| any | Zoom, Teams, Google Meet, Firefox | Single window | **Untested** | |
| any | OBS Studio | Window and game capture | **Untested** | |
| any | Snipping Tool | Screen recording | **Untested** | |
| 10 (any) | Any | Any | **Untested** | |

Zoom, Teams, Discord and OBS each use one or more of the capture methods above, but which
one depends on the app's version and settings. The rows checked on 2026-11-26 shared the entire screen; sharing a
single window, and OBS's window and game capture, are still unchecked. The versions are the
ones installed on the test PC that day, and the browser used for Google Meet was not written
down.

## Browser extension

The [browser extension](EXTENSION.md) was run end to end in Microsoft Edge 158.0.4370.33 on
Windows 11 25H2 (26200.9974) on 2026-11-24; its page lists the eight situations checked.
Chrome and other Chromium browsers are **Untested**.

## Target windows

What happened when Hidely was asked to protect a window, observed with `hidely-cli` on
Windows 11 25H2 (26200.9974), Hidely not elevated, on 2026-11-23.

| Target | Result |
|---|---|
| 64-bit app, not elevated (Python 3.11 Tk window) | Protection applied, confirmed by reading the setting back |
| 32-bit app (32-bit Windows PowerShell form) | Protection applied and removed three times in a row, and through a session. The app kept running and responding |
| App running as administrator (Task Manager), Hidely not elevated | Reported as Error (access denied). The window was left unchanged |
| App running as administrator, Hidely restarted as administrator | **Untested** |
| Window closed before the request | Reported as gone. Nothing changed |
| Roblox (`RobloxPlayerBeta.exe`, version not recorded), seen once in the app | Reported as Error: the request ran, but reading the setting back showed it had not changed. The game's anti-cheat most likely blocked it. Do not use Hidely on games with anti-cheat |
| Protected process (for example some security software) | **Untested**. Not reachable from user mode by design |

## Measured behaviour

Measured on the same machine and date, with the debug build.

**Newly opened windows.** During a session with a profile, Hidely looks for new windows every
150 ms. Over 5 runs, a new test window was capturable for between 0 and 139 ms (mean 52 ms)
after it became visible, before protection was applied. The change itself took 0 to 4 ms once
the window was noticed. So a newly opened window can appear in a capture for a moment; the
figure will vary with machine load and how the app creates its windows.

**Normal exit.** Ending a session, or quitting, made the protected test window capturable
again and left no recovery file behind.

**Forced termination.** After killing Hidely while a window was protected, the window stayed
excluded from capture: nothing can restore it at that moment. On the next start, Hidely read
its recovery file and made the window capturable again. If Hidely is never started again,
closing and reopening the affected app also clears the setting.

**Stale recovery data.** A recovery record that named the right window and process id but the
wrong process start time was discarded, and the window it pointed at was not modified.
