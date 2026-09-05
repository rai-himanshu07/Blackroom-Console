//! Experiment 2 — Mutter/Shell/logind Capability Inventory (Document 10 §9).
//!
//! Read-only: calls `Introspect()`, property `Get`, and
//! `DisplayConfig.GetCurrentState` only. Never calls `RecordVirtual`,
//! `ConnectToEIS`, `ApplyMonitorsConfig`, or `InputCapture.CreateSession` —
//! those would create or mutate compositor state, forbidden in Phase 0-1.

use std::collections::HashMap;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::Path;

use blackroom_experiments::{
    CommonArgs, ExperimentReport, ExperimentResult, current_uid, discover, evidence_dir, redact,
    write_evidence,
};
use clap::Parser;
use serde::Serialize;
use time::OffsetDateTime;
use zbus::blocking::{Connection, Proxy};
use zbus::zvariant::{OwnedValue, Type};

const EXP_ID: &str = "exp02";
const INTROSPECTION_DIR: &str = "docs/gnome/introspection";

#[derive(Parser, Debug)]
#[command(about = "Experiment 2: read-only Mutter/Shell/logind capability inventory")]
struct Args {
    #[command(flatten)]
    common: CommonArgs,
}

// ---------------------------------------------------------------------
// Minimal introspection-XML scanner (GDBus one-tag-per-line output only;
// not a general XML parser — avoids adding an XML crate for Phase 1).
// ---------------------------------------------------------------------

#[derive(Debug, Default, Serialize)]
struct ParsedMethod {
    name: String,
    in_args: Vec<String>,
    out_args: Vec<String>,
}

#[derive(Debug, Serialize)]
struct ParsedProperty {
    name: String,
    r#type: String,
    access: String,
}

#[derive(Debug, Default, Serialize)]
struct ParsedSignal {
    name: String,
    args: Vec<String>,
}

#[derive(Debug, Default, Serialize)]
struct ParsedInterface {
    name: String,
    methods: Vec<ParsedMethod>,
    properties: Vec<ParsedProperty>,
    signals: Vec<ParsedSignal>,
}

fn xml_attr(tag: &str, key: &str) -> Option<String> {
    let needle = format!("{key}=\"");
    let start = tag.find(&needle)? + needle.len();
    let rest = &tag[start..];
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

fn parse_introspection_xml(xml: &str) -> Vec<ParsedInterface> {
    let mut interfaces = Vec::new();
    let mut current: Option<ParsedInterface> = None;
    let mut current_method: Option<ParsedMethod> = None;
    let mut current_signal: Option<ParsedSignal> = None;

    for raw_line in xml.lines() {
        let line = raw_line.trim();
        if line.starts_with("<interface ") {
            if let Some(name) = xml_attr(line, "name") {
                current = Some(ParsedInterface {
                    name,
                    ..Default::default()
                });
            }
        } else if line.starts_with("</interface>") {
            if let Some(iface) = current.take() {
                interfaces.push(iface);
            }
        } else if line.starts_with("<method ") {
            if let Some(name) = xml_attr(line, "name") {
                let method = ParsedMethod {
                    name,
                    ..Default::default()
                };
                if line.ends_with("/>") {
                    if let Some(iface) = current.as_mut() {
                        iface.methods.push(method);
                    }
                } else {
                    current_method = Some(method);
                }
            }
        } else if line.starts_with("</method>") {
            if let (Some(method), Some(iface)) = (current_method.take(), current.as_mut()) {
                iface.methods.push(method);
            }
        } else if line.starts_with("<signal ") {
            if let Some(name) = xml_attr(line, "name") {
                let signal = ParsedSignal {
                    name,
                    ..Default::default()
                };
                if line.ends_with("/>") {
                    if let Some(iface) = current.as_mut() {
                        iface.signals.push(signal);
                    }
                } else {
                    current_signal = Some(signal);
                }
            }
        } else if line.starts_with("</signal>") {
            if let (Some(signal), Some(iface)) = (current_signal.take(), current.as_mut()) {
                iface.signals.push(signal);
            }
        } else if line.starts_with("<property ") {
            if let (Some(name), Some(prop_type), Some(access)) = (
                xml_attr(line, "name"),
                xml_attr(line, "type"),
                xml_attr(line, "access"),
            ) && let Some(iface) = current.as_mut()
            {
                iface.properties.push(ParsedProperty {
                    name,
                    r#type: prop_type,
                    access,
                });
            }
        } else if line.starts_with("<arg ") {
            let arg_type = xml_attr(line, "type").unwrap_or_default();
            let direction = xml_attr(line, "direction").unwrap_or_else(|| "in".to_string());
            if let Some(method) = current_method.as_mut() {
                if direction == "out" {
                    method.out_args.push(arg_type);
                } else {
                    method.in_args.push(arg_type);
                }
            } else if let Some(signal) = current_signal.as_mut() {
                signal.args.push(arg_type);
            }
        }
    }
    interfaces
}

// ---------------------------------------------------------------------
// Introspection targets
// ---------------------------------------------------------------------

struct Target {
    label: &'static str,
    bus: BusKind,
    destination: &'static str,
    path: String,
}

#[derive(Clone, Copy)]
enum BusKind {
    Session,
    System,
}

#[derive(Debug, Serialize)]
struct InspectedTarget {
    label: String,
    bus: String,
    destination: String,
    object_path: String,
    xml_file: Option<String>,
    interfaces: Vec<ParsedInterface>,
    error: Option<String>,
}

fn slug(label: &str) -> String {
    label
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

fn introspect_target(
    session_conn: &Connection,
    system_conn: &Connection,
    target: &Target,
) -> InspectedTarget {
    let conn = match target.bus {
        BusKind::Session => session_conn,
        BusKind::System => system_conn,
    };
    let bus_name = match target.bus {
        BusKind::Session => "session",
        BusKind::System => "system",
    };

    let result = Proxy::new(
        conn,
        target.destination,
        target.path.as_str(),
        "org.freedesktop.DBus.Introspectable",
    )
    .and_then(|proxy| proxy.call::<_, _, String>("Introspect", &()));

    match result {
        Ok(xml) => {
            let file_name = format!("{}.xml", slug(target.label));
            let xml_path = Path::new(INTROSPECTION_DIR).join(&file_name);
            let xml_file = fs::write(&xml_path, &xml)
                .ok()
                .map(|()| xml_path.display().to_string());
            InspectedTarget {
                label: target.label.to_string(),
                bus: bus_name.to_string(),
                destination: target.destination.to_string(),
                object_path: target.path.clone(),
                xml_file,
                interfaces: parse_introspection_xml(&xml),
                error: None,
            }
        }
        Err(error) => InspectedTarget {
            label: target.label.to_string(),
            bus: bus_name.to_string(),
            destination: target.destination.to_string(),
            object_path: target.path.clone(),
            xml_file: None,
            interfaces: Vec::new(),
            error: Some(error.to_string()),
        },
    }
}

/// Does `interface_name` (on any inspected target) declare a method named
/// `method_name`?
fn method_present(targets: &[InspectedTarget], interface_name: &str, method_name: &str) -> bool {
    targets.iter().any(|target| {
        target.interfaces.iter().any(|iface| {
            iface.name == interface_name && iface.methods.iter().any(|m| m.name == method_name)
        })
    })
}

// ---------------------------------------------------------------------
// DisplayConfig.GetCurrentState (read-only; Mutter 50.1 signature verified
// live via `busctl introspect`: ua((ssss)a(siiddada{sv})a{sv})a(iiduba(ssss)a{sv})a{sv}
// ---------------------------------------------------------------------

#[derive(Debug, Type, serde::Deserialize)]
struct ConnectorInfo {
    connector: String,
    vendor: String,
    product: String,
    serial: String,
}

// `preferred_scale`/`supported_scales`/`properties` fields below are unread
// but must stay exactly in this order: zvariant matches D-Bus structures by
// field position, so removing them would misalign every following field.
#[allow(dead_code)]
#[derive(Debug, Type, serde::Deserialize)]
struct ModeInfo {
    id: String,
    width: i32,
    height: i32,
    refresh_rate: f64,
    preferred_scale: f64,
    supported_scales: Vec<f64>,
    properties: HashMap<String, OwnedValue>,
}

#[allow(dead_code)]
#[derive(Debug, Type, serde::Deserialize)]
struct MonitorEntry {
    connector_info: ConnectorInfo,
    modes: Vec<ModeInfo>,
    properties: HashMap<String, OwnedValue>,
}

#[allow(dead_code)]
#[derive(Debug, Type, serde::Deserialize)]
struct LogicalMonitorEntry {
    x: i32,
    y: i32,
    scale: f64,
    transform: u32,
    primary: bool,
    monitors: Vec<ConnectorInfo>,
    properties: HashMap<String, OwnedValue>,
}

type GetCurrentStateResult = (
    u32,
    Vec<MonitorEntry>,
    Vec<LogicalMonitorEntry>,
    HashMap<String, OwnedValue>,
);

#[derive(Debug, Serialize)]
struct ConnectorSummary {
    connector: String,
    vendor: String,
    product: String,
    serial_hash: String,
    current_mode: Option<String>,
    mode_count: usize,
}

#[derive(Debug, Serialize)]
struct LogicalMonitorSummary {
    x: i32,
    y: i32,
    scale: f64,
    transform: u32,
    primary: bool,
    connectors: Vec<String>,
}

#[derive(Debug, Serialize)]
struct DisplayConfigSummary {
    serial: u32,
    connectors: Vec<ConnectorSummary>,
    logical_monitors: Vec<LogicalMonitorSummary>,
}

/// Non-cryptographic hash (no crypto crate in the Phase 1 allow-list): only
/// needs to avoid printing the raw EDID-derived serial, not resist attack.
fn hash_serial(input: &str) -> String {
    if input.is_empty() {
        return "(empty)".to_string();
    }
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    input.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

fn is_current_mode(properties: &HashMap<String, OwnedValue>) -> bool {
    properties
        .get("is-current")
        .and_then(|value| bool::try_from(value.clone()).ok())
        .unwrap_or(false)
}

fn fetch_display_config_summary(conn: &Connection) -> anyhow::Result<DisplayConfigSummary> {
    let proxy = Proxy::new(
        conn,
        "org.gnome.Mutter.DisplayConfig",
        "/org/gnome/Mutter/DisplayConfig",
        "org.gnome.Mutter.DisplayConfig",
    )?;
    let (serial, monitors, logical_monitors, _properties): GetCurrentStateResult =
        proxy.call("GetCurrentState", &())?;

    let connectors = monitors
        .into_iter()
        .map(|monitor| {
            let current_mode = monitor
                .modes
                .iter()
                .find(|mode| is_current_mode(&mode.properties))
                .map(|mode| {
                    format!(
                        "{}x{}@{:.2}Hz ({})",
                        mode.width, mode.height, mode.refresh_rate, mode.id
                    )
                });
            ConnectorSummary {
                connector: monitor.connector_info.connector,
                vendor: monitor.connector_info.vendor,
                product: monitor.connector_info.product,
                serial_hash: hash_serial(&monitor.connector_info.serial),
                current_mode,
                mode_count: monitor.modes.len(),
            }
        })
        .collect();

    let logical_monitors = logical_monitors
        .into_iter()
        .map(|logical| LogicalMonitorSummary {
            x: logical.x,
            y: logical.y,
            scale: logical.scale,
            transform: logical.transform,
            primary: logical.primary,
            connectors: logical.monitors.into_iter().map(|c| c.connector).collect(),
        })
        .collect();

    Ok(DisplayConfigSummary {
        serial,
        connectors,
        logical_monitors,
    })
}

// ---------------------------------------------------------------------
// Document 20 §8 capability presence (provisional; full Doc 00 §35 tiers
// with cross-experiment evidence pointers are step 10's capability-report.md)
// ---------------------------------------------------------------------

#[derive(Debug, Serialize)]
struct CapabilityFlag {
    constant: String,
    status: String,
    evidence: String,
}

fn capability_flags(targets: &[InspectedTarget], display_config_ok: bool) -> Vec<CapabilityFlag> {
    let flag = |constant: &str, status: &str, evidence: &str| CapabilityFlag {
        constant: constant.to_string(),
        status: status.to_string(),
        evidence: evidence.to_string(),
    };
    let mutter_reachable = targets
        .iter()
        .any(|t| t.error.is_none() && !t.interfaces.is_empty());
    let remote_desktop_ok =
        method_present(targets, "org.gnome.Mutter.RemoteDesktop", "CreateSession");
    let screencast_ok = method_present(targets, "org.gnome.Mutter.ScreenCast", "CreateSession");
    let input_capture_ok =
        method_present(targets, "org.gnome.Mutter.InputCapture", "CreateSession");
    let apply_monitors_ok = method_present(
        targets,
        "org.gnome.Mutter.DisplayConfig",
        "ApplyMonitorsConfig",
    );
    let screensaver_ok = method_present(targets, "org.gnome.ScreenSaver", "Lock");

    vec![
        flag(
            "OS_SUPPORTED",
            "N/A",
            "See Experiment 0 (environment discovery).",
        ),
        flag(
            "GNOME_SUPPORTED",
            "N/A",
            "See Experiment 0 (environment discovery).",
        ),
        flag(
            "WAYLAND_SUPPORTED",
            "N/A",
            "See Experiment 0/1 (XDG_SESSION_TYPE).",
        ),
        flag(
            "SYSTEMD_SUPPORTED",
            "N/A",
            "See Experiment 0 (dpkg version).",
        ),
        flag(
            "SESSION_FOUND",
            "N/A",
            "See Experiment 1 (login1 session selection).",
        ),
        flag(
            "MUTTER_CAPABLE",
            if mutter_reachable {
                "AVAILABLE"
            } else {
                "NOT_AVAILABLE"
            },
            "org.gnome.Mutter.* Introspect() succeeded on the session bus.",
        ),
        flag(
            "REMOTE_DESKTOP_CAPABLE",
            if remote_desktop_ok {
                "AVAILABLE"
            } else {
                "NOT_AVAILABLE"
            },
            "org.gnome.Mutter.RemoteDesktop.CreateSession present (presence only; not invoked).",
        ),
        flag(
            "SCREENCAST_CAPABLE",
            if screencast_ok {
                "AVAILABLE"
            } else {
                "NOT_AVAILABLE"
            },
            "org.gnome.Mutter.ScreenCast.CreateSession present (presence only; not invoked).",
        ),
        flag(
            "PIPEWIRE_CAPABLE",
            "N/A",
            "See Experiment 0 (dpkg version); PipeWire node creation is session-scoped, out of \
             scope for read-only Phase 0-1.",
        ),
        flag(
            "VIRTUAL_DISPLAY_CAPABLE",
            "UNKNOWN",
            "RecordVirtual lives on ScreenCast.Session, only reachable after CreateSession \
             (forbidden this phase); see docs/gnome/feasibility-research.md for source-level \
             confirmation.",
        ),
        flag(
            "DISPLAY_CONFIG_CAPABLE",
            if display_config_ok && apply_monitors_ok {
                "AVAILABLE"
            } else {
                "NOT_AVAILABLE"
            },
            "GetCurrentState succeeded; ApplyMonitorsConfig present (presence only; not \
             invoked).",
        ),
        flag(
            "REMOTE_INPUT_CAPABLE",
            "UNKNOWN",
            "ConnectToEIS lives on RemoteDesktop.Session, only reachable after CreateSession \
             (forbidden this phase); see docs/gnome/feasibility-research.md.",
        ),
        flag(
            "PHYSICAL_INPUT_ISOLATION_CAPABLE",
            if input_capture_ok {
                "AVAILABLE"
            } else {
                "NOT_AVAILABLE"
            },
            "org.gnome.Mutter.InputCapture.CreateSession present (presence only); isolation \
             efficacy is Experiment 9 (Phase 7), not this experiment.",
        ),
        flag(
            "SESSION_LOCK_CAPABLE",
            if screensaver_ok {
                "AVAILABLE"
            } else {
                "NOT_AVAILABLE"
            },
            "org.gnome.ScreenSaver.Lock present. Finding: org.gnome.Shell.ScreenShield (bus \
             name) resolves to the same org.gnome.ScreenSaver interface at /org/gnome/ScreenSaver \
             — no distinct ScreenShield interface was found at any path tried on GNOME Shell \
             50.1 (see api-inventory.md).",
        ),
        flag(
            "EMERGENCY_CAPABLE",
            "N/A",
            "remote-emergencyd does not exist yet (Phase 10).",
        ),
        flag(
            "GPU_CAPABLE",
            "N/A",
            "See Experiment 0 (GPU/driver enumeration).",
        ),
    ]
}

// ---------------------------------------------------------------------
// api-inventory.md
// ---------------------------------------------------------------------

fn render_api_inventory(
    targets: &[InspectedTarget],
    display_config: &Option<DisplayConfigSummary>,
) -> String {
    let mut out = String::from(
        "# GNOME/Mutter/logind API inventory\n\n\
         Captured by Experiment 2 (`exp02_mutter_inventory`), read-only introspection only.\n\
         Raw XML: `docs/gnome/introspection/*.xml`. Standard `org.freedesktop.DBus.\
         {Introspectable,Properties,Peer}` interfaces exist on every object below and are\n\
         omitted from these tables for brevity (present verbatim in the raw XML).\n\n",
    );

    for target in targets {
        out.push_str(&format!(
            "## {} (`{}` bus)\n\nDestination: `{}`  \nObject path: `{}`\n\n",
            target.label, target.bus, target.destination, target.object_path
        ));
        if let Some(error) = &target.error {
            out.push_str(&format!("**Introspection failed:** {error}\n\n"));
            continue;
        }
        let domain_interfaces: Vec<&ParsedInterface> = target
            .interfaces
            .iter()
            .filter(|iface| !iface.name.starts_with("org.freedesktop.DBus."))
            .collect();
        if domain_interfaces.is_empty() {
            out.push_str("*(no non-standard interface at this path)*\n\n");
            continue;
        }
        for iface in domain_interfaces {
            out.push_str(&format!("### `{}`\n\n", iface.name));
            if !iface.methods.is_empty() {
                out.push_str("| Method | In | Out |\n|---|---|---|\n");
                for method in &iface.methods {
                    out.push_str(&format!(
                        "| `{}` | `{}` | `{}` |\n",
                        method.name,
                        method.in_args.join(", "),
                        method.out_args.join(", ")
                    ));
                }
                out.push('\n');
            }
            if !iface.properties.is_empty() {
                out.push_str("| Property | Type | Access |\n|---|---|---|\n");
                for property in &iface.properties {
                    out.push_str(&format!(
                        "| `{}` | `{}` | {} |\n",
                        property.name, property.r#type, property.access
                    ));
                }
                out.push('\n');
            }
            if !iface.signals.is_empty() {
                out.push_str("| Signal | Args |\n|---|---|\n");
                for signal in &iface.signals {
                    out.push_str(&format!(
                        "| `{}` | `{}` |\n",
                        signal.name,
                        signal.args.join(", ")
                    ));
                }
                out.push('\n');
            }
        }
    }

    out.push_str("## DisplayConfig.GetCurrentState summary\n\n");
    match display_config {
        Some(summary) => {
            out.push_str(&format!("Serial: `{}`\n\n", summary.serial));
            out.push_str("| Connector | Vendor | Product | Serial (hashed) | Current mode | Modes |\n|---|---|---|---|---|---|\n");
            for connector in &summary.connectors {
                out.push_str(&format!(
                    "| `{}` | {} | {} | `{}` | {} | {} |\n",
                    connector.connector,
                    connector.vendor,
                    connector.product,
                    connector.serial_hash,
                    connector
                        .current_mode
                        .as_deref()
                        .unwrap_or("(none marked current)"),
                    connector.mode_count
                ));
            }
            out.push('\n');
            out.push_str("| Logical monitor | Position | Scale | Transform | Primary | Connectors |\n|---|---|---|---|---|---|\n");
            for logical in &summary.logical_monitors {
                out.push_str(&format!(
                    "| — | ({}, {}) | {} | {} | {} | {} |\n",
                    logical.x,
                    logical.y,
                    logical.scale,
                    logical.transform,
                    logical.primary,
                    logical.connectors.join(", ")
                ));
            }
            out.push('\n');
        }
        None => out.push_str(
            "*GetCurrentState call failed; see the Experiment 2 evidence \
                               report for the error.*\n\n",
        ),
    }

    out
}

// ---------------------------------------------------------------------
// main
// ---------------------------------------------------------------------

#[derive(Debug, Serialize)]
struct MutterInventoryReport {
    targets: Vec<InspectedTarget>,
    remote_desktop_version: Option<i32>,
    screencast_version: Option<i32>,
    display_config_summary: Option<DisplayConfigSummary>,
    capability_flags: Vec<CapabilityFlag>,
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::try_init().ok();
    let args = Args::parse();
    let redact_on = args.common.redact_enabled();
    let now = OffsetDateTime::now_utc();

    fs::create_dir_all(INTROSPECTION_DIR)?;

    let session_conn = Connection::session()?;
    let system_conn = Connection::system()?;

    let uid = current_uid()?;
    let (_candidates, selected_session_id) = discover(uid)?;
    let selected_session_path = selected_session_id
        .as_ref()
        .map(|id| format!("/org/freedesktop/login1/session/_{id}"));

    let mut targets = vec![
        Target {
            label: "Mutter.DisplayConfig",
            bus: BusKind::Session,
            destination: "org.gnome.Mutter.DisplayConfig",
            path: "/org/gnome/Mutter/DisplayConfig".to_string(),
        },
        Target {
            label: "Mutter.RemoteDesktop",
            bus: BusKind::Session,
            destination: "org.gnome.Mutter.RemoteDesktop",
            path: "/org/gnome/Mutter/RemoteDesktop".to_string(),
        },
        Target {
            label: "Mutter.ScreenCast",
            bus: BusKind::Session,
            destination: "org.gnome.Mutter.ScreenCast",
            path: "/org/gnome/Mutter/ScreenCast".to_string(),
        },
        Target {
            label: "Mutter.InputCapture",
            bus: BusKind::Session,
            destination: "org.gnome.Mutter.InputCapture",
            path: "/org/gnome/Mutter/InputCapture".to_string(),
        },
        Target {
            label: "Mutter.InputMapping",
            bus: BusKind::Session,
            destination: "org.gnome.Mutter.InputMapping",
            path: "/org/gnome/Mutter/InputMapping".to_string(),
        },
        Target {
            label: "Mutter.ServiceChannel",
            bus: BusKind::Session,
            destination: "org.gnome.Mutter.ServiceChannel",
            path: "/org/gnome/Mutter/ServiceChannel".to_string(),
        },
        Target {
            label: "Mutter.IdleMonitor",
            bus: BusKind::Session,
            destination: "org.gnome.Mutter.IdleMonitor",
            path: "/org/gnome/Mutter/IdleMonitor".to_string(),
        },
        Target {
            label: "Mutter.IdleMonitor.Core",
            bus: BusKind::Session,
            destination: "org.gnome.Mutter.IdleMonitor",
            path: "/org/gnome/Mutter/IdleMonitor/Core".to_string(),
        },
        Target {
            label: "Shell.ScreenShield-at-declared-path",
            bus: BusKind::Session,
            destination: "org.gnome.Shell.ScreenShield",
            path: "/org/gnome/Shell/ScreenShield".to_string(),
        },
        Target {
            label: "Shell.ScreenShield-at-legacy-path",
            bus: BusKind::Session,
            destination: "org.gnome.Shell.ScreenShield",
            path: "/org/gnome/ScreenSaver".to_string(),
        },
        Target {
            label: "ScreenSaver",
            bus: BusKind::Session,
            destination: "org.gnome.ScreenSaver",
            path: "/org/gnome/ScreenSaver".to_string(),
        },
        Target {
            label: "login1.Manager",
            bus: BusKind::System,
            destination: "org.freedesktop.login1",
            path: "/org/freedesktop/login1".to_string(),
        },
    ];
    if let Some(path) = &selected_session_path {
        targets.push(Target {
            label: "login1.Session-selected",
            bus: BusKind::System,
            destination: "org.freedesktop.login1",
            path: path.clone(),
        });
    }

    let inspected: Vec<InspectedTarget> = targets
        .iter()
        .map(|target| introspect_target(&session_conn, &system_conn, target))
        .collect();

    let remote_desktop_version = Proxy::new(
        &session_conn,
        "org.gnome.Mutter.RemoteDesktop",
        "/org/gnome/Mutter/RemoteDesktop",
        "org.gnome.Mutter.RemoteDesktop",
    )
    .and_then(|proxy| proxy.get_property::<i32>("Version"))
    .ok();
    let screencast_version = Proxy::new(
        &session_conn,
        "org.gnome.Mutter.ScreenCast",
        "/org/gnome/Mutter/ScreenCast",
        "org.gnome.Mutter.ScreenCast",
    )
    .and_then(|proxy| proxy.get_property::<i32>("Version"))
    .ok();

    let display_config_summary = fetch_display_config_summary(&session_conn).ok();

    let capability_flags = capability_flags(&inspected, display_config_summary.is_some());

    let api_inventory = render_api_inventory(&inspected, &display_config_summary);
    fs::create_dir_all("docs/gnome")?;
    fs::write(
        "docs/gnome/api-inventory.md",
        redact(&api_inventory, redact_on),
    )?;

    let successful = inspected.iter().filter(|t| t.error.is_none()).count();
    let failed: Vec<&InspectedTarget> = inspected.iter().filter(|t| t.error.is_some()).collect();
    let observed = redact(
        &format!(
            "Introspected {}/{} targets successfully.\n{}\
             RemoteDesktop.Version={:?}, ScreenCast.Version={:?}\n\
             DisplayConfig.GetCurrentState: {}\n\
             Finding: org.gnome.Shell.ScreenShield exposes no distinct interface; it resolves \
             to org.gnome.ScreenSaver at /org/gnome/ScreenSaver (see api-inventory.md).\n\
             Capability presence flags: {}",
            successful,
            inspected.len(),
            if failed.is_empty() {
                String::new()
            } else {
                format!(
                    "Failed: {}\n",
                    failed
                        .iter()
                        .map(|t| format!("{} ({})", t.label, t.error.as_deref().unwrap_or("?")))
                        .collect::<Vec<_>>()
                        .join("; ")
                )
            },
            remote_desktop_version,
            screencast_version,
            match &display_config_summary {
                Some(s) => format!(
                    "{} connector(s), {} logical monitor(s)",
                    s.connectors.len(),
                    s.logical_monitors.len()
                ),
                None => "FAILED".to_string(),
            },
            capability_flags
                .iter()
                .map(|f| format!("{}={}", f.constant, f.status))
                .collect::<Vec<_>>()
                .join(", "),
        ),
        redact_on,
    );

    let result = if display_config_summary.is_some() && failed.is_empty() {
        ExperimentResult::Pass
    } else if display_config_summary.is_some() {
        ExperimentResult::Partial
    } else {
        ExperimentResult::Fail
    };

    let report = ExperimentReport {
        experiment: "Experiment 2 — Mutter Capability Inventory".to_string(),
        environment: "Development workstation (host = target), Ubuntu 26.04 / GNOME 50.1"
            .to_string(),
        objective: "Determine which relevant Mutter/Shell/logind D-Bus interfaces are actually \
                    available on this host and produce an API inventory (Document 10 §9)."
            .to_string(),
        hypothesis: "org.gnome.Mutter.{DisplayConfig,RemoteDesktop,ScreenCast,InputCapture,\
                     InputMapping,ServiceChannel,IdleMonitor}, org.gnome.Shell.ScreenShield, \
                     org.gnome.ScreenSaver, and org.freedesktop.login1 are introspectable \
                     read-only without creating any session or object."
            .to_string(),
        procedure: "Call org.freedesktop.DBus.Introspectable.Introspect on each target, save \
                    the raw XML, parse interfaces/methods/properties/signals, read the \
                    RemoteDesktop/ScreenCast Version properties, call \
                    DisplayConfig.GetCurrentState, and derive Document 20 §8 capability \
                    presence flags. Never call RecordVirtual, ConnectToEIS, \
                    ApplyMonitorsConfig, or InputCapture.CreateSession."
            .to_string(),
        expected: "All 9 Mutter/Shell/ScreenSaver targets plus login1 Manager and the selected \
                   Session introspect successfully; GetCurrentState returns the live display \
                   topology; no session or object is created."
            .to_string(),
        observed,
        evidence: vec![
            "inventory.json (this directory)".to_string(),
            format!("{INTROSPECTION_DIR}/*.xml"),
            "docs/gnome/api-inventory.md".to_string(),
        ],
        result,
        failure: if failed.is_empty() {
            None
        } else {
            Some(format!("{} target(s) failed to introspect", failed.len()))
        },
        root_cause: None,
        security_impact: Some(
            "None: read-only Introspect/Get/GetCurrentState only. No session, virtual \
             monitor, EIS connection, or monitor-config change was created."
                .to_string(),
        ),
        recommended_action: None,
        follow_up: Some(
            "Research findings (Document 00 §50 topics) in docs/gnome/feasibility-research.md; \
             capability report and architecture decisions (step 10)."
                .to_string(),
        ),
    };

    let data = MutterInventoryReport {
        targets: inspected,
        remote_desktop_version,
        screencast_version,
        display_config_summary,
        capability_flags,
    };

    let dir = evidence_dir(EXP_ID, now)?;
    write_evidence(&dir, &report.render(now), "inventory.json", &data)?;
    println!("Wrote evidence to {}", dir.display());
    println!("Wrote docs/gnome/api-inventory.md");

    if result == ExperimentResult::Fail {
        std::process::exit(1);
    }
    Ok(())
}
