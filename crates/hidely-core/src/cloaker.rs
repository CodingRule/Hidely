use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use crate::windows::process_start_time;
use crate::{
    display_affinity, inject, is_window, owner_pid, CloakError, WDA_EXCLUDEFROMCAPTURE, WDA_NONE,
};

/// What changing a window's capture setting actually did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// We changed the setting, and reading it back confirms the new value.
    Changed { pid: u32, started: u64 },
    /// The app already excludes this window from capture itself. Nothing was changed,
    /// and it is not ours to undo later.
    AlreadyProtected,
}

/// Change one window's capture setting and confirm that it took.
///
/// `expected_pid`, when given, is the process that owned the window when the request was
/// made. Window handles are reused, so the owner is checked again here and the window is
/// left alone if it changed.
///
/// This touches no bookkeeping, so it can run without holding any lock.
pub fn apply(hwnd: isize, hide: bool, expected_pid: Option<u32>) -> Result<Outcome, CloakError> {
    if !is_window(hwnd) {
        return Err(CloakError::WindowGone);
    }
    let pid = owner_pid(hwnd).ok_or(CloakError::WindowGone)?;
    if expected_pid.is_some_and(|expected| expected != pid) {
        return Err(CloakError::OwnerChanged);
    }
    if hide && display_affinity(hwnd).is_some_and(|current| current != WDA_NONE) {
        return Ok(Outcome::AlreadyProtected);
    }

    let wanted = if hide { WDA_EXCLUDEFROMCAPTURE } else { WDA_NONE };
    if pid == std::process::id() {
        inject::set_own(hwnd, wanted)?;
    } else {
        inject::set_remote(hwnd, pid, wanted)?;
    }
    // A request being accepted is not proof. Ask Windows what the setting is now.
    if display_affinity(hwnd) != Some(wanted) {
        return Err(if is_window(hwnd) { CloakError::NotVerified } else { CloakError::WindowGone });
    }
    Ok(Outcome::Changed { pid, started: process_start_time(pid).unwrap_or(0) })
}

/// The result of trying to make everything capturable again.
#[derive(Debug, Default, Clone)]
pub struct RestoreReport {
    pub restored: usize,
    /// Windows that are still hidden, with the reason. They stay in the journal.
    pub failed: Vec<(isize, CloakError)>,
}

#[derive(Debug, Clone, Copy)]
struct Tracked {
    pid: u32,
    /// Process start time, or 0 for records written before this was stored.
    started: u64,
}

/// Remembers which windows Hidely hid, so they can always be restored.
///
/// Every change is written to a small journal file holding only window handles, process
/// ids and process start times: no titles, paths or content. If Hidely is killed before it
/// can restore anything, the next `Cloaker` reads the journal and
/// [`Cloaker::restore_all`] un-hides the windows that are still the same windows.
pub struct Cloaker {
    hidden: HashMap<isize, Tracked>,
    journal: Option<PathBuf>,
}

impl Cloaker {
    /// A cloaker that keeps its journal in `%LOCALAPPDATA%\Hidely\hidden.txt`.
    pub fn new() -> Self {
        Self::with_journal(data_dir().map(|dir| dir.join("hidden.txt")))
    }

    pub fn with_journal(journal: Option<PathBuf>) -> Self {
        let mut cloaker = Self { hidden: HashMap::new(), journal };
        cloaker.load();
        cloaker
    }

    /// Hide a window and remember it. Blocking convenience over [`apply`] + [`Self::record`].
    pub fn hide(&mut self, hwnd: isize) -> Result<Outcome, CloakError> {
        let outcome = apply(hwnd, true, None)?;
        self.record(hwnd, outcome);
        Ok(outcome)
    }

    /// Make a window capturable again and forget it.
    pub fn show(&mut self, hwnd: isize) -> Result<(), CloakError> {
        let result = apply(hwnd, false, None).map(|_| ());
        if matches!(result, Ok(()) | Err(CloakError::WindowGone)) {
            self.forget(hwnd);
        }
        result
    }

    /// Note that we hid `hwnd`. Windows that were already protected are not ours to track.
    pub fn record(&mut self, hwnd: isize, outcome: Outcome) {
        if let Outcome::Changed { pid, started } = outcome {
            self.hidden.insert(hwnd, Tracked { pid, started });
            self.save();
        }
    }

    pub fn forget(&mut self, hwnd: isize) {
        if self.hidden.remove(&hwnd).is_some() {
            self.save();
        }
    }

    /// The process that owned `hwnd` when we hid it, if we did.
    pub fn owner_of(&self, hwnd: isize) -> Option<u32> {
        self.hidden.get(&hwnd).map(|tracked| tracked.pid)
    }

    /// Whether Hidely hid this window and it is still the same window.
    pub fn is_hidden(&self, hwnd: isize) -> bool {
        self.hidden.get(&hwnd).is_some_and(|tracked| still_same_window(hwnd, tracked))
    }

    pub fn hidden_count(&self) -> usize {
        self.hidden.keys().filter(|&&hwnd| self.is_hidden(hwnd)).count()
    }

    /// Forget windows that have been closed, or whose handle now belongs to something else.
    pub fn prune(&mut self) {
        let before = self.hidden.len();
        self.hidden.retain(|&hwnd, tracked| still_same_window(hwnd, tracked));
        if self.hidden.len() != before {
            self.save();
        }
    }

    /// Un-hide everything Hidely hid. Windows that cannot be restored stay tracked (and in
    /// the journal) so a later call, or the next launch, can try again.
    pub fn restore_all(&mut self) -> RestoreReport {
        self.prune();
        let mut report = RestoreReport::default();
        let targets: Vec<(isize, u32)> =
            self.hidden.iter().map(|(&hwnd, tracked)| (hwnd, tracked.pid)).collect();
        for (hwnd, pid) in targets {
            match apply(hwnd, false, Some(pid)) {
                Ok(_) | Err(CloakError::WindowGone) | Err(CloakError::OwnerChanged) => {
                    self.hidden.remove(&hwnd);
                    report.restored += 1;
                }
                Err(error) => report.failed.push((hwnd, error)),
            }
        }
        self.save();
        report
    }

    fn load(&mut self) {
        let Some(text) = self.journal.as_ref().and_then(|path| fs::read_to_string(path).ok())
        else {
            return;
        };
        for (hwnd, tracked) in text.lines().filter_map(parse_journal_line) {
            // Only keep records that still describe the same window of the same process.
            if still_same_window(hwnd, &tracked) {
                self.hidden.insert(hwnd, tracked);
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
        let text: String = self
            .hidden
            .iter()
            .map(|(hwnd, tracked)| format!("{hwnd} {} {}\n", tracked.pid, tracked.started))
            .collect();
        let _ = fs::write(path, text);
    }
}

impl Default for Cloaker {
    fn default() -> Self {
        Self::new()
    }
}

/// Where Hidely keeps its local files: `%LOCALAPPDATA%\Hidely`.
pub fn data_dir() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA").map(|dir| PathBuf::from(dir).join("Hidely"))
}

/// `hwnd pid started`; older journals have only `hwnd pid`.
fn parse_journal_line(line: &str) -> Option<(isize, Tracked)> {
    let mut parts = line.split_whitespace();
    let hwnd = parts.next()?.parse().ok()?;
    let pid = parts.next()?.parse().ok()?;
    let started = parts.next().and_then(|value| value.parse().ok()).unwrap_or(0);
    Some((hwnd, Tracked { pid, started }))
}

/// A handle value can be reused by a new window, and a process id by a new process. A
/// record is only trusted if the window still belongs to the same run of the same process.
fn still_same_window(hwnd: isize, tracked: &Tracked) -> bool {
    owner_pid(hwnd) == Some(tracked.pid)
        && (tracked.started == 0 || process_start_time(tracked.pid) == Some(tracked.started))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn journal_lines_parse_with_and_without_start_time() {
        let (hwnd, tracked) = parse_journal_line("1234 42 133760000000000000").unwrap();
        assert_eq!((hwnd, tracked.pid, tracked.started), (1234, 42, 133_760_000_000_000_000));
        let (_, legacy) = parse_journal_line("1234 42").unwrap();
        assert_eq!(legacy.started, 0);
        assert!(parse_journal_line("not a record").is_none());
        assert!(parse_journal_line("").is_none());
    }

    #[test]
    fn stale_records_are_dropped_and_never_acted_on() {
        let dir = std::env::temp_dir().join(format!("hidely-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let journal = dir.join("stale.txt");

        // A real window of another process, paired with the wrong owner and start time:
        // exactly what a reused handle looks like after a crash.
        let Some(window) = crate::list_windows(Some(std::process::id())).into_iter().next() else {
            return; // no other windows on this machine (headless CI)
        };
        let before = display_affinity(window.hwnd);
        fs::write(&journal, format!("{} {} 1\n{} 4294967295 1\n", window.hwnd, window.pid, window.hwnd))
            .unwrap();

        let mut cloaker = Cloaker::with_journal(Some(journal.clone()));
        assert_eq!(cloaker.hidden_count(), 0, "stale records must not be loaded");
        let report = cloaker.restore_all();
        assert_eq!(report.restored, 0);
        assert_eq!(display_affinity(window.hwnd), before, "an unrelated window was modified");
        let _ = fs::remove_dir_all(&dir);
    }
}
