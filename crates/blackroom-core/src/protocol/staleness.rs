//! Stale-request rejection (Doc 16 §36) and timeout semantics (Doc 16
//! §37–§38): "a stale request must not overwrite newer state" and "never
//! interpret timeout as success".

use crate::epoch::SecurityEpoch;

/// Doc 16 §36: a message carrying an older security epoch than what is
/// currently known must be rejected — it must not overwrite newer state.
/// A message without an epoch (`None`) is not considered stale by this
/// check alone (some messages, e.g. early Plane A authentication, do not
/// yet have epoch context).
pub fn is_stale(current_epoch: SecurityEpoch, message_epoch: Option<u64>) -> bool {
    match message_epoch {
        Some(epoch) => epoch < current_epoch.value(),
        None => false,
    }
}

/// The outcome of an IPC/backend operation that can time out (Doc 16 §37).
/// `Timeout` is a distinct outcome from both `Success` and `Failure` so a
/// caller cannot accidentally treat it as success.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperationOutcome {
    Success,
    Failure,
    Timeout,
}

impl OperationOutcome {
    /// Doc 16 §37: "never interpret timeout as success" / §38: a `TIMEOUT`
    /// on any mandatory step blocks entry to `REMOTE_ACTIVE` and forces the
    /// rollback/recovery path. Both `Failure` and `Timeout` require it;
    /// only `Success` does not.
    pub const fn requires_recovery(self) -> bool {
        !matches!(self, OperationOutcome::Success)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn older_epoch_is_stale() {
        let current = SecurityEpoch::from_value(5);
        assert!(is_stale(current, Some(4)));
        assert!(!is_stale(current, Some(5)));
        assert!(!is_stale(current, Some(6)));
        assert!(!is_stale(current, None));
    }

    #[test]
    fn timeout_never_counts_as_success() {
        assert!(!OperationOutcome::Success.requires_recovery());
        assert!(OperationOutcome::Timeout.requires_recovery());
        assert!(OperationOutcome::Failure.requires_recovery());
    }
}
