//! Live-host checks for GNOME session discovery and the Phase 3 capability
//! gate. Gated `#[ignore]`; run explicitly via
//! `BLACKROOM_SYSTEST=1 cargo test -p blackroom-systest -- --ignored` on a
//! prepared host with a real Wayland/GNOME session (Doc 12 §52).

use blackroom_gnome::mutter::{capability, session};

/// Host preflight: refuses to run unless explicitly opted in and the
/// environment looks like a real Wayland session.
fn require_systest_host() {
    assert!(
        std::env::var_os("BLACKROOM_SYSTEST").is_some(),
        "set BLACKROOM_SYSTEST=1 to run live-host system tests"
    );
    assert_eq!(
        std::env::var("XDG_SESSION_TYPE").ok().as_deref(),
        Some("wayland"),
        "blackroom-systest requires a real Wayland session (XDG_SESSION_TYPE=wayland)"
    );
}

#[test]
#[ignore = "live-host only; run with BLACKROOM_SYSTEST=1 cargo test -p blackroom-systest -- --ignored"]
fn discover_session_selects_a_valid_wayland_user_session() {
    require_systest_host();
    let info =
        session::discover_session().expect("discover_session should succeed on a prepared host");
    assert!(!info.session_id.is_empty());
    assert!(info.is_wayland, "selected session must be Wayland");
    assert!(info.active, "selected session must be active");
    // On a host with more than one seat0 session (this development machine
    // has two, Phase 0-1 finding), reaching this assertion at all
    // demonstrates the unique-match disambiguation worked, rather than
    // silently picking "the first" candidate.
}

#[test]
#[ignore = "live-host only; run with BLACKROOM_SYSTEST=1 cargo test -p blackroom-systest -- --ignored"]
fn phase3_capability_gate_passes_on_a_supported_host() {
    require_systest_host();
    let info =
        session::discover_session().expect("discover_session should succeed on a prepared host");
    let report = capability::detect(&info);
    assert!(
        report.phase3_gate_passed(),
        "Phase 3 gate (OS/GNOME/Wayland/systemd/session all SUPPORTED) failed: {report:?}"
    );
}
