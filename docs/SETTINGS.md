# The settings file

Hidely keeps your profiles, the selected profile and your shortcuts in one JSON file:

```text
%LOCALAPPDATA%\Hidely\settings.json
```

You can back it up, copy it to another machine, or generate it from a script. Hidely reads it
when it starts and writes it when you change a profile or a shortcut, so edit it while Hidely
is closed.

The file format is defined by the `Settings`, `Profile`, `AppRule` and `Shortcuts` structs in
[`crates/hidely-core/src/model.rs`](../crates/hidely-core/src/model.rs).

## Example

Two profiles, the first one selected, and the default shortcuts:

```json
{
  "version": 1,
  "profiles": [
    {
      "id": "p1",
      "name": "Work",
      "rules": [
        { "exe": "slack.exe" },
        { "exe": "teams.exe" }
      ]
    },
    {
      "id": "p2",
      "name": "Streaming",
      "rules": [
        {
          "exe": "obs64.exe",
          "path": "C:\\Program Files\\obs-studio\\bin\\64bit\\obs64.exe"
        }
      ]
    }
  ],
  "selected_profile": "p1",
  "shortcuts": {
    "toggle": "Super+Shift+H",
    "restore": "Super+Shift+U"
  }
}
```

## Fields

| Field | Meaning |
|---|---|
| `version` | Layout of the file. The current value is `1`. Older files are brought up to date when they are loaded. |
| `profiles` | The list of profiles. |
| `profiles[].id` | A unique id such as `p1`. Hidely makes new ones by taking the highest number in use and adding one. |
| `profiles[].name` | The name shown in the sidebar. The app keeps names unique, ignoring case, and at most 40 characters when you edit them; the file is not checked for this on load. |
| `profiles[].rules` | The programs this profile protects. |
| `rules[].exe` | The executable file name, such as `slack.exe`. Hidely trims it and lowercases it. |
| `rules[].path` | Optional. If set, the program must also be running from exactly this file. |
| `selected_profile` | The `id` of the profile the next session will use, or `null`. Selecting a profile does not start a session. If the id matches no profile, Hidely clears it when it loads the file. |
| `shortcuts.toggle` | Starts a privacy session, or ends the one that is running. |
| `shortcuts.restore` | Makes every protected window capturable again. |

If the whole `shortcuts` object is left out, or a shortcut is an empty string, Hidely uses the
default (`Super+Shift+H` for `toggle`, `Super+Shift+U` for `restore`). If `shortcuts` is present,
it needs both `toggle` and `restore`; with one of them missing, the file counts as unreadable
(see below).

## How rules match

A rule matches a window when the window's executable file name equals `exe`. The whole name is
compared and case does not matter, so `notepad.exe` and `NOTEPAD.EXE` are the same, while
`note.exe`, `pad.exe` and `notepad` (without the extension) do not match `notepad.exe`.

If a rule has a `path`, the program's full path must also be equal to it, again ignoring case.
That tells a real `chrome.exe` apart from another program that happens to use the same file
name. Rules with an empty `exe` are dropped.

## Shortcut format

A shortcut is a list of modifiers followed by one key, joined with `+`, for example
`Super+Shift+H` or `Control+Alt+F9`. The modifiers Hidely's own shortcut dialog writes are
`Control`, `Alt`, `Shift` and `Super`. `Super` is the Windows key, and Hidely's window shows it
as Win.

A combination needs `Control`, `Alt` or `Super`. Hidely refuses one with only `Shift`, or with
no modifier, so that the shortcut cannot fire while you type.

## A file Hidely cannot read

If `settings.json` is not valid or does not match this layout, Hidely does not overwrite it
silently. It copies the file to `settings.json.bad` in the same folder (replacing an earlier `.bad` copy), starts with default
settings, and shows a warning with the reason.

## What is not stored

Window titles and window contents are never written to the settings file. A profile holds only
program names and, optionally, their paths.

## `hidden.txt`

Next to `settings.json`, a `hidden.txt` appears while Hidely has windows protected, and is removed when none are. It is a recovery record for the windows Hidely has
protected, kept so they can be made capturable again if Hidely is closed unexpectedly. Each
line holds a window handle, a process id and a process start time, and nothing else: no
titles, paths or content. You do not need to edit or back it up.
