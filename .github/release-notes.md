Hidely asks Windows to leave the windows you choose out of screen capture, while they stay visible to you.

## What's new in 0.3.1

- **The browser extension from the Chrome Web Store can connect to Hidely.** Before, only the copy loaded from source could. If you connected Chrome and Edge with 0.3.0, Hidely updates that connection by itself the next time it starts; there's nothing to do.

Everything else is the same as 0.3.0: the [browser extension](https://github.com/CodingRule/Hidely/blob/main/docs/EXTENSION.md) for private tabs and sites, and Settings for starting with Windows, checking for updates, and remembering the window's position.

## Please read before relying on it

- **Check your meeting app first.** Discord's screen share and screen sharing in Chrome and Edge are confirmed; Zoom, Teams, OBS and Firefox have not been checked yet. Use the [test page](https://codingrule.github.io/Hidely/) and your app's own preview. The [compatibility record](https://github.com/CodingRule/Hidely/blob/main/docs/COMPATIBILITY.md) lists what has been tested.
- **A private tab can be seen for a moment** when you switch to it, before the window is protected.
- **Don't use it on games with anti-cheat** such as Roblox. The change is blocked there and it could flag your account.
- It only affects screen capture. It does not conceal a window from someone looking at your monitor, or from software that lists running programs.

## Install

Pick one of the app downloads below. Neither needs Rust, Node or any other developer tools.

| File | What it is |
|---|---|
| **Hidely_0.3.1_x64-setup.exe** | Installer. Adds Hidely to the Start menu and installs Microsoft's WebView2 runtime for you if it is missing. Recommended. |
| **Hidely_0.3.1_x64-portable.exe** | Single file, no install. Just run it. Needs the WebView2 runtime, which Windows 11 already has. |
| **hidely-extension_0.3.1.zip** | The browser extension, to unzip and load in Chrome or Edge if you don't use the store version. Optional. |
| **hidely-cli_0.3.1_x64.exe** | Optional command-line tool for scripts. |

Requires Windows 10 version 2004 or later, 64-bit Windows. Hidely is Windows-only.

Windows will show a "Windows protected your PC" warning because the downloads are not code-signed yet. Choose **More info**, then **Run anyway**. Some antivirus products may also flag Hidely, because it has to run a small call inside other apps to change their windows. The source is open if you want to check what it does.

See the [README](https://github.com/CodingRule/Hidely#readme) for how it works, and the [changelog](https://github.com/CodingRule/Hidely/blob/main/CHANGELOG.md) for earlier versions.
