//! Doc 20 §8 host capability detection using the Doc 00 §35 five-tier
//! vocabulary. Every call here is read-only (`Get`/`GetCurrentState`/
//! `dpkg-query`/filesystem reads) — never `CreateSession`, `RecordVirtual`,
//! `ApplyMonitorsConfig`, or `ConnectToEIS` (those stay Phase 4+). Ports the
//! evidence techniques proven in `exp00_environment`/`exp02_mutter_inventory`
//! (Phase 0-1) as real production code; classifications mirror
//! `docs/gnome/capability-report.md`.

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::process::Command;

use zbus::blocking::{Connection, Proxy};
use zbus::zvariant::{OwnedValue, Type};

use crate::backend::SessionInfo;

/// Doc 00 §35 five-tier compatibility vocabulary. `Unknown` never activates
/// (Doc 00 §35 explicit rule): a D-Bus method's mere presence is not proof
/// of support (Doc 13 §16).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CapabilityTier {
    Supported,
    SupportedWithLimitations,
    Experimental,
    Unsupported,
    Unknown,
}

impl CapabilityTier {
    pub const fn as_str(self) -> &'static str {
        match self {
            CapabilityTier::Supported => "SUPPORTED",
            CapabilityTier::SupportedWithLimitations => "SUPPORTED_WITH_LIMITATIONS",
            CapabilityTier::Experimental => "EXPERIMENTAL",
            CapabilityTier::Unsupported => "UNSUPPORTED",
            CapabilityTier::Unknown => "UNKNOWN",
        }
    }
}

impl std::fmt::Display for CapabilityTier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Doc 20 §8 capability constants.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CapabilityReport {
    pub os_supported: CapabilityTier,
    pub gnome_supported: CapabilityTier,
    pub wayland_supported: CapabilityTier,
    pub systemd_supported: CapabilityTier,
    pub session_found: CapabilityTier,
    pub mutter_capable: CapabilityTier,
    pub remote_desktop_capable: CapabilityTier,
    pub screencast_capable: CapabilityTier,
    pub pipewire_capable: CapabilityTier,
    pub virtual_display_capable: CapabilityTier,
    pub display_config_capable: CapabilityTier,
    pub remote_input_capable: CapabilityTier,
    pub physical_input_isolation_capable: CapabilityTier,
    pub session_lock_capable: CapabilityTier,
    pub emergency_capable: CapabilityTier,
    pub gpu_capable: CapabilityTier,
}

impl CapabilityReport {
    /// Roadmap Phase 3 gate: OS/GNOME/Wayland/systemd/session all `Supported`.
    pub fn phase3_gate_passed(&self) -> bool {
        [
            self.os_supported,
            self.gnome_supported,
            self.wayland_supported,
            self.systemd_supported,
            self.session_found,
        ]
        .into_iter()
        .all(|tier| matches!(tier, CapabilityTier::Supported))
    }
}

// ---------------------------------------------------------------------
// Pure tier-mapping functions (unit-tested below with injected evidence).
// ---------------------------------------------------------------------

fn os_supported_tier(os_release: &BTreeMap<String, String>) -> CapabilityTier {
    match (
        os_release.get("ID").map(String::as_str),
        os_release.get("VERSION_ID").map(String::as_str),
    ) {
        (Some("ubuntu"), Some(version)) if version.starts_with("26.") => CapabilityTier::Supported,
        (Some(_), Some(_)) => CapabilityTier::Unsupported,
        _ => CapabilityTier::Unknown,
    }
}

/// Strips a dpkg epoch prefix (`N:`) and any trailing revision, returning
/// just the leading numeric major-version component.
fn major_version(version: &str) -> Option<u32> {
    let without_epoch = version.rsplit_once(':').map_or(version, |(_, rest)| rest);
    without_epoch.split(['.', '-', '~']).next()?.parse().ok()
}

fn gnome_supported_tier(gnome_shell_version: Option<&str>) -> CapabilityTier {
    match gnome_shell_version.and_then(major_version) {
        Some(major) if major >= 50 => CapabilityTier::Supported,
        Some(_) => CapabilityTier::Unsupported,
        None => CapabilityTier::Unknown,
    }
}

fn wayland_supported_tier(is_wayland: bool) -> CapabilityTier {
    if is_wayland {
        CapabilityTier::Supported
    } else {
        CapabilityTier::Unsupported
    }
}

fn systemd_supported_tier(systemd_version: Option<&str>) -> CapabilityTier {
    if systemd_version.is_some() {
        CapabilityTier::Supported
    } else {
        CapabilityTier::Unknown
    }
}

fn gpu_capable_tier(loaded_gpu_modules: &[String]) -> CapabilityTier {
    if loaded_gpu_modules.is_empty() {
        CapabilityTier::Unknown
    } else {
        CapabilityTier::SupportedWithLimitations
    }
}

/// `Supported` ceiling reflects Phase 4 evidence (Experiments 3-4: real
/// PipeWire frames/nodes, not merely installed packages); `Unknown` if
/// either package is absent.
fn pipewire_capable_tier(pipewire_installed: bool, wireplumber_installed: bool) -> CapabilityTier {
    if pipewire_installed && wireplumber_installed {
        CapabilityTier::Supported
    } else {
        CapabilityTier::Unknown
    }
}

// ---------------------------------------------------------------------
// Real, non-D-Bus evidence gathering (ported from `exp00_environment`).
// ---------------------------------------------------------------------

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
    let output = Command::new("dpkg-query")
        .args(["-W", "--showformat=${Version}", package])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if text.is_empty() { None } else { Some(text) }
}

fn loaded_gpu_modules() -> Vec<String> {
    const KNOWN: [&str; 6] = ["nvidia", "nouveau", "i915", "amdgpu", "xe", "radeon"];
    fs::read_to_string("/proc/modules")
        .unwrap_or_default()
        .lines()
        .filter_map(|line| line.split_whitespace().next())
        .filter(|module| KNOWN.contains(module))
        .map(str::to_string)
        .collect()
}

// ---------------------------------------------------------------------
// DisplayConfig.GetCurrentState (read-only; Mutter 50.1 signature proven
// live via Experiment 2, `docs/gnome/api-inventory.md`):
// ua((ssss)a(siiddada{sv})a{sv})a(iiduba(ssss)a{sv})a{sv}
// ---------------------------------------------------------------------

#[allow(dead_code)]
#[derive(Debug, Type, serde::Deserialize)]
struct ConnectorInfo {
    connector: String,
    vendor: String,
    product: String,
    serial: String,
}

// Trailing fields are unread but must stay in this exact order: zvariant
// matches D-Bus structures by field position, not name.
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

// ---------------------------------------------------------------------
// Session-bus reachability checks (presence/version/one real call only).
// ---------------------------------------------------------------------

fn mutter_display_config_reachable(conn: &Connection) -> bool {
    Proxy::new(
        conn,
        "org.gnome.Mutter.DisplayConfig",
        "/org/gnome/Mutter/DisplayConfig",
        "org.gnome.Mutter.DisplayConfig",
    )
    .and_then(|proxy| proxy.get_property::<bool>("HasExternalMonitor"))
    .is_ok()
}

fn mutter_remote_desktop_version(conn: &Connection) -> Option<i32> {
    Proxy::new(
        conn,
        "org.gnome.Mutter.RemoteDesktop",
        "/org/gnome/Mutter/RemoteDesktop",
        "org.gnome.Mutter.RemoteDesktop",
    )
    .and_then(|proxy| proxy.get_property::<i32>("Version"))
    .ok()
}

fn mutter_screencast_version(conn: &Connection) -> Option<i32> {
    Proxy::new(
        conn,
        "org.gnome.Mutter.ScreenCast",
        "/org/gnome/Mutter/ScreenCast",
        "org.gnome.Mutter.ScreenCast",
    )
    .and_then(|proxy| proxy.get_property::<i32>("Version"))
    .ok()
}

fn mutter_input_capture_reachable(conn: &Connection) -> bool {
    Proxy::new(
        conn,
        "org.gnome.Mutter.InputCapture",
        "/org/gnome/Mutter/InputCapture",
        "org.gnome.Mutter.InputCapture",
    )
    .and_then(|proxy| proxy.get_property::<u32>("SupportedCapabilities"))
    .is_ok()
}

fn display_config_get_current_state_ok(conn: &Connection) -> bool {
    Proxy::new(
        conn,
        "org.gnome.Mutter.DisplayConfig",
        "/org/gnome/Mutter/DisplayConfig",
        "org.gnome.Mutter.DisplayConfig",
    )
    .and_then(|proxy| proxy.call::<_, _, GetCurrentStateResult>("GetCurrentState", &()))
    .is_ok()
}

/// `org.gnome.Shell.ScreenShield` has no distinct interface on this GNOME
/// version; it aliases to `org.gnome.ScreenSaver` (Phase 1 finding,
/// `docs/gnome/api-inventory.md`).
fn screensaver_reachable(conn: &Connection) -> bool {
    Proxy::new(
        conn,
        "org.gnome.ScreenSaver",
        "/org/gnome/ScreenSaver",
        "org.gnome.ScreenSaver",
    )
    .and_then(|proxy| proxy.call::<_, _, bool>("GetActive", &()))
    .is_ok()
}

/// `EMERGENCY_CAPABLE` is the one Doc 20 §8 constant that is still fixed at
/// `Unknown`: it is **not** computed from runtime evidence, matching
/// `docs/gnome/capability-report.md`'s own classification, until Phase 10
/// observes the daemon as a hardened, installed component (the offline
/// `remote-emergencyd` exists and was observed only under the operator's own
/// uid). (`VIRTUAL_DISPLAY_CAPABLE` left the group in Phase 4, see
/// [`VIRTUAL_DISPLAY_PROVEN_WITH_LIMITATIONS`]; `REMOTE_INPUT_CAPABLE` after
/// Experiment 8, see [`REMOTE_INPUT_PROVEN_WITH_LIMITATIONS`];
/// `PHYSICAL_INPUT_ISOLATION_CAPABLE` after the Phase 7 review, see
/// [`PHYSICAL_INPUT_ISOLATION_OBSERVED_EXPERIMENTAL`].)
const NOT_YET_DETERMINABLE: CapabilityTier = CapabilityTier::Unknown;

/// `PHYSICAL_INPUT_ISOLATION_CAPABLE` (Gate E): `Experimental` after the Phase 7
/// review of 2026-10-01 (`docs/security/input-isolation-decision.md`, evidence
/// `docs/experiments/evidence/exp09/`). An exclusive `EVIOCGRAB` on event2-7 hid
/// physical input from an observer page while an injected Shift arrived, and the
/// gateway/hostd/daemon path released on revoke, heartbeat loss, a frozen hostd
/// and the chord. Gate E stays UNPROVEN: privilege is unaccepted (same uid as
/// hostd, unit never run), a frozen holder keeps the grab, the product daemon had
/// no independent observer, hotplug and the held-key negative test were not run,
/// and remote pointer/click/scroll under a grab were not observed. Never read as
/// permission to activate remote mode.
const PHYSICAL_INPUT_ISOLATION_OBSERVED_EXPERIMENTAL: CapabilityTier = CapabilityTier::Experimental;

/// `VIRTUAL_DISPLAY_CAPABLE`: promoted from `Unknown` this phase (Phase 4,
/// Experiments 4–5, `docs/experiments/evidence/exp0{4,5}/`) —
/// `RecordVirtual` creates a real, `DisplayConfig`-confirmed monitor at
/// 1280×720/1920×1080/2560×1440@60Hz, usable as an *additional* active
/// display (Experiment 5), destroyed cleanly across 50 create/destroy
/// cycles with 0 leaked PipeWire nodes and no GNOME Shell crash (Doc 19
/// §16–17). **Not** a clean `Supported`: Mutter accepts **zero** physical
/// monitors enabled on the single built-in eDP layout only (Phase 5,
/// `docs/gnome/display-isolation.md`; connected HDMI unsupported), and GPU-specific
/// cross-buffer-scanout / cursor behaviour on this hybrid host remain
/// `UNVERIFIED` (`feasibility-research.md` topics 3, 9; escalated to Phase
/// 9/24). Structurally fixed (not computed inside `detect()`) for the same
/// reason as [`NOT_YET_DETERMINABLE`]: creating/destroying a real virtual
/// monitor on every `gnome-session-agent` startup would itself be the
/// repeated-cycle reliability risk Doc 19 §16–17 warns against.
const VIRTUAL_DISPLAY_PROVEN_WITH_LIMITATIONS: CapabilityTier =
    CapabilityTier::SupportedWithLimitations;

/// `REMOTE_INPUT_CAPABLE`: `SupportedWithLimitations` after Experiment 8 (runs
/// 1-4, `docs/experiments/evidence/exp08/`; run 4, 2026-10-01, passed every
/// stage). Observed on the single built-in display, in an input-only session,
/// with a browser observer page and a fake in-process authority: Shift, a
/// Shift+Right Ctrl chord, `a` and Left, a click, a scroll, a pointer path of
/// +40, -40, -10 from a captured start, and no delivery after a revoke or after
/// session stop. Limits: no virtual-monitor routing, no hostd-to-agent-to-EIS
/// authority, owner-loss teardown unobserved (Gate F), relative pointer only,
/// no cursor or absolute mapping (Exp 28), a browser page as the only target,
/// and the revoke refusal is the local authorization check, not Mutter.
/// Structurally fixed, not computed in `detect()`, because observing it
/// injects input into the live session.
const REMOTE_INPUT_PROVEN_WITH_LIMITATIONS: CapabilityTier =
    CapabilityTier::SupportedWithLimitations;

/// Detects all 16 Doc 20 §8 capability constants for the already-discovered
/// `session`. Strictly read-only throughout. 12 of the 16 are computed from
/// live evidence gathered this phase; the remaining 4 are structurally
/// fixed (see [`NOT_YET_DETERMINABLE`]).
pub fn detect(session: &SessionInfo) -> CapabilityReport {
    let os_release = read_os_release();
    let os_supported = os_supported_tier(&os_release);
    let gnome_supported = gnome_supported_tier(dpkg_version("gnome-shell").as_deref());
    let wayland_supported = wayland_supported_tier(session.is_wayland);
    let systemd_supported = systemd_supported_tier(dpkg_version("systemd").as_deref());
    let gpu_capable = gpu_capable_tier(&loaded_gpu_modules());
    // Presence check stays live/cheap every `detect()` call (Doc 19 §17:
    // safe, read-only); the tier ceiling for "present" was raised from
    // `Experimental` to `Supported` this phase because Phase 4 (Experiments
    // 3-4, `docs/experiments/evidence/exp0{3,4}/`) proved real PipeWire
    // frames/nodes, not merely that the packages are installed.
    let pipewire_capable = pipewire_capable_tier(
        dpkg_version("pipewire").is_some(),
        dpkg_version("wireplumber").is_some(),
    );

    let (
        mutter_capable,
        remote_desktop_capable,
        screencast_capable,
        display_config_capable,
        session_lock_capable,
    ) = match Connection::session() {
        Ok(conn) => {
            let display_config_ok = mutter_display_config_reachable(&conn);
            let remote_desktop_version = mutter_remote_desktop_version(&conn);
            let screencast_version = mutter_screencast_version(&conn);
            let input_capture_ok = mutter_input_capture_reachable(&conn);
            let mutter_capable = if display_config_ok
                && remote_desktop_version.is_some()
                && screencast_version.is_some()
                && input_capture_ok
            {
                CapabilityTier::Supported
            } else {
                CapabilityTier::Unsupported
            };
            // Presence stays a live check. Experiment 8 (2026-09-30) added the
            // missing evidence: input-only `CreateSession`/`Start`/
            // `ConnectToEIS`/`Stop` with verified teardown on the single
            // built-in display. Owner-loss teardown, touch, clipboard and
            // other layouts are unobserved, hence not a clean `Supported`.
            let remote_desktop_capable = if remote_desktop_version.is_some() {
                CapabilityTier::SupportedWithLimitations
            } else {
                CapabilityTier::Unsupported
            };
            // Promoted `Experimental`→`Supported` this phase: Phase 4
            // Experiments 3-5 proved real `CreateSession`/`RecordMonitor`/
            // `RecordVirtual`/`Start`/`Stop` calls with verified cleanup
            // (`RemoteDesktop` above is limited to the input-only session).
            let screencast_capable = if screencast_version.is_some() {
                CapabilityTier::Supported
            } else {
                CapabilityTier::Unsupported
            };
            // `GetCurrentState` works and `ApplyMonitorsConfig` zero-physical isolation was
            // restored reliably on the single built-in eDP layout only; four connected-HDMI
            // runs failed restoration (docs/gnome/display-isolation.md), hence not `Supported`.
            let display_config_capable = if display_config_get_current_state_ok(&conn) {
                CapabilityTier::SupportedWithLimitations
            } else {
                CapabilityTier::Unsupported
            };
            let session_lock_capable = if screensaver_reachable(&conn) {
                CapabilityTier::Experimental
            } else {
                CapabilityTier::Unsupported
            };
            (
                mutter_capable,
                remote_desktop_capable,
                screencast_capable,
                display_config_capable,
                session_lock_capable,
            )
        }
        Err(error) => {
            tracing::warn!(%error, "session bus unreachable; Mutter-derived capabilities are UNKNOWN");
            (
                CapabilityTier::Unknown,
                CapabilityTier::Unknown,
                CapabilityTier::Unknown,
                CapabilityTier::Unknown,
                CapabilityTier::Unknown,
            )
        }
    };

    CapabilityReport {
        os_supported,
        gnome_supported,
        wayland_supported,
        systemd_supported,
        // A valid `SessionInfo` means `session::discover_session` already
        // succeeded, so a session was, by construction, found.
        session_found: CapabilityTier::Supported,
        mutter_capable,
        remote_desktop_capable,
        screencast_capable,
        pipewire_capable,
        // Promoted from `Unknown` this phase (see
        // `VIRTUAL_DISPLAY_PROVEN_WITH_LIMITATIONS`): Experiments 4-5 created,
        // confirmed, and destroyed real virtual monitors.
        virtual_display_capable: VIRTUAL_DISPLAY_PROVEN_WITH_LIMITATIONS,
        display_config_capable,
        // Observed in Experiment 8 but not computed live (see the constant).
        remote_input_capable: REMOTE_INPUT_PROVEN_WITH_LIMITATIONS,
        // Not evidence-computed this phase: Gate E, the highest project
        // risk, stays UNKNOWN until Phase 7.
        physical_input_isolation_capable: PHYSICAL_INPUT_ISOLATION_OBSERVED_EXPERIMENTAL,
        session_lock_capable,
        // Not evidence-computed this phase: `remote-emergencyd` does not
        // exist yet (Phase 10).
        emergency_capable: NOT_YET_DETERMINABLE,
        gpu_capable,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn os_release(id: &str, version_id: &str) -> BTreeMap<String, String> {
        let mut map = BTreeMap::new();
        map.insert("ID".to_string(), id.to_string());
        map.insert("VERSION_ID".to_string(), version_id.to_string());
        map
    }

    #[test]
    fn os_supported_for_ubuntu_26() {
        assert_eq!(
            os_supported_tier(&os_release("ubuntu", "26.04")),
            CapabilityTier::Supported
        );
    }

    #[test]
    fn os_unsupported_for_other_distro_or_version() {
        assert_eq!(
            os_supported_tier(&os_release("fedora", "40")),
            CapabilityTier::Unsupported
        );
        assert_eq!(
            os_supported_tier(&os_release("ubuntu", "24.04")),
            CapabilityTier::Unsupported
        );
    }

    #[test]
    fn os_unknown_when_unreadable() {
        assert_eq!(os_supported_tier(&BTreeMap::new()), CapabilityTier::Unknown);
    }

    #[test]
    fn gnome_supported_for_major_50_and_above() {
        assert_eq!(
            gnome_supported_tier(Some("50.1-1ubuntu1")),
            CapabilityTier::Supported
        );
        assert_eq!(
            gnome_supported_tier(Some("1:52.0-1")),
            CapabilityTier::Supported
        );
    }

    #[test]
    fn gnome_unsupported_below_50() {
        assert_eq!(
            gnome_supported_tier(Some("45.0-1ubuntu1")),
            CapabilityTier::Unsupported
        );
    }

    #[test]
    fn gnome_unknown_when_absent_or_unparseable() {
        assert_eq!(gnome_supported_tier(None), CapabilityTier::Unknown);
        assert_eq!(
            gnome_supported_tier(Some("not-a-version")),
            CapabilityTier::Unknown
        );
    }

    #[test]
    fn wayland_tier_matches_session_flag() {
        assert_eq!(wayland_supported_tier(true), CapabilityTier::Supported);
        assert_eq!(wayland_supported_tier(false), CapabilityTier::Unsupported);
    }

    #[test]
    fn systemd_supported_when_version_present() {
        assert_eq!(
            systemd_supported_tier(Some("259.5-0ubuntu3.4")),
            CapabilityTier::Supported
        );
        assert_eq!(systemd_supported_tier(None), CapabilityTier::Unknown);
    }

    #[test]
    fn gpu_capable_with_limitations_when_any_driver_loaded() {
        assert_eq!(
            gpu_capable_tier(&["nvidia".to_string(), "i915".to_string()]),
            CapabilityTier::SupportedWithLimitations
        );
        assert_eq!(gpu_capable_tier(&[]), CapabilityTier::Unknown);
    }

    #[test]
    fn pipewire_capable_supported_when_both_packages_present() {
        assert_eq!(pipewire_capable_tier(true, true), CapabilityTier::Supported);
        assert_eq!(pipewire_capable_tier(true, false), CapabilityTier::Unknown);
        assert_eq!(pipewire_capable_tier(false, true), CapabilityTier::Unknown);
        assert_eq!(pipewire_capable_tier(false, false), CapabilityTier::Unknown);
    }

    #[test]
    fn virtual_display_promoted_to_supported_with_limitations_not_a_clean_supported() {
        // Phase 4 evidence (Experiments 4-5) justifies a real promotion from
        // `Unknown`; zero-physical isolation is proven only on the single eDP
        // layout (Phase 5) and GPU/cursor caveats (Phase 9/24) keep it below a
        // clean `Supported`.
        assert_eq!(
            VIRTUAL_DISPLAY_PROVEN_WITH_LIMITATIONS,
            CapabilityTier::SupportedWithLimitations
        );
    }

    fn all_supported_report() -> CapabilityReport {
        CapabilityReport {
            os_supported: CapabilityTier::Supported,
            gnome_supported: CapabilityTier::Supported,
            wayland_supported: CapabilityTier::Supported,
            systemd_supported: CapabilityTier::Supported,
            session_found: CapabilityTier::Supported,
            mutter_capable: CapabilityTier::Unknown,
            remote_desktop_capable: CapabilityTier::Unknown,
            screencast_capable: CapabilityTier::Unknown,
            pipewire_capable: CapabilityTier::Unknown,
            virtual_display_capable: CapabilityTier::Unknown,
            display_config_capable: CapabilityTier::Unknown,
            remote_input_capable: CapabilityTier::Unknown,
            physical_input_isolation_capable: CapabilityTier::Unknown,
            session_lock_capable: CapabilityTier::Unknown,
            emergency_capable: CapabilityTier::Unknown,
            gpu_capable: CapabilityTier::Unknown,
        }
    }

    #[test]
    fn phase3_gate_passes_when_the_five_required_constants_are_supported() {
        assert!(all_supported_report().phase3_gate_passed());
    }

    #[test]
    fn phase3_gate_fails_closed_when_any_required_constant_is_not_supported() {
        let mut report = all_supported_report();
        report.session_found = CapabilityTier::Unknown;
        assert!(!report.phase3_gate_passed());
    }
}
