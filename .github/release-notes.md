Hidely hides any window from screen sharing, recordings and screenshots while it stays visible to you.

## What's new in 0.1.2

- **New look.** A borderless, rounded, see-through black window with its own title bar.
- **Search and filters.** Find a window by name, or switch between All, Pinned and Hidden.
- **Clearer status.** A table with App and Status columns, and a pill in the title bar showing how many windows are hidden.
- The light theme is gone for now; Hidely is dark only.

## Install

Pick one of the downloads below. Neither needs Rust, Node or any other developer tools.

| File | What it is |
|---|---|
| **Hidely_0.1.2_x64-setup.exe** | Installer. Adds Hidely to the Start menu and installs Microsoft's WebView2 runtime for you if it is missing. Recommended. |
| **Hidely_0.1.2_x64-portable.exe** | Single file, no install. Just run it. Needs the WebView2 runtime, which Windows 11 already has. |
| **hidely-cli_0.1.2_x64.exe** | Optional command-line tool for scripts. |

Requires Windows 10 version 2004 or later, 64-bit. Hidely is Windows-only: it relies on a Windows feature that macOS and Linux don't have.

Windows will show a "Windows protected your PC" warning because the downloads are not code-signed yet. Choose **More info**, then **Run anyway**. Some antivirus products may also flag Hidely, because it has to run a small call inside other apps to hide their windows. The source is open if you want to check what it does.

Then open the [test page](https://codingrule.github.io/Hidely/), share your screen with it, and hide a window to see what your viewers see.

## Known limits

- 64-bit apps only
- Apps running as administrator need Hidely to run as administrator too
- Protected processes can't be hidden
- Hotkeys are fixed for now

See the [README](https://github.com/CodingRule/Hidely#readme) for how it works and what's planned, and the [changelog](https://github.com/CodingRule/Hidely/blob/main/CHANGELOG.md) for earlier versions.
