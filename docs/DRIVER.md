# The kernel driver (optional, not built yet)

Hidely works without a driver. This page is about the planned v0.3 driver that would extend
hiding to windows the user-mode method can't reach (protected processes).

## What is in `driver/`

`driver/HideWindow.c` and `driver/protect.c` are the unmodified
[DWMShield](https://github.com/ahossu/DWMShield) sources by Alexandru Hossu, included with
permission under `driver/LICENCE`. The driver receives a window handle over an IOCTL and calls
the internal `win32kfull!GreProtectSpriteContent(0, hwnd, 1, 0x11)`, which is what
`SetWindowDisplayAffinity` ends up calling.

`crates/hidely-core/src/driver.rs` is a Rust port of the user-mode client (`protect.c`). The app
does not use it yet.

## Why it is not wired in

The upstream driver is a proof of concept. Before Hidely can ship it, these need solving:

1. **Hardcoded address.** `GRE_PROTECT_SPRITE_CONTENT_ADDR` changes on every boot (KASLR) and has
   to be found with WinDbg and compiled in. Calling a stale address is an instant blue screen.
   The driver needs to locate `win32kfull.sys` and resolve the function itself, and refuse to
   run if it can't verify what it found.
2. **No un-hide.** The affinity is always `0x11`. Hidely's panic key must be able to restore
   every window, so the IOCTL needs an affinity parameter.
3. **Open to everyone.** The device is created without a security descriptor and the IOCTL uses
   `FILE_ANY_ACCESS`, so any process on the machine can drive it. It should be restricted to
   administrators at least.
4. **No handle validation.** The handle from user mode is passed straight to a kernel function.
5. **Signing.** Loading it today needs test-signing mode, which lowers the security of the whole
   machine. A real release needs an attestation-signed driver.

## Building upstream as-is

You need Visual Studio with the Windows Driver Kit, a test machine or VM with test signing
enabled, and WinDbg to read the function address. Follow the instructions in the DWMShield
repository. Do this in a VM: a wrong address crashes Windows.
