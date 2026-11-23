# The kernel driver (optional, not built, not tested)

Hidely works without a driver, and that covers 32-bit and 64-bit apps, and apps running as
administrator (restart Hidely as administrator). The only windows left are those of
*protected processes*, which Windows deliberately shields from every user-mode program,
administrators included. Reaching those would take a kernel driver. This page is about that
driver and why it is not part of Hidely yet.

## Status

Nothing in `driver/` is built, shipped or loaded by Hidely, and none of it has been tested as
part of this project. Three things stand in the way, and none of them can be worked around
in code:

1. **It cannot be built without the Windows Driver Kit**, and it cannot be loaded without
   either a Microsoft-signed build or Windows' test-signing mode, which lowers the security
   of the whole machine and has to be a deliberate choice by the machine's owner.
2. **It cannot be tested safely on a machine someone is using.** A mistake in a kernel
   driver is a blue screen. Development needs a throwaway virtual machine.
3. **The design is not safe to ship yet.** See the list below.

Hidely will not grow features that conceal processes or network activity, with or without a
driver. The driver's only intended job is the same capture exclusion the app already does.

## What is in `driver/`

`driver/HideWindow.c` and `driver/protect.c` are the
[DWMShield](https://github.com/ahossu/DWMShield) sources by Alexandru Hossu, included with
permission under `driver/LICENSE` and maintained here together with him. The driver receives a window handle over an IOCTL and calls
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
