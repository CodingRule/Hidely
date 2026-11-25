//! Hidely core: find top-level windows and exclude them from screen capture.
//!
//! Windows only lets a process change the display affinity of windows it owns, so
//! hiding another application's window means running `SetWindowDisplayAffinity`
//! *inside* that application. See [`inject`] for how that is done.

#![cfg(windows)]

pub mod capture;
pub mod cloaker;
pub mod engine;
pub mod model;
pub mod driver;
pub mod elevation;
mod error;
pub mod inject;
pub mod meeting;
pub mod overlay;
pub mod pointer;
pub mod system;
mod windows;

pub use cloaker::{data_dir, Cloaker};
pub use error::CloakError;
pub use windows::{
    display_affinity, icon_rgba, is_window, list_windows, os_version, owner_pid, process_identity,
    process_start_time, window_rect, window_scale, window_title, WindowInfo,
};

/// Window is captured normally.
pub const WDA_NONE: u32 = 0x0;
/// Window is left out of every capture (Windows 10 2004+).
pub const WDA_EXCLUDEFROMCAPTURE: u32 = 0x11;
