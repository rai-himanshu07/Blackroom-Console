//! In-memory `GnomeBackend` fake with fault injection: fail, timeout,
//! partial, duplicate, concurrent (Doc 10 §55 / Doc 12 §12, 18: "mock/fake
//! structuring ... resolved by the `GnomeBackend` trait boundary with an
//! in-memory fake").

use std::collections::HashMap;

use blackroom_core::error::{BlackroomError, ErrorCode};

use crate::backend::{CursorState, DisplayState, GnomeBackend, SessionInfo};

/// Method name a [`FaultConfig`] entry applies to (matches the Doc 05 §8
/// operation names on [`GnomeBackend`]).
pub type Operation = &'static str;

/// Fault mode to inject for a given operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FaultMode {
    /// No fault; behave normally.
    None,
    /// The operation returns an error immediately.
    Fail,
    /// The operation returns `IPC_TIMEOUT` (Doc 16 §37: never success).
    Timeout,
    /// The operation reports success but does not actually apply its
    /// effect — simulating the Doc 16 §38 "claims success, effect didn't
    /// take" shape that the Doc 07 §9 activation transaction's explicit
    /// "verify X" steps (12, 14, 17, 19, 20) exist to catch.
    Partial,
    /// No special single-call behavior; combine with calling the same
    /// operation twice in a test and asserting via [`FakeGnomeBackend::call_count`]
    /// that the effect is idempotent (Doc 07 §27).
    Duplicate,
    /// No special single-call behavior; combine with invoking the backend
    /// from multiple threads (behind `blackroom_core::lock::StateMachineLock`
    /// in the caller) and asserting via `call_count` that no call was lost
    /// or double-counted.
    Concurrent,
}

/// Which fault to inject per operation name. Unlisted operations behave
/// normally.
#[derive(Debug, Default, Clone)]
pub struct FaultConfig {
    faults: HashMap<Operation, FaultMode>,
}

impl FaultConfig {
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn inject(mut self, operation: Operation, mode: FaultMode) -> Self {
        self.faults.insert(operation, mode);
        self
    }

    fn mode_for(&self, operation: Operation) -> FaultMode {
        self.faults
            .get(operation)
            .copied()
            .unwrap_or(FaultMode::None)
    }
}

enum FaultDecision {
    ApplyEffect,
    SkipEffect,
}

/// In-memory [`GnomeBackend`] fake.
pub struct FakeGnomeBackend {
    faults: FaultConfig,
    virtual_monitor_active: bool,
    physical_outputs_disabled: bool,
    physical_input_isolated: bool,
    remote_input_enabled: bool,
    capturing: bool,
    locked: bool,
    call_counts: HashMap<Operation, u32>,
}

impl FakeGnomeBackend {
    pub fn new(faults: FaultConfig) -> Self {
        Self {
            faults,
            virtual_monitor_active: false,
            physical_outputs_disabled: false,
            physical_input_isolated: false,
            remote_input_enabled: false,
            capturing: false,
            locked: false,
            call_counts: HashMap::new(),
        }
    }

    /// Number of times `operation` has been invoked so far (for
    /// duplicate/concurrent fault-mode assertions).
    pub fn call_count(&self, operation: Operation) -> u32 {
        self.call_counts.get(operation).copied().unwrap_or(0)
    }

    pub fn is_locked(&self) -> bool {
        self.locked
    }

    pub fn is_remote_input_enabled(&self) -> bool {
        self.remote_input_enabled
    }

    pub fn is_capturing(&self) -> bool {
        self.capturing
    }

    pub fn is_physical_input_isolated(&self) -> bool {
        self.physical_input_isolated
    }

    pub fn physical_device_accepted(&self, _device: &str) -> bool {
        !self.physical_input_isolated
    }

    pub fn emergency_chord_observable(&self) -> bool {
        true
    }

    pub fn isolate_physical_input(&mut self) -> Result<(), BlackroomError> {
        if let FaultDecision::ApplyEffect =
            self.decide("isolate_physical_input", ErrorCode::InputIsolationFailed)?
        {
            self.physical_input_isolated = true;
        }
        Ok(())
    }

    pub fn restore_physical_input(&mut self) -> Result<(), BlackroomError> {
        if let FaultDecision::ApplyEffect =
            self.decide("restore_physical_input", ErrorCode::InputRestoreFailed)?
        {
            self.physical_input_isolated = false;
        }
        Ok(())
    }

    pub fn observed_lock_state(&mut self) -> Result<bool, BlackroomError> {
        match self.decide("observed_lock_state", ErrorCode::SessionLockFailed)? {
            FaultDecision::ApplyEffect => Ok(self.locked),
            FaultDecision::SkipEffect => Ok(false),
        }
    }

    pub fn is_physical_outputs_disabled(&self) -> bool {
        self.physical_outputs_disabled
    }

    pub fn is_virtual_monitor_active(&self) -> bool {
        self.virtual_monitor_active
    }

    fn decide(
        &mut self,
        operation: Operation,
        fail_code: ErrorCode,
    ) -> Result<FaultDecision, BlackroomError> {
        *self.call_counts.entry(operation).or_insert(0) += 1;
        match self.faults.mode_for(operation) {
            FaultMode::Fail => Err(BlackroomError::new(
                fail_code,
                format!("{operation} failed (fault injection)"),
            )),
            FaultMode::Timeout => Err(BlackroomError::new(
                ErrorCode::IpcTimeout,
                format!("{operation} timed out (fault injection)"),
            )),
            FaultMode::Partial => Ok(FaultDecision::SkipEffect),
            FaultMode::None | FaultMode::Duplicate | FaultMode::Concurrent => {
                Ok(FaultDecision::ApplyEffect)
            }
        }
    }
}

impl Default for FakeGnomeBackend {
    fn default() -> Self {
        Self::new(FaultConfig::new())
    }
}

impl GnomeBackend for FakeGnomeBackend {
    fn discover_session(&mut self) -> Result<SessionInfo, BlackroomError> {
        self.decide("discover_session", ErrorCode::GnomeSessionUnavailable)?;
        Ok(SessionInfo {
            session_id: "fake-session".to_string(),
            uid: 1000,
            seat: "seat0".to_string(),
            is_wayland: true,
            active: true,
        })
    }

    fn get_display_state(&mut self) -> Result<DisplayState, BlackroomError> {
        self.decide("get_display_state", ErrorCode::MutterUnavailable)?;
        Ok(DisplayState {
            connectors: vec!["eDP-1".to_string()],
            virtual_monitor_active: self.virtual_monitor_active,
        })
    }

    fn create_virtual_monitor(&mut self) -> Result<(), BlackroomError> {
        if let FaultDecision::ApplyEffect =
            self.decide("create_virtual_monitor", ErrorCode::VirtualDisplayFailed)?
        {
            self.virtual_monitor_active = true;
        }
        Ok(())
    }

    fn destroy_virtual_monitor(&mut self) -> Result<(), BlackroomError> {
        if let FaultDecision::ApplyEffect =
            self.decide("destroy_virtual_monitor", ErrorCode::VirtualDisplayFailed)?
        {
            self.virtual_monitor_active = false;
        }
        Ok(())
    }

    fn disable_physical_outputs(&mut self) -> Result<(), BlackroomError> {
        if let FaultDecision::ApplyEffect = self.decide(
            "disable_physical_outputs",
            ErrorCode::DisplayIsolationFailed,
        )? {
            self.physical_outputs_disabled = true;
        }
        Ok(())
    }

    fn restore_physical_outputs(&mut self) -> Result<(), BlackroomError> {
        if let FaultDecision::ApplyEffect =
            self.decide("restore_physical_outputs", ErrorCode::DisplayRestoreFailed)?
        {
            self.physical_outputs_disabled = false;
        }
        Ok(())
    }

    fn enable_remote_input(&mut self) -> Result<(), BlackroomError> {
        if let FaultDecision::ApplyEffect =
            self.decide("enable_remote_input", ErrorCode::InputIsolationFailed)?
        {
            self.remote_input_enabled = true;
        }
        Ok(())
    }

    fn disable_remote_input(&mut self) -> Result<(), BlackroomError> {
        if let FaultDecision::ApplyEffect =
            self.decide("disable_remote_input", ErrorCode::InputRestoreFailed)?
        {
            self.remote_input_enabled = false;
        }
        Ok(())
    }

    fn start_capture(&mut self) -> Result<(), BlackroomError> {
        if let FaultDecision::ApplyEffect =
            self.decide("start_capture", ErrorCode::PipewireUnavailable)?
        {
            self.capturing = true;
        }
        Ok(())
    }

    fn stop_capture(&mut self) -> Result<(), BlackroomError> {
        if let FaultDecision::ApplyEffect =
            self.decide("stop_capture", ErrorCode::PipewireUnavailable)?
        {
            self.capturing = false;
        }
        Ok(())
    }

    fn lock_session(&mut self) -> Result<(), BlackroomError> {
        if let FaultDecision::ApplyEffect =
            self.decide("lock_session", ErrorCode::SessionLockFailed)?
        {
            self.locked = true;
        }
        Ok(())
    }

    fn get_cursor_state(&mut self) -> Result<CursorState, BlackroomError> {
        self.decide("get_cursor_state", ErrorCode::MutterUnavailable)?;
        Ok(CursorState {
            x: 0,
            y: 0,
            visible: !self.remote_input_enabled,
        })
    }

    fn restore_session(&mut self) -> Result<(), BlackroomError> {
        if let FaultDecision::ApplyEffect =
            self.decide("restore_session", ErrorCode::RecoveryFailed)?
        {
            // Invariant 8: restores local ownership only, never unlocks.
            self.remote_input_enabled = false;
            self.physical_outputs_disabled = false;
            self.virtual_monitor_active = false;
            self.capturing = false;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    use std::thread;

    #[test]
    fn physical_isolation_covers_hotplug_and_preserves_emergency_observation() {
        let mut backend = FakeGnomeBackend::default();
        backend.enable_remote_input().unwrap();
        backend.isolate_physical_input().unwrap();
        assert!(!backend.physical_device_accepted("internal-keyboard"));
        assert!(!backend.physical_device_accepted("new-usb-keyboard"));
        assert!(backend.is_remote_input_enabled());
        assert!(backend.emergency_chord_observable());
        backend.restore_physical_input().unwrap();
        assert!(backend.physical_device_accepted("new-usb-keyboard"));
    }

    #[test]
    fn lock_and_input_isolation_require_observed_effects() {
        let faults = FaultConfig::new()
            .inject("observed_lock_state", FaultMode::Partial)
            .inject("isolate_physical_input", FaultMode::Partial);
        let mut backend = FakeGnomeBackend::new(faults);
        backend.lock_session().unwrap();
        assert!(!backend.observed_lock_state().unwrap());
        backend.isolate_physical_input().unwrap();
        assert!(!backend.is_physical_input_isolated());
    }

    #[test]
    fn normal_operation_applies_effects() {
        let mut backend = FakeGnomeBackend::default();
        backend.create_virtual_monitor().unwrap();
        assert!(backend.is_virtual_monitor_active());
        backend.disable_physical_outputs().unwrap();
        assert!(backend.is_physical_outputs_disabled());
        backend.enable_remote_input().unwrap();
        assert!(backend.is_remote_input_enabled());
        backend.lock_session().unwrap();
        assert!(backend.is_locked());
    }

    #[test]
    fn fail_mode_returns_an_error_and_does_not_apply() {
        let faults = FaultConfig::new().inject("create_virtual_monitor", FaultMode::Fail);
        let mut backend = FakeGnomeBackend::new(faults);
        let err = backend.create_virtual_monitor().unwrap_err();
        assert_eq!(err.code, ErrorCode::VirtualDisplayFailed);
        assert!(!backend.is_virtual_monitor_active());
    }

    #[test]
    fn timeout_mode_returns_ipc_timeout_never_success() {
        let faults = FaultConfig::new().inject("disable_physical_outputs", FaultMode::Timeout);
        let mut backend = FakeGnomeBackend::new(faults);
        let err = backend.disable_physical_outputs().unwrap_err();
        assert_eq!(err.code, ErrorCode::IpcTimeout);
        assert!(!backend.is_physical_outputs_disabled());
    }

    /// Doc 16 §38's exact shape: an operation can report success while the
    /// effect silently did not take — this is exactly why the Doc 07 §9
    /// activation transaction has separate "verify" steps.
    #[test]
    fn partial_mode_reports_success_but_skips_the_effect() {
        let faults = FaultConfig::new().inject("enable_remote_input", FaultMode::Partial);
        let mut backend = FakeGnomeBackend::new(faults);
        assert!(backend.enable_remote_input().is_ok());
        assert!(
            !backend.is_remote_input_enabled(),
            "a verify step must catch this lie"
        );
    }

    #[test]
    fn duplicate_mode_call_count_reflects_repeated_invocation() {
        let faults = FaultConfig::new().inject("lock_session", FaultMode::Duplicate);
        let mut backend = FakeGnomeBackend::new(faults);
        backend.lock_session().unwrap();
        backend.lock_session().unwrap();
        assert_eq!(backend.call_count("lock_session"), 2);
        assert!(backend.is_locked());
    }

    /// Doc 07 §27's named idempotent operations, each called twice: the
    /// second call must succeed (not error) and leave the same observable
    /// state as the first — "safe to call repeatedly ... because crash
    /// recovery may repeat cleanup".
    #[test]
    fn lock_session_is_idempotent() {
        let mut backend = FakeGnomeBackend::default();
        assert!(backend.lock_session().is_ok());
        assert!(
            backend.lock_session().is_ok(),
            "calling lock_session again must not error"
        );
        assert!(backend.is_locked());
        assert_eq!(backend.call_count("lock_session"), 2);
    }

    #[test]
    fn disable_remote_input_is_idempotent() {
        let mut backend = FakeGnomeBackend::default();
        backend.enable_remote_input().unwrap();
        assert!(backend.disable_remote_input().is_ok());
        assert!(
            backend.disable_remote_input().is_ok(),
            "calling disable_remote_input when already disabled must not error"
        );
        assert!(!backend.is_remote_input_enabled());
    }

    #[test]
    fn restore_physical_outputs_is_idempotent() {
        let mut backend = FakeGnomeBackend::default();
        backend.disable_physical_outputs().unwrap();
        assert!(backend.restore_physical_outputs().is_ok());
        assert!(
            backend.restore_physical_outputs().is_ok(),
            "calling restore_physical_outputs when already restored must not error"
        );
        assert!(!backend.is_physical_outputs_disabled());
    }

    #[test]
    fn destroy_virtual_monitor_is_idempotent() {
        let mut backend = FakeGnomeBackend::default();
        backend.create_virtual_monitor().unwrap();
        assert!(backend.destroy_virtual_monitor().is_ok());
        assert!(
            backend.destroy_virtual_monitor().is_ok(),
            "calling destroy_virtual_monitor when it no longer exists must not error"
        );
        assert!(!backend.is_virtual_monitor_active());
    }

    /// `Concurrent` fault mode: every `GnomeBackend` method takes `&mut
    /// self`, so Rust's borrow checker already forbids a true data race on
    /// a bare `FakeGnomeBackend` — real concurrent access can only happen
    /// through external synchronization (a shared `Mutex`, mirroring how a
    /// real `gnome-session-agent` would guard its own backend handle
    /// behind `StateMachineLock`). This proves that under that
    /// synchronization, every concurrent call is still counted and
    /// applied — none lost, none double-applied beyond what was requested.
    #[test]
    fn concurrent_calls_through_a_shared_mutex_are_all_counted() {
        let faults = FaultConfig::new().inject("lock_session", FaultMode::Concurrent);
        let backend = Arc::new(Mutex::new(FakeGnomeBackend::new(faults)));
        let mut handles = Vec::new();
        for _ in 0..8 {
            let backend = Arc::clone(&backend);
            handles.push(thread::spawn(move || {
                backend.lock().unwrap().lock_session().unwrap();
            }));
        }
        for handle in handles {
            handle.join().unwrap();
        }
        let backend = backend.lock().unwrap();
        assert_eq!(backend.call_count("lock_session"), 8);
        assert!(backend.is_locked());
    }

    #[test]
    fn restore_session_never_unlocks() {
        let mut backend = FakeGnomeBackend::default();
        backend.lock_session().unwrap();
        backend.enable_remote_input().unwrap();
        backend.restore_session().unwrap();
        assert!(!backend.is_remote_input_enabled());
        // Invariant 8: still locked — restore_session must not unlock.
        assert!(backend.is_locked());
    }
}
