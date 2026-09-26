Experiment: Experiment 7 — Display Restoration
Date: 2026-09-26T17:30:56.028620289Z
Environment: Development workstation (host = target), Ubuntu 26.04 / GNOME 50.1
Objective:
Restore the exact original physical display topology from a persisted DisplayBackup, independently of whether the isolating process is still alive (Document 10 §14).

Hypothesis:
Re-applying the backup's write-side config (reusing its captured mode IDs, only the ApplyMonitorsConfig serial re-read fresh) restores the exact original topology regardless of which process created the backup.

Procedure:
Load the DisplayBackup JSON; read current state (diagnostic); apply the reconstructed write-side config (retry once on failure per Doc 05 §62); poll GetCurrentState until the topology matches or the wait bound elapses.

Expected:
Every backed-up output's position/mode/scale/transform/primary matches exactly; no unexpected connector remains from a lingering virtual monitor.

Observed:
Findings {
    backup_path: "/media/[USER]/Playground/Playground_Sys/Blackroom Console/docs/experiments/evidence/exp06/2026-09-26-2/backup.json",
    monitors_before_restore: [
        "eDP-1",
        "HDMI-1",
        "Meta-0",
    ],
    unexpected_connectors_before_restore: [
        "eDP-1",
        "Meta-0",
    ],
    apply_error: None,
    retried: false,
    topology_matches: true,
    unexpected_connectors_after_restore: [
        "eDP-1",
        "Meta-0",
    ],
}

Evidence:
- docs/experiments/evidence/exp07/2026-09-26-2/report.md

Result:
PASS

Failure:
(none)

Root Cause:
(none)

Security Impact:
ApplyMonitorsConfig uses method=Temporary; this binary never writes monitors.xml and never enables remote input regardless of outcome (Doc 05 §62 step 2).

Recommended Action:
(none)

Follow-up:
(none)
