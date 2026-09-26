Experiment: Experiment 4 — Virtual Monitor Owner-Loss Probe
Date: 2026-09-26T17:08:38.709249956Z
Environment: Development workstation (host = target), Ubuntu 26.04 / GNOME 50.1
Objective:
Observe session-owner disappearance without disabling physical outputs.

Hypothesis:
Closing the ScreenCast owner's D-Bus connection without Stop removes its virtual monitor without crashing GNOME Shell.

Procedure:
Create and confirm one virtual monitor, capture frames, persist evidence, then close the owning D-Bus connection without Stop. External read-only checks must verify the outcome.

Expected:
Original physical displays remain active; virtual monitor removal and Shell survival must be confirmed independently.

Observed:
before_owner_close=ResolutionResult {
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
    teardown_confirmed: false,
}
shell_pid_before="34735"

Evidence:
- docs/experiments/evidence/exp04/2026-09-26-2/report.md

Result:
PARTIAL

Failure:
(none)

Root Cause:
(none)

Security Impact:
May crash GNOME Shell; physical outputs are not disabled. No FEAS-C conclusion follows from this probe.

Recommended Action:
Check Shell PID, journal, and GetCurrentState from an independent connection before any further experiment.

Follow-up:
(none)
