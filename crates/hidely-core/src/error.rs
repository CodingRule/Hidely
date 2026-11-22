use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CloakError {
    /// The window no longer exists.
    WindowGone,
    /// The target cannot be handled by this build (e.g. a 32-bit process).
    Unsupported(&'static str),
    /// The target runs at a higher integrity level or is a protected process.
    AccessDenied,
    /// A Win32 call failed.
    Os { call: &'static str, code: u32 },
    /// The call inside the target process did not finish in time.
    Timeout,
    /// `SetWindowDisplayAffinity` ran in the target and returned FALSE.
    Rejected,
}

impl fmt::Display for CloakError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WindowGone => write!(f, "the window no longer exists"),
            Self::Unsupported(why) => write!(f, "not supported: {why}"),
            Self::AccessDenied => write!(
                f,
                "access denied (the app is elevated or protected; try running Hidely as administrator)"
            ),
            Self::Os { call, code } => write!(f, "{call} failed with Win32 error {code}"),
            Self::Timeout => write!(f, "the target app did not respond in time"),
            Self::Rejected => write!(f, "Windows refused to change this window's capture setting"),
        }
    }
}

impl std::error::Error for CloakError {}
