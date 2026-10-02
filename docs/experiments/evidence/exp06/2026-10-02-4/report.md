Experiment: Experiment 6 — Physical Output Isolation
Date: 2026-10-02T08:05:23.793593857Z
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
        kernel: "7.0.0-34-generic",
        gpu_modules: [
            "xe",
            "nvidia",
            "i915",
        ],
    },
    physical_connectors_before: [
        "eDP-1",
    ],
    virtual_connector: Some(
        "Meta-0",
    ),
    apply_monitors_config_allowed: true,
    watchdog_armed: true,
    watchdog_unit: Some(
        "blackroom-exp06-watchdog-1790928323",
    ),
    cleanup_watchdog_unit: Some(
        "blackroom-exp06-watchdog-1790928323-cleanup",
    ),
    cycles: [],
    stop_error: None,
    virtual_connector_fully_gone: true,
    post_stop_restore_attempted: false,
    post_stop_restore_error: None,
    post_stop_pre_repair_state: None,
    paused_for_manual_kill_test: true,
    final_state: Some(
        FinalState {
            raw_connectors: [
                "eDP-1",
            ],
            logical_connectors: [
                "eDP-1",
            ],
            power_save_mode: Some(
                0,
            ),
            shell_pid: Some(
                3828099,
            ),
            session_id: Some(
                "182",
            ),
            watchdog_timer_active: Some(
                true,
            ),
            watchdog_timer_state: Some(
                "active",
            ),
            watchdog_service_state: Some(
                "inactive",
            ),
            topology_matches_original: true,
            configuration_hash_matches: Some(
                true,
            ),
            read_error: None,
            probe_errors: [],
        },
    ),
}

Evidence:
- docs/experiments/evidence/exp06/2026-10-02-4/report.md
- docs/experiments/evidence/exp06/2026-10-02-4/backup.json

Result:
PARTIAL

Failure:
Findings {
    host: HostInfo {
        kernel: "7.0.0-34-generic",
        gpu_modules: [
            "xe",
            "nvidia",
            "i915",
        ],
    },
    physical_connectors_before: [
        "eDP-1",
    ],
    virtual_connector: Some(
        "Meta-0",
    ),
    apply_monitors_config_allowed: true,
    watchdog_armed: true,
    watchdog_unit: Some(
        "blackroom-exp06-watchdog-1790928323",
    ),
    cleanup_watchdog_unit: Some(
        "blackroom-exp06-watchdog-1790928323-cleanup",
    ),
    cycles: [],
    stop_error: None,
    virtual_connector_fully_gone: true,
    post_stop_restore_attempted: false,
    post_stop_restore_error: None,
    post_stop_pre_repair_state: None,
    paused_for_manual_kill_test: true,
    final_state: Some(
        FinalState {
            raw_connectors: [
                "eDP-1",
            ],
            logical_connectors: [
                "eDP-1",
            ],
            power_save_mode: Some(
                0,
            ),
            shell_pid: Some(
                3828099,
            ),
            session_id: Some(
                "182",
            ),
            watchdog_timer_active: Some(
                true,
            ),
            watchdog_timer_state: Some(
                "active",
            ),
            watchdog_service_state: Some(
                "inactive",
            ),
            topology_matches_original: true,
            configuration_hash_matches: Some(
                true,
            ),
            read_error: None,
            probe_errors: [],
        },
    ),
}

Root Cause:
(none)

Security Impact:
gnome-remote-desktop.service must stay masked (experiment-safety.md §5); every ApplyMonitorsConfig call uses method=Temporary and never writes monitors.xml; a systemd-run watchdog is armed before the first real disable.

Recommended Action:
(none)

Follow-up:
exp07_restore.rs independently restores from the persisted DisplayBackup JSON, decoupled from whether this process is still alive (Phase 5 plan Decision 1).
