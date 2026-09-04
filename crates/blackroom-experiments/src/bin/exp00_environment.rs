//! Experiment 0 — Environment Discovery (Document 10 §7).
//!
//! Read-only: reports OS/GNOME/Mutter/PipeWire/GPU/session/input facts.
//! Never modifies the system (no writes outside its own evidence directory).

use std::collections::BTreeMap;
use std::fs;
use std::process::Command;

use blackroom_experiments::{
    CommonArgs, ExperimentReport, ExperimentResult, evidence_dir, redact, write_evidence,
};
use clap::Parser;
use serde::Serialize;
use time::OffsetDateTime;

const EXP_ID: &str = "exp00";

#[derive(Parser, Debug)]
#[command(about = "Experiment 0: read-only environment discovery")]
struct Args {
    #[command(flatten)]
    common: CommonArgs,
}

#[derive(Debug, Serialize)]
struct PackageVersion {
    package: String,
    version: Option<String>,
}

#[derive(Debug, Serialize)]
struct GpuDevice {
    drm_card: String,
    driver: Option<String>,
    pci_id: Option<String>,
    pci_slot: Option<String>,
    lspci_description: Option<String>,
}

#[derive(Debug, Serialize)]
struct ConnectedOutput {
    connector: String,
    status: String,
}

#[derive(Debug, Serialize)]
struct InputDevice {
    name: Option<String>,
    bus: Option<String>,
}

#[derive(Debug, Serialize)]
struct EnvironmentReport {
    os_release: BTreeMap<String, String>,
    kernel: Option<String>,
    gnome_shell_version: Option<String>,
    packages: Vec<PackageVersion>,
    gpus: Vec<GpuDevice>,
    loaded_gpu_modules: Vec<String>,
    xdg_session_type: Option<String>,
    gnome_remote_desktop_unit_state: String,
    connected_outputs: Vec<ConnectedOutput>,
    input_devices: Vec<InputDevice>,
}

/// Run `cmd`, returning trimmed stdout only when the process exits successfully.
fn command_output(cmd: &str, args: &[&str]) -> Option<String> {
    let output = Command::new(cmd).args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if text.is_empty() { None } else { Some(text) }
}

/// Like [`command_output`], but keeps stdout regardless of exit status
/// (`systemctl is-active` exits non-zero for `inactive`/`failed`, yet its
/// stdout is still the answer we want).
fn command_output_any_status(cmd: &str, args: &[&str]) -> Option<String> {
    let output = Command::new(cmd).args(args).output().ok()?;
    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if text.is_empty() { None } else { Some(text) }
}

fn read_os_release() -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    if let Ok(text) = fs::read_to_string("/etc/os-release") {
        for line in text.lines() {
            if let Some((key, value)) = line.split_once('=') {
                map.insert(key.to_string(), value.trim().trim_matches('"').to_string());
            }
        }
    }
    map
}

fn dpkg_version(package: &str) -> Option<String> {
    command_output("dpkg-query", &["-W", "--showformat=${Version}", package])
}

/// Debian/Ubuntu ships Mutter's shared library as versioned
/// `libmutter-<abi>-0`, never a plain `mutter` package; fall back to the
/// installed ABI when the plain name does not exist.
fn mutter_version() -> (String, Option<String>) {
    if let Some(version) = dpkg_version("mutter") {
        return ("mutter".to_string(), Some(version));
    }
    let listing = command_output(
        "dpkg-query",
        &[
            "-W",
            "--showformat=${Package} ${Version}\n",
            "libmutter-*-0",
        ],
    );
    if let Some(listing) = listing {
        for line in listing.lines() {
            if let Some((package, version)) = line.split_once(' ')
                && !version.is_empty()
            {
                return (package.to_string(), Some(version.to_string()));
            }
        }
    }
    ("libmutter-*-0".to_string(), None)
}

fn packages() -> Vec<PackageVersion> {
    let (mutter_pkg, mutter_ver) = mutter_version();
    let mut list = vec![PackageVersion {
        package: mutter_pkg,
        version: mutter_ver,
    }];
    for package in [
        "gnome-shell",
        "gnome-remote-desktop",
        "pipewire",
        "wireplumber",
        "libei1",
        "libeis1",
        "xdg-desktop-portal-gnome",
        "systemd",
    ] {
        list.push(PackageVersion {
            package: package.to_string(),
            version: dpkg_version(package),
        });
    }
    list
}

fn lspci_vga_lines() -> Vec<String> {
    command_output("lspci", &["-nn"])
        .map(|text| {
            text.lines()
                .filter(|line| {
                    let lower = line.to_lowercase();
                    lower.contains("vga") || lower.contains("3d") || lower.contains("display")
                })
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn gpus() -> Vec<GpuDevice> {
    let lspci_lines = lspci_vga_lines();
    let Ok(entries) = fs::read_dir("/sys/class/drm") else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| name.starts_with("card") && !name[4..].contains('-'))
        .collect();
    names.sort();

    names
        .into_iter()
        .map(|card| {
            let driver = fs::read_link(format!("/sys/class/drm/{card}/device/driver"))
                .ok()
                .and_then(|path| path.file_name().map(|s| s.to_string_lossy().to_string()));
            let uevent = fs::read_to_string(format!("/sys/class/drm/{card}/device/uevent"))
                .unwrap_or_default();
            let mut pci_id = None;
            let mut pci_slot = None;
            for line in uevent.lines() {
                if let Some(value) = line.strip_prefix("PCI_ID=") {
                    pci_id = Some(value.to_string());
                }
                if let Some(value) = line.strip_prefix("PCI_SLOT_NAME=") {
                    pci_slot = Some(value.to_string());
                }
            }
            // lspci prints the bus address without the leading "0000:" domain.
            let lspci_description = pci_slot.as_ref().and_then(|slot| {
                let short_slot = slot.trim_start_matches("0000:");
                lspci_lines
                    .iter()
                    .find(|line| line.starts_with(short_slot))
                    .cloned()
            });
            GpuDevice {
                drm_card: card,
                driver,
                pci_id,
                pci_slot,
                lspci_description,
            }
        })
        .collect()
}

fn loaded_gpu_modules() -> Vec<String> {
    let known = ["nvidia", "nouveau", "i915", "amdgpu", "xe", "radeon"];
    let mut modules: Vec<String> = fs::read_to_string("/proc/modules")
        .unwrap_or_default()
        .lines()
        .filter_map(|line| line.split_whitespace().next())
        .filter(|module| known.iter().any(|prefix| module.starts_with(prefix)))
        .map(str::to_string)
        .collect();
    modules.sort();
    modules.dedup();
    modules
}

fn connected_outputs() -> Vec<ConnectedOutput> {
    let Ok(entries) = fs::read_dir("/sys/class/drm") else {
        return Vec::new();
    };
    let mut outputs: Vec<ConnectedOutput> = entries
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| name.starts_with("card") && name[4..].contains('-'))
        .map(|connector| {
            let status = fs::read_to_string(format!("/sys/class/drm/{connector}/status"))
                .map(|s| s.trim().to_string())
                .unwrap_or_else(|_| "unknown".to_string());
            ConnectedOutput { connector, status }
        })
        .collect();
    outputs.sort_by(|a, b| a.connector.cmp(&b.connector));
    outputs
}

/// Parse `/proc/bus/input/devices`: only `Name` and `Bus` are kept (no
/// serials — Document 10 §46, Document 13 §31 minimal collection).
fn input_devices() -> Vec<InputDevice> {
    let text = fs::read_to_string("/proc/bus/input/devices").unwrap_or_default();
    let mut devices = Vec::new();
    let mut name = None;
    let mut bus = None;
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("I: ") {
            bus = rest
                .split_whitespace()
                .find_map(|field| field.strip_prefix("Bus=").map(str::to_string));
        } else if let Some(rest) = line.strip_prefix("N: Name=") {
            name = Some(rest.trim_matches('"').to_string());
        } else if line.trim().is_empty() && (name.is_some() || bus.is_some()) {
            devices.push(InputDevice {
                name: name.take(),
                bus: bus.take(),
            });
        }
    }
    if name.is_some() || bus.is_some() {
        devices.push(InputDevice { name, bus });
    }
    devices
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::try_init().ok();
    let args = Args::parse();
    let redact_on = args.common.redact_enabled();
    let now = OffsetDateTime::now_utc();

    let data = EnvironmentReport {
        os_release: read_os_release(),
        kernel: command_output("uname", &["-r"]),
        gnome_shell_version: command_output("gnome-shell", &["--version"]),
        packages: packages(),
        gpus: gpus(),
        loaded_gpu_modules: loaded_gpu_modules(),
        xdg_session_type: std::env::var("XDG_SESSION_TYPE").ok(),
        gnome_remote_desktop_unit_state: command_output_any_status(
            "systemctl",
            &["--user", "is-active", "gnome-remote-desktop.service"],
        )
        .unwrap_or_else(|| "unknown".to_string()),
        connected_outputs: connected_outputs(),
        input_devices: input_devices(),
    };

    let gpu_summary = data
        .gpus
        .iter()
        .map(|g| format!("{}({})", g.drm_card, g.driver.as_deref().unwrap_or("?")))
        .collect::<Vec<_>>()
        .join(", ");
    let output_summary = data
        .connected_outputs
        .iter()
        .map(|o| format!("{}={}", o.connector, o.status))
        .collect::<Vec<_>>()
        .join(", ");
    let observed = redact(
        &format!(
            "OS: {}\nKernel: {}\nGNOME Shell: {}\nMutter: {}\nGPUs: {gpu_summary}\n\
             Connected outputs: {output_summary}\nInput devices found: {}\n\
             XDG_SESSION_TYPE: {}\ngnome-remote-desktop unit: {}",
            data.os_release
                .get("PRETTY_NAME")
                .map(String::as_str)
                .unwrap_or("unknown"),
            data.kernel.as_deref().unwrap_or("unknown"),
            data.gnome_shell_version.as_deref().unwrap_or("unknown"),
            data.packages
                .first()
                .and_then(|p| p.version.as_deref())
                .unwrap_or("unknown"),
            data.input_devices.len(),
            data.xdg_session_type.as_deref().unwrap_or("unset"),
            data.gnome_remote_desktop_unit_state,
        ),
        redact_on,
    );

    let report = ExperimentReport {
        experiment: "Experiment 0 — Environment Discovery".to_string(),
        environment: "Development workstation (host = target), Ubuntu 26.04 / GNOME 50.1"
            .to_string(),
        objective: "Produce a reproducible, non-mutating environment report (Document 10 §7)."
            .to_string(),
        hypothesis: "OS/GNOME/Mutter/PipeWire/GPU/session facts can be collected entirely \
                     read-only via dpkg-query, /sys, /proc, and --version flags."
            .to_string(),
        procedure: "Run `exp00_environment`; read /etc/os-release, dpkg-query, \
                    /sys/class/drm, /proc/modules, /proc/bus/input/devices; query \
                    XDG_SESSION_TYPE and the gnome-remote-desktop user unit state."
            .to_string(),
        expected: "A complete, deterministic report; no system file outside the evidence \
                   directory changes."
            .to_string(),
        observed,
        evidence: vec!["environment.json (this directory)".to_string()],
        result: ExperimentResult::Pass,
        failure: None,
        root_cause: None,
        security_impact: Some(
            "None: read-only. No secrets, passwords, or device serials collected.".to_string(),
        ),
        recommended_action: None,
        follow_up: Some("Experiment 1 — GNOME Session Discovery.".to_string()),
    };

    let dir = evidence_dir(EXP_ID, now)?;
    write_evidence(&dir, &report.render(now), "environment.json", &data)?;
    println!("Wrote evidence to {}", dir.display());
    Ok(())
}
