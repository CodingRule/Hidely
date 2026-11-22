//! Change the display affinity of a window owned by another process.
//!
//! `SetWindowDisplayAffinity` fails with `ERROR_ACCESS_DENIED` unless the caller
//! owns the window, so the call has to be made from inside the owning process.
//! We write a ~40 byte stub into the target that does exactly one thing —
//! `SetWindowDisplayAffinity(hwnd, affinity)` — run it on a short-lived thread,
//! read the BOOL it returned, and free the memory again. No DLL stays loaded and
//! nothing is patched.
//!
//! Limits: x64 targets only, and the target must not be elevated above
//! Hidely or be a protected process.

use std::ffi::c_void;
use std::mem::transmute;
use std::ptr::{null, null_mut};

use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, HANDLE, HWND};
use windows_sys::Win32::System::Diagnostics::Debug::WriteProcessMemory;
use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
use windows_sys::Win32::System::Memory::{VirtualAllocEx, VirtualFreeEx, VirtualProtectEx};
use windows_sys::Win32::System::Threading::{
    CreateRemoteThread, GetExitCodeThread, IsWow64Process2, OpenProcess, WaitForSingleObject,
};
use windows_sys::Win32::UI::WindowsAndMessaging::SetWindowDisplayAffinity;

use crate::CloakError;

const PROCESS_ACCESS: u32 = 0x0002 // CREATE_THREAD
    | 0x0008 // VM_OPERATION
    | 0x0010 // VM_READ
    | 0x0020 // VM_WRITE
    | 0x0400; // QUERY_INFORMATION
const MEM_COMMIT_RESERVE: u32 = 0x3000;
const MEM_RELEASE: u32 = 0x8000;
const PAGE_READWRITE: u32 = 0x04;
const PAGE_EXECUTE_READ: u32 = 0x20;
const ERROR_ACCESS_DENIED: u32 = 5;
const IMAGE_FILE_MACHINE_UNKNOWN: u16 = 0;
const IMAGE_FILE_MACHINE_AMD64: u16 = 0x8664;
const WAIT_OBJECT_0: u32 = 0;
const TIMEOUT_MS: u32 = 3000;

/// Set the affinity of a window owned by the current process.
pub fn set_own(hwnd: isize, affinity: u32) -> Result<(), CloakError> {
    if unsafe { SetWindowDisplayAffinity(hwnd as HWND, affinity) } != 0 {
        Ok(())
    } else {
        Err(os_error("SetWindowDisplayAffinity"))
    }
}

/// Set the affinity of a window owned by process `pid`.
pub fn set_remote(hwnd: isize, pid: u32, affinity: u32) -> Result<(), CloakError> {
    if !cfg!(target_arch = "x86_64") {
        return Err(CloakError::Unsupported("Hidely must be built for x64"));
    }
    unsafe {
        let process = Handle(OpenProcess(PROCESS_ACCESS, 0, pid));
        if process.0.is_null() {
            return Err(os_error("OpenProcess"));
        }

        let (mut process_machine, mut native_machine) = (0u16, 0u16);
        if IsWow64Process2(process.0, &mut process_machine, &mut native_machine) == 0 {
            return Err(os_error("IsWow64Process2"));
        }
        if process_machine != IMAGE_FILE_MACHINE_UNKNOWN {
            return Err(CloakError::Unsupported("32-bit apps are not supported yet"));
        }
        if native_machine != IMAGE_FILE_MACHINE_AMD64 {
            return Err(CloakError::Unsupported("only x64 Windows is supported"));
        }

        // user32.dll is mapped at the same address in every process of a boot session,
        // so our own address for the function is valid in the target as well.
        let user32: Vec<u16> = "user32.dll\0".encode_utf16().collect();
        let function =
            GetProcAddress(GetModuleHandleW(user32.as_ptr()), c"SetWindowDisplayAffinity".as_ptr().cast())
                .ok_or_else(|| os_error("GetProcAddress"))? as usize;

        let stub = build_stub(hwnd as u64, affinity, function as u64);
        let memory =
            VirtualAllocEx(process.0, null(), stub.len(), MEM_COMMIT_RESERVE, PAGE_READWRITE);
        if memory.is_null() {
            return Err(os_error("VirtualAllocEx"));
        }

        let result = run_stub(process.0, memory, &stub);
        // On a timeout the thread may still be executing the stub, so leak the page.
        if result != Err(CloakError::Timeout) {
            VirtualFreeEx(process.0, memory, 0, MEM_RELEASE);
        }
        result
    }
}

unsafe fn run_stub(process: HANDLE, memory: *mut c_void, stub: &[u8]) -> Result<(), CloakError> {
    if WriteProcessMemory(process, memory, stub.as_ptr().cast(), stub.len(), null_mut()) == 0 {
        return Err(os_error("WriteProcessMemory"));
    }
    let mut old_protection = 0u32;
    if VirtualProtectEx(process, memory, stub.len(), PAGE_EXECUTE_READ, &mut old_protection) == 0 {
        return Err(os_error("VirtualProtectEx"));
    }

    let entry: unsafe extern "system" fn(*mut c_void) -> u32 = transmute(memory);
    let thread =
        Handle(CreateRemoteThread(process, null(), 0, Some(entry), null(), 0, null_mut()));
    if thread.0.is_null() {
        return Err(os_error("CreateRemoteThread"));
    }
    if WaitForSingleObject(thread.0, TIMEOUT_MS) != WAIT_OBJECT_0 {
        return Err(CloakError::Timeout);
    }
    let mut returned = 0u32;
    if GetExitCodeThread(thread.0, &mut returned) == 0 {
        return Err(os_error("GetExitCodeThread"));
    }
    if returned == 0 {
        return Err(CloakError::Rejected);
    }
    Ok(())
}

/// x64 thread entry point: `return SetWindowDisplayAffinity(hwnd, affinity);`
fn build_stub(hwnd: u64, affinity: u32, function: u64) -> Vec<u8> {
    let mut code = Vec::with_capacity(40);
    code.extend_from_slice(&[0x48, 0x83, 0xEC, 0x28]); // sub  rsp, 28h  (shadow space + align)
    code.extend_from_slice(&[0x48, 0xB9]); //             mov  rcx, hwnd
    code.extend_from_slice(&hwnd.to_le_bytes());
    code.push(0xBA); //                                   mov  edx, affinity
    code.extend_from_slice(&affinity.to_le_bytes());
    code.extend_from_slice(&[0x48, 0xB8]); //             mov  rax, function
    code.extend_from_slice(&function.to_le_bytes());
    code.extend_from_slice(&[0xFF, 0xD0]); //             call rax
    code.extend_from_slice(&[0x48, 0x83, 0xC4, 0x28]); // add  rsp, 28h
    code.push(0xC3); //                                   ret
    code
}

fn os_error(call: &'static str) -> CloakError {
    match unsafe { GetLastError() } {
        ERROR_ACCESS_DENIED => CloakError::AccessDenied,
        code => CloakError::Os { call, code },
    }
}

struct Handle(HANDLE);

impl Drop for Handle {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { CloseHandle(self.0) };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stub_embeds_arguments_at_fixed_offsets() {
        let stub = build_stub(0x1122_3344_5566_7788, 0x11, 0x7FFA_BBCC_DDEE_FF00);
        assert_eq!(stub.len(), 36);
        assert_eq!(&stub[..4], &[0x48, 0x83, 0xEC, 0x28]);
        assert_eq!(&stub[6..14], &0x1122_3344_5566_7788u64.to_le_bytes());
        assert_eq!(&stub[15..19], &0x11u32.to_le_bytes());
        assert_eq!(&stub[21..29], &0x7FFA_BBCC_DDEE_FF00u64.to_le_bytes());
        assert_eq!(&stub[29..], &[0xFF, 0xD0, 0x48, 0x83, 0xC4, 0x28, 0xC3]);
    }

    #[test]
    fn foreign_window_cannot_be_changed_directly() {
        // The reason this module exists: the direct call is refused for other processes.
        let other = crate::list_windows(Some(std::process::id()));
        if let Some(window) = other.first() {
            assert_eq!(set_own(window.hwnd, crate::WDA_NONE), Err(CloakError::AccessDenied));
        }
    }
}
