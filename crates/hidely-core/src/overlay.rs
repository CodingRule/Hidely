//! Helpers for the HUD: a window that appears without taking keyboard focus.

use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetWindowLongPtrW, SetWindowLongPtrW, ShowWindow,
};

const GWL_EXSTYLE: i32 = -20;
const WS_EX_TOOLWINDOW: isize = 0x80;
const WS_EX_NOACTIVATE: isize = 0x0800_0000;
const SW_HIDE: i32 = 0;
const SW_SHOWNOACTIVATE: i32 = 4;

/// Mark a window so clicking or showing it never activates it.
pub fn make_passive(hwnd: isize) {
    unsafe {
        let style = GetWindowLongPtrW(hwnd as HWND, GWL_EXSTYLE);
        SetWindowLongPtrW(hwnd as HWND, GWL_EXSTYLE, style | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW);
    }
}

pub fn show_passive(hwnd: isize) {
    unsafe { ShowWindow(hwnd as HWND, SW_SHOWNOACTIVATE) };
}

pub fn hide(hwnd: isize) {
    unsafe { ShowWindow(hwnd as HWND, SW_HIDE) };
}
