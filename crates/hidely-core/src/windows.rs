use std::ffi::c_void;
use std::mem::{size_of, zeroed};
use std::path::Path;
use std::ptr::null_mut;

use windows_sys::Win32::Foundation::{CloseHandle, BOOL, FILETIME, HWND, LPARAM, RECT};
use windows_sys::Win32::Graphics::Dwm::DwmGetWindowAttribute;
use windows_sys::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, SelectObject, BITMAPINFO,
    BITMAPINFOHEADER,
};
use windows_sys::Win32::UI::HiDpi::GetDpiForWindow;
use windows_sys::Win32::System::Threading::{
    GetProcessTimes, OpenProcess, QueryFullProcessImageNameW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    DrawIconEx, EnumWindows, GetClassLongPtrW, GetWindow, GetWindowDisplayAffinity,
    GetWindowLongPtrW, GetWindowRect, GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId, IsWindow,
    IsWindowVisible, SendMessageTimeoutW,
};

const GWL_EXSTYLE: i32 = -20;
const GW_OWNER: u32 = 4;
const WS_EX_TOOLWINDOW: isize = 0x80;
const WS_EX_APPWINDOW: isize = 0x40000;
const DWMWA_CLOAKED: u32 = 14;
const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;

#[derive(Debug, Clone)]
pub struct WindowInfo {
    pub hwnd: isize,
    pub title: String,
    pub pid: u32,
    /// Executable name without extension, e.g. `notepad`. For display.
    pub process: String,
    /// Executable file name in lower case, e.g. `notepad.exe`. Empty if it could not be read.
    pub exe: String,
    /// Full path of the executable. Empty if it could not be read.
    pub path: String,
}

pub fn is_window(hwnd: isize) -> bool {
    unsafe { IsWindow(hwnd as HWND) != 0 }
}

/// Process that owns `hwnd`, or `None` if the window is gone.
pub fn owner_pid(hwnd: isize) -> Option<u32> {
    let mut pid = 0u32;
    let tid = unsafe { GetWindowThreadProcessId(hwnd as HWND, &mut pid) };
    (tid != 0 && pid != 0).then_some(pid)
}

/// A window's position and size on screen, as (left, top, width, height) in pixels.
pub fn window_rect(hwnd: isize) -> Option<(i32, i32, i32, i32)> {
    let mut rect = RECT { left: 0, top: 0, right: 0, bottom: 0 };
    let ok = unsafe { GetWindowRect(hwnd as HWND, &mut rect) };
    (ok != 0).then_some((rect.left, rect.top, rect.right - rect.left, rect.bottom - rect.top))
}

/// How much the window's monitor is scaled: 1.0 at 100%, 1.5 at 150%.
pub fn window_scale(hwnd: isize) -> f64 {
    match unsafe { GetDpiForWindow(hwnd as HWND) } {
        0 => 1.0,
        dpi => dpi as f64 / 96.0,
    }
}

/// Current display affinity of a window (works for other processes' windows too).
pub fn display_affinity(hwnd: isize) -> Option<u32> {
    let mut affinity = 0u32;
    let ok = unsafe { GetWindowDisplayAffinity(hwnd as HWND, &mut affinity) };
    (ok != 0).then_some(affinity)
}

/// Top-level windows a user would recognise (roughly what Alt+Tab shows).
pub fn list_windows(exclude_pid: Option<u32>) -> Vec<WindowInfo> {
    let mut handles: Vec<isize> = Vec::new();
    unsafe {
        EnumWindows(Some(collect), &mut handles as *mut Vec<isize> as LPARAM);
    }
    handles
        .into_iter()
        .filter_map(|hwnd| {
            let pid = owner_pid(hwnd)?;
            if Some(pid) == exclude_pid || !is_user_window(hwnd) {
                return None;
            }
            let title = window_title(hwnd)?;
            let identity = process_identity(pid);
            Some(WindowInfo {
                hwnd,
                title,
                pid,
                process: identity.name,
                exe: identity.exe,
                path: identity.path,
            })
        })
        .collect()
}

unsafe extern "system" fn collect(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let handles = &mut *(lparam as *mut Vec<isize>);
    handles.push(hwnd as isize);
    1
}

fn is_user_window(hwnd: isize) -> bool {
    let h = hwnd as HWND;
    unsafe {
        if IsWindowVisible(h) == 0 {
            return false;
        }
        let ex = GetWindowLongPtrW(h, GWL_EXSTYLE);
        if ex & WS_EX_TOOLWINDOW != 0 {
            return false;
        }
        if !GetWindow(h, GW_OWNER).is_null() && ex & WS_EX_APPWINDOW == 0 {
            return false;
        }
        // Suspended UWP apps and windows on other virtual desktops are "cloaked" by DWM.
        let mut cloaked = 0u32;
        let hr = DwmGetWindowAttribute(
            h,
            DWMWA_CLOAKED,
            &mut cloaked as *mut u32 as *mut c_void,
            size_of::<u32>() as u32,
        );
        hr != 0 || cloaked == 0
    }
}

fn window_title(hwnd: isize) -> Option<String> {
    let h = hwnd as HWND;
    unsafe {
        let len = GetWindowTextLengthW(h);
        if len <= 0 {
            return None;
        }
        let mut buf = vec![0u16; len as usize + 1];
        let got = GetWindowTextW(h, buf.as_mut_ptr(), buf.len() as i32);
        if got <= 0 {
            return None;
        }
        Some(String::from_utf16_lossy(&buf[..got as usize]))
    }
}

/// What we can tell about the program behind a process id.
pub struct ProcessIdentity {
    pub name: String,
    pub exe: String,
    pub path: String,
}

pub fn process_identity(pid: u32) -> ProcessIdentity {
    let unknown =
        || ProcessIdentity { name: format!("pid {pid}"), exe: String::new(), path: String::new() };
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle.is_null() {
            return unknown();
        }
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(handle, 0, buf.as_mut_ptr(), &mut len);
        CloseHandle(handle);
        if ok == 0 {
            return unknown();
        }
        let path = String::from_utf16_lossy(&buf[..len as usize]);
        let file = Path::new(&path);
        let exe = file.file_name().map(|s| s.to_string_lossy().to_lowercase()).unwrap_or_default();
        let name =
            file.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| path.clone());
        ProcessIdentity { name, exe, path }
    }
}

/// When the process started, as a Windows FILETIME value. Together with the process id
/// this identifies one run of a program even after ids are reused.
pub fn process_start_time(pid: u32) -> Option<u64> {
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle.is_null() {
            return None;
        }
        let mut times: [FILETIME; 4] = zeroed();
        let [created, exited, kernel, user] = &mut times;
        let ok = GetProcessTimes(handle, created, exited, kernel, user);
        CloseHandle(handle);
        (ok != 0).then(|| ((times[0].dwHighDateTime as u64) << 32) | times[0].dwLowDateTime as u64)
    }
}

/// The window's icon as `size`×`size` straight RGBA pixels, if it has one.
pub fn icon_rgba(hwnd: isize, size: u32) -> Option<Vec<u8>> {
    const DI_NORMAL: u32 = 3;
    let icon = find_icon(hwnd)?;
    let px = size as usize;
    unsafe {
        let dc = CreateCompatibleDC(null_mut());
        if dc.is_null() {
            return None;
        }
        let mut info: BITMAPINFO = zeroed();
        info.bmiHeader.biSize = size_of::<BITMAPINFOHEADER>() as u32;
        info.bmiHeader.biWidth = size as i32;
        info.bmiHeader.biHeight = -(size as i32); // top-down
        info.bmiHeader.biPlanes = 1;
        info.bmiHeader.biBitCount = 32;
        let mut bits: *mut c_void = null_mut();
        let bitmap = CreateDIBSection(dc, &info, 0, &mut bits, null_mut(), 0);
        if bitmap.is_null() || bits.is_null() {
            DeleteDC(dc);
            return None;
        }
        let previous = SelectObject(dc, bitmap);
        let drawn =
            DrawIconEx(dc, 0, 0, icon, size as i32, size as i32, 0, null_mut(), DI_NORMAL);
        let mut pixels = std::slice::from_raw_parts(bits as *const u8, px * px * 4).to_vec();
        SelectObject(dc, previous);
        DeleteObject(bitmap);
        DeleteDC(dc);
        if drawn == 0 {
            return None;
        }
        // BGRA -> RGBA. Icons without an alpha channel come out with alpha 0 everywhere.
        let opaque = pixels.chunks_exact(4).all(|p| p[3] == 0);
        for p in pixels.chunks_exact_mut(4) {
            p.swap(0, 2);
            if opaque {
                p[3] = 255;
            }
        }
        Some(pixels)
    }
}

fn find_icon(hwnd: isize) -> Option<*mut c_void> {
    const WM_GETICON: u32 = 0x7F;
    const SMTO_ABORTIFHUNG: u32 = 0x2;
    const GCLP_HICONSM: i32 = -34;
    const GCLP_HICON: i32 = -14;
    let h = hwnd as HWND;
    unsafe {
        // ICON_SMALL2 (2), then ICON_BIG (1). A hung app must not stall us.
        for which in [2usize, 1] {
            let mut result = 0usize;
            let sent =
                SendMessageTimeoutW(h, WM_GETICON, which, 0, SMTO_ABORTIFHUNG, 50, &mut result);
            if sent != 0 && result != 0 {
                return Some(result as *mut c_void);
            }
        }
        for index in [GCLP_HICONSM, GCLP_HICON] {
            let icon = GetClassLongPtrW(h, index);
            if icon != 0 {
                return Some(icon as *mut c_void);
            }
        }
    }
    None
}

/// The real Windows version as (major, minor, build). Unlike `GetVersionEx`, this is not
/// affected by compatibility settings.
pub fn os_version() -> (u32, u32, u32) {
    #[repr(C)]
    struct VersionInfo {
        size: u32,
        major: u32,
        minor: u32,
        build: u32,
        platform: u32,
        service_pack: [u16; 128],
    }
    #[link(name = "ntdll")]
    extern "system" {
        fn RtlGetVersion(info: *mut VersionInfo) -> i32;
    }
    unsafe {
        let mut info: VersionInfo = zeroed();
        info.size = size_of::<VersionInfo>() as u32;
        RtlGetVersion(&mut info);
        (info.major, info.minor, info.build)
    }
}
