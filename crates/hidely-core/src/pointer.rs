//! Keep the mouse pointer from giving a protected window away.
//!
//! Screen captures draw the pointer even over a window they leave out. If it turns into a
//! text bar or a hand there, viewers can tell something is under it. Apps choose their own
//! pointer shape, so the only way to stop that without reaching into them is to make
//! Windows draw every standard shape as the plain arrow for a while.
//!
//! This changes the pointer for the whole system, including what the user sees, so the
//! app only switches it on while the pointer is over a protected window.

use std::ptr::null_mut;
use std::sync::atomic::{AtomicBool, Ordering};

use windows_sys::Win32::Foundation::POINT;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CopyIcon, GetAncestor, GetCursorPos, LoadCursorW, SetSystemCursor, SystemParametersInfoW,
    WindowFromPoint,
};

/// The arrow, `OCR_NORMAL`.
const ARROW: u32 = 32512;
/// Every other standard pointer: text bar, wait, cross, up arrow, the resize arrows,
/// move, not-allowed, hand, working-in-background and help.
const SHAPES: [u32; 13] =
    [32513, 32514, 32515, 32516, 32642, 32643, 32644, 32645, 32646, 32648, 32649, 32650, 32651];
const SPI_SETCURSORS: u32 = 0x0057;
const GA_ROOT: u32 = 2;

static MASKED: AtomicBool = AtomicBool::new(false);

/// Draw every standard pointer shape as the arrow (`true`), or go back to the user's own
/// pointers (`false`). Does nothing if that is already the case.
pub fn set_masked(masked: bool) {
    if MASKED.swap(masked, Ordering::SeqCst) == masked {
        return;
    }
    if masked {
        unsafe {
            let arrow = LoadCursorW(null_mut(), ARROW as usize as *const u16);
            for shape in SHAPES {
                // SetSystemCursor takes ownership of the handle, so each shape gets a copy.
                let copy = CopyIcon(arrow);
                if !copy.is_null() {
                    SetSystemCursor(copy, shape);
                }
            }
        }
    } else {
        restore();
    }
}

/// Reload the user's pointer scheme. Safe to call at any time, masked or not; the app
/// calls it at startup in case a previous run was killed while the mask was on.
pub fn restore() {
    MASKED.store(false, Ordering::SeqCst);
    unsafe {
        SystemParametersInfoW(SPI_SETCURSORS, 0, null_mut(), 0);
    }
}

/// The top-level window the pointer is over right now.
pub fn window_under_pointer() -> Option<isize> {
    unsafe {
        let mut point = POINT { x: 0, y: 0 };
        if GetCursorPos(&mut point) == 0 {
            return None;
        }
        let window = WindowFromPoint(point);
        if window.is_null() {
            return None;
        }
        let root = GetAncestor(window, GA_ROOT);
        Some(if root.is_null() { window } else { root } as isize)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::c_void;
    use std::mem::{size_of, zeroed};
    use windows_sys::Win32::Graphics::Gdi::{
        CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, SelectObject, BITMAPINFO,
        BITMAPINFOHEADER,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::DrawIconEx;

    /// The pixels Windows currently draws for a standard pointer shape.
    fn pixels(shape: u32) -> Vec<u8> {
        const SIZE: i32 = 32;
        unsafe {
            let cursor = LoadCursorW(null_mut(), shape as usize as *const u16);
            let dc = CreateCompatibleDC(null_mut());
            let mut info: BITMAPINFO = zeroed();
            info.bmiHeader.biSize = size_of::<BITMAPINFOHEADER>() as u32;
            info.bmiHeader.biWidth = SIZE;
            info.bmiHeader.biHeight = -SIZE;
            info.bmiHeader.biPlanes = 1;
            info.bmiHeader.biBitCount = 32;
            let mut bits: *mut c_void = null_mut();
            let bitmap = CreateDIBSection(dc, &info, 0, &mut bits, null_mut(), 0);
            let previous = SelectObject(dc, bitmap);
            DrawIconEx(dc, 0, 0, cursor, SIZE, SIZE, 0, null_mut(), 3);
            let data = std::slice::from_raw_parts(bits as *const u8, (SIZE * SIZE * 4) as usize).to_vec();
            SelectObject(dc, previous);
            DeleteObject(bitmap);
            DeleteDC(dc);
            data
        }
    }

    #[test]
    fn masking_turns_the_text_bar_into_the_arrow_and_restoring_brings_it_back() {
        const TEXT_BAR: u32 = 32513;
        const HAND: u32 = 32649;
        restore();
        let arrow = pixels(ARROW);
        let text_bar = pixels(TEXT_BAR);
        if arrow == text_bar {
            return; // no distinguishable pointers on this machine (headless session)
        }

        set_masked(true);
        let (masked_text, masked_hand) = (pixels(TEXT_BAR), pixels(HAND));
        set_masked(false); // put the machine back before asserting anything

        assert_eq!(masked_text, arrow, "the text bar should be drawn as the arrow");
        assert_eq!(masked_hand, arrow, "the hand should be drawn as the arrow");
        assert_eq!(pixels(TEXT_BAR), text_bar, "the user's text bar should be back");
        assert_eq!(pixels(ARROW), arrow);
    }
}
