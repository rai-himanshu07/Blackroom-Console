//! Experiment 9a — Input device inventory (Document 10 Experiment 9 prerequisite; Phase 7 step 5).
//!
//! Read-only and unprivileged: parses `/proc/bus/input/devices`, `/sys/class/input`
//! and the world-readable udev database. It never opens `/dev/input`, takes no
//! grab and sends no input. It applies the Phase 7 classification
//! (`remote-input-helper`) to this host's real nodes and compares it with
//! libinput's udev tags. It proves nothing about the grab itself (FEAS-E).

use std::collections::BTreeSet;
use std::fs;

use blackroom_experiments::{
    CommonArgs, ExperimentReport, ExperimentResult, evidence_dir, write_evidence,
};
use clap::Parser;
use remote_input_helper::{Caps, Classification, Role, classify};
use serde::Serialize;
use time::OffsetDateTime;

const EXP_ID: &str = "exp09";
const EV_SW: u64 = 0x05;
const BUS_VIRTUAL: u64 = 0x06;

#[derive(Parser, Debug)]
#[command(about = "Experiment 9a: read-only input device inventory and grab classification")]
struct Args {
    #[command(flatten)]
    common: CommonArgs,
    /// Print the table but write no evidence.
    #[arg(long, default_value_t = false)]
    no_evidence: bool,
}

#[derive(Debug, Default, PartialEq, Eq)]
struct Block {
    bus: u64,
    name: String,
    phys: String,
    handlers: Vec<String>,
    ev: u64,
    keys: BTreeSet<u16>,
    rel: BTreeSet<u16>,
    abs: BTreeSet<u16>,
    has_switches: bool,
}

/// `B:` bitmaps are hex words, most significant first, 64 bits each.
fn bits(words: &str) -> BTreeSet<u16> {
    let mut set = BTreeSet::new();
    for (index, word) in words.split_whitespace().rev().enumerate() {
        let Ok(value) = u64::from_str_radix(word, 16) else {
            continue;
        };
        for bit in 0..64_u32 {
            if value >> bit & 1 == 1 {
                let code = index * 64 + bit as usize;
                if let Ok(code) = u16::try_from(code) {
                    set.insert(code);
                }
            }
        }
    }
    set
}

/// Parses the kernel list; `U:` (unique id) lines are skipped, never stored.
fn parse_devices(text: &str) -> Vec<Block> {
    let mut blocks = Vec::new();
    let mut current = Block::default();
    let mut started = false;
    for line in text.lines().chain(std::iter::once("")) {
        if line.is_empty() {
            if started {
                blocks.push(std::mem::take(&mut current));
                started = false;
            }
            continue;
        }
        started = true;
        let Some((tag, rest)) = line.split_once(": ") else {
            continue;
        };
        match tag {
            "I" => {
                current.bus = rest
                    .split_whitespace()
                    .find_map(|field| field.strip_prefix("Bus="))
                    .and_then(|bus| u64::from_str_radix(bus, 16).ok())
                    .unwrap_or(0);
            }
            "N" => {
                current.name = rest
                    .trim_start_matches("Name=")
                    .trim_matches('"')
                    .to_string()
            }
            "P" => current.phys = rest.trim_start_matches("Phys=").to_string(),
            "H" => {
                current.handlers = rest
                    .trim_start_matches("Handlers=")
                    .split_whitespace()
                    .map(str::to_string)
                    .collect();
            }
            "B" => {
                if let Some(value) = rest.strip_prefix("EV=") {
                    current.ev = u64::from_str_radix(value.trim(), 16).unwrap_or(0);
                } else if let Some(value) = rest.strip_prefix("KEY=") {
                    current.keys = bits(value);
                } else if let Some(value) = rest.strip_prefix("REL=") {
                    current.rel = bits(value);
                } else if let Some(value) = rest.strip_prefix("ABS=") {
                    current.abs = bits(value);
                } else if rest.starts_with("SW=") {
                    current.has_switches = true;
                }
            }
            _ => {}
        }
    }
    blocks
}

fn event_node(block: &Block) -> Option<u32> {
    block
        .handlers
        .iter()
        .find_map(|handler| handler.strip_prefix("event")?.parse().ok())
}

fn caps_of(block: &Block, seat0: bool) -> Caps {
    Caps {
        keys: block.keys.clone(),
        rel_axes: block.rel.clone(),
        abs_axes: block.abs.clone(),
        has_switches: block.has_switches || block.ev >> EV_SW & 1 == 1,
        bus_virtual: block.bus == BUS_VIRTUAL,
        seat0,
    }
}

#[derive(Debug, Default, Serialize)]
struct Udev {
    seat: Option<String>,
    keyboard: bool,
    mouse: bool,
    touchpad: bool,
    touchscreen: bool,
    pointingstick: bool,
    tablet: bool,
    readable: bool,
}

impl Udev {
    fn wants_grab(&self) -> bool {
        self.keyboard
            || self.mouse
            || self.touchpad
            || self.touchscreen
            || self.pointingstick
            || self.tablet
    }
}

fn read_udev(node: u32) -> Udev {
    let dev = fs::read_to_string(format!("/sys/class/input/event{node}/dev")).unwrap_or_default();
    let Ok(text) = fs::read_to_string(format!("/run/udev/data/c{}", dev.trim())) else {
        return Udev::default();
    };
    let flag = |name: &str| text.lines().any(|line| line == format!("E:{name}=1"));
    Udev {
        seat: text
            .lines()
            .find_map(|line| line.strip_prefix("E:ID_SEAT=").map(str::to_string)),
        keyboard: flag("ID_INPUT_KEYBOARD"),
        mouse: flag("ID_INPUT_MOUSE"),
        touchpad: flag("ID_INPUT_TOUCHPAD"),
        touchscreen: flag("ID_INPUT_TOUCHSCREEN"),
        pointingstick: flag("ID_INPUT_POINTINGSTICK"),
        tablet: flag("ID_INPUT_TABLET"),
        readable: true,
    }
}

#[derive(Debug, Serialize)]
struct Row {
    node: u32,
    name: String,
    phys: String,
    bus: String,
    handlers: Vec<String>,
    key_codes: usize,
    classification: String,
    grab: bool,
    udev: Udev,
    /// Only set when the udev entry was readable.
    agrees_with_udev: Option<bool>,
}

fn describe(classification: &Classification) -> (String, bool) {
    match classification {
        Classification::Grab(roles) => (
            format!(
                "grab:{}",
                roles
                    .iter()
                    .map(|role| match role {
                        Role::Keyboard => "keyboard",
                        Role::Pointer => "pointer",
                        Role::Touchpad => "touchpad",
                        Role::Touchscreen => "touchscreen",
                    })
                    .collect::<Vec<_>>()
                    .join("+")
            ),
            true,
        ),
        Classification::Skip(reason) => (format!("skip:{reason:?}"), false),
    }
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::try_init().ok();
    let args = Args::parse();
    let now = OffsetDateTime::now_utc();
    let text = fs::read_to_string("/proc/bus/input/devices")?;
    let mut rows = Vec::new();
    for block in parse_devices(&text) {
        let Some(node) = event_node(&block) else {
            continue;
        };
        let udev = read_udev(node);
        let seat0 = udev.seat.as_deref().is_none_or(|seat| seat == "seat0");
        let (classification, grab) = describe(&classify(&caps_of(&block, seat0)));
        let agrees = udev.readable.then(|| grab == udev.wants_grab());
        rows.push(Row {
            node,
            name: block.name.clone(),
            phys: block.phys.clone(),
            bus: format!("{:04x}", block.bus),
            handlers: block.handlers.clone(),
            key_codes: block.keys.len(),
            classification,
            grab,
            udev,
            agrees_with_udev: agrees,
        });
    }
    rows.sort_by_key(|row| row.node);
    for row in &rows {
        println!(
            "event{:<3} {:<34} {:<24} udev_grab={:<5} {}",
            row.node,
            row.name,
            row.classification,
            row.udev.wants_grab(),
            match row.agrees_with_udev {
                Some(true) => "",
                Some(false) => "MISMATCH",
                None => "(udev unreadable)",
            }
        );
    }
    let grabbed = rows.iter().filter(|row| row.grab).count();
    let mismatches = rows
        .iter()
        .filter(|row| row.agrees_with_udev == Some(false))
        .count();
    let has_keyboard = rows
        .iter()
        .any(|row| row.classification.contains("keyboard"));
    let has_pointing = rows.iter().any(|row| {
        ["pointer", "touchpad", "touchscreen"]
            .iter()
            .any(|r| row.classification.contains(r))
    });
    let sysrq_on_grabbed = rows
        .iter()
        .filter(|row| row.grab)
        .any(|row| row.handlers.iter().any(|handler| handler == "sysrq"));
    let result = if mismatches == 0 && has_keyboard && has_pointing {
        ExperimentResult::Pass
    } else {
        ExperimentResult::Partial
    };
    let observed = format!(
        "{} event nodes, {} would be grabbed, {} differ from udev tags; keyboard={}, pointing={}; \
         a grabbed node carries the kernel `sysrq` handler: {}",
        rows.len(),
        grabbed,
        mismatches,
        has_keyboard,
        has_pointing,
        sysrq_on_grabbed
    );
    println!("{observed}");
    if args.no_evidence {
        return Ok(());
    }
    let report = ExperimentReport {
        experiment: "Experiment 9a — Input device inventory (read-only)".to_string(),
        environment: "Development workstation (host = target), Ubuntu 26.04 / GNOME 50.1".to_string(),
        objective: "Apply the Phase 7 grab classification to this host's real input nodes without \
                    opening any device."
            .to_string(),
        hypothesis: "Capability-bit classification selects the built-in keyboard and touchpad nodes and \
                     skips power, lid and hotkey nodes, agreeing with libinput's udev tags."
            .to_string(),
        procedure: "Parse /proc/bus/input/devices, read /sys/class/input/event*/dev and \
                    /run/udev/data/c13:*, classify each node, compare with udev ID_INPUT_* tags. No \
                    /dev/input access, no grab, no input."
            .to_string(),
        expected: "Keyboard and pointing roles present, zero mismatches with udev.".to_string(),
        observed,
        evidence: vec!["inventory.json (this directory)".to_string()],
        result,
        failure: None,
        root_cause: None,
        security_impact: Some(
            "None: read-only procfs, sysfs and udev-database reads; unique ids are not recorded.".to_string(),
        ),
        recommended_action: None,
        follow_up: Some(
            "Phase 7 step 5 live grab needs its own approval; this does not prove FEAS-E.".to_string(),
        ),
    };
    let dir = evidence_dir(EXP_ID, now)?;
    write_evidence(&dir, &report.render(now), "inventory.json", &rows)?;
    println!("Wrote evidence to {}", dir.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "I: Bus=0011 Vendor=0001 Product=0001 Version=ab83\n\
N: Name=\"AT Translated Set 2 keyboard\"\n\
P: Phys=isa0060/serio0/input0\n\
U: Uniq=secret-id\n\
H: Handlers=sysrq kbd event2 leds \n\
B: EV=120013\n\
B: KEY=2000000000000000 0 40000 0 0 0 0 11100f02902007 f780307cfb10f001 feffffdfffcfffff fffffffffffffffe\n\
\n\
I: Bus=0019 Vendor=0000 Product=0005 Version=0000\n\
N: Name=\"Lid Switch\"\n\
H: Handlers=event0 \n\
B: EV=21\n\
B: SW=1\n";

    #[test]
    fn bitmaps_use_64_bit_words_most_significant_first() {
        assert_eq!(bits("30000 0 0 0 0"), BTreeSet::from([272, 273]));
        assert_eq!(bits("5"), BTreeSet::from([0, 2]));
        assert!(bits("zz").is_empty());
    }

    #[test]
    fn the_device_list_parses_without_keeping_unique_ids() {
        let blocks = parse_devices(SAMPLE);
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].name, "AT Translated Set 2 keyboard");
        assert_eq!(event_node(&blocks[0]), Some(2));
        assert!(blocks[0].handlers.contains(&"sysrq".to_string()));
        assert!(blocks[0].keys.contains(&30) && blocks[0].keys.contains(&16));
        assert!(!format!("{blocks:?}").contains("secret-id"));
        assert!(blocks[1].has_switches);
        assert_eq!(
            classify(&caps_of(&blocks[0], true)),
            Classification::Grab(vec![Role::Keyboard])
        );
        assert!(matches!(
            classify(&caps_of(&blocks[1], true)),
            Classification::Skip(_)
        ));
    }
}
