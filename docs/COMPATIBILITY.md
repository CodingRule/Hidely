# Compatibility record

Hidely asks Windows to leave a window out of screen capture. Whether a given app's capture
actually leaves it out depends on how that app captures the screen, so every row here is
something someone looked at, with the versions they used. Nothing is inferred from another
app: if a combination has not been checked, it says **Untested**.

To check a combination yourself, protect the harmless test window from the
[test page](https://codingrule.github.io/Hidely/) and look at that app's own preview. Results
and corrections are welcome as issues or pull requests.

## Capture apps

| Windows | Capture app and version | Capture mode | Result | Checked |
|---|---|---|---|---|
| 11 25H2 (26200.9974) | GDI screen copy (`BitBlt`, via .NET `CopyFromScreen`) | Region of the primary monitor | Protected window absent; what was behind it shows instead | 2026-11-22, during development |
| 11 25H2 (26200) | Google Chrome, `getDisplayMedia` (Hidely test page). Chrome version not recorded at the time; 157.0.8103.80 was installed the next day | Entire screen | Protected window absent from the preview | 2026-11-22, reported by the maintainer |
| any | Google Chrome, `getDisplayMedia` | Single window, or browser tab | **Untested** | |
| any | Microsoft Edge, `getDisplayMedia` | Any | **Untested** | |
| any | Mozilla Firefox, `getDisplayMedia` | Any | **Untested** | |
| any | Zoom | Any | **Untested** | |
| any | Microsoft Teams | Any | **Untested** | |
| any | Google Meet | Any | **Untested** | |
| any | Discord | Any | **Untested** | |
| any | OBS Studio | Display, window and game capture | **Untested** | |
| any | Snipping Tool, Print Screen | Screenshot and recording | **Untested** | |
| 10 (any) | Any | Any | **Untested** | |

## Target windows

What happened when Hidely was asked to protect a window, observed with `hidely-cli` on
Windows 11 25H2 (26200.9974), Hidely not elevated, on 2026-11-23.

| Target | Result |
|---|---|
| 64-bit app, not elevated (Python 3.11 Tk window) | Protection applied, confirmed by reading the setting back |
| 32-bit app (32-bit Windows PowerShell form) | Reported as Unsupported. The window was left unchanged |
| Window closed before the request | Reported as gone. Nothing changed |
| Roblox (`RobloxPlayerBeta.exe`, version not recorded), seen once in the app | Reported as Error: the request ran, but reading the setting back showed it had not changed. The game's anti-cheat most likely blocked it. Do not use Hidely on games with anti-cheat |
| Elevated or protected app | **Untested** on a real window. The code reports it as an error and never as protected (covered by a unit test only) |

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
