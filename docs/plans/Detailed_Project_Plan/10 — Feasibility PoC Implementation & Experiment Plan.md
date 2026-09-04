# 10 — Feasibility PoC Implementation & Experiment Plan

## 1. Purpose

This document defines the implementation sequence for the feasibility proof-of-concept.

The PoC exists to answer one question:

> **Can this system safely take control of an existing Ubuntu 26.04 + GNOME 50+ Wayland session, move the active display to a virtual output, isolate the physical display and input, accept remote input, and reliably fail back to a locked physical console?**

The PoC is not the product.

Do not implement the complete application until the feasibility gates in this document pass.

---

# 2. Critical Principle

The project has several technically difficult assumptions.

Do not assume that because GNOME exposes an API, the complete required workflow is supported.

In particular, experimentally validate:

```text
same GNOME session
+
virtual monitor
+
physical display isolation
+
remote input
+
physical input isolation
+
GNOME locking
+
safe teardown
+
emergency takeover
```

The PoC must produce evidence for each.

---

# 3. Supported Environment

All experiments must initially target:

```text
Ubuntu 26.04 LTS
GNOME 50+
Wayland
systemd
PipeWire
Mutter
single-user workstation
```

Record actual versions during every test run.

Example diagnostic output:

```text
OS:
GNOME:
Mutter:
gnome-shell:
gnome-remote-desktop:
PipeWire:
WirePlumber:
Kernel:
GPU:
GPU driver:
Wayland compositor:
```

Do not assume the installed package version from the Ubuntu release alone.

---

# 4. Explicit PoC Non-Goals

Do NOT implement during the initial PoC:

- production authentication;
- Remote Access Key management;
- trusted devices;
- TOTP;
- public Internet access;
- NAT traversal;
- TURN;
- rendezvous infrastructure;
- polished browser UI;
- native clients;
- file transfer;
- clipboard synchronization;
- multi-user support;
- KDE support;
- wlroots support;
- X11 support;
- generic remote administration;
- arbitrary shell execution;
- kernel modifications.

The PoC should be local and controlled.

---

# 5. PoC Success Definition

The PoC succeeds only if it demonstrates:

```text id="q6m2v8"
Existing GNOME session
        ↓
Virtual monitor created
        ↓
Virtual monitor becomes active display
        ↓
Physical outputs disabled
        ↓
Physical keyboard/mouse isolated
        ↓
Remote input reaches same GNOME session
        ↓
Remote display is captured
        ↓
Network/control failure
        ↓
Remote authority revoked
        ↓
GNOME locked
        ↓
Physical display restored
        ↓
Physical input restored
        ↓
Same GNOME session remains available
```

The exact implementation can evolve.

The demonstrated behavior cannot be hand-waved.

---

# 6. Experiment Methodology

Every experiment must contain:

```text
Hypothesis
Environment
Prerequisites
Procedure
Expected Result
Observed Result
Evidence
Failure Analysis
Conclusion
```

Use:

```text
PASS
FAIL
PARTIAL
BLOCKED
NOT_TESTED
```

Do not mark an experiment PASS based only on visual inspection when programmatic verification is possible.

---

# 7. Experiment 0 — Environment Discovery

Create a diagnostic utility that reports the environment.

Collect:

```text id="f8p2r7"
OS version
GNOME version
Mutter version
gnome-shell version
Wayland display
session type
user
UID
seat
systemd user session
PipeWire availability
WirePlumber availability
GPU
GPU driver
connected outputs
input devices
```

Also detect relevant D-Bus services/interfaces.

The diagnostic utility must not modify the system.

### Acceptance

The utility produces a reproducible environment report.

---

# 8. Experiment 1 — GNOME Session Discovery

Determine exactly how the active GNOME session is identified.

Investigate:

```text id="p7v3k2"
loginctl
systemd-logind
D-Bus
XDG_SESSION_ID
XDG_RUNTIME_DIR
WAYLAND_DISPLAY
DBUS_SESSION_BUS_ADDRESS
GNOME-specific session information
```

Determine:

- active user;
- UID;
- session ID;
- seat;
- session type;
- graphical state;
- Wayland display;
- user runtime directory.

### Acceptance

The implementation can reliably identify the intended user's active GNOME Wayland session without guessing.

---

# 9. Experiment 2 — Mutter Capability Inventory

Determine which relevant Mutter interfaces are actually available.

Investigate:

```text id="m4q8v1"
org.gnome.Mutter.ScreenCast
org.gnome.Mutter.RemoteDesktop
org.gnome.Mutter.DisplayConfig
```

Document:

- available interfaces;
- methods;
- signals;
- object paths;
- permissions;
- behavior;
- version differences.

Pay particular attention to:

```text
RecordVirtual
RemoteDesktop
ScreenCast
DisplayConfig
```

### Acceptance

Produce an API inventory containing the exact interfaces available on the test system.

---

# 10. Experiment 3 — Basic Screen Capture

Prove that the existing GNOME session can be captured.

Start with the simplest supported capture mechanism.

Verify:

```text id="w6r2k9"
capture starts
frames arrive
frames correspond to current GNOME session
PipeWire stream is valid
capture stops cleanly
```

Do not yet modify the physical display.

### Acceptance

A reproducible test demonstrates live frames from the existing GNOME session.

---

# 11. Experiment 4 — Virtual Monitor Creation

Use the Mutter virtual-monitor mechanism.

Investigate `RecordVirtual` and associated behavior.

Create a virtual display.

Record:

```text id="q9m4x6"
virtual monitor identifier
resolution
refresh rate
position
scale
associated PipeWire node
Mutter object path
```

Verify that the virtual monitor actually exists rather than merely creating a capture stream.

### Acceptance

A real virtual monitor appears in GNOME/Mutter state.

---

# 12. Experiment 5 — Virtual Monitor as Active Display

Determine whether the virtual monitor can become the active display.

Test:

```text id="n7p3k8"
physical output(s)
virtual output
display topology
primary output
window placement
GNOME Shell behavior
```

Verify that applications can render on the virtual monitor.

### Acceptance

The virtual monitor can function as a usable display in the same GNOME session.

---

# 13. Experiment 6 — Physical Output Isolation

Test disabling physical outputs while keeping the virtual monitor active.

Before changing anything:

```text id="t8q2m5"
capture complete physical display topology
```

Then:

```text id="x4v9k1"
create virtual monitor
activate virtual monitor
disable physical outputs
```

Verify:

- physical output is no longer an active display;
- virtual monitor remains active;
- GNOME session continues;
- remote capture remains functional.

### Important

Do not equate:

```text output disabled
```

with:

```text monitor physically powered off
```

Test the actual hardware behavior.

### Acceptance

The physical display cannot expose the active desktop during remote mode.

---

# 14. Experiment 7 — Display Restoration

Restore the original topology.

Verify:

```text id="m6r2p8"
resolution restored
refresh rate restored
position restored
scale restored
primary display restored
physical outputs restored
virtual monitor removed
```

Test at least:

- one monitor;
- multiple monitors where available;
- different resolutions;
- high-refresh display where available.

### Acceptance

The original physical display topology is restored reliably.

---

# 15. Experiment 8 — Remote Input

Investigate Mutter RemoteDesktop and libei/EIS.

Prove:

```text id="q3w8m5"
remote pointer movement
remote clicks
remote keyboard
modifier keys
special keys
```

are delivered to the existing GNOME session.

Verify that input reaches the intended session and not another session.

### Acceptance

Remote keyboard and pointer control works reliably.

---

# 16. Experiment 9 — Physical Input Isolation

This is one of the hardest feasibility gates.

Determine how to prevent physical keyboard/mouse events from controlling the GNOME session while remote control is active.

Investigate, in order:

```text id="v8q4n2"
GNOME/Mutter/libei mechanisms
seat/input routing
libinput integration
uinput-based approaches
minimal privileged input helper
```

Do NOT immediately implement a global `/dev/input` grab.

Determine whether that is actually required.

Record:

- privileges required;
- devices affected;
- hotplug behavior;
- multi-device behavior;
- emergency-key behavior;
- interaction with GNOME lock;
- failure behavior.

### Acceptance

Physical keyboard and pointer cannot alter the remote GNOME session during REMOTE_ACTIVE.

If this cannot be achieved safely, the project is blocked.

---

# 17. Experiment 10 — Input Restoration

After remote mode ends:

```text id="j2m7q4"
restore physical keyboard
restore physical mouse
```

Test:

- USB keyboard;
- USB mouse;
- Bluetooth devices where available;
- docking/hotplug;
- device reconnect.

### Acceptance

Local input works again after teardown.

---

# 18. Experiment 11 — GNOME Lock Semantics

This experiment is critical.

Test:

```text id="p9x3m6"
GNOME active
+
remote desktop attached
+
lock GNOME
```

Determine whether the remote-control connection:

- remains usable;
- disconnects;
- loses input;
- loses capture;
- terminates completely.

Document exact behavior.

The previous assumption that ordinary GNOME Remote Desktop can remain attached through screen lock must NOT be treated as proven.

### Acceptance

Determine experimentally whether the desired:

```text
locked physical console
+
same-session remote control
```

is technically achievable with the selected GNOME mechanisms.

If not, document the exact incompatibility.

---

# 19. Experiment 12 — Same-Session Continuity

Prove that remote mode is controlling the original GNOME session.

Use a visible application opened before remote activation.

Example:

```text id="k7p4m1"
open terminal locally
open text editor
create test marker
activate remote mode
```

Verify remotely that the exact same application/session state is present.

Then terminate remote mode.

Verify locally that the original session remains intact.

### Acceptance

Remote mode demonstrably controls the same GNOME session.

---

# 20. Experiment 13 — Normal Teardown

Test:

```text id="w4m8q2"
REMOTE_ACTIVE
      ↓
explicit disconnect
      ↓
revoke remote input
      ↓
restore physical display
      ↓
restore physical input
      ↓
lock GNOME
```

Verify each step independently.

### Acceptance

Normal disconnect produces:

```text LOCAL_LOCKED
```

---

# 21. Experiment 14 — Abrupt Client Disconnect

Kill the client without clean shutdown.

Examples:

```text id="x8q3m6"
close browser process
kill client
disable network
remove Ethernet cable
disable Wi-Fi
```

Verify:

```text id="p4v7n1"
lease eventually expires
remote input revoked
GNOME locked
physical display restored
physical input restored
```

### Acceptance

The client is not required to perform cleanup.

---

# 22. Experiment 15 — Main Agent Crash

Terminate the main remote process during REMOTE_ACTIVE.

Verify:

```text id="m5q8r2"
remote authority expires/revokes
physical input restored
physical display restored
GNOME locked
```

The exact mechanism may use lease expiration, systemd supervision, GNOME-agent behavior, or another validated mechanism.

### Acceptance

Main-agent failure cannot leave indefinite remote control.

---

# 23. Experiment 16 — GNOME Agent Crash

Terminate the user-session GNOME agent while remote control is active.

Verify:

```text id="k2v9p5"
remote input revoked
remote session terminated
local state recovered
```

### Acceptance

The user-session agent cannot become a single point of permanent remote-control authority.

---

# 24. Experiment 17 — PipeWire Failure

Cause the PipeWire capture pipeline to fail.

Determine whether the system should:

```text id="j8m3q7"
recover media
```

or:

```text id="v4p9k2"
terminate remote control
```

For the initial product, safety should take priority over preserving a broken connection.

### Acceptance

A broken media pipeline cannot result in uncontrolled remote input.

---

# 25. Experiment 18 — Mutter Failure

Simulate or induce a Mutter-related failure where safely possible.

Verify:

```text id="r7m2x5"
remote input revoked
remote session terminated
recovery attempted
physical display restored
physical input restored
GNOME locked
```

Do not intentionally crash the production desktop repeatedly without a safe test environment.

---

# 26. Experiment 19 — Display Restoration Failure

Inject a controlled failure into the display restoration layer.

The state machine should detect:

```text id="x5q8m3"
restore operation failed
```

and transition to:

```text id="p2v7k9"
RECOVERING
```

rather than falsely declaring:

```text LOCAL_ACTIVE
```

If verification remains uncertain:

```text FAILED_SAFE
```

---

# 27. Experiment 20 — Input Restoration Failure

Similarly inject failure into physical input restoration.

Expected:

```text id="q8m4v1"
remote authority revoked
recovery attempted
FAILED_SAFE if local control cannot be verified
```

The system must never preserve remote input as a fallback.

---

# 28. Experiment 21 — Emergency Takeover

Implement the smallest possible emergency prototype.

Suggested trigger:

```text id="n3x7p8"
Ctrl + Alt + Shift + F12
```

held approximately two seconds.

The exact key combination remains configurable.

Test while:

```text id="w6q2m9"
remote active
network connected
main daemon healthy
```

Expected:

```text id="r4p8k1"
revoke remote input
terminate remote session
increment security epoch
lock GNOME
restore physical display
restore physical input
remain locked
```

### Acceptance

Emergency takeover works reliably.

---

# 29. Experiment 22 — Emergency During Network Failure

Disconnect the network first.

Then trigger emergency locally.

Expected:

```text id="m7q3v9"
emergency works
```

without requiring network communication.

---

# 30. Experiment 23 — Emergency During Main-Daemon Failure

Cause the main remote daemon to become unavailable.

Trigger emergency.

Expected:

```text id="p5x8m2"
emergency still works
```

This is a mandatory gate.

---

# 31. Experiment 24 — Stale Session After Emergency

Sequence:

```text id="c9v4q7"
1. Establish remote session
2. Record session identifier
3. Trigger emergency
4. Increment security epoch
5. Attempt reconnect using old session
```

Expected:

```text id="x2m8p5"
DENIED
```

Normal authentication must be required again.

---

# 32. Experiment 25 — Reconnect After Normal Network Failure

Test:

```text id="g6q2v8"
REMOTE_ACTIVE
    ↓
network interruption
    ↓
REMOTE_DEGRADED
    ↓
network restored before lease expiry
```

Verify that reconnection is possible without violating security.

Then repeat with:

```text id="r9m3x6"
network interruption
    ↓
lease expires
    ↓
network restored
```

Expected:

```text id="k4p8q1"
remote session no longer valid
new authentication required
```

---

# 33. Experiment 26 — Monitor Hotplug

During remote mode:

```text id="w7m2q5"
connect monitor
disconnect monitor
dock laptop
undock laptop
```

Verify:

```text id="x3v9k4"
physical outputs remain isolated
new outputs do not unexpectedly become visible
original topology can still be restored
```

---

# 34. Experiment 27 — Resolution and Refresh Rates

Test combinations such as:

```text id="p8q4m2"
1920x1080 @ 60 Hz
2560x1440 @ 60 Hz
3840x2160 @ 60 Hz
high-refresh display
mixed-resolution monitors
```

Record:

- virtual monitor creation time;
- stability;
- capture quality;
- teardown reliability;
- restoration reliability.

Do not optimize performance before correctness.

---

# 35. Experiment 28 — Cursor Behavior

Test:

```text id="m4x8q2"
hardware cursor
software cursor
cursor movement
cursor visibility
cursor updates without screen damage
```

If GNOME/Mutter hardware cursor behavior causes issues, document and isolate any required workaround.

Do not globally disable hardware acceleration merely because one test fails.

---

# 36. Experiment 29 — GPU Matrix

Test available hardware where practical.

At minimum aim for:

```text id="q7m3v8"
Intel
AMD
NVIDIA
```

Do not claim universal support based on one GPU.

Record:

```text GPU
driver
kernel
GNOME
Mutter
virtual monitor result
capture result
input result
teardown result
```

---

# 37. Experiment 30 — Suspend / Resume

Test:

```text id="x9p4m6"
remote active
suspend
resume
```

Determine whether:

- session survives;
- virtual monitor survives;
- PipeWire survives;
- input survives.

The preferred product behavior is to inhibit suspend during remote mode if technically appropriate.

If suspend occurs anyway, recover safely.

---

# 38. Experiment 31 — GNOME Logout

During remote mode:

```text id="k6q2m8"
logout GNOME session
```

Expected:

```text id="r3v7p1"
remote session terminated
remote authority revoked
no attachment to another session
```

---

# 39. Experiment 32 — Power-Loss Recovery

Where practical, simulate abrupt power loss.

After reboot verify:

```text id="q8m3v5"
no stale remote session
no stale remote input
no automatic unlock
no automatic reconnection
safe local state
```

---

# 40. Experiment 33 — State-Machine Fault Injection

Implement an internal test mode that can force failures in:

```text id="w5p9m2"
session discovery
virtual monitor creation
display switching
input isolation
capture setup
remote input
display restoration
input restoration
locking
```

Verify every failure path.

The fault-injection framework should not exist as an unrestricted production feature.

---

# 41. Experiment 34 — Race Conditions

Test simultaneous events:

```text id="m8q4v7"
disconnect + emergency
reconnect + emergency
lease expiry + reconnect
monitor hotplug + disconnect
Mutter failure + emergency
PipeWire failure + disconnect
daemon restart + reconnect
```

Expected priority:

```text id="x2p7m9"
EMERGENCY
>
SAFETY FAILURE
>
LEASE EXPIRY
>
DISCONNECT
>
NORMAL OPERATION
>
RECONNECT
```

---

# 42. Experiment 35 — Repeated Activation/Teardown

Perform many cycles:

```text id="k7m3q8"
activate
use
disconnect
restore
lock
repeat
```

Record:

- Mutter stability;
- memory growth;
- PipeWire leaks;
- display configuration drift;
- input-device drift;
- GNOME Shell crashes.

A single successful cycle is insufficient.

---

# 43. Experiment 36 — Long-Running Stability

Run remote mode for an extended period.

Measure:

```text id="p4x8m2"
CPU
memory
GPU
PipeWire stability
frame delivery
input latency
lease renewal
GNOME stability
```

The exact duration can be determined during PoC execution.

Correctness remains more important than performance.

---

# 44. Experiment 37 — Physical Privacy Verification

Do not rely solely on software state.

During remote mode inspect the physical display.

Verify:

```text id="m7q2v4"
desktop is not visible
notifications are not exposing the active session
physical monitor is not simply showing the remote desktop
```

Where possible, test using an independent camera/observer rather than the same software stack being evaluated.

---

# 45. Experiment 38 — Physical Input Verification

Use an independent physical input test.

During REMOTE_ACTIVE:

```text id="x8p3m6"
type on physical keyboard
move physical mouse
click
```

Verify that the remote GNOME session does not react.

Then after teardown:

```text id="q5v9m2"
physical keyboard works
physical mouse works
```

---

# 46. Evidence Requirements

For each hard gate capture:

```text id="n6m2q8"
environment report
commands/API calls used
logs
screenshots where useful
state-machine transitions
Mutter object/interface information
PipeWire information
display topology before/after
input state before/after
failure result
recovery result
```

Avoid collecting secrets.

---

# 47. Experiment Result Format

Use a consistent report:

```text
Experiment:
Date:
Environment:
Objective:

Hypothesis:

Procedure:

Expected:

Observed:

Evidence:

Result:
PASS / FAIL / PARTIAL / BLOCKED

Failure:

Root Cause:

Security Impact:

Recommended Action:

Follow-up:
```

---

# 48. Hard Feasibility Gates

The following gates are mandatory.

## Gate A — Same Session

Remote control demonstrably uses the existing GNOME session.

## Gate B — Virtual Monitor

A usable virtual monitor can be created and captured.

## Gate C — Physical Display Isolation

Physical display cannot expose the active desktop during remote mode.

## Gate D — Remote Input

Keyboard/pointer input can reliably reach the GNOME session.

## Gate E — Physical Input Isolation

Physical keyboard/mouse cannot control the session during remote mode.

## Gate F — Safe Teardown

Normal and abnormal termination revoke remote control and restore the physical console.

## Gate G — Emergency

Independent local emergency takeover works even if the main remote system is unhealthy.

## Gate H — Session Continuity

The same GNOME session survives remote activation and teardown.

---

# 49. Stop Conditions

Stop product development and reassess architecture if any of these occur:

```text id="q3m7v9"
same-session reuse is impossible
physical input cannot be isolated safely
physical display cannot be isolated reliably
remote input requires unsafe privileges
emergency takeover cannot work independently
display restoration is unreliable
input restoration is unreliable
Mutter becomes unstable
GNOME lock semantics make the desired workflow impossible
required privileges become too broad
```

Do not work around a failed security gate by weakening the requirement without explicitly revisiting the product specification.

---

# 50. PoC Architecture

The PoC should use the smallest possible architecture.

Conceptually:

```text id="x7m4q2"
+-------------------------+
| PoC Controller          |
| unprivileged            |
+------------+------------+
             |
       authenticated IPC
             |
             v
+-------------------------+
| GNOME Session Adapter   |
| user session            |
+------------+------------+
             |
       +-----+------+
       |            |
       v            v
    Mutter       PipeWire
```

A minimal emergency prototype may be added separately:

```text id="p8q3m6"
+-------------------------+
| Emergency Prototype     |
| minimal privilege       |
+-------------------------+
```

Do not build the production gateway architecture yet.

---

# 51. PoC Security Requirements

Even though this is a prototype:

- do not expose it to the public Internet;
- do not hard-code production credentials;
- do not store secrets unnecessarily;
- do not expose arbitrary shell execution;
- do not create a generic privileged D-Bus proxy;
- do not run the whole PoC as root;
- isolate privileged experiments;
- clearly label experimental interfaces.

---

# 52. PoC Logging

Log:

```text id="m5q8v3"
experiment
state
transition
Mutter operation
PipeWire operation
display operation
input operation
failure
recovery
```

Never log:

```text id="q2x7m9"
password
TOTP
Remote Access Key
session bearer credential
private keys
```

---

# 53. PoC State Machine

Implement only the states necessary to test feasibility:

```text id="v8m3q5"
LOCAL_ACTIVE
LOCAL_LOCKED
PREPARING_REMOTE
REMOTE_ACTIVE
TEARING_DOWN
RECOVERING
EMERGENCY
FAILED_SAFE
```

Authentication and networking can use mocked/local test adapters.

---

# 54. PoC Interfaces

Keep the state machine independent from GNOME implementation.

Conceptual interfaces:

```python id="f3q7m9"
class DisplayController:
    def snapshot(self): ...
    def create_virtual_display(self): ...
    def disable_physical_outputs(self): ...
    def restore(self): ...
    def destroy_virtual_display(self): ...


class InputController:
    def snapshot(self): ...
    def enable_remote_input(self): ...
    def disable_physical_input(self): ...
    def restore(self): ...


class SessionController:
    def lock(self): ...
    def verify_session(self): ...


class CaptureController:
    def start(self): ...
    def stop(self): ...


class RemoteInputController:
    def start(self): ...
    def stop(self): ...


class SafetyController:
    def emergency_stop(self): ...
```

These are conceptual interfaces.

Follow the repository/workflow structure already established by `adaptive-workflow-configurator`.

---

# 55. Mock Testing

Before touching real GNOME hardware, test the state machine using mocked adapters.

Example:

```text id="n9q4m7"
DisplayController
InputController
SessionController
CaptureController
RemoteInputController
```

Simulate:

```text id="x5m8p2"
success
failure
timeout
partial completion
duplicate cleanup
concurrent events
```

This allows the state machine to be validated independently.

---

# 56. Real-System Testing

After mocked state-machine tests pass:

```text id="q7v3m8"
run against real GNOME
```

Start with:

```text id="m4x9p2"
single monitor
standard resolution
wired keyboard
wired mouse
Intel/AMD if available
```

Then expand the hardware matrix.

---

# 57. No Premature Optimization

Do not optimize:

- codec selection;
- frame rate;
- GPU acceleration;
- bandwidth;
- latency;
- browser rendering;

until:

```text id="p6q2m8"
display isolation
input isolation
safe teardown
emergency takeover
```

are proven.

A fast unsafe remote console is a failed product.

---

# 58. PoC Deliverables

At the end of the feasibility phase, produce:

```text id="x3m7q9"
1. Environment report
2. GNOME/Mutter API inventory
3. Session-discovery implementation
4. Virtual-monitor prototype
5. Display-isolation prototype
6. Remote-input prototype
7. Physical-input-isolation prototype
8. Lock-semantics findings
9. Teardown/recovery prototype
10. Emergency prototype
11. State-machine tests
12. Failure-injection tests
13. Hardware test results
14. Feasibility report
15. List of blockers
16. Recommendation:
       PROCEED
       MODIFY ARCHITECTURE
       STOP
```

---

# 59. Final Feasibility Report

The final report must explicitly answer:

### 1. Can we control the same GNOME session?

```text
YES / NO / PARTIAL
```

### 2. Can we create a virtual monitor?

```text
YES / NO / PARTIAL
```

### 3. Can we reliably isolate physical displays?

```text
YES / NO / PARTIAL
```

### 4. Can remote keyboard/pointer input work?

```text
YES / NO / PARTIAL
```

### 5. Can physical keyboard/mouse be isolated safely?

```text
YES / NO / PARTIAL
```

### 6. Can GNOME remain usable remotely under the required lock semantics?

```text
YES / NO / PARTIAL
```

### 7. Can the system safely recover after network failure?

```text
YES / NO / PARTIAL
```

### 8. Can the emergency mechanism work independently?

```text
YES / NO / PARTIAL
```

### 9. Can the original physical console be restored reliably?

```text
YES / NO / PARTIAL
```

### 10. Are the required privileges acceptable?

```text
YES / NO / PARTIAL
```

---

# 60. Decision Rule

The product may proceed to full implementation only if:

```text
Gate A = PASS
Gate B = PASS
Gate C = PASS
Gate D = PASS
Gate E = PASS
Gate F = PASS
Gate G = PASS
Gate H = PASS
```

and:

```text
required privileges are acceptable
Mutter/GNOME stability is acceptable
recovery behavior is deterministic
```

If any hard gate is `FAIL`, stop.

If any gate is `PARTIAL`, investigate until it becomes either:

```text
PASS
```

or:

```text
FAIL
```

Do not treat `PARTIAL` as production-ready.

---

# 61. Instructions to GitHub Copilot Agent

When executing this document:

1. Inspect the repository first.
2. Inspect the workflow/configuration generated by `adaptive-workflow-configurator`.
3. Follow that workflow exactly where applicable.
4. Do not invent a competing project-management structure.
5. Read the existing project documents before implementation.
6. Start with Experiment 0.
7. Do not jump directly to full product implementation.
8. Research the actual Ubuntu/GNOME APIs available on the target system.
9. Prefer official GNOME/Mutter/PipeWire/libei documentation and actual system introspection.
10. Validate assumptions experimentally.
11. Build the smallest possible diagnostic tools first.
12. Implement mocked state-machine tests before real hardware operations.
13. Keep GNOME-specific/private APIs isolated.
14. Keep privileged functionality minimal.
15. Never introduce arbitrary privileged command execution.
16. Record experiment results as implementation progresses.
17. Treat physical input isolation as a hard blocker.
18. Treat GNOME lock semantics as a hard blocker.
19. Treat emergency takeover as a hard safety requirement.
20. Do not proceed to production networking/authentication/UI until the feasibility gates pass.

If an experiment contradicts the current architecture, stop and report the contradiction.

Do not silently redesign around it.

---

# 62. Final Engineering Principle

The purpose of this phase is not to prove that the application can be made to work once.

It is to prove that it can be made to work:

```text
RELIABLY
REPEATABLY
SAFELY
RECOVERABLY
```

on the supported Ubuntu/GNOME environment.

The most important experiment is therefore not:

> "Can we display the desktop remotely?"

It is:

> **"Can we safely transfer interactive authority to the remote client and guarantee that the workstation returns to a locked, locally controllable state when anything goes wrong?"**

Until that question has a demonstrated answer, the product should remain a feasibility PoC.