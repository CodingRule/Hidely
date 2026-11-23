#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod art;

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use art::TrayLook;
use base64::Engine as _;
use hidely_core::engine::{EndReport, Engine, Source, Status, Summary, WATCH_INTERVAL_MS};
use hidely_core::model::{AppRule, Profile, Settings, Shortcuts, DEFAULT_RESTORE, DEFAULT_TOGGLE};
use hidely_core::{
    data_dir, elevation, icon_rgba, list_windows, os_version, overlay, pointer, Cloaker, WindowInfo,
};
use serde::Serialize;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{
    AppHandle, Emitter, Manager, PhysicalPosition, RunEvent, State, WebviewUrl,
    WebviewWindowBuilder, WindowEvent, Wry,
};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};

const HUD_MILLIS: u64 = 1600;

/// The two global shortcuts.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Action {
    /// Start a privacy session, or end the running one.
    Toggle,
    /// Make every protected window capturable again.
    Restore,
}

impl Action {
    fn parse(name: &str) -> Result<Self, String> {
        match name {
            "toggle" => Ok(Self::Toggle),
            "restore" => Ok(Self::Restore),
            _ => Err(format!("unknown shortcut '{name}'")),
        }
    }

    fn id(self) -> &'static str {
        match self {
            Self::Toggle => "toggle",
            Self::Restore => "restore",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Toggle => "Start or end the privacy session",
            Self::Restore => "Make protected windows capturable again",
        }
    }

    fn default_keys(self) -> &'static str {
        match self {
            Self::Toggle => DEFAULT_TOGGLE,
            Self::Restore => DEFAULT_RESTORE,
        }
    }
}

/// A shortcut as it is right now: what the user chose, and whether Windows gave it to us.
#[derive(Clone, Default)]
struct Binding {
    keys: String,
    shortcut: Option<Shortcut>,
    registered: bool,
}

#[derive(Default)]
struct Bindings {
    toggle: Binding,
    restore: Binding,
}

impl Bindings {
    fn get(&self, action: Action) -> &Binding {
        match action {
            Action::Toggle => &self.toggle,
            Action::Restore => &self.restore,
        }
    }

    fn get_mut(&mut self, action: Action) -> &mut Binding {
        match action {
            Action::Toggle => &mut self.toggle,
            Action::Restore => &mut self.restore,
        }
    }
}

struct TrayItems {
    status: MenuItem<Wry>,
    toggle: MenuItem<Wry>,
    restore: MenuItem<Wry>,
}

struct AppState {
    engine: Arc<Mutex<Engine>>,
    settings: Mutex<Settings>,
    settings_path: PathBuf,
    /// Things the user should be told once, such as a recovery after a crash.
    notices: Mutex<Vec<String>>,
    bindings: Mutex<Bindings>,
    tray: Mutex<Option<TrayItems>>,
    hud_generation: AtomicU64,
}

impl AppState {
    fn engine(&self) -> MutexGuard<'_, Engine> {
        lock(&self.engine)
    }

    fn save_settings(&self, settings: &Settings) -> Result<(), String> {
        settings.save(&self.settings_path)
    }
}

/// A panic elsewhere must never stop us from restoring windows, so ignore poisoning.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn parse_hwnd(hwnd: &str) -> Result<isize, String> {
    hwnd.parse().map_err(|_| format!("invalid window handle '{hwnd}'"))
}

fn plural(count: usize) -> &'static str {
    if count == 1 { "" } else { "s" }
}

// -------------------------------------------------------------- data for the UI

#[derive(Serialize)]
struct WindowDto {
    /// Sent as a string: JS numbers cannot hold every 64-bit handle.
    hwnd: String,
    title: String,
    process: String,
    exe: String,
    path: String,
    status: Status,
    detail: String,
    /// Hidely changed this window and will restore it.
    owned: bool,
    pinned: bool,
    /// This is Hidely's own window.
    own: bool,
}

#[derive(Serialize)]
struct ShortcutDto {
    action: &'static str,
    label: &'static str,
    keys: String,
    default_keys: &'static str,
    registered: bool,
}

/// Everything the window needs to draw itself, from the one model.
#[derive(Serialize)]
struct Snapshot {
    windows: Vec<WindowDto>,
    summary: Summary,
    last_end: Option<EndReport>,
    profiles: Vec<Profile>,
    selected_profile: Option<String>,
    shortcuts: Vec<ShortcutDto>,
    notices: Vec<String>,
    watch_interval_ms: u64,
    /// Hidely is running as administrator, so it can reach apps that do too.
    elevated: bool,
}

#[derive(Serialize)]
struct PreviewApp {
    exe: String,
    /// Open windows this rule matches right now.
    windows: usize,
    path_restricted: bool,
}

/// What starting a session would do, for the user to read before agreeing.
#[derive(Serialize)]
struct SessionPreview {
    profile_name: Option<String>,
    apps: Vec<PreviewApp>,
    /// Programs in the profile that are not running. They are not protected until opened.
    missing: Vec<String>,
    pinned: usize,
    watch_interval_ms: u64,
}

fn own_window(app: &AppHandle) -> Option<WindowInfo> {
    let hwnd = app.get_webview_window("main")?.hwnd().ok()?;
    Some(WindowInfo {
        hwnd: hwnd.0 as isize,
        title: "Hidely (this window)".to_string(),
        pid: std::process::id(),
        process: "Hidely".to_string(),
        exe: "hidely.exe".to_string(),
        path: String::new(),
    })
}

/// Every window the user can act on: Hidely's own first, then everyone else's.
fn all_windows(app: &AppHandle) -> Vec<WindowInfo> {
    own_window(app).into_iter().chain(list_windows(Some(std::process::id()))).collect()
}

fn shortcut_dtos(bindings: &Bindings) -> Vec<ShortcutDto> {
    [Action::Toggle, Action::Restore]
        .into_iter()
        .map(|action| {
            let binding = bindings.get(action);
            ShortcutDto {
                action: action.id(),
                label: action.label(),
                keys: binding.keys.clone(),
                default_keys: action.default_keys(),
                registered: binding.registered,
            }
        })
        .collect()
}

// ---------------------------------------------------------------- commands

#[tauri::command(async)]
fn snapshot(app: AppHandle, state: State<AppState>) -> Snapshot {
    let windows = all_windows(&app);
    let own = std::process::id();
    let (views, summary, last_end) = {
        let mut engine = state.engine();
        let views = engine.views(windows);
        (views, engine.summary(), engine.last_end().cloned())
    };
    let settings = lock(&state.settings);
    Snapshot {
        windows: views
            .into_iter()
            .map(|view| WindowDto {
                hwnd: view.info.hwnd.to_string(),
                own: view.info.pid == own,
                title: view.info.title,
                process: view.info.process,
                exe: view.info.exe,
                path: view.info.path,
                status: view.status,
                detail: view.detail,
                owned: view.owned,
                pinned: view.pinned,
            })
            .collect(),
        summary,
        last_end,
        profiles: settings.profiles.clone(),
        selected_profile: settings.selected_profile.clone(),
        shortcuts: shortcut_dtos(&lock(&state.bindings)),
        notices: lock(&state.notices).clone(),
        watch_interval_ms: WATCH_INTERVAL_MS,
        elevated: elevation::is_elevated(),
    }
}

#[tauri::command(async)]
fn dismiss_notices(state: State<AppState>) {
    lock(&state.notices).clear();
}

/// Protect one window, or make it capturable again. The outcome is in the next snapshot.
#[tauri::command(async)]
fn set_protected(app: AppHandle, hwnd: String, protected: bool) -> Result<(), String> {
    let hwnd = parse_hwnd(&hwnd)?;
    apply_one(&app, hwnd, protected, Source::Manual);
    refresh(&app);
    Ok(())
}

#[tauri::command(async)]
fn set_pinned(state: State<AppState>, hwnd: String, pinned: bool) -> Result<(), String> {
    state.engine().set_pinned(parse_hwnd(&hwnd)?, pinned);
    Ok(())
}

/// What starting a session with the selected profile and the pinned windows would affect.
#[tauri::command(async)]
fn session_preview(app: AppHandle, state: State<AppState>) -> SessionPreview {
    let profile = lock(&state.settings).selected().cloned();
    let windows = list_windows(Some(std::process::id()));
    let mut apps = Vec::new();
    let mut missing = Vec::new();
    for rule in profile.iter().flat_map(|profile| &profile.rules) {
        let count = windows.iter().filter(|window| rule.matches(window)).count();
        if count == 0 {
            missing.push(rule.exe.clone());
        } else {
            apps.push(PreviewApp {
                exe: rule.exe.clone(),
                windows: count,
                path_restricted: rule.path.is_some(),
            });
        }
    }
    let open = all_windows(&app);
    let pinned = state
        .engine()
        .pinned()
        .into_iter()
        .filter(|hwnd| open.iter().any(|window| window.hwnd == *hwnd))
        .count();
    SessionPreview {
        profile_name: profile.map(|profile| profile.name),
        apps,
        missing,
        pinned,
        watch_interval_ms: WATCH_INTERVAL_MS,
    }
}

#[tauri::command(async)]
fn start_session(app: AppHandle) -> Result<(), String> {
    let message = start_session_action(&app)?;
    refresh(&app);
    show_hud(&app, &message);
    Ok(())
}

#[tauri::command(async)]
fn end_session(app: AppHandle) {
    let message = end_session_action(&app);
    refresh(&app);
    show_hud(&app, &message);
}

#[tauri::command(async)]
fn save_profile(
    state: State<AppState>,
    id: Option<String>,
    name: String,
    rules: Vec<AppRule>,
) -> Result<String, String> {
    edit_settings(&state, |settings| settings.upsert_profile(id.as_deref(), &name, rules))
}

#[tauri::command(async)]
fn duplicate_profile(state: State<AppState>, id: String) -> Result<String, String> {
    edit_settings(&state, |settings| settings.duplicate_profile(&id))
}

#[tauri::command(async)]
fn delete_profile(state: State<AppState>, id: String) -> Result<(), String> {
    edit_settings(&state, |settings| settings.delete_profile(&id))
}

/// Choose the profile the next session will use. This does not start anything.
#[tauri::command(async)]
fn select_profile(state: State<AppState>, id: Option<String>) -> Result<(), String> {
    edit_settings(&state, |settings| settings.select_profile(id.as_deref()))
}

/// Change settings and save them. If saving fails the change is undone, so what is on
/// screen always matches what is on disk.
fn edit_settings<T>(
    state: &AppState,
    change: impl FnOnce(&mut Settings) -> Result<T, String>,
) -> Result<T, String> {
    let mut settings = lock(&state.settings);
    let before = settings.clone();
    let result = change(&mut settings).and_then(|value| {
        state.save_settings(&settings)?;
        Ok(value)
    });
    if result.is_err() {
        *settings = before;
    }
    result
}

#[tauri::command(async)]
fn set_shortcut(app: AppHandle, action: String, keys: String) -> Result<(), String> {
    change_shortcut(&app, Action::parse(&action)?, keys.trim())?;
    refresh(&app);
    Ok(())
}

#[tauri::command(async)]
fn reset_shortcuts(app: AppHandle) -> Result<(), String> {
    let result = reset_shortcuts_action(&app);
    refresh(&app);
    result
}

/// Close Hidely and start it again as administrator, so it can protect windows of apps
/// that run as administrator. Windows asks the user to approve; if they decline, nothing
/// changes. The session is ended first, like any other quit.
#[tauri::command(async)]
fn restart_elevated(app: AppHandle) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| format!("Could not find Hidely's own program file: {e}"))?;
    // The new copy waits for this one to exit, so the two never run side by side.
    if !elevation::launch_elevated(&exe, &format!("--after {}", std::process::id())) {
        return Err("Hidely was not restarted, because the administrator prompt was declined.".into());
    }
    app.exit(0);
    Ok(())
}

/// The diagnostic report, for the user to read before deciding to save it.
#[tauri::command(async)]
fn diagnostics(state: State<AppState>) -> String {
    build_diagnostics(&state)
}

/// Save the diagnostic report to a file on this computer. Nothing is uploaded.
#[tauri::command(async)]
fn save_diagnostics(app: AppHandle, state: State<AppState>) -> Result<String, String> {
    let folder = app
        .path()
        .document_dir()
        .ok()
        .or_else(data_dir)
        .ok_or("Could not find a folder to save into.")?
        .join("Hidely");
    std::fs::create_dir_all(&folder).map_err(|e| format!("Could not create {}: {e}", folder.display()))?;
    let file = folder.join(format!("hidely-diagnostics-{}.json", unix_now()));
    std::fs::write(&file, build_diagnostics(&state))
        .map_err(|e| format!("Could not write {}: {e}", file.display()))?;
    Ok(file.display().to_string())
}

/// The window's icon as a `data:` URL, or `None` if it has none.
#[tauri::command(async)]
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

/// Change one window. The engine lock is released while Windows does the work, so the
/// window shows as "Applying" in the meantime instead of the whole app freezing.
fn apply_one(app: &AppHandle, hwnd: isize, hide: bool, source: Source) -> Status {
    let state = app.state::<AppState>();
    let ticket = state.engine().begin(hwnd, hide, source);
    let _ = app.emit("state-changed", ());
    let result = ticket.run();
    let status = state.engine().finish(&ticket, result);
    status
}

/// Start a session with the selected profile and the pinned windows.
fn start_session_action(app: &AppHandle) -> Result<String, String> {
    let state = app.state::<AppState>();
    let profile = lock(&state.settings).selected().cloned();
    let open = all_windows(app);
    let pinned: Vec<isize> = state
        .engine()
        .pinned()
        .into_iter()
        .filter(|hwnd| open.iter().any(|window| window.hwnd == *hwnd))
        .collect();
    let has_rules = profile.as_ref().is_some_and(|profile| !profile.rules.is_empty());
    if !has_rules && pinned.is_empty() {
        return Err("Nothing to protect yet. Pick a profile with apps in it, or pin a window.".into());
    }

    let rule_targets = {
        let mut engine = state.engine();
        let (name, rules) =
            profile.map_or((None, Vec::new()), |profile| (Some(profile.name), profile.rules));
        engine.start_session(name, rules);
        engine.rule_targets(&open)
    };
    let mut done = Vec::new();
    for hwnd in pinned {
        done.push((hwnd, apply_one(app, hwnd, true, Source::Manual)));
    }
    for hwnd in rule_targets {
        if !done.iter().any(|(seen, _)| *seen == hwnd) {
            done.push((hwnd, apply_one(app, hwnd, true, Source::Rule)));
        }
    }
    let applied = done.iter().filter(|(_, status)| *status == Status::Applied).count();
    let failed = done.len() - applied;
    Ok(match failed {
        0 => format!("Session started: {applied} window{} protected", plural(applied)),
        _ => format!("Session started: {applied} protected, {failed} not protected"),
    })
}

/// End the session and make everything Hidely changed capturable again.
fn end_session_action(app: &AppHandle) -> String {
    let report = app.state::<AppState>().engine().end_session();
    match report.failed {
        0 => format!("Capturable again: {} window{}", report.restored, plural(report.restored)),
        failed => format!("{} restored, {failed} still excluded from capture", report.restored),
    }
}

/// The toggle shortcut and tray item.
fn toggle_session(app: &AppHandle) {
    let active = app.state::<AppState>().engine().summary().session_active;
    let message = if active {
        end_session_action(app)
    } else {
        start_session_action(app).unwrap_or_else(|problem| problem)
    };
    refresh(app);
    show_hud(app, &message);
}

/// The restore shortcut and tray item.
fn restore_all(app: &AppHandle) {
    let message = end_session_action(app);
    refresh(app);
    show_hud(app, &message);
}

/// Tell the tray and the window that something changed.
fn refresh(app: &AppHandle) {
    update_tray(app);
    let _ = app.emit("state-changed", ());
}

fn update_tray(app: &AppHandle) {
    let state = app.state::<AppState>();
    let summary = state.engine().summary();
    let problems = summary.errors + summary.unsupported;
    let (look, text) = if !summary.session_active {
        let text = match summary.errors {
            0 => "No privacy session. Nothing is protected.".to_string(),
            n => format!("No privacy session. {n} window{} still excluded from capture.", plural(n)),
        };
        (if summary.errors > 0 { TrayLook::Attention } else { TrayLook::Idle }, text)
    } else {
        let mut text = format!(
            "Privacy session on. {} window{} protected.",
            summary.applied,
            plural(summary.applied)
        );
        if problems > 0 {
            text.push_str(&format!(" {problems} could not be protected."));
        }
        (if problems > 0 { TrayLook::Attention } else { TrayLook::Active }, text)
    };

    if let Some(tray) = app.tray_by_id("main") {
        let _ = tray.set_icon(Some(art::tray_icon(look)));
        let _ = tray.set_tooltip(Some(format!("Hidely: {text}")));
    }
    let tray = lock(&state.tray);
    if let Some(items) = tray.as_ref() {
        let bindings = lock(&state.bindings);
        let _ = items.status.set_text(&text);
        let toggle = if summary.session_active { "End privacy session" } else { "Start privacy session" };
        let _ = items.toggle.set_text(with_keys(toggle, &bindings.toggle));
        let _ = items.restore.set_text(with_keys("Make everything capturable again", &bindings.restore));
    }
}

/// A menu label with its shortcut after a tab, when the shortcut is working.
fn with_keys(label: &str, binding: &Binding) -> String {
    if binding.registered {
        format!("{label}\t{}", binding.keys.replace("Super", "Win"))
    } else {
        label.to_string()
    }
}

// --------------------------------------------------------------- shortcuts

fn parse_shortcut(keys: &str) -> Result<Shortcut, String> {
    let shortcut: Shortcut = keys
        .parse()
        .map_err(|_| format!("\"{keys}\" is not a key combination Hidely understands."))?;
    // Without Ctrl, Alt or Win this would swallow ordinary typing in every app.
    if !shortcut.mods.intersects(Modifiers::CONTROL | Modifiers::ALT | Modifiers::SUPER) {
        return Err("Include Ctrl, Alt or the Windows key, so the shortcut can't fire while you type.".into());
    }
    Ok(shortcut)
}

/// Try to take `keys` at startup. Failure is recorded, not fatal.
fn register_initial(app: &AppHandle, keys: &str) -> Binding {
    let shortcut = parse_shortcut(keys).ok();
    let registered =
        shortcut.is_some_and(|shortcut| app.global_shortcut().register(shortcut).is_ok());
    Binding { keys: keys.to_string(), shortcut, registered }
}

/// Move `action` to `keys`. If Windows refuses the new combination, the old one is put
/// back and keeps working.
fn change_shortcut(app: &AppHandle, action: Action, keys: &str) -> Result<(), String> {
    let state = app.state::<AppState>();
    let shortcut = parse_shortcut(keys)?;
    let mut bindings = lock(&state.bindings);

    let other = if action == Action::Toggle { Action::Restore } else { Action::Toggle };
    if bindings.get(other).shortcut == Some(shortcut) {
        return Err(format!("That combination is already used for \"{}\".", other.label()));
    }
    let previous = bindings.get(action).clone();
    if previous.shortcut == Some(shortcut) && previous.registered {
        return Ok(());
    }

    let shortcuts = app.global_shortcut();
    if let (Some(old), true) = (previous.shortcut, previous.registered) {
        let _ = shortcuts.unregister(old);
    }
    if shortcuts.register(shortcut).is_err() {
        // Put the old one back before reporting, so the user is never left without it.
        let restored = previous
            .shortcut
            .filter(|_| previous.registered)
            .is_some_and(|old| shortcuts.register(old).is_ok());
        bindings.get_mut(action).registered = restored;
        return Err(format!(
            "Windows or another app already uses {}. Your previous shortcut is unchanged.",
            keys.replace("Super", "Win")
        ));
    }

    *bindings.get_mut(action) = Binding { keys: keys.to_string(), shortcut: Some(shortcut), registered: true };
    let saved = edit_settings(&state, |settings| {
        match action {
            Action::Toggle => settings.shortcuts.toggle = keys.to_string(),
            Action::Restore => settings.shortcuts.restore = keys.to_string(),
        }
        Ok(())
    });
    saved.map_err(|problem| format!("The shortcut works now, but could not be saved: {problem}"))
}

fn reset_shortcuts_action(app: &AppHandle) -> Result<(), String> {
    // Free both first, so swapping back to the defaults cannot collide with itself.
    {
        let state = app.state::<AppState>();
        let mut bindings = lock(&state.bindings);
        for action in [Action::Toggle, Action::Restore] {
            let binding = bindings.get_mut(action);
            if let (Some(shortcut), true) = (binding.shortcut, binding.registered) {
                let _ = app.global_shortcut().unregister(shortcut);
            }
            *binding = Binding::default();
        }
    }
    let mut problems = Vec::new();
    for action in [Action::Toggle, Action::Restore] {
        if let Err(problem) = change_shortcut(app, action, action.default_keys()) {
            problems.push(problem);
        }
    }
    if problems.is_empty() { Ok(()) } else { Err(problems.join(" ")) }
}

// ------------------------------------------------------------- diagnostics

fn unix_now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |elapsed| elapsed.as_secs())
}

/// A report about Hidely itself. It names programs by file name and never contains
/// window titles, full paths, screenshots or anything about the network.
fn build_diagnostics(state: &AppState) -> String {
    let (major, minor, build) = os_version();
    let (summary, operations, recovery) = {
        let engine = state.engine();
        (engine.summary(), engine.log().cloned().collect::<Vec<_>>(), engine.recovery().cloned())
    };
    let bindings = lock(&state.bindings);
    let report = serde_json::json!({
        "generated_unix": unix_now(),
        "hidely_version": env!("CARGO_PKG_VERSION"),
        "windows_version": format!("{major}.{minor}.{build}"),
        "architecture": std::env::consts::ARCH,
        "running_as_administrator": elevation::is_elevated(),
        "session": {
            "active": summary.session_active,
            "watching_for_new_windows": summary.watching,
            "protection_applied": summary.applied,
            "applying": summary.applying,
            "unsupported": summary.unsupported,
            "errors": summary.errors,
        },
        "recovered_at_startup": recovery,
        "shortcuts_registered": {
            "toggle": bindings.toggle.registered,
            "restore": bindings.restore.registered,
        },
        "profile_count": lock(&state.settings).profiles.len(),
        "operations": operations,
    });
    serde_json::to_string_pretty(&report).unwrap_or_default()
}

// --------------------------------------------------------------------- HUD

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

/// While a session with profile rules is running, protect matching windows as they open.
fn spawn_watcher(app: AppHandle) {
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_millis(WATCH_INTERVAL_MS));
        let state = app.state::<AppState>();
        if !state.engine().summary().watching {
            continue;
        }
        let windows = list_windows(Some(std::process::id()));
        // Asking again under the lock means a session that just ended yields nothing.
        let targets = state.engine().rule_targets(&windows);
        if targets.is_empty() {
            continue;
        }
        for hwnd in targets {
            apply_one(&app, hwnd, true, Source::Rule);
        }
        refresh(&app);
    });
}

/// Screen captures show the pointer even over a window they leave out. While it is over a
/// window Hidely is protecting, draw it as a plain arrow, so a text bar or a hand cannot
/// give the window away. Everywhere else the user keeps their normal pointers.
fn spawn_pointer_guard(app: AppHandle) {
    const CHECK_MILLIS: u64 = 20;
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_millis(CHECK_MILLIS));
        let over_protected = pointer::window_under_pointer()
            .is_some_and(|hwnd| app.state::<AppState>().engine().is_protecting(hwnd));
        pointer::set_masked(over_protected);
    });
}

fn setup(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let handle = app.handle().clone();

    // HUD window: created hidden, shown on demand by `show_hud`.
    let hud = WebviewWindowBuilder::new(app, "hud", WebviewUrl::App("hud.html".into()))
        .title("Hidely HUD")
        .inner_size(380.0, 56.0)
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

    // Global shortcuts. Registration fails if another app already owns the combination;
    // the UI reports that instead of the app refusing to start.
    handle.plugin(
        tauri_plugin_global_shortcut::Builder::new()
            .with_handler(|app, shortcut, event| {
                if event.state() != ShortcutState::Pressed {
                    return;
                }
                let action = {
                    let state = app.state::<AppState>();
                    let bindings = lock(&state.bindings);
                    if bindings.toggle.shortcut.as_ref() == Some(shortcut) {
                        Some(Action::Toggle)
                    } else if bindings.restore.shortcut.as_ref() == Some(shortcut) {
                        Some(Action::Restore)
                    } else {
                        None
                    }
                };
                match action {
                    Some(Action::Toggle) => toggle_session(app),
                    Some(Action::Restore) => restore_all(app),
                    None => {}
                }
            })
            .build(),
    )?;
    let state = app.state::<AppState>();
    let chosen: Shortcuts = lock(&state.settings).shortcuts.clone();
    *lock(&state.bindings) = Bindings {
        toggle: register_initial(&handle, &chosen.toggle),
        restore: register_initial(&handle, &chosen.restore),
    };

    // Tray icon and menu. Everything here works with the main window closed.
    let status = MenuItem::with_id(app, "status", "", false, None::<&str>)?;
    let toggle = MenuItem::with_id(app, "toggle", "Start privacy session", true, None::<&str>)?;
    let restore = MenuItem::with_id(app, "restore", "Make everything capturable again", true, None::<&str>)?;
    let menu = Menu::with_items(
        app,
        &[
            &status,
            &PredefinedMenuItem::separator(app)?,
            &MenuItem::with_id(app, "open", "Open Hidely", true, None::<&str>)?,
            &toggle,
            &restore,
            &PredefinedMenuItem::separator(app)?,
            &MenuItem::with_id(app, "reset_shortcuts", "Restore default shortcuts", true, None::<&str>)?,
            &MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?,
        ],
    )?;
    TrayIconBuilder::with_id("main")
        .icon(art::tray_icon(TrayLook::Idle))
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "open" => open_main(app),
            "toggle" => toggle_session(app),
            "restore" => restore_all(app),
            "reset_shortcuts" => {
                let message = match reset_shortcuts_action(app) {
                    Ok(()) => "Shortcuts are back to their defaults".to_string(),
                    Err(problem) => problem,
                };
                refresh(app);
                show_hud(app, &message);
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
    *lock(&state.tray) = Some(TrayItems { status, toggle, restore });
    update_tray(&handle);

    spawn_watcher(handle.clone());
    spawn_pointer_guard(handle);
    Ok(())
}

fn main() {
    // Started by `restart_elevated`: let the copy we are replacing finish closing first.
    let arguments: Vec<String> = std::env::args().collect();
    if let Some(pid) = arguments
        .iter()
        .position(|argument| argument == "--after")
        .and_then(|index| arguments.get(index + 1))
        .and_then(|pid| pid.parse().ok())
    {
        elevation::wait_for_exit(pid, 15_000);
    }

    // Anything a previous run left excluded (because it was killed) is restored here,
    // before the window opens. The engine remembers how that went.
    let engine = Arc::new(Mutex::new(Engine::new(Cloaker::new())));
    // If a previous run was killed while it had the pointer masked, undo that too.
    pointer::restore();

    let settings_path = data_dir().unwrap_or_default().join("settings.json");
    let (settings, settings_warning) = Settings::load(&settings_path);
    let mut notices: Vec<String> = settings_warning.into_iter().collect();
    if let Some(recovery) = lock(&engine).recovery() {
        notices.push(match recovery.failed {
            0 => format!(
                "Hidely did not close normally last time. {} window{} it had excluded from capture \
                 {} capturable again.",
                recovery.restored,
                plural(recovery.restored),
                if recovery.restored == 1 { "is" } else { "are" },
            ),
            failed => format!(
                "Hidely did not close normally last time. {} window{} restored, but {failed} could \
                 not be and may still be excluded from capture. Closing and reopening those apps \
                 clears it.",
                recovery.restored,
                plural(recovery.restored),
            ),
        });
    }

    // If we crash, make everything capturable again on the way down.
    let on_panic = Arc::clone(&engine);
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        if let Ok(mut engine) = on_panic.try_lock() {
            engine.end_session();
        }
        pointer::restore();
        default_hook(info);
    }));

    let app = tauri::Builder::default()
        // Running Hidely again while it is open hands the arguments to the running copy,
        // so `Hidely.exe --toggle-session` works from a script, a shortcut or a stream deck.
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            if args.iter().any(|arg| arg == "--toggle-session") {
                toggle_session(app);
            } else if args.iter().any(|arg| arg == "--end-session") {
                restore_all(app);
            } else {
                open_main(app);
            }
        }))
        .manage(AppState {
            engine: Arc::clone(&engine),
            settings: Mutex::new(settings),
            settings_path,
            notices: Mutex::new(notices),
            bindings: Mutex::new(Bindings::default()),
            tray: Mutex::new(None),
            hud_generation: AtomicU64::new(0),
        })
        .invoke_handler(tauri::generate_handler![
            snapshot,
            dismiss_notices,
            set_protected,
            set_pinned,
            session_preview,
            start_session,
            end_session,
            save_profile,
            duplicate_profile,
            delete_profile,
            select_profile,
            set_shortcut,
            reset_shortcuts,
            restart_elevated,
            diagnostics,
            save_diagnostics,
            window_icon
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
            // Windows that cannot be restored stay in the journal for the next launch.
            lock(&engine).end_session();
            pointer::restore();
        }
    });
}
