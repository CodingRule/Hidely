#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod art;

use std::collections::HashSet;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use base64::Engine;
use hidely_core::{icon_rgba, is_window, list_windows, overlay, Cloaker};
use serde::Serialize;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{
    AppHandle, Emitter, Manager, PhysicalPosition, RunEvent, State, WebviewUrl,
    WebviewWindowBuilder, WindowEvent,
};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};

const TOGGLE_LABEL: &str = "Win+Shift+H";
const PANIC_LABEL: &str = "Win+Shift+U";
const HUD_MILLIS: u64 = 1600;

fn toggle_shortcut() -> Shortcut {
    Shortcut::new(Some(Modifiers::SUPER | Modifiers::SHIFT), Code::KeyH)
}

fn panic_shortcut() -> Shortcut {
    Shortcut::new(Some(Modifiers::SUPER | Modifiers::SHIFT), Code::KeyU)
}

struct AppState {
    cloaker: Arc<Mutex<Cloaker>>,
    pinned: Mutex<HashSet<isize>>,
    hud_generation: AtomicU64,
    hotkeys: Mutex<Vec<HotkeyStatus>>,
}

impl AppState {
    fn cloaker(&self) -> MutexGuard<'_, Cloaker> {
        lock(&self.cloaker)
    }
}

/// A panic elsewhere must never stop us from restoring windows, so ignore poisoning.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[derive(Serialize)]
struct WindowDto {
    /// Sent as a string: JS numbers cannot hold every 64-bit handle.
    hwnd: String,
    title: String,
    process: String,
    pid: u32,
    hidden: bool,
    pinned: bool,
}

#[derive(Serialize, Clone)]
struct HotkeyStatus {
    action: &'static str,
    keys: &'static str,
    registered: bool,
}

fn parse_hwnd(hwnd: &str) -> Result<isize, String> {
    hwnd.parse().map_err(|_| format!("invalid window handle '{hwnd}'"))
}

// ---------------------------------------------------------------- commands

#[tauri::command]
fn windows(state: State<AppState>) -> Vec<WindowDto> {
    let mut cloaker = state.cloaker();
    cloaker.prune();
    let mut pinned = lock(&state.pinned);
    pinned.retain(|&hwnd| is_window(hwnd));
    list_windows(Some(std::process::id()))
        .into_iter()
        .map(|w| WindowDto {
            hwnd: w.hwnd.to_string(),
            hidden: cloaker.is_hidden(w.hwnd),
            pinned: pinned.contains(&w.hwnd),
            title: w.title,
            process: w.process,
            pid: w.pid,
        })
        .collect()
}

#[tauri::command]
fn set_hidden(app: AppHandle, state: State<AppState>, hwnd: String, hidden: bool) -> Result<(), String> {
    let hwnd = parse_hwnd(&hwnd)?;
    let result = {
        let mut cloaker = state.cloaker();
        if hidden { cloaker.hide(hwnd) } else { cloaker.show(hwnd) }
    };
    update_tray(&app);
    result.map_err(|error| error.to_string())
}

#[tauri::command]
fn set_pinned(state: State<AppState>, hwnd: String, pinned: bool) -> Result<(), String> {
    let hwnd = parse_hwnd(&hwnd)?;
    let mut set = lock(&state.pinned);
    if pinned {
        set.insert(hwnd);
    } else {
        set.remove(&hwnd);
    }
    Ok(())
}

#[tauri::command]
fn show_all(app: AppHandle) -> usize {
    restore_everything(&app)
}

#[tauri::command]
fn hotkeys(state: State<AppState>) -> Vec<HotkeyStatus> {
    lock(&state.hotkeys).clone()
}

/// The window's icon as a `data:` URL, or `None` if it has none.
#[tauri::command]
fn window_icon(hwnd: String) -> Option<String> {
    const SIZE: u32 = 32;
    let rgba = icon_rgba(hwnd.parse().ok()?, SIZE)?;
    let mut png_bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut png_bytes, SIZE, SIZE);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.write_header().ok()?.write_image_data(&rgba).ok()?;
    }
    let encoded = base64::engine::general_purpose::STANDARD.encode(png_bytes);
    Some(format!("data:image/png;base64,{encoded}"))
}

// ----------------------------------------------------------------- actions

/// Hotkey / tray action: hide every pinned window, or show them if any is hidden.
fn toggle_pinned(app: &AppHandle) {
    let state = app.state::<AppState>();
    let targets: Vec<isize> = {
        let mut pinned = lock(&state.pinned);
        pinned.retain(|&hwnd| is_window(hwnd));
        pinned.iter().copied().collect()
    };
    let message = if targets.is_empty() {
        "No pinned windows".to_string()
    } else {
        let mut cloaker = state.cloaker();
        let hide = !targets.iter().any(|&hwnd| cloaker.is_hidden(hwnd));
        let done = targets
            .iter()
            .filter(|&&hwnd| if hide { cloaker.hide(hwnd) } else { cloaker.show(hwnd) }.is_ok())
            .count();
        let verb = if hide { "Hidden" } else { "Showing" };
        match targets.len() - done {
            0 => format!("{verb}: {done} window{}", plural(done)),
            failed => format!("{verb}: {done} window{}, {failed} failed", plural(done)),
        }
    };
    after_change(app, &message);
}

fn restore_everything(app: &AppHandle) -> usize {
    let restored = app.state::<AppState>().cloaker().restore_all();
    after_change(app, &format!("Showing everything ({restored} restored)"));
    restored
}

fn plural(count: usize) -> &'static str {
    if count == 1 { "" } else { "s" }
}

fn after_change(app: &AppHandle, message: &str) {
    update_tray(app);
    let _ = app.emit("state-changed", ());
    show_hud(app, message);
}

fn update_tray(app: &AppHandle) {
    let hidden = app.state::<AppState>().cloaker().hidden_count();
    if let Some(tray) = app.tray_by_id("main") {
        let _ = tray.set_icon(Some(art::tray_icon(hidden > 0)));
        let _ = tray.set_tooltip(Some(match hidden {
            0 => "Hidely — nothing hidden".to_string(),
            n => format!("Hidely — {n} window{} hidden from capture", plural(n)),
        }));
    }
}

/// Briefly show a message near the bottom of the screen without taking focus.
/// The HUD itself is excluded from capture, so viewers of a share never see it.
fn show_hud(app: &AppHandle, message: &str) {
    let Some(hud) = app.get_webview_window("hud") else { return };
    let Ok(handle) = hud.hwnd() else { return };
    let hwnd = handle.0 as isize;
    let _ = hud.emit("hud", message);

    if let (Ok(Some(monitor)), Ok(size)) = (hud.primary_monitor(), hud.outer_size()) {
        let screen = monitor.size();
        let origin = monitor.position();
        let x = origin.x + (screen.width as i32 - size.width as i32) / 2;
        let y = origin.y + screen.height as i32 - size.height as i32 - (screen.height as i32 / 8);
        let _ = hud.set_position(PhysicalPosition::new(x, y));
    }
    overlay::show_passive(hwnd);

    let state = app.state::<AppState>();
    let generation = state.hud_generation.fetch_add(1, Ordering::SeqCst) + 1;
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(HUD_MILLIS));
        // Only hide if no newer message has been shown in the meantime.
        if app.state::<AppState>().hud_generation.load(Ordering::SeqCst) == generation {
            overlay::hide(hwnd);
        }
    });
}

fn open_main(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

// ------------------------------------------------------------------- setup

fn setup(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let handle = app.handle().clone();

    // HUD window: created hidden, shown on demand by `show_hud`.
    let hud = WebviewWindowBuilder::new(app, "hud", WebviewUrl::App("hud.html".into()))
        .title("Hidely HUD")
        .inner_size(340.0, 56.0)
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .resizable(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .focused(false)
        .visible(false)
        .content_protected(true)
        .build()?;
    overlay::make_passive(hud.hwnd()?.0 as isize);

    // Tray icon and menu.
    let menu = Menu::with_items(
        app,
        &[
            &MenuItem::with_id(app, "open", "Open Hidely", true, None::<&str>)?,
            &MenuItem::with_id(app, "toggle", format!("Hide / show pinned\t{TOGGLE_LABEL}"), true, None::<&str>)?,
            &MenuItem::with_id(app, "show_all", format!("Show everything\t{PANIC_LABEL}"), true, None::<&str>)?,
            &MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?,
        ],
    )?;
    TrayIconBuilder::with_id("main")
        .icon(art::tray_icon(false))
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "open" => open_main(app),
            "toggle" => toggle_pinned(app),
            "show_all" => {
                restore_everything(app);
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                open_main(tray.app_handle());
            }
        })
        .build(app)?;
    update_tray(&handle);

    // Global hotkeys. Registration fails if another app already owns the combination;
    // the UI reports that instead of the app refusing to start.
    handle.plugin(
        tauri_plugin_global_shortcut::Builder::new()
            .with_handler(|app, shortcut, event| {
                if event.state() != ShortcutState::Pressed {
                    return;
                }
                if shortcut == &toggle_shortcut() {
                    toggle_pinned(app);
                } else if shortcut == &panic_shortcut() {
                    restore_everything(app);
                }
            })
            .build(),
    )?;
    let shortcuts = app.global_shortcut();
    let status = vec![
        HotkeyStatus {
            action: "Hide / show pinned windows",
            keys: TOGGLE_LABEL,
            registered: shortcuts.register(toggle_shortcut()).is_ok(),
        },
        HotkeyStatus {
            action: "Show everything",
            keys: PANIC_LABEL,
            registered: shortcuts.register(panic_shortcut()).is_ok(),
        },
    ];
    *lock(&app.state::<AppState>().hotkeys) = status;
    Ok(())
}

fn main() {
    let cloaker = Arc::new(Mutex::new(Cloaker::new()));
    // Windows left hidden by a previous run that was killed: put them back first.
    lock(&cloaker).restore_all();

    // If we crash, un-hide everything on the way down.
    let on_panic = Arc::clone(&cloaker);
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        if let Ok(mut cloaker) = on_panic.try_lock() {
            cloaker.restore_all();
        }
        default_hook(info);
    }));

    let app = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| open_main(app)))
        .manage(AppState {
            cloaker: Arc::clone(&cloaker),
            pinned: Mutex::new(HashSet::new()),
            hud_generation: AtomicU64::new(0),
            hotkeys: Mutex::new(Vec::new()),
        })
        .invoke_handler(tauri::generate_handler![
            windows, set_hidden, set_pinned, show_all, hotkeys, window_icon
        ])
        .on_window_event(|window, event| {
            // Closing the main window sends it to the tray instead of quitting.
            if let WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == "main" {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .setup(setup)
        .build(tauri::generate_context!())
        .expect("failed to start Hidely");

    app.run(move |_app, event| {
        if let RunEvent::Exit = event {
            lock(&cloaker).restore_all();
        }
    });
}
