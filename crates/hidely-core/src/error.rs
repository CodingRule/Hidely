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
    /// The call reported success, but reading the setting back shows it did not take.
    NotVerified,
    /// The window now belongs to a different process than when the request was made.
    OwnerChanged,
}

impl CloakError {
    /// A short, stable identifier that is safe to put in a diagnostic export.
    pub fn code(&self) -> String {
        match self {
            Self::WindowGone => "WINDOW_GONE".into(),
            Self::Unsupported(_) => "UNSUPPORTED".into(),
            Self::AccessDenied => "ACCESS_DENIED".into(),
            Self::Os { call, code } => format!("OS_ERROR:{call}:{code}"),
            Self::Timeout => "TIMEOUT".into(),
            Self::Rejected => "REJECTED".into(),
            Self::NotVerified => "NOT_VERIFIED".into(),
            Self::OwnerChanged => "OWNER_CHANGED".into(),
        }
    }
}

impl fmt::Display for CloakError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WindowGone => write!(f, "the window no longer exists"),
            Self::Unsupported(why) => write!(f, "not supported: {why}"),
            Self::AccessDenied => write!(
                f,
                "access denied (the app runs as administrator or is protected; restart Hidely as administrator to reach it)"
            ),
            Self::Os { call, code } => write!(f, "{call} failed with Win32 error {code}"),
            Self::Timeout => write!(f, "the target app did not respond in time"),
            Self::Rejected => write!(f, "Windows refused to change this window's capture setting"),
            Self::NotVerified => write!(
                f,
                "Windows reported success, but the window's capture setting did not change"
            ),
            Self::OwnerChanged => {
                write!(f, "the window changed hands before it could be updated, so it was left alone")
            }
        }
    }
}

impl std::error::Error for CloakError {}
