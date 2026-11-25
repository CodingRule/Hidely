//! The one place that knows what Hidely is doing: which windows are protected, whether a
//! privacy session is running, and what happened to every request.
//!
//! The tray, the window and the CLI all read and change this model, so they cannot
//! disagree with each other.

use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};
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

/// How many times one window is re-protected in a session after it lost protection.
/// After that the app is clearly resetting it on purpose, and fighting it would loop.
pub const VERIFY_MAX_REAPPLY: u8 = 3;

const LOST_DETAIL: &str = "Windows reports this window is capturable again.";

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
    /// A periodic read-back found the window capturable again, so it is re-protected.
    Reverify,
}

impl Source {
    fn label(self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::Rule => "rule",
            Self::Extension => "extension",
            Self::Reverify => "reverify",
        }
    }
}

#[derive(Debug, Clone)]
struct WindowState {
    status: Status,
    detail: String,
    /// True when Hidely changed this window and must restore it.
    owned: bool,
    /// When reading the setting back last confirmed the window is excluded from capture.
    verified_unix: Option<u64>,
}

impl WindowState {
    fn new(status: Status, detail: impl Into<String>, owned: bool) -> Self {
        Self { status, detail: detail.into(), owned, verified_unix: None }
    }
}

/// A window together with what Hidely knows about it.
#[derive(Debug, Clone)]
pub struct WindowView {
    pub info: WindowInfo,
    pub status: Status,
    pub detail: String,
    pub owned: bool,
    pub pinned: bool,
    /// When Windows last confirmed this window is excluded from capture.
    pub verified_unix: Option<u64>,
}

/// A window that [`Engine::reverify`] found capturable again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lost {
    pub hwnd: isize,
    /// Executable file name only.
    pub app: String,
    /// Whether the caller should protect it again. False once the cap is reached.
    pub reapply: bool,
    /// The session it was lost in. Re-protecting is only allowed in that same session.
    pub session: Option<u64>,
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
    /// The session this request belongs to, so a late result is not counted in the next.
    session: Option<u64>,
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
    id: u64,
    stats: SessionStats,
}

/// What happened during one session. Windows are counted once each, not per event, and
/// programs are named by file name only, never by window title. Nothing here is saved.
#[derive(Debug, Clone, Default)]
struct SessionStats {
    protected: HashSet<isize>,
    apps: BTreeSet<String>,
    by_rule: HashSet<isize>,
    by_extension: HashSet<isize>,
    failed: HashSet<isize>,
    lost: HashSet<isize>,
    reapplied: usize,
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
    /// The newest time a read-back confirmed a protected window, this session.
    pub last_verified_unix: Option<u64>,
    /// Windows that lost protection during this session.
    pub protection_lost: usize,
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

/// How a session went, for the summary shown when it ends. `restored` and `failed` describe
/// the final restore, exactly as in [`EndReport`]; `failures` counts windows that could not
/// be protected during the session.
#[derive(Debug, Clone, Default, Serialize)]
pub struct SessionReport {
    pub restored: usize,
    pub failed: usize,
    pub started_unix: Option<u64>,
    pub ended_unix: u64,
    pub duration_secs: u64,
    pub profile_name: Option<String>,
    pub windows_protected: usize,
    pub apps: Vec<String>,
    pub caught_by_rules: usize,
    pub by_extension: usize,
    pub failures: usize,
    pub protection_lost: usize,
    pub reapplied: usize,
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
    last_end: Option<SessionReport>,
    /// How often each window lost protection this session.
    lost_counts: HashMap<isize, u8>,
    last_verified: Option<u64>,
    last_session_id: u64,
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
            lost_counts: HashMap::new(),
            last_verified: None,
            last_session_id: 0,
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
        self.states.insert(hwnd, WindowState::new(Status::Applying, "Working on it…", owned));
        // When un-hiding, insist on the process we hid it for.
        let expected_pid = if hide { pid } else { self.cloaker.owner_of(hwnd).or(pid) };
        let session = self.session_id();
        Ticket { hwnd, hide, expected_pid, source, app, session }
    }

    /// Announce protecting a window on behalf of one particular session (a rule, a re-apply
    /// or a pin). Unlike [`Engine::begin`] this never starts a session: if that session has
    /// ended, or another one has started since, nothing happens and `None` is returned.
    pub fn begin_in(&mut self, hwnd: isize, source: Source, session: u64) -> Option<Ticket> {
        (self.session_id() == Some(session)).then(|| self.begin(hwnd, true, source))
    }

    /// Identifies the running session, for [`Engine::begin_in`].
    pub fn session_id(&self) -> Option<u64> {
        self.session.as_ref().map(|session| session.id)
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
                match ticket.source {
                    Source::Extension => {
                        self.by_extension.insert(hwnd);
                    }
                    // Re-protecting is not a new choice, so whoever owned it still does.
                    Source::Reverify => {}
                    // The user or a profile now wants this window protected too, so the
                    // extension may no longer release it.
                    Source::Manual | Source::Rule => {
                        self.by_extension.remove(&hwnd);
                    }
                }
                let mut state = WindowState::new(
                    Status::Applied,
                    "Windows confirmed this window is excluded from capture.",
                    true,
                );
                state.verified_unix = Some(now());
                Some(state)
            }
            (true, Ok(Outcome::AlreadyProtected)) => {
                // The app protects this window itself now, so ending the session must not
                // undo it, even if Hidely had protected it earlier.
                self.cloaker.forget(hwnd);
                Some(already_protected())
            }
            (false, Ok(_)) => {
                self.cloaker.forget(hwnd);
                self.by_extension.remove(&hwnd);
                None
            }
            (_, Err(CloakError::WindowGone)) | (false, Err(CloakError::OwnerChanged)) => {
                self.cloaker.forget(hwnd);
                None
            }
            (true, Err(CloakError::Unsupported(why))) => Some(WindowState::new(
                Status::Unsupported,
                format!("Hidely can't protect this window: {why}."),
                false,
            )),
            (true, Err(error)) => {
                Some(WindowState::new(Status::Error, format!("Not protected: {error}."), false))
            }
            (false, Err(error)) => Some(WindowState::new(
                Status::Error,
                format!("Still excluded from capture. Could not undo it: {error}."),
                true,
            )),
        };
        if ticket.hide {
            self.count(ticket, state.as_ref());
        }
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
        // Giving a running session a profile keeps its start time and what it has done.
        let (started_unix, id, stats) = match self.session.take() {
            Some(session) => (session.started_unix, session.id, session.stats),
            None => {
                self.last_session_id += 1;
                self.lost_counts.clear();
                self.last_verified = None;
                (now(), self.last_session_id, SessionStats::default())
            }
        };
        self.session = Some(SessionInfo { started_unix, profile_name, rules, id, stats });
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
    pub fn end_session(&mut self) -> SessionReport {
        let session = self.session.take();
        self.attempted.clear();
        self.by_extension.clear();
        self.lost_counts.clear();
        self.last_verified = None;
        let report = self.cloaker.restore_all();
        self.log_restore(&report, "session-end");

        self.states.clear();
        for (hwnd, error) in &report.failed {
            self.states.insert(
                *hwnd,
                WindowState::new(
                    Status::Error,
                    format!("Still excluded from capture. Could not undo it: {error}."),
                    true,
                ),
            );
        }
        let ended_unix = now();
        let mut summary = SessionReport {
            restored: report.restored,
            failed: report.failed.len(),
            ended_unix,
            ..SessionReport::default()
        };
        if let Some(session) = session {
            let stats = session.stats;
            summary.started_unix = Some(session.started_unix);
            summary.duration_secs = ended_unix.saturating_sub(session.started_unix);
            summary.profile_name = session.profile_name;
            summary.windows_protected = stats.protected.len();
            summary.apps = stats.apps.into_iter().collect();
            summary.caught_by_rules = stats.by_rule.len();
            summary.by_extension = stats.by_extension.len();
            summary.failures = stats.failed.len();
            summary.protection_lost = stats.lost.len();
            summary.reapplied = stats.reapplied;
        }
        self.last_end = Some(summary.clone());
        summary
    }

    // ------------------------------------------------------- re-verification

    /// Read back the setting of every window that should be excluded from capture, and
    /// report the ones Windows now says are capturable. Runs under the lock, so `read` must
    /// be a cheap call that sends no window messages, such as [`display_affinity`].
    ///
    /// Windows still being changed are skipped. Windows Hidely changed that have closed or
    /// now belong to another process are forgotten. A window whose app protected it by
    /// itself and then stopped simply shows as Unprotected again: it is not ours to fix.
    pub fn reverify(&mut self, read: impl Fn(isize) -> Option<u32>, now: u64) -> Vec<Lost> {
        let owned: Vec<(isize, bool)> = self
            .states
            .iter()
            .filter(|(_, state)| state.status == Status::Applied && state.owned)
            .map(|(hwnd, _)| (*hwnd, self.cloaker.is_hidden(*hwnd)))
            .collect();
        self.reverify_with(read, &owned, now)
    }

    /// [`Engine::reverify`] with "is it still the same window" already answered for each
    /// owned window, so tests can supply the answers.
    fn reverify_with(
        &mut self,
        read: impl Fn(isize) -> Option<u32>,
        owned: &[(isize, bool)],
        now: u64,
    ) -> Vec<Lost> {
        let mut lost = Vec::new();
        for &(hwnd, same_window) in owned {
            match same_window.then(|| read(hwnd)).flatten() {
                // Closed, or the handle now belongs to something else.
                None => {
                    self.cloaker.forget(hwnd);
                    self.states.remove(&hwnd);
                    self.by_extension.remove(&hwnd);
                }
                Some(affinity) if affinity != WDA_NONE => self.mark_verified(hwnd, now),
                Some(_) => lost.push(self.mark_lost(hwnd)),
            }
        }

        let theirs: Vec<isize> = self
            .states
            .iter()
            .filter(|(_, state)| state.status == Status::Applied && !state.owned)
            .map(|(hwnd, _)| *hwnd)
            .collect();
        for hwnd in theirs {
            match read(hwnd) {
                Some(affinity) if affinity != WDA_NONE => self.mark_verified(hwnd, now),
                _ => {
                    self.states.remove(&hwnd);
                }
            }
        }
        lost
    }

    fn mark_verified(&mut self, hwnd: isize, now: u64) {
        if let Some(state) = self.states.get_mut(&hwnd) {
            state.verified_unix = Some(now);
            self.last_verified = self.last_verified.max(Some(now));
        }
    }

    fn mark_lost(&mut self, hwnd: isize) -> Lost {
        let times = self.lost_counts.entry(hwnd).or_insert(0);
        let reapply = *times < VERIFY_MAX_REAPPLY;
        *times = times.saturating_add(1);
        let detail = if reapply {
            LOST_DETAIL.to_string()
        } else {
            format!("{LOST_DETAIL} The app keeps resetting it, so Hidely stopped re-applying.")
        };
        // Not owned: it is capturable now, so it must not read as "still hidden", and the
        // extension and the pointer guard must not treat it as protected. A successful
        // re-apply records it again. `by_extension` is kept so the extension still owns it.
        self.states.insert(hwnd, WindowState::new(Status::Error, detail, false));
        self.cloaker.forget(hwnd);
        let app = owner_pid(hwnd).map(|pid| process_identity(pid).exe).unwrap_or_default();
        self.push_log("verify", Source::Reverify.label(), &app, "LOST".into());
        if let Some(session) = self.session.as_mut() {
            session.stats.lost.insert(hwnd);
        }
        Lost { hwnd, app, reapply, session: self.session_id() }
    }

    /// The windows Hidely has excluded from capture right now.
    pub fn protected_windows(&self) -> Vec<isize> {
        self.states
            .iter()
            .filter(|(_, state)| state.status == Status::Applied && state.owned)
            .map(|(hwnd, _)| *hwnd)
            .filter(|hwnd| self.cloaker.is_hidden(*hwnd))
            .collect()
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
        // A protected window can drop out of the list while it still exists (minimised to
        // the tray, on another virtual desktop). Keep what Hidely knows about it until it
        // is really gone or its handle belongs to something else.
        let cloaker = &self.cloaker;
        let known = |hwnd: &isize| open.contains(hwnd) || cloaker.is_hidden(*hwnd);
        self.states.retain(|hwnd, _| known(hwnd));
        self.lost_counts.retain(|hwnd, _| known(hwnd));
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
                        verified_unix: state.verified_unix,
                    },
                    None => WindowView {
                        info,
                        status: Status::Unprotected,
                        detail: "Captured normally.".into(),
                        owned: false,
                        pinned,
                        verified_unix: None,
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
            last_verified_unix: self.session.as_ref().and(self.last_verified),
            protection_lost: self.session.as_ref().map_or(0, |session| session.stats.lost.len()),
        }
    }

    /// How restoring a previous run's leftovers went at startup, if there were any.
    pub fn recovery(&self) -> Option<&EndReport> {
        self.recovery.as_ref()
    }

    /// How the most recently ended session went.
    pub fn last_end(&self) -> Option<&SessionReport> {
        self.last_end.as_ref()
    }

    pub fn log(&self) -> impl Iterator<Item = &LogEntry> {
        self.log.iter()
    }

    // -------------------------------------------------------------- internal

    /// Add a finished protect request to the running session's statistics. A request that
    /// belongs to a session that has since ended is not counted.
    fn count(&mut self, ticket: &Ticket, state: Option<&WindowState>) {
        let Some(session) = self.session.as_mut() else { return };
        if ticket.session != Some(session.id) {
            return;
        }
        let stats = &mut session.stats;
        let hwnd = ticket.hwnd;
        match state.map(|state| (state.status, state.owned)) {
            Some((Status::Applied, true)) => {
                stats.protected.insert(hwnd);
                // A window that failed once and then worked is not a failure.
                stats.failed.remove(&hwnd);
                if !ticket.app.is_empty() {
                    stats.apps.insert(ticket.app.clone());
                }
                match ticket.source {
                    Source::Rule => {
                        stats.by_rule.insert(hwnd);
                    }
                    Source::Extension => {
                        stats.by_extension.insert(hwnd);
                    }
                    Source::Reverify => stats.reapplied += 1,
                    Source::Manual => {}
                }
            }
            Some((Status::Error | Status::Unsupported, _)) => {
                stats.failed.insert(hwnd);
            }
            _ => {}
        }
    }

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
    let mut state = WindowState::new(
        Status::Applied,
        "This app already keeps this window out of captures by itself. Hidely leaves it alone.",
        false,
    );
    // This was found out by reading the setting back.
    state.verified_unix = Some(now());
    state
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
        Ticket {
            hwnd,
            hide,
            expected_pid: None,
            source: Source::Manual,
            app: "test.exe".into(),
            session: None,
        }
    }

    /// A protect request that belongs to the engine's running session.
    fn session_ticket(engine: &Engine, hwnd: isize, source: Source, app: &str) -> Ticket {
        Ticket {
            hwnd,
            hide: true,
            expected_pid: None,
            source,
            app: app.into(),
            session: engine.session.as_ref().map(|session| session.id),
        }
    }

    const CHANGED: Outcome = Outcome::Changed { pid: 4_000_000_000, started: 1 };

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
        let from_extension = Ticket {
            hwnd: 5,
            hide: true,
            expected_pid: None,
            source: Source::Extension,
            app: "chrome.exe".into(),
            session: None,
        };
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

    // ------------------------------------------------------ re-verification

    /// Fake read-back: every handle reads as excluded except the listed ones.
    fn reads(capturable: &'static [isize]) -> impl Fn(isize) -> Option<u32> {
        move |hwnd| Some(if capturable.contains(&hwnd) { WDA_NONE } else { crate::WDA_EXCLUDEFROMCAPTURE })
    }

    #[test]
    fn a_window_that_loses_protection_is_reported_once_and_shows_as_an_error() {
        let mut engine = engine();
        engine.start_session(None, Vec::new());
        engine.finish(&session_ticket(&engine, 1, Source::Manual, "zoom.exe"), Ok(CHANGED));
        engine.finish(&session_ticket(&engine, 2, Source::Manual, "notepad.exe"), Ok(CHANGED));

        let lost = engine.reverify_with(reads(&[1]), &[(1, true), (2, true)], 100);
        assert_eq!(lost.len(), 1);
        assert_eq!((lost[0].hwnd, lost[0].reapply), (1, true));
        assert_eq!(engine.states[&1].status, Status::Error);
        assert!(!engine.states[&1].owned, "a capturable window must not read as still hidden");
        assert!(engine.states[&1].detail.contains("capturable again"));
        assert_eq!(engine.states[&2].verified_unix, Some(100), "the healthy one is verified");

        // An Error window is not checked again, so it is reported only once.
        let owned: Vec<(isize, bool)> = vec![(2, true)];
        assert!(engine.reverify_with(reads(&[1]), &owned, 101).is_empty());

        let summary = engine.summary();
        assert_eq!((summary.protection_lost, summary.errors, summary.applied), (1, 1, 1));
        assert_eq!(summary.last_verified_unix, Some(101));
        let entry = engine.log().last().unwrap();
        assert_eq!((entry.action, entry.source, entry.outcome.as_str()), ("verify", "reverify", "LOST"));
    }

    #[test]
    fn applying_windows_are_skipped_and_closed_or_reused_ones_are_dropped() {
        let mut engine = engine();
        engine.start_session(None, Vec::new());
        engine.finish(&session_ticket(&engine, 1, Source::Manual, "a.exe"), Ok(CHANGED));
        engine.finish(&session_ticket(&engine, 2, Source::Manual, "b.exe"), Ok(CHANGED));
        engine.finish(&session_ticket(&engine, 3, Source::Manual, "c.exe"), Ok(CHANGED));
        // Window 4 is still being changed.
        engine.states.insert(4, WindowState::new(Status::Applying, "Working on it…", false));

        // 1 now belongs to another process; 2 has closed (the read fails); 3 is fine.
        let read = |hwnd: isize| (hwnd != 2).then_some(if hwnd == 4 { WDA_NONE } else { 0x11 });
        let lost = engine.reverify_with(read, &[(1, false), (2, true), (3, true)], 5);
        assert!(lost.is_empty());
        assert!(!engine.states.contains_key(&1) && !engine.states.contains_key(&2));
        assert_eq!(engine.states[&3].verified_unix, Some(5));
        assert_eq!(engine.states[&4].status, Status::Applying, "Applying is left alone");

        // The public entry point asks the cloaker, which knows these fake handles are not
        // the windows it hid, so they are forgotten rather than reported as lost.
        assert!(engine.reverify(read, 6).is_empty());
        assert!(!engine.states.contains_key(&3));
    }

    #[test]
    fn re_applying_stops_after_the_cap() {
        let mut engine = engine();
        engine.start_session(None, Vec::new());
        for round in 0..=VERIFY_MAX_REAPPLY {
            engine.finish(&session_ticket(&engine, 7, Source::Reverify, "app.exe"), Ok(CHANGED));
            let lost = engine.reverify_with(reads(&[7]), &[(7, true)], 1);
            assert_eq!(lost[0].reapply, round < VERIFY_MAX_REAPPLY, "round {round}");
        }
        assert!(engine.states[&7].detail.contains("keeps resetting it"));
        assert_eq!(engine.summary().protection_lost, 1, "one window, however often it is lost");

        let report = engine.end_session();
        assert_eq!(report.reapplied, usize::from(VERIFY_MAX_REAPPLY) + 1);
        assert_eq!(report.protection_lost, 1);

        // A new session starts with a clean count.
        engine.start_session(None, Vec::new());
        engine.finish(&session_ticket(&engine, 7, Source::Manual, "app.exe"), Ok(CHANGED));
        assert!(engine.reverify_with(reads(&[7]), &[(7, true)], 2)[0].reapply);
    }

    #[test]
    fn re_applying_keeps_the_extensions_ownership() {
        let mut engine = engine();
        let mut from_extension = session_ticket(&engine, 5, Source::Extension, "chrome.exe");
        from_extension.session = None;
        engine.finish(&from_extension, Ok(CHANGED));
        engine.finish(&session_ticket(&engine, 5, Source::Reverify, "chrome.exe"), Ok(CHANGED));
        assert!(engine.by_extension.contains(&5), "the extension may still release it");
    }

    #[test]
    fn verification_time_is_recorded_and_self_protected_windows_are_rechecked() {
        let mut engine = engine();
        engine.start_session(None, Vec::new());
        engine.finish(&session_ticket(&engine, 1, Source::Manual, "a.exe"), Ok(CHANGED));
        engine.finish(&ticket(2, true), Ok(Outcome::AlreadyProtected));
        assert!(engine.states[&1].verified_unix.is_some(), "the apply itself was a read-back");

        assert!(engine.reverify_with(reads(&[2]), &[(1, true)], 42).is_empty());
        assert_eq!(engine.states[&1].verified_unix, Some(42));
        assert!(!engine.states.contains_key(&2), "an app that dropped its own protection is not ours");
        assert_eq!(engine.summary().last_verified_unix, Some(42));
        assert_eq!(engine.summary().protection_lost, 0);
    }

    // ------------------------------------------------------ session summary

    #[test]
    fn the_session_report_counts_windows_not_events() {
        let mut engine = engine();
        engine.start_session(None, Vec::new());
        let manual = session_ticket(&engine, 1, Source::Manual, "zoom.exe");
        engine.finish(&manual, Ok(CHANGED));
        engine.finish(&manual, Ok(CHANGED));
        engine.finish(&session_ticket(&engine, 2, Source::Rule, "slack.exe"), Ok(CHANGED));
        engine.finish(&session_ticket(&engine, 2, Source::Rule, "slack.exe"), Ok(CHANGED));
        engine.finish(&session_ticket(&engine, 3, Source::Rule, "slack.exe"), Err(CloakError::AccessDenied));
        engine.finish(&session_ticket(&engine, 3, Source::Rule, "slack.exe"), Err(CloakError::AccessDenied));
        engine.finish(&session_ticket(&engine, 4, Source::Extension, "chrome.exe"), Ok(CHANGED));
        // Failed once, then worked: not a failure.
        engine.finish(&session_ticket(&engine, 5, Source::Manual, "a.exe"), Err(CloakError::Timeout));
        engine.finish(&session_ticket(&engine, 5, Source::Manual, "a.exe"), Ok(CHANGED));

        // Giving the running session a profile keeps what it has done.
        engine.start_session(Some("Work".into()), vec![rule("slack.exe")]);

        let report = engine.end_session();
        assert_eq!(report.windows_protected, 4);
        assert_eq!(report.caught_by_rules, 1);
        assert_eq!(report.by_extension, 1);
        assert_eq!(report.failures, 1);
        assert_eq!(report.apps, vec!["a.exe", "chrome.exe", "slack.exe", "zoom.exe"]);
        assert_eq!(report.profile_name.as_deref(), Some("Work"));
        assert!(report.started_unix.is_some());
        assert_eq!(engine.last_end().unwrap().windows_protected, 4);
    }

    #[test]
    fn a_late_result_is_not_counted_in_the_next_session() {
        let mut engine = engine();
        engine.start_session(None, Vec::new());
        let late = session_ticket(&engine, 1, Source::Manual, "a.exe");
        engine.end_session();
        engine.start_session(None, Vec::new());
        engine.finish(&late, Ok(CHANGED));
        assert_eq!(engine.end_session().windows_protected, 0);
    }

    #[test]
    fn the_report_keeps_restored_and_failed_at_the_top_level() {
        let mut engine = engine();
        let json = serde_json::to_value(engine.end_session()).unwrap();
        for key in [
            "restored", "failed", "started_unix", "duration_secs", "windows_protected", "apps",
            "caught_by_rules", "failures", "protection_lost", "reapplied", "profile_name",
        ] {
            assert!(json.get(key).is_some(), "{key} is missing");
        }
        assert_eq!(json["started_unix"], serde_json::Value::Null, "no session was running");
    }

    // ------------------------------------------------------ review fixes

    /// A real, never-shown window owned by this test process, so the cloaker's "is it still
    /// the same window" check passes for it.
    fn hidden_window() -> isize {
        use windows_sys::Win32::UI::WindowsAndMessaging::CreateWindowExW;
        let class: Vec<u16> = "STATIC\0".encode_utf16().collect();
        let hwnd = unsafe {
            CreateWindowExW(
                0,
                class.as_ptr(),
                class.as_ptr(),
                0,
                0,
                0,
                10,
                10,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        };
        assert!(!hwnd.is_null(), "could not create a test window");
        hwnd as isize
    }

    fn destroy(hwnd: isize) {
        unsafe { windows_sys::Win32::UI::WindowsAndMessaging::DestroyWindow(hwnd as _) };
    }

    #[test]
    fn a_protected_window_missing_from_the_list_is_still_tracked() {
        let hwnd = hidden_window();
        let pid = std::process::id();
        let started = crate::process_start_time(pid).unwrap();
        let mut engine = engine();
        engine.start_session(None, Vec::new());
        let ticket = session_ticket(&engine, hwnd, Source::Manual, "test.exe");
        engine.finish(&ticket, Ok(Outcome::Changed { pid, started }));
        engine.lost_counts.insert(hwnd, 2);

        // Not visible, so list_windows would leave it out. It still exists.
        engine.views(Vec::new());
        assert!(engine.states[&hwnd].owned, "the state must survive the window list");
        assert_eq!(engine.lost_counts.get(&hwnd), Some(&2), "the re-apply count must survive too");

        destroy(hwnd);
        engine.views(Vec::new());
        assert!(!engine.states.contains_key(&hwnd), "a closed window is forgotten");
        assert!(!engine.lost_counts.contains_key(&hwnd));
    }

    #[test]
    fn rule_and_reapply_requests_never_start_a_session() {
        let mut engine = engine();
        engine.start_session(None, Vec::new());
        let old = engine.session_id().unwrap();
        assert!(engine.begin_in(1, Source::Reverify, old).is_some(), "the running session may act");

        engine.end_session();
        assert!(engine.begin_in(1, Source::Reverify, old).is_none());
        assert!(engine.begin_in(2, Source::Rule, old).is_none());
        assert!(!engine.summary().session_active, "a late request must not restart the session");

        engine.start_session(None, Vec::new());
        assert!(engine.begin_in(1, Source::Reverify, old).is_none(), "nor act in a newer one");
        assert_eq!(engine.summary().applying, 0);
    }

    #[test]
    fn a_lost_window_is_no_longer_treated_as_protected() {
        let mut engine = engine();
        engine.start_session(None, Vec::new());
        let mut from_extension = session_ticket(&engine, 5, Source::Extension, "chrome.exe");
        from_extension.session = engine.session_id();
        engine.finish(&from_extension, Ok(CHANGED));
        assert_eq!(engine.cloaker.owner_of(5), Some(4_000_000_000));

        let lost = engine.reverify_with(reads(&[5]), &[(5, true)], 1);
        assert_eq!(lost[0].session, engine.session_id());
        assert_eq!(engine.cloaker.owner_of(5), None, "the cloaker must forget it");
        assert!(engine.by_extension.contains(&5), "the extension still owns it");

        engine.finish(&session_ticket(&engine, 5, Source::Reverify, "chrome.exe"), Ok(CHANGED));
        assert_eq!(engine.cloaker.owner_of(5), Some(4_000_000_000), "re-applying records it again");
    }

    #[test]
    fn a_window_the_app_now_protects_itself_is_not_restored_by_hidely() {
        let mut engine = engine();
        engine.finish(&ticket(9, true), Ok(CHANGED));
        assert!(engine.cloaker.owner_of(9).is_some());
        engine.finish(&ticket(9, true), Ok(Outcome::AlreadyProtected));
        assert_eq!(engine.cloaker.owner_of(9), None, "session end must not clear the app's own setting");
        assert!(!engine.states[&9].owned);
    }
}
