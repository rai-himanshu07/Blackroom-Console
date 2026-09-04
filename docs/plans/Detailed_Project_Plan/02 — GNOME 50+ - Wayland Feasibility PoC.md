# GNOME 50+ / Wayland Feasibility PoC

## Purpose

This document defines the first technical implementation phase of the Remote Console project.

The goal is NOT to build the remote-access product.

The goal is to experimentally prove that the core GNOME/Wayland architecture required by the product is technically viable on:

- Ubuntu 26.04 LTS
- GNOME 50+
- Wayland
- systemd
- PipeWire
- Mutter

The PoC must answer one fundamental question:

> Can an existing logged-in GNOME session be safely transformed into a remote-controlled session using a virtual display while the physical display and physical input are isolated, and can the system reliably fail back to a locked local console?

Do not proceed to full product implementation until the critical requirements have been experimentally validated.

---

# 1. Relationship to Project Master Plan

The Project Master Plan is the overall product specification.

This document is the implementation specification for the first feasibility phase.

The implementation must respect:

- the repository structure
- coding conventions
- agent instructions
- testing conventions
- workflow configuration

established by `adaptive-workflow-configurator`.

Do not replace or duplicate its workflow.

Before starting implementation, inspect the configured repository and determine how this PoC should fit into the existing project structure.

---

# 2. Scope

This PoC is intentionally narrow.

## Must investigate

- GNOME session discovery
- Wayland session detection
- Mutter RemoteDesktop
- Mutter ScreenCast
- Mutter `RecordVirtual`
- Mutter DisplayConfig
- PipeWire
- libei
- libinput/input routing
- GNOME session locking
- systemd/logind
- physical monitor isolation
- physical input isolation
- remote input
- connection-loss recovery
- emergency recovery

## Must NOT implement yet

Do not implement:

- Internet connectivity
- rendezvous server
- STUN
- TURN
- production WebRTC networking
- public authentication service
- PAM authentication
- TOTP
- Remote Access Key
- trusted clients
- production browser UI
- file transfer
- audio
- clipboard
- power management
- multi-user support
- KDE
- wlroots
- X11

A temporary local test interface is acceptable where necessary.

---

# 3. Research Before Coding

Before writing significant code, research the current Ubuntu 26.04/GNOME 50 implementation.

Prioritize:

1. Current GNOME/Mutter source
2. Current Ubuntu 26.04 packages
3. Current GNOME documentation
4. Current API documentation
5. Working current implementations
6. Older documentation only for historical context

Do not rely on old X11-oriented remote-desktop tutorials.

Do not assume behavior from GNOME versions before 50.

---

# 4. Required Research Areas

## 4.1 Mutter RemoteDesktop

Determine:

- how a remote desktop session is created
- how input is authorized
- how keyboard input is injected
- how pointer input is injected
- whether the API can operate without a desktop portal
- whether the API can operate against an existing session
- session lifetime
- cleanup behavior
- failure behavior

Document exact D-Bus interfaces and methods used.

---

## 4.2 Mutter ScreenCast

Determine:

- how a ScreenCast session is created
- how it associates with RemoteDesktop
- how PipeWire streams are exposed
- how virtual monitors are represented
- lifecycle and cleanup requirements
- error conditions

---

## 4.3 RecordVirtual

Determine:

- whether `RecordVirtual` is available on GNOME 50
- required parameters
- supported modes
- cursor modes
- behavior of `is-platform`
- behavior when used with RemoteDesktop
- PipeWire stream lifecycle
- virtual monitor lifecycle
- cleanup behavior
- monitor topology behavior

Treat this as a private/unstable API.

Do not assume compatibility guarantees.

Isolate this functionality behind a dedicated abstraction.

---

# 5. DisplayConfig Research

Determine whether Mutter DisplayConfig can:

1. inspect current physical monitor topology
2. save current configuration
3. temporarily disable physical outputs
4. leave only the virtual monitor active
5. restore the previous physical topology
6. restore monitor arrangement after failure
7. avoid permanently modifying the user's monitor configuration

Determine whether temporary configuration can be used instead of persistent configuration.

Document:

- D-Bus interface
- methods
- parameters
- configuration format
- error conditions
- rollback behavior

---

# 6. GNOME Session Discovery

The PoC must identify:

- active user
- active graphical session
- session type
- Wayland/X11
- session ID
- relevant runtime directory
- relevant D-Bus session bus
- PipeWire availability

Expected example:

```text
User: <user>
Session ID: <session>
Type: wayland
Desktop: GNOME
GNOME version: 50.x
Mutter version: 50.x
PipeWire: available
```

Do not hard-code usernames or session IDs.

---

# 7. First Experiment — Basic Session Discovery

Create a minimal diagnostic program.

It should report:

```text
Remote Console GNOME Diagnostic

Operating system:
Ubuntu 26.04

Desktop:
GNOME 50.x

Session:
Wayland

User:
<detected user>

Session ID:
<detected>

Mutter:
<detected version>

PipeWire:
AVAILABLE

RemoteDesktop:
AVAILABLE / UNAVAILABLE

ScreenCast:
AVAILABLE / UNAVAILABLE

RecordVirtual:
AVAILABLE / UNAVAILABLE

DisplayConfig:
AVAILABLE / UNAVAILABLE

libei:
AVAILABLE / UNAVAILABLE
```

This tool should not modify the system.

---

# 8. Second Experiment — RemoteDesktop

Build the smallest possible experiment that:

1. connects to the relevant D-Bus service
2. creates a RemoteDesktop session
3. starts the session
4. verifies it is operational
5. terminates it
6. verifies cleanup

Do not implement video yet.

Do not implement networking.

Do not implement authentication.

Record:

- method calls
- returned identifiers
- errors
- cleanup behavior

---

# 9. Third Experiment — ScreenCast

Build a minimal ScreenCast PoC.

The PoC should:

1. create a ScreenCast session
2. associate it with the RemoteDesktop session if required
3. request a capture source
4. obtain the PipeWire stream information
5. connect to PipeWire
6. receive frames
7. report frame dimensions/rate
8. terminate cleanly

Initially, saving frames to disk or displaying them in a simple local test application is sufficient.

Do not optimize encoding yet.

---

# 10. Fourth Experiment — Virtual Monitor

Test `RecordVirtual`.

Create a virtual monitor with a known resolution.

Initial test:

```text
1920x1080
60 Hz
```

Confirm:

- virtual monitor is created
- Mutter reports the virtual monitor
- ScreenCast exposes the expected stream
- PipeWire receives frames
- virtual monitor can be destroyed cleanly

Test at least:

```text
1280x720
1920x1080
2560x1440
```

if supported.

Record actual results.

---

# 11. Fifth Experiment — Virtual Monitor as Active Display

Determine whether the virtual monitor can become the active display target for the existing GNOME session.

The PoC must verify:

- desktop is rendered on the virtual monitor
- applications continue running
- existing windows remain
- workspace state remains
- keyboard focus behaves correctly
- the physical monitors can be removed from the active topology

Do not permanently modify the user's display configuration.

---

# 12. Sixth Experiment — Physical Display Isolation

This is a critical experiment.

Start with:

```text
Physical monitor(s)
+
existing GNOME session
```

Then:

1. record current topology
2. create virtual monitor
3. activate virtual monitor
4. temporarily disable physical monitors
5. verify physical monitor no longer exposes the active desktop
6. continue interacting through virtual monitor
7. restore physical topology
8. verify exact previous topology returns

Success criteria:

```text
REMOTE MODE

Physical monitor:
NO ACTIVE DESKTOP OUTPUT

Virtual monitor:
ACTIVE
```

After cleanup:

```text
LOCAL MODE

Physical monitor:
RESTORED

Virtual monitor:
DESTROYED
```

---

# 13. Important Physical Display Caveat

Do not assume that "disabled output" and "physical panel powered off" are identical on every GPU/monitor combination.

The PoC must distinguish:

1. desktop pixels are no longer routed to the physical output
2. physical panel reports no active signal / enters standby
3. physical monitor may still show hardware-generated messages

Document exactly what occurs.

The security requirement is primarily:

> The active desktop must not be visible on the physical console during remote mode.

---

# 14. Seventh Experiment — Remote Input

Test remote input independently of networking.

Create a local test mechanism capable of generating:

- key press
- key release
- pointer movement
- mouse button press
- mouse button release
- scroll

Verify that input reaches the existing GNOME session.

Test:

- terminal
- text editor
- browser
- window movement
- modifier keys
- Ctrl
- Alt
- Shift
- Super
- function keys

Do not provide arbitrary shell execution.

---

# 15. Eighth Experiment — Physical Input Isolation

This is one of the two major architecture gates.

While remote mode is active:

```text
Physical keyboard → MUST NOT affect GNOME session
Physical mouse    → MUST NOT affect GNOME session
```

At the same time:

```text
Remote keyboard → MUST affect GNOME session
Remote mouse    → MUST affect GNOME session
```

Test using an application designed to make input obvious, such as a text editor or terminal.

---

# 16. Input Isolation Research

Investigate all reasonable GNOME-native mechanisms before resorting to privileged device-level interception.

Research:

- Mutter input routing
- libei
- libinput
- uinput
- Wayland seat semantics
- GNOME session lock state
- physical seat/input handling
- remote input authorization

Do NOT immediately implement a global `/dev/input` grab.

If device-level interception is required, document:

- why GNOME-native mechanisms are insufficient
- exact privileges required
- affected devices
- failure behavior
- recovery mechanism
- security implications

---

# 17. Ninth Experiment — GNOME Lock

Determine how to reliably lock the existing GNOME session.

The PoC must:

1. have an existing active session
2. trigger normal GNOME locking
3. verify the session is locked
4. ensure applications remain alive
5. unlock physically
6. verify the same applications/session remain

Do not implement a custom lock screen.

Use GNOME's normal locking mechanism.

---

# 18. Critical Locking Question

Investigate the interaction between:

- GNOME lock
- Mutter RemoteDesktop
- existing GNOME session
- virtual monitor
- remote input

GNOME Remote Desktop's normal remote-assistance behavior may terminate remote access when the screen is locked.

Do not assume this existing behavior satisfies the project's requirements.

The PoC must experimentally determine whether the required:

```text
existing session
+
physical console locked
+
remote virtual display
+
remote control
```

state is possible using the current GNOME architecture.

If it is not possible using the intended APIs, document the blocker before continuing.

---

# 19. Tenth Experiment — Same Session Continuity

Create an unmistakable session state.

For example:

1. open terminal
2. open text editor
3. create a test file in the editor without closing it
4. open browser
5. arrange windows
6. create identifiable workspace state

Then enter remote mode.

From the remote side verify:

- same applications
- same windows
- same workspace
- same user
- same session state

Disconnect remote mode.

Restore physical console.

Unlock locally.

Verify:

- terminal remains
- editor remains
- unsaved state remains if expected
- browser remains
- workspace remains

This is a critical acceptance test.

---

# 20. Eleventh Experiment — Normal Disconnect

Sequence:

```text
LOCAL_ACTIVE
     ↓
REMOTE_PREPARING
     ↓
REMOTE_ACTIVE
     ↓
DISCONNECT
```

Expected:

```text
remote input revoked
        ↓
remote session terminated
        ↓
GNOME locked
        ↓
physical display restored
        ↓
physical input restored
        ↓
LOCKED
```

Do not automatically unlock.

---

# 21. Twelfth Experiment — Abrupt Network Failure

Even before implementing Internet networking, simulate transport failure.

Terminate the local test connection abruptly.

Verify:

```text
remote input stops
remote lease becomes invalid
GNOME locks
physical display restores
physical input restores
```

There must be no manual cleanup required.

---

# 22. Thirteenth Experiment — Main Agent Crash

While in remote mode:

```text
kill -9 <main session agent>
```

Determine what state the system enters.

The goal is:

```text
remote authority eventually revoked
physical console eventually restored
session eventually locked
```

If this cannot happen automatically, document the failure and determine which watchdog/failsafe mechanism is required.

Do not hide the failure with manual intervention.

---

# 23. Fourteenth Experiment — Emergency Controller

Build the smallest possible emergency daemon.

It must:

- run independently of the main remote agent
- listen for the configured physical emergency shortcut
- revoke remote authority
- lock GNOME
- restore physical outputs
- restore physical input
- increment security epoch

Initial shortcut:

```text
Ctrl + Alt + Shift + F12
```

Prefer a two-second hold.

---

# 24. Emergency Test

Put the system into:

```text
REMOTE_ACTIVE
```

Then intentionally make the main remote software unavailable.

Examples:

- kill main daemon
- stop gateway
- break network
- freeze test component if possible

Press:

```text
Ctrl + Alt + Shift + F12
```

The emergency controller must still operate.

Expected:

```text
REMOTE AUTHORITY REVOKED
REMOTE SESSION TERMINATED
SECURITY EPOCH INCREMENTED
GNOME LOCKED
PHYSICAL DISPLAY RESTORED
PHYSICAL INPUT RESTORED
```

The physical user must then unlock GNOME normally.

---

# 25. Security Epoch PoC

Implement a simple prototype security epoch.

Example:

```text
epoch = 1
```

Create a test remote-control lease:

```text
lease.epoch = 1
```

Trigger emergency takeover.

Expected:

```text
epoch = 2
```

The old lease must immediately become invalid.

Test:

```text
old lease
    ↓
input request
    ↓
REJECT
```

This behavior will later become part of production session security.

---

# 26. Fifteenth Experiment — Reconnect

After normal disconnect:

```text
REMOTE
 ↓
LOCKED
```

Reconnect from the remote client.

Verify:

```text
same GNOME session
same applications
new remote lease
new virtual monitor
physical display isolated
physical input isolated
```

The user must not be logged out.

---

# 27. Sixteenth Experiment — Monitor Hotplug

Test:

1. physical monitor connected
2. remote mode active
3. unplug physical monitor
4. reconnect monitor
5. disconnect remote mode

Determine:

- whether Mutter survives
- whether virtual monitor survives
- whether physical topology can be restored
- whether GNOME Shell crashes
- whether display state becomes inconsistent

Repeat with multiple physical monitors.

---

# 28. Seventeenth Experiment — Resolution and Refresh Rates

Test virtual monitor modes:

```text
1280x720 @ 60
1920x1080 @ 60
2560x1440 @ 60
```

If hardware supports it, also test:

```text
1920x1080 @ 120
2560x1440 @ 120
```

Record:

- frame stability
- PipeWire stream behavior
- cursor behavior
- teardown behavior
- GNOME Shell stability

High-refresh virtual-monitor teardown must receive particular attention.

---

# 29. Eighteenth Experiment — Cursor

Verify:

- cursor visible
- cursor moves without desktop damage
- cursor remains responsive
- cursor shape changes correctly where supported
- cursor continues working when the desktop is otherwise static

Test both hardware and software cursor paths where possible.

If disabling hardware cursors is required for reliability, document it as a compatibility workaround rather than silently depending on it.

---

# 30. Nineteenth Experiment — GPU Matrix

Where hardware is available, test:

### Intel

- integrated GPU
- physical monitor
- virtual monitor
- capture
- remote input

### AMD

Same tests.

### NVIDIA

Same tests.

Record:

- driver version
- GPU model
- Wayland status
- Mutter version
- PipeWire version
- virtual monitor behavior
- capture behavior
- cursor behavior
- teardown behavior

---

# 31. Twentieth Experiment — Suspend/Resume

Test:

```text
LOCAL_ACTIVE
→ REMOTE_ACTIVE
→ suspend
→ resume
```

Determine:

- whether remote session survives
- whether virtual monitor survives
- whether PipeWire survives
- whether physical topology is restored
- whether remote lease becomes invalid

Do not assume remote access can survive system suspend.

A safe failure is acceptable.

---

# 32. Twenty-First Experiment — GNOME Session Restart / Failure

Investigate what happens if:

- GNOME Shell restarts
- Mutter becomes unavailable
- session agent crashes
- PipeWire restarts

The project must eventually fail safely.

If complete recovery is not possible, document the safe fallback mechanism.

---

# 33. State Machine Prototype

Implement a small explicit state machine.

Required states:

```text
LOCAL_ACTIVE
LOCKED
REMOTE_AUTHENTICATING
REMOTE_PREPARING
REMOTE_ACTIVE
REMOTE_STOPPING
FAILSAFE
EMERGENCY
RESTORING_PHYSICAL_CONSOLE
```

Transitions must be explicit.

Do not allow arbitrary state mutation from unrelated components.

---

# 34. State Machine Invariants

Enforce at least:

### LOCAL_ACTIVE

```text
physical display active
physical input active
no remote controller
```

### LOCKED

```text
GNOME session locked
no active remote controller unless explicitly transitioning
```

### REMOTE_ACTIVE

```text
remote lease valid
remote input enabled
physical input isolated
physical display isolated
virtual display active
```

### FAILSAFE

```text
remote input disabled
remote lease invalid
session locking/restoration underway
```

### EMERGENCY

```text
remote input disabled
remote sessions revoked
security epoch incremented
```

---

# 35. Cleanup Must Be Transactional

Remote activation involves several resources:

```text
remote session
virtual monitor
PipeWire stream
remote input
physical display topology
physical input policy
control lease
```

If activation fails halfway through, cleanup must happen in reverse order.

Example:

```text
create remote session       ✓
create virtual monitor     ✓
create PipeWire stream     ✓
enable remote input        ✓
disable physical display  ✗
```

The system must not remain in a partially modified state.

It must:

- revoke remote input
- destroy PipeWire stream
- destroy virtual monitor
- restore physical display
- invalidate lease
- lock if required

---

# 36. Recovery Must Be Idempotent

Calling recovery twice should not make the machine less safe.

For example:

```text
restore_physical_console()
restore_physical_console()
```

must be safe.

Likewise:

```text
revoke_remote_session()
revoke_remote_session()
```

must be safe.

Emergency takeover must be idempotent.

---

# 37. Physical Monitor Configuration Backup

Before remote mode, capture:

- physical outputs
- connector names
- modes
- logical monitor layout
- scale
- transform
- primary monitor
- relevant properties

Do not assume connector names remain stable after hotplug.

Use Mutter's current configuration representation where possible.

---

# 38. Temporary Display Configuration

Prefer temporary display configuration.

The remote mode should not permanently modify:

- monitors.xml
- GNOME display preferences
- user display layout

After disconnect/recovery, the user's normal display configuration should be restored.

---

# 39. No Custom Kernel Components

Do not create:

- kernel modules
- custom drivers
- kernel patches

for the PoC.

Use existing Linux/GNOME interfaces.

If a kernel-level limitation is discovered, document it instead of immediately attempting to solve it with a custom kernel component.

---

# 40. No Global Input Grab Without Evidence

Do not implement a global privileged `/dev/input` grab as the first solution.

First determine whether GNOME/Mutter/libei can provide the desired behavior.

If not, evaluate:

- uinput
- libinput interception
- seat-level routing
- minimal privileged helper

The final decision must be documented in the security model.

---

# 41. PoC Code Quality

Although this is a PoC:

- use Rust where practical
- avoid unsafe code unless necessary
- isolate unsafe code
- document unsafe blocks
- use structured logging
- don't log secrets
- don't use arbitrary shell commands
- don't use `sudo` from inside the application
- don't run everything as root

PoC code may be discarded later, but it must not establish unsafe assumptions for the production architecture.

---

# 42. Test Evidence

Each experiment must produce evidence.

Evidence can include:

- command output
- structured logs
- screenshots
- screen recordings
- D-Bus traces
- PipeWire diagnostics
- GNOME/Mutter logs
- test results
- observed state transitions

Do not merely write:

> "This appears to work."

Record what was actually tested.

---

# 43. Experiment Result Format

For each experiment, record:

```text
Experiment:
Date:
Ubuntu version:
GNOME version:
Mutter version:
Kernel:
GPU:
Driver:
PipeWire version:

Objective:

Procedure:

Expected result:

Observed result:

Status:
CONFIRMED / LIKELY / UNVERIFIED / UNSUPPORTED

Issues:

Workarounds:

Security implications:

Follow-up:
```

---

# 44. Mandatory Feasibility Report

Create:

```text
docs/gnome/feasibility-report.md
```

The report must contain:

1. Environment
2. GNOME API findings
3. RemoteDesktop findings
4. ScreenCast findings
5. RecordVirtual findings
6. DisplayConfig findings
7. PipeWire findings
8. libei findings
9. physical input isolation findings
10. GNOME lock findings
11. same-session findings
12. failure/recovery findings
13. emergency-controller findings
14. GPU findings
15. monitor findings
16. known limitations
17. architecture recommendation
18. blockers

---

# 45. Mandatory API Inventory

Create:

```text
docs/gnome/api-inventory.md
```

For every API used, document:

```text
API:
Provider:
GNOME version:
Public/private:
Purpose:
Required permissions:
Lifecycle:
Failure behavior:
Known compatibility issues:
```

Pay particular attention to private Mutter APIs.

---

# 46. Mandatory Security Findings

Create:

```text
docs/security/poc-findings.md
```

Include:

- privilege requirements
- input interception risks
- display manipulation risks
- D-Bus risks
- session-lock risks
- failure risks
- emergency-controller risks
- recovery risks

---

# 47. Critical Acceptance Gates

The PoC is considered successful only if these requirements are demonstrated.

## Gate A — Same session

Existing applications remain in the same GNOME session.

Status must be:

```text
CONFIRMED
```

---

## Gate B — Virtual monitor

A virtual monitor can be created and captured reliably.

Status:

```text
CONFIRMED
```

---

## Gate C — Physical display isolation

The physical display cannot expose the active desktop during remote mode.

Status:

```text
CONFIRMED
```

---

## Gate D — Remote input

Remote keyboard and mouse reliably control the session.

Status:

```text
CONFIRMED
```

---

## Gate E — Physical input isolation

Physical keyboard and mouse cannot control the session during remote mode.

Status:

```text
CONFIRMED
```

This is a hard gate.

---

## Gate F — Fail-safe

Unexpected remote failure eventually produces:

```text
remote authority revoked
GNOME locked
physical display restored
physical input restored
```

Status:

```text
CONFIRMED
```

---

## Gate G — Emergency takeover

Emergency shortcut works independently of the main remote process.

Status:

```text
CONFIRMED
```

This is a hard gate.

---

## Gate H — Same-session recovery

After local unlock:

```text
same GNOME session
same applications
same user state
```

Status:

```text
CONFIRMED
```

---

# 48. Stop Conditions

Stop implementation and report a blocker if:

1. same-session remote control is fundamentally incompatible with GNOME 50's lock/session semantics
2. physical input cannot be isolated safely
3. physical display cannot be reliably isolated
4. remote failure cannot reliably fail closed
5. emergency takeover cannot operate independently
6. restoring display topology is unreliable
7. Mutter crashes or becomes unstable in normal operation
8. the required privileges become unreasonably broad

Do not work around a fundamental blocker by weakening the security requirements.

---

# 49. What Counts as a Successful PoC

A successful PoC should demonstrate:

```text
Existing GNOME session
        |
        v
Create remote virtual monitor
        |
        v
Capture through PipeWire
        |
        v
Inject remote input
        |
        v
Disable physical display
        |
        v
Disable physical input
        |
        v
REMOTE_ACTIVE
        |
        | network/transport failure
        v
Revoke remote authority
        |
        v
Lock GNOME
        |
        v
Restore physical display
        |
        v
Restore physical input
        |
        v
LOCKED
        |
        | physical unlock
        v
Same GNOME session
```

Emergency:

```text
REMOTE_ACTIVE
      |
      | kill main remote process
      v
Press emergency shortcut
      |
      v
Independent recovery
      |
      +-- revoke
      +-- invalidate epoch
      +-- lock
      +-- restore display
      +-- restore input
      |
      v
LOCKED
```

---

# 50. Do Not Optimize Yet

Do not spend time optimizing:

- bitrate
- codec selection
- GPU encoding
- WebRTC congestion control
- network latency
- browser rendering
- bandwidth
- audio
- clipboard

Until the session-control architecture is proven.

Correctness comes first.

---

# 51. Final PoC Deliverables

At the end of this phase, produce:

```text
1. Working GNOME diagnostic utility
2. RemoteDesktop experiment
3. ScreenCast experiment
4. RecordVirtual experiment
5. DisplayConfig experiment
6. PipeWire capture experiment
7. Remote input experiment
8. Physical input isolation experiment
9. Same-session continuity experiment
10. Failure/recovery experiment
11. Emergency controller experiment
12. State machine prototype
13. GNOME API inventory
14. Feasibility report
15. Security findings
16. Hardware compatibility results
17. Explicit architecture recommendation
```

---

# 52. Final Agent Instruction

Start by inspecting the repository and the workflow configuration produced by `adaptive-workflow-configurator`.

Then perform the research and experiments in this document sequentially.

Do NOT jump directly to building the complete Remote Console product.

For each experiment:

1. explain what is being tested
2. identify the relevant GNOME/Wayland API
3. implement the smallest possible test
4. execute it on Ubuntu 26.04/GNOME 50+
5. record the actual result
6. classify the result
7. document limitations
8. only proceed when the result is sufficiently understood

If a requirement cannot be demonstrated, report it as a blocker rather than silently implementing a weaker approximation.

The primary objective of this phase is not code volume.

The primary objective is to establish, with evidence, whether the following architecture is viable:

```text
SAME GNOME SESSION
       |
       +-- physical console
       |
       +-- remote virtual console
                 |
                 +-- PipeWire capture
                 +-- remote input
                 +-- physical display isolation
                 +-- physical input isolation
                 |
                 v
              Browser

Failure:
       |
       v
REVOKE
LOCK
RESTORE
```

Only after all critical acceptance gates are confirmed should the project proceed to the browser/network/authentication implementation phases.