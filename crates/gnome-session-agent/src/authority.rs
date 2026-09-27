use std::time::SystemTime;

use blackroom_core::epoch::SecurityEpoch;
use blackroom_core::error::{BlackroomError, ErrorCode};
use blackroom_core::lease::{ControlLease, InputAuthorization};
use blackroom_core::protocol::AuthorityUpdate;
use blackroom_core::state::State;
use blackroom_gnome::mutter::eis::EiConnection;
use ed25519_dalek::{Signature, VerifyingKey};
use reis::event::DeviceResumed;

/// Agent-owned input authority. No peer request supplies the verifier or an
/// authorization snapshot. Production hostd-to-agent trust is not yet wired.
pub struct InputAuthority {
    verifying_key: VerifyingKey,
    epoch: SecurityEpoch,
    session_id: String,
    state: State,
    credential: Option<(ControlLease, Signature)>,
    revoked: bool,
}

pub enum EiInput {
    KeyTap(u32),
    ButtonClick(u32),
    KeyChord(u32, u32),
    PointerMotion(f32, f32),
    ScrollDelta(f32, f32),
}

impl InputAuthority {
    pub fn new(verifying_key: VerifyingKey, epoch: SecurityEpoch, session_id: String) -> Self {
        Self {
            verifying_key,
            epoch,
            session_id,
            state: State::LocalLocked,
            credential: None,
            revoked: false,
        }
    }

    pub fn epoch(&self) -> SecurityEpoch {
        self.epoch
    }

    /// The caller must establish authentication/authorization outside this API;
    /// the offline gateway supplies only a synthetic grant.
    pub fn grant_control(
        &mut self,
        lease: ControlLease,
        signature: Signature,
    ) -> Result<(), BlackroomError> {
        self.credential = None;
        if self.revoked {
            return Err(BlackroomError::new(
                ErrorCode::LeaseRevoked,
                "control remains revoked until a new epoch",
            ));
        }
        let authorization = InputAuthorization {
            lease: &lease,
            signature: &signature,
            verifying_key: &self.verifying_key,
            authenticated: true,
            authorized: true,
            current_epoch: self.epoch,
            current_state: State::RemoteActive,
            current_session_id: &self.session_id,
            revoked: false,
            now: SystemTime::now(),
        };
        authorization.validate()?;
        self.credential = Some((lease, signature));
        Ok(())
    }

    pub fn set_state(&mut self, state: State) {
        self.state = state;
    }

    pub fn revoke(&mut self) {
        self.revoked = true;
        self.credential = None;
    }

    pub fn advance_epoch(&mut self, epoch: SecurityEpoch) {
        if epoch > self.epoch {
            self.epoch = epoch;
            self.credential = None;
            self.revoked = false;
        }
    }

    pub fn apply_host_update(&mut self, update: AuthorityUpdate) -> Result<(), BlackroomError> {
        match update {
            AuthorityUpdate::Grant { lease, signature } => {
                let signature = Signature::from_slice(&signature).map_err(|_| {
                    BlackroomError::new(ErrorCode::LeaseInvalid, "invalid host lease signature")
                })?;
                self.grant_control(lease, signature)?;
                self.state = State::RemoteActive;
            }
            AuthorityUpdate::Revoke { epoch } => {
                if epoch <= self.epoch {
                    return Err(BlackroomError::new(
                        ErrorCode::SessionEpochMismatch,
                        "host revocation must advance the epoch",
                    ));
                }
                self.revoke();
                self.advance_epoch(epoch);
                self.state = State::LocalLocked;
            }
        }
        Ok(())
    }

    pub fn fail_closed(&mut self) {
        self.revoke();
        self.state = State::LocalLocked;
    }

    pub fn send_ei(
        &self,
        connection: &mut EiConnection,
        device: &DeviceResumed,
        event: EiInput,
    ) -> Result<(), BlackroomError> {
        self.dispatch(event, |event, authorization| match event {
            EiInput::KeyTap(keycode) => connection.send_key_tap(authorization, device, keycode),
            EiInput::ButtonClick(button) => {
                connection.send_button_click(authorization, device, button)
            }
            EiInput::KeyChord(modifier, keycode) => {
                connection.send_key_chord(authorization, device, modifier, keycode)
            }
            EiInput::PointerMotion(dx, dy) => {
                connection.send_pointer_motion(authorization, device, dx, dy)
            }
            EiInput::ScrollDelta(dx, dy) => {
                connection.send_scroll_delta(authorization, device, dx, dy)
            }
        })
    }

    pub fn dispatch<T>(
        &self,
        event: T,
        send: impl FnOnce(T, &InputAuthorization<'_>) -> Result<(), BlackroomError>,
    ) -> Result<(), BlackroomError> {
        let (lease, signature) = self.credential.as_ref().ok_or_else(|| {
            BlackroomError::new(ErrorCode::AuthInvalid, "no authenticated control grant")
        })?;
        let authorization = InputAuthorization {
            lease,
            signature,
            verifying_key: &self.verifying_key,
            authenticated: true,
            authorized: true,
            current_epoch: self.epoch,
            current_state: self.state,
            current_session_id: &self.session_id,
            revoked: self.revoked,
            now: SystemTime::now(),
        };
        authorization.validate()?;
        send(event, &authorization)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use blackroom_core::lease::Capability;
    use ed25519_dalek::SigningKey;
    use remote_hostd::OfflineHostAuthority;
    use std::cell::Cell;
    use std::time::Duration;

    #[test]
    fn host_changes_stop_dispatch_to_a_fake_sink() {
        let key = SigningKey::from_bytes(&[7; 32]);
        let mut authority = InputAuthority::new(
            key.verifying_key(),
            SecurityEpoch::INITIAL,
            "session-a".into(),
        );
        let sent = Cell::new(0);
        let send = |event, _: &InputAuthorization<'_>| {
            sent.set(sent.get() + event);
            Ok(())
        };
        assert!(authority.dispatch(1, send).is_err());

        let now = SystemTime::now();
        let lease = ControlLease {
            session_id: "session-a".into(),
            host_id: "host-a".into(),
            user_id: "user-a".into(),
            client_id: "client-a".into(),
            security_epoch: SecurityEpoch::INITIAL,
            issued_at: now,
            expires_at: now + Duration::from_secs(60),
            capabilities: vec![Capability::Control],
        };
        authority
            .grant_control(lease.clone(), lease.sign(&key))
            .unwrap();
        assert!(authority.dispatch(1, send).is_err());
        authority.set_state(State::RemoteActive);
        authority.dispatch(1, send).unwrap();
        authority.revoke();
        assert!(authority.dispatch(1, send).is_err());
        assert!(
            authority
                .grant_control(lease.clone(), lease.sign(&key))
                .is_err()
        );
        authority.advance_epoch(SecurityEpoch::INITIAL.next());
        assert!(authority.dispatch(1, send).is_err());
        assert!(
            authority
                .grant_control(lease.clone(), lease.sign(&key))
                .is_err()
        );
        assert_eq!(sent.get(), 1);
    }

    #[test]
    fn invalid_replacement_grant_clears_previous_control() {
        let key = SigningKey::from_bytes(&[7; 32]);
        let other_key = SigningKey::from_bytes(&[8; 32]);
        let now = SystemTime::now();
        let lease = ControlLease {
            session_id: "session-a".into(),
            host_id: "host-a".into(),
            user_id: "user-a".into(),
            client_id: "client-a".into(),
            security_epoch: SecurityEpoch::INITIAL,
            issued_at: now,
            expires_at: now + Duration::from_secs(60),
            capabilities: vec![Capability::Control],
        };
        let mut authority = InputAuthority::new(
            key.verifying_key(),
            SecurityEpoch::INITIAL,
            "session-a".into(),
        );
        authority
            .grant_control(lease.clone(), lease.sign(&key))
            .unwrap();
        authority.set_state(State::RemoteActive);
        assert!(authority.dispatch((), |_, _| Ok(())).is_ok());
        assert!(
            authority
                .grant_control(lease.clone(), lease.sign(&other_key))
                .is_err()
        );
        assert_eq!(
            authority.dispatch((), |_, _| Ok(())).unwrap_err().code,
            ErrorCode::AuthInvalid
        );
    }

    #[test]
    fn host_update_transitions_and_rejects_stale_epoch() {
        let mut host = OfflineHostAuthority::new();
        let mut authority = InputAuthority::new(
            host.verifying_key(),
            host.epoch(),
            remote_hostd::SIMULATED_SESSION_ID.into(),
        );
        let old_grant = host.grant_update().unwrap();
        authority.apply_host_update(old_grant.clone()).unwrap();
        assert!(authority.dispatch((), |_, _| Ok(())).is_ok());
        authority.apply_host_update(host.revoke_update()).unwrap();
        assert!(authority.dispatch((), |_, _| Ok(())).is_err());
        assert_eq!(
            authority.apply_host_update(old_grant).unwrap_err().code,
            ErrorCode::SessionEpochMismatch
        );
        authority
            .apply_host_update(host.grant_update().unwrap())
            .unwrap();
        assert!(authority.dispatch((), |_, _| Ok(())).is_ok());
    }

    #[test]
    fn agent_authority_gates_a_synthetic_eis_socket() -> Result<(), Box<dyn std::error::Error>> {
        use std::os::fd::OwnedFd;
        use std::os::unix::net::UnixStream;
        use std::sync::mpsc;
        use std::thread;
        use std::time::Instant;

        let (client, server_socket) = UnixStream::pair()?;
        let (done_tx, done_rx) = mpsc::channel();
        let server = thread::spawn(move || -> Result<(), std::io::Error> {
            let context = reis::eis::Context::new(server_socket)?;
            let mut handshake = reis::handshake::EisHandshaker::new(&context, 1);
            let mut converter: Option<reis::request::EisRequestConverter> = None;
            let mut key_states = Vec::new();
            let deadline = Instant::now() + Duration::from_secs(2);
            while Instant::now() < deadline {
                match context.read() {
                    Ok(_) => {}
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::yield_now();
                        continue;
                    }
                    Err(error) => return Err(error),
                }
                while let Some(request) = context.pending_request() {
                    let reis::PendingRequestResult::Request(request) = request else {
                        return Err(std::io::Error::other("invalid synthetic EI request"));
                    };
                    if let Some(converter) = converter.as_mut() {
                        converter
                            .handle_request(request)
                            .map_err(|error| std::io::Error::other(error.to_string()))?;
                    } else if let Some(response) = handshake
                        .handle_request(request)
                        .map_err(|error| std::io::Error::other(error.to_string()))?
                    {
                        if response.context_type != reis::eis::handshake::ContextType::Sender {
                            return Err(std::io::Error::other("EI client must be a Sender"));
                        }
                        context
                            .flush()
                            .map_err(|error| std::io::Error::other(error.to_string()))?;
                        let converted =
                            reis::request::EisRequestConverter::new(&context, response, 1);
                        let _seat = converted.handle().add_seat(
                            Some("fake-keyboard"),
                            reis::request::DeviceCapability::Keyboard.into(),
                        );
                        context
                            .flush()
                            .map_err(|error| std::io::Error::other(error.to_string()))?;
                        converter = Some(converted);
                    }
                }
                if let Some(converter) = converter.as_mut() {
                    while let Some(request) = converter.next_request() {
                        match request {
                            reis::request::EisRequest::Bind(binding) => {
                                let device = binding.seat.add_device(
                                    Some("fake-device"),
                                    reis::eis::device::DeviceType::Virtual,
                                    reis::request::DeviceCapability::Keyboard.into(),
                                    |_| {},
                                );
                                device.resumed();
                                context
                                    .flush()
                                    .map_err(|error| std::io::Error::other(error.to_string()))?;
                            }
                            reis::request::EisRequest::KeyboardKey(key) => {
                                if key.key != 30 {
                                    return Err(std::io::Error::other("unexpected keycode"));
                                }
                                key_states.push(key.state);
                            }
                            reis::request::EisRequest::DeviceStopEmulating(_) => {
                                if key_states
                                    != [
                                        reis::eis::keyboard::KeyState::Press,
                                        reis::eis::keyboard::KeyState::Released,
                                    ]
                                {
                                    return Err(std::io::Error::other("missing key release"));
                                }
                                done_rx
                                    .recv_timeout(Duration::from_secs(1))
                                    .map_err(std::io::Error::other)?;
                                match context.read() {
                                    Err(error)
                                        if error.kind() == std::io::ErrorKind::WouldBlock => {}
                                    Ok(_) => {}
                                    Err(error) => return Err(error),
                                }
                                if context.pending_request().is_some() {
                                    return Err(std::io::Error::other(
                                        "input sent after revocation",
                                    ));
                                }
                                return Ok(());
                            }
                            _ => {}
                        }
                    }
                }
            }
            Err(std::io::Error::other("synthetic EIS peer timed out"))
        });

        let mut sender =
            EiConnection::from_fd(zbus::zvariant::OwnedFd::from(OwnedFd::from(client)))?;
        sender.handshake_sender(Duration::from_secs(2))?;
        let Some(reis::event::EiEvent::SeatAdded(seat)) =
            sender.next_event_until(Duration::from_secs(2))?
        else {
            return Err("synthetic EI seat missing".into());
        };
        sender.bind_seat(&seat, reis::event::DeviceCapability::Keyboard)?;
        assert!(matches!(
            sender.next_event_until(Duration::from_secs(2))?,
            Some(reis::event::EiEvent::DeviceAdded(_))
        ));
        let Some(reis::event::EiEvent::DeviceResumed(device)) =
            sender.next_event_until(Duration::from_secs(2))?
        else {
            return Err("synthetic EI device missing".into());
        };
        let key = SigningKey::from_bytes(&[7; 32]);
        let mut authority = InputAuthority::new(
            key.verifying_key(),
            SecurityEpoch::INITIAL,
            "session-a".into(),
        );
        assert_eq!(
            authority
                .send_ei(&mut sender, &device, EiInput::KeyTap(30))
                .unwrap_err()
                .code,
            ErrorCode::AuthInvalid
        );
        let now = SystemTime::now();
        let lease = ControlLease {
            session_id: "session-a".into(),
            host_id: "host-a".into(),
            user_id: "user-a".into(),
            client_id: "client-a".into(),
            security_epoch: SecurityEpoch::INITIAL,
            issued_at: now,
            expires_at: now + Duration::from_secs(30),
            capabilities: vec![Capability::Control],
        };
        authority.grant_control(lease.clone(), lease.sign(&key))?;
        authority.set_state(State::RemoteActive);
        authority.send_ei(&mut sender, &device, EiInput::KeyTap(30))?;
        authority.revoke();
        assert_eq!(
            authority
                .send_ei(&mut sender, &device, EiInput::KeyTap(30))
                .unwrap_err()
                .code,
            ErrorCode::AuthInvalid
        );
        done_tx.send(())?;
        server.join().map_err(|_| "synthetic EIS peer panicked")??;
        Ok(())
    }
}
