Experiment: Experiment 5 — Virtual Monitor as Active Display
Date: 2026-09-05T14:05:12.901922046Z
Environment: Development workstation (host = target), Ubuntu 26.04 / GNOME 50.1
Objective:
Make the virtual monitor an additional active display alongside the existing physical monitor(s), without disabling any physical monitor (Document 10 §12, Document 02 §11).

Hypothesis:
ApplyMonitorsConfig(method=Temporary) can add the virtual monitor's connector as a new logical monitor while leaving existing physical logical monitors unchanged; the compositor then renders the desktop on it (provable by capturing it via RecordMonitor); the original topology is fully restorable.

Procedure:
Snapshot GetCurrentState; create a virtual monitor (Experiment 4 mechanics); build a new logical-monitors array = original entries + one new entry for the virtual connector; ApplyMonitorsConfig(Temporary); verify; RecordMonitor the virtual connector and receive frames; restore the original array; verify restoration; tear down.

Expected:
The virtual monitor becomes a second active logical monitor, physical monitors remain enabled and unchanged, real frames are captured from the virtual connector, and the original topology is exactly restored.

Observed:
Findings {
    physical_connectors_before: [
        "HDMI-1",
        "eDP-1",
    ],
    virtual_connector: Some(
        "Meta-0",
    ),
    logical_monitor_count_before: 2,
    logical_monitor_count_after_apply: Some(
        3,
    ),
    apply_error: None,
    virtual_logical_monitor_present: true,
    physical_connectors_still_present_after_apply: true,
    render_frames_received: 2,
    restore_error: None,
    topology_restored: true,
    physical_connectors_after_restore: [
        "HDMI-1",
        "eDP-1",
    ],
    virtual_connector_fully_gone: true,
    stop_error: None,
}

Evidence:
- docs/experiments/evidence/exp05/<date>/report.md

Result:
PASS

Failure:
(none)

Root Cause:
(none)

Security Impact:
gnome-remote-desktop.service must stay masked for this run (docs/ops/experiment-safety.md §5); ApplyMonitorsConfig(Temporary) never persists to monitors.xml, and this experiment explicitly restores the original topology before exiting (RAII guard covers early-error paths too).

Recommended Action:
(none)

Follow-up:
Physical-monitor removal (Doc 02 §11's 'can be removed from the active topology') is deferred to Phase 5 Experiment 6 under its full safety procedure (Phase 4 plan Decision #3) — not attempted here.
