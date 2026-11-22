use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use crate::{inject, is_window, owner_pid, CloakError, WDA_EXCLUDEFROMCAPTURE, WDA_NONE};

/// Hides windows from capture and remembers which ones, so they can always be restored.
///
/// Every change is written to a small journal file. If the process is killed before it
/// can restore anything, the next `Cloaker` picks those windows up again and
/// [`Cloaker::restore_all`] un-hides them.
pub struct Cloaker {
    /// hwnd -> owning pid (guards against the handle value being reused).
    hidden: HashMap<isize, u32>,
    journal: Option<PathBuf>,
}

impl Cloaker {
    /// A cloaker that keeps its journal in `%LOCALAPPDATA%\Hidely\hidden.txt`.
    pub fn new() -> Self {
        let journal = std::env::var_os("LOCALAPPDATA")
            .map(|dir| PathBuf::from(dir).join("Hidely").join("hidden.txt"));
        Self::with_journal(journal)
    }

    pub fn with_journal(journal: Option<PathBuf>) -> Self {
        let mut cloaker = Self { hidden: HashMap::new(), journal };
        cloaker.load();
        cloaker
    }

    pub fn hide(&mut self, hwnd: isize) -> Result<(), CloakError> {
        let pid = owner_pid(hwnd).ok_or(CloakError::WindowGone)?;
        set_affinity(hwnd, pid, WDA_EXCLUDEFROMCAPTURE)?;
        self.hidden.insert(hwnd, pid);
        self.save();
        Ok(())
    }

    pub fn show(&mut self, hwnd: isize) -> Result<(), CloakError> {
        let result = match owner_pid(hwnd) {
            Some(pid) => set_affinity(hwnd, pid, WDA_NONE),
            None => Err(CloakError::WindowGone),
        };
        if matches!(result, Ok(()) | Err(CloakError::WindowGone)) {
            self.hidden.remove(&hwnd);
            self.save();
        }
        result
    }

    /// Whether Hidely hid this window (and it still exists).
    pub fn is_hidden(&self, hwnd: isize) -> bool {
        self.hidden.get(&hwnd).is_some_and(|&pid| owner_pid(hwnd) == Some(pid))
    }

    pub fn hidden_count(&self) -> usize {
        self.hidden.keys().filter(|&&hwnd| self.is_hidden(hwnd)).count()
    }

    /// Forget windows that have been closed since they were hidden.
    pub fn prune(&mut self) {
        let before = self.hidden.len();
        self.hidden.retain(|&hwnd, &mut pid| owner_pid(hwnd) == Some(pid));
        if self.hidden.len() != before {
            self.save();
        }
    }

    /// Un-hide everything Hidely hid. Returns how many windows were restored;
    /// windows that could not be restored stay tracked so a later call can retry.
    pub fn restore_all(&mut self) -> usize {
        self.prune();
        let mut restored = 0;
        let targets: Vec<(isize, u32)> = self.hidden.iter().map(|(&h, &p)| (h, p)).collect();
        for (hwnd, pid) in targets {
            if set_affinity(hwnd, pid, WDA_NONE).is_ok() || !is_window(hwnd) {
                self.hidden.remove(&hwnd);
                restored += 1;
            }
        }
        self.save();
        restored
    }

    fn load(&mut self) {
        let Some(text) = self.journal.as_ref().and_then(|path| fs::read_to_string(path).ok())
        else {
            return;
        };
        for line in text.lines() {
            let mut parts = line.split_whitespace();
            let (Some(hwnd), Some(pid)) = (parts.next(), parts.next()) else { continue };
            let (Ok(hwnd), Ok(pid)) = (hwnd.parse::<isize>(), pid.parse::<u32>()) else { continue };
            if owner_pid(hwnd) == Some(pid) {
                self.hidden.insert(hwnd, pid);
            }
        }
    }

    fn save(&self) {
        let Some(path) = &self.journal else { return };
        if self.hidden.is_empty() {
            let _ = fs::remove_file(path);
            return;
        }
        if let Some(dir) = path.parent() {
            let _ = fs::create_dir_all(dir);
        }
        let text: String =
            self.hidden.iter().map(|(hwnd, pid)| format!("{hwnd} {pid}\n")).collect();
        let _ = fs::write(path, text);
    }
}

impl Default for Cloaker {
    fn default() -> Self {
        Self::new()
    }
}

fn set_affinity(hwnd: isize, pid: u32, affinity: u32) -> Result<(), CloakError> {
    if !is_window(hwnd) {
        return Err(CloakError::WindowGone);
    }
    if pid == std::process::id() {
        inject::set_own(hwnd, affinity)
    } else {
        inject::set_remote(hwnd, pid, affinity)
    }
}
