use blackroom_core::error::{BlackroomError, ErrorCode};
use blackroom_core::state::State;
use blackroom_gnome::backend::GnomeBackend;
use blackroom_gnome::fake::{FakeGnomeBackend, FaultConfig};

use crate::authority::InputAuthority;

pub struct OfflineLifecycle {
    backend: FakeGnomeBackend,
    state: State,
    supported_connector: Option<String>,
}

impl OfflineLifecycle {
    pub fn new(faults: FaultConfig) -> Self {
        Self {
            backend: FakeGnomeBackend::new(faults),
            state: State::LocalLocked,
            supported_connector: None,
        }
    }

    pub fn state(&self) -> State {
        self.state
    }

    pub fn backend(&self) -> &FakeGnomeBackend {
        &self.backend
    }

    fn expected_output_present(&self, connectors: &[String]) -> bool {
        matches!(self.supported_connector.as_deref(), Some(expected) if connectors.len() == 1 && connectors[0] == expected)
    }

    pub fn confirm_active(&mut self, authority: &mut InputAuthority) -> Result<(), BlackroomError> {
        let result = (|| {
            Self::verify(self.state == State::RemoteActive, ErrorCode::RecoveryFailed)?;
            Self::verify(
                self.backend.observed_lock_state()?,
                ErrorCode::SessionLockFailed,
            )?;
            let display = self.backend.get_display_state()?;
            Self::verify(
                display.virtual_monitor_active
                    && self.expected_output_present(&display.connectors)
                    && self.backend.is_physical_outputs_disabled()
                    && self.backend.is_physical_input_isolated()
                    && self.backend.is_remote_input_enabled()
                    && self.backend.is_capturing(),
                ErrorCode::RecoveryFailed,
            )?;
            authority.dispatch((), |_, _| Ok(()))
        })();
        if result.is_err() {
            self.recover(authority);
        }
        result
    }

    pub fn activate(&mut self, authority: &mut InputAuthority) -> Result<(), BlackroomError> {
        if self.state != State::LocalLocked {
            return Err(BlackroomError::new(
                ErrorCode::RecoveryFailed,
                "offline recovery must complete before activation",
            ));
        }
        self.state = State::PreparingRemote;
        authority.set_state(State::PreparingRemote);
        let prepared = (|| {
            let session = self.backend.discover_session()?;
            if !session.active || !session.is_wayland {
                return Err(BlackroomError::new(
                    ErrorCode::GnomeSessionUnavailable,
                    "offline session is not usable",
                ));
            }
            let original = self.backend.get_display_state()?;
            Self::verify(
                original.connectors.len() == 1 && !original.connectors[0].is_empty(),
                ErrorCode::DisplayIsolationFailed,
            )?;
            self.supported_connector = Some(original.connectors[0].clone());
            self.backend.lock_session()?;
            Self::verify(
                self.backend.observed_lock_state()?,
                ErrorCode::SessionLockFailed,
            )?;
            self.backend.create_virtual_monitor()?;
            Self::verify(
                self.backend.get_display_state()?.virtual_monitor_active,
                ErrorCode::VirtualDisplayFailed,
            )?;
            self.backend.disable_physical_outputs()?;
            Self::verify(
                self.backend.is_physical_outputs_disabled(),
                ErrorCode::DisplayIsolationFailed,
            )?;
            self.backend.enable_remote_input()?;
            Self::verify(
                self.backend.is_remote_input_enabled(),
                ErrorCode::InputIsolationFailed,
            )?;
            self.backend.isolate_physical_input()?;
            Self::verify(
                self.backend.is_physical_input_isolated(),
                ErrorCode::InputIsolationFailed,
            )?;
            self.backend.start_capture()?;
            Self::verify(self.backend.is_capturing(), ErrorCode::PipewireUnavailable)?;
            Self::verify(
                self.backend.discover_session()? == session,
                ErrorCode::GnomeSessionUnavailable,
            )?;
            let display = self.backend.get_display_state()?;
            Self::verify(
                display.virtual_monitor_active
                    && self.expected_output_present(&display.connectors)
                    && self.backend.is_physical_outputs_disabled(),
                ErrorCode::DisplayIsolationFailed,
            )?;
            authority.set_state(State::RemoteActive);
            authority.dispatch((), |_, _| Ok(()))?;
            Ok(())
        })();
        if let Err(error) = prepared {
            self.recover(authority);
            return Err(error);
        }
        self.state = State::RemoteActive;
        Ok(())
    }

    fn verify(observed: bool, code: ErrorCode) -> Result<(), BlackroomError> {
        if observed {
            Ok(())
        } else {
            Err(BlackroomError::new(
                code,
                "offline safety effect not observed",
            ))
        }
    }

    pub fn recover(&mut self, authority: &mut InputAuthority) {
        authority.fail_closed();
        self.restore(authority);
    }

    pub fn recover_after_revoke(&mut self, authority: &mut InputAuthority) {
        self.restore(authority);
    }

    fn restore(&mut self, authority: &mut InputAuthority) {
        self.state = State::Recovering;
        let results = [
            self.backend.disable_remote_input(),
            self.backend.stop_capture(),
            self.backend.destroy_virtual_monitor(),
            self.backend.restore_physical_outputs(),
            self.backend.restore_physical_input(),
            self.backend.lock_session(),
        ];
        let verified = results.iter().all(Result::is_ok)
            && !self.backend.is_remote_input_enabled()
            && !self.backend.is_capturing()
            && !self.backend.is_virtual_monitor_active()
            && !self.backend.is_physical_outputs_disabled()
            && !self.backend.is_physical_input_isolated()
            && self.backend.observed_lock_state().unwrap_or(false);
        self.state = if verified {
            State::LocalLocked
        } else {
            authority.fail_closed();
            State::FailedSafe
        };
        self.supported_connector = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use blackroom_core::epoch::SecurityEpoch;
    use blackroom_gnome::fake::FaultMode;
    use remote_hostd::{OfflineHostAuthority, SIMULATED_SESSION_ID};

    fn setup(faults: FaultConfig) -> (OfflineHostAuthority, InputAuthority, OfflineLifecycle) {
        let host = OfflineHostAuthority::new();
        let authority = InputAuthority::new(
            host.verifying_key(),
            SecurityEpoch::INITIAL,
            SIMULATED_SESSION_ID.into(),
        );
        (host, authority, OfflineLifecycle::new(faults))
    }

    #[test]
    fn verified_start_then_revoke_restores_local_ownership_without_unlock() {
        let (mut host, mut authority, mut lifecycle) = setup(FaultConfig::new());
        authority
            .apply_host_update(host.grant_update().unwrap())
            .unwrap();
        lifecycle.activate(&mut authority).unwrap();
        assert_eq!(lifecycle.state(), State::RemoteActive);
        assert!(
            !lifecycle
                .backend()
                .physical_device_accepted("hotplugged-usb")
        );
        assert!(lifecycle.backend().emergency_chord_observable());
        authority.apply_host_update(host.revoke_update()).unwrap();
        lifecycle.recover(&mut authority);
        assert_eq!(lifecycle.state(), State::LocalLocked);
        assert!(lifecycle.backend().is_locked());
        assert!(
            lifecycle
                .backend()
                .physical_device_accepted("hotplugged-usb")
        );
        assert!(authority.dispatch((), |_, _| Ok(())).is_err());
        lifecycle.recover(&mut authority);
        assert_eq!(lifecycle.state(), State::LocalLocked);
    }

    #[test]
    fn new_physical_output_revokes_active_fake_control() {
        for connectors in [
            vec!["eDP-1".into(), "HDMI-1".into()],
            vec!["HDMI-1".into()],
            vec![],
        ] {
            let (mut host, mut authority, mut lifecycle) = setup(FaultConfig::new());
            authority
                .apply_host_update(host.grant_update().unwrap())
                .unwrap();
            lifecycle.activate(&mut authority).unwrap();
            lifecycle.backend.set_connectors(connectors);

            assert!(lifecycle.confirm_active(&mut authority).is_err());
            assert_ne!(lifecycle.state(), State::RemoteActive);
            assert!(authority.dispatch((), |_, _| Ok(())).is_err());
        }
    }

    #[test]
    fn sole_desktop_output_is_accepted_without_an_hdmi_fallback() {
        let (mut host, mut authority, mut lifecycle) = setup(FaultConfig::new());
        lifecycle.backend.set_connectors(vec!["DP-1".into()]);
        authority
            .apply_host_update(host.grant_update().unwrap())
            .unwrap();

        lifecycle.activate(&mut authority).unwrap();
        lifecycle.confirm_active(&mut authority).unwrap();
        assert_eq!(lifecycle.state(), State::RemoteActive);
        lifecycle.recover(&mut authority);
        assert_eq!(lifecycle.state(), State::LocalLocked);
    }

    #[test]
    fn absent_or_extra_output_refuses_activation_before_virtual_monitor() {
        for connectors in [vec![], vec!["eDP-1".into(), "HDMI-1".into()]] {
            let (mut host, mut authority, mut lifecycle) = setup(FaultConfig::new());
            lifecycle.backend.set_connectors(connectors);
            authority
                .apply_host_update(host.grant_update().unwrap())
                .unwrap();

            assert_eq!(
                lifecycle.activate(&mut authority).unwrap_err().code,
                ErrorCode::DisplayIsolationFailed
            );
            assert!(!lifecycle.backend().is_virtual_monitor_active());
            assert_ne!(lifecycle.state(), State::RemoteActive);
            assert!(authority.dispatch((), |_, _| Ok(())).is_err());
        }
    }

    #[test]
    fn silent_isolation_or_lock_failure_rolls_back_before_active() {
        for operation in [
            "observed_lock_state",
            "isolate_physical_input",
            "start_capture",
        ] {
            let (mut host, mut authority, mut lifecycle) =
                setup(FaultConfig::new().inject(operation, FaultMode::Partial));
            authority
                .apply_host_update(host.grant_update().unwrap())
                .unwrap();
            assert!(lifecycle.activate(&mut authority).is_err(), "{operation}");
            let expected = if operation == "observed_lock_state" {
                State::FailedSafe
            } else {
                State::LocalLocked
            };
            assert_eq!(lifecycle.state(), expected);
            assert!(authority.dispatch((), |_, _| Ok(())).is_err());
        }
    }

    #[test]
    fn restore_failure_is_explicit_failed_safe_and_cannot_restart() {
        let (mut host, mut authority, mut lifecycle) =
            setup(FaultConfig::new().inject("restore_physical_input", FaultMode::Partial));
        authority
            .apply_host_update(host.grant_update().unwrap())
            .unwrap();
        lifecycle.activate(&mut authority).unwrap();
        lifecycle.recover(&mut authority);
        assert_eq!(lifecycle.state(), State::FailedSafe);
        assert!(lifecycle.activate(&mut authority).is_err());
        assert!(authority.dispatch((), |_, _| Ok(())).is_err());
    }
}
