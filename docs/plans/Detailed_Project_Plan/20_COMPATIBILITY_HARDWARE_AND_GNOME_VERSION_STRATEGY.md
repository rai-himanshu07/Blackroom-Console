# 20_COMPATIBILITY_HARDWARE_AND_GNOME_VERSION_STRATEGY.md

## 1. Purpose

This document defines the compatibility strategy for the remote-access system.

The project intentionally starts with a narrow platform target:

- Ubuntu 26.04 LTS
- Ubuntu Desktop
- GNOME 50+
- Wayland
- systemd
- single-user workstation
- physical display and input devices
- supported GPU/display stack

The objective is not to support every Linux desktop environment.

The objective is to provide a **deeply reliable implementation for a clearly defined environment**, while detecting unsupported environments before attempting operations that could leave the workstation in an unsafe state.

This document defines:

- supported platform boundaries
- GNOME/Mutter compatibility
- private API handling
- Ubuntu/kernel/systemd compatibility
- GPU compatibility
- display compatibility
- input-device compatibility
- multi-monitor behavior
- session/environment detection
- capability detection
- version gating
- upgrade strategy
- unsupported-environment behavior
- compatibility testing
- CI/hardware matrices
- safe degradation
- Copilot implementation rules

---

# 2. Compatibility Philosophy

The project must follow this principle:

> **Support fewer environments well rather than many environments unpredictably.**

Do not add compatibility layers merely because another Linux configuration exists.

The initial product is intentionally:

```text
Ubuntu 26.04
+
GNOME 50+
+
Wayland
+
systemd
+
single user
```

Everything outside that environment must be explicitly classified as:

```text
SUPPORTED
SUPPORTED WITH LIMITATIONS
EXPERIMENTAL
UNSUPPORTED
UNKNOWN
```

Never silently treat `UNKNOWN` as `SUPPORTED`.

---

# 3. Hard Platform Boundary

The v1 implementation must explicitly detect:

### Operating system

Expected:

```text
Ubuntu 26.04 LTS
```

Do not assume that:

```text
Debian
Fedora
Arch
Pop!_OS
Linux Mint
Ubuntu derivatives
```

behave identically.

They may be useful for development or future support, but are not automatically supported.

---

### Desktop environment

Required:

```text
GNOME
```

The implementation must not claim support for:

- KDE Plasma
- XFCE
- Cinnamon
- MATE
- LXQt
- sway
- Hyprland
- other wlroots compositors

---

### Display protocol

Required:

```text
Wayland
```

X11 is out of scope for v1.

XWayland applications may still run inside the GNOME session because they are applications within the Wayland desktop.

Do not implement a separate X11 remote-control path.

---

### Session model

Required:

- one active local GNOME user session
- same-session remote control
- no second desktop session

Do not silently switch to headless remote login if the same-session workflow is unavailable.

---

# 4. Supported GNOME Range

The initial supported range should be:

```text
GNOME 50.x
```

Future GNOME releases should initially enter:

```text
EXPERIMENTAL
```

until compatibility validation is completed.

Do not automatically declare:

```text
GNOME 51+
```

supported merely because the APIs appear similar.

---

# 5. Why GNOME Version Gating Is Required

The implementation depends on GNOME/Mutter functionality that may include:

- RemoteDesktop
- ScreenCast
- PipeWire
- DisplayConfig
- virtual monitor creation
- input injection
- session locking
- display topology manipulation

Some of these interfaces may be private or unstable.

Therefore:

```text
GNOME version
!=
compatibility guarantee
```

Compatibility must be validated behaviorally.

---

# 6. Private Mutter API Isolation

Any private or unstable Mutter integration must exist behind a narrow internal abstraction.

Do not scatter private API assumptions throughout the application.

Use a conceptual boundary such as:

```text id="8n1s5d"
GNOME Compatibility Layer
        |
        +-- GNOME 50 backend
        |
        +-- Capability Detection
        |
        +-- Display Backend
        |
        +-- Remote Desktop Backend
```

The rest of the application should consume stable internal operations such as:

```text
create_virtual_display()
destroy_virtual_display()

capture_remote_display()
start_remote_input()
stop_remote_input()

disable_physical_outputs()
restore_physical_outputs()

lock_session()
verify_session_state()
```

The higher-level state machine must not know Mutter-specific implementation details.

---

# 7. No API Guessing

Copilot must never assume that an undocumented or private API behaves in a particular way.

Before using an API:

1. inspect available system interfaces
2. inspect installed versions
3. inspect authoritative documentation/source where available
4. inspect existing working examples
5. create a minimal experiment
6. record observed behavior
7. implement against the observed behavior
8. add regression coverage

If behavior cannot be established reliably:

```text
UNKNOWN
```

is the correct result.

Do not guess.

---

# 8. Capability Detection

At startup and before remote activation, determine capabilities such as:

```text
OS_SUPPORTED
GNOME_SUPPORTED
WAYLAND_SUPPORTED
SYSTEMD_SUPPORTED
SESSION_FOUND
MUTTER_CAPABLE
REMOTE_DESKTOP_CAPABLE
SCREENCAST_CAPABLE
PIPEWIRE_CAPABLE
VIRTUAL_DISPLAY_CAPABLE
DISPLAY_CONFIG_CAPABLE
REMOTE_INPUT_CAPABLE
PHYSICAL_INPUT_ISOLATION_CAPABLE
SESSION_LOCK_CAPABLE
EMERGENCY_CAPABLE
GPU_CAPABLE
```

A capability should only be marked true when it has been verified sufficiently for the current implementation.

---

# 9. Capability vs Version

Prefer:

```text
capability detection
```

over:

```text
version comparison
```

where practical.

For example:

Do not assume:

```text
GNOME >= 50
```

automatically means:

```text
virtual display creation works
```

Instead verify the actual required capability.

Version checks remain useful for known incompatibilities.

Use both:

```text
version gating
+
runtime capability detection
```

---

# 10. Compatibility Result

The host should expose a compatibility result such as:

```text
COMPATIBLE
COMPATIBLE_WITH_LIMITATIONS
EXPERIMENTAL
UNSUPPORTED
UNKNOWN
```

Include reasons.

Example:

```text
Platform:
  Ubuntu 26.04: PASS
  GNOME 50.3: PASS
  Wayland: PASS
  PipeWire: PASS
  Virtual display: PASS
  Remote input: PASS
  Physical input isolation: UNKNOWN
  Emergency controller: PASS

Overall:
  BLOCKED

Reason:
  Physical input isolation has not been verified.
```

The system must not permit remote activation in this state.

---

# 11. Startup Compatibility Check

Perform an initial environment check during installation/setup.

Then perform a lightweight runtime check before remote activation.

The runtime check must account for changes since installation:

- GNOME update
- kernel update
- GPU driver update
- monitor topology change
- PipeWire changes
- session changes
- configuration changes

A machine that was previously compatible can become incompatible.

---

# 12. Compatibility Is a Security Boundary

Compatibility failures must fail closed.

For example:

```text
unknown GNOME behavior
        |
        v
do not activate remote mode
```

not:

```text
unknown GNOME behavior
        |
        v
try anyway
```

This is especially important for:

- display isolation
- physical input isolation
- session locking
- teardown
- emergency recovery

---

# 13. Ubuntu Compatibility

The supported operating-system baseline is:

```text
Ubuntu 26.04 LTS Desktop
```

Record:

- Ubuntu release
- kernel version
- systemd version
- GNOME version
- Mutter version
- PipeWire version
- Mesa version where applicable
- GPU driver version
- GPU model
- display server/session information

These values must be included in diagnostics.

---

# 14. Kernel Compatibility

The system may depend indirectly on:

- input subsystem behavior
- DRM/KMS
- uinput if used
- device permissions
- systemd
- cgroup/resource behavior

Do not establish broad kernel support without testing.

Record the kernel version as part of the compatibility report.

If a kernel update causes a previously verified capability to fail, the system should identify the environment as requiring compatibility review.

---

# 15. systemd Compatibility

The supported environment requires systemd.

Compatibility must include:

- service startup
- service ordering
- watchdog
- restart behavior
- user services where applicable
- system services
- sandboxing
- resource limits
- shutdown
- boot recovery

Do not assume all systemd versions have identical behavior.

The package must not depend on undocumented behavior when a documented mechanism exists.

---

# 16. GPU Strategy

The project must explicitly test representative GPU configurations.

At minimum:

### Intel

Test integrated Intel graphics on supported Ubuntu hardware.

### AMD

Test supported AMD graphics.

### NVIDIA

Test supported NVIDIA configurations, including both:

- proprietary driver
- relevant supported open driver configurations where applicable

Do not assume NVIDIA behaves identically to Mesa-based stacks.

---

# 17. GPU Compatibility Classification

Each tested GPU/driver combination should be classified:

```text
SUPPORTED
SUPPORTED_WITH_LIMITATIONS
EXPERIMENTAL
UNSUPPORTED
```

Record:

- GPU model
- driver
- driver version
- kernel
- GNOME
- Mutter
- PipeWire
- capture method
- virtual display behavior
- cursor behavior
- display restoration behavior
- input behavior
- stability

---

# 18. GPU Failure Is a Safety Event

If the GPU/display stack becomes unreliable during remote operation:

```text
REMOTE_ACTIVE
       |
       v
DEGRADED
       |
       v
REVOKE
       |
       v
LOCK
       |
       v
RECOVER
```

Do not allow a graphics failure to leave remote input authority active.

---

# 19. Hardware Cursor Compatibility

Test cursor behavior explicitly.

Verify:

- pointer movement
- cursor visibility
- cursor updates
- cursor position
- cursor capture
- cursor restoration

If a particular hardware-cursor configuration causes unreliable remote cursor updates, classify it explicitly.

Do not silently apply global workarounds without documenting their impact.

---

# 20. Display Hardware

Test representative:

- HDMI
- DisplayPort
- USB-C display output
- laptop internal panel
- external monitor
- multiple monitors
- mixed resolutions
- mixed refresh rates
- monitor hotplug

The implementation must distinguish:

```text
physical output disabled
```

from:

```text
window visually covered
```

A black fullscreen window is not considered sufficient physical privacy.

---

# 21. Laptop Internal Display

Laptop internal panels require explicit testing.

Test:

- internal panel only
- internal + external monitor
- lid open
- lid close where supported
- monitor hotplug
- display sleep/wake

Do not assume an internal panel behaves exactly like an HDMI/DP output.

---

# 22. Multi-Monitor Strategy

Multi-monitor support must have explicit semantics.

The system should:

1. capture the original topology
2. record physical outputs and modes
3. create the remote virtual display
4. disable physical outputs according to policy
5. expose the intended remote workspace
6. restore the exact previous topology during teardown

Test:

- 1 monitor
- 2 monitors
- 3+ monitors where available
- different resolutions
- different refresh rates
- mixed orientation
- fractional scaling
- monitor ordering
- hotplug

---

# 23. Display Topology Snapshot

Before modifying displays, capture sufficient information to restore:

- connector/output identity
- enabled/disabled state
- resolution
- refresh rate
- position
- scale
- orientation
- primary display
- relevant GNOME display configuration

The snapshot must be treated as transactional state.

Do not rely solely on:

```text
"turn display back on"
```

when the original configuration was more complex.

---

# 24. Display Restoration Compatibility

Test restoration after:

- normal disconnect
- browser close
- network failure
- host daemon crash
- GNOME agent crash
- PipeWire failure
- Mutter failure
- emergency takeover
- monitor hotplug
- service restart
- system reboot

The expected result is not merely:

```text
display exists
```

but:

```text
original topology restored
```

or a documented safe fallback.

---

# 25. Physical Input Hardware

Test:

- USB keyboard
- USB mouse
- Bluetooth keyboard
- Bluetooth mouse
- laptop internal keyboard
- laptop touchpad
- multiple keyboards
- multiple mice
- hotplugged devices

The key requirement is:

```text
REMOTE_ACTIVE
+
physical input
=
physical input cannot control the session
```

---

# 26. Input Device Hotplug

While remote mode is active:

1. unplug keyboard
2. reconnect keyboard
3. unplug mouse
4. reconnect mouse
5. attach an additional input device

Verify that newly appearing devices do not accidentally bypass the physical-input-isolation policy.

This is a critical compatibility test.

---

# 27. Emergency Input Path

The emergency controller must be tested independently against:

- USB keyboard
- Bluetooth keyboard where supported
- multiple keyboards
- input isolation active
- GNOME Shell busy
- main daemon unavailable

The emergency shortcut must remain available.

---

# 28. GNOME Session Variations

The supported environment should explicitly test:

- normal GNOME login
- session locked before remote connection
- session unlocked before remote connection
- screen idle
- screen wake
- display sleep
- application fullscreen
- multiple workspaces
- active XWayland application
- active Wayland application

Do not assume application type changes the remote session semantics.

---

# 29. GNOME Lock Semantics

Lock behavior is a compatibility gate.

Verify experimentally:

```text
remote active
    |
    v
GNOME lock
    |
    v
remote session behavior
```

The implementation must explicitly document whether:

- locking preserves the required same-session remote operation
- locking terminates the remote desktop
- locking changes capture behavior
- locking changes input permissions

If GNOME semantics make the required product behavior impossible on a particular version:

```text
UNSUPPORTED
```

is preferable to an unsafe workaround.

---

# 30. GNOME Shell Restart

Test:

```text
REMOTE_ACTIVE
    |
    v
GNOME Shell/Mutter restart or equivalent failure
```

Expected:

- remote authority is revoked
- session enters recovery
- physical display/input are restored
- session locks
- stale client cannot resume control automatically

---

# 31. GNOME Upgrade Strategy

When GNOME/Mutter changes:

1. detect version change
2. mark compatibility as requiring validation
3. run compatibility checks
4. run automated tests
5. run physical display/input tests
6. run long-running session tests
7. run failure/recovery tests
8. classify new version

Do not automatically broaden the supported version range.

---

# 32. Private API Compatibility Matrix

Maintain an explicit matrix:

| Component | Interface | Stability | Version | Tested | Status |
|---|---|---|---|---|---|
| Mutter | Virtual display | Private/unstable | GNOME 50.x | Yes/No | |
| Mutter | DisplayConfig | Version-dependent | GNOME 50.x | Yes/No | |
| Mutter | RemoteDesktop | Version-dependent | GNOME 50.x | Yes/No | |
| ScreenCast | Virtual capture | Version-dependent | GNOME 50.x | Yes/No | |
| PipeWire | Capture | Stable-ish | Tested version | Yes/No | |
| libei/EIS | Input | Version-dependent | Tested version | Yes/No | |

Populate this from actual experiments rather than assumptions.

---

# 33. Compatibility Backend Design

Keep environment-specific behavior behind compatibility boundaries.

Conceptually:

```text id="h4x0xq"
Application / State Machine
          |
          v
GNOME Compatibility Interface
          |
          +--> Capability Detection
          |
          +--> Display Backend
          |
          +--> Input Backend
          |
          +--> Capture Backend
          |
          +--> Session Backend
```

The state machine should not contain code such as:

```text
if GNOME == 50:
    ...
```

throughout the codebase.

Version-specific behavior belongs in the compatibility layer.

---

# 34. Feature Flags

Use feature flags only when they represent a genuine capability or controlled experimental feature.

Examples:

```text
virtual_display
physical_display_isolation
physical_input_isolation
hardware_cursor_workaround
experimental_gnome_backend
```

Do not use feature flags to permanently hide broken functionality.

Every experimental feature must have:

- owner
- rationale
- compatibility scope
- test coverage
- failure behavior
- removal/upgrade criteria

---

# 35. No Silent Fallbacks

Do not silently fall back from:

```text
physical display isolation
```

to:

```text
black fullscreen window
```

Do not silently fall back from:

```text
same-session remote control
```

to:

```text
headless second session
```

Do not silently fall back from:

```text
physical input isolation
```

to:

```text
best effort
```

Such behavior changes the security/product semantics.

The user must receive an explicit unsupported/blocked state.

---

# 36. Browser Compatibility

The browser client should initially target modern browsers supporting required:

- WebRTC
- WebSocket
- secure contexts
- keyboard/pointer APIs used by the client

Test at minimum:

- Chromium-based browser
- Firefox
- Safari where relevant to future client support

Browser support should be classified independently from host compatibility.

A browser limitation must not cause unsafe host behavior.

---

# 37. Input API Compatibility

Browser keyboard/pointer behavior can differ.

Test:

- normal keys
- modifier keys
- function keys
- navigation keys
- mouse buttons
- wheel
- pointer movement
- focus changes
- browser shortcuts

The browser must clearly distinguish:

```text
browser handled key
```

from:

```text
remote host received key
```

Do not assume every key can be captured.

---

# 38. Unsupported Browser Behavior

If a browser cannot reliably provide a required input capability:

- identify it
- report it
- disable affected feature if necessary
- never weaken host authorization

The host remains authoritative.

---

# 39. Resolution and Scaling Compatibility

Test:

- 100% scaling
- 125%
- 150%
- 200%
- fractional scaling where supported

Verify:

- pointer coordinate mapping
- keyboard input
- application rendering
- remote display dimensions
- cursor location

Coordinate transformations must be tested independently.

---

# 40. Orientation

Test:

- landscape
- portrait
- rotated display
- mixed-orientation multi-monitor topology

Remote coordinate mapping must remain correct.

If a topology cannot be safely represented:

```text
block activation
```

rather than activating with incorrect input coordinates.

---

# 41. High Refresh Rate

Test high-refresh physical monitors.

Pay particular attention to:

- virtual monitor creation
- CRTC changes
- teardown
- restoration
- GNOME/Mutter stability

A high-refresh configuration that causes compositor instability should be classified explicitly.

Do not consider a successful activation alone sufficient.

---

# 42. HDR

HDR should initially be treated as:

```text
compatibility-dependent / non-core
```

unless explicitly implemented and tested.

Do not claim HDR preservation merely because the physical monitor supports HDR.

If remote mode changes HDR state, document the behavior.

---

# 43. Color Management

Color accuracy is not a v1 safety requirement.

The implementation should prioritize:

1. stability
2. privacy
3. input correctness
4. reliable restoration

over perfect color reproduction.

Any color-management changes must not affect the security state machine.

---

# 44. Audio Compatibility

Audio is not a core requirement unless explicitly implemented.

Do not allow audio support to introduce:

- unnecessary privileged access
- additional attack surface
- unbounded resource consumption

If audio is added later, it receives its own capability detection and compatibility tests.

---

# 45. Suspend and Resume

Test:

- suspend before remote session
- suspend during remote session
- resume during remote session
- display wake
- input wake

If suspend causes remote control to become unreliable:

```text
revoke
→ lock
→ restore
```

Do not attempt unsafe continuation.

The system may inhibit suspend while remote mode is active if that is part of the product policy.

---

# 46. Laptop Lid Behavior

Where applicable, test:

- lid close during local mode
- lid close during remote mode
- lid open during remote mode
- external monitor attached

Do not assume lid events are equivalent to monitor hotplug.

Any ambiguous state must result in safe recovery.

---

# 47. Virtualization

Virtual machines may be useful for automated testing, but must not be assumed equivalent to physical hardware.

Test where useful:

- QEMU/KVM
- virtual GPU
- virtual display
- virtual input

Mark virtualization results separately from physical hardware results.

Physical display/input acceptance tests remain mandatory.

---

# 48. Container Environments

The product is not intended to run entirely inside a container.

Do not add container support unless explicitly designed.

The architecture depends on:

- systemd
- GNOME session
- Mutter
- PipeWire
- physical devices
- privileged system services

Containerized CI can test protocol/state-machine components but must not be used as evidence of full host compatibility.

---

# 49. Compatibility Test Matrix

Maintain a matrix including:

### OS

- Ubuntu 26.04

### GNOME

- supported 50.x versions

### GPU

- Intel
- AMD
- NVIDIA

### Display

- internal panel
- HDMI
- DisplayPort
- USB-C
- single monitor
- multiple monitors

### Input

- USB
- Bluetooth
- laptop internal devices
- hotplug

### Browser

- Chromium-based
- Firefox
- additional supported browser

### Network

- LAN
- WAN
- NAT
- TURN
- degraded network

### Power

- normal
- display sleep
- suspend/resume

---

# 50. Compatibility Tiers

Define:

## Tier 1 — Release Supported

Fully tested:

- activation
- remote input
- display isolation
- input isolation
- teardown
- emergency
- recovery
- long-running stability

## Tier 2 — Supported With Limitations

Known limitations are documented and do not violate safety invariants.

## Tier 3 — Experimental

Works in testing but lacks sufficient validation.

Never enable experimental compatibility silently.

## Tier 4 — Unsupported

Known incompatible.

## Tier 5 — Unknown

Insufficient evidence.

Unknown must not automatically be treated as experimental.

---

# 51. Compatibility Regression Testing

Every change involving:

- GNOME integration
- Mutter
- PipeWire
- libei
- DisplayConfig
- virtual monitor
- input isolation
- systemd
- GPU handling

must trigger compatibility testing.

At minimum:

```text
environment check
→ activation
→ remote input
→ display isolation
→ input isolation
→ disconnect
→ restoration
→ emergency
```

---

# 52. GNOME Update Detection

The application should detect changes to important environment components.

At minimum record:

```text
Ubuntu release
kernel
systemd
GNOME Shell
Mutter
PipeWire
Mesa
GPU driver
GPU
```

When a significant component changes, compatibility diagnostics should make it obvious that the machine may need revalidation.

---

# 53. Compatibility Cache

If compatibility results are cached:

- bind them to relevant environment versions
- invalidate when relevant versions change
- never treat old validation as permanent

Example:

```text
validated:
GNOME 50.3
Mutter X
Kernel Y
Driver Z
```

must not automatically imply:

```text
GNOME 51
```

is compatible.

---

# 54. Upgrade Safety

Before enabling remote access after an upgrade:

1. detect changed components
2. run compatibility checks
3. verify safety-critical capabilities
4. if validation fails, keep remote access disabled
5. preserve local recovery
6. inform the user

Never automatically reactivate remote access after an upgrade if compatibility is uncertain.

---

# 55. Emergency Compatibility

The emergency controller must have an even narrower dependency set than normal remote control.

Its operation must not depend on:

- WebRTC
- browser
- gateway
- PipeWire
- normal remote media path

If the GNOME session is unavailable, emergency behavior must still revoke remote authority and perform every remaining safe action possible.

---

# 56. Hardware Failure

Test failures such as:

- monitor disconnect
- GPU reset
- driver restart
- input device removal
- input device reconnect
- PipeWire restart
- GNOME Shell failure

Expected behavior:

```text
detect
→ revoke
→ lock
→ recover
→ verify
```

Never continue remote control merely because one subsystem remains partially functional.

---

# 57. Compatibility and Security

A compatibility workaround must pass the same security requirements as the primary implementation.

Do not approve a workaround merely because:

> "It works on this machine."

The workaround must also preserve:

- authentication
- authorization
- lease validation
- epoch validation
- display privacy
- physical input isolation
- emergency control
- fail-safe recovery

---

# 58. Compatibility Documentation

The project must publish:

- supported Ubuntu version
- supported GNOME range
- tested GPU configurations
- tested display configurations
- browser support
- known limitations
- unsupported environments
- experimental environments
- upgrade guidance

Avoid claiming generic:

> "Linux Wayland support."

The supported environment should be stated precisely.

---

# 59. Copilot Agent Instructions

Before implementing compatibility logic:

1. Inspect the repository.
2. Inspect the workflow/configuration created by `adaptive-workflow-configurator`.
3. Respect the existing workflow.
4. Read Documents 1–19.
5. Identify existing compatibility abstractions.
6. Do not duplicate environment-detection systems.
7. Inspect the actual Ubuntu/GNOME environment.
8. Record actual versions.
9. Verify required capabilities experimentally.
10. Implement capability detection.
11. Isolate GNOME/Mutter-specific code.
12. Add compatibility tests.
13. Add explicit unsupported states.
14. Never silently fall back to unsafe behavior.

---

# 60. Copilot Rules for GNOME/Mutter Changes

When modifying GNOME/Mutter integration:

```text
RESEARCH
    ↓
MINIMAL EXPERIMENT
    ↓
OBSERVE
    ↓
DOCUMENT
    ↓
ABSTRACTION
    ↓
IMPLEMENT
    ↓
TEST
    ↓
FAILURE TEST
    ↓
COMPATIBILITY CLASSIFICATION
```

Do not jump directly from:

```text
"I found an API"
```

to:

```text
"implement production behavior."
```

---

# 61. Copilot Stop Conditions

Stop and report instead of inventing compatibility behavior if:

- required GNOME behavior differs from assumptions
- private API behavior is unclear
- physical input isolation is not proven
- physical display isolation is not proven
- display restoration is unreliable
- session locking behaves differently than required
- GPU behavior is unstable
- a workaround requires broad privileges
- a fallback changes product security semantics
- a version-specific behavior cannot be isolated cleanly
- the compatibility result is unknown

---

# 62. Definition of Done

Compatibility implementation is complete when:

- supported environment is explicitly detected
- unsupported environments are rejected safely
- capabilities are detected
- GNOME/Mutter integration is isolated
- version-specific behavior is isolated
- Ubuntu/systemd/kernel versions are recorded
- GPU configurations are classified
- display configurations are classified
- input configurations are classified
- browser compatibility is documented
- multi-monitor behavior is tested
- display restoration is tested
- input isolation is tested
- emergency behavior is tested
- GNOME update detection exists
- compatibility cache invalidation works if caching is used
- upgrade safety is implemented
- compatibility diagnostics are available

---

# 63. Release Compatibility Gate

A configuration can be marked release-supported only if it passes:

```text
Environment Detection
        ↓
Capability Detection
        ↓
Remote Activation
        ↓
Virtual Display
        ↓
Physical Display Isolation
        ↓
Remote Input
        ↓
Physical Input Isolation
        ↓
Normal Disconnect
        ↓
Failure Recovery
        ↓
Emergency Takeover
        ↓
Display Restoration
        ↓
Input Restoration
        ↓
LOCAL_LOCKED
```

A configuration that passes only streaming/capture tests is not considered fully supported.

---

# 64. Final Compatibility Principle

The product should not ask:

> "Can we make this work somehow?"

It should ask:

> "Can we prove that this environment supports the complete security and recovery contract?"

The correct compatibility model is:

```text
KNOWN GOOD
    |
    v
SUPPORTED

UNKNOWN
    |
    v
DO NOT ASSUME

INCOMPATIBLE
    |
    v
REFUSE SAFELY
```

The system should prefer a clear message such as:

> "This GNOME/Mutter configuration has not been validated for safe physical display/input isolation. Remote access has been disabled."

over silently activating a partially compatible implementation.

The ultimate compatibility requirement is:

> **Every supported environment must preserve the same security invariants, regardless of GPU, display topology, browser, network path, or GNOME/Mutter implementation details.**