#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod art;
mod native_host;

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use art::TrayLook;
use base64::Engine as _;
use hidely_core::capture::{capture_screen, pick_monitor};
use hidely_core::engine::{Engine, Lost, SessionReport, Source, Status, Summary, WATCH_INTERVAL_MS};
use hidely_core::meeting::{MeetingOffer, MeetingWatch, MEETING_SCAN_MS};
use hidely_core::model::{AppRule, Profile, Settings, Shortcuts, DEFAULT_RESTORE, DEFAULT_TOGGLE};
use hidely_core::{
    data_dir, display_affinity, elevation, icon_rgba, list_windows, os_version, overlay, pointer,
    process_start_time, system, window_rect, window_title, Cloaker, WindowInfo,
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
/// Problems stay up longer, so they can be read before they go.
const HUD_PROBLEM_MILLIS: u64 = 4000;
/// One reminder per session, after this long, that it is still on.
const STILL_ON_SECS: u64 = 2 * 60 * 60;
const TEST_PAGE_URL: &str = "https://codingrule.github.io/Hidely/#try";
/// "What viewers see" frames: default and allowed widths. Small frames are cheap to send and
/// keep text unreadable, which suits a privacy tool.
const VIEWER_WIDTH: u32 = 640;
const VIEWER_MIN_WIDTH: u32 = 320;
const VIEWER_MAX_WIDTH: u32 = 960;
/// The name of Hidely's entry in the list of programs that start with Windows.
const STARTUP_NAME: &str = "Hidely";
const RELEASES_URL: &str = "https://github.com/CodingRule/Hidely/releases/latest";

/// How browsers find Hidely: a registry key per browser pointing at a small description file.
const HOST_NAME: &str = "io.github.codingrule.hidely";
/// The extensions that may talk to Hidely: the copy loaded from source (its ID is fixed by
/// the key in its manifest) and the copy installed from the Chrome Web Store.
const EXTENSION_ORIGINS: [&str; 2] = [
    "chrome-extension://mnihmjkcfncphonkdphhcjojghfbmnae/",
    "chrome-extension://kifbmljgdnggophdknoliofdafghkgcg/",
];
const BROWSER_KEYS: [&str; 2] = [
    r"Software\Google\Chrome\NativeMessagingHosts\io.github.codingrule.hidely",
    r"Software\Microsoft\Edge\NativeMessagingHosts\io.github.codingrule.hidely",
];

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
    meeting: Mutex<MeetingWatch>,
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
    /// When Windows last confirmed this window is excluded from capture (seconds since
    /// 1970), or null if it never has.
    verified_unix: Option<u64>,
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
    last_end: Option<SessionReport>,
    profiles: Vec<Profile>,
    selected_profile: Option<String>,
    shortcuts: Vec<ShortcutDto>,
    notices: Vec<String>,
    watch_interval_ms: u64,
    /// Hidely is running as administrator, so it can reach apps that do too.
    elevated: bool,
    version: &'static str,
    starts_with_windows: bool,
    check_updates_at_startup: bool,
    /// Chrome and Edge have been told where to find Hidely for the browser extension.
    browsers_connected: bool,
    /// The first-run tour was finished or skipped.
    first_run_done: bool,
    /// Offer a privacy session when a meeting app opens.
    meeting_detection: bool,
    /// A meeting app that opened and is waiting for the user to start a session or say no.
    meeting_offer: Option<MeetingOffer>,
}

#[derive(Clone, Serialize)]
struct ProtectionLost {
    hwnd: String,
    app: String,
    reapplied: bool,
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
                verified_unix: view.verified_unix,
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
        version: env!("CARGO_PKG_VERSION"),
        starts_with_windows: system::starts_with_windows(STARTUP_NAME),
        check_updates_at_startup: settings.check_updates_at_startup,
        browsers_connected: browsers_connected(),
        first_run_done: settings.first_run_done,
        meeting_detection: settings.meeting_detection,
        meeting_offer: settings
            .meeting_detection
            .then(|| lock(&state.meeting).offer().cloned())
            .flatten(),
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
    let note = start_session_action(&app)?;
    refresh(&app);
    show_note(&app, &note);
    Ok(())
}

#[tauri::command(async)]
fn end_session(app: AppHandle) {
    let note = end_session_action(&app);
    refresh(&app);
    show_note(&app, &note);
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

fn host_description_path() -> Option<PathBuf> {
    data_dir().map(|dir| dir.join("browser-host.json"))
}

/// Whether Chrome and Edge currently point at this copy of Hidely.
fn browsers_connected() -> bool {
    let Some(path) = host_description_path() else { return false };
    let expected = path.display().to_string();
    path.exists()
        && BROWSER_KEYS
            .iter()
            .all(|key| system::user_key_default(key).as_deref() == Some(expected.as_str()))
}

/// Let the Hidely browser extension talk to this copy of Hidely, or stop letting it.
///
/// Connecting writes a small file that names this program and the one extension allowed to
/// use it, and tells Chrome and Edge where that file is. Nothing is installed in the
/// browser itself.
#[tauri::command(async)]
fn set_browsers_connected(enabled: bool) -> Result<(), String> {
    let path = host_description_path().ok_or("Could not find Hidely's data folder.")?;
    let refused = |code: u32| format!("Windows would not update the browser settings (error {code}).");
    if !enabled {
        for key in BROWSER_KEYS {
            system::set_user_key_default(key, None).map_err(refused)?;
        }
        let _ = std::fs::remove_file(&path);
        return Ok(());
    }
    let exe = std::env::current_exe().map_err(|e| format!("Could not find Hidely's own program file: {e}"))?;
    let description = serde_json::json!({
        "name": HOST_NAME,
        "description": "Hidely: keeps browser windows with private tabs out of screen capture",
        "path": exe,
        "type": "stdio",
        "allowed_origins": EXTENSION_ORIGINS,
    });
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("Could not create {}: {e}", dir.display()))?;
    }
    std::fs::write(&path, serde_json::to_string_pretty(&description).unwrap_or_default())
        .map_err(|e| format!("Could not write {}: {e}", path.display()))?;
    for key in BROWSER_KEYS {
        system::set_user_key_default(key, Some(&path.display().to_string())).map_err(refused)?;
    }
    Ok(())
}

/// A request from the browser extension, passed on by the copy of Hidely the browser
/// started. Only browser windows are accepted, and the extension can only release
/// windows it protected itself.
fn extension_request(app: &AppHandle, protect: bool, hwnd: isize) {
    let is_browser = hidely_core::owner_pid(hwnd)
        .map(|pid| hidely_core::process_identity(pid).exe)
        .is_some_and(|exe| native_host::BROWSERS.contains(&exe.as_str()));
    if !is_browser {
        return;
    }
    let app = app.clone();
    // Off the main thread: changing a window can take a moment.
    std::thread::spawn(move || {
        if protect {
            let already = app.state::<AppState>().engine().is_protecting(hwnd);
            if !already {
                apply_one(&app, hwnd, true, Source::Extension);
            }
        } else if app.state::<AppState>().engine().is_by_extension(hwnd) {
            apply_one(&app, hwnd, false, Source::Extension);
        }
        refresh(&app);
    });
}

/// The window handle after `flag` in a list of arguments, if there is one.
fn window_argument(args: &[String], flag: &str) -> Option<isize> {
    let index = args.iter().position(|argument| argument == flag)?;
    args.get(index + 1)?.parse().ok()
}

/// Start Hidely in the tray when the user signs in to Windows, or stop doing so.
#[tauri::command(async)]
fn set_start_with_windows(enabled: bool) -> Result<(), String> {
    let command = if enabled {
        let exe = std::env::current_exe().map_err(|e| format!("Could not find Hidely's own program file: {e}"))?;
        Some(format!("\"{}\" --minimized", exe.display()))
    } else {
        None
    };
    system::set_starts_with_windows(STARTUP_NAME, command.as_deref())
        .map_err(|code| format!("Windows would not change the startup list (error {code})."))
}

/// Choose whether Hidely asks GitHub for the latest version when it starts.
#[tauri::command(async)]
fn set_check_updates(state: State<AppState>, enabled: bool) -> Result<(), String> {
    edit_settings(&state, |settings| {
        settings.check_updates_at_startup = enabled;
        Ok(())
    })
}

/// Record that the first-run tour was finished or skipped (true), or ask for it again (false).
#[tauri::command(async)]
fn set_first_run_done(state: State<AppState>, done: bool) -> Result<(), String> {
    edit_settings(&state, |settings| {
        settings.first_run_done = done;
        Ok(())
    })
}

/// Turn the meeting-app offer on or off. Turning it off also withdraws a waiting offer, and
/// turning it back on starts fresh, so apps already open then are not offered.
#[tauri::command(async)]
fn set_meeting_detection(app: AppHandle, enabled: bool) -> Result<(), String> {
    let state = app.state::<AppState>();
    edit_settings(&state, |settings| {
        settings.meeting_detection = enabled;
        Ok(())
    })?;
    *lock(&state.meeting) = MeetingWatch::default();
    refresh(&app);
    Ok(())
}

/// The user said no to the meeting offer. The same run of that app will not ask again.
#[tauri::command(async)]
fn dismiss_meeting_offer(app: AppHandle) {
    lock(&app.state::<AppState>().meeting).dismiss();
    refresh(&app);
}

/// Open the page that shows whether a protected window is missing from a real screen share.
#[tauri::command(async)]
fn open_test_page() -> Result<(), String> {
    if system::open_in_browser(TEST_PAGE_URL) {
        Ok(())
    } else {
        Err(format!("Could not open your browser. The page is {TEST_PAGE_URL}"))
    }
}

/// A small picture of one monitor as Windows screen capture returns it, for "What viewers
/// see". The reply is binary: four little-endian u32 (width, height, monitor index, monitor
/// count) followed by width × height RGBA pixels.
///
/// The picture is only ever held in memory and handed to Hidely's own window. It is never
/// saved, logged or put in diagnostics, and it is refused while that window is hidden.
#[tauri::command(async)]
fn viewer_frame(
    app: AppHandle,
    monitor: Option<usize>,
    max_width: Option<u32>,
) -> Result<tauri::ipc::Response, String> {
    let main = app.get_webview_window("main").ok_or("Hidely's window is not open.")?;
    let shown = main.is_visible().unwrap_or(false) && !main.is_minimized().unwrap_or(true);
    if !shown {
        return Err("Open Hidely's window to see this.".into());
    }
    let monitors: Vec<(i32, i32, u32, u32)> = main
        .available_monitors()
        .map_err(|_| "Can't read the screen right now.")?
        .iter()
        .map(|m| (m.position().x, m.position().y, m.size().width, m.size().height))
        .collect();
    if monitors.is_empty() {
        return Err("Can't read the screen right now.".into());
    }
    let primary = main
        .primary_monitor()
        .ok()
        .flatten()
        .and_then(|p| {
            monitors.iter().position(|&(x, y, w, h)| {
                (x, y, w, h) == (p.position().x, p.position().y, p.size().width, p.size().height)
            })
        })
        .unwrap_or(0);
    let index = match monitor {
        Some(index) if index < monitors.len() => index,
        _ => {
            let protected: Vec<_> = app
                .state::<AppState>()
                .engine()
                .protected_windows()
                .into_iter()
                .filter_map(window_rect)
                .collect();
            let own = main.hwnd().ok().and_then(|hwnd| window_rect(hwnd.0 as isize));
            pick_monitor(&monitors, &protected, own, primary)
        }
    };
    let (x, y, w, h) = monitors[index];
    let width = max_width.unwrap_or(VIEWER_WIDTH).clamp(VIEWER_MIN_WIDTH, VIEWER_MAX_WIDTH);
    let frame = capture_screen((x, y, w as i32, h as i32), width)
        .ok_or("Can't read the screen right now.")?;

    let mut body = Vec::with_capacity(16 + frame.rgba.len());
    for value in [frame.width, frame.height, index as u32, monitors.len() as u32] {
        body.extend_from_slice(&value.to_le_bytes());
    }
    body.extend_from_slice(&frame.rgba);
    Ok(tauri::ipc::Response::new(body))
}

/// Whether `latest` (a release tag such as `v0.3.0`) is newer than this build.
#[tauri::command(async)]
fn is_newer(latest: String) -> bool {
    system::is_newer_version(&latest, env!("CARGO_PKG_VERSION"))
}

/// Open the download page for the latest release in the user's browser.
#[tauri::command(async)]
fn open_releases() -> Result<(), String> {
    if system::open_in_browser(RELEASES_URL) {
        Ok(())
    } else {
        Err(format!("Could not open your browser. The page is {RELEASES_URL}"))
    }
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

/// Protect one window for a particular session. Nothing happens, and `None` comes back, if
/// that session ended (or another started) in the meantime, so a late rule or re-apply can
/// never start a new session or hide a window after "Session ended".
fn apply_in_session(app: &AppHandle, hwnd: isize, source: Source, session: Option<u64>) -> Option<Status> {
    let state = app.state::<AppState>();
    let ticket = state.engine().begin_in(hwnd, source, session?)?;
    let _ = app.emit("state-changed", ());
    let result = ticket.run();
    let status = state.engine().finish(&ticket, result);
    Some(status)
}

/// A HUD message, and whether it reports a problem (problems stay up longer).
struct Note {
    text: String,
    problem: bool,
}

impl Note {
    fn info(text: impl Into<String>) -> Self {
        Self { text: text.into(), problem: false }
    }

    fn problem(text: impl Into<String>) -> Self {
        Self { text: text.into(), problem: true }
    }
}

/// Start a session with the selected profile and the pinned windows.
fn start_session_action(app: &AppHandle) -> Result<Note, String> {
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

    let (rule_targets, session) = {
        let mut engine = state.engine();
        let (name, rules) =
            profile.map_or((None, Vec::new()), |profile| (Some(profile.name), profile.rules));
        engine.start_session(name, rules);
        (engine.rule_targets(&open), engine.session_id())
    };
    // A waiting meeting offer has been answered.
    lock(&state.meeting).dismiss();
    // If the session is ended while this runs, the remaining windows are left alone.
    let mut done = Vec::new();
    for hwnd in pinned {
        if let Some(status) = apply_in_session(app, hwnd, Source::Manual, session) {
            done.push((hwnd, status));
        }
    }
    for hwnd in rule_targets {
        if !done.iter().any(|(seen, _)| *seen == hwnd) {
            if let Some(status) = apply_in_session(app, hwnd, Source::Rule, session) {
                done.push((hwnd, status));
            }
        }
    }
    let applied = done.iter().filter(|(_, status)| *status == Status::Applied).count();
    let failed = done.len() - applied;
    Ok(match failed {
        0 => Note::info(format!("Session on: {applied} window{} protected", plural(applied))),
        _ => Note::problem(format!("Session on: {applied} protected, {failed} not protected")),
    })
}

/// End the session and make everything Hidely changed capturable again.
fn end_session_action(app: &AppHandle) -> Note {
    let report = app.state::<AppState>().engine().end_session();
    match report.failed {
        0 => Note::info(format!(
            "Session ended: {} window{} capturable again",
            report.restored,
            plural(report.restored)
        )),
        failed => Note::problem(format!(
            "Session ended: {} capturable again, {failed} still hidden from capture",
            report.restored
        )),
    }
}

/// The toggle shortcut and tray item.
fn toggle_session(app: &AppHandle) {
    let active = app.state::<AppState>().engine().summary().session_active;
    let note = if active {
        end_session_action(app)
    } else {
        start_session_action(app).unwrap_or_else(Note::problem)
    };
    refresh(app);
    show_note(app, &note);
}

/// The restore shortcut and tray item.
fn restore_all(app: &AppHandle) {
    let note = end_session_action(app);
    refresh(app);
    show_note(app, &note);
}

/// Tell the tray and the window that something changed.
fn refresh(app: &AppHandle) {
    update_tray(app);
    let _ = app.emit("state-changed", ());
}

/// The tray's look and text: the worst state wins. Windows cuts tooltips at 127 characters,
/// and every one of these stays well under that.
fn tray_status(summary: &Summary) -> (TrayLook, String) {
    let problems = summary.errors + summary.unsupported;
    if !summary.session_active {
        return match summary.errors {
            0 => (TrayLook::Idle, "Hidely: off. Nothing is hidden.".to_string()),
            n => (
                TrayLook::Attention,
                format!("Hidely: off. {n} window{} still hidden from capture.", plural(n)),
            ),
        };
    }
    let hidden = summary.applied;
    let total = hidden + summary.applying + problems;
    if problems > 0 {
        (TrayLook::Attention, format!("Hidely: on. {hidden} of {total} hidden. {problems} visible to viewers."))
    } else if summary.applying > 0 {
        (TrayLook::Active, format!("Hidely: on. {hidden} of {total} hidden, {} applying.", summary.applying))
    } else if total == 0 {
        (TrayLook::Active, "Hidely: on. Nothing is hidden yet.".to_string())
    } else {
        (TrayLook::Active, format!("Hidely: on. {hidden} of {total} hidden from capture."))
    }
}

fn update_tray(app: &AppHandle) {
    let state = app.state::<AppState>();
    let summary = state.engine().summary();
    let (look, text) = tray_status(&summary);

    if let Some(tray) = app.tray_by_id("main") {
        let _ = tray.set_icon(Some(art::tray_icon(look)));
        let _ = tray.set_tooltip(Some(&text));
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
            "protection_lost": summary.protection_lost,
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

fn show_hud(app: &AppHandle, message: &str) {
    show_note(app, &Note::info(message));
}

/// Briefly show a message near the bottom of the screen without taking focus. Problems
/// get the warning look and stay up longer. The HUD itself is excluded from capture, so
/// viewers of a share never see it.
fn show_note(app: &AppHandle, note: &Note) {
    let millis = if note.problem { HUD_PROBLEM_MILLIS } else { HUD_MILLIS };
    let Some(hud) = app.get_webview_window("hud") else { return };
    let Ok(handle) = hud.hwnd() else { return };
    let hwnd = handle.0 as isize;
    let _ = hud.emit("hud", serde_json::json!({ "message": note.text, "problem": note.problem }));

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
        std::thread::sleep(Duration::from_millis(millis));
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

/// The window's title cut to 40 characters for the HUD, or the program name without one.
fn short_title(hwnd: isize, app: &str) -> String {
    let title = window_title(hwnd).filter(|title| !title.trim().is_empty());
    let name = title.unwrap_or_else(|| app.trim_end_matches(".exe").to_string());
    if name.chars().count() > 40 {
        format!("{}…", name.chars().take(39).collect::<String>())
    } else {
        name
    }
}

/// "Press Win+Shift+H" when the toggle shortcut works, otherwise a pointer to the window.
fn toggle_hint(app: &AppHandle) -> Option<String> {
    let state = app.state::<AppState>();
    let bindings = lock(&state.bindings);
    bindings.toggle.registered.then(|| bindings.toggle.keys.replace("Super", "Win"))
}

/// While a session runs: every tick, read back each protected window's setting and
/// re-protect any that lost it; when the session has profile rules, also protect matching
/// windows as they open. Every couple of seconds, look for newly opened meeting apps.
fn spawn_watcher(app: AppHandle) {
    std::thread::spawn(move || {
        let mut next_meeting_scan = Instant::now();
        let mut reminded_for: Option<u64> = None;
        loop {
            std::thread::sleep(Duration::from_millis(WATCH_INTERVAL_MS));
            let state = app.state::<AppState>();
            let summary = state.engine().summary();
            if Instant::now() >= next_meeting_scan {
                next_meeting_scan = Instant::now() + Duration::from_millis(MEETING_SCAN_MS);
                scan_meeting_apps(&app, summary.session_active);
            }
            if !summary.session_active {
                continue;
            }

            let mut changed = false;
            // A cheap read per window that sends no messages, so it is fine under the lock.
            let lost = state.engine().reverify(display_affinity, unix_now());
            for window in lost {
                changed = true;
                handle_lost(&app, window);
            }

            if summary.watching {
                let windows = list_windows(Some(std::process::id()));
                // Asking again under the lock means a session that just ended yields nothing.
                let (targets, session) = {
                    let mut engine = state.engine();
                    (engine.rule_targets(&windows), engine.session_id())
                };
                for hwnd in targets {
                    changed = true;
                    let app_name = windows.iter().find(|w| w.hwnd == hwnd).map(|w| w.exe.clone());
                    let title = short_title(hwnd, app_name.as_deref().unwrap_or_default());
                    match apply_in_session(&app, hwnd, Source::Rule, session) {
                        Some(Status::Applied) => show_hud(&app, &format!("Protected: {title}")),
                        // The session ended, or the window closed first: nothing to say.
                        None | Some(Status::Unprotected) => {}
                        Some(_) => show_note(&app, &Note::problem(format!("Not protected: {title}. Viewers can see it."))),
                    }
                }
            }

            if let Some(started) = summary.started_unix {
                if reminded_for != Some(started) && unix_now().saturating_sub(started) >= STILL_ON_SECS {
                    reminded_for = Some(started);
                    let ends = toggle_hint(&app).map_or_else(
                        || "End it from Hidely's window.".to_string(),
                        |keys| format!("{keys} ends it."),
                    );
                    show_hud(&app, &format!("Privacy session still on, 2 h. {ends}"));
                }
            }

            if changed {
                refresh(&app);
            }
        }
    });
}

/// A window Windows now reports as capturable. Re-protect it if the cap allows, then say so
/// loudly either way: it was visible to viewers, even if only for a moment.
fn handle_lost(app: &AppHandle, window: Lost) {
    let title = short_title(window.hwnd, &window.app);
    let reapplied = window.reapply
        && apply_in_session(app, window.hwnd, Source::Reverify, window.session) == Some(Status::Applied);
    let _ = app.emit(
        "protection-lost",
        ProtectionLost { hwnd: window.hwnd.to_string(), app: window.app, reapplied },
    );
    let message = if reapplied {
        format!("{title} lost protection. Protected again.")
    } else {
        format!("{title} is visible to viewers again")
    };
    show_note(app, &Note::problem(message));
}

/// Offer a privacy session when a meeting app opens. Only offers: nothing starts by itself.
fn scan_meeting_apps(app: &AppHandle, session_active: bool) {
    let state = app.state::<AppState>();
    if !lock(&state.settings).meeting_detection {
        return;
    }
    let windows = list_windows(Some(std::process::id()));
    let (had_offer, offer, has_offer) = {
        let mut watch = lock(&state.meeting);
        let had = watch.offer().is_some();
        let offer = watch.scan(&windows, process_start_time, session_active);
        (had, offer, watch.offer().is_some())
    };
    if let Some(offer) = offer {
        let _ = app.emit("meeting-app-detected", &offer);
        refresh(app);
        let message = match toggle_hint(app) {
            Some(keys) => format!("{} is open. Press {keys} to start a privacy session.", offer.process),
            None => format!("{} is open. Open Hidely to start a privacy session.", offer.process),
        };
        show_hud(app, &message);
    } else if had_offer != has_offer {
        // The offer went away (the app closed), so the banner should too.
        refresh(app);
    }
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
            &MenuItem::with_id(app, "viewer", "What viewers see", true, None::<&str>)?,
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
            "viewer" => {
                open_main(app);
                let _ = app.emit("open-viewer", ());
                refresh(app);
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
    spawn_pointer_guard(handle.clone());

    // If the browsers were connected by an older version, or Hidely has moved, rewrite the
    // description so it names this program and every extension this version accepts.
    if browsers_connected() || host_description_path().is_some_and(|path| path.exists()) {
        let _ = set_browsers_connected(true);
    }

    // The window starts hidden. Show it unless Hidely was started with Windows, in which
    // case it waits in the tray.
    if !std::env::args().any(|argument| argument == "--minimized") {
        open_main(&handle);
    }
    Ok(())
}

fn main() {
    // Started by a browser for the extension: talk to it and show nothing.
    if native_host::requested() {
        native_host::run();
        return;
    }
    // A window request with no app running has nowhere to go. Do not start the app for it.
    let launch: Vec<String> = std::env::args().collect();
    let is_window_request =
        launch.iter().any(|argument| argument == "--protect-window" || argument == "--unprotect-window");
    if is_window_request && !system::is_app_running() {
        return;
    }

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

    system::mark_app_running();

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
            } else if args.iter().any(|arg| arg == "--quit") {
                app.exit(0);
            } else if let Some(hwnd) = window_argument(&args, "--protect-window") {
                extension_request(app, true, hwnd);
            } else if let Some(hwnd) = window_argument(&args, "--unprotect-window") {
                extension_request(app, false, hwnd);
            } else {
                open_main(app);
            }
        }))
        // Remember where the main window was. The HUD is placed by code every time,
        // and whether the window is shown is decided below, so neither is restored.
        .plugin(
            tauri_plugin_window_state::Builder::default()
                .with_denylist(&["hud"])
                .with_state_flags(
                    tauri_plugin_window_state::StateFlags::all()
                        & !tauri_plugin_window_state::StateFlags::VISIBLE,
                )
                .build(),
        )
        .manage(AppState {
            engine: Arc::clone(&engine),
            settings: Mutex::new(settings),
            settings_path,
            notices: Mutex::new(notices),
            bindings: Mutex::new(Bindings::default()),
            tray: Mutex::new(None),
            hud_generation: AtomicU64::new(0),
            meeting: Mutex::new(MeetingWatch::default()),
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
            set_start_with_windows,
            set_check_updates,
            set_browsers_connected,
            is_newer,
            open_releases,
            diagnostics,
            save_diagnostics,
            window_icon,
            set_first_run_done,
            set_meeting_detection,
            dismiss_meeting_offer,
            open_test_page,
            viewer_frame
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

#[cfg(test)]
mod tests {
    use super::*;

    fn summary(active: bool, applied: usize, applying: usize, unsupported: usize, errors: usize) -> Summary {
        Summary { session_active: active, applied, applying, unsupported, errors, ..Summary::default() }
    }

    #[test]
    fn the_tray_says_what_is_true_and_the_worst_state_wins() {
        let text = |s: Summary| tray_status(&s).1;
        assert_eq!(text(summary(false, 0, 0, 0, 0)), "Hidely: off. Nothing is hidden.");
        assert_eq!(text(summary(false, 0, 0, 0, 1)), "Hidely: off. 1 window still hidden from capture.");
        assert_eq!(text(summary(true, 3, 0, 0, 0)), "Hidely: on. 3 of 3 hidden from capture.");
        assert_eq!(text(summary(true, 2, 1, 0, 0)), "Hidely: on. 2 of 3 hidden, 1 applying.");
        assert_eq!(text(summary(true, 2, 0, 1, 0)), "Hidely: on. 2 of 3 hidden. 1 visible to viewers.");
        assert_eq!(text(summary(true, 2, 1, 0, 1)), "Hidely: on. 2 of 4 hidden. 1 visible to viewers.");
        assert_eq!(text(summary(true, 0, 0, 0, 0)), "Hidely: on. Nothing is hidden yet.");
        assert!(matches!(tray_status(&summary(true, 2, 0, 1, 0)).0, TrayLook::Attention));
        assert!(matches!(tray_status(&summary(false, 0, 0, 0, 0)).0, TrayLook::Idle));

        let worst = text(summary(true, 99_999, 99_999, 99_999, 99_999));
        assert!(worst.chars().count() < 127, "Windows would cut this tooltip off");
    }
}
