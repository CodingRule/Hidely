//! Change the display affinity of a window owned by another process.
//!
//! `SetWindowDisplayAffinity` fails with `ERROR_ACCESS_DENIED` unless the caller
//! owns the window, so the call has to be made from inside the owning process.
//! We write a ~40 byte stub into the target that does exactly one thing —
//! `SetWindowDisplayAffinity(hwnd, affinity)` — run it on a short-lived thread,
//! read the BOOL it returned, and free the memory again. No DLL stays loaded and
//! nothing is patched.
//!
//! Both 64-bit and 32-bit targets are handled, each with a stub in its own machine code.
//! The target must not be elevated above Hidely or be a protected process.

use std::ffi::c_void;
use std::mem::transmute;
use std::mem::size_of;
use std::ptr::{null, null_mut};
use std::sync::OnceLock;

use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, HANDLE, HWND};
use windows_sys::Win32::System::Diagnostics::Debug::WriteProcessMemory;
use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
use windows_sys::Win32::System::Memory::{VirtualAllocEx, VirtualFreeEx, VirtualProtectEx};
use windows_sys::Win32::System::ProcessStatus::{EnumProcessModulesEx, GetModuleBaseNameW};
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
const IMAGE_FILE_MACHINE_I386: u16 = 0x014C;
const LIST_MODULES_32BIT: u32 = 0x01;
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
        if native_machine != IMAGE_FILE_MACHINE_AMD64 {
            return Err(CloakError::Unsupported("only x64 Windows is supported"));
        }

        let stub = match process_machine {
            // A native 64-bit process. user32.dll is mapped at the same address in every
            // 64-bit process of a boot session, so our own address for the function is
            // valid in the target as well.
            IMAGE_FILE_MACHINE_UNKNOWN => {
                let user32: Vec<u16> = "user32.dll\0".encode_utf16().collect();
                let function = GetProcAddress(
                    GetModuleHandleW(user32.as_ptr()),
                    c"SetWindowDisplayAffinity".as_ptr().cast(),
                )
                .ok_or_else(|| os_error("GetProcAddress"))? as usize;
                build_stub(hwnd as u64, affinity, function as u64)
            }
            // A 32-bit process. We cannot ask our own (64-bit) user32 for the address, so
            // find where the target has its 32-bit user32 and add the function's offset.
            IMAGE_FILE_MACHINE_I386 => {
                let base = user32_base_x86(process.0)?;
                let offset = user32_offset_x86()
                    .ok_or(CloakError::Unsupported("could not read the 32-bit user32.dll"))?;
                build_stub_x86(hwnd as u32, affinity, base.wrapping_add(offset))
            }
            _ => return Err(CloakError::Unsupported("only x64 and x86 apps are supported")),
        };
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

/// Where the 32-bit user32.dll is loaded in a 32-bit process.
unsafe fn user32_base_x86(process: HANDLE) -> Result<u32, CloakError> {
    let mut modules = [null_mut::<c_void>(); 1024];
    let mut needed = 0u32;
    let size = (modules.len() * size_of::<*mut c_void>()) as u32;
    if EnumProcessModulesEx(process, modules.as_mut_ptr(), size, &mut needed, LIST_MODULES_32BIT) == 0 {
        return Err(os_error("EnumProcessModulesEx"));
    }
    let count = (needed as usize / size_of::<*mut c_void>()).min(modules.len());
    for &module in &modules[..count] {
        let mut name = [0u16; 64];
        let length = GetModuleBaseNameW(process, module, name.as_mut_ptr(), name.len() as u32);
        if String::from_utf16_lossy(&name[..length as usize]).eq_ignore_ascii_case("user32.dll") {
            return Ok(module as usize as u32);
        }
    }
    Err(CloakError::Unsupported("this app has not loaded user32.dll"))
}

/// The offset of `SetWindowDisplayAffinity` inside the 32-bit user32.dll, read from the
/// file's export table once and remembered.
fn user32_offset_x86() -> Option<u32> {
    static OFFSET: OnceLock<Option<u32>> = OnceLock::new();
    *OFFSET.get_or_init(|| {
        let windows = std::env::var_os("WINDIR")?;
        let file = std::fs::read(std::path::Path::new(&windows).join("SysWOW64").join("user32.dll")).ok()?;
        export_offset(&file, "SetWindowDisplayAffinity")
    })
}

/// Look up an exported function's offset (RVA) in a 32-bit DLL image read from disk.
fn export_offset(file: &[u8], wanted: &str) -> Option<u32> {
    let u16_at = |at: usize| Some(u16::from_le_bytes(file.get(at..at + 2)?.try_into().ok()?));
    let u32_at = |at: usize| Some(u32::from_le_bytes(file.get(at..at + 4)?.try_into().ok()?));

    let pe = u32_at(0x3C)? as usize;
    if file.get(pe..pe + 4)? != b"PE\0\0" {
        return None;
    }
    let sections = u16_at(pe + 6)? as usize;
    let optional = pe + 24;
    if u16_at(optional)? != 0x10B {
        return None; // not a 32-bit image
    }
    let section_table = optional + u16_at(pe + 20)? as usize;
    // Turn an address in the loaded image into a position in the file.
    let to_file = |rva: u32| -> Option<usize> {
        (0..sections).find_map(|index| {
            let header = section_table + index * 40;
            let virtual_address = u32_at(header + 12)?;
            let raw_size = u32_at(header + 16)?;
            let raw_offset = u32_at(header + 20)?;
            (rva >= virtual_address && rva < virtual_address + raw_size)
                .then(|| (rva - virtual_address + raw_offset) as usize)
        })
    };

    let exports_rva = u32_at(optional + 96)?;
    let exports_size = u32_at(optional + 100)?;
    let exports = to_file(exports_rva)?;
    let names = u32_at(exports + 24)? as usize;
    let functions_table = to_file(u32_at(exports + 28)?)?;
    let names_table = to_file(u32_at(exports + 32)?)?;
    let ordinals_table = to_file(u32_at(exports + 36)?)?;

    for index in 0..names {
        let name_at = to_file(u32_at(names_table + index * 4)?)?;
        let name = file.get(name_at..name_at + wanted.len() + 1)?;
        if &name[..wanted.len()] == wanted.as_bytes() && name[wanted.len()] == 0 {
            let ordinal = u16_at(ordinals_table + index * 2)? as usize;
            let offset = u32_at(functions_table + ordinal * 4)?;
            // An address inside the export table itself is a redirect to another DLL.
            let forwarded = offset >= exports_rva && offset < exports_rva + exports_size;
            return (!forwarded && offset != 0).then_some(offset);
        }
    }
    None
}

/// x86 thread entry point: `return SetWindowDisplayAffinity(hwnd, affinity);`
fn build_stub_x86(hwnd: u32, affinity: u32, function: u32) -> Vec<u8> {
    let mut code = Vec::with_capacity(20);
    code.push(0x68); //                       push affinity
    code.extend_from_slice(&affinity.to_le_bytes());
    code.push(0x68); //                       push hwnd
    code.extend_from_slice(&hwnd.to_le_bytes());
    code.push(0xB8); //                       mov  eax, function
    code.extend_from_slice(&function.to_le_bytes());
    code.extend_from_slice(&[0xFF, 0xD0]); // call eax   (it removes its own two arguments)
    code.extend_from_slice(&[0xC2, 0x04, 0x00]); // ret 4 (remove the thread parameter)
    code
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
    fn x86_stub_embeds_arguments_at_fixed_offsets() {
        let stub = build_stub_x86(0x0012_3456, 0x11, 0x7654_3210);
        assert_eq!(stub.len(), 20);
        assert_eq!((stub[0], &stub[1..5]), (0x68, &0x11u32.to_le_bytes()[..]));
        assert_eq!((stub[5], &stub[6..10]), (0x68, &0x0012_3456u32.to_le_bytes()[..]));
        assert_eq!((stub[10], &stub[11..15]), (0xB8, &0x7654_3210u32.to_le_bytes()[..]));
        assert_eq!(&stub[15..], &[0xFF, 0xD0, 0xC2, 0x04, 0x00]);
    }

    #[test]
    fn the_function_is_found_in_the_32_bit_user32() {
        let Some(offset) = user32_offset_x86() else {
            return; // no 32-bit subsystem on this machine
        };
        assert!(offset > 0x1000, "an offset inside the code section, got {offset:#x}");
        // Names that are not exported, or only share a prefix, are not found.
        let windows = std::env::var_os("WINDIR").unwrap();
        let file = std::fs::read(std::path::Path::new(&windows).join("SysWOW64").join("user32.dll")).unwrap();
        assert_eq!(export_offset(&file, "SetWindowDisplayAffinit"), None);
        assert_eq!(export_offset(&file, "NoSuchFunctionHere"), None);
        assert_ne!(export_offset(&file, "GetWindowDisplayAffinity"), Some(offset));
        assert_eq!(export_offset(b"not a dll", "SetWindowDisplayAffinity"), None);
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
