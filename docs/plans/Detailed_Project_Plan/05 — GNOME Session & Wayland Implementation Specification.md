# GNOME Session & Wayland Implementation Specification

## 1. Purpose

This document defines the GNOME/Wayland implementation architecture for the Remote Console project.

It covers:

- GNOME session discovery
- Wayland session ownership
- Mutter integration
- GNOME RemoteDesktop
- GNOME ScreenCast
- PipeWire
- virtual monitors
- physical display isolation
- physical input isolation
- libei/EIS
- session locking
- session restoration
- monitor configuration
- cursor handling
- GPU behavior
- suspend/resume
- failure recovery
- GNOME restart behavior
- teardown
- version compatibility

The target platform is intentionally narrow:

```text
Ubuntu 26.04 LTS
GNOME 50+
Wayland
systemd
single-user workstation
```

Do not expand this architecture to KDE, wlroots, X11, or arbitrary Linux distributions until the GNOME implementation is stable.

---

# 2. Core Principle

The project is not implementing a generic Wayland compositor.

It is integrating with the existing GNOME/Mutter session.

The intended architecture is:

```text id="f5j0pd"
Linux boot
   |
   v
GDM
   |
   v
GNOME user session
   |
   +-----------------------+
   |                       |
   v                       v
remote-hostd         GNOME Session Agent
                          |
              +-----------+-----------+
              |           |           |
              v           v           v
           Mutter      PipeWire     libei
```

---

# 3. Existing GNOME Session Is Authoritative

The remote session must attach to the user's existing GNOME session.

Do not create a second normal desktop session for the primary v1 workflow.

The desired behavior is:

```text id="qu8jz4"
Physical user session
        |
        v
GNOME session already running
        |
        v
Remote access attaches to SAME session
```

---

# 4. No Separate Desktop

Do not implement:

```text id="e8q5o1"
Remote login
    |
    v
new GNOME desktop
```

as the primary remote-control mode.

The user must see and control the same session that exists on the workstation.

---

# 5. Important GNOME Constraint

GNOME's existing remote-assistance/session behavior has important locking semantics.

In particular, locking the screen may terminate an ordinary GNOME remote-assistance connection.

Therefore:

> Do not assume that standard GNOME Remote Desktop can directly provide the required "physically locked/isolated console while continuing to remotely control the same session" behavior.

The feasibility PoC must validate the exact behavior on Ubuntu 26.04/GNOME 50 before production implementation.

---

# 6. Implementation Strategy

Use GNOME's native APIs wherever possible.

Potential components:

```text id="zv1n3d"
Mutter
org.gnome.Mutter.RemoteDesktop
org.gnome.Mutter.ScreenCast
org.gnome.Mutter.DisplayConfig
PipeWire
libei / EIS
GNOME Session / logind
```

Do not replace GNOME functionality unnecessarily.

---

# 7. Private API Isolation

Some required Mutter functionality is private or unstable.

This is acceptable for v1 if necessary.

However:

> All private/unstable GNOME integration must be isolated behind a small internal abstraction.

Example:

```text id="6gt7av"
GNOME Backend
      |
      +-- Session
      +-- Display
      +-- Virtual Monitor
      +-- Capture
      +-- Input
      +-- Lock
```

The rest of the application must not directly depend on private Mutter D-Bus details.

---

# 8. GNOME Backend Interface

Define an abstraction similar to:

```text id="u8h7cz"
GnomeBackend

    discover_session()
    get_display_state()
    create_virtual_monitor()
    destroy_virtual_monitor()
    disable_physical_outputs()
    restore_physical_outputs()
    enable_remote_input()
    disable_remote_input()
    start_capture()
    stop_capture()
    lock_session()
    get_cursor_state()
    restore_session()
```

Exact language and API names may differ.

The important design requirement is isolation.

---

# 9. Capability Detection

Do not assume every GNOME 50.x version behaves identically.

At startup, detect:

```text id="z3w2r6"
GNOME version
Mutter version
Wayland
PipeWire
libei
DisplayConfig availability
ScreenCast availability
RemoteDesktop availability
RecordVirtual support
```

Build a capability matrix.

---

# 10. Version Compatibility

Do not implement:

```text id="rrupbv"
GNOME version >= 50
→ assume everything works
```

Instead:

```text id="l6o7pj"
GNOME 50.x
   |
   v
capability detection
   |
   v
supported feature set
```

This is especially important because private Mutter interfaces can change.

---

# 11. Supported Platform Check

Before enabling remote access, verify:

```text id="op6tjo"
Ubuntu 26.04
GNOME
Wayland
systemd
supported Mutter version
```

If unsupported:

```text id="r5w4fo"
Remote Console is not supported on this system.
```

Do not attempt unsafe fallback.

---

# 12. Session Discovery

The GNOME session agent must identify the correct local session.

Use system/session APIs rather than guessing based on:

- process names
- `$DISPLAY`
- hostname
- active terminal
- arbitrary environment variables

Determine:

```text id="9n1x7x"
Linux user
UID
session ID
seat
session type
active state
Wayland display
GNOME/Mutter ownership
```

---

# 13. Session Ownership

For v1:

```text id="lhj33o"
configured remote user
        =
GNOME session owner
```

Do not allow arbitrary cross-user session attachment.

---

# 14. Wayland Detection

The session agent must verify that it is operating inside a Wayland session.

Do not treat XWayland as equivalent to Wayland.

The application should explicitly detect:

```text id="6ntjko"
XDG_SESSION_TYPE=wayland
```

and corroborate with appropriate session/runtime state.

---

# 15. XWayland

XWayland may exist because normal Linux applications can still use it.

This does not change the architecture.

The remote session remains:

```text id="u4y2qi"
GNOME + Mutter + Wayland
```

Do not build X11 capture/input paths in v1.

---

# 16. User Session Agent

The GNOME session agent runs as the logged-in user.

It should not run as root.

Example:

```text id="6s5n4f"
GNOME user session
       |
       v
gnome-session-agent
```

It owns the user-session-level GNOME integration.

---

# 17. System Daemon vs Session Agent

Keep the responsibilities separate.

## remote-hostd

System-level:

- authentication
- authorization
- leases
- security epoch
- remote state
- orchestration
- policy

## gnome-session-agent

User-level:

- Mutter
- PipeWire
- virtual display
- physical display
- libei
- GNOME lock/session operations

---

# 18. Do Not Pass Secrets to GNOME Agent

The GNOME agent does not need:

- Linux password
- TOTP secret
- Remote Access Key
- recovery codes

It should receive only the minimum session-control commands required.

---

# 19. Authenticated IPC

Communication:

```text id="2m2p9w"
remote-hostd
      |
      | authenticated local IPC
      v
gnome-session-agent
```

The GNOME agent should validate that requests come from the expected host daemon.

Do not expose the session agent as an unauthenticated local socket.

---

# 20. Session Agent State

Maintain explicit state.

Example:

```text id="xg0a5e"
SESSION_UNKNOWN
SESSION_READY
PREPARING
VIRTUAL_DISPLAY_READY
PHYSICAL_DISPLAY_ISOLATED
PHYSICAL_INPUT_ISOLATED
REMOTE_READY
REMOTE_ACTIVE
RESTORING
RESTORED
FAILED
```

Do not collapse all of this into one boolean.

---

# 21. Global State Machine

The global state remains:

```text id="c5i5ny"
LOCAL_ACTIVE
     |
     v
PREPARING_REMOTE
     |
     v
REMOTE_ACTIVE
     |
     v
RECOVERING
     |
     v
LOCKED
```

The GNOME agent reports local subsystem state.

`remote-hostd` remains the global security authority.

---

# 22. Mutter RemoteDesktop

Research and use the current GNOME/Mutter RemoteDesktop interfaces.

The implementation should support the required remote-control flow without assuming that the standard desktop UI/portal workflow exactly matches unattended product requirements.

---

# 23. ScreenCast

Use Mutter/ScreenCast and PipeWire for desktop capture where possible.

Potential architecture:

```text id="v3hr42"
Mutter
   |
   v
ScreenCast
   |
   v
PipeWire
   |
   v
Encoder
   |
   v
WebRTC
```

---

# 24. PipeWire

PipeWire is the preferred media transport between GNOME/Mutter and the application.

Do not capture the desktop by:

- screenshots at intervals
- framebuffer polling
- X11 APIs
- `/dev/fb0`
- arbitrary DRM scraping

unless the GNOME PoC proves a required fallback is necessary.

---

# 25. Virtual Monitor

The central display mechanism should be a Mutter virtual monitor.

Investigate:

```text id="3q6ax5"
org.gnome.Mutter.ScreenCast
RecordVirtual
```

The exact interface must be verified against the installed GNOME/Mutter version.

---

# 26. Why Virtual Monitor

The remote desktop should have its own logical display surface.

Conceptually:

```text id="m2i0hk"
Physical monitors
    HDMI-1
    DP-1

          ↓ remote mode

Virtual monitor
    REMOTE-0
```

The virtual monitor becomes the remote desktop target.

---

# 27. Virtual Monitor Creation

When entering remote mode:

1. save current physical display topology
2. determine appropriate remote resolution
3. create virtual monitor
4. validate virtual monitor
5. prepare capture
6. prepare input
7. isolate physical outputs
8. enter remote mode only after validation

---

# 28. Original Display Configuration

Before modifying display state, capture the complete original configuration.

At minimum:

```text id="f8xk9b"
physical output identity
connector
enabled/disabled
mode
resolution
refresh rate
position
scale
rotation
primary output
HDR-related state where relevant
color state where relevant
```

Do not assume restoring only resolution is enough.

---

# 29. Display Configuration Backup

Store the original configuration transactionally.

Example:

```text id="8knk3e"
DisplayBackup

timestamp
session
outputs[]
topology
primary_output
configuration_hash
```

The backup exists only for restoration and crash recovery.

---

# 30. Display Configuration Validation

Before disabling physical outputs, validate that the requested virtual configuration is supported.

Do not modify physical display state first and discover afterward that the virtual monitor cannot be created.

---

# 31. Physical Display Isolation

The preferred strategy is:

```text id="r3h5w7"
Virtual monitor = active remote display

Physical outputs = disabled
```

This is preferable to displaying a black fullscreen window.

---

# 32. Black Window Is Not Sufficient

Do not use:

```text id="j6y8j4"
fullscreen black window
```

as the primary privacy mechanism.

The requirement is to remove the physical display from the active desktop topology where possible.

---

# 33. Physical Output Disable

Use Mutter DisplayConfig or the appropriate GNOME-native mechanism to disable physical outputs.

Example conceptual state:

```text id="b7s2zy"
Before:

HDMI-1 ACTIVE
DP-1 ACTIVE

Remote:

HDMI-1 DISABLED
DP-1 DISABLED
REMOTE-0 ACTIVE
```

The exact implementation must be validated experimentally.

---

# 34. Important Display Caveat

"Disabled" output does not necessarily guarantee that the physical panel's electronics are literally powered down.

The security requirement is:

> The user's desktop contents must not be rendered onto an accessible physical display while remote mode is active.

Validate actual behavior on representative hardware.

Document any hardware-dependent limitations.

---

# 35. Multi-Monitor Host

A host may have:

```text id="j09w7g"
HDMI
DP
DP
```

Remote mode should initially treat the virtual monitor as the sole active display.

All physical outputs should be disabled.

---

# 36. Remote Resolution

Allow configuration such as:

```text id="f1k8ta"
1920x1080
2560x1440
3840x2160
```

but do not assume all resolutions are supported.

Prefer detecting valid modes/capabilities.

---

# 37. Refresh Rate

Remote mode may use:

```text id="3m6bkw"
30 Hz
60 Hz
```

Higher refresh rates should not be required for v1.

Optimize for stable low-latency interaction.

---

# 38. HiDPI

Support scale factors.

Example:

```text id="kq2l54"
1.0
1.25
1.5
2.0
```

The virtual monitor's logical dimensions and physical pixel dimensions must be handled consistently.

---

# 39. Cursor

The cursor must remain visible and responsive remotely.

Investigate GNOME 50 hardware cursor behavior.

If hardware cursor problems prevent reliable remote cursor updates, evaluate the known software-cursor workaround such as:

```text id="g70m2j"
MUTTER_DEBUG_DISABLE_HW_CURSORS=1
```

Do not enable such a workaround globally without testing its performance and stability.

---

# 40. Cursor Requirements

Verify:

- cursor position
- movement
- button state
- cursor visibility
- cursor shape where supported
- no cursor stuck on physical display

---

# 41. Remote Input Architecture

Use GNOME's native input infrastructure where possible.

Preferred path:

```text id="0f5fpi"
Browser
   |
   v
WebRTC
   |
   v
remote-hostd
   |
   v
GNOME session agent
   |
   v
libei / EIS
   |
   v
GNOME
```

The exact process boundary may differ.

---

# 42. libei

Investigate current libei/EIS support on Ubuntu 26.04.

The implementation should use the standard Linux input-emulation mechanism rather than inventing an application-specific compositor injection mechanism.

---

# 43. Remote Input Authorization

The GNOME agent must not accept remote input merely because a client can reach it.

Before processing input:

```text id="plj5x3"
authenticated
AND
authorized
AND
current epoch
AND
valid control lease
AND
REMOTE_ACTIVE
```

must be true.

---

# 44. Physical Input Isolation

This is one of the project's hardest requirements.

When remote mode is active:

```text id="g72w9e"
Physical keyboard → ignored
Physical mouse → ignored
Remote keyboard → accepted
Remote mouse → accepted
```

This must be implemented safely.

---

# 45. Do Not Blindly Grab /dev/input

Do not begin with:

```text id="7b3k0f"
/dev/input/event*
```

global grabs.

This can:

- interfere with system functions
- break emergency recovery
- break other devices
- create privilege problems
- behave poorly with hotplugging

Research the actual GNOME/libinput/seat architecture first.

---

# 46. Physical Input Isolation Research

The PoC must determine the safest mechanism among possibilities such as:

- GNOME/Mutter/libei seat handling
- libinput device routing
- input inhibition
- compositor-supported input isolation
- narrowly scoped privileged helper
- other standard Linux mechanisms

Document evidence for the chosen mechanism.

---

# 47. Input Isolation Requirement

If safe physical input isolation cannot be implemented reliably:

> The project must not claim to satisfy the core product requirement.

Do not substitute a cosmetic UI indication.

---

# 48. Input Device Hotplug

Test:

```text id="6t3z8p"
USB keyboard unplug/replug
USB mouse unplug/replug
Bluetooth mouse
Bluetooth keyboard
USB receiver
```

Physical isolation must remain correct after hotplug.

---

# 49. Emergency Shortcut

The emergency shortcut must be independent of the normal remote input stack.

Candidate:

```text id="bxn5f5"
Ctrl + Alt + Shift + F12
```

held for approximately two seconds.

The actual shortcut should be configurable.

---

# 50. Emergency Input Path

Preferred:

```text id="7o4qyw"
Physical keyboard
       |
       v
remote-emergencyd
       |
       +--> revoke remote authority
       +--> increment security epoch
       +--> terminate remote state
       +--> lock GNOME
       +--> restore display
       +--> restore input
```

It must not depend on the GNOME agent responding normally.

---

# 51. Emergency Daemon Scope

`remote-emergencyd` should be extremely small.

It must NOT implement:

- WebRTC
- browser communication
- video encoding
- authentication UI
- arbitrary D-Bus
- arbitrary shell execution
- file transfer
- networking

---

# 52. Emergency Daemon Privileges

Grant only the privileges necessary for:

- observing the emergency shortcut
- triggering the predefined emergency operation
- coordinating with the host daemon/session agent
- restoring required local state

Do not simply run the daemon with unrestricted root powers without analysis.

---

# 53. Emergency Shortcut Timing

Use a deliberate hold time rather than a single accidental key combination.

Example:

```text id="1lq8o9"
Ctrl+Alt+Shift+F12
       |
       | held 2 sec
       v
Emergency takeover
```

The hold duration should be configurable.

---

# 54. Emergency Sequence

Recommended sequence:

```text id="xk9xq3"
1. Disable remote input
2. Revoke control lease
3. Increment security epoch
4. Terminate remote session
5. Lock GNOME
6. Restore physical display
7. Restore physical input
8. Remain LOCKED
```

Exact low-level ordering may need adjustment based on the PoC.

---

# 55. Emergency Ordering Principle

The most important rule:

> Disable remote input before attempting potentially slow cleanup.

If display restoration takes several seconds, remote input must already be blocked.

---

# 56. GNOME Lock

Use the supported GNOME/session locking mechanism.

Do not simulate locking using:

```text id="em1t9w"
black window
```

or:

```text id="lhf4ly"
fullscreen overlay
```

The session must actually be locked.

---

# 57. Lock Verification

Do not treat:

```text id="9etjks"
lock() returned successfully
```

as proof that the session is locked.

Verify actual session state where possible.

---

# 58. Critical GNOME Behavior

The PoC must explicitly determine:

```text id="3n0k21"
Can GNOME remain the same session
while the physical console is locked/isolated
and the remote client continues controlling that session?
```

If GNOME's normal lock semantics terminate the remote-control infrastructure, the implementation must find a supported/robust alternative or reconsider the architecture.

Do not hide this incompatibility.

---

# 59. Same-Session Continuity

The following must be verified:

```text id="x9p9qv"
Application state remains
GNOME shell remains
User processes remain
Wayland session remains
Remote connection remains
```

during remote operation.

---

# 60. Physical Console Restoration

On disconnect:

```text id="t3n4w5"
Remote virtual monitor
        |
        v
destroy
        |
        v
original physical topology
        |
        v
restore
```

The exact topology must be restored, not merely "enable first monitor."

---

# 61. Restoration Verification

After restoration, verify:

```text id="2j7a7n"
all expected physical outputs restored
correct modes
correct refresh
correct positions
correct scale
correct primary display
```

Where possible, compare against the original configuration snapshot.

---

# 62. Restoration Failure

If restoration fails:

1. retry safely
2. do not enable remote input
3. attempt fallback restoration
4. report diagnostic state
5. remain in a safe locked state where appropriate

Do not silently return to `LOCAL_ACTIVE` if physical state is unknown.

---

# 63. Virtual Monitor Teardown

Destroy the virtual monitor only after remote capture/input have been stopped in the correct order.

The exact teardown sequence must be experimentally validated.

---

# 64. Teardown Ordering

Potential sequence:

```text id="p7j8w0"
revoke remote input
      ↓
stop media
      ↓
stop capture
      ↓
destroy virtual monitor
      ↓
restore physical displays
      ↓
restore physical input
      ↓
lock
```

However, if GNOME requires a different ordering, follow the experimentally proven safe sequence.

---

# 65. GNOME Shell Stability

Test virtual-monitor creation and teardown repeatedly.

Pay particular attention to:

- high refresh rates
- multi-monitor hosts
- NVIDIA
- monitor hotplug
- resolution changes
- repeated connect/disconnect cycles

A teardown path that occasionally crashes GNOME Shell is not acceptable for production.

---

# 66. Idempotent Teardown

The following must be safe if called multiple times:

```text id="1bh1f6"
stop_capture()
destroy_virtual_monitor()
restore_displays()
disable_remote_input()
restore_input()
lock_session()
```

This is essential for crash recovery.

---

# 67. Crash Recovery

The session agent may crash.

The system must not assume cleanup completed.

`remote-hostd` should detect loss of the session agent and trigger safe recovery.

---

# 68. Agent Restart

If the GNOME session agent restarts:

```text id="5s6f1h"
old remote state
      |
      v
invalidate
      |
      v
restore physical console
      |
      v
lock
```

Do not automatically resume control.

---

# 69. Main Daemon Restart

If `remote-hostd` restarts:

- active remote leases must be invalidated or safely reconstructed
- stale sessions must not regain input
- security epoch must remain correct
- GNOME agent must not accept stale authorization

---

# 70. Session Agent Communication Loss

If:

```text id="pyr9p4"
remote-hostd
     X
GNOME agent
```

communication is lost:

```text id="0f9c0n"
remote input disabled
```

and the host must eventually reach a safe state.

---

# 71. PipeWire Failure

If PipeWire capture fails:

```text id="qlh9z1"
video unavailable
```

Remote mode must not remain fully active.

At minimum:

```text id="q3px5b"
remote input revoked
```

and the state machine must initiate safe recovery.

---

# 72. Input Failure

If remote input cannot be confirmed:

Do not enter `REMOTE_ACTIVE`.

A remote session that can view but cannot reliably control may be represented as a separate diagnostic/view state if desired.

---

# 73. Physical Display Failure

If the physical outputs cannot be disabled:

Do not activate remote control.

Example:

```text id="yq2w7r"
virtual monitor ready
physical display still active
        |
        v
REMOTE_ACTIVE = FORBIDDEN
```

---

# 74. Physical Input Failure

If physical input cannot be isolated:

Do not activate remote control.

This is a hard safety gate.

---

# 75. Virtual Monitor Failure

If `RecordVirtual` fails:

```text id="yx5p7n"
abort
restore original display
restore input
lock where appropriate
```

Do not fall back silently to physical display capture.

---

# 76. Monitor Hotplug During Remote Mode

If a physical monitor is connected/disconnected during remote operation:

The system should maintain:

```text id="a5s1o6"
physical outputs isolated
remote virtual monitor active
```

where possible.

If topology changes make this unsafe:

```text id="m8v1h4"
revoke remote input
lock
restore safely
```

---

# 77. Physical Monitor Reappearance

A newly plugged physical monitor must not automatically begin displaying the remote session.

This is a critical privacy requirement.

Test:

```text id="s2q4j1"
REMOTE_ACTIVE
    |
    +--> plug HDMI monitor
```

Expected:

```text id="n3b1pr"
new physical output remains isolated
```

or remote mode safely terminates.

---

# 78. Docking Stations

Test common USB-C/Thunderbolt docking behavior.

Examples:

```text id="a6j3zz"
dock connected
dock disconnected
external monitor connected
external monitor removed
```

The display-isolation policy must remain safe.

---

# 79. GPU Compatibility

Test at minimum where hardware is available:

```text id="9n0r5x"
Intel
AMD
NVIDIA
```

Do not claim universal support based on one GPU.

---

# 80. NVIDIA

NVIDIA requires particular attention to:

- explicit sync
- hardware cursors
- PipeWire
- encoder availability
- virtual outputs
- Mutter stability

Document driver versions used in testing.

---

# 81. GPU Failure

If GPU initialization or capture fails:

```text id="e3x8v0"
no remote activation
```

The local workstation must remain usable.

---

# 82. Software Encoding

A software encoder may be acceptable as fallback if performance is usable.

Do not require a particular GPU encoder for correctness.

---

# 83. Audio

Audio is not required for the first implementation.

Do not allow audio integration to complicate the core display/input safety architecture.

Future audio support should be an independent capability.

---

# 84. Camera / USB Redirection

Not required for v1.

Do not implement until the core remote-console state machine is stable.

---

# 85. File Transfer

Not required.

No filesystem redirection should be introduced through the GNOME agent.

---

# 86. Clipboard

Not required for the first GNOME implementation.

If added later, make it an explicit authorized capability.

---

# 87. Desktop Notifications

The remote user is interacting with the existing GNOME session.

Notifications may continue to exist in the session.

Do not build a second notification system.

---

# 88. Screen Privacy

The physical display isolation mechanism must be tested against:

- lock screen
- notifications
- screen saver
- monitor hotplug
- GNOME Shell restart
- GPU reset
- DPMS/display power changes

---

# 89. Screen Blanking

Screen blanking is not the same as output isolation.

Do not use:

```text id="6ovf3b"
DPMS off
```

as the sole security mechanism.

Use the display-topology approach where possible.

---

# 90. System Suspend

While remote mode is active, inhibit suspend where appropriate.

Use systemd/logind mechanisms.

Do not rely on:

```text id="ayc4by"
desktop power settings
```

alone.

---

# 91. Lid Close

For laptop hosts, lid-close behavior may trigger suspend.

The implementation should either:

- inhibit suspend while remote mode is active, or
- detect suspend and fail safely.

Do not assume laptop lid behavior.

---

# 92. Resume

After resume:

```text id="g2s2f6"
remote lease
```

should not automatically become valid merely because the network reconnects.

Prefer:

```text id="uy0l58"
remote control revoked
session locked
```

followed by a new authenticated session.

---

# 93. GNOME Shell Restart

If GNOME Shell/Mutter restarts or crashes:

```text id="f3f3g5"
remote control revoked
```

The host must fail safe.

Do not attempt to keep sending input through an invalid compositor connection.

---

# 94. GDM

GDM is not the normal remote-session target.

The initial product is designed around an already-running user session.

Do not implement headless remote login as part of this document.

That may become a separate future mode.

---

# 95. Headless GNOME Session

GNOME's headless-session infrastructure may be useful for future functionality.

It is not the primary v1 model.

Do not substitute:

```text id="3r4n5c"
headless GNOME session
```

for:

```text id="8j3x6b"
existing user's session
```

---

# 96. Remote Login

Remote login is explicitly out of scope for v1.

The user should already have a running GNOME session.

---

# 97. Session Lock on Disconnect

Normal disconnect:

```text id="4xqf2k"
remote session ends
      |
      v
GNOME locks
```

The user then performs the normal GNOME unlock.

---

# 98. No Automatic Unlock

The remote application must never automatically unlock the GNOME session after disconnect.

---

# 99. Reconnect

Reconnection should:

1. establish new authenticated network session
2. create new control lease
3. verify current security epoch
4. prepare GNOME remote mode
5. create/restore virtual monitor
6. isolate physical outputs
7. isolate physical input
8. start media
9. activate remote control

Do not reuse stale remote state blindly.

---

# 100. Session Lock State

The system should explicitly track:

```text id="i8ym1p"
LOCAL_ACTIVE
LOCAL_LOCKED
REMOTE_PREPARING
REMOTE_ACTIVE
RECOVERING
```

Do not infer security state from display state alone.

---

# 101. GNOME Agent API

Define narrow commands such as:

```text id="0atq9d"
get_session_state
get_display_state
prepare_remote_display
create_virtual_monitor
start_capture
prepare_remote_input
stop_remote_input
stop_capture
destroy_virtual_monitor
restore_display
lock_session
get_recovery_state
```

Avoid arbitrary D-Bus pass-through.

---

# 102. GNOME Agent Responses

Responses should be structured.

Example:

```text id="2s8q1u"
{
    "operation": "prepare_remote_display",
    "status": "success",
    "virtual_monitor_id": "...",
    "physical_outputs_isolated": true
}
```

The exact protocol may differ.

---

# 103. GNOME Errors

Classify errors.

Example:

```text id="6j2p3m"
SESSION_NOT_FOUND
WAYLAND_UNAVAILABLE
MUTTER_UNSUPPORTED
VIRTUAL_MONITOR_FAILED
DISPLAY_ISOLATION_FAILED
INPUT_ISOLATION_FAILED
PIPEWIRE_FAILED
LOCK_FAILED
RESTORE_FAILED
```

Do not expose raw D-Bus errors directly to users.

---

# 104. Capability Report

The GNOME agent should expose a capability report to `remote-hostd`.

Example:

```text id="84m18z"
GNOME:
50.1

Wayland:
yes

RecordVirtual:
yes

DisplayConfig:
yes

RemoteDesktop:
yes

PipeWire:
yes

libei:
yes

Physical display isolation:
supported

Physical input isolation:
supported
```

The last two must reflect actual validated behavior, not merely API presence.

---

# 105. Capability vs Configuration

Distinguish:

```text id="1j2lqf"
SUPPORTED
```

from:

```text id="t1m5c8"
CONFIGURED
```

and:

```text id="x8z4np"
CURRENTLY_READY
```

---

# 106. Startup Validation

At host service startup, perform a non-destructive validation.

Do not create a virtual monitor automatically.

Check:

- session available
- required APIs available
- required services running
- PipeWire reachable
- input mechanism available
- recovery mechanisms available

---

# 107. Remote Mode Preparation

When requested:

```text id="ny5z3c"
validate prerequisites
      |
      v
snapshot display
      |
      v
create virtual monitor
      |
      v
validate capture
      |
      v
prepare input
      |
      v
disable physical outputs
      |
      v
verify isolation
```

Only then report ready.

---

# 108. Verification

Every critical operation must have a corresponding verification.

Examples:

```text id="q4d3yn"
create_virtual_monitor()
    +
verify_virtual_monitor()

disable_physical_outputs()
    +
verify_physical_outputs_disabled()

enable_remote_input()
    +
verify_remote_input()

lock_session()
    +
verify_locked()
```

---

# 109. No Blind Trust in API Return Values

A successful D-Bus method call is not necessarily proof of the resulting system state.

Query the resulting state where practical.

---

# 110. State Reconciliation

Periodically reconcile expected state against actual GNOME state.

Example:

```text id="v6a1r7"
Expected:
physical outputs disabled

Actual:
DP-1 enabled
```

This is a safety violation.

Immediately revoke remote input and initiate recovery.

---

# 111. Runtime Safety Monitor

The GNOME agent should monitor:

- virtual monitor existence
- physical output state
- remote input state
- PipeWire capture state
- GNOME session state

Unexpected state changes should trigger safe recovery.

---

# 112. Watchdog

The system should have a watchdog relationship:

```text id="5n7j3s"
systemd
   |
   v
remote-hostd
   |
   v
gnome-session-agent
```

If the session agent becomes unhealthy:

```text id="c3h6po"
remote control revoked
```

---

# 113. Recovery Journal

Where necessary, maintain a small recovery state.

Example:

```text id="4h8m5k"
remote_state = PREPARING
display_backup = available
virtual_monitor = created
```

If a crash occurs, startup can determine whether cleanup is required.

Do not persist unnecessary desktop state.

---

# 114. Crash Recovery on Boot

After host reboot:

```text id="2q8z8w"
no remote session
physical outputs normal
physical input normal
GNOME normal
```

Any stale remote-session state must be invalidated.

---

# 115. Security Epoch on Boot

The persisted security epoch must remain valid across reboot.

If the implementation chooses to increment the epoch on every boot, document that behavior.

This can provide an additional stale-session boundary.

---

# 116. Monitor Configuration Recovery

If the system crashes while physical outputs are disabled:

The next startup/session initialization should detect abnormal state where possible and restore the original configuration.

This must be tested rather than assumed.

---

# 117. GNOME Session Restart Recovery

If GNOME/Mutter restarts independently:

```text id="h8j7cp"
old remote state invalid
```

The host should transition to safe state.

---

# 118. Display Configuration Race

Display configuration may change asynchronously.

Protect against:

```text id="j8f8h4"
remote-hostd says:
disable DP-1

user/hotplug changes topology

agent applies stale configuration
```

Always query current state before destructive display operations.

---

# 119. Transactional Display Changes

Treat display changes as transactions:

```text id="g3p0oc"
snapshot
   ↓
validate
   ↓
apply
   ↓
verify
```

If verification fails:

```text id="l4n9c5"
rollback
```

---

# 120. Physical Input Transaction

Similarly:

```text id="7s2t1r"
prepare input isolation
   ↓
verify physical input blocked
   ↓
enable remote input
   ↓
verify remote input
```

Only then enter remote mode.

---

# 121. Remote Mode Entry Gate

`REMOTE_ACTIVE` requires all:

```text id="q5y1p3"
[ ] authenticated
[ ] authorized
[ ] valid security epoch
[ ] valid control lease
[ ] GNOME session confirmed
[ ] virtual monitor confirmed
[ ] capture confirmed
[ ] physical outputs isolated
[ ] physical input isolated
[ ] remote input confirmed
[ ] WebRTC media ready
```

---

# 122. Remote Mode Exit Gate

Leaving `REMOTE_ACTIVE` immediately disables remote control.

Do not wait for media teardown.

---

# 123. Fail-Safe Priority

Priority order:

```text id="1g8o9z"
1. revoke remote input
2. revoke authorization
3. lock session
4. restore physical console
5. clean up remote resources
```

This ordering can be adapted only where GNOME behavior requires it.

---

# 124. Safety Invariant

The most important GNOME invariant:

> If the system cannot prove that remote input is authorized and the physical console is isolated, remote input must be disabled.

---

# 125. Privacy Invariant

Another invariant:

> If remote mode is active, no physical output may knowingly display the remote desktop.

---

# 126. Session Invariant

Another invariant:

> Remote mode must operate inside the configured user's existing GNOME session.

---

# 127. Recovery Invariant

Another invariant:

> Every remote failure must eventually converge to a locally locked, physically restored state.

---

# 128. No X11 Fallback

Do not implement:

```text id="w9xj8r"
Wayland failed
→ X11 remote desktop
```

in v1.

The product's architecture is intentionally Wayland-native.

---

# 129. No KDE Fallback

Do not add KDE support in this implementation.

Create a future backend abstraction if useful, but do not implement it now.

---

# 130. No wlroots Fallback

Likewise, do not implement compositor-specific wlroots support in v1.

---

# 131. GNOME Backend Tests

Unit tests should cover:

- capability detection
- state transitions
- configuration parsing
- display snapshot
- restoration logic
- IPC validation
- error handling

---

# 132. GNOME Integration Tests

Integration tests should cover:

```text id="x1e9d7y"
session discovery
virtual monitor
capture
display isolation
input isolation
lock
disconnect
restore
reconnect
```

---

# 133. Repetition Testing

Repeat:

```text id="p8m3z0"
connect
disconnect
```

at least dozens/hundreds of times during development.

Look for:

- leaked virtual monitors
- stale PipeWire nodes
- GNOME Shell crashes
- display configuration drift
- input state drift
- resource leaks

---

# 134. Stress Testing

Test:

```text id="u3j5b0"
rapid reconnect
network failure
monitor hotplug
resolution changes
GPU load
CPU load
high refresh rate
multiple physical monitors
```

---

# 135. GPU Stress

Run applications that use GPU heavily while remote mode starts/stops.

Verify:

- GNOME remains stable
- remote capture remains stable
- physical display recovery remains reliable

---

# 136. Application Continuity

Test applications such as:

- terminals
- browsers
- IDEs
- Electron applications
- GPU applications
- fullscreen applications

The remote session must control the existing desktop rather than creating a second application environment.

---

# 137. Wayland-Native Input

Verify that remote keyboard/mouse events work correctly with:

- native Wayland applications
- XWayland applications
- GNOME Shell

Do not test only one application.

---

# 138. Keyboard Layout

Respect the host's current keyboard layout.

Test:

```text id="1n4m5v"
US
UK
Indian layouts where relevant
dead keys
Compose
AltGr
```

Remote key events must not blindly assume US layout.

---

# 139. Modifier Keys

Test:

```text id="w2l7v0"
Ctrl
Alt
Shift
Super
AltGr
Caps Lock
Num Lock
```

Ensure stuck modifier state cannot persist after disconnect.

---

# 140. Input Cleanup

On any remote failure:

```text id="g8m5b2"
release held keys
release buttons
disable remote input
```

Do not leave:

```text id="s1t7q9"
Ctrl = DOWN
```

after the remote client disappears.

---

# 141. Pointer Cleanup

Similarly ensure:

- no stuck mouse buttons
- no runaway pointer movement
- no stale scroll state

---

# 142. Remote Input Flood

If a malicious or malfunctioning client sends excessive events:

- rate-limit
- revoke lease if necessary
- fail safe

---

# 143. GNOME Agent Security

The session agent must reject:

- malformed IPC
- unknown commands
- commands from unauthorized processes
- commands that violate current state
- arbitrary D-Bus method forwarding

---

# 144. D-Bus Isolation

Do not expose a generic proxy such as:

```text id="m1q2v8"
call_dbus(service, path, method, args)
```

This would effectively turn the agent into a privileged GNOME control bridge.

Use explicit methods.

---

# 145. Environment Security

Do not blindly trust environment variables supplied by external processes.

The session agent should verify its actual GNOME/Wayland session context.

---

# 146. File Permissions

Configuration/state files should use restrictive permissions.

Do not place security-sensitive state in world-readable locations.

---

# 147. Systemd User Service

The GNOME session agent may be managed as a user-level systemd service where appropriate.

Evaluate:

```text id="j3k6p7"
systemd --user
```

integration.

---

# 148. Service Startup

The agent should start only after the relevant GNOME session is ready.

Do not assume that system boot and GNOME readiness are equivalent.

---

# 149. Service Shutdown

On session logout:

```text id="q5b7k9"
remote control must already be invalid
```

Any active remote session must be terminated safely.

---

# 150. User Logout

Do not allow a remote session to survive the logout of the configured GNOME user.

Expected:

```text id="p2k7v4"
logout
  ↓
remote lease invalid
  ↓
remote input disabled
```

---

# 151. Session Lock Without Remote Mode

Normal local GNOME locking should remain normal.

The application must not modify ordinary local lock behavior unnecessarily.

---

# 152. Remote Mode and Local Lock

This is the critical special case.

The implementation must experimentally validate how GNOME's locking model interacts with the remote-control mechanism.

Do not simply assume that:

```text id="t9r0x3"
lock()
```

can coexist with:

```text id="m5h4y8"
RemoteDesktop control
```

indefinitely.

---

# 153. Feasibility Gate

The GNOME implementation cannot be considered viable until the following are experimentally demonstrated:

```text id="f3w1n8"
A. Same GNOME session
B. Virtual monitor
C. Physical display isolation
D. Remote input
E. Physical input isolation
F. Fail-safe disconnect
G. Independent emergency takeover
H. Session restoration
```

---

# 154. Hard Stop Conditions

Stop implementation and report the blocker if:

- same-session remote control cannot survive required state transitions
- physical display cannot be reliably isolated
- physical input cannot be reliably isolated
- emergency takeover depends on the failing remote stack
- GNOME crashes during normal teardown
- display restoration is unreliable
- required privileges become excessive
- the architecture requires unsafe global input grabbing
- the system cannot fail closed

Do not work around these by weakening requirements without explicit approval.

---

# 155. PoC Evidence

For each major capability, capture evidence:

```text id="5q4x6j"
GNOME version
Mutter version
kernel
GPU
driver
PipeWire version
libei version
configuration
steps
result
failure mode
logs
```

Screenshots/videos may be useful for development evidence, but do not include sensitive desktop content in public reports.

---

# 156. Hardware Matrix

Record testing against:

```text id="z4r9j2"
Intel GPU
AMD GPU
NVIDIA GPU
single monitor
multi-monitor
HDMI
DisplayPort
USB-C dock
HiDPI
60 Hz
high refresh rate
```

Where hardware is unavailable, explicitly mark the matrix as untested.

---

# 157. GNOME Version Matrix

At minimum, document tested:

```text id="x9j8s7"
GNOME 50.x
Mutter 50.x
Ubuntu 26.04
```

Do not advertise compatibility with versions that have not been tested.

---

# 158. Private API Compatibility

If a private Mutter API changes:

The expected response is:

```text id="x7n2c8"
capability detection
      |
      v
unsupported
      |
      v
safe failure
```

not:

```text id="a6b4z9"
guess new API behavior
```

---

# 159. GNOME Backend Versioning

Where necessary, implement:

```text id="n7v5s2"
MutterBackendV50
MutterBackendV51
...
```

rather than scattering version checks throughout the entire application.

Do this only when actual incompatibilities are demonstrated.

---

# 160. Logging

Useful logs:

```text id="q1z4b6"
GNOME session discovered
Mutter capability detected
virtual monitor created
physical outputs disabled
input isolation enabled
PipeWire stream started
remote state active
display restored
```

Never log:

- keyboard contents
- clipboard
- screen contents
- credentials

---

# 161. Diagnostics

Expose safe diagnostic information:

```text id="e4r7k1"
GNOME version
Mutter version
Wayland
PipeWire
GPU
driver
virtual monitor support
display isolation support
input isolation support
```

Do not expose sensitive session internals unnecessarily.

---

# 162. Production Architecture

The intended final architecture is:

```text id="g5m1q7"
                     remote-hostd
                           |
                    authenticated IPC
                           |
                           v
                +----------------------+
                | GNOME Session Agent  |
                | USER PRIVILEGE       |
                +----------+-----------+
                           |
             +-------------+-------------+
             |             |             |
             v             v             v
          Mutter       PipeWire       libei
             |             |             |
             v             v             v
       Virtual Monitor   Capture      Input
             |
             v
      Physical Output
        Isolation
```

And independently:

```text id="s3w9x1"
Physical Keyboard
       |
       v
remote-emergencyd
       |
       v
Emergency Recovery
```

---

# 163. Final GNOME State Model

```text id="4h2m7q"
LOCAL_ACTIVE
     |
     | authenticated + authorized
     v
PREPARING_REMOTE
     |
     +--> virtual monitor
     |
     +--> capture
     |
     +--> display isolation
     |
     +--> input isolation
     |
     v
REMOTE_ACTIVE
     |
     | disconnect/failure/emergency
     v
FAIL_SAFE
     |
     +--> revoke input
     +--> lock
     +--> restore display
     +--> restore input
     |
     v
LOCKED
```

---

# 164. Final GNOME Invariants

The implementation must preserve:

```text id="f9z3w6"
1. Existing GNOME session is reused.
2. No second normal desktop session is created.
3. Remote input requires valid authorization.
4. Physical input is isolated during remote control.
5. Physical displays are isolated during remote control.
6. Failure revokes remote input.
7. Disconnect locks the GNOME session.
8. Physical display configuration is restored.
9. Physical input is restored.
10. Emergency takeover is independent of the main remote path.
11. Stale remote sessions cannot regain control.
12. GNOME/private APIs are isolated behind a backend.
```

---

# 165. Final Copilot Agent Instruction

Implement the GNOME/Wayland integration only after reviewing:

```text id="x8n5k1"
01_PROJECT_MASTER_PLAN.md
02_GNOME_WAYLAND_FEASIBILITY_POC.md
03_SECURITY_AND_AUTHENTICATION.md
04_NETWORKING_AND_BROWSER_CLIENT.md
```

Also inspect the workflow configured by `adaptive-workflow-configurator` before making repository changes.

Then:

1. identify the current Ubuntu 26.04/GNOME 50 APIs
2. verify assumptions experimentally
3. isolate private Mutter APIs
4. implement capability detection
5. implement session discovery
6. implement virtual monitor support
7. implement display snapshot/restore
8. implement physical display isolation
9. implement PipeWire capture
10. implement libei/EIS input
11. implement physical input isolation
12. implement GNOME lock
13. implement teardown
14. implement failure recovery
15. implement emergency integration
16. write integration tests
17. document unsupported hardware/configurations

Do not jump directly to the complete remote desktop.

Build and validate each capability independently.

Most importantly:

> **Do not weaken the physical display/input isolation requirements merely because GNOME makes them difficult.**

If the GNOME platform prevents a required property, stop and document the exact limitation.

The purpose of this implementation is not to make a demo that happens to work on one machine.

It is to establish whether a reliable, fail-closed remote console can be built on Ubuntu 26.04 + GNOME 50+ while preserving the user's existing session.

The final implementation must prefer:

```text id="x6q8p4"
GNOME-native mechanism
        >
well-contained privileged helper
        >
unsupported workaround
```

and:

```text id="h4w9k2"
safe failure
        >
ambiguous success
```

at every security boundary.