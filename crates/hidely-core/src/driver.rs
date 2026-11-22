//! Client for the DWMShield kernel driver (`driver/HideWindow.c`).
//!
//! **Experimental and not used by the app.** The upstream driver can only hide — it
//! has no un-hide request — so wiring it in would break the "show all" guarantee.
//! This is a straight port of `driver/protect.c`, kept here for the v0.3 work
//! described in `docs/DRIVER.md`.

use std::ptr::{null, null_mut};

use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, HANDLE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::Storage::FileSystem::CreateFileW;
use windows_sys::Win32::System::IO::DeviceIoControl;

use crate::CloakError;

/// `CTL_CODE(FILE_DEVICE_UNKNOWN, 0x800, METHOD_BUFFERED, FILE_ANY_ACCESS)`
const IOCTL_HIDE_WINDOW: u32 = (0x22 << 16) | (0x800 << 2);
const GENERIC_READ_WRITE: u32 = 0xC000_0000;
const OPEN_EXISTING: u32 = 3;

#[repr(C)]
struct HideWindowData {
    window_handle: u64,
    flags: u32,
}

pub struct DriverClient(HANDLE);

impl DriverClient {
    /// Opens `\\.\HideWindow`. Returns `None` when the driver is not loaded.
    pub fn open() -> Option<Self> {
        let path: Vec<u16> = "\\\\.\\HideWindow\0".encode_utf16().collect();
        let handle = unsafe {
            CreateFileW(path.as_ptr(), GENERIC_READ_WRITE, 0, null(), OPEN_EXISTING, 0, null_mut())
        };
        (handle != INVALID_HANDLE_VALUE).then_some(Self(handle))
    }

    /// Ask the driver to exclude `hwnd` from capture. This cannot be undone by the driver.
    pub fn hide(&self, hwnd: isize) -> Result<(), CloakError> {
        let data = HideWindowData { window_handle: hwnd as u64, flags: 0 };
        let mut returned = 0u32;
        let ok = unsafe {
            DeviceIoControl(
                self.0,
                IOCTL_HIDE_WINDOW,
                (&data as *const HideWindowData).cast(),
                size_of::<HideWindowData>() as u32,
                null_mut(),
                0,
                &mut returned,
                null_mut(),
            )
        };
        if ok != 0 {
            Ok(())
        } else {
            Err(CloakError::Os { call: "DeviceIoControl", code: unsafe { GetLastError() } })
        }
    }
}

impl Drop for DriverClient {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.0) };
    }
}
