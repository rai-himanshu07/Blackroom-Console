# 12 — TESTING STRATEGY & TEST MATRIX

## 1. Purpose

This document defines the testing strategy for the project.

The project has unusually strong safety requirements because it controls:

- remote keyboard and pointer input
- the physical display state
- an existing GNOME session
- authentication and authorization
- privileged system services
- recovery after network, process, GNOME, GPU, and hardware failures

Testing must therefore validate not only whether remote desktop functionality works, but whether the system **fails safely**.

The primary testing principle is:

> A feature is not complete because the happy path works. It is complete only when expected failures, races, restarts, disconnects, and recovery paths preserve the project's security invariants.

Do not optimize testing around code coverage alone.

Functional correctness, safety invariants, fault recovery, security boundaries, GNOME compatibility, and real hardware behavior are more important than arbitrary coverage percentages.

---

# 2. Testing Principles

The test strategy follows these principles:

1. Test safety invariants before convenience features.
2. Test failure paths as first-class behavior.
3. Test real GNOME/Wayland behavior, not only mocks.
4. Keep unit tests deterministic and fast.
5. Use integration tests for IPC and service interactions.
6. Use system tests for actual GNOME/Mutter/PipeWire/libei behavior.
7. Use fault injection deliberately.
8. Test concurrent events and race conditions.
9. Repeat activation/teardown cycles many times.
10. Test recovery after process crashes.
11. Test recovery after system restart.
12. Test physical display privacy manually on real hardware.
13. Test physical input isolation manually and, where possible, programmatically.
14. Treat security regressions as release blockers.
15. Do not mark a test passed based only on logs.
16. Prefer observable evidence over assumptions.

---

# 3. Testing Layers

The project should use the following testing layers.

```text
                         RELEASE / ACCEPTANCE
                                |
                    +-----------+-----------+
                    |                       |
             HARDWARE / GPU          SECURITY TESTS
                    |                       |
             SYSTEM TESTS             FAULT INJECTION
                    |                       |
             INTEGRATION TESTS     CONCURRENCY / RACE TESTS
                    |                       |
                    +-----------+-----------+
                                |
                         UNIT TESTS
```

Each layer has a different purpose.

---

# 4. Unit Tests

Unit tests should validate deterministic logic independently of GNOME and hardware.

Important unit-test targets:

- state machine
- state transitions
- transition validation
- rollback logic
- idempotent cleanup
- security epoch handling
- control lease validation
- lease expiration
- session credential validation
- authentication decisions
- trusted-device authorization
- Remote Access Key verification
- TOTP validation
- recovery-code validation
- rate limiting
- replay protection
- session revocation
- capability checks
- configuration parsing
- configuration validation
- monitor snapshot/restore logic
- event classification
- failure classification
- retry/backoff logic
- connection-health logic
- timeout handling
- authorization decisions
- protocol message validation
- IPC request validation

Unit tests must not require:

- a running GNOME session
- a physical monitor
- PipeWire
- Mutter
- real `/dev/input`
- Internet access
- a browser

---

# 5. State Machine Unit Tests

The state machine is safety-critical and must have extensive deterministic testing.

At minimum, test every valid transition.

Example:

```text
LOCAL_ACTIVE
    -> LOCAL_LOCKED

LOCAL_LOCKED
    -> AUTHENTICATING

AUTHENTICATING
    -> AUTHENTICATED
    -> LOCAL_LOCKED

AUTHENTICATED
    -> PREPARING_REMOTE

PREPARING_REMOTE
    -> REMOTE_ACTIVE
    -> RECOVERING
    -> FAILED_SAFE

REMOTE_ACTIVE
    -> REMOTE_DEGRADED
    -> TEARING_DOWN
    -> EMERGENCY
    -> RECOVERING

REMOTE_DEGRADED
    -> REMOTE_ACTIVE
    -> TEARING_DOWN
    -> RECOVERING
    -> EMERGENCY

TEARING_DOWN
    -> LOCAL_LOCKED
    -> RECOVERING
    -> FAILED_SAFE

RECOVERING
    -> LOCAL_LOCKED
    -> FAILED_SAFE

EMERGENCY
    -> LOCAL_LOCKED
    -> FAILED_SAFE
```

For each transition test:

- valid input event
- invalid input event
- missing prerequisite
- duplicate event
- event arriving too late
- event arriving during another transition
- failure halfway through transition
- retry
- process restart
- stale event
- stale transition ID

The state machine must reject illegal transitions rather than attempting to improvise.

---

# 6. State Machine Invariant Tests

The following invariants are mandatory.

## Invariant 1 — No valid lease means no remote input

Test:

```text
lease valid
    -> remote input accepted

lease expires
    -> remote input rejected

lease revoked
    -> remote input rejected

security epoch changes
    -> old lease rejected

session terminates
    -> remote input rejected
```

---

## Invariant 2 — Emergency takeover revokes remote authority

Test:

```text
REMOTE_ACTIVE
    -> emergency trigger
    -> remote input revoked
    -> session terminated
    -> security epoch incremented
    -> GNOME locked
    -> physical display restored
    -> physical input restored
    -> LOCAL_LOCKED
```

---

## Invariant 3 — Network loss fails closed

Test:

```text
REMOTE_ACTIVE
    -> network disappears
    -> control lease eventually expires/revokes
    -> remote input disabled
    -> GNOME locked
    -> physical display restored
    -> physical input restored
    -> LOCAL_LOCKED
```

There must not be a state where:

```text
network disconnected
+
remote input still active indefinitely
```

---

## Invariant 4 — Main daemon crash cannot leave remote control permanently active

Kill:

```text
remote-hostd
```

while remote access is active.

Verify:

- remote authority expires/revokes
- session terminates safely
- GNOME locks
- display is restored
- local input is restored
- stale credentials cannot resume control

---

## Invariant 5 — Emergency daemon must remain useful when the main stack fails

While remote access is active, deliberately break:

- gateway
- host daemon
- GNOME agent
- WebRTC
- PipeWire

Then trigger the emergency mechanism.

The emergency mechanism must still provide the intended local recovery path wherever technically possible.

---

# 7. Authentication Tests

Authentication must be tested independently from remote desktop functionality.

## Linux password

Test:

- correct password
- incorrect password
- empty password
- repeated failures
- account locked/disabled
- PAM failure
- PAM timeout/error
- password change while service is running

---

## TOTP

Test:

- valid code
- invalid code
- expired code
- replayed code
- clock skew within accepted tolerance
- excessive clock skew
- repeated invalid attempts
- rate limiting
- TOTP secret unavailable
- TOTP configuration disabled
- authenticator reconfiguration

TOTP must remain mandatory for trusted devices.

---

## Remote Access Key

Test:

- valid key
- invalid key
- malformed key
- revoked key
- rotated key
- old key after rotation
- repeated failed attempts
- key leakage through logs
- key leakage through browser storage
- key leakage through URLs
- key leakage through error messages

---

## Trusted Devices

Test:

- register device
- authenticate trusted device
- revoke one device
- revoke all devices
- rotate device credential
- expired device credential
- malformed device credential
- copied credential on another browser
- credential after security epoch increment

Trusted-device status must never bypass TOTP.

---

## Authentication Flow Matrix

| Scenario | Password | TOTP | Remote Access Key | Trusted Credential | Expected |
|---|---:|---:|---:|---:|---|
| New device | Yes | Yes | Yes | No | Allow |
| Trusted device | Yes | Yes | No | Yes | Allow |
| Wrong password | No | Yes | Yes | No | Deny |
| Wrong TOTP | Yes | No | Yes | No | Deny |
| New device without Access Key | Yes | Yes | No | No | Deny |
| Trusted device without TOTP | Yes | No | No | Yes | Deny |
| Revoked trusted device | Yes | Yes | No | Revoked | Deny |
| Revoked Access Key | Yes | Yes | Revoked | No | Deny |
| Lost authenticator recovery | Yes | Recovery | Yes | No | Allow if recovery policy permits |
| Revoked all sessions | Yes | Yes | Yes | Yes | Existing sessions terminated |

---

# 8. Session Credential Tests

Authentication credentials and remote-session credentials must be tested separately.

Verify:

- authentication succeeds
- short-lived session credential is issued
- session credential has an expiry
- session credential cannot be reused after expiry
- session credential cannot be used for another host
- session credential cannot be used by another user
- session credential cannot bypass authorization
- session credential is revoked on logout
- session credential is revoked on emergency
- session credential is invalid after security epoch change

---

# 9. Control Lease Tests

Test:

- lease creation
- lease renewal
- lease expiry
- renewal failure
- renewal after session termination
- renewal after security epoch change
- lease from wrong session
- lease from wrong client
- lease from wrong host
- concurrent leases
- duplicate leases
- stale lease
- malformed lease
- clock issues
- daemon restart

The safest default behavior is:

```text
uncertain lease validity
        ↓
      DENY
```

Never:

```text
uncertain lease validity
        ↓
      ALLOW
```

---

# 10. Security Epoch Tests

The security epoch is a major revocation mechanism.

Test:

1. Start remote session.
2. Record current epoch.
3. Trigger emergency.
4. Verify epoch increments.
5. Attempt to reuse old session credentials.
6. Attempt to reuse old control lease.
7. Attempt reconnect from old browser state.
8. Verify all are rejected.

Also test epoch changes caused by:

- emergency
- revoke-all
- service restart
- explicit security reset
- credential rotation where applicable

---

# 11. GNOME Session Integration Tests

These tests require an actual supported Ubuntu/GNOME environment.

Test:

- session discovery
- session ownership
- Wayland environment discovery
- correct user identification
- correct DBus session
- correct runtime directory
- Mutter availability
- ScreenCast availability
- RemoteDesktop availability
- PipeWire availability
- libei/EIS availability
- capability detection
- unsupported environment detection

The system must fail clearly if the environment is unsupported.

---

# 12. Virtual Monitor Tests

Test:

- create virtual monitor
- capture virtual monitor
- set supported resolution
- set unsupported resolution
- set refresh rate
- remove virtual monitor
- recreate virtual monitor
- create repeatedly
- multiple virtual monitor requests
- virtual monitor after physical hotplug
- virtual monitor after GNOME restart
- virtual monitor after PipeWire restart

Verify:

- correct dimensions
- correct orientation
- correct cursor behavior
- correct frame delivery
- no unintended physical output activation

---

# 13. Physical Display Isolation Tests

This is a critical security test category.

For each supported hardware configuration:

1. Record original display topology.
2. Activate remote session.
3. Create virtual output.
4. Disable physical outputs.
5. Verify physical outputs are no longer active.
6. Verify remote output remains functional.
7. Disconnect.
8. Lock session.
9. Restore original topology.
10. Verify physical display returns.

Test with:

- one monitor
- multiple monitors
- different resolutions
- mixed refresh rates
- HDMI
- DisplayPort
- USB-C display
- monitor unplug/replug
- monitor power cycle
- monitor hotplug during remote session

Do not treat a fullscreen black window as equivalent to output isolation.

---

# 14. Physical Input Isolation Tests

This is one of the highest-priority hardware test areas.

Test:

```text
REMOTE_ACTIVE
```

and physically attempt:

- keyboard typing
- mouse movement
- mouse clicks
- modifier keys
- function keys
- media keys where relevant
- hotkeys
- repeated key presses
- mouse buttons
- touchpad if applicable

Expected:

```text
physical input -> ignored by GNOME session
remote input   -> accepted
```

Then:

```text
REMOTE_ACTIVE
    -> disconnect
```

Expected:

```text
physical input -> restored
remote input   -> rejected
```

Test for input leakage during:

- activation
- teardown
- network failure
- daemon restart
- GNOME restart
- emergency takeover

Input isolation must not accidentally block the emergency mechanism.

---

# 15. Emergency Controller Tests

Test the emergency path independently.

## Normal emergency test

```text
REMOTE_ACTIVE
    -> hold emergency shortcut
    -> emergency action begins
    -> remote authority revoked
    -> remote session terminated
    -> epoch incremented
    -> session locked
    -> physical outputs restored
    -> physical input restored
    -> LOCAL_LOCKED
```

## Main daemon failure

Kill `remote-hostd`.

Then trigger emergency.

Expected:

- emergency remains operational
- local recovery still occurs wherever technically possible

## Gateway failure

Disconnect network completely.

Trigger emergency.

Expected:

- emergency does not depend on network connectivity

## GNOME agent failure

Kill the GNOME agent.

Trigger emergency.

Expected:

- emergency follows its independent recovery path
- system reaches safest achievable state

## Accidental activation

Test:

- short key press
- partial sequence
- wrong sequence
- sequence while not remotely active

Expected:

- no unintended disruptive behavior

---

# 16. Lock Semantics Tests

GNOME lock behavior is a critical compatibility issue.

Test experimentally:

```text
LOCAL_ACTIVE
    -> activate remote
    -> lock GNOME
```

Determine exactly what happens to:

- Mutter RemoteDesktop
- ScreenCast
- PipeWire
- virtual monitor
- remote input
- same-session continuity

The project must not assume that GNOME lock semantics behave like a traditional desktop environment.

Document actual behavior on every supported GNOME release.

If locking necessarily destroys the remote session, the design must explicitly account for that behavior rather than hiding the problem.

---

# 17. Same-Session Continuity Tests

Verify that the remote session uses the existing GNOME session.

Test:

1. Login locally.
2. Start normal GNOME session.
3. Activate remote access.
4. Interact remotely.
5. Disconnect.
6. Restore local display.
7. Unlock locally.
8. Verify the same session remains intact.

Check:

- running applications
- environment
- user identity
- desktop state
- application windows
- background services
- session processes

The system must not silently create a second desktop session.

---

# 18. Teardown Tests

Teardown must be tested from every relevant state.

Test disconnect from:

- REMOTE_ACTIVE
- REMOTE_DEGRADED
- PREPARING_REMOTE
- media-ready-but-not-active
- input-isolated state
- display-isolated state
- partial activation

Verify:

```text
remote authority revoked
        ↓
session locked
        ↓
physical display restored
        ↓
physical input restored
        ↓
LOCAL_LOCKED
```

Teardown must be:

- idempotent
- transactional where practical
- safe when partially completed
- safe when called twice
- safe after process restart

---

# 19. Failure Injection Tests

The project must deliberately kill or break components during every critical lifecycle stage.

Inject failures into:

- `remote-gateway`
- `remote-hostd`
- `gnome-session-agent`
- `remote-emergencyd`
- PipeWire
- Mutter/GNOME Shell where practical
- WebRTC connection
- network interface
- DNS
- STUN
- TURN
- rendezvous service
- IPC channel

Examples:

```text
kill -9 remote-hostd
kill -9 gnome-session-agent
kill -9 remote-gateway
```

Do not assume a graceful shutdown is representative of real failures.

---

# 20. Activation Failure Matrix

Inject failure after each activation step.

Example:

```text
AUTHENTICATED
    ↓
lease created
    ↓
virtual monitor created
    ↓
physical outputs disabled
    ↓
physical input isolated
    ↓
PipeWire ready
    ↓
WebRTC ready
    ↓
REMOTE_ACTIVE
```

For each point, terminate the responsible component.

Expected result:

```text
rollback
    ↓
remote authority revoked
    ↓
session locked
    ↓
physical display restored
    ↓
physical input restored
    ↓
LOCAL_LOCKED
```

There must be no permanent half-configured state.

---

# 21. Network Failure Tests

Test:

- Ethernet unplug
- Wi-Fi disconnect
- temporary packet loss
- high latency
- packet reordering
- DNS failure
- gateway unavailable
- TURN unavailable
- STUN unavailable
- NAT rebinding
- IP address change
- IPv4 loss
- IPv6 loss
- browser network suspension
- laptop sleep
- browser tab backgrounding

Verify control lease and fail-safe behavior.

---

# 22. Reconnect Tests

Test:

```text
REMOTE_ACTIVE
    -> network interruption
    -> REMOTE_DEGRADED
    -> connection restored
```

Determine whether reconnect is allowed.

If reconnect is allowed:

- authenticate/reauthorize as required
- validate session credential
- validate security epoch
- validate control lease
- verify GNOME state
- verify display state
- verify input state

Never reconnect using stale authority without validation.

---

# 23. Browser Failure Tests

Test:

- browser refresh
- browser tab close
- browser crash
- browser process kill
- laptop sleep
- laptop shutdown
- browser network suspension
- duplicate browser tab
- two simultaneous clients
- stale browser tab
- browser opened with stale session

Expected behavior must preserve:

```text
No valid current authorization
        ↓
No remote input
```

---

# 24. Multiple Client Tests

Test simultaneous:

- two trusted clients
- two new clients
- trusted + new client
- same user from two browsers
- duplicate connection from same browser

The authorization policy must be explicit.

If only one remote controller is permitted:

```text
client A active
client B requests control
        ↓
client B denied or requires explicit takeover
```

Never allow ambiguous simultaneous keyboard/pointer control.

---

# 25. Race Condition Tests

Race testing is mandatory.

Examples:

### Disconnect + emergency

```text
network loss
+
emergency shortcut
```

### Reconnect + emergency

```text
reconnect begins
+
emergency
```

### Lease expiry + input event

```text
lease expires
+
input arrives
```

Expected:

```text
input rejected
```

### Teardown + GNOME restart

```text
teardown
+
Mutter restart
```

### Display restore + monitor hotplug

```text
restore topology
+
monitor unplugged
```

### Multiple teardown calls

```text
disconnect
+
timeout cleanup
+
emergency cleanup
```

Expected:

```text
one safe final state
```

Use transition IDs/generations to ensure stale operations cannot overwrite newer state.

---

# 26. Repeated-Cycle Stability Tests

Run repeated:

```text
activate
remote interaction
disconnect
restore
lock
unlock
```

for at least:

- 10 cycles during development
- 50 cycles before release candidate
- 100+ cycles on representative hardware

Monitor:

- GNOME crashes
- Mutter crashes
- PipeWire failures
- memory growth
- file descriptor growth
- orphaned processes
- orphaned virtual monitors
- orphaned display configurations
- stale sessions
- input leakage
- restoration failures

A single successful cycle proves very little.

---

# 27. Long-Running Stability Tests

Run remote sessions for:

- 30 minutes
- 2 hours
- 8 hours
- overnight where practical

Monitor:

- CPU
- memory
- GPU memory
- network bandwidth
- WebRTC stability
- PipeWire stability
- input latency
- frame rate
- session state
- lease renewal
- log volume

Look specifically for gradual resource leaks.

---

# 28. GPU and Hardware Matrix

At minimum, test representative configurations.

## GPU

Where hardware is available:

- Intel integrated graphics
- AMD integrated/discrete graphics
- NVIDIA proprietary driver

For each:

- virtual display creation
- capture
- hardware cursor
- physical display disable/restore
- high-resolution display
- high-refresh display
- reconnect
- repeated cycles

Do not claim universal GPU compatibility from a single test machine.

---

# 29. Display Matrix

Test combinations such as:

| Configuration | Required |
|---|---:|
| One 1080p monitor | Yes |
| One 1440p monitor | Yes |
| One 4K monitor | Yes |
| Multiple monitors | Yes |
| Mixed resolutions | Yes |
| Mixed refresh rates | Yes |
| HDMI | Yes |
| DisplayPort | Yes |
| USB-C display | Recommended |
| Monitor hotplug | Yes |
| Monitor power cycle | Recommended |

Record:

- original topology
- remote topology
- restored topology
- failures
- GNOME crashes
- visual artifacts

---

# 30. Resolution and Refresh Tests

Test:

- 1280×720
- 1920×1080
- 2560×1440
- 3840×2160

Where supported, test:

- 60 Hz
- 120 Hz
- 144 Hz
- higher refresh rates available on hardware

The test suite must not assume every GPU/display supports every mode.

---

# 31. Cursor Tests

Test:

- cursor visible
- cursor movement
- cursor shape changes
- click positioning
- high-DPI scaling
- multi-monitor coordinates
- virtual monitor coordinates
- cursor after reconnect
- cursor after display restoration

Investigate hardware-cursor-specific GNOME behavior.

If a workaround such as disabling hardware cursors is needed, document:

- affected versions
- affected GPUs
- performance impact
- whether it is required or optional

---

# 32. Suspend/Resume Tests

Test:

```text
LOCAL_ACTIVE
    -> suspend
    -> resume
```

and:

```text
REMOTE_ACTIVE
    -> suspend
```

Determine supported behavior.

If remote operation inhibits suspend:

- verify inhibitor exists
- verify it is removed after teardown
- verify emergency does not leave stale inhibitors

If suspend is allowed:

- verify remote session fails safely
- verify local console is restored/locked as appropriate

---

# 33. Logout and Session Restart Tests

Test:

- user logout
- GNOME Shell restart where possible
- GDM restart
- user session restart
- systemd user manager restart
- PipeWire restart

Expected behavior must be explicit.

The system must never assume the old GNOME session still exists after logout.

---

# 34. Power-Loss Tests

Power loss is not controllable in automated testing, but recovery must be evaluated.

Test conceptually and, where practical, with controlled power interruption:

```text
REMOTE_ACTIVE
    -> power loss
    -> system reboot
```

After reboot:

- stale remote session must not remain valid
- stale control lease must not remain valid
- security state must be fail-safe
- physical console must require normal unlock
- remote service must not silently restore an old remote-control session

---

# 35. Systemd Tests

Verify:

- service starts
- service stops
- service restarts
- service watchdog
- crash restart
- startup ordering
- shutdown ordering
- sandboxing
- capability restrictions
- filesystem restrictions
- resource limits
- journal behavior

Test services individually and as a complete stack.

---

# 36. Privilege Boundary Tests

Attempt deliberate privilege violations.

Examples:

- gateway attempts privileged IPC
- gateway attempts filesystem access to secrets
- GNOME agent attempts privileged operation
- untrusted client attempts host-admin operation
- malformed IPC request
- unauthorized D-Bus call
- path traversal
- arbitrary command injection attempt
- privilege escalation through configuration

Expected:

```text
DENY
LOG SECURITY EVENT
NO PRIVILEGE ESCALATION
```

Do not rely solely on application-level authorization.

Verify operating-system-level boundaries as well.

---

# 37. IPC Security Tests

Test:

- unauthorized local client
- wrong UID
- wrong peer credentials
- malformed message
- oversized message
- truncated message
- replayed message
- stale message
- invalid transition ID
- invalid session ID
- invalid epoch
- invalid capability
- unexpected command

IPC must reject malformed or unauthorized requests safely.

---

# 38. Browser Security Tests

Test:

- HTTPS enforcement
- certificate validation
- origin validation
- CSRF protection
- XSS resistance
- clickjacking protection
- WebSocket authentication
- WebSocket origin handling
- session-token handling
- secure cookies where used
- SameSite behavior
- Content Security Policy
- no credentials in URL
- no secrets in localStorage
- no secrets in browser logs

Attempt to connect using:

- wrong origin
- expired token
- revoked token
- old epoch
- invalid host identity
- malformed protocol messages

---

# 39. Rate-Limiting Tests

Test brute-force behavior against:

- username/password
- TOTP
- Remote Access Key
- trusted-device authentication
- session establishment
- WebSocket authentication

Verify:

- rate limiting
- progressive delay or equivalent protection
- security logging
- no trivial account enumeration
- legitimate clients can recover after lockout period

---

# 40. Logging and Secret-Handling Tests

Search all logs after authentication and remote sessions.

Verify that logs do NOT contain:

- Linux password
- TOTP secret
- TOTP recovery codes
- Remote Access Key
- trusted-device secret
- session token
- control lease
- browser authentication headers

Also test:

```text
invalid credential
```

and verify error messages do not accidentally echo submitted secrets.

---

# 41. Configuration Tests

Test:

- missing configuration
- invalid configuration
- unsupported configuration
- permissions too broad
- permissions too restrictive
- corrupted configuration
- configuration upgrade
- configuration downgrade
- unknown fields
- invalid values
- concurrent configuration update

Configuration must fail safely.

Never silently enable insecure behavior because configuration is malformed.

---

# 42. Dependency Failure Tests

Test behavior when:

- Mutter API changes
- required D-Bus interface unavailable
- PipeWire missing
- libei unavailable
- unsupported GNOME version
- unsupported GPU
- unsupported compositor capability
- missing systemd feature

Expected behavior:

```text
unsupported
    ↓
clear diagnostic
    ↓
remote access not activated
```

Never attempt an unsafe partial fallback.

---

# 43. Compatibility Tests

The supported platform is intentionally narrow.

Primary compatibility target:

```text
Ubuntu 26.04 LTS
GNOME 50+
Wayland
systemd
single-user workstation
```

Within that target, test supported GNOME point releases as practical.

Do not broaden compatibility claims until tested.

Do not add KDE/wlroots/X11 compatibility merely to make tests pass.

---

# 44. Automated vs Manual Tests

Not everything should be automated.

## Automate

- state machine
- authentication
- authorization
- leases
- security epoch
- protocol validation
- IPC
- configuration
- rate limiting
- session lifecycle logic
- failure injection
- repeated-cycle logic
- mocked GNOME interfaces
- service startup/shutdown
- security regression tests

## Manual / Hardware

- physical display visibility
- monitor power behavior
- physical keyboard isolation
- physical mouse isolation
- emergency shortcut
- visual artifacts
- GPU-specific behavior
- high-refresh displays
- monitor hotplug
- suspend/resume
- physical privacy

The project should maintain a clear distinction between:

```text
AUTOMATED PASS
```

and:

```text
PHYSICAL/HARDWARE VERIFIED
```

---

# 45. Test Evidence

A passing test should produce enough evidence to understand what was verified.

Useful evidence includes:

- test result
- GNOME version
- Ubuntu version
- kernel version
- GPU
- GPU driver
- monitor topology
- PipeWire version
- relevant component versions
- state before test
- event injected
- expected state
- actual state
- relevant logs
- failure reason if applicable

For hardware tests, screenshots/photos may be useful for visual verification.

Do not store credentials or secrets in test artifacts.

---

# 46. Test Result Format

Use a consistent format:

```text
Test ID:
Test Name:

Environment:
Ubuntu:
GNOME:
Kernel:
GPU:
Driver:
Monitor:

Initial State:

Action:

Expected Result:

Actual Result:

Final State:

Pass/Fail:

Evidence:

Logs:

Known Limitations:

Follow-up:
```

---

# 47. Critical Acceptance Matrix

The following are release-gating tests.

| Area | Requirement | Gate |
|---|---|---:|
| Same GNOME session | Confirmed | BLOCKER |
| Virtual monitor | Reliable | BLOCKER |
| Physical display isolation | Reliable | BLOCKER |
| Physical input isolation | Reliable | BLOCKER |
| Remote input | Reliable | BLOCKER |
| Session lock | Correct semantics | BLOCKER |
| Network-loss recovery | Fail-safe | BLOCKER |
| Main daemon crash | Fail-safe | BLOCKER |
| Emergency takeover | Independent | BLOCKER |
| Security epoch | Correct revocation | BLOCKER |
| Control lease | Correct enforcement | BLOCKER |
| Authentication | Password + TOTP | BLOCKER |
| New-device Access Key | Enforced | BLOCKER |
| Trusted-device revocation | Correct | BLOCKER |
| Privilege separation | Verified | BLOCKER |
| Secret handling | No leakage | BLOCKER |
| Display restoration | Reliable | BLOCKER |
| Input restoration | Reliable | BLOCKER |
| Repeated cycles | Stable | BLOCKER |
| Supported GPU matrix | Tested | BLOCKER |
| Browser security | Verified | BLOCKER |
| NAT traversal | Functional | RELEASE |
| LAN discovery | Functional | RELEASE |
| UX polish | Good | RELEASE |
| Clipboard | Functional | OPTIONAL |
| Audio | Functional | OPTIONAL |
| File transfer | Functional | FUTURE |

---

# 48. Definition of Test Pass

A test is PASS only when:

1. Expected behavior occurred.
2. Security invariants remained intact.
3. No unexpected privilege was exercised.
4. No stale remote authority remained.
5. Final state was correct.
6. Logs contain no unexpected secrets.
7. No unexplained GNOME/system instability occurred.
8. Evidence is recorded for important system/hardware tests.

A test is NOT PASS merely because:

- the remote screen appeared
- the connection succeeded once
- the browser displayed video
- logs looked normal
- the process did not crash
- a mock returned the expected value

---

# 49. Definition of Failure Severity

## P0 — Critical

Examples:

- remote input remains active after lease expiry
- remote input remains active after disconnect
- emergency takeover fails
- physical keyboard remains remotely controllable
- physical display remains exposed when isolation is expected
- stale credential reconnects after revocation
- security epoch can be bypassed
- privilege escalation
- credential leakage
- system can remain remotely controllable after main stack failure

P0 blocks all development progression until resolved.

---

## P1 — High

Examples:

- display restoration occasionally fails
- input restoration occasionally fails
- GNOME crash during common lifecycle
- repeated-cycle instability
- major GPU incompatibility
- reconnect leaves inconsistent state

Blocks release and normally blocks progression of dependent phases.

---

## P2 — Medium

Examples:

- performance degradation
- unusual but recoverable display issue
- browser UX issue
- non-critical diagnostics problem

May be deferred with explicit documentation.

---

## P3 — Low

Examples:

- cosmetic issue
- documentation typo
- minor UI improvement

Does not block release unless it hides a security or operational problem.

---

# 50. Regression Testing

Every fix involving any of the following requires regression tests:

- state machine
- authentication
- authorization
- lease
- security epoch
- GNOME lock
- virtual monitor
- physical display
- physical input
- emergency
- teardown
- recovery
- privileged IPC
- systemd service behavior

A regression test should reproduce the original bug before the fix and pass afterward.

---

# 51. CI Strategy

CI should have multiple tiers.

## Tier 1 — Fast CI

Run on every change:

- lint
- formatting
- type checks where applicable
- unit tests
- state machine tests
- authentication tests
- protocol tests
- security regression tests

---

## Tier 2 — Integration CI

Run on relevant changes:

- IPC tests
- service integration
- mocked GNOME interfaces
- protocol integration
- browser/client tests
- WebRTC signalling tests

---

## Tier 3 — Real GNOME System Tests

Run on supported Ubuntu/GNOME test machines:

- session discovery
- Mutter integration
- virtual monitor
- PipeWire
- input
- display isolation
- lock
- teardown
- recovery

---

## Tier 4 — Hardware Matrix

Run before release candidates and after major GNOME/Mutter changes.

Include:

- Intel
- AMD
- NVIDIA where supported
- multiple monitor configurations
- high-resolution displays
- high-refresh displays

---

# 52. Test Isolation

Tests must avoid damaging the developer's normal workstation.

Where possible, use:

- dedicated test users
- disposable VMs
- dedicated test machines
- snapshots
- isolated networks
- virtual displays
- dedicated monitors
- controlled hardware

Never make destructive system tests part of an ordinary development command unless explicitly requested.

In particular, tests that:

- disable physical displays
- suppress physical input
- manipulate display topology
- lock the desktop
- modify privileged system services

must be clearly identified as system/hardware tests.

---

# 53. Fault-Injection Harness

Build a reusable fault-injection mechanism for development/testing.

It should be able to simulate events such as:

```text
NETWORK_LOST
NETWORK_RESTORED
LEASE_EXPIRED
LEASE_REVOKED
SECURITY_EPOCH_CHANGED
HOSTD_CRASH
GATEWAY_CRASH
GNOME_AGENT_CRASH
PIPEWIRE_FAILURE
MUTTER_FAILURE
DISPLAY_RESTORE_FAILURE
INPUT_RESTORE_FAILURE
EMERGENCY_TRIGGER
SESSION_LOGOUT
SYSTEM_SUSPEND
```

The state machine should be testable against these events without requiring every test to reproduce the physical condition.

Real-system fault injection should then validate that the actual components generate equivalent events.

---

# 54. Property-Based / Model Testing

Where practical, use property-based testing for the state machine.

Generate random sequences of events such as:

```text
CONNECT
DISCONNECT
LEASE_EXPIRE
RECONNECT
EMERGENCY
NETWORK_LOSS
NETWORK_RESTORE
HOSTD_RESTART
GNOME_FAILURE
DISPLAY_FAILURE
INPUT_FAILURE
```

After every generated event sequence, assert:

```text
remote input is allowed
    ONLY IF
    authenticated
    AND authorized
    AND session valid
    AND epoch valid
    AND lease valid
    AND GNOME state valid
```

Also assert that no event sequence leaves an impossible state.

---

# 55. Concurrency Model Testing

Test simultaneous events from:

- network thread
- state-machine controller
- GNOME agent
- watchdog
- emergency daemon
- browser client

Do not rely on event arrival order.

Examples:

```text
disconnect + emergency
emergency + reconnect
lease expiry + input
display failure + teardown
GNOME restart + reconnect
daemon restart + stale event
```

The final state must be deterministic and safe even when event ordering varies.

---

# 56. Performance Testing

Measure:

- end-to-end input latency
- video latency
- frame rate
- CPU usage
- GPU usage
- memory usage
- bandwidth
- startup time
- activation time
- teardown time
- reconnect time

Performance optimization must never weaken:

- authorization
- lease enforcement
- display isolation
- input isolation
- emergency handling
- fail-safe behavior

---

# 57. Security Test Checklist Before Release

Before release, explicitly verify:

```text
[ ] Password authentication tested
[ ] TOTP tested
[ ] Remote Access Key tested
[ ] Trusted-device authentication tested
[ ] Trusted-device revocation tested
[ ] Session token expiry tested
[ ] Control lease expiry tested
[ ] Security epoch tested
[ ] Emergency invalidation tested
[ ] Replay attempts tested
[ ] Brute-force protection tested
[ ] Browser credential handling tested
[ ] Secrets absent from logs
[ ] IPC authorization tested
[ ] Privilege boundaries tested
[ ] Gateway isolation tested
[ ] GNOME agent isolation tested
[ ] Emergency daemon isolation tested
[ ] Stale-session recovery tested
```

---

# 58. Safety Test Checklist Before Release

```text
[ ] Network loss fails closed
[ ] Gateway crash fails closed
[ ] Host daemon crash fails closed
[ ] GNOME agent crash fails closed
[ ] PipeWire failure fails closed
[ ] Browser crash fails closed
[ ] Lease expiry fails closed
[ ] Emergency works
[ ] Emergency works without network
[ ] Physical input is restored
[ ] Physical display is restored
[ ] GNOME session is locked
[ ] Old remote credentials cannot regain control
[ ] Reboot leaves system safe
[ ] Logout leaves system safe
[ ] Repeated activation/teardown is stable
```

---

# 59. Final Release-Gate Test

The complete product must pass the following scenario on every release candidate.

### Starting condition

```text
Ubuntu 26.04 LTS
GNOME 50+
Wayland
normal physical GNOME session
physical monitor(s) active
physical keyboard/mouse active
```

### Sequence

```text
1. Start with normal local session.

2. Lock the workstation.

3. Authenticate remotely:
   - username
   - password
   - TOTP
   - Remote Access Key if required

4. Establish session.

5. Create virtual monitor.

6. Disable physical outputs.

7. Isolate physical input.

8. Establish WebRTC media.

9. Enter REMOTE_ACTIVE.

10. Verify remote keyboard/mouse.

11. Verify physical keyboard/mouse cannot control session.

12. Verify physical display does not expose the remote desktop.

13. Simulate network failure.

14. Verify remote authority is revoked.

15. Verify session is locked.

16. Verify physical display is restored.

17. Verify physical input is restored.

18. Reconnect using valid authentication.

19. Repeat remote operation.

20. Trigger emergency takeover.

21. Verify remote authority is cryptographically invalidated.

22. Verify security epoch changed.

23. Verify remote reconnect using stale credentials fails.

24. Verify local console remains locked.

25. Unlock normally at the physical workstation.

26. Verify original GNOME session remains usable.
```

This scenario is a **release blocker** if any critical safety invariant fails.

---

# 60. Copilot Agent Instructions

When implementing tests:

1. Inspect the repository and the workflow/configuration produced by `adaptive-workflow-configurator` before creating or modifying test structure.
2. Respect its established conventions.
3. Do not replace, duplicate, or fight the configured workflow.
4. Do not prescribe a directory layout merely to satisfy this document.
5. Place tests according to the repository's existing architecture and workflow.

For every implementation task:

```text
RESEARCH
    ↓
IDENTIFY TESTABLE BEHAVIOR
    ↓
WRITE/UPDATE TESTS
    ↓
IMPLEMENT
    ↓
RUN FAST TESTS
    ↓
RUN RELEVANT INTEGRATION TESTS
    ↓
RUN SYSTEM/HARDWARE TESTS WHEN REQUIRED
    ↓
ANALYZE FAILURES
    ↓
DOCUMENT EVIDENCE
    ↓
REGRESSION TEST
```

Do not skip directly from implementation to claiming completion.

---

# 61. Copilot Agent Stop Rules

Stop and report rather than hiding a failure when:

- a safety invariant cannot be tested
- physical input isolation cannot be verified
- physical display isolation cannot be verified
- GNOME lock semantics are ambiguous
- a failure path leaves uncertain remote authority
- emergency recovery depends on the main remote stack
- stale credentials can reconnect
- display restoration is unreliable
- input restoration is unreliable
- a privileged operation cannot be bounded
- GNOME/Mutter behavior contradicts an architectural assumption
- a test passes only because of a workaround that changes the security model

Do not weaken a test merely to make CI green.

---

# 62. Definition of Done for Testing

Testing for a feature is complete only when:

```text
[ ] Happy path tested
[ ] Failure path tested
[ ] Invalid input tested
[ ] Timeout tested where applicable
[ ] Restart tested where applicable
[ ] Race condition considered
[ ] Security implications tested
[ ] Regression test added for discovered bugs
[ ] Relevant integration tests pass
[ ] Relevant system tests pass
[ ] Hardware behavior verified where required
[ ] Evidence recorded
[ ] Documentation updated
```

For security-critical functionality, all applicable items are mandatory.

---

# 63. Final Testing Principle

The most important test is not:

> “Can the remote client control the computer?”

It is:

> “Can the system guarantee that remote control stops when authorization, connectivity, session validity, or system health is no longer trustworthy?”

The project should therefore optimize its testing around the following invariant:

```text
REMOTE CONTROL IS A TEMPORARY PRIVILEGE
```

It exists only while all required conditions remain true.

```text
AUTHENTICATED
    +
AUTHORIZED
    +
VALID SESSION
    +
VALID SECURITY EPOCH
    +
VALID CONTROL LEASE
    +
VALID GNOME STATE
    +
VALID REMOTE CHANNEL
    +
VALID INPUT/DISPLAY STATE
        |
        v
REMOTE CONTROL ALLOWED
```

If any required condition becomes false:

```text
REMOTE CONTROL
      ↓
    REVOKE
      ↓
     LOCK
      ↓
   RESTORE
      ↓
    VERIFY
      ↓
 LOCAL_LOCKED
```

**Fail closed. Recover deterministically. Verify on real hardware. Never assume that a mocked GNOME environment represents actual Wayland behavior.**