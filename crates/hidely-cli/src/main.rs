use std::process::ExitCode;

use hidely_core::{display_affinity, list_windows, Cloaker, WindowInfo, WDA_NONE};

const USAGE: &str = "\
hidely-cli — hide windows from screen capture

USAGE:
    hidely-cli list                 List windows (* = excluded from capture)
    hidely-cli hide <target>        Hide matching windows from capture
    hidely-cli show <target>        Make matching windows capturable again
    hidely-cli restore              Un-hide everything Hidely has hidden

<target> is a window handle (0x1A2B3C) or a process name (notepad).";

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
            println!("restored {} window(s)", Cloaker::new().restore_all());
            ExitCode::SUCCESS
        }
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
        let result = if hide { cloaker.hide(w.hwnd) } else { cloaker.show(w.hwnd) };
        match result {
            Ok(()) => println!(
                "{} {:#x} ({}: {})",
                if hide { "hidden" } else { "shown" },
                w.hwnd,
                w.process,
                w.title
            ),
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
