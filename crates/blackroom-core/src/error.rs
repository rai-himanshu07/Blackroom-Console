//! Stable error categories and codes (Doc 16 §51 `err001`; assessment C17
//! "canonical machine catalogue") and the `ErrorResponse` shape (Doc 16
//! §52 `err002`).
//!
//! `BlackroomError`'s `Display` impl and `user_message` must never carry a
//! password, TOTP value, Access Key, session secret, or stack trace
//! (Doc 16 §52) — callers are responsible for keeping secret material out
//! of `user_message`; this module never has access to any secret to leak.

use std::fmt;

/// Canonical machine error code catalogue (Doc 16 §51 `err001`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ErrorCode {
    AuthInvalid,
    AuthRateLimited,
    AuthTotpRequired,
    AuthAccessKeyRequired,
    AuthDeviceRevoked,

    HostUnavailable,
    HostUnsupported,

    SessionNotFound,
    SessionRevoked,
    SessionEpochMismatch,

    LeaseExpired,
    LeaseRevoked,
    LeaseInvalid,

    GnomeSessionUnavailable,
    MutterUnavailable,
    VirtualDisplayFailed,
    DisplayIsolationFailed,
    InputIsolationFailed,
    SessionLockFailed,

    PipewireUnavailable,
    WebrtcFailed,
    NetworkFailed,

    IpcUnauthorized,
    IpcInvalidMessage,
    IpcTimeout,

    EmergencyTriggered,
    EmergencyRecoveryFailed,

    DisplayRestoreFailed,
    InputRestoreFailed,
    RecoveryFailed,
}

/// Grouping used by `ErrorCode::category` (Doc 16 §51's blank-line-separated
/// groups, taken as the canonical category boundaries).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ErrorCategory {
    Auth,
    Host,
    Session,
    Lease,
    Platform,
    Media,
    Ipc,
    Emergency,
    Recovery,
}

/// Whether retrying the operation that produced an error is ever
/// appropriate (Doc 16 §53). `Conditional` means the caller must apply its
/// own recovery-state check; safety-critical operations must never be
/// retried automatically and indefinitely.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Retryable {
    Yes,
    No,
    Conditional,
}

impl ErrorCode {
    /// All catalogue entries, for exhaustiveness tests.
    pub const ALL: [ErrorCode; 30] = [
        ErrorCode::AuthInvalid,
        ErrorCode::AuthRateLimited,
        ErrorCode::AuthTotpRequired,
        ErrorCode::AuthAccessKeyRequired,
        ErrorCode::AuthDeviceRevoked,
        ErrorCode::HostUnavailable,
        ErrorCode::HostUnsupported,
        ErrorCode::SessionNotFound,
        ErrorCode::SessionRevoked,
        ErrorCode::SessionEpochMismatch,
        ErrorCode::LeaseExpired,
        ErrorCode::LeaseRevoked,
        ErrorCode::LeaseInvalid,
        ErrorCode::GnomeSessionUnavailable,
        ErrorCode::MutterUnavailable,
        ErrorCode::VirtualDisplayFailed,
        ErrorCode::DisplayIsolationFailed,
        ErrorCode::InputIsolationFailed,
        ErrorCode::SessionLockFailed,
        ErrorCode::PipewireUnavailable,
        ErrorCode::WebrtcFailed,
        ErrorCode::NetworkFailed,
        ErrorCode::IpcUnauthorized,
        ErrorCode::IpcInvalidMessage,
        ErrorCode::IpcTimeout,
        ErrorCode::EmergencyTriggered,
        ErrorCode::EmergencyRecoveryFailed,
        ErrorCode::DisplayRestoreFailed,
        ErrorCode::InputRestoreFailed,
        ErrorCode::RecoveryFailed,
    ];

    /// The `SCREAMING_SNAKE_CASE` wire/log identifier (Doc 16 §51).
    pub const fn as_str(self) -> &'static str {
        match self {
            ErrorCode::AuthInvalid => "AUTH_INVALID",
            ErrorCode::AuthRateLimited => "AUTH_RATE_LIMITED",
            ErrorCode::AuthTotpRequired => "AUTH_TOTP_REQUIRED",
            ErrorCode::AuthAccessKeyRequired => "AUTH_ACCESS_KEY_REQUIRED",
            ErrorCode::AuthDeviceRevoked => "AUTH_DEVICE_REVOKED",
            ErrorCode::HostUnavailable => "HOST_UNAVAILABLE",
            ErrorCode::HostUnsupported => "HOST_UNSUPPORTED",
            ErrorCode::SessionNotFound => "SESSION_NOT_FOUND",
            ErrorCode::SessionRevoked => "SESSION_REVOKED",
            ErrorCode::SessionEpochMismatch => "SESSION_EPOCH_MISMATCH",
            ErrorCode::LeaseExpired => "LEASE_EXPIRED",
            ErrorCode::LeaseRevoked => "LEASE_REVOKED",
            ErrorCode::LeaseInvalid => "LEASE_INVALID",
            ErrorCode::GnomeSessionUnavailable => "GNOME_SESSION_UNAVAILABLE",
            ErrorCode::MutterUnavailable => "MUTTER_UNAVAILABLE",
            ErrorCode::VirtualDisplayFailed => "VIRTUAL_DISPLAY_FAILED",
            ErrorCode::DisplayIsolationFailed => "DISPLAY_ISOLATION_FAILED",
            ErrorCode::InputIsolationFailed => "INPUT_ISOLATION_FAILED",
            ErrorCode::SessionLockFailed => "SESSION_LOCK_FAILED",
            ErrorCode::PipewireUnavailable => "PIPEWIRE_UNAVAILABLE",
            ErrorCode::WebrtcFailed => "WEBRTC_FAILED",
            ErrorCode::NetworkFailed => "NETWORK_FAILED",
            ErrorCode::IpcUnauthorized => "IPC_UNAUTHORIZED",
            ErrorCode::IpcInvalidMessage => "IPC_INVALID_MESSAGE",
            ErrorCode::IpcTimeout => "IPC_TIMEOUT",
            ErrorCode::EmergencyTriggered => "EMERGENCY_TRIGGERED",
            ErrorCode::EmergencyRecoveryFailed => "EMERGENCY_RECOVERY_FAILED",
            ErrorCode::DisplayRestoreFailed => "DISPLAY_RESTORE_FAILED",
            ErrorCode::InputRestoreFailed => "INPUT_RESTORE_FAILED",
            ErrorCode::RecoveryFailed => "RECOVERY_FAILED",
        }
    }

    /// Doc 16 §51's blank-line-separated grouping.
    pub const fn category(self) -> ErrorCategory {
        match self {
            ErrorCode::AuthInvalid
            | ErrorCode::AuthRateLimited
            | ErrorCode::AuthTotpRequired
            | ErrorCode::AuthAccessKeyRequired
            | ErrorCode::AuthDeviceRevoked => ErrorCategory::Auth,

            ErrorCode::HostUnavailable | ErrorCode::HostUnsupported => ErrorCategory::Host,

            ErrorCode::SessionNotFound
            | ErrorCode::SessionRevoked
            | ErrorCode::SessionEpochMismatch => ErrorCategory::Session,

            ErrorCode::LeaseExpired | ErrorCode::LeaseRevoked | ErrorCode::LeaseInvalid => {
                ErrorCategory::Lease
            }

            ErrorCode::GnomeSessionUnavailable
            | ErrorCode::MutterUnavailable
            | ErrorCode::VirtualDisplayFailed
            | ErrorCode::DisplayIsolationFailed
            | ErrorCode::InputIsolationFailed
            | ErrorCode::SessionLockFailed => ErrorCategory::Platform,

            ErrorCode::PipewireUnavailable | ErrorCode::WebrtcFailed | ErrorCode::NetworkFailed => {
                ErrorCategory::Media
            }

            ErrorCode::IpcUnauthorized | ErrorCode::IpcInvalidMessage | ErrorCode::IpcTimeout => {
                ErrorCategory::Ipc
            }

            ErrorCode::EmergencyTriggered | ErrorCode::EmergencyRecoveryFailed => {
                ErrorCategory::Emergency
            }

            ErrorCode::DisplayRestoreFailed
            | ErrorCode::InputRestoreFailed
            | ErrorCode::RecoveryFailed => ErrorCategory::Recovery,
        }
    }

    /// Doc 16 §53 retry semantics. Unlisted codes default to `No`
    /// (safety-critical operations must not be retried automatically).
    pub const fn retryable(self) -> Retryable {
        match self {
            ErrorCode::NetworkFailed => Retryable::Yes,
            ErrorCode::AuthInvalid | ErrorCode::SessionRevoked => Retryable::No,
            ErrorCode::GnomeSessionUnavailable
            | ErrorCode::MutterUnavailable
            | ErrorCode::PipewireUnavailable => Retryable::Conditional,
            ErrorCode::DisplayIsolationFailed => Retryable::No,
            _ => Retryable::No,
        }
    }
}

impl fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Browser/log-facing error shape (Doc 16 §52 `err002`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlackroomError {
    pub code: ErrorCode,
    pub category: ErrorCategory,
    pub retryable: Retryable,
    pub user_message: String,
    pub diagnostic_id: String,
}

impl BlackroomError {
    /// Builds an error with a fresh `diag_<ULID>` correlation ID
    /// (architecture.md §2 correlation-ID convention).
    pub fn new(code: ErrorCode, user_message: impl Into<String>) -> Self {
        Self {
            code,
            category: code.category(),
            retryable: code.retryable(),
            user_message: user_message.into(),
            diagnostic_id: format!("diag_{}", ulid::Ulid::generate()),
        }
    }
}

impl fmt::Display for BlackroomError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} ({}): {}",
            self.code, self.diagnostic_id, self.user_message
        )
    }
}

impl std::error::Error for BlackroomError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_catalogue_entry_has_a_screaming_snake_case_name() {
        assert_eq!(ErrorCode::ALL.len(), 30);
        for code in ErrorCode::ALL {
            let s = code.as_str();
            assert!(s.chars().all(|c| c.is_ascii_uppercase() || c == '_'));
        }
    }

    #[test]
    fn display_never_contains_known_secret_markers() {
        let err = BlackroomError::new(ErrorCode::AuthInvalid, "invalid credentials supplied");
        let rendered = err.to_string();
        for marker in ["password", "totp", "access_key", "secret"] {
            assert!(!rendered.to_lowercase().contains(marker));
        }
    }

    #[test]
    fn diagnostic_ids_are_unique_per_instance() {
        let a = BlackroomError::new(ErrorCode::NetworkFailed, "network unreachable");
        let b = BlackroomError::new(ErrorCode::NetworkFailed, "network unreachable");
        assert_ne!(a.diagnostic_id, b.diagnostic_id);
    }

    #[test]
    fn retry_semantics_match_doc16_section53_examples() {
        assert_eq!(ErrorCode::NetworkFailed.retryable(), Retryable::Yes);
        assert_eq!(ErrorCode::AuthInvalid.retryable(), Retryable::No);
        assert_eq!(ErrorCode::SessionRevoked.retryable(), Retryable::No);
        assert_eq!(
            ErrorCode::GnomeSessionUnavailable.retryable(),
            Retryable::Conditional
        );
        assert_eq!(ErrorCode::DisplayIsolationFailed.retryable(), Retryable::No);
    }
}
