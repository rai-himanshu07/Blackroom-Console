Experiment: Experiment 9 (frozen daemon) — supervisor release (FEAS-E)
Date: 2026-10-01T13:38:49.504341857Z
Environment: Development workstation (host = target), Ubuntu 26.04 / GNOME 50.1, built-in keyboard, mouse and touchpad
Objective:
Show that a frozen remote-emergencyd holding the physical-input grab is killed by its supervisor, which releases the grab, within 30 s.

Hypothesis:
systemd WatchdogSec=10 with WatchdogSignal=SIGKILL kills a SIGSTOPped daemon and the kernel drops every grab.

Procedure:
Operator starts remote-emergencyd under systemd-run --user with a watchdog and arms a 1 s-accuracy kill timer; this probe isolates (lease 30 s), renews for 8 s, verifies the peer is the daemon, SIGSTOPs it and waits up to 60 s for the connection to close.

Expected:
Connection closed by the supervisor kill within 30 s; probe did not have to kill the daemon.

Observed:
result=PASS; Findings { isolated_nodes: Some(4), daemon_pid: Some(3597316), daemon_comm: Some("remote-emergenc"), closed_after_ms: Some(9883), ended_by: Some("daemon_closed"), daemon_gone_after_close: Some(true), notes: [] }

Evidence:
- findings.json (this directory)

Result:
PASS

Failure:
(none)

Root Cause:
(none)

Security Impact:
The physical keyboard, mouse and touchpad are grabbed for about ten seconds or more; the lock-out recovery is the external kill timer and SSH.

Recommended Action:
(none)

Follow-up:
FEAS-E decision is a separate review.
