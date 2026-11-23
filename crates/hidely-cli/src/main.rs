use std::process::ExitCode;
use std::time::{Duration, Instant};

use hidely_core::cloaker::Outcome;
use hidely_core::engine::{Engine, Source, Status, WATCH_INTERVAL_MS};
use hidely_core::model::AppRule;
use hidely_core::{display_affinity, list_windows, Cloaker, WindowInfo, WDA_NONE};

const USAGE: &str = "\
hidely-cli — keep windows out of screen captures

USAGE:
    hidely-cli list                      List windows (* = excluded from capture)
    hidely-cli hide <target>             Exclude matching windows from capture
    hidely-cli show <target>             Make matching windows capturable again
    hidely-cli restore                   Make everything Hidely changed capturable again
    hidely-cli watch <exe>... [--seconds N]
                                         Run a privacy session: protect these programs'
                                         windows, including ones opened while it runs,
                                         then restore them. Runs until Enter is pressed,
                                         or for N seconds.

<target> is a window handle (0x1A2B3C) or a process name (notepad).
<exe> is an executable file name (notepad.exe); the whole name must match.";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    match args.as_slice() {
        ["list"] => {
            for w in list_windows(None) {
                let mark = match display_affinity(w.hwnd) {
                    Some(WDA_NONE) | None => ' ',
                    Some(_) => '*',
                };
                println!("{mark} {:#010x}  {:<24} {}", w.hwnd, w.process, w.title);
            }
            ExitCode::SUCCESS
        }
        ["hide", target] => apply(target, true),
        ["show", target] => apply(target, false),
        ["restore"] => {
            let report = Cloaker::new().restore_all();
            println!("restored {} window(s)", report.restored);
            for (hwnd, error) in &report.failed {
                eprintln!("still excluded {hwnd:#x}: {error}");
            }
            if report.failed.is_empty() { ExitCode::SUCCESS } else { ExitCode::FAILURE }
        }
        ["watch", rest @ ..] if !rest.is_empty() => watch(rest),
        _ => {
            eprintln!("{USAGE}");
            ExitCode::from(2)
        }
    }
}

fn apply(target: &str, hide: bool) -> ExitCode {
    let windows = resolve(target);
    if windows.is_empty() {
        eprintln!("no window matches '{target}'");
        return ExitCode::FAILURE;
    }
    let mut cloaker = Cloaker::new();
    let mut failed = false;
    for w in windows {
        let result = if hide {
            cloaker.hide(w.hwnd).map(|outcome| match outcome {
                Outcome::Changed { .. } => "protection applied",
                Outcome::AlreadyProtected => "already protected by the app itself, left alone",
            })
        } else {
            cloaker.show(w.hwnd).map(|()| "capturable again")
        };
        match result {
            Ok(what) => println!("{what}: {:#x} ({}: {})", w.hwnd, w.process, w.title),
            Err(error) => {
                failed = true;
                eprintln!("failed {:#x} ({}): {error}", w.hwnd, w.process);
            }
        }
    }
    if failed { ExitCode::FAILURE } else { ExitCode::SUCCESS }
}

fn resolve(target: &str) -> Vec<WindowInfo> {
    let all = list_windows(None);
    let hex = target.strip_prefix("0x").or_else(|| target.strip_prefix("0X"));
    if let Some(hwnd) = hex.and_then(|h| isize::from_str_radix(h, 16).ok()) {
        return all.into_iter().filter(|w| w.hwnd == hwnd).collect();
    }
    let name = target.trim_end_matches(".exe");
    all.into_iter().filter(|w| w.process.eq_ignore_ascii_case(name)).collect()
}

/// A privacy session in the terminal, using the same engine as the app.
fn watch(args: &[&str]) -> ExitCode {
    let mut seconds: Option<u64> = None;
    let mut rules = Vec::new();
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if *arg == "--seconds" {
            match iter.next().and_then(|value| value.parse().ok()) {
                Some(value) => seconds = Some(value),
                None => {
                    eprintln!("--seconds needs a number");
                    return ExitCode::from(2);
                }
            }
        } else {
            rules.push(AppRule { exe: arg.to_lowercase(), path: None });
        }
    }
    if rules.is_empty() {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    }

    let mut engine = Engine::new(Cloaker::new());
    engine.start_session(None, rules);
    println!("privacy session started. A window opened from now on can appear in a capture");
    println!("for a moment before it is protected (checked every {WATCH_INTERVAL_MS} ms).");

    // Stop on Enter, or when the time is up.
    let (stop, stopped) = std::sync::mpsc::channel();
    if seconds.is_none() {
        println!("press Enter to end the session.");
        std::thread::spawn(move || {
            let _ = std::io::stdin().read_line(&mut String::new());
            let _ = stop.send(());
        });
    }
    let deadline = seconds.map(|s| Instant::now() + Duration::from_secs(s));

    loop {
        let windows = list_windows(Some(std::process::id()));
        let seen = Instant::now();
        for hwnd in engine.rule_targets(&windows) {
            let window = windows.iter().find(|w| w.hwnd == hwnd);
            let name = window.map_or("?", |w| w.exe.as_str());
            let status = engine.protect(hwnd, Source::Rule);
            let took = seen.elapsed().as_millis();
            match status {
                Status::Applied => println!("protection applied: {hwnd:#x} ({name}), {took} ms after it was seen"),
                Status::Unsupported => println!("unsupported: {hwnd:#x} ({name})"),
                _ => println!("not protected: {hwnd:#x} ({name})"),
            }
        }
        if deadline.is_some_and(|deadline| Instant::now() >= deadline) || stopped.try_recv().is_ok() {
            break;
        }
        std::thread::sleep(Duration::from_millis(WATCH_INTERVAL_MS));
    }

    let report = engine.end_session();
    println!("session ended: {} window(s) capturable again, {} could not be restored", report.restored, report.failed);
    if report.failed == 0 { ExitCode::SUCCESS } else { ExitCode::FAILURE }
}
