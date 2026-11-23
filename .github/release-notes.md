Hidely asks Windows to leave the windows you choose out of screen capture, while they stay visible to you.

## What's new in 0.2.1

- **32-bit apps are supported.** They were reported as Unsupported before.
- **Restart as administrator.** A link at the bottom left restarts Hidely elevated, so it can protect windows of apps that run as administrator. Windows asks you to approve.
- **More of it is tested.** The three main Windows capture methods, direct window capture, and screen sharing in Chrome and Edge are now confirmed to leave a protected window out. The [compatibility record](https://github.com/CodingRule/Hidely/blob/main/docs/COMPATIBILITY.md) says exactly what was checked and how.
- New demo recording showing the current design.

## Please read before relying on it

- **Check your meeting app first.** Zoom, Teams, Discord, OBS and Firefox have not been checked in the apps themselves yet. Use the [test page](https://codingrule.github.io/Hidely/) and your app's own preview.
- **Don't use it on games with anti-cheat** such as Roblox. The change is blocked there and it could flag your account.
- It only affects screen capture. It does not conceal a window from someone looking at your monitor, or from software that lists running programs.

## Install

Pick one of the downloads below. Neither needs Rust, Node or any other developer tools.

| File | What it is |
|---|---|
| **Hidely_0.2.1_x64-setup.exe** | Installer. Adds Hidely to the Start menu and installs Microsoft's WebView2 runtime for you if it is missing. Recommended. |
| **Hidely_0.2.1_x64-portable.exe** | Single file, no install. Just run it. Needs the WebView2 runtime, which Windows 11 already has. |
| **hidely-cli_0.2.1_x64.exe** | Optional command-line tool for scripts. |

Requires Windows 10 version 2004 or later, 64-bit Windows. Hidely is Windows-only.

Windows will show a "Windows protected your PC" warning because the downloads are not code-signed yet. Choose **More info**, then **Run anyway**. Some antivirus products may also flag Hidely, because it has to run a small call inside other apps to change their windows. The source is open if you want to check what it does.

## Known limits

- Apps running as administrator need Hidely restarted as administrator
- Protected processes can't be changed
- A window opened during a session can be captured for a moment before it is protected
- If Hidely is killed outright, windows stay protected until it is started again

See the [README](https://github.com/CodingRule/Hidely#readme) for how it works, and the [changelog](https://github.com/CodingRule/Hidely/blob/main/CHANGELOG.md) for earlier versions.
