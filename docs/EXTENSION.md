# Hidely for browsers

A Chrome and Edge extension that keeps a browser window out of screen capture while a
private tab is showing. It needs the Hidely app, version 0.3.0 or later, running on the same
PC.

## What it does, exactly

Screen capture can only leave out whole windows, not a single tab. So the extension watches
which tab is in front in each browser window:

- When that tab is one you marked private, or is on a site you marked private, it asks
  Hidely to protect that **whole browser window**. Viewers see the window disappear from
  your share.
- When you switch to an ordinary tab, the window is released and shows up again.

You mark things private from the extension's toolbar button:

- **Keep this tab private** lasts until you close the tab or the browser.
- **Always keep _site_ private** applies to every tab on that site, including subdomains,
  and is remembered.

The toolbar button shows **on** while the window is protected and **!** when something went
wrong, and the popup says what in words.

## Setting it up

1. In Hidely, open **Settings** and switch on **Connect Chrome and Edge**.
2. Install the extension from the
   [Chrome Web Store](https://chromewebstore.google.com/detail/hidely-for-browsers/kifbmljgdnggophdknoliofdafghkgcg).
   In Edge, the same page works once you press **Allow extensions from other stores**.
   Or load it by hand:
   1. Download `hidely-extension_x.y.z.zip` from the
      [latest release](https://github.com/CodingRule/Hidely/releases/latest) and unzip it
      somewhere it can stay. (Or use the `extension/` folder of this repository.)
   2. Open `chrome://extensions` or `edge://extensions` and switch on **Developer mode**.
   3. Press **Load unpacked** and choose the unzipped folder.
3. Click the Hidely button in the toolbar. It should say **Connected**.

If it says **Not connected**, step 1 has not been done, or Hidely has moved since: switch
the setting off and on again. If it says **Hidely is closed**, start Hidely.

## Limits you should know about

- **The whole window goes, not just the tab.** If you want the rest of your browser to stay
  in the share, keep private tabs in a separate window.
- **A private tab can be seen briefly.** When you switch to one, it is on screen for a
  moment before Hidely protects the window (57 ms in our one measurement, and longer on a
  busy machine). Switch to it before you need it, not while everyone is watching.
- **Hidely has to be running.** If it is closed, nothing is protected, and the extension
  says so instead of pretending.
- **Ending the session in Hidely releases browser windows too.** The extension protects
  the window again the next time the tab changes.
- **Only Edge has been tested.** Chrome uses the same extension system and should behave
  the same, but it has not been checked. Brave, Vivaldi and Opera are allowed and untested.
  Firefox is not supported.
- **Private (InPrivate/Incognito) windows** are only covered if you allow the extension to
  run there in the browser's extension settings.

## Privacy

- The extension asks for three permissions: **tabs** (to know which tab is in front and
  what site it is on), **storage** (to remember your private sites), and
  **nativeMessaging** (to talk to the Hidely app).
- It makes no network requests. The only thing it talks to is the Hidely app on your PC.
- What it sends Hidely is the front tab's title and the window's position, so Hidely can
  work out which window is meant. Hidely does not store either.
- Your list of private sites stays in the browser's own extension storage.

## How it is put together

```text
extension (background.js)  --native messaging-->  Hidely.exe started by the browser
                                                        |
                                                        | "protect window 0x1234"
                                                        v
                                                  the running Hidely app
```

- Connecting in Settings writes `%LOCALAPPDATA%\Hidely\browser-host.json`, which names
  `Hidely.exe` and the one extension allowed to use it, and points a per-user registry key
  for Chrome and for Edge at that file. Switching the setting off removes all three.
- When the browser starts `Hidely.exe` for the extension, Hidely shows no window. It finds
  the browser window the extension means by its position on screen, using the tab title
  only to break ties, and passes the request to the running app.
- The app is the only thing that changes windows. It accepts these requests for browser
  windows only, and the extension can only release windows it protected itself. A window
  you protected by hand, or through a profile, stays protected.
- Before answering, Hidely reads the window's capture setting back from Windows, so
  "protected" in the popup means Windows confirmed it.

## Tested

On Windows 11 25H2 (26200.9974) with Microsoft Edge 158.0.4370.33, on 2026-11-24, driving
the real extension in a throwaway browser profile and reading the window's capture setting
from Windows:

| Situation | Result |
|---|---|
| Ordinary tab in front | Window captured normally |
| Tab marked private, but in the background | Window still captured normally |
| Private tab brought to the front | Window excluded from capture; popup state "protected" |
| Back to the ordinary tab | Window captured normally again |
| Site marked private, tab on another site in front | Not protected |
| Site marked private, tab on that site in front | Protected |
| Session ended in Hidely | Window capturable |
| Hidely closed | Extension reports "not running", shows no protection, and gives a reason |

Not tested: Chrome and other Chromium browsers, the popup's buttons clicked by hand, more
than one browser window at once, and private windows.
