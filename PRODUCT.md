# Product

<!-- impeccable:product-schema 1 -->

## Platform

web

## Stack

Two surfaces. The app UI is plain HTML/CSS/JS in `ui/`, embedded in a Tauri 2 desktop app (Rust). The website is static HTML/CSS/JS in `docs/`, served by GitHub Pages at https://codingrule.github.io/Hidely/, with no build step and no third-party requests.

## Users

Three audiences, in no strict order:

- **Streamers**, streaming or recording with OBS or Discord Go Live, afraid of leaking DMs, keys or notifications on stream.
- **People in meetings**: remote workers, students and presenters sharing their screen in Zoom, Teams, Discord or Meet, who don't want a password manager or chat seen.
- **Developers** arriving from GitHub, Hacker News or Reddit, who care how it works and may contribute.

## Product Purpose

Hidely is a free, open-source Windows tray app that asks Windows to leave chosen windows out of screen capture, so they disappear from screen shares, recordings and screenshots while staying fully visible and usable on the user's own monitor. Success on the website: a visitor downloads the app, and stars the GitHub repository.

## Positioning

It works on other apps' windows, not only its own: Hidely makes the one documented Windows call (`SetWindowDisplayAffinity` with `WDA_EXCLUDEFROMCAPTURE`) from inside the target app with a tiny temporary stub (36 bytes on x64, 20 on x86), then reads the setting back and only reports "Protection applied" when Windows confirms it. No driver, no admin rights for ordinary apps.

## Operating Context

Used just before and during screen sharing. Privacy sessions with start and end, meeting profiles of apps, per-window statuses (Unprotected, Applying, Protection applied, Unsupported, Error), global shortcuts (Win+Shift+H session, Win+Shift+U restore), tray control, a capture-excluded HUD, a plain mouse pointer over protected windows, and a Chrome/Edge extension that protects a browser window while a private tab is in front. The website includes a test that plays the visitor's own screen share back to them.

## Capabilities and Constraints

- Windows 10 2004+ and Windows 11, 64-bit Windows; 32-bit and 64-bit apps.
- Apps running as administrator need Hidely restarted as administrator.
- Protected processes are not reachable; games with anti-cheat block it and must not be recommended.
- A newly opened window or a private tab switched to can be captured briefly (0 to 139 ms measured for new windows) before protection.
- Only affects screen capture: not Task Manager, window titles, network traffic, or someone looking at the monitor. Hidely will not grow features that conceal processes or network activity.
- Installer is not code-signed; Windows SmartScreen warns on first run; some antivirus may flag it.

## Brand Commitments

- Name: Hidely. Logo: a ghost (red on dark). Author shown as "Elure"; MIT licence.
- Built on DWMShield by Alexandru Hossu (@ahossu), who discovered the technique; credit him by full name.
- Voice: plain, honest, specific. Never "invisible" or "undetectable"; never claim untested compatibility.
- Credits and limits are visible but brief on the website.

## Evidence on Hand

- Compatibility record: `docs/COMPATIBILITY.md`. Confirmed: GDI BitBlt, DXGI Desktop Duplication, Windows Graphics Capture, PrintWindow; Chrome 157 and Edge 158 `getDisplayMedia` (entire screen); Discord stable entire-screen share (community report, issue 10). Untested: Zoom, Teams, OBS, Firefox, Snipping Tool, Windows 10.
- Demo recording: `docs/images/demo.gif`, `docs/images/demo.mp4` (local only). App screenshot: `docs/images/screenshot.png`. Icon: `src-tauri/icons/`.
- Launched on Product Hunt (24 November 2026); first outside contributor and tester.
- No testimonials, user counts, press or star counts to quote. Do not invent any.

## Product Principles

1. Never report protection that Windows did not confirm.
2. Say what is tested and what is not, with versions.
3. Always put things back: ending a session or quitting restores every window.
4. Nothing leaves the computer; no accounts, analytics or tracking.
5. Privacy for the user's own screen, not evasion of monitoring they agreed to.
