//! "What viewers see": a small picture of the screen as Windows screen capture returns it.
//!
//! Windows excluded from capture are left out of this picture by Windows itself, so it is
//! direct evidence of what a screen share shows. The picture only ever lives in memory.
//! It is never written to disk, logged or put in diagnostics.

use std::ffi::c_void;
use std::mem::{size_of, zeroed};
use std::ptr::null_mut;

use windows_sys::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GdiFlush, GetDC, ReleaseDC,
    SelectObject,
    SetBrushOrgEx, SetStretchBltMode, StretchBlt, BITMAPINFO, BITMAPINFOHEADER, HALFTONE, SRCCOPY,
};

/// Straight RGBA pixels, top row first, alpha always 255.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// The size to scale a `w`×`h` area to so it is at most `max_w` wide, keeping its shape.
/// Never scales up, and never returns a zero side for a non-empty area.
pub fn scaled_size(w: u32, h: u32, max_w: u32) -> (u32, u32) {
    if w == 0 || h == 0 {
        return (0, 0);
    }
    if w <= max_w {
        return (w, h);
    }
    let height = (u64::from(h) * u64::from(max_w) + u64::from(w) / 2) / u64::from(w);
    (max_w.max(1), (height as u32).max(1))
}

/// Which monitor to show. `monitors` are (x, y, width, height); windows are (x, y, width,
/// height) too. The monitor holding the centre of the most protected windows wins; with
/// none, the one holding Hidely's own window; otherwise `primary`.
pub fn pick_monitor(
    monitors: &[(i32, i32, u32, u32)],
    protected: &[(i32, i32, i32, i32)],
    main: Option<(i32, i32, i32, i32)>,
    primary: usize,
) -> usize {
    let holding = |rect: &(i32, i32, i32, i32)| {
        let (cx, cy) = (
            i64::from(rect.0) + i64::from(rect.2) / 2,
            i64::from(rect.1) + i64::from(rect.3) / 2,
        );
        monitors.iter().position(|&(x, y, w, h)| {
            let (x, y) = (i64::from(x), i64::from(y));
            cx >= x && cx < x + i64::from(w) && cy >= y && cy < y + i64::from(h)
        })
    };
    let mut counts = vec![0usize; monitors.len()];
    for index in protected.iter().filter_map(holding) {
        counts[index] += 1;
    }
    // Ties go to the earlier monitor, so the choice does not flicker between frames.
    let best = counts.iter().enumerate().fold(None, |best: Option<(usize, usize)>, (index, &n)| {
        match best {
            Some((_, most)) if most >= n => best,
            _ if n > 0 => Some((index, n)),
            _ => best,
        }
    });
    if let Some((index, _)) = best {
        return index;
    }
    if let Some(index) = main.as_ref().and_then(holding) {
        return index;
    }
    if primary < monitors.len() { primary } else { 0 }
}

/// GDI gives blue, green, red, unused. Turn that into opaque red, green, blue, alpha.
pub fn bgra_to_rgba(pixels: &mut [u8]) {
    for pixel in pixels.chunks_exact_mut(4) {
        pixel.swap(0, 2);
        pixel[3] = 255;
    }
}

/// Capture `rect` (x, y, width, height in physical screen pixels) scaled to at most
/// `max_width` wide. `None` when the screen cannot be read, for example while the lock
/// screen or an administrator prompt is showing.
pub fn capture_screen(rect: (i32, i32, i32, i32), max_width: u32) -> Option<Frame> {
    let (x, y, w, h) = rect;
    if w <= 0 || h <= 0 {
        return None;
    }
    let (width, height) = scaled_size(w as u32, h as u32, max_width.max(1));
    unsafe {
        let screen = GetDC(null_mut());
        if screen.is_null() {
            return None;
        }
        let memory = CreateCompatibleDC(screen);
        if memory.is_null() {
            ReleaseDC(null_mut(), screen);
            return None;
        }
        let mut info: BITMAPINFO = zeroed();
        info.bmiHeader.biSize = size_of::<BITMAPINFOHEADER>() as u32;
        info.bmiHeader.biWidth = width as i32;
        info.bmiHeader.biHeight = -(height as i32); // top row first
        info.bmiHeader.biPlanes = 1;
        info.bmiHeader.biBitCount = 32;
        let mut bits: *mut c_void = null_mut();
        let bitmap = CreateDIBSection(memory, &info, 0, &mut bits, null_mut(), 0);
        if bitmap.is_null() || bits.is_null() {
            DeleteDC(memory);
            ReleaseDC(null_mut(), screen);
            return None;
        }
        let previous = SelectObject(memory, bitmap);
        // HALFTONE averages pixels instead of dropping them, so small text blurs rather
        // than turning into noise. It needs the brush origin set after the mode.
        SetStretchBltMode(memory, HALFTONE);
        SetBrushOrgEx(memory, 0, 0, null_mut());
        let copied =
            StretchBlt(memory, 0, 0, width as i32, height as i32, screen, x, y, w, h, SRCCOPY);
        // GDI may batch drawing calls; make sure the pixels are in the bitmap before reading.
        GdiFlush();
        let len = width as usize * height as usize * 4;
        let mut rgba = std::slice::from_raw_parts(bits as *const u8, len).to_vec();
        SelectObject(memory, previous);
        DeleteObject(bitmap);
        DeleteDC(memory);
        ReleaseDC(null_mut(), screen);
        if copied == 0 {
            return None;
        }
        bgra_to_rgba(&mut rgba);
        Some(Frame { width, height, rgba })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_shrink_to_fit_and_keep_their_shape() {
        assert_eq!(scaled_size(3840, 2160, 640), (640, 360));
        assert_eq!(scaled_size(1920, 1200, 640), (640, 400));
        assert_eq!(scaled_size(500, 300, 640), (500, 300), "never scaled up");
        assert_eq!(scaled_size(10_000, 1, 640), (640, 1), "a thin strip keeps one row");
        assert_eq!(scaled_size(0, 100, 640), (0, 0));
    }

    #[test]
    fn the_monitor_with_the_most_protected_windows_is_shown() {
        let monitors = [(0, 0, 1920, 1080), (1920, 0, 2560, 1440), (-1280, 0, 1280, 1024)];
        let on_second = (2000, 100, 800, 600);
        let on_third = (-1000, 100, 400, 300);
        let main_on_third = Some((-1200, 50, 600, 400));

        assert_eq!(pick_monitor(&monitors, &[on_second, on_second, on_third], None, 0), 1);
        assert_eq!(pick_monitor(&monitors, &[on_second, on_third], None, 0), 1, "ties go to the earlier one");
        assert_eq!(pick_monitor(&monitors, &[], main_on_third, 0), 2, "then Hidely's own monitor");
        assert_eq!(pick_monitor(&monitors, &[], None, 1), 1, "then the primary");
        assert_eq!(pick_monitor(&monitors, &[(9000, 9000, 10, 10)], None, 7), 0, "never out of range");
        assert_eq!(pick_monitor(&[], &[on_second], None, 0), 0);
    }

    #[test]
    fn pixels_become_opaque_rgba() {
        let mut pixels = vec![1, 2, 3, 0, 10, 20, 30, 7];
        bgra_to_rgba(&mut pixels);
        assert_eq!(pixels, vec![3, 2, 1, 255, 30, 20, 10, 255]);
    }

    /// The product's promise as a test: a solid magenta window excluded from capture must
    /// not appear in the capture. Needs a real desktop, so it only runs when asked for:
    /// `cargo test -p hidely-core -- --ignored`.
    #[test]
    #[ignore = "needs an interactive desktop"]
    fn an_excluded_window_is_missing_from_the_capture() {
        use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
        use windows_sys::Win32::Graphics::Gdi::CreateSolidBrush;
        use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
        use windows_sys::Win32::UI::HiDpi::{
            SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
        };
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, PeekMessageW,
            RegisterClassW, SetWindowDisplayAffinity, TranslateMessage, MSG, PM_REMOVE,
            WNDCLASSW, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP, WS_VISIBLE,
        };

        unsafe extern "system" fn procedure(h: HWND, m: u32, w: WPARAM, l: LPARAM) -> LRESULT {
            DefWindowProcW(h, m, w, l)
        }
        fn pump(millis: u64) {
            let until = std::time::Instant::now() + std::time::Duration::from_millis(millis);
            while std::time::Instant::now() < until {
                unsafe {
                    let mut message: MSG = zeroed();
                    while PeekMessageW(&mut message, null_mut(), 0, 0, PM_REMOVE) != 0 {
                        TranslateMessage(&message);
                        DispatchMessageW(&message);
                    }
                }
                std::thread::sleep(std::time::Duration::from_millis(15));
            }
        }
        fn magenta(frame: &Frame) -> usize {
            frame
                .rgba
                .chunks_exact(4)
                .filter(|p| p[0] > 230 && p[1] < 25 && p[2] > 230)
                .count()
        }

        let (x, y, size) = (200, 200, 240);
        // Sample the middle of the window, away from any edge blending.
        let inner = (x + 40, y + 40, size - 80, size - 80);
        unsafe {
            SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
            let class: Vec<u16> = "HidelyMagentaTest\0".encode_utf16().collect();
            let instance = GetModuleHandleW(null_mut());
            let mut wc: WNDCLASSW = zeroed();
            wc.lpfnWndProc = Some(procedure);
            wc.hInstance = instance;
            wc.lpszClassName = class.as_ptr();
            wc.hbrBackground = CreateSolidBrush(0x00FF_00FF); // COLORREF is 0x00BBGGRR
            RegisterClassW(&wc);
            let hwnd = CreateWindowExW(
                WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
                class.as_ptr(),
                class.as_ptr(),
                WS_POPUP | WS_VISIBLE,
                x,
                y,
                size,
                size,
                null_mut(),
                null_mut(),
                instance,
                null_mut(),
            );
            assert!(!hwnd.is_null(), "could not create the test window");
            pump(400);

            let before = capture_screen(inner, 4096).expect("the screen could not be read");
            assert!(magenta(&before) > 0, "the window should be visible before it is excluded");

            assert_ne!(SetWindowDisplayAffinity(hwnd, crate::WDA_EXCLUDEFROMCAPTURE), 0);
            pump(400);
            let after = capture_screen(inner, 4096).expect("the screen could not be read");
            DestroyWindow(hwnd);
            assert_eq!(magenta(&after), 0, "an excluded window showed up in the capture");
        }
    }
}
