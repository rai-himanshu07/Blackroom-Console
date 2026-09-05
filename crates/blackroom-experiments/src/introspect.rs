//! Live D-Bus introspection (Document 10 §9 technique, promoted from
//! `exp02_mutter_inventory` for reuse): a minimal GDBus one-tag-per-line XML
//! scanner (not a general XML parser — avoids adding an XML crate) plus a
//! helper to `Introspect()` a target object and parse its result. Mutter
//! session sub-objects (`RemoteDesktop.Session`, `ScreenCast.Session`) do not
//! exist until created, so their real methods must be discovered live
//! (`feasibility-research.md` topic 2) rather than assumed — every Phase 4+
//! experiment that calls a session sub-object introspects it first with
//! [`introspect_target`].

use std::fs;
use std::path::Path;

use serde::Serialize;
use zbus::blocking::{Connection, Proxy};

#[derive(Debug, Default, Serialize)]
pub struct ParsedMethod {
    pub name: String,
    pub in_args: Vec<String>,
    pub out_args: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct ParsedProperty {
    pub name: String,
    pub r#type: String,
    pub access: String,
}

#[derive(Debug, Default, Serialize)]
pub struct ParsedSignal {
    pub name: String,
    pub args: Vec<String>,
}

#[derive(Debug, Default, Serialize)]
pub struct ParsedInterface {
    pub name: String,
    pub methods: Vec<ParsedMethod>,
    pub properties: Vec<ParsedProperty>,
    pub signals: Vec<ParsedSignal>,
}

fn xml_attr(tag: &str, key: &str) -> Option<String> {
    let needle = format!("{key}=\"");
    let start = tag.find(&needle)? + needle.len();
    let rest = &tag[start..];
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

pub fn parse_introspection_xml(xml: &str) -> Vec<ParsedInterface> {
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

pub struct Target {
    pub label: &'static str,
    pub bus: BusKind,
    pub destination: &'static str,
    pub path: String,
}

#[derive(Clone, Copy)]
pub enum BusKind {
    Session,
    System,
}

#[derive(Debug, Serialize)]
pub struct InspectedTarget {
    pub label: String,
    pub bus: String,
    pub destination: String,
    pub object_path: String,
    pub xml_file: Option<String>,
    pub interfaces: Vec<ParsedInterface>,
    pub error: Option<String>,
}

pub fn slug(label: &str) -> String {
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

/// `Introspect()`s `target` and parses the result, saving the raw XML into
/// `xml_dir` (created if needed). `xml_dir` is a parameter (not a shared
/// constant) so each experiment controls where its evidence lands.
pub fn introspect_target(
    session_conn: &Connection,
    system_conn: &Connection,
    target: &Target,
    xml_dir: &str,
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
            let xml_file = fs::create_dir_all(xml_dir).ok().and_then(|()| {
                let file_name = format!("{}.xml", slug(target.label));
                let xml_path = Path::new(xml_dir).join(&file_name);
                fs::write(&xml_path, &xml)
                    .ok()
                    .map(|()| xml_path.display().to_string())
            });
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
pub fn method_present(
    targets: &[InspectedTarget],
    interface_name: &str,
    method_name: &str,
) -> bool {
    targets.iter().any(|target| {
        target.interfaces.iter().any(|iface| {
            iface.name == interface_name && iface.methods.iter().any(|m| m.name == method_name)
        })
    })
}
