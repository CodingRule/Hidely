//! Small pieces of Windows integration: starting with Windows, and opening a web page.

use std::ptr::{null, null_mut};

use windows_sys::Win32::Foundation::CloseHandle;
use windows_sys::Win32::System::Registry::{
    RegCloseKey, RegCreateKeyExW, RegDeleteKeyW, RegDeleteValueW, RegOpenKeyExW,
    RegQueryValueExW, RegSetValueExW, HKEY, HKEY_CURRENT_USER, KEY_QUERY_VALUE, KEY_SET_VALUE,
    REG_SZ,
};
use windows_sys::Win32::System::Threading::{CreateMutexW, OpenMutexW};
use windows_sys::Win32::UI::Shell::ShellExecuteW;

/// The per-user list of programs Windows starts at sign-in. No administrator rights needed.
const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const ERROR_FILE_NOT_FOUND: u32 = 2;

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(Some(0)).collect()
}

fn open_run_key(access: u32) -> Result<HKEY, u32> {
    let mut key: HKEY = null_mut();
    let status = unsafe {
        RegCreateKeyExW(
            HKEY_CURRENT_USER,
            wide(RUN_KEY).as_ptr(),
            0,
            null(),
            0,
            access,
            null(),
            &mut key,
            null_mut(),
        )
    };
    if status == 0 { Ok(key) } else { Err(status) }
}

/// Whether an entry called `name` starts with Windows for this user.
pub fn starts_with_windows(name: &str) -> bool {
    let Ok(key) = open_run_key(KEY_QUERY_VALUE) else { return false };
    let status = unsafe {
        let status =
            RegQueryValueExW(key, wide(name).as_ptr(), null(), null_mut(), null_mut(), null_mut());
        RegCloseKey(key);
        status
    };
    status == 0
}

/// Make `command` start at sign-in under `name`, or remove the entry when `command` is `None`.
/// Returns the Win32 error code on failure.
pub fn set_starts_with_windows(name: &str, command: Option<&str>) -> Result<(), u32> {
    let key = open_run_key(KEY_SET_VALUE)?;
    let status = unsafe {
        let status = match command {
            Some(command) => {
                let data = wide(command);
                RegSetValueExW(
                    key,
                    wide(name).as_ptr(),
                    0,
                    REG_SZ,
                    data.as_ptr().cast(),
                    (data.len() * 2) as u32,
                )
            }
            None => RegDeleteValueW(key, wide(name).as_ptr()),
        };
        RegCloseKey(key);
        status
    };
    // Removing an entry that is not there is already the wanted result.
    if status == 0 || (command.is_none() && status == ERROR_FILE_NOT_FOUND) {
        Ok(())
    } else {
        Err(status)
    }
}

/// The text stored as the default value of a key under `HKEY_CURRENT_USER`, if it exists.
pub fn user_key_default(subkey: &str) -> Option<String> {
    unsafe {
        let mut key: HKEY = null_mut();
        if RegOpenKeyExW(HKEY_CURRENT_USER, wide(subkey).as_ptr(), 0, KEY_QUERY_VALUE, &mut key) != 0 {
            return None;
        }
        let mut buffer = [0u16; 1024];
        let mut bytes = (buffer.len() * 2) as u32;
        let status =
            RegQueryValueExW(key, null(), null(), null_mut(), buffer.as_mut_ptr().cast(), &mut bytes);
        RegCloseKey(key);
        if status != 0 {
            return None;
        }
        let length = (bytes as usize / 2).min(buffer.len());
        Some(String::from_utf16_lossy(&buffer[..length]).trim_end_matches('\0').to_string())
    }
}

/// Create a key under `HKEY_CURRENT_USER` with `value` as its default value, or delete the
/// key when `value` is `None`. Returns the Win32 error code on failure.
pub fn set_user_key_default(subkey: &str, value: Option<&str>) -> Result<(), u32> {
    unsafe {
        let Some(value) = value else {
            let status = RegDeleteKeyW(HKEY_CURRENT_USER, wide(subkey).as_ptr());
            return if status == 0 || status == ERROR_FILE_NOT_FOUND { Ok(()) } else { Err(status) };
        };
        let mut key: HKEY = null_mut();
        let status = RegCreateKeyExW(
            HKEY_CURRENT_USER,
            wide(subkey).as_ptr(),
            0,
            null(),
            0,
            KEY_SET_VALUE,
            null(),
            &mut key,
            null_mut(),
        );
        if status != 0 {
            return Err(status);
        }
        let data = wide(value);
        let status = RegSetValueExW(key, null(), 0, REG_SZ, data.as_ptr().cast(), (data.len() * 2) as u32);
        RegCloseKey(key);
        if status == 0 { Ok(()) } else { Err(status) }
    }
}

/// A named marker that exists for as long as the Hidely app is running.
const RUNNING_MARKER: &str = "Local\\Hidely.Running";

/// Announce that the app is running. The marker disappears when the process ends.
pub fn mark_app_running() {
    // The handle is deliberately never closed: Windows releases it at exit.
    unsafe { CreateMutexW(null(), 0, wide(RUNNING_MARKER).as_ptr()) };
}

/// Whether the Hidely app is running in this session.
pub fn is_app_running() -> bool {
    const SYNCHRONIZE: u32 = 0x0010_0000;
    unsafe {
        let handle = OpenMutexW(SYNCHRONIZE, 0, wide(RUNNING_MARKER).as_ptr());
        if handle.is_null() {
            return false;
        }
        CloseHandle(handle);
        true
    }
}

/// Open a web page in the user's default browser. Only `https://` addresses are opened.
pub fn open_in_browser(url: &str) -> bool {
    if !url.starts_with("https://") {
        return false;
    }
    let result = unsafe {
        ShellExecuteW(null_mut(), wide("open").as_ptr(), wide(url).as_ptr(), null(), null(), 1)
    };
    result as usize > 32
}

/// Compare two versions such as `0.2.1` and `v0.10.0` by their numbers. Returns true when
/// `candidate` is newer than `current`. Anything that is not a plain version is not newer.
pub fn is_newer_version(candidate: &str, current: &str) -> bool {
    fn parts(version: &str) -> Option<Vec<u64>> {
        let numbers: Option<Vec<u64>> =
            version.trim().trim_start_matches('v').split('.').map(|part| part.parse().ok()).collect();
        numbers.filter(|numbers| !numbers.is_empty())
    }
    match (parts(candidate), parts(current)) {
        (Some(candidate), Some(current)) => candidate > current,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_compare_by_number_not_by_text() {
        assert!(is_newer_version("0.2.2", "0.2.1"));
        assert!(is_newer_version("v0.10.0", "0.9.9"), "10 is more than 9");
        assert!(is_newer_version("1.0.0", "0.99.99"));
        assert!(!is_newer_version("0.2.1", "0.2.1"), "the same version is not newer");
        assert!(!is_newer_version("0.2.0", "0.2.1"));
        assert!(!is_newer_version("nightly", "0.2.1"), "unknown formats are never 'newer'");
        assert!(!is_newer_version("", "0.2.1"));
        assert!(!is_newer_version("0.3.0-beta", "0.2.1"), "pre-release tags are ignored");
    }

    #[test]
    fn a_startup_entry_can_be_added_and_removed() {
        let name = format!("HidelyTest{}", std::process::id());
        assert!(!starts_with_windows(&name));
        set_starts_with_windows(&name, Some(r#""C:\nowhere\Hidely.exe" --minimized"#)).unwrap();
        assert!(starts_with_windows(&name));
        set_starts_with_windows(&name, None).unwrap();
        assert!(!starts_with_windows(&name));
        set_starts_with_windows(&name, None).unwrap(); // removing twice is fine
    }

    #[test]
    fn a_registry_key_can_be_written_read_and_removed() {
        let key = format!(r"Software\HidelyTest{}\Nested", std::process::id());
        let parent = format!(r"Software\HidelyTest{}", std::process::id());
        assert_eq!(user_key_default(&key), None);
        set_user_key_default(&key, Some(r"C:\somewhere\host.json")).unwrap();
        assert_eq!(user_key_default(&key).as_deref(), Some(r"C:\somewhere\host.json"));
        set_user_key_default(&key, None).unwrap();
        assert_eq!(user_key_default(&key), None);
        set_user_key_default(&key, None).unwrap(); // removing twice is fine
        set_user_key_default(&parent, None).unwrap();
    }

    #[test]
    fn the_running_marker_is_seen_once_set() {
        // No other test sets it, and the app is a different process, so this only checks
        // that setting it makes it visible.
        mark_app_running();
        assert!(is_app_running());
    }

    #[test]
    fn only_https_pages_are_opened() {
        assert!(!open_in_browser("file:///C:/Windows/System32/calc.exe"));
        assert!(!open_in_browser("http://example.com"));
        assert!(!open_in_browser("calc.exe"));
    }
}
