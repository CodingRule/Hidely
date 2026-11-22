# Contributing to Hidely

Thanks for wanting to help. Bug reports, ideas and pull requests are all welcome.

## Reporting a bug

Open an [issue](https://github.com/CodingRule/Hidely/issues/new/choose) and include:

- your Windows version (press <kbd>Win</kbd> + <kbd>R</kbd>, type `winver`)
- the app you tried to hide, and whether it runs as administrator
- the message Hidely showed at the bottom of its window, if any
- which capture tool still showed the window (Zoom, OBS, the test page, …)

Security problems go through [SECURITY.md](SECURITY.md), not public issues.

## Setting up

You need Windows 10 2004 or later, [Rust](https://rustup.rs) (stable, MSVC toolchain) and
[Node.js](https://nodejs.org) 18+.

```powershell
git clone https://github.com/CodingRule/Hidely
cd Hidely
npm install
npm run dev
```

## Where things live

| Path | What it is |
|---|---|
| `crates/hidely-core/` | All Win32 code: finding windows and changing their capture setting |
| `crates/hidely-cli/` | Command-line front end over the core crate |
| `src-tauri/` | The desktop app: tray, hotkeys, HUD and the commands the UI calls |
| `ui/` | The app's front end in plain HTML, CSS and JavaScript, with no build step |
| `docs/` | The screen-share test page (served by GitHub Pages) and driver notes |
| `driver/` | Upstream DWMShield sources. Please don't modify these. |

## Before you open a pull request

```powershell
cargo test --workspace
cargo build --workspace
```

Then check it for real:

1. Run the app and open the [test page](https://codingrule.github.io/Hidely/).
2. Share your entire screen and hide a window. It must disappear from the preview.
3. Press <kbd>Win</kbd> + <kbd>Shift</kbd> + <kbd>U</kbd>. It must come back.
4. Quit Hidely while something is hidden. It must come back then too.

Step 4 matters most. Hidely must never leave a window hidden after it is gone, so any change
that touches hiding needs to keep the restore path working.

## Guidelines

- Keep pull requests focused on one change.
- All `unsafe` and Win32 calls belong in `hidely-core`, so the app itself stays safe Rust.
- Explain the "why" in comments where the code can't.
- Features that only make sense for evading monitoring you have agreed to (exam proctoring,
  workplace policy) are out of scope. See the README's note on responsible use.

## Licence

By contributing you agree that your work is released under the [MIT licence](LICENSE).
