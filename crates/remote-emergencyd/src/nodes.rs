//! evdev implementation of [`Nodes`]: opens every physical input node it may need, reads events
//! without blocking, and takes or drops the exclusive grab. Adapted from the supervised live
//! probe (`exp09_grab_probe`, Phase 7 step 5). Key codes and coordinates are never stored or
//! logged; the only consumer of a code is the chord detector inside [`crate::core`].

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::ErrorKind;
use std::path::PathBuf;

use evdev::{BusType, Device, EventType};
use remote_input_helper::{Caps, Classification, DeviceGrab, DeviceId, GrabError, classify};

use crate::core::{Nodes, Observed};

/// `ENODEV`: the node was unplugged; the kernel already dropped any grab on it.
const ENODEV: i32 = 19;

pub fn event_number(name: &str) -> Option<DeviceId> {
    name.strip_prefix("event")?.parse().ok()
}

/// `ID_SEAT` from a udev database record; a record without one belongs to the default seat.
pub fn seat_from_udev_data(text: &str) -> String {
    text.lines()
        .find_map(|line| line.strip_prefix("E:ID_SEAT="))
        .unwrap_or("seat0")
        .to_string()
}

pub struct EvdevNodes {
    dev_root: PathBuf,
    devices: BTreeMap<DeviceId, Device>,
    caps: BTreeMap<DeviceId, Caps>,
    /// Nodes that opened fine but are not allow-listed; never retried.
    skipped: BTreeSet<DeviceId>,
}

impl EvdevNodes {
    pub fn new() -> Self {
        Self::with_root(PathBuf::from("/dev/input"))
    }

    pub fn with_root(dev_root: PathBuf) -> Self {
        Self {
            dev_root,
            devices: BTreeMap::new(),
            caps: BTreeMap::new(),
            skipped: BTreeSet::new(),
        }
    }

    /// Whether the node is on `seat0` per the udev database; `None` while the record is not
    /// readable (a just-plugged node may not have one yet), which is retried, not cached.
    fn seat0(node: DeviceId) -> Option<bool> {
        let dev = fs::read_to_string(format!("/sys/class/input/event{node}/dev")).ok()?;
        let text = fs::read_to_string(format!("/run/udev/data/c{}", dev.trim())).ok()?;
        Some(seat_from_udev_data(&text) == "seat0")
    }

    fn caps_of(device: &Device, seat0: bool) -> Caps {
        Caps {
            keys: device
                .supported_keys()
                .map(|keys| keys.iter().map(|key| key.code()).collect())
                .unwrap_or_default(),
            rel_axes: device
                .supported_relative_axes()
                .map(|axes| axes.iter().map(|axis| axis.0).collect())
                .unwrap_or_default(),
            abs_axes: device
                .supported_absolute_axes()
                .map(|axes| axes.iter().map(|axis| axis.0).collect())
                .unwrap_or_default(),
            has_switches: device.supported_switches().is_some(),
            bus_virtual: device.input_id().bus_type() == BusType::BUS_VIRTUAL,
            seat0,
        }
    }

    fn present_nodes(&self) -> BTreeSet<DeviceId> {
        fs::read_dir(&self.dev_root)
            .map(|entries| {
                entries
                    .filter_map(Result::ok)
                    .filter_map(|entry| event_number(&entry.file_name().to_string_lossy()))
                    .collect()
            })
            .unwrap_or_default()
    }

    fn forget(&mut self, node: DeviceId) {
        self.devices.remove(&node);
        self.caps.remove(&node);
    }
}

impl Default for EvdevNodes {
    fn default() -> Self {
        Self::new()
    }
}

impl DeviceGrab for EvdevNodes {
    fn grab(&mut self, id: DeviceId) -> Result<(), GrabError> {
        let device = self
            .devices
            .get_mut(&id)
            .ok_or_else(|| GrabError("node not open".to_string()))?;
        device.grab().map_err(|error| GrabError(error.to_string()))
    }

    /// A node that is already gone has no grab left to release.
    fn release(&mut self, id: DeviceId) -> Result<(), GrabError> {
        match self.devices.get_mut(&id) {
            Some(device) => device
                .ungrab()
                .or_else(|error| {
                    if error.raw_os_error() == Some(ENODEV) {
                        Ok(())
                    } else {
                        Err(error)
                    }
                })
                .map_err(|error| GrabError(error.to_string())),
            None => Ok(()),
        }
    }
}

impl Nodes for EvdevNodes {
    fn candidates(&mut self) -> Vec<(DeviceId, Caps)> {
        let present = self.present_nodes();
        let gone: Vec<DeviceId> = self
            .devices
            .keys()
            .filter(|id| !present.contains(id))
            .copied()
            .collect();
        for id in gone {
            self.forget(id);
        }
        self.skipped.retain(|id| present.contains(id));
        for node in present {
            if self.devices.contains_key(&node) || self.skipped.contains(&node) {
                continue;
            }
            let Ok(device) = Device::open(self.dev_root.join(format!("event{node}"))) else {
                continue;
            };
            if device.set_nonblocking(true).is_err() {
                continue;
            }
            let seat = Self::seat0(node);
            let caps = Self::caps_of(&device, seat.unwrap_or(false));
            match classify(&caps) {
                Classification::Grab(_) => {
                    self.caps.insert(node, caps);
                    self.devices.insert(node, device);
                }
                // An unknown seat fails closed now but is looked at again on the next rescan.
                Classification::Skip(_) if seat.is_none() => {}
                Classification::Skip(_) => {
                    self.skipped.insert(node);
                }
            }
        }
        self.caps
            .iter()
            .map(|(id, caps)| (*id, caps.clone()))
            .collect()
    }

    /// A state that cannot be read counts as a key down, so the gate fails closed.
    fn keys_down(&self) -> usize {
        self.devices
            .values()
            .map(|device| device.get_key_state().map_or(1, |keys| keys.iter().count()))
            .sum()
    }

    fn poll(&mut self) -> Result<Vec<Observed>, String> {
        let mut observed = Vec::new();
        let mut unplugged = Vec::new();
        for (id, device) in &mut self.devices {
            let mut active = false;
            for _ in 0..64 {
                match device.fetch_events() {
                    Ok(events) => {
                        for event in events {
                            match event.event_type() {
                                EventType::KEY if event.value() != 2 => {
                                    observed.push(Observed::Key {
                                        node: *id,
                                        code: event.code(),
                                        pressed: event.value() == 1,
                                    });
                                }
                                EventType::RELATIVE | EventType::ABSOLUTE => active = true,
                                _ => {}
                            }
                        }
                    }
                    Err(error) if error.kind() == ErrorKind::WouldBlock => break,
                    Err(error) if error.raw_os_error() == Some(ENODEV) => {
                        unplugged.push(*id);
                        break;
                    }
                    Err(error) => return Err(error.to_string()),
                }
            }
            if active {
                observed.push(Observed::Activity { node: *id });
            }
        }
        for id in unplugged {
            self.forget(id);
            observed.push(Observed::Removed { node: id });
        }
        Ok(observed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_event_nodes_are_numbered() {
        assert_eq!(event_number("event6"), Some(6));
        assert_eq!(event_number("event"), None);
        assert_eq!(event_number("mouse0"), None);
        assert_eq!(event_number("event6x"), None);
    }

    #[test]
    fn a_udev_record_without_a_seat_is_the_default_seat() {
        assert_eq!(
            seat_from_udev_data("E:ID_INPUT=1\nE:ID_INPUT_KEYBOARD=1\n"),
            "seat0"
        );
        assert_eq!(
            seat_from_udev_data("E:ID_SEAT=seat1\nE:ID_INPUT=1\n"),
            "seat1"
        );
        assert_eq!(seat_from_udev_data(""), "seat0");
    }

    #[test]
    fn a_missing_device_root_yields_no_candidates_and_nothing_to_release() {
        let mut nodes = EvdevNodes::with_root(PathBuf::from("/nonexistent/blackroom-test"));
        assert!(nodes.candidates().is_empty());
        assert_eq!(nodes.keys_down(), 0);
        assert_eq!(nodes.poll(), Ok(Vec::new()));
        assert_eq!(
            nodes.release(5),
            Ok(()),
            "an unknown node has no grab to release"
        );
        assert!(nodes.grab(5).is_err(), "an unknown node cannot be grabbed");
    }
}
