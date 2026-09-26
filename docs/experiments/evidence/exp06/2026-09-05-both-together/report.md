Experiment: Experiment 6 — Physical Output Isolation
Date: 2026-09-05T19:24:19.400825496Z
Environment: Development workstation (host = target), Ubuntu 26.04 / GNOME 50.1
Objective:
Disable every physical output while the virtual monitor stays active (Document 10 §13, Document 02 §12).

Hypothesis:
ApplyMonitorsConfig(method=Temporary) accepts a logical-monitors array containing only the virtual monitor's entry (zero physical monitors enabled); every physical connector then disappears from logical_monitors[] while remaining in the raw monitors[] inventory.

Procedure:
Snapshot + persist DisplayBackup; create and confirm a virtual monitor; check ApplyMonitorsConfigAllowed; arm the restore watchdog; apply a zero-physical logical-monitors config; verify; restore; verify restoration; tear down.

Expected:
Every physical connector is absent from logical_monitors[] while isolated, and the exact original topology is restored afterward.

Observed:
Findings {
    host: HostInfo {
        kernel: "7.0.0-31-generic",
        gpu_modules: [
            "xe",
            "nvidia",
            "i915",
        ],
    },
    physical_connectors_before: [
        "eDP-1",
        "HDMI-1",
    ],
    virtual_connector: Some(
        "Meta-0",
    ),
    apply_monitors_config_allowed: true,
    watchdog_armed: true,
    watchdog_unit: Some(
        "blackroom-exp06-watchdog-1788636259",
    ),
    cycles: [],
    stop_error: None,
    virtual_connector_fully_gone: true,
    paused_for_manual_kill_test: true,
}

Evidence:
- docs/experiments/evidence/exp06/<date>/report.md
- docs/experiments/evidence/exp06/<date>/backup.json

Result:
PARTIAL

Failure:
Findings {
    host: HostInfo {
        kernel: "7.0.0-31-generic",
        gpu_modules: [
            "xe",
            "nvidia",
            "i915",
        ],
    },
    physical_connectors_before: [
        "eDP-1",
        "HDMI-1",
    ],
    virtual_connector: Some(
        "Meta-0",
    ),
    apply_monitors_config_allowed: true,
    watchdog_armed: true,
    watchdog_unit: Some(
        "blackroom-exp06-watchdog-1788636259",
    ),
    cycles: [],
    stop_error: None,
    virtual_connector_fully_gone: true,
    paused_for_manual_kill_test: true,
}

Root Cause:
(none)

Security Impact:
gnome-remote-desktop.service must stay masked (experiment-safety.md §5); every ApplyMonitorsConfig call uses method=Temporary and never writes monitors.xml; a systemd-run watchdog is armed before the first real disable.

Recommended Action:
(none)

Follow-up:
exp07_restore.rs independently restores from the persisted DisplayBackup JSON, decoupled from whether this process is still alive (Phase 5 plan Decision 1).
