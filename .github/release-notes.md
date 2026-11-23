Hidely asks Windows to leave the windows you choose out of screen capture, while they stay visible to you.

## What's new in 0.2.0

- **Privacy sessions.** Protection now has a clear start and end. Ending a session makes every window Hidely changed capturable again.
- **Meeting profiles.** Save sets of apps such as Work or Client demo. Before a session starts, Hidely shows which apps it will affect and which are not running.
- **New windows are covered.** During a session with a profile, windows those apps open later are protected too. A new window can be captured for a moment first (0 to 139 ms in our measurements).
- **Honest status.** Every window is Unprotected, Applying, Protection applied, Unsupported or Error, in words and with its own icon. "Protection applied" is only shown after Windows confirms the setting.
- **Plain pointer over protected windows.** The mouse pointer stays an arrow there, so a text bar or a hand can't give the window away in a capture.
- **Shortcuts you can change.** If a new combination is taken, the old one keeps working.
- **More from the tray.** Start, end and restore with the window closed.
- **Safer recovery.** If Hidely is killed, the next launch restores your windows and tells you. A reused window can never be changed by old recovery data.
- **Local diagnostics**, shown in full before you save them. Nothing is uploaded.
- **Test page.** It now opens a harmless test window and says plainly what its preview does and does not prove.

### Changed

- Hide and Show are now **Protect** and **Unprotect**.
- <kbd>Win</kbd> + <kbd>Shift</kbd> + <kbd>H</kbd> now starts or ends a session instead of toggling pinned windows.
- Windows that an app already excludes from capture itself are left alone.

## Please read before relying on it

- **Check your meeting app first.** Only two capture methods have been confirmed so far; Zoom, Teams, Meet, Discord and OBS are untested. See the [compatibility record](https://github.com/CodingRule/Hidely/blob/main/docs/COMPATIBILITY.md) and use the [test page](https://codingrule.github.io/Hidely/).
- **Don't use it on games with anti-cheat** such as Roblox. The change is blocked there and it could flag your account.
- It only affects screen capture. It does not conceal a window from someone looking at your monitor, or from software that lists running programs.

## Install

Pick one of the downloads below. Neither needs Rust, Node or any other developer tools.

| File | What it is |
|---|---|
| **Hidely_0.2.0_x64-setup.exe** | Installer. Adds Hidely to the Start menu and installs Microsoft's WebView2 runtime for you if it is missing. Recommended. |
| **Hidely_0.2.0_x64-portable.exe** | Single file, no install. Just run it. Needs the WebView2 runtime, which Windows 11 already has. |
| **hidely-cli_0.2.0_x64.exe** | Optional command-line tool for scripts. |

Requires Windows 10 version 2004 or later, 64-bit. Hidely is Windows-only.

Windows will show a "Windows protected your PC" warning because the downloads are not code-signed yet. Choose **More info**, then **Run anyway**. Some antivirus products may also flag Hidely, because it has to run a small call inside other apps to change their windows. The source is open if you want to check what it does.

## Known limits

- 64-bit apps only
- Apps running as administrator need Hidely to run as administrator too
- Protected processes can't be changed
- If Hidely is killed outright, windows stay protected until it is started again

See the [README](https://github.com/CodingRule/Hidely#readme) for how it works, and the [changelog](https://github.com/CodingRule/Hidely/blob/main/CHANGELOG.md) for earlier versions.
