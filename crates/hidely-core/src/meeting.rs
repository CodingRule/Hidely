//! Notice when a meeting or streaming app opens, so Hidely can offer a privacy session.
//!
//! Hidely only offers. It never starts or ends a session because an app opened or closed.
//! Programs are matched by executable file name; window titles are never looked at.

use std::collections::HashSet;

use serde::Serialize;

use crate::WindowInfo;

/// How often the window list is checked for newly opened meeting apps.
pub const MEETING_SCAN_MS: u64 = 2000;

/// Apps that usually mean a screen is about to be shared. Chat apps such as Slack and
/// Discord are left out on purpose: they run all day, so offering on them would nag.
pub const MEETING_APPS: [(&str, &str); 6] = [
    ("zoom.exe", "Zoom"),
    ("ms-teams.exe", "Teams"),
    ("teams.exe", "Teams"),
    ("obs64.exe", "OBS"),
    ("webex.exe", "Webex"),
    ("ciscocollabhost.exe", "Webex"),
];

/// The app an offer is about.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MeetingOffer {
    /// Executable file name, e.g. `zoom.exe`.
    pub exe: String,
    /// A name to show, e.g. `Zoom`.
    pub process: String,
}

/// One run of a program: process id plus process start time, because ids are reused.
type Run = (u32, u64);

/// Watches for new runs of meeting apps.
#[derive(Debug, Default)]
pub struct MeetingWatch {
    /// The first scan only records what is already running, so apps open at sign-in never
    /// trigger an offer.
    primed: bool,
    /// Every run seen so far. Kept, so a run whose window hides and shows again is not new.
    seen: HashSet<Run>,
    offer: Option<(Run, MeetingOffer)>,
}

impl MeetingWatch {
    /// Look at the current windows. Returns a new offer when a meeting app has started a new
    /// run since the last scan and no session is running. `started` gives a process's start
    /// time, or `None` once it has exited.
    pub fn scan(
        &mut self,
        windows: &[WindowInfo],
        started: impl Fn(u32) -> Option<u64>,
        session_active: bool,
    ) -> Option<MeetingOffer> {
        // An offer for an app that has since closed is no use.
        if let Some(((pid, start), _)) = &self.offer {
            if started(*pid) != Some(*start) {
                self.offer = None;
            }
        }
        if session_active {
            self.offer = None;
        }

        let mut fresh = None;
        for window in windows {
            let Some(name) = meeting_name(&window.exe) else { continue };
            let Some(start) = started(window.pid) else { continue };
            let run = (window.pid, start);
            if self.seen.insert(run) && self.primed && !session_active && fresh.is_none() {
                fresh = Some((run, MeetingOffer { exe: window.exe.clone(), process: name.into() }));
            }
        }
        self.primed = true;
        let (run, offer) = fresh?;
        self.offer = Some((run, offer.clone()));
        Some(offer)
    }

    /// The offer still waiting for an answer, if any.
    pub fn offer(&self) -> Option<&MeetingOffer> {
        self.offer.as_ref().map(|(_, offer)| offer)
    }

    /// The user said no, or started a session some other way. The same run never asks again.
    pub fn dismiss(&mut self) {
        self.offer = None;
    }
}

/// The display name of a meeting app, if `exe` is one.
pub fn meeting_name(exe: &str) -> Option<&'static str> {
    MEETING_APPS
        .iter()
        .find(|(known, _)| known.eq_ignore_ascii_case(exe.trim()))
        .map(|(_, name)| *name)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn window(pid: u32, exe: &str) -> WindowInfo {
        WindowInfo {
            hwnd: pid as isize,
            title: String::new(),
            pid,
            process: exe.trim_end_matches(".exe").into(),
            exe: exe.into(),
            path: String::new(),
        }
    }

    /// Every process started at time `pid * 10`, unless listed as exited.
    fn started(exited: &'static [u32]) -> impl Fn(u32) -> Option<u64> {
        move |pid| (!exited.contains(&pid)).then_some(u64::from(pid) * 10)
    }

    #[test]
    fn names_are_matched_whole_and_ignoring_case() {
        assert_eq!(meeting_name("Zoom.EXE"), Some("Zoom"));
        assert_eq!(meeting_name("ms-teams.exe"), Some("Teams"));
        assert_eq!(meeting_name("zoomit.exe"), None);
        assert_eq!(meeting_name("slack.exe"), None, "chat apps would nag");
        assert_eq!(meeting_name(""), None);
    }

    #[test]
    fn apps_already_open_at_the_first_scan_are_not_offered() {
        let mut watch = MeetingWatch::default();
        assert_eq!(watch.scan(&[window(1, "zoom.exe")], started(&[]), false), None);
        assert_eq!(watch.offer(), None);
    }

    #[test]
    fn a_new_run_is_offered_exactly_once() {
        let mut watch = MeetingWatch::default();
        watch.scan(&[window(1, "notepad.exe")], started(&[]), false);
        let open = [window(1, "notepad.exe"), window(2, "zoom.exe"), window(2, "zoom.exe")];
        let offer = watch.scan(&open, started(&[]), false).unwrap();
        assert_eq!(offer, MeetingOffer { exe: "zoom.exe".into(), process: "Zoom".into() });
        assert_eq!(watch.scan(&open, started(&[]), false), None, "the same run is not new again");
        assert_eq!(watch.offer(), Some(&offer), "the offer waits for an answer");
    }

    #[test]
    fn a_reused_process_id_with_a_new_start_time_is_a_new_run() {
        let mut watch = MeetingWatch::default();
        watch.scan(&[window(2, "zoom.exe")], started(&[]), false);
        let restarted = |pid: u32| Some(u64::from(pid) * 10 + 1);
        assert!(watch.scan(&[window(2, "zoom.exe")], restarted, false).is_some());
    }

    #[test]
    fn a_dismissed_offer_stays_quiet() {
        let mut watch = MeetingWatch::default();
        watch.scan(&[], started(&[]), false);
        assert!(watch.scan(&[window(2, "zoom.exe")], started(&[]), false).is_some());
        watch.dismiss();
        assert_eq!(watch.scan(&[window(2, "zoom.exe")], started(&[]), false), None);
        assert_eq!(watch.offer(), None);
    }

    #[test]
    fn nothing_is_offered_while_a_session_runs_or_after_the_app_closes() {
        let mut watch = MeetingWatch::default();
        watch.scan(&[], started(&[]), false);
        assert_eq!(watch.scan(&[window(2, "zoom.exe")], started(&[]), true), None);
        // Opened during the session, so it is not offered once the session ends either.
        assert_eq!(watch.scan(&[window(2, "zoom.exe")], started(&[]), false), None);

        assert!(watch.scan(&[window(3, "obs64.exe")], started(&[]), false).is_some());
        watch.scan(&[], started(&[]), true);
        assert_eq!(watch.offer(), None, "starting a session answers the offer");

        assert!(watch.scan(&[window(4, "teams.exe")], started(&[]), false).is_some());
        watch.scan(&[], started(&[4]), false);
        assert_eq!(watch.offer(), None, "the app closed");
    }
}
