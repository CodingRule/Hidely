//! Saved settings: meeting profiles and shortcut choices.
//!
//! Everything here is plain data with no Windows calls, so it can be tested anywhere.
//! Profiles store which *programs* to protect, never window titles or contents.

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::WindowInfo;

/// The current layout of the settings file. Bump when the layout changes and add a step
/// to [`Settings::migrate`].
pub const SETTINGS_VERSION: u32 = 2;

pub const DEFAULT_TOGGLE: &str = "Super+Shift+H";
pub const DEFAULT_RESTORE: &str = "Super+Shift+U";

/// Selects the windows of one program.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppRule {
    /// Executable file name, e.g. `spotify.exe`. Compared whole, ignoring case.
    pub exe: String,
    /// If set, the program must also be running from exactly this file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

impl AppRule {
    /// Whole-name matching only: `note.exe` never matches `notepad.exe`.
    pub fn matches(&self, window: &WindowInfo) -> bool {
        if self.exe.is_empty() || window.exe.is_empty() {
            return false;
        }
        same(&self.exe, &window.exe)
            && self.path.as_ref().is_none_or(|path| same(path, &window.path))
    }
}

fn same(a: &str, b: &str) -> bool {
    a.to_lowercase() == b.to_lowercase()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Profile {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub rules: Vec<AppRule>,
}

impl Profile {
    pub fn matches(&self, window: &WindowInfo) -> bool {
        self.rules.iter().any(|rule| rule.matches(window))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Shortcuts {
    /// Starts a privacy session, or ends the one that is running.
    pub toggle: String,
    /// Makes every protected window capturable again.
    pub restore: String,
}

impl Default for Shortcuts {
    fn default() -> Self {
        Self { toggle: DEFAULT_TOGGLE.into(), restore: DEFAULT_RESTORE.into() }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Settings {
    #[serde(default)]
    pub version: u32,
    #[serde(default)]
    pub profiles: Vec<Profile>,
    /// The profile the next session will use. Selecting one does not start anything.
    #[serde(default)]
    pub selected_profile: Option<String>,
    #[serde(default)]
    pub shortcuts: Shortcuts,
    /// Ask GitHub whether a newer version exists when Hidely starts. Off unless the user
    /// turns it on, so Hidely makes no network connection by itself.
    #[serde(default)]
    pub check_updates_at_startup: bool,
    /// The first-run tour was finished or skipped. Files from before version 2 load as
    /// true, so people who already use Hidely never see the tour.
    #[serde(default)]
    pub first_run_done: bool,
    /// Offer a privacy session when a meeting app opens. Hidely only offers; it never
    /// starts a session by itself.
    #[serde(default = "yes")]
    pub meeting_detection: bool,
}

fn yes() -> bool {
    true
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            version: SETTINGS_VERSION,
            profiles: Vec::new(),
            selected_profile: None,
            shortcuts: Shortcuts::default(),
            check_updates_at_startup: false,
            first_run_done: false,
            meeting_detection: true,
        }
    }
}

impl Settings {
    /// Read settings from `path`. A missing file gives defaults. A file that cannot be
    /// understood is kept next to it as `.bad` and defaults are used, with a warning to
    /// show the user, so nothing they saved is silently destroyed.
    pub fn load(path: &Path) -> (Self, Option<String>) {
        let Ok(text) = fs::read_to_string(path) else {
            return (Self::default(), None);
        };
        // Notepad and PowerShell put a byte-order mark at the start of the file.
        match serde_json::from_str::<Self>(text.trim_start_matches('\u{feff}')) {
            Ok(mut settings) => {
                settings.migrate();
                (settings, None)
            }
            Err(error) => {
                let backup = path.with_extension("json.bad");
                let _ = fs::copy(path, &backup);
                let warning = format!(
                    "Your saved settings could not be read ({error}), so Hidely started with \
                     defaults. The old file was kept as {}.",
                    backup.display()
                );
                (Self::default(), Some(warning))
            }
        }
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir).map_err(|e| format!("could not create {}: {e}", dir.display()))?;
        }
        let text = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        // Write to a temporary file first so a crash cannot leave half a settings file.
        let temporary = path.with_extension("json.tmp");
        fs::write(&temporary, text).map_err(|e| format!("could not write settings: {e}"))?;
        fs::rename(&temporary, path).map_err(|e| format!("could not save settings: {e}"))
    }

    /// Bring settings written by an older version up to date.
    fn migrate(&mut self) {
        // Version 0: files from before the version field existed. Same layout as 1.
        for profile in &mut self.profiles {
            for rule in &mut profile.rules {
                rule.exe = rule.exe.trim().to_lowercase();
            }
            profile.rules.retain(|rule| !rule.exe.is_empty());
        }
        if self.shortcuts.toggle.trim().is_empty() {
            self.shortcuts.toggle = DEFAULT_TOGGLE.into();
        }
        if self.shortcuts.restore.trim().is_empty() {
            self.shortcuts.restore = DEFAULT_RESTORE.into();
        }
        if self.selected_profile.as_ref().is_some_and(|id| self.profile(id).is_none()) {
            self.selected_profile = None;
        }
        // Version 1 had no tour. Whoever wrote that file has used Hidely already.
        if self.version < 2 {
            self.first_run_done = true;
        }
        self.version = SETTINGS_VERSION;
    }

    pub fn profile(&self, id: &str) -> Option<&Profile> {
        self.profiles.iter().find(|profile| profile.id == id)
    }

    pub fn selected(&self) -> Option<&Profile> {
        self.selected_profile.as_deref().and_then(|id| self.profile(id))
    }

    /// Create a profile, or replace the name and rules of the one with `id`.
    /// Returns the profile's id.
    pub fn upsert_profile(
        &mut self,
        id: Option<&str>,
        name: &str,
        rules: Vec<AppRule>,
    ) -> Result<String, String> {
        let name = self.valid_name(name, id)?;
        let rules = clean_rules(rules);
        if let Some(id) = id {
            let profile = self
                .profiles
                .iter_mut()
                .find(|profile| profile.id == id)
                .ok_or("That profile no longer exists.")?;
            profile.name = name;
            profile.rules = rules;
            return Ok(id.to_string());
        }
        let id = self.next_id();
        self.profiles.push(Profile { id: id.clone(), name, rules });
        Ok(id)
    }

    pub fn duplicate_profile(&mut self, id: &str) -> Result<String, String> {
        let source = self.profile(id).ok_or("That profile no longer exists.")?.clone();
        let mut name = format!("{} copy", source.name);
        let mut n = 2;
        while self.profiles.iter().any(|profile| same(&profile.name, &name)) {
            name = format!("{} copy {n}", source.name);
            n += 1;
        }
        let new_id = self.next_id();
        self.profiles.push(Profile { id: new_id.clone(), name, rules: source.rules });
        Ok(new_id)
    }

    pub fn delete_profile(&mut self, id: &str) -> Result<(), String> {
        let before = self.profiles.len();
        self.profiles.retain(|profile| profile.id != id);
        if self.profiles.len() == before {
            return Err("That profile no longer exists.".into());
        }
        if self.selected_profile.as_deref() == Some(id) {
            self.selected_profile = None;
        }
        Ok(())
    }

    pub fn select_profile(&mut self, id: Option<&str>) -> Result<(), String> {
        if let Some(id) = id {
            self.profile(id).ok_or("That profile no longer exists.")?;
        }
        self.selected_profile = id.map(str::to_string);
        Ok(())
    }

    fn valid_name(&self, name: &str, own_id: Option<&str>) -> Result<String, String> {
        let name = name.trim();
        if name.is_empty() {
            return Err("Give the profile a name.".into());
        }
        if name.chars().count() > 40 {
            return Err("Profile names can be at most 40 characters.".into());
        }
        let taken = self
            .profiles
            .iter()
            .any(|profile| Some(profile.id.as_str()) != own_id && same(&profile.name, name));
        if taken {
            return Err(format!("You already have a profile called \"{name}\"."));
        }
        Ok(name.to_string())
    }

    fn next_id(&self) -> String {
        let highest = self
            .profiles
            .iter()
            .filter_map(|profile| profile.id.strip_prefix('p')?.parse::<u32>().ok())
            .max()
            .unwrap_or(0);
        format!("p{}", highest + 1)
    }
}

/// Normalise rules and drop empty ones and duplicates.
fn clean_rules(rules: Vec<AppRule>) -> Vec<AppRule> {
    let mut cleaned: Vec<AppRule> = Vec::new();
    for rule in rules {
        let rule = AppRule {
            exe: rule.exe.trim().to_lowercase(),
            path: rule.path.map(|path| path.trim().to_string()).filter(|path| !path.is_empty()),
        };
        if !rule.exe.is_empty() && !cleaned.contains(&rule) {
            cleaned.push(rule);
        }
    }
    cleaned
}

#[cfg(test)]
mod tests {
    use super::*;

    fn window(exe: &str, path: &str) -> WindowInfo {
        WindowInfo {
            hwnd: 1,
            title: "ignored".into(),
            pid: 1,
            process: exe.trim_end_matches(".exe").into(),
            exe: exe.into(),
            path: path.into(),
        }
    }

    fn rule(exe: &str, path: Option<&str>) -> AppRule {
        AppRule { exe: exe.into(), path: path.map(str::to_string) }
    }

    #[test]
    fn rules_match_the_whole_executable_name_only() {
        let notepad = window("notepad.exe", r"C:\Windows\notepad.exe");
        assert!(rule("notepad.exe", None).matches(&notepad));
        assert!(rule("NOTEPAD.EXE", None).matches(&notepad), "case must not matter");
        assert!(!rule("note.exe", None).matches(&notepad), "no prefix matching");
        assert!(!rule("pad.exe", None).matches(&notepad), "no substring matching");
        assert!(!rule("notepad", None).matches(&notepad), "the extension is part of the name");
        assert!(!rule("", None).matches(&notepad));
        assert!(!rule("notepad.exe", None).matches(&window("", "")), "unknown programs never match");
    }

    #[test]
    fn a_path_restriction_narrows_the_match() {
        let real = window("chrome.exe", r"C:\Program Files\Google\Chrome\Application\chrome.exe");
        let other = window("chrome.exe", r"C:\Users\someone\Downloads\chrome.exe");
        let only_real = rule("chrome.exe", Some(r"c:\program files\google\chrome\application\chrome.exe"));
        assert!(only_real.matches(&real));
        assert!(!only_real.matches(&other));
        assert!(rule("chrome.exe", None).matches(&other));
    }

    #[test]
    fn profiles_can_be_created_renamed_duplicated_and_deleted() {
        let mut settings = Settings::default();
        let work = settings.upsert_profile(None, " Work ", vec![rule("Slack.exe", None)]).unwrap();
        assert_eq!(settings.profile(&work).unwrap().name, "Work");
        assert_eq!(settings.profile(&work).unwrap().rules[0].exe, "slack.exe");

        assert!(settings.upsert_profile(None, "work", vec![]).is_err(), "names are unique");
        assert!(settings.upsert_profile(None, "   ", vec![]).is_err(), "names are required");

        settings.upsert_profile(Some(&work), "Client Demo", vec![]).unwrap();
        assert_eq!(settings.profile(&work).unwrap().name, "Client Demo");

        let copy = settings.duplicate_profile(&work).unwrap();
        assert_ne!(copy, work);
        assert_eq!(settings.profile(&copy).unwrap().name, "Client Demo copy");

        settings.select_profile(Some(&work)).unwrap();
        settings.delete_profile(&work).unwrap();
        assert_eq!(settings.selected_profile, None, "a deleted profile cannot stay selected");
        assert!(settings.delete_profile(&work).is_err());
        assert!(settings.select_profile(Some("nope")).is_err());
    }

    #[test]
    fn duplicate_and_blank_rules_are_dropped() {
        let mut settings = Settings::default();
        let id = settings
            .upsert_profile(
                None,
                "Work",
                vec![rule("a.exe", None), rule("A.EXE", None), rule("  ", None), rule("a.exe", Some(""))],
            )
            .unwrap();
        assert_eq!(settings.profile(&id).unwrap().rules, vec![rule("a.exe", None)]);
    }

    #[test]
    fn settings_survive_a_save_and_reload() {
        let dir = std::env::temp_dir().join(format!("hidely-settings-{}", std::process::id()));
        let path = dir.join("settings.json");
        let mut settings = Settings::default();
        let id = settings
            .upsert_profile(None, "Streaming", vec![rule("obs64.exe", Some(r"C:\obs\obs64.exe"))])
            .unwrap();
        settings.select_profile(Some(&id)).unwrap();
        settings.shortcuts.toggle = "Control+Alt+H".into();
        settings.save(&path).unwrap();

        let (loaded, warning) = Settings::load(&path);
        assert_eq!(warning, None);
        assert_eq!(loaded, settings);
        assert!(!fs::read_to_string(&path).unwrap().contains("title"), "titles are never stored");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn older_and_damaged_files_are_handled() {
        let dir = std::env::temp_dir().join(format!("hidely-migrate-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.json");

        // A file from before versions, with a messy rule, a blank shortcut and a
        // selected profile that does not exist.
        fs::write(
            &path,
            r#"{"profiles":[{"id":"p1","name":"Work","rules":[{"exe":" Slack.EXE "},{"exe":""}]}],
                "selected_profile":"gone","shortcuts":{"toggle":"","restore":"Alt+F9"}}"#,
        )
        .unwrap();
        let (settings, warning) = Settings::load(&path);
        assert_eq!(warning, None);
        assert_eq!(settings.version, SETTINGS_VERSION);
        assert_eq!(settings.profiles[0].rules, vec![rule("slack.exe", None)]);
        assert_eq!(settings.selected_profile, None);
        assert_eq!(settings.shortcuts.toggle, DEFAULT_TOGGLE);
        assert_eq!(settings.shortcuts.restore, "Alt+F9");

        // A file saved by Notepad or PowerShell, with a byte-order mark in front.
        fs::write(&path, "\u{feff}{\"version\":1,\"check_updates_at_startup\":true}").unwrap();
        let (settings, warning) = Settings::load(&path);
        assert_eq!(warning, None, "a byte-order mark must not make the file unreadable");
        assert!(settings.check_updates_at_startup);

        fs::write(&path, "{ this is not json").unwrap();
        let (settings, warning) = Settings::load(&path);
        assert_eq!(settings, Settings::default());
        assert!(warning.is_some());
        assert!(path.with_extension("json.bad").exists(), "the unreadable file must be kept");

        assert_eq!(Settings::load(&dir.join("missing.json")), (Settings::default(), None));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_tour_shows_only_on_a_new_install() {
        let dir = std::env::temp_dir().join(format!("hidely-first-run-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.json");

        let (fresh, _) = Settings::load(&dir.join("missing.json"));
        assert!(!fresh.first_run_done, "a new install sees the tour");
        assert!(fresh.meeting_detection, "the meeting offer is on by default");

        for old in [r#"{"version":1}"#, "{}"] {
            fs::write(&path, old).unwrap();
            let (settings, warning) = Settings::load(&path);
            assert_eq!(warning, None);
            assert!(settings.first_run_done, "{old}: existing users skip the tour");
            assert!(settings.meeting_detection, "{old}: a missing field keeps the default");
        }

        // A version 2 file keeps what it says, both ways.
        fs::write(&path, r#"{"version":2,"first_run_done":false,"meeting_detection":false}"#).unwrap();
        let (settings, _) = Settings::load(&path);
        assert!(!settings.first_run_done);
        assert!(!settings.meeting_detection);

        let done = Settings { first_run_done: true, ..Settings::default() };
        done.save(&path).unwrap();
        assert!(Settings::load(&path).0.first_run_done);
        let _ = fs::remove_dir_all(&dir);
    }
}
