use std::ffi::c_void;
use std::mem::{size_of, zeroed};
use std::path::Path;
use std::ptr::null_mut;

use windows_sys::Win32::Foundation::{CloseHandle, BOOL, HWND, LPARAM};
use windows_sys::Win32::Graphics::Dwm::DwmGetWindowAttribute;
use windows_sys::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, SelectObject, BITMAPINFO,
    BITMAPINFOHEADER,
};
use windows_sys::Win32::System::Threading::{OpenProcess, QueryFullProcessImageNameW};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    DrawIconEx, EnumWindows, GetClassLongPtrW, GetWindow, GetWindowDisplayAffinity,
    GetWindowLongPtrW, GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId, IsWindow,
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
    /// Executable name without extension, e.g. `notepad`.
    pub process: String,
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
            Some(WindowInfo { hwnd, title, pid, process: process_name(pid) })
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

fn process_name(pid: u32) -> String {
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle.is_null() {
            return format!("pid {pid}");
        }
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(handle, 0, buf.as_mut_ptr(), &mut len);
        CloseHandle(handle);
        if ok == 0 {
            return format!("pid {pid}");
        }
        let path = String::from_utf16_lossy(&buf[..len as usize]);
        Path::new(&path)
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or(path)
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
