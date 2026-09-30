//! Which evdev nodes are grabbed: by capability bits per node, never by name.

use std::collections::BTreeSet;

// Linux input-event-codes.h values.
const REL_X: u16 = 0x00;
const REL_Y: u16 = 0x01;
const ABS_X: u16 = 0x00;
const ABS_Y: u16 = 0x01;
const ABS_MT_POSITION_X: u16 = 0x35;
const ABS_MT_POSITION_Y: u16 = 0x36;
const BTN_LEFT: u16 = 0x110;
const BTN_TOUCH: u16 = 0x14a;
const BTN_TOOL_FINGER: u16 = 0x145;
/// Key codes of the letters A-Z on a standard keyboard.
const LETTER_KEYS: [u16; 26] = [
    16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 30, 31, 32, 33, 34, 35, 36, 37, 38, 44, 45, 46, 47, 48,
    49, 50,
];
/// A node with fewer letter keys is a hotkey or button node, not a keyboard.
const MIN_LETTER_KEYS: usize = 20;

/// What the kernel reports for one evdev node.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Caps {
    pub keys: BTreeSet<u16>,
    pub rel_axes: BTreeSet<u16>,
    pub abs_axes: BTreeSet<u16>,
    pub has_switches: bool,
    pub bus_virtual: bool,
    pub seat0: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Role {
    Keyboard,
    Pointer,
    Touchpad,
    Touchscreen,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Exclusion {
    NotSeat0,
    /// Software-created (uinput-style) node; not physical input.
    Virtual,
    /// Switch-only node such as a lid or tablet-mode switch.
    Switch,
    /// Power or sleep buttons, hotkey nodes and everything else without a role.
    NoInputRole,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Classification {
    /// Grab the whole node; a mixed keyboard and hotkey node is grabbed whole.
    Grab(Vec<Role>),
    Skip(Exclusion),
}

fn has_all(set: &BTreeSet<u16>, codes: &[u16]) -> bool {
    codes.iter().all(|code| set.contains(code))
}

pub fn classify(caps: &Caps) -> Classification {
    if !caps.seat0 {
        return Classification::Skip(Exclusion::NotSeat0);
    }
    if caps.bus_virtual {
        return Classification::Skip(Exclusion::Virtual);
    }
    let mut roles = Vec::new();
    let letters = LETTER_KEYS
        .iter()
        .filter(|code| caps.keys.contains(code))
        .count();
    if letters >= MIN_LETTER_KEYS {
        roles.push(Role::Keyboard);
    }
    let has_button = caps.keys.contains(&BTN_LEFT) || caps.keys.contains(&BTN_TOUCH);
    let rel_pointer = has_all(&caps.rel_axes, &[REL_X, REL_Y]) && caps.keys.contains(&BTN_LEFT);
    let abs_pointer = has_all(&caps.abs_axes, &[ABS_X, ABS_Y]) && has_button;
    let multitouch = has_all(&caps.abs_axes, &[ABS_MT_POSITION_X, ABS_MT_POSITION_Y]);
    if caps.keys.contains(&BTN_TOOL_FINGER) && (abs_pointer || multitouch) {
        roles.push(Role::Touchpad);
    } else if multitouch && caps.keys.contains(&BTN_TOUCH) {
        roles.push(Role::Touchscreen);
    } else if rel_pointer || abs_pointer {
        roles.push(Role::Pointer);
    }
    if !roles.is_empty() {
        return Classification::Grab(roles);
    }
    Classification::Skip(if caps.has_switches && caps.keys.is_empty() {
        Exclusion::Switch
    } else {
        Exclusion::NoInputRole
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(codes: &[u16]) -> BTreeSet<u16> {
        codes.iter().copied().collect()
    }

    fn base() -> Caps {
        Caps {
            seat0: true,
            ..Caps::default()
        }
    }

    fn keyboard() -> Caps {
        Caps {
            keys: LETTER_KEYS.iter().copied().collect(),
            ..base()
        }
    }

    #[test]
    fn keyboard_mouse_touchpad_and_touchscreen_are_grabbed_by_capability() {
        assert_eq!(
            classify(&keyboard()),
            Classification::Grab(vec![Role::Keyboard])
        );
        let mouse = Caps {
            keys: set(&[BTN_LEFT]),
            rel_axes: set(&[REL_X, REL_Y]),
            ..base()
        };
        assert_eq!(classify(&mouse), Classification::Grab(vec![Role::Pointer]));
        let touchpad = Caps {
            keys: set(&[BTN_LEFT, BTN_TOOL_FINGER, BTN_TOUCH]),
            abs_axes: set(&[ABS_X, ABS_Y, ABS_MT_POSITION_X, ABS_MT_POSITION_Y]),
            ..base()
        };
        assert_eq!(
            classify(&touchpad),
            Classification::Grab(vec![Role::Touchpad])
        );
        let touchscreen = Caps {
            keys: set(&[BTN_TOUCH]),
            abs_axes: set(&[ABS_X, ABS_Y, ABS_MT_POSITION_X, ABS_MT_POSITION_Y]),
            ..base()
        };
        assert_eq!(
            classify(&touchscreen),
            Classification::Grab(vec![Role::Touchscreen])
        );
    }

    #[test]
    fn a_combo_node_gets_both_roles_and_a_keyboard_with_hotkeys_is_grabbed_whole() {
        let mut combo = keyboard();
        combo.keys.extend([BTN_LEFT, 0x161, 0x164]);
        combo.rel_axes = set(&[REL_X, REL_Y]);
        assert_eq!(
            classify(&combo),
            Classification::Grab(vec![Role::Keyboard, Role::Pointer])
        );
    }

    #[test]
    fn power_buttons_hotkey_nodes_switches_virtual_and_other_seats_are_skipped() {
        let power = Caps {
            keys: set(&[116]),
            ..base()
        };
        assert_eq!(
            classify(&power),
            Classification::Skip(Exclusion::NoInputRole)
        );
        let hotkeys = Caps {
            keys: set(&[0xe0, 0xe1, 0x1d4]),
            ..base()
        };
        assert_eq!(
            classify(&hotkeys),
            Classification::Skip(Exclusion::NoInputRole)
        );
        let lid = Caps {
            has_switches: true,
            ..base()
        };
        assert_eq!(classify(&lid), Classification::Skip(Exclusion::Switch));
        let virtual_keyboard = Caps {
            bus_virtual: true,
            ..keyboard()
        };
        assert_eq!(
            classify(&virtual_keyboard),
            Classification::Skip(Exclusion::Virtual)
        );
        let other_seat = Caps {
            seat0: false,
            ..keyboard()
        };
        assert_eq!(
            classify(&other_seat),
            Classification::Skip(Exclusion::NotSeat0)
        );
    }

    #[test]
    fn a_node_with_a_few_letters_is_not_a_keyboard() {
        let remote = Caps {
            keys: set(&[16, 17, 18, 19, 20]),
            ..base()
        };
        assert_eq!(
            classify(&remote),
            Classification::Skip(Exclusion::NoInputRole)
        );
    }
}
