//! The other end of the browser extension.
//!
//! Chrome and Edge start `Hidely.exe` with the extension's address as an argument and talk
//! to it over stdin/stdout ("native messaging"). In that mode Hidely shows no window. It
//! works out which browser window the extension means and asks the running Hidely app to
//! protect or release it, so the app stays the one place that knows what is protected.
//!
//! Each message is a 4-byte little-endian length followed by that many bytes of JSON.

use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::process::Command;
use std::time::{Duration, Instant};

use hidely_core::{display_affinity, list_windows, system, window_rect, WDA_NONE};
use serde_json::{json, Value};

/// Programs the extension is allowed to act on. Anything else is refused, so the
/// extension can never be used to change a window that is not a browser's.
pub const BROWSERS: [&str; 6] =
    ["chrome.exe", "msedge.exe", "brave.exe", "vivaldi.exe", "opera.exe", "chromium.exe"];

/// Chrome refuses messages from a host above 1 MB; ours are a few hundred bytes.
const MAX_MESSAGE: usize = 64 * 1024;

/// True when the browser started us as its native messaging host.
pub fn requested() -> bool {
    std::env::args().any(|argument| argument.starts_with("chrome-extension://"))
}

/// Serve the extension until the browser closes the connection, then release every window
/// this connection protected.
pub fn run() {
    let mut windows: HashMap<i64, isize> = HashMap::new(); // browser window id -> hwnd
    let mut input = io::stdin().lock();
    while let Some(message) = read_message(&mut input) {
        let reply = handle(&message, &mut windows);
        let mut reply = reply;
        reply["id"] = message.get("id").cloned().unwrap_or(Value::Null);
        if write_message(&reply).is_err() {
            break;
        }
    }
    for hwnd in windows.into_values() {
        ask_app("--unprotect-window", hwnd);
    }
}

fn read_message(input: &mut impl Read) -> Option<Value> {
    let mut length = [0u8; 4];
    input.read_exact(&mut length).ok()?;
    let length = u32::from_le_bytes(length) as usize;
    if length == 0 || length > MAX_MESSAGE {
        return None;
    }
    let mut body = vec![0u8; length];
    input.read_exact(&mut body).ok()?;
    serde_json::from_slice(&body).ok()
}

fn write_message(message: &Value) -> io::Result<()> {
    let body = serde_json::to_vec(message)?;
    let mut output = io::stdout().lock();
    output.write_all(&(body.len() as u32).to_le_bytes())?;
    output.write_all(&body)?;
    output.flush()
}

fn handle(message: &Value, windows: &mut HashMap<i64, isize>) -> Value {
    match message["type"].as_str() {
        Some("hello") => json!({
            "ok": true,
            "version": env!("CARGO_PKG_VERSION"),
            "running": system::is_app_running(),
        }),
        Some("set") => set_window(message, windows),
        Some("forget") => {
            if let Some(hwnd) = message["windowId"].as_i64().and_then(|id| windows.remove(&id)) {
                ask_app("--unprotect-window", hwnd);
            }
            json!({ "ok": true, "protected": false })
        }
        _ => json!({ "ok": false, "error": "Hidely did not understand that request." }),
    }
}

/// Protect or release the browser window the extension is talking about.
fn set_window(message: &Value, windows: &mut HashMap<i64, isize>) -> Value {
    let Some(window_id) = message["windowId"].as_i64() else {
        return json!({ "ok": false, "error": "The request named no browser window." });
    };
    let protect = message["protect"].as_bool().unwrap_or(false);
    if !system::is_app_running() {
        return json!({
            "ok": false,
            "running": false,
            "error": "Hidely is not running. Start Hidely, then try again.",
        });
    }

    if !protect {
        if let Some(hwnd) = windows.remove(&window_id) {
            ask_app("--unprotect-window", hwnd);
            wait_for(hwnd, false);
        }
        return json!({ "ok": true, "running": true, "protected": false });
    }

    // Reuse the window we already matched, as long as it still exists.
    let known = windows.get(&window_id).copied().filter(|&hwnd| hidely_core::is_window(hwnd));
    let Some(hwnd) = known.or_else(|| find_window(message)) else {
        return json!({
            "ok": false,
            "running": true,
            "error": "Hidely could not tell which browser window this tab is in.",
        });
    };
    windows.insert(window_id, hwnd);
    ask_app("--protect-window", hwnd);
    // Report what Windows says, not what was asked for.
    let protected = wait_for(hwnd, true);
    if protected {
        json!({ "ok": true, "running": true, "protected": true })
    } else {
        json!({
            "ok": false,
            "running": true,
            "protected": false,
            "error": "Hidely could not protect this window. Open Hidely to see why.",
        })
    }
}

/// Find the top-level window that is showing the tab the extension described.
///
/// The browser tells the extension where each of its windows is on screen, so position is
/// the main clue: it does not depend on the browser's language or on how it words its
/// window titles. The active tab's title breaks ties between windows in the same place
/// (two maximised windows, say). The window can lag behind a tab switch by a moment, so
/// this keeps looking for up to a second.
fn find_window(message: &Value) -> Option<isize> {
    let title = message["title"].as_str().unwrap_or("").trim().to_string();
    let bound = |name: &str| message["bounds"][name].as_i64().map(|value| value as i32);
    let bounds = match (bound("left"), bound("top"), bound("width"), bound("height")) {
        (Some(left), Some(top), Some(width), Some(height)) => Some((left, top, width, height)),
        _ => None,
    };

    let deadline = Instant::now() + Duration::from_millis(1000);
    loop {
        let candidates: Vec<Candidate> = list_windows(None)
            .into_iter()
            .filter(|window| BROWSERS.contains(&window.exe.as_str()))
            .map(|window| Candidate {
                hwnd: window.hwnd,
                title_fits: title_fits(&window.title, &title),
                off_by: bounds
                    .zip(window_rect(window.hwnd))
                    .map(|(wanted, rect)| off_by(rect, wanted, hidely_core::window_scale(window.hwnd))),
            })
            .collect();
        if let Some(hwnd) = choose(&candidates) {
            return Some(hwnd);
        }
        if Instant::now() >= deadline {
            return None;
        }
        std::thread::sleep(Duration::from_millis(80));
    }
}

/// One browser window, and how well it fits what the extension described.
struct Candidate {
    hwnd: isize,
    /// The window's title starts with the active tab's title.
    title_fits: bool,
    /// How far the window is from where the extension said it is, in pixels summed over
    /// its four edges. `None` when the position is unknown.
    off_by: Option<i64>,
}

/// A window within this many pixels (summed over its edges) is "in the right place".
/// Browsers report positions a few pixels off because of invisible resize borders.
const CLOSE_ENOUGH: i64 = 64;

/// Pick the window the extension means, or none if nothing fits well enough to be sure.
fn choose(candidates: &[Candidate]) -> Option<isize> {
    let in_place = |candidate: &&Candidate| candidate.off_by.is_some_and(|off| off <= CLOSE_ENOUGH);
    let nearest = |list: Vec<&Candidate>| list.into_iter().min_by_key(|c| c.off_by.unwrap_or(i64::MAX)).map(|c| c.hwnd);

    // Best: in the right place and showing the right tab.
    let both: Vec<_> = candidates.iter().filter(in_place).filter(|c| c.title_fits).collect();
    if !both.is_empty() {
        return nearest(both);
    }
    // In the right place, and the only window there: the title is just lagging.
    let placed: Vec<_> = candidates.iter().filter(in_place).collect();
    if placed.len() == 1 {
        return Some(placed[0].hwnd);
    }
    // Position unknown or unusable, but exactly one window shows that tab.
    let titled: Vec<_> = candidates.iter().filter(|c| c.title_fits).collect();
    if placed.is_empty() && titled.len() == 1 {
        return Some(titled[0].hwnd);
    }
    None
}

/// A browser window's title begins with its active tab's title, whatever follows:
/// "Inbox - Google Chrome", "Inbox and 2 more pages - Personal - Microsoft Edge".
fn title_fits(window_title: &str, tab_title: &str) -> bool {
    !tab_title.is_empty() && window_title.starts_with(tab_title)
}

/// How far a window's real rectangle is from the one the browser reported. The browser
/// reports unscaled units, while Windows may report scaled ones, so try both and keep the
/// better fit.
fn off_by(rect: (i32, i32, i32, i32), reported: (i32, i32, i32, i32), scale: f64) -> i64 {
    let scaled = |value: i32| (value as f64 * scale).round() as i32;
    let as_scaled = (scaled(reported.0), scaled(reported.1), scaled(reported.2), scaled(reported.3));
    distance(rect, reported).min(distance(rect, as_scaled))
}

/// How far apart two rectangles are, as a single number to compare candidates by.
fn distance(a: (i32, i32, i32, i32), b: (i32, i32, i32, i32)) -> i64 {
    [(a.0, b.0), (a.1, b.1), (a.2, b.2), (a.3, b.3)]
        .iter()
        .map(|(x, y)| (*x as i64 - *y as i64).abs())
        .sum()
}

/// Hand a request to the running app. Starting Hidely again while it is open passes the
/// arguments to the running copy, which is the only one that changes windows.
fn ask_app(argument: &str, hwnd: isize) {
    if let Ok(exe) = std::env::current_exe() {
        let _ = Command::new(exe).arg(argument).arg(hwnd.to_string()).status();
    }
}

/// Wait up to two seconds for the window to become excluded (or capturable), and say
/// whether it did.
fn wait_for(hwnd: isize, protected: bool) -> bool {
    let deadline = Instant::now() + Duration::from_millis(2000);
    loop {
        let excluded = display_affinity(hwnd).is_some_and(|affinity| affinity != WDA_NONE);
        if excluded == protected {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(40));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_window_title_starts_with_its_active_tab_whatever_the_browser_adds() {
        assert!(title_fits("Inbox - Google Chrome", "Inbox"));
        assert!(title_fits("Inbox and 2 more pages - Personal - Microsoft\u{200b} Edge", "Inbox"));
        assert!(title_fits("Inbox și încă 1 pagină - Personal - Microsoft Edge", "Inbox"), "other languages");
        assert!(!title_fits("My Inbox - Google Chrome", "Inbox"));
        assert!(!title_fits("Anything - Google Chrome", ""), "an empty title fits nothing");
    }

    fn candidate(hwnd: isize, title_fits: bool, off_by: Option<i64>) -> Candidate {
        Candidate { hwnd, title_fits, off_by }
    }

    #[test]
    fn the_window_in_the_right_place_showing_the_right_tab_is_chosen() {
        let windows = [candidate(1, false, Some(900)), candidate(2, true, Some(12)), candidate(3, true, Some(700))];
        assert_eq!(choose(&windows), Some(2));
    }

    #[test]
    fn two_windows_in_the_same_place_are_told_apart_by_their_tab() {
        let maximised = [candidate(1, false, Some(0)), candidate(2, true, Some(0))];
        assert_eq!(choose(&maximised), Some(2));
    }

    #[test]
    fn a_lagging_title_is_tolerated_when_only_one_window_is_in_place() {
        assert_eq!(choose(&[candidate(1, false, Some(10)), candidate(2, false, Some(800))]), Some(1));
    }

    #[test]
    fn nothing_is_chosen_when_it_would_be_a_guess() {
        // Two windows in place, neither showing the tab.
        assert_eq!(choose(&[candidate(1, false, Some(0)), candidate(2, false, Some(0))]), None);
        // Nothing in place, and two windows show a tab with that title.
        assert_eq!(choose(&[candidate(1, true, Some(900)), candidate(2, true, Some(950))]), None);
        // A window showing that tab, but another window is where the browser said.
        assert_eq!(choose(&[candidate(1, true, Some(900)), candidate(2, false, Some(5))]), Some(2));
        assert_eq!(choose(&[]), None);
    }

    #[test]
    fn position_alone_is_not_needed_when_exactly_one_window_shows_the_tab() {
        assert_eq!(choose(&[candidate(1, true, None), candidate(2, false, None)]), Some(1));
    }

    #[test]
    fn reported_positions_are_compared_scaled_and_unscaled() {
        // A 150% monitor: the browser says 100,100 800x600; Windows says 150,150 1200x900.
        assert_eq!(off_by((150, 150, 1200, 900), (100, 100, 800, 600), 1.5), 0);
        assert_eq!(off_by((100, 100, 800, 600), (100, 100, 800, 600), 1.5), 0);
        assert!(off_by((900, 100, 800, 600), (100, 100, 800, 600), 1.0) > CLOSE_ENOUGH);
    }

    #[test]
    fn messages_round_trip_through_the_wire_format() {
        let body = br#"{"type":"hello","id":7}"#;
        let mut wire = (body.len() as u32).to_le_bytes().to_vec();
        wire.extend_from_slice(body);
        let message = read_message(&mut wire.as_slice()).unwrap();
        assert_eq!(message["type"], "hello");
        assert_eq!(message["id"], 7);

        assert!(read_message(&mut [0u8, 0, 0, 0].as_slice()).is_none(), "empty message");
        let huge = (10_000_000u32).to_le_bytes();
        assert!(read_message(&mut huge.as_slice()).is_none(), "oversized message is refused");
        assert!(read_message(&mut [5u8, 0, 0, 0, b'{'].as_slice()).is_none(), "cut-off message");
    }

    #[test]
    fn unknown_requests_are_refused_and_nothing_is_touched() {
        let mut windows = HashMap::new();
        let reply = handle(&json!({ "type": "launch-missiles" }), &mut windows);
        assert_eq!(reply["ok"], false);
        assert!(windows.is_empty());
        let reply = handle(&json!({ "type": "set", "protect": true }), &mut windows);
        assert_eq!(reply["ok"], false, "a request without a window id is refused");
    }

    #[test]
    fn the_nearest_window_wins_a_tie() {
        let wanted = (100, 100, 800, 600);
        assert!(distance((100, 100, 800, 600), wanted) < distance((900, 100, 800, 600), wanted));
        assert_eq!(distance(wanted, wanted), 0);
    }
}
