Experiment: Experiment 4 — Virtual Monitor Creation
Date: 2026-09-05T12:02:51.707587575Z
Environment: Development workstation (host = target), Ubuntu 26.04 / GNOME 50.1
Objective:
Create a real virtual monitor via RecordVirtual at 3 resolutions, confirm via DisplayConfig.GetCurrentState, destroy cleanly, and verify 50-cycle reliability (Document 10 §11, Doc 19 §16-17).

Hypothesis:
ScreenCast.Session.RecordVirtual with a width/height/framerate properties dict creates a real logical monitor visible in GetCurrentState; Stop() removes it; repeated cycles leak no PipeWire nodes and do not crash GNOME Shell.

Procedure:
For each resolution: CreateSession, RecordVirtual, Start, wait for PipeWireStreamAdded, receive frames, snapshot GetCurrentState, Stop, snapshot again. Then repeat the 1920x1080 cycle 50 times with a 10s/cycle bound, diffing `pw-dump` and comparing the GNOME Shell PID before/after.

Expected:
All 3 resolutions produce a confirmed, then cleanly torn-down, virtual monitor; 50/50 cycles complete within the bound with no pw-dump diff and no GNOME Shell PID change.

Observed:
resolution_results=[
    ResolutionResult {
        width: 1280,
        height: 720,
        refresh_rate: 60.0,
        record_virtual_error: None,
        new_connectors_after_create: [
            "Meta-0",
        ],
        observed_mode: Some(
            ConnectorMode {
                connector: "Meta-0",
                width: 1280,
                height: 720,
                refresh_rate: 60.0,
            },
        ),
        resolution_honored: true,
        frames_received: 2,
        stop_error: None,
        new_connectors_after_destroy: [],
        monitor_confirmed: true,
        teardown_confirmed: true,
    },
    ResolutionResult {
        width: 1920,
        height: 1080,
        refresh_rate: 60.0,
        record_virtual_error: None,
        new_connectors_after_create: [
            "Meta-0",
        ],
        observed_mode: Some(
            ConnectorMode {
                connector: "Meta-0",
                width: 1920,
                height: 1080,
                refresh_rate: 60.0,
            },
        ),
        resolution_honored: true,
        frames_received: 2,
        stop_error: None,
        new_connectors_after_destroy: [],
        monitor_confirmed: true,
        teardown_confirmed: true,
    },
    ResolutionResult {
        width: 2560,
        height: 1440,
        refresh_rate: 60.0,
        record_virtual_error: None,
        new_connectors_after_create: [
            "Meta-0",
        ],
        observed_mode: Some(
            ConnectorMode {
                connector: "Meta-0",
                width: 2560,
                height: 1440,
                refresh_rate: 60.0,
            },
        ),
        resolution_honored: true,
        frames_received: 2,
        stop_error: None,
        new_connectors_after_destroy: [],
        monitor_confirmed: true,
        teardown_confirmed: true,
    },
]
all_confirmed=true
all_torn_down=true
cycles_run=50
cycles_clean=true
screencast_video_nodes_before=Some(0)
screencast_video_nodes_after=Some(0)
no_leaked_nodes=true
shell_pid_before=Some("7841")
shell_pid_after=Some("7841")
shell_survived=true

Evidence:
- docs/experiments/evidence/exp04/<date>/report.md

Result:
PASS

Failure:
(none)

Root Cause:
(none)

Security Impact:
gnome-remote-desktop.service must stay masked for this run (docs/ops/experiment-safety.md §5).

Recommended Action:
(none)

Follow-up:
Port the proven mechanics into crates/blackroom-gnome/src/mutter/{virtual_monitor.rs,pipewire_capture.rs} (Phase 4 plan step 6).
