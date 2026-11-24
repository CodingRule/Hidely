Hidely asks Windows to leave the windows you choose out of screen capture, while they stay visible to you.

## What's new in 0.3.0

### Browser extension for Chrome and Edge

Mark a tab or a whole site private, and Hidely protects that browser window while it is showing, then releases it when you switch to an ordinary tab. Screen capture can only leave out whole windows, so the entire browser window disappears from your share, not just the tab.

To use it: switch on **Connect Chrome and Edge** in Hidely's Settings, then load the extension from `hidely-extension_0.3.0.zip` below. The [extension guide](https://github.com/CodingRule/Hidely/blob/main/docs/EXTENSION.md) has the steps, the limits and the privacy details. It has been tested in Edge; Chrome uses the same extension system but has not been checked yet.

### Settings

- **Start Hidely when you sign in to Windows** (opt-in). It opens in the tray.
- **Check for a new version**, with a **Check now** button and an opt-in check at startup. Both are off by default, so Hidely still makes no network connection by itself.
- **The window remembers** its size and position.

### Also

- `Hidely.exe --minimized` starts in the tray and `Hidely.exe --quit` closes it.
- Fixed: a settings file saved by Notepad or PowerShell was treated as unreadable and replaced with defaults.

## Please read before relying on it

- **Check your meeting app first.** Zoom, Teams, Discord, OBS and Firefox have not been checked in the apps themselves yet. Use the [test page](https://codingrule.github.io/Hidely/) and your app's own preview. The [compatibility record](https://github.com/CodingRule/Hidely/blob/main/docs/COMPATIBILITY.md) lists what has been tested.
- **A private tab can be seen for a moment** when you switch to it, before the window is protected.
- **Don't use it on games with anti-cheat** such as Roblox. The change is blocked there and it could flag your account.
- It only affects screen capture. It does not conceal a window from someone looking at your monitor, or from software that lists running programs.

## Install

Pick one of the app downloads below. Neither needs Rust, Node or any other developer tools.

| File | What it is |
|---|---|
| **Hidely_0.3.0_x64-setup.exe** | Installer. Adds Hidely to the Start menu and installs Microsoft's WebView2 runtime for you if it is missing. Recommended. |
| **Hidely_0.3.0_x64-portable.exe** | Single file, no install. Just run it. Needs the WebView2 runtime, which Windows 11 already has. |
| **hidely-extension_0.3.0.zip** | The browser extension, to unzip and load in Chrome or Edge. Optional. |
| **hidely-cli_0.3.0_x64.exe** | Optional command-line tool for scripts. |

Requires Windows 10 version 2004 or later, 64-bit Windows. Hidely is Windows-only.

Windows will show a "Windows protected your PC" warning because the downloads are not code-signed yet. Choose **More info**, then **Run anyway**. Some antivirus products may also flag Hidely, because it has to run a small call inside other apps to change their windows. The source is open if you want to check what it does.

## Known limits

- Apps running as administrator need Hidely restarted as administrator
- Protected processes can't be changed
- A window opened during a session can be captured for a moment before it is protected
- If Hidely is killed outright, windows stay protected until it is started again

See the [README](https://github.com/CodingRule/Hidely#readme) for how it works, and the [changelog](https://github.com/CodingRule/Hidely/blob/main/CHANGELOG.md) for earlier versions.
