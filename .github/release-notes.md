Hidely asks Windows to leave the windows you choose out of screen capture, while they stay visible to you.

## What's new in 0.4.0

- **What viewers see.** A picture of your screen as Windows screen capture returns it on this PC, refreshed once a second while it is open. Protected windows are missing from it. The picture is never saved or sent.
- **Protected windows are re-checked.** If an app resets its protection during a session, Hidely says so and protects the window again, up to three times. Each row shows when it was last checked.
- **Session summary.** Ending a session shows how long it ran, how many windows were hidden, and anything that went wrong.
- **Meeting app offer.** When Zoom, Teams, OBS or Webex opens, Hidely offers to start a session. It only offers; it never starts by itself. You can turn this off in Settings.
- **Keyboard and command palette.** Arrow keys select, Enter protects, P pins, `/` searches, and Ctrl+K lists everything Hidely can do.
- **First-run tour.** Three short steps the first time Hidely opens. It can be skipped, and replayed from Settings.
- **A refreshed look**, with status wording that says what viewers get: "Hidden from capture" and "Visible to viewers".

The full list is in the [changelog](https://github.com/CodingRule/Hidely/blob/main/CHANGELOG.md).

## Please read before relying on it

- **Check your meeting app first.** Discord's screen share and screen sharing in Chrome and Edge are confirmed; Zoom, Teams, OBS and Firefox have not been checked yet. Use the [test page](https://codingrule.github.io/Hidely/) and your app's own preview. The [compatibility record](https://github.com/CodingRule/Hidely/blob/main/docs/COMPATIBILITY.md) lists what has been tested.
- **"What viewers see" shows what Windows returns.** A meeting app can capture differently, so the test page is still the check for a real share.
- **A private tab can be seen for a moment** when you switch to it, before the window is protected.
- **Don't use it on games with anti-cheat** such as Roblox. The change is blocked there and it could flag your account.
- It only affects screen capture. It does not conceal a window from someone looking at your monitor, or from software that lists running programs.

## Install

Pick one of the app downloads below. Neither needs Rust, Node or any other developer tools.

| File | What it is |
|---|---|
| **Hidely_0.4.0_x64-setup.exe** | Installer. Adds Hidely to the Start menu and installs Microsoft's WebView2 runtime for you if it is missing. Recommended. |
| **Hidely_0.4.0_x64-portable.exe** | Single file, no install. Just run it. Needs the WebView2 runtime, which Windows 11 already has. |
| **hidely-extension_0.4.0.zip** | The browser extension, to unzip and load in Chrome or Edge if you don't use the store version. Optional. |
| **hidely-cli_0.4.0_x64.exe** | Optional command-line tool for scripts. |

Requires Windows 10 version 2004 or later, 64-bit Windows. Hidely is Windows-only.

Windows will show a "Windows protected your PC" warning because the downloads are not code-signed yet. Choose **More info**, then **Run anyway**. Some antivirus products may also flag Hidely, because it has to run a small call inside other apps to change their windows. The source is open if you want to check what it does.

See the [README](https://github.com/CodingRule/Hidely#readme) for how it works, and the [changelog](https://github.com/CodingRule/Hidely/blob/main/CHANGELOG.md) for earlier versions.
