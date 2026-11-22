<p align="center">
  <img src="src-tauri/icons/128x128.png" width="96" height="96" alt="Hidely logo: a red ghost" />
</p>

<h1 align="center">Hidely</h1>

<p align="center">
  Hide any window from screen sharing, recordings and screenshots.<br />
  You still see it. Your viewers don't.
</p>

<p align="center">
  <a href="https://github.com/CodingRule/Hidely/actions/workflows/ci.yml"><img src="https://github.com/CodingRule/Hidely/actions/workflows/ci.yml/badge.svg" alt="CI status" /></a>
  <img src="https://img.shields.io/badge/platform-Windows%2010%202004%2B%20%C2%B7%20x64-blue" alt="Windows 10 2004 or later, x64" />
  <img src="https://img.shields.io/badge/built%20with-Rust%20%2B%20Tauri%202-orange" alt="Built with Rust and Tauri 2" />
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-MIT-green" alt="MIT license" /></a>
</p>

<p align="center">
  <a href="https://github.com/CodingRule/Hidely/releases/latest"><strong>Download</strong></a> ·
  <a href="https://codingrule.github.io/Hidely/"><strong>Test page</strong></a> ·
  <a href="#how-it-works">How it works</a> ·
  <a href="#roadmap">Roadmap</a>
</p>

<p align="center">
  <strong>Built on <a href="https://github.com/ahossu/DWMShield">DWMShield</a> by Alexandru Hossu (<a href="https://github.com/ahossu">@ahossu</a>).</strong><br />
  Alexandru Hossu discovered the technique this project grew out of, and his kernel driver is<br />
  included in <a href="driver/"><code>driver/</code></a> with permission, under its own MIT licence.
</p>

<p align="center">
  <img src="docs/images/screenshot.png" width="576" alt="The Hidely window: a list of open windows, two of them marked as hidden from capture, each with Pin and Hide buttons" />
</p>

You are sharing your screen in a meeting and need to check your password manager, a private chat
or a terminal. Normally everyone watching sees it too. With Hidely, you pick the windows that are
private, press a hotkey, and they disappear from Zoom, Teams, Meet, OBS, Discord, the Snipping
Tool and Print Screen, while staying fully visible and usable on your own monitor.

Hidely lives in the system tray, needs no driver and no administrator rights for ordinary apps,
and always puts things back the way it found them.

## Features

- **Window list.** Every open window with its icon and title, and a Hide button next to each.
- **Pin and hotkey.** Pin the windows you care about, then press <kbd>Win</kbd> + <kbd>Shift</kbd> + <kbd>H</kbd> to hide or show them all at once.
- **Panic key.** <kbd>Win</kbd> + <kbd>Shift</kbd> + <kbd>U</kbd> un-hides everything instantly.
- **Tray icon.** The ghost turns red while anything is hidden, so you always know your state.
- **Private HUD.** A small on-screen confirmation that is itself excluded from capture, so viewers never see it.
- **Always restores.** Hidden windows are un-hidden when Hidely quits or crashes. If it is killed outright, the next launch restores them from a journal.
- **Light and dark.** Follows your Windows theme.
- **CLI included.** `hidely-cli hide notepad` for scripts and automation.

## Download

**[Download the latest release](https://github.com/CodingRule/Hidely/releases/latest)** for
Windows 10 (version 2004 or later) and Windows 11, 64-bit. No Rust, Node or other developer tools
needed.

- **`Hidely_x.y.z_x64-setup.exe`** is the installer. It sets up everything Hidely needs,
  including Microsoft's WebView2 runtime if your PC doesn't have it.
- **`Hidely_x.y.z_x64-portable.exe`** is a single file you can run without installing.

Hidely is Windows-only. It is built on a Windows capture-exclusion feature that macOS and Linux
don't offer, so there is nothing to build or run on those systems.

The downloads are not code-signed yet, so Windows shows a "Windows protected your PC" warning the
first time. Choose **More info**, then **Run anyway**.

Then open the **[test page](https://codingrule.github.io/Hidely/)**, press **Start screen share**,
choose **Entire screen**, and hide a window in Hidely. It vanishes from the preview and stays on
your monitor. Nothing from the test page is recorded or uploaded.

## Build from source

### 1. Build and run

You need [Rust](https://rustup.rs) (stable, MSVC toolchain), [Node.js](https://nodejs.org) 18+ and
the [WebView2 runtime](https://developer.microsoft.com/microsoft-edge/webview2/) (already present
on Windows 11).

```powershell
git clone https://github.com/CodingRule/Hidely
cd Hidely
npm install
npm run dev
```

`npm run build` produces an installer in `target/release/bundle/nsis/`.

### 2. Run the test page locally

The test page lives in [`docs/index.html`](docs/index.html). Screen sharing needs a secure
origin, so serve it instead of opening the file:

```powershell
npx http-server docs -p 4173
```

Then open <http://localhost:4173>.

### 3. Cut a release

Bump the version in `src-tauri/tauri.conf.json` and `Cargo.toml`, update
`.github/release-notes.md`, then push a tag such as `v0.2.0`. The release workflow builds the
installer and publishes it.

## Using Hidely

| I want to… | Do this |
|---|---|
| Hide one window right now | Click **Hide** next to it |
| Hide a set of windows with one key | Click **Pin** on each, then press <kbd>Win</kbd> + <kbd>Shift</kbd> + <kbd>H</kbd> |
| Bring everything back | Press <kbd>Win</kbd> + <kbd>Shift</kbd> + <kbd>U</kbd>, or click **Show everything** |
| Keep it out of the way | Close the window. Hidely stays in the tray. |
| Quit | Right-click the tray icon and choose **Quit** |

### Command line

```text
hidely-cli list                 List windows (* = excluded from capture)
hidely-cli hide <target>        Hide matching windows from capture
hidely-cli show <target>        Make matching windows capturable again
hidely-cli restore              Un-hide everything Hidely has hidden
```

`<target>` is a window handle such as `0x1A2B3C` or a process name such as `notepad`.
Build it with `cargo build --release -p hidely-cli`.

## How it works

Windows has a documented switch for this:
[`SetWindowDisplayAffinity(hwnd, WDA_EXCLUDEFROMCAPTURE)`](https://learn.microsoft.com/windows/win32/api/winuser/nf-winuser-setwindowdisplayaffinity).
The compositor then leaves that window out of every capture. Password managers use it on their
own windows.

The catch is that Windows only accepts the call from the process that owns the window. So for
another app's window, Hidely makes the call from inside that app:

1. It writes a 36-byte stub into the target process that does one thing: call
   `SetWindowDisplayAffinity` for that window.
2. It runs the stub on a short-lived thread and reads back whether Windows accepted it.
3. It frees the memory again. No DLL stays loaded and nothing in the app is patched.

The whole mechanism is about 150 lines in
[`crates/hidely-core/src/inject.rs`](crates/hidely-core/src/inject.rs).

```text
ui/                  Front end (plain HTML, CSS, JS)
src-tauri/           Tauri 2 app: tray, hotkeys, HUD, commands
crates/hidely-core/  Window enumeration and capture exclusion (Win32)
crates/hidely-cli/   Command-line front end
docs/                Screen-share test page, driver notes
driver/              DWMShield kernel driver, unmodified (see below)
```

## Limits you should know about

- **64-bit apps only.** 32-bit apps are reported as unsupported.
- **Elevated apps need an elevated Hidely.** To hide a window that runs as administrator, run Hidely as administrator too.
- **Protected processes can't be hidden** in user mode. That is what the optional kernel driver is for.
- **Antivirus may object.** Writing into another process is also what malware does, so some security products flag or block it. The code is short and open so you can check what it does.
- **Hidden means hidden from capture, nothing more.** Hidely does not hide a window from someone looking at your monitor, from software that reads window titles, or from an administrator of the machine.
- **Hotkeys can be taken.** If another app already owns a shortcut, Hidely says so at the bottom of its window.

## Roadmap

- [x] **v0.1** Tray app, window list, hide and show, pins, hotkeys, panic key, restore on exit
- [ ] **v0.2** Profiles ("Work", "Stream") and rules that auto-hide by process name
- [ ] **v0.2** 32-bit app support and configurable hotkeys
- [ ] **v0.3** Optional kernel driver for protected windows, see [docs/DRIVER.md](docs/DRIVER.md)
- [ ] **v0.4** Browser extension for per-tab hiding
- [ ] **v1.0** Signed installer and auto-update

## Contributing

Issues and pull requests are welcome. [CONTRIBUTING.md](CONTRIBUTING.md) explains how to set up,
what to check before a pull request, and where things live. Changes are listed in the
[changelog](CHANGELOG.md).

Found a security problem? Please report it privately as described in [SECURITY.md](SECURITY.md).

## Responsible use

Hidely is for your own privacy on your own computer: keeping your password manager, messages and
notes out of a screen share. Do not use it to get around monitoring you have agreed to, such as
exam proctoring or workplace policy, or on machines you are not allowed to modify.

## Credits

- [DWMShield](https://github.com/ahossu/DWMShield) by Alexandru Hossu
  ([@ahossu](https://github.com/ahossu)), who discovered the technique and whose kernel driver is included in [`driver/`](driver/) with
  permission, under its own MIT licence. Hidely exists because of his work.
- [Tauri](https://tauri.app) for the app shell.

## License

[MIT](LICENSE) © 2026 Elure
