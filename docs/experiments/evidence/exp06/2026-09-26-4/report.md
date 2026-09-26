Experiment: Experiment 6 — Automatic HDMI-only Owner-Loss Probe
Date: 2026-09-26T18:40:40.133976991Z
Environment: Development workstation (host = target), Ubuntu 26.04 / GNOME 50.1
Objective:
Observe process death while a virtual-only display configuration is active.

Hypothesis:
Mutter removes the owning virtual monitor and restores physical outputs when exp06 is killed.

Procedure:
Persist the original HDMI-only backup, arm the watchdog, isolate outputs, verify timer/topology/Shell/power twice, persist pre-kill evidence, then signal only this exp06 PID. Inspect restoration independently.

Expected:
Physical panels show no desktop during isolation; watchdog or Mutter restores the original topology without a Shell crash.

Observed:
pre_kill=AutoKillPreflight {
    original_logical_connectors: [
        "HDMI-1",
    ],
    raw_connectors: [
        "eDP-1",
        "HDMI-1",
        "Meta-0",
    ],
    active_logical_connectors: [
        "Meta-0",
    ],
    logical_monitor_count: 1,
    virtual_connector: "Meta-0",
    power_save_mode: 3,
    shell_pid_before: 34735,
    shell_pid_now: 34735,
    timer_active: true,
    arm_elapsed_ms: 826,
}

Evidence:
- docs/experiments/evidence/exp06/2026-09-26-4/backup.json
- docs/experiments/evidence/exp06/2026-09-26-4/pre_kill.json

Result:
PARTIAL

Failure:
(none)

Root Cause:
(none)

Security Impact:
Physical desktop privacy cannot be verified by D-Bus; the independent observer reports after recovery. GNOME Shell may crash.

Recommended Action:
Verify Shell PID, journal, physical screen and original topology from independent SSH before any further experiment.

Follow-up:
(none)
