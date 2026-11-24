//! The one place that knows what Hidely is doing: which windows are protected, whether a
//! privacy session is running, and what happened to every request.
//!
//! The tray, the window and the CLI all read and change this model, so they cannot
//! disagree with each other.

use std::collections::{HashMap, HashSet, VecDeque};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;

use crate::cloaker::{apply, Outcome, RestoreReport};
use crate::model::AppRule;
use crate::windows::process_identity;
use crate::{display_affinity, owner_pid, CloakError, Cloaker, WindowInfo, WDA_NONE};

const LOG_CAPACITY: usize = 200;

/// How often a running session looks for newly opened windows. A new window can be
/// captured for up to about this long, plus the time the change itself takes.
pub const WATCH_INTERVAL_MS: u64 = 150;

/// Where a window stands. `Applied` means Windows confirmed the setting when asked; it
/// does not mean any particular capture tool has been checked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Unprotected,
    Applying,
    Applied,
    Unsupported,
    Error,
}

/// Who asked for a change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// The user clicked on this window, or pinned it.
    Manual,
    /// A profile rule matched it during a session.
    Rule,
    /// The browser extension asked, because a private tab is showing in that window.
    Extension,
}

impl Source {
    fn label(self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::Rule => "rule",
            Self::Extension => "extension",
        }
    }
}

#[derive(Debug, Clone)]
struct WindowState {
    status: Status,
    detail: String,
    /// True when Hidely changed this window and must restore it.
    owned: bool,
}

/// A window together with what Hidely knows about it.
#[derive(Debug, Clone)]
pub struct WindowView {
    pub info: WindowInfo,
    pub status: Status,
    pub detail: String,
    pub owned: bool,
    pub pinned: bool,
}

/// A request that has been announced (the window shows as Applying) but not carried out.
/// Run it without holding the engine lock, then hand the result to [`Engine::finish`].
#[derive(Debug, Clone)]
pub struct Ticket {
    pub hwnd: isize,
    pub hide: bool,
    expected_pid: Option<u32>,
    source: Source,
    app: String,
}

impl Ticket {
    /// Carry out the request. The window's owner is checked again first.
    pub fn run(&self) -> Result<Outcome, CloakError> {
        apply(self.hwnd, self.hide, self.expected_pid)
    }
}

#[derive(Debug, Clone)]
pub struct SessionInfo {
    pub started_unix: u64,
    pub profile_name: Option<String>,
    pub rules: Vec<AppRule>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Summary {
    pub session_active: bool,
    pub started_unix: Option<u64>,
    pub profile_name: Option<String>,
    /// True while new windows are being matched against profile rules.
    pub watching: bool,
    pub applied: usize,
    pub applying: usize,
    pub unsupported: usize,
    pub errors: usize,
}

/// One line of the local operation log. It names the program, never the window title.
#[derive(Debug, Clone, Serialize)]
pub struct LogEntry {
    pub unix_time: u64,
    pub action: &'static str,
    pub source: &'static str,
    /// Executable file name only, e.g. `spotify.exe`.
    pub app: String,
    pub outcome: String,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct EndReport {
    pub restored: usize,
    pub failed: usize,
}

pub struct Engine {
    cloaker: Cloaker,
    states: HashMap<isize, WindowState>,
    pinned: HashSet<isize>,
    session: Option<SessionInfo>,
    /// Windows a rule has already been tried on, so a failure is not retried forever.
    attempted: HashSet<isize>,
    /// Windows protected only because the browser extension asked. The extension may
    /// release these, and nothing else.
    by_extension: HashSet<isize>,
    log: VecDeque<LogEntry>,
    recovery: Option<EndReport>,
    last_end: Option<EndReport>,
}

impl Engine {
    /// Take over a cloaker. Anything its journal says a previous run left hidden is
    /// restored first; see [`Engine::recovery`] for how that went.
    pub fn new(mut cloaker: Cloaker) -> Self {
        let left_over = cloaker.hidden_count();
        let report = cloaker.restore_all();
        let mut engine = Self {
            cloaker,
            states: HashMap::new(),
            pinned: HashSet::new(),
            session: None,
            attempted: HashSet::new(),
            by_extension: HashSet::new(),
            log: VecDeque::new(),
            recovery: None,
            last_end: None,
        };
        if left_over > 0 {
            engine.log_restore(&report, "recovery");
            engine.recovery =
                Some(EndReport { restored: report.restored, failed: report.failed.len() });
        }
        engine
    }

    // ------------------------------------------------------------ one window

    /// Announce a change to one window. Hiding a window starts a session if none is
    /// running, so there is always something to end.
    pub fn begin(&mut self, hwnd: isize, hide: bool, source: Source) -> Ticket {
        let pid = owner_pid(hwnd);
        let app = pid.map(|pid| process_identity(pid).exe).unwrap_or_default();
        if hide && self.session.is_none() {
            self.start_session(None, Vec::new());
        }
        let owned = self.states.get(&hwnd).is_some_and(|state| state.owned);
        self.states.insert(
            hwnd,
            WindowState { status: Status::Applying, detail: "Working on it…".into(), owned },
        );
        // When un-hiding, insist on the process we hid it for.
        let expected_pid = if hide { pid } else { self.cloaker.owner_of(hwnd).or(pid) };
        Ticket { hwnd, hide, expected_pid, source, app }
    }

    /// Record how a request turned out.
    pub fn finish(&mut self, ticket: &Ticket, result: Result<Outcome, CloakError>) -> Status {
        let outcome_code = match &result {
            Ok(Outcome::Changed { .. }) => "OK".to_string(),
            Ok(Outcome::AlreadyProtected) => "ALREADY_PROTECTED".to_string(),
            Err(error) => error.code(),
        };
        self.push_log(
            if ticket.hide { "protect" } else { "restore" },
            ticket.source.label(),
            &ticket.app,
            outcome_code,
        );

        let hwnd = ticket.hwnd;
        let state = match (ticket.hide, result) {
            (true, Ok(outcome @ Outcome::Changed { .. })) => {
                self.cloaker.record(hwnd, outcome);
                if ticket.source == Source::Extension {
                    self.by_extension.insert(hwnd);
                } else {
                    // The user or a profile now wants this window protected too, so the
                    // extension may no longer release it.
                    self.by_extension.remove(&hwnd);
                }
                Some(WindowState {
                    status: Status::Applied,
                    detail: "Windows confirmed this window is excluded from capture.".into(),
                    owned: true,
                })
            }
            (true, Ok(Outcome::AlreadyProtected)) => Some(already_protected()),
            (false, Ok(_)) => {
                self.cloaker.forget(hwnd);
                self.by_extension.remove(&hwnd);
                None
            }
            (_, Err(CloakError::WindowGone)) | (false, Err(CloakError::OwnerChanged)) => {
                self.cloaker.forget(hwnd);
                None
            }
            (true, Err(CloakError::Unsupported(why))) => Some(WindowState {
                status: Status::Unsupported,
                detail: format!("Hidely can't protect this window: {why}."),
                owned: false,
            }),
            (true, Err(error)) => Some(WindowState {
                status: Status::Error,
                detail: format!("Not protected: {error}."),
                owned: false,
            }),
            (false, Err(error)) => Some(WindowState {
                status: Status::Error,
                detail: format!("Still excluded from capture. Could not undo it: {error}."),
                owned: true,
            }),
        };
        let status = state.as_ref().map_or(Status::Unprotected, |state| state.status);
        match state {
            Some(state) => self.states.insert(hwnd, state),
            None => self.states.remove(&hwnd),
        };
        status
    }

    /// Hide one window and wait for the result.
    pub fn protect(&mut self, hwnd: isize, source: Source) -> Status {
        let ticket = self.begin(hwnd, true, source);
        let result = ticket.run();
        self.finish(&ticket, result)
    }

    /// Make one window capturable again and wait for the result.
    pub fn unprotect(&mut self, hwnd: isize) -> Status {
        let ticket = self.begin(hwnd, false, Source::Manual);
        let result = ticket.run();
        self.finish(&ticket, result)
    }

    // --------------------------------------------------------------- session

    /// Start a session, or give the running one a profile's rules.
    pub fn start_session(&mut self, profile_name: Option<String>, rules: Vec<AppRule>) {
        let started_unix = self.session.as_ref().map_or_else(now, |session| session.started_unix);
        self.session = Some(SessionInfo { started_unix, profile_name, rules });
        self.attempted.clear();
        self.last_end = None;
    }

    pub fn session(&self) -> Option<&SessionInfo> {
        self.session.as_ref()
    }

    /// Which of these windows the session's rules should now be applied to. Each window
    /// is offered once; nothing is offered when no session with rules is running.
    pub fn rule_targets(&mut self, windows: &[WindowInfo]) -> Vec<isize> {
        let Some(session) = &self.session else { return Vec::new() };
        if session.rules.is_empty() {
            return Vec::new();
        }
        // Forget windows that have closed, so a reused handle gets a fresh look.
        self.attempted.retain(|hwnd| windows.iter().any(|window| window.hwnd == *hwnd));
        let own = std::process::id();
        let mut targets = Vec::new();
        for window in windows {
            let wanted = window.pid != own
                && !self.attempted.contains(&window.hwnd)
                && session.rules.iter().any(|rule| rule.matches(window));
            if wanted {
                self.attempted.insert(window.hwnd);
                targets.push(window.hwnd);
            }
        }
        targets
    }

    /// End the session: stop applying rules at once and make every window Hidely changed
    /// capturable again. Windows that could not be restored are reported and kept.
    pub fn end_session(&mut self) -> EndReport {
        self.session = None;
        self.attempted.clear();
        self.by_extension.clear();
        let report = self.cloaker.restore_all();
        self.log_restore(&report, "session-end");

        self.states.clear();
        for (hwnd, error) in &report.failed {
            self.states.insert(
                *hwnd,
                WindowState {
                    status: Status::Error,
                    detail: format!("Still excluded from capture. Could not undo it: {error}."),
                    owned: true,
                },
            );
        }
        let summary = EndReport { restored: report.restored, failed: report.failed.len() };
        self.last_end = Some(summary.clone());
        summary
    }

    // ------------------------------------------------------------------ pins

    pub fn set_pinned(&mut self, hwnd: isize, pinned: bool) {
        if pinned {
            self.pinned.insert(hwnd);
        } else {
            self.pinned.remove(&hwnd);
        }
    }

    pub fn pinned(&self) -> Vec<isize> {
        self.pinned.iter().copied().collect()
    }

    // --------------------------------------------------------------- reading

    /// Combine a fresh window list with what Hidely knows. Closed windows are forgotten.
    pub fn views(&mut self, windows: Vec<WindowInfo>) -> Vec<WindowView> {
        let open: HashSet<isize> = windows.iter().map(|window| window.hwnd).collect();
        self.states.retain(|hwnd, _| open.contains(hwnd));
        self.pinned.retain(|hwnd| open.contains(hwnd));
        self.cloaker.prune();

        windows
            .into_iter()
            .map(|info| {
                let state = self.states.get(&info.hwnd).cloned().or_else(|| {
                    // Not something we touched. Report honestly if the app protects itself.
                    display_affinity(info.hwnd)
                        .is_some_and(|affinity| affinity != WDA_NONE)
                        .then(already_protected)
                });
                let pinned = self.pinned.contains(&info.hwnd);
                match state {
                    Some(state) => WindowView {
                        info,
                        status: state.status,
                        detail: state.detail,
                        owned: state.owned,
                        pinned,
                    },
                    None => WindowView {
                        info,
                        status: Status::Unprotected,
                        detail: "Captured normally.".into(),
                        owned: false,
                        pinned,
                    },
                }
            })
            .collect()
    }

    /// Whether this window is protected only because the browser extension asked.
    pub fn is_by_extension(&self, hwnd: isize) -> bool {
        self.by_extension.contains(&hwnd) && self.cloaker.is_hidden(hwnd)
    }

    /// Whether Hidely itself has this window excluded from capture right now.
    pub fn is_protecting(&self, hwnd: isize) -> bool {
        self.cloaker.is_hidden(hwnd)
    }

    pub fn summary(&self) -> Summary {
        let count = |status| self.states.values().filter(|state| state.status == status).count();
        Summary {
            session_active: self.session.is_some(),
            started_unix: self.session.as_ref().map(|session| session.started_unix),
            profile_name: self.session.as_ref().and_then(|session| session.profile_name.clone()),
            watching: self.session.as_ref().is_some_and(|session| !session.rules.is_empty()),
            applied: self.states.values().filter(|s| s.status == Status::Applied && s.owned).count(),
            applying: count(Status::Applying),
            unsupported: count(Status::Unsupported),
            errors: count(Status::Error),
        }
    }

    /// How restoring a previous run's leftovers went at startup, if there were any.
    pub fn recovery(&self) -> Option<&EndReport> {
        self.recovery.as_ref()
    }

    /// How the most recently ended session's restore went.
    pub fn last_end(&self) -> Option<&EndReport> {
        self.last_end.as_ref()
    }

    pub fn log(&self) -> impl Iterator<Item = &LogEntry> {
        self.log.iter()
    }

    // -------------------------------------------------------------- internal

    fn log_restore(&mut self, report: &RestoreReport, source: &'static str) {
        for _ in 0..report.restored {
            self.push_log("restore", source, "", "OK".into());
        }
        for (_, error) in &report.failed {
            self.push_log("restore", source, "", error.code());
        }
    }

    fn push_log(&mut self, action: &'static str, source: &'static str, app: &str, outcome: String) {
        if self.log.len() == LOG_CAPACITY {
            self.log.pop_front();
        }
        self.log.push_back(LogEntry {
            unix_time: now(),
            action,
            source,
            app: app.to_string(),
            outcome,
        });
    }
}

fn already_protected() -> WindowState {
    WindowState {
        status: Status::Applied,
        detail: "This app already keeps this window out of captures by itself. Hidely leaves it alone."
            .into(),
        owned: false,
    }
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |elapsed| elapsed.as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn engine() -> Engine {
        Engine::new(Cloaker::with_journal(None))
    }

    /// A ticket for a window handle that does not exist, so nothing real is touched.
    fn ticket(hwnd: isize, hide: bool) -> Ticket {
        Ticket { hwnd, hide, expected_pid: None, source: Source::Manual, app: "test.exe".into() }
    }

    fn window(hwnd: isize, exe: &str) -> WindowInfo {
        WindowInfo {
            hwnd,
            title: String::new(),
            pid: 4_000_000_000,
            process: exe.trim_end_matches(".exe").into(),
            exe: exe.into(),
            path: format!(r"C:\Apps\{exe}"),
        }
    }

    fn rule(exe: &str) -> AppRule {
        AppRule { exe: exe.into(), path: None }
    }

    #[test]
    fn denied_and_unsupported_requests_never_count_as_protected() {
        let mut engine = engine();
        assert_eq!(engine.finish(&ticket(1, true), Err(CloakError::AccessDenied)), Status::Error);
        assert_eq!(
            engine.finish(&ticket(2, true), Err(CloakError::Unsupported("32-bit apps are not supported yet"))),
            Status::Unsupported
        );
        assert_eq!(engine.finish(&ticket(3, true), Err(CloakError::Timeout)), Status::Error);
        assert_eq!(engine.finish(&ticket(4, true), Err(CloakError::NotVerified)), Status::Error);

        let summary = engine.summary();
        assert_eq!((summary.applied, summary.unsupported, summary.errors), (0, 1, 3));
        assert!(engine.states[&1].detail.contains("administrator"), "the fix should be explained");
        assert!(engine.states[&2].detail.contains("32-bit"));
    }

    #[test]
    fn a_confirmed_change_is_applied_and_partial_success_is_visible() {
        let mut engine = engine();
        let changed = Outcome::Changed { pid: 4_000_000_000, started: 1 };
        assert_eq!(engine.finish(&ticket(1, true), Ok(changed)), Status::Applied);
        engine.finish(&ticket(2, true), Err(CloakError::AccessDenied));

        let summary = engine.summary();
        assert_eq!((summary.applied, summary.errors), (1, 1), "one worked and one did not");
    }

    #[test]
    fn a_window_the_app_protects_itself_is_not_ours_to_restore() {
        let mut engine = engine();
        assert_eq!(engine.finish(&ticket(1, true), Ok(Outcome::AlreadyProtected)), Status::Applied);
        assert!(!engine.states[&1].owned);
        assert_eq!(engine.summary().applied, 0, "only windows Hidely changed are counted");
        assert_eq!(engine.cloaker.owner_of(1), None);
    }

    #[test]
    fn requests_show_as_applying_until_they_finish() {
        let mut engine = engine();
        let ticket = engine.begin(999_999_999, true, Source::Manual);
        assert_eq!(engine.summary().applying, 1);
        assert!(engine.summary().session_active, "hiding a window starts a session");
        // The handle is not a real window, so running the ticket fails cleanly.
        let result = ticket.run();
        assert_eq!(result, Err(CloakError::WindowGone));
        assert_eq!(engine.finish(&ticket, result), Status::Unprotected);
        assert_eq!(engine.summary().applying, 0);
    }

    #[test]
    fn rules_pick_matching_windows_once_and_stop_when_the_session_ends() {
        let mut engine = engine();
        let windows = vec![window(10, "slack.exe"), window(11, "notepad.exe"), window(12, "slack.exe")];

        assert!(engine.rule_targets(&windows).is_empty(), "no session, no rules applied");

        engine.start_session(Some("Work".into()), vec![rule("slack.exe")]);
        assert_eq!(engine.rule_targets(&windows), vec![10, 12], "unrelated windows are left alone");
        assert!(engine.rule_targets(&windows).is_empty(), "each window is tried once");

        let mut more = windows.clone();
        more.push(window(13, "slack.exe"));
        assert_eq!(engine.rule_targets(&more), vec![13], "a new matching window is picked up");

        engine.end_session();
        more.push(window(14, "slack.exe"));
        assert!(engine.rule_targets(&more).is_empty(), "rules stop the moment the session ends");
    }

    #[test]
    fn the_extension_can_only_release_what_it_protected() {
        let mut engine = engine();
        let changed = Outcome::Changed { pid: 4_000_000_000, started: 1 };
        let from_extension =
            Ticket { hwnd: 5, hide: true, expected_pid: None, source: Source::Extension, app: "chrome.exe".into() };
        engine.finish(&from_extension, Ok(changed));
        assert!(engine.by_extension.contains(&5));

        // The user protects the same window by hand: it is theirs now.
        engine.finish(&ticket(5, true), Ok(changed));
        assert!(!engine.by_extension.contains(&5), "a manual choice outranks the extension");

        // A window the user protected was never the extension's.
        engine.finish(&ticket(6, true), Ok(changed));
        assert!(!engine.by_extension.contains(&6));

        engine.finish(&from_extension, Ok(changed));
        engine.end_session();
        assert!(engine.by_extension.is_empty(), "ending the session clears it");
    }

    #[test]
    fn a_manual_session_has_no_rules() {
        let mut engine = engine();
        engine.start_session(None, Vec::new());
        assert!(engine.summary().session_active);
        assert!(!engine.summary().watching);
        assert!(engine.rule_targets(&[window(10, "slack.exe")]).is_empty());
    }

    #[test]
    fn the_log_names_programs_and_outcomes_only() {
        let mut engine = engine();
        engine.finish(&ticket(1, true), Err(CloakError::Os { call: "OpenProcess", code: 87 }));
        let entry = engine.log().next().unwrap();
        assert_eq!((entry.action, entry.source, entry.app.as_str()), ("protect", "manual", "test.exe"));
        assert_eq!(entry.outcome, "OS_ERROR:OpenProcess:87");
    }
}
