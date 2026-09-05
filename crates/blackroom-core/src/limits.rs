//! Numeric defaults (`docs/security/architecture.md` §5 — reused verbatim,
//! none invented here; architecture.md itself cites assessment §6.5).
//!
//! These are runtime timeouts/rate-limits/sizes only. Doc 12/19 cycle-count
//! and soak-duration numbers are test-harness/CI parameters, not runtime
//! constants, and are out of scope for this module.

use std::time::Duration;

/// Authentication session TTL (Doc 07 §14 "relatively long-lived" auth vs
/// short lease).
pub const AUTH_SESSION_TTL: Duration = Duration::from_secs(5 * 60);

/// Session credential absolute maximum lifetime (Doc 03 §24-26).
pub const SESSION_CREDENTIAL_MAX_LIFETIME: Duration = Duration::from_secs(12 * 60 * 60);

/// Control lease TTL (Doc 07 §14; Doc 04 §53).
pub const CONTROL_LEASE_TTL: Duration = Duration::from_secs(30);

/// Control lease renewal interval.
pub const CONTROL_LEASE_RENEWAL_INTERVAL: Duration = Duration::from_secs(10);

/// Control lease heartbeat interval.
pub const CONTROL_LEASE_HEARTBEAT_INTERVAL: Duration = Duration::from_secs(5);

/// Maximum time `REMOTE_DEGRADED` may persist before a forced
/// `TEARING_DOWN` (Doc 07 §5.7) — bounded by the remaining lease TTL, never
/// more than [`CONTROL_LEASE_TTL`].
pub const REMOTE_DEGRADED_MAX: Duration = CONTROL_LEASE_TTL;

/// `PREPARING_REMOTE` per-step timeout (Doc 07 §14; Doc 16 §37).
pub const PREPARING_REMOTE_STEP_TIMEOUT: Duration = Duration::from_secs(10);

/// `PREPARING_REMOTE` total timeout.
pub const PREPARING_REMOTE_TOTAL_TIMEOUT: Duration = Duration::from_secs(60);

/// `TEARING_DOWN`/`RECOVERING` per-step timeout (Doc 19 §45).
pub const TEARDOWN_RECOVERY_STEP_TIMEOUT: Duration = Duration::from_secs(10);

/// `TEARING_DOWN`/`RECOVERING` total timeout, after which `FAILED_SAFE`
/// applies.
pub const TEARDOWN_RECOVERY_TOTAL_TIMEOUT: Duration = Duration::from_secs(60);

/// Emergency hotkey hold duration (`Ctrl+Alt+Shift+F12`; configurable at
/// runtime, this is the default).
pub const EMERGENCY_CHORD_HOLD: Duration = Duration::from_millis(2000);

/// Authentication failures allowed per `(client_id, account)` before
/// lockout (Doc 09 §48-adjacent; architecture.md §5).
pub const AUTH_RATE_LIMIT_FAILURES: u32 = 5;

/// Window over which [`AUTH_RATE_LIMIT_FAILURES`] applies.
pub const AUTH_RATE_LIMIT_WINDOW: Duration = Duration::from_secs(15 * 60);

/// Lockout duration once the failure limit is exceeded.
pub const AUTH_RATE_LIMIT_LOCKOUT: Duration = Duration::from_secs(15 * 60);

/// Per-source-IP request rate limit (requests per minute).
pub const AUTH_RATE_LIMIT_PER_IP_PER_MINUTE: u32 = 20;

/// Maximum concurrent authentication sessions (Doc 09 §48).
pub const MAX_CONCURRENT_AUTH_SESSIONS: u32 = 5;

/// Maximum IPC/WebSocket message size in bytes (Doc 16 §61).
pub const MAX_MESSAGE_SIZE_BYTES: usize = 64 * 1024;

/// Maximum individual input-event payload size in bytes.
pub const MAX_INPUT_EVENT_SIZE_BYTES: usize = 512;

/// Input event rate cap, events per second (Doc 19 §22; coalesce pointer
/// motion above this rate).
pub const INPUT_EVENT_RATE_CAP_PER_SECOND: u32 = 2000;

#[cfg(test)]
mod tests {
    use super::*;

    /// Pins every constant to the exact value recorded in
    /// `docs/security/architecture.md` §5 so a future edit here cannot
    /// silently drift from the documented source of truth.
    #[test]
    fn matches_architecture_md_section5() {
        assert_eq!(AUTH_SESSION_TTL, Duration::from_secs(300));
        assert_eq!(SESSION_CREDENTIAL_MAX_LIFETIME, Duration::from_secs(43_200));
        assert_eq!(CONTROL_LEASE_TTL, Duration::from_secs(30));
        assert_eq!(CONTROL_LEASE_RENEWAL_INTERVAL, Duration::from_secs(10));
        assert_eq!(CONTROL_LEASE_HEARTBEAT_INTERVAL, Duration::from_secs(5));
        assert_eq!(REMOTE_DEGRADED_MAX, Duration::from_secs(30));
        assert_eq!(PREPARING_REMOTE_STEP_TIMEOUT, Duration::from_secs(10));
        assert_eq!(PREPARING_REMOTE_TOTAL_TIMEOUT, Duration::from_secs(60));
        assert_eq!(TEARDOWN_RECOVERY_STEP_TIMEOUT, Duration::from_secs(10));
        assert_eq!(TEARDOWN_RECOVERY_TOTAL_TIMEOUT, Duration::from_secs(60));
        assert_eq!(EMERGENCY_CHORD_HOLD, Duration::from_millis(2000));
        assert_eq!(AUTH_RATE_LIMIT_FAILURES, 5);
        assert_eq!(AUTH_RATE_LIMIT_WINDOW, Duration::from_secs(900));
        assert_eq!(AUTH_RATE_LIMIT_LOCKOUT, Duration::from_secs(900));
        assert_eq!(AUTH_RATE_LIMIT_PER_IP_PER_MINUTE, 20);
        assert_eq!(MAX_CONCURRENT_AUTH_SESSIONS, 5);
        assert_eq!(MAX_MESSAGE_SIZE_BYTES, 65_536);
        assert_eq!(MAX_INPUT_EVENT_SIZE_BYTES, 512);
        assert_eq!(INPUT_EVENT_RATE_CAP_PER_SECOND, 2000);
    }
}
