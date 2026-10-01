//! Shared EIS plumbing for the supervised experiments (exp08, exp11): a throwaway signed control
//! lease, the latest resumed device per input kind, and a bounded event pump.

use std::process::Command;
use std::time::{Duration, Instant, SystemTime};

use blackroom_core::epoch::SecurityEpoch;
use blackroom_core::lease::{Capability, ControlLease, InputAuthorization};
use blackroom_core::state::State;
use blackroom_gnome::mutter::eis::EiConnection;
use ed25519_dalek::{Signature, SigningKey, VerifyingKey};
use reis::event::{Device, DeviceCapability, DeviceResumed, EiEvent};
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct DeviceSeen {
    event: &'static str,
    name: Option<String>,
    capabilities: Vec<&'static str>,
}

pub struct Authority {
    lease: ControlLease,
    signature: Signature,
    verifying_key: VerifyingKey,
}

impl Authority {
    pub fn new(ttl: Duration) -> Self {
        use getrandom::rand_core::UnwrapErr;
        let now = SystemTime::now();
        let signing_key = SigningKey::generate(&mut UnwrapErr(getrandom::SysRng));
        let lease = ControlLease {
            session_id: "rs_EXP".to_string(),
            host_id: "bc_EXP".to_string(),
            user_id: "exp".to_string(),
            client_id: "cl_EXP".to_string(),
            security_epoch: SecurityEpoch::INITIAL,
            issued_at: now,
            expires_at: now + ttl,
            capabilities: vec![Capability::View, Capability::Control],
        };
        Self {
            signature: lease.sign(&signing_key),
            verifying_key: signing_key.verifying_key(),
            lease,
        }
    }

    pub fn authorization(&self, revoked: bool) -> InputAuthorization<'_> {
        InputAuthorization {
            lease: &self.lease,
            signature: &self.signature,
            verifying_key: &self.verifying_key,
            authenticated: true,
            authorized: true,
            current_epoch: SecurityEpoch::INITIAL,
            current_state: State::RemoteActive,
            current_session_id: &self.lease.session_id,
            revoked,
            now: SystemTime::now(),
        }
    }
}

const ALL_CAPABILITIES: [(DeviceCapability, &str); 7] = [
    (DeviceCapability::Pointer, "pointer"),
    (DeviceCapability::PointerAbsolute, "pointer_absolute"),
    (DeviceCapability::Keyboard, "keyboard"),
    (DeviceCapability::Touch, "touch"),
    (DeviceCapability::Scroll, "scroll"),
    (DeviceCapability::Button, "button"),
    (DeviceCapability::Text, "text"),
];

fn describe(event: &'static str, device: &Device) -> DeviceSeen {
    DeviceSeen {
        event,
        name: device.name().map(str::to_owned),
        capabilities: ALL_CAPABILITIES
            .iter()
            .filter(|(capability, _)| device.has_capability(*capability))
            .map(|(_, label)| *label)
            .collect(),
    }
}

/// Latest resumed device per input kind the run needs.
#[derive(Default)]
pub struct Devices {
    pub keyboard: Option<DeviceResumed>,
    pub pointer: Option<DeviceResumed>,
    pub button: Option<DeviceResumed>,
    pub scroll: Option<DeviceResumed>,
}

impl Devices {
    fn slots(&mut self) -> [(&mut Option<DeviceResumed>, DeviceCapability); 4] {
        [
            (&mut self.keyboard, DeviceCapability::Keyboard),
            (&mut self.pointer, DeviceCapability::Pointer),
            (&mut self.button, DeviceCapability::Button),
            (&mut self.scroll, DeviceCapability::Scroll),
        ]
    }

    pub fn missing(&self) -> Vec<&'static str> {
        [
            (self.keyboard.is_none(), "keyboard"),
            (self.pointer.is_none(), "pointer"),
            (self.button.is_none(), "button"),
            (self.scroll.is_none(), "scroll"),
        ]
        .into_iter()
        .filter_map(|(absent, label)| absent.then_some(label))
        .collect()
    }

    fn clear(&mut self, device: &Device) {
        for (slot, _) in self.slots() {
            if slot
                .as_ref()
                .is_some_and(|resumed| &resumed.device == device)
            {
                *slot = None;
            }
        }
    }

    pub fn observe(&mut self, event: &EiEvent, seen: &mut Vec<DeviceSeen>) {
        match event {
            EiEvent::DeviceAdded(added) => seen.push(describe("added", &added.device)),
            EiEvent::DeviceResumed(resumed) => {
                seen.push(describe("resumed", &resumed.device));
                for (slot, capability) in self.slots() {
                    if resumed.device.has_capability(capability) {
                        *slot = Some(resumed.clone());
                    }
                }
            }
            EiEvent::DevicePaused(paused) => self.clear(&paused.device),
            EiEvent::DeviceRemoved(removed) => {
                seen.push(describe("removed", &removed.device));
                self.clear(&removed.device);
            }
            EiEvent::SeatRemoved(_) | EiEvent::Disconnected(_) => *self = Self::default(),
            _ => {}
        }
    }
}

/// Binds the seat for keyboard, pointer, button and scroll and waits up to 5 s until each has a
/// resumed device. The error carries the failing step and its detail.
pub fn bind_devices(
    eis: &mut EiConnection,
    devices: &mut Devices,
    seen: &mut Vec<DeviceSeen>,
) -> Result<(), (&'static str, String)> {
    let mut bound = false;
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline && !(bound && devices.missing().is_empty()) {
        match eis
            .next_event_until(Duration::from_millis(250))
            .map_err(|error| ("EIS negotiation", error.to_string()))?
        {
            Some(EiEvent::SeatAdded(added)) if !bound => {
                eis.bind_seat(
                    &added,
                    DeviceCapability::Keyboard
                        | DeviceCapability::Pointer
                        | DeviceCapability::Button
                        | DeviceCapability::Scroll,
                )
                .map_err(|error| ("EIS seat bind", error.to_string()))?;
                bound = true;
            }
            Some(event) => devices.observe(&event, seen),
            None => {}
        }
    }
    if !bound || !devices.missing().is_empty() {
        let missing = devices.missing();
        return Err((
            "EIS negotiation",
            format!("seat bound={bound}, missing resumed devices: {missing:?}"),
        ));
    }
    Ok(())
}

/// Drains events for `duration`, keeping `devices` current. Returns the
/// transport error text if the socket closed while draining.
pub fn pump(
    eis: &mut EiConnection,
    devices: &mut Devices,
    seen: &mut Vec<DeviceSeen>,
    duration: Duration,
    mut on_event: impl FnMut(&EiEvent),
) -> Option<String> {
    let deadline = Instant::now() + duration;
    while Instant::now() < deadline {
        match eis.next_event_until(deadline.saturating_duration_since(Instant::now())) {
            Ok(Some(event)) => {
                devices.observe(&event, seen);
                on_event(&event);
            }
            Ok(None) => {}
            Err(error) => return Some(error.to_string()),
        }
    }
    None
}

pub fn command_line(program: &str, args: &[&str]) -> Option<String> {
    let output = Command::new(program).args(args).output().ok()?;
    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!text.is_empty()).then_some(text)
}

#[cfg(test)]
mod tests {
    use blackroom_core::error::ErrorCode;

    use super::*;

    #[test]
    fn authority_signs_a_lease_that_validates_and_revokes() {
        let authority = Authority::new(Duration::from_secs(30));
        assert!(authority.authorization(false).validate().is_ok());
        assert_eq!(
            authority
                .authorization(true)
                .validate()
                .map_err(|error| error.code),
            Err(ErrorCode::LeaseRevoked)
        );
    }
}
