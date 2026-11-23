//! Running as administrator.
//!
//! Windows does not let a normal program reach into one that runs as administrator, so
//! Hidely cannot protect such an app's windows unless Hidely itself is elevated. This is
//! the supported way to cover elevated apps, and it needs no driver.

use std::mem::size_of;
use std::ptr::{null, null_mut};

use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
use windows_sys::Win32::Security::{GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY};
use windows_sys::Win32::System::Threading::{
    GetCurrentProcess, OpenProcess, OpenProcessToken, WaitForSingleObject,
};
use windows_sys::Win32::UI::Shell::ShellExecuteW;

const SYNCHRONIZE: u32 = 0x0010_0000;
const SW_SHOWNORMAL: i32 = 1;

/// Whether this process is running as administrator.
pub fn is_elevated() -> bool {
    unsafe {
        let mut token: HANDLE = null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return false;
        }
        let mut elevation = TOKEN_ELEVATION { TokenIsElevated: 0 };
        let mut size = 0u32;
        let ok = GetTokenInformation(
            token,
            TokenElevation,
            (&mut elevation as *mut TOKEN_ELEVATION).cast(),
            size_of::<TOKEN_ELEVATION>() as u32,
            &mut size,
        );
        CloseHandle(token);
        ok != 0 && elevation.TokenIsElevated != 0
    }
}

/// Start `exe` as administrator with `arguments`. Windows asks the user to approve. Returns
/// false if they decline or it cannot be started.
pub fn launch_elevated(exe: &std::path::Path, arguments: &str) -> bool {
    let wide = |text: &str| -> Vec<u16> { text.encode_utf16().chain(Some(0)).collect() };
    let verb = wide("runas");
    let file = wide(&exe.to_string_lossy());
    let arguments = wide(arguments);
    let result = unsafe {
        ShellExecuteW(null_mut(), verb.as_ptr(), file.as_ptr(), arguments.as_ptr(), null(), SW_SHOWNORMAL)
    };
    // ShellExecute reports success with a value above 32.
    result as usize > 32
}

/// Wait up to `millis` for the process `pid` to end. Returns at once if it is already gone.
pub fn wait_for_exit(pid: u32, millis: u32) {
    unsafe {
        let process = OpenProcess(SYNCHRONIZE, 0, pid);
        if !process.is_null() {
            WaitForSingleObject(process, millis);
            CloseHandle(process);
        }
    }
}
