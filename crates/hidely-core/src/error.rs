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

#[cfg(test)]
mod tests {
    use super::CloakError;

    fn errors() -> [CloakError; 8] {
        [
            CloakError::WindowGone,
            CloakError::Unsupported("32-bit apps are not supported"),
            CloakError::AccessDenied,
            CloakError::Os {
                call: "OpenProcess",
                code: 5,
            },
            CloakError::Timeout,
            CloakError::Rejected,
            CloakError::NotVerified,
            CloakError::OwnerChanged,
        ]
    }

    #[test]
    fn every_error_has_a_user_facing_message() {
        for error in errors() {
            let message = error.to_string();
            assert!(!message.trim().is_empty(), "{error:?} has an empty message");
            assert!(
                !message.ends_with('.'),
                "{error:?} message ends with a full stop: {message}"
            );
        }
    }

    #[test]
    fn access_denied_message_suggests_running_as_administrator() {
        assert!(CloakError::AccessDenied
            .to_string()
            .contains("administrator"));
    }

    #[test]
    fn each_error_has_its_documented_code() {
        let cases = [
            (CloakError::WindowGone, "WINDOW_GONE"),
            (CloakError::Unsupported("reason"), "UNSUPPORTED"),
            (CloakError::AccessDenied, "ACCESS_DENIED"),
            (CloakError::Timeout, "TIMEOUT"),
            (CloakError::Rejected, "REJECTED"),
            (CloakError::NotVerified, "NOT_VERIFIED"),
            (CloakError::OwnerChanged, "OWNER_CHANGED"),
        ];

        for (error, expected) in cases {
            assert_eq!(error.code(), expected);
        }
        assert_eq!(
            CloakError::Os {
                call: "OpenProcess",
                code: 5,
            }
            .code(),
            "OS_ERROR:OpenProcess:5"
        );
    }

    #[test]
    fn error_codes_are_searchable() {
        for error in errors() {
            let code = error.code();
            if let Some(rest) = code.strip_prefix("OS_ERROR:") {
                let (call, error_code) = rest
                    .split_once(':')
                    .expect("OS_ERROR code has call and code");
                assert!(!call.is_empty(), "OS_ERROR code has an empty function name");
                assert!(error_code
                    .chars()
                    .all(|character| character.is_ascii_digit()));
            } else {
                assert!(
                    code.chars().all(|character| {
                        character.is_ascii_uppercase()
                            || character.is_ascii_digit()
                            || character == '_'
                    }),
                    "code contains a space or lowercase letter: {code}"
                );
            }
        }
    }
}
