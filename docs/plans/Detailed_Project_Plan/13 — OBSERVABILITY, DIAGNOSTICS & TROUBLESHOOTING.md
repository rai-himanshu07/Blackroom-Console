# 13 — OBSERVABILITY, DIAGNOSTICS & TROUBLESHOOTING

## 1. Purpose

This document defines the observability and diagnostic requirements for the project.

The system must make it possible to answer, for any remote-session incident:

1. What state was the system in?
2. What transition was occurring?
3. What event caused the transition?
4. Which component handled it?
5. Which component failed?
6. Was remote authority revoked?
7. Was the GNOME session locked?
8. Were physical outputs restored?
9. Was physical input restored?
10. Why did recovery succeed or fail?

Diagnostics must be useful to developers and users without exposing:

- passwords
- TOTP secrets
- Remote Access Keys
- trusted-device credentials
- session tokens
- control leases
- private browser credentials
- unnecessary desktop/application content

---

# 2. Observability Principles

Follow these principles:

1. Every important state transition must be observable.
2. Every security decision must be auditable.
3. Every failure must have a meaningful classification.
4. Logs must explain what happened without exposing secrets.
5. Logs must distinguish expected disconnects from failures.
6. Diagnostics must work even when the remote connection does not.
7. The emergency path must remain diagnosable without depending on the gateway.
8. GNOME/Mutter/PipeWire/systemd failures must be correlated.
9. State must be more important than raw log volume.
10. Diagnostic tooling must never become an administrative command-execution interface.
11. Sensitive information must be deliberately excluded rather than filtered after logging.
12. Diagnostic output should help determine whether the failure is software, configuration, GNOME, hardware, network, or authentication related.

---

# 3. Observability Architecture

The project should expose observability through several layers:

```text
                    +----------------------+
                    |   Diagnostic CLI     |
                    +----------+-----------+
                               |
                    +----------v-----------+
                    |   Host Diagnostics   |
                    +----------+-----------+
                               |
          +--------------------+--------------------+
          |                    |                    |
          v                    v                    v
     remote-hostd       GNOME Session Agent   Emergency Daemon
          |                    |                    |
          +--------------------+--------------------+
                               |
                    +----------v-----------+
                    | systemd / journald   |
                    +----------------------+
```

The browser may expose limited user-facing status, but detailed diagnostics should remain local.

---

# 4. Component Health

Each major component should expose a health state.

Components:

- `remote-hostd`
- `remote-gateway`
- `gnome-session-agent`
- `remote-emergencyd`
- PipeWire
- Mutter/GNOME Shell
- networking subsystem
- WebRTC session
- authentication subsystem

Example:

```text id="u2bqgm"
remote-hostd
    HEALTHY

gnome-session-agent
    HEALTHY

PipeWire
    HEALTHY

Mutter
    HEALTHY

WebRTC
    CONNECTED

REMOTE SESSION
    ACTIVE
```

Health must not be interpreted as permission to perform remote control.

A component can be healthy while a control lease is invalid.

---

# 5. State as the Primary Diagnostic Signal

The current authoritative state must always be available to local diagnostics.

Example:

```text id="a8p6d2"
State:
    REMOTE_ACTIVE

Session:
    8f2c...

User:
    <redacted or local username according to policy>

Client:
    trusted-device-3

Security Epoch:
    42

Control Lease:
    VALID

Physical Display:
    ISOLATED

Physical Input:
    ISOLATED

GNOME Session:
    PRESENT

Virtual Display:
    ACTIVE

WebRTC:
    CONNECTED
```

Sensitive identifiers should be truncated or hashed where full values are unnecessary.

---

# 6. State Transition Events

Every state transition should generate a structured event.

Example:

```text id="w9q4b3"
timestamp:
event:
previous_state:
new_state:
transition_id:
trigger:
component:
result:
failure_code:
```

Example conceptual event:

```text
REMOTE_ACTIVE -> TEARING_DOWN
trigger=NETWORK_LOSS
component=remote-hostd
result=started
```

Do not log:

- session tokens
- access keys
- TOTP values
- passwords
- raw authentication headers

---

# 7. Structured Logging

Prefer structured logs over free-form messages.

Recommended fields:

```text id="w3dr7e"
timestamp
severity
component
event
state
transition_id
session_id
client_id
security_epoch
result
error_code
duration_ms
```

Where appropriate, include:

```text
gnome_version
mutter_version
pipewire_version
kernel_version
gpu_identifier
display_count
virtual_display_state
network_state
```

Do not include secrets.

---

# 8. Log Severity

Use consistent severity levels.

## DEBUG

Detailed developer information.

Examples:

- capability detection
- IPC message type
- state evaluation
- lease renewal timing

DEBUG logging should be configurable and disabled by default where excessive detail could create privacy concerns.

---

## INFO

Normal operational events.

Examples:

- service started
- session authenticated
- remote session established
- remote session disconnected
- display isolation completed
- display restoration completed

---

## NOTICE / WARNING

Unexpected but recoverable conditions.

Examples:

- temporary PipeWire interruption
- network degradation
- reconnect attempt
- monitor hotplug
- unsupported optional feature

---

## ERROR

Operation failed but the system recovered or entered a safe state.

Examples:

- virtual monitor creation failed
- display restoration required fallback
- GNOME agent unavailable

---

## CRITICAL

Safety or security failure.

Examples:

- remote authority could not be revoked
- physical input could not be restored
- stale remote lease accepted
- emergency action failed
- security epoch inconsistency
- privilege boundary violation

Critical events must be especially easy to identify.

---

# 9. Security Event Logging

Security-relevant events must be logged.

Examples:

- authentication success
- authentication failure
- TOTP failure
- Remote Access Key failure
- trusted-device registration
- trusted-device revocation
- session creation
- session termination
- lease creation
- lease expiry
- lease revocation
- security epoch increment
- emergency activation
- stale credential rejection
- replay rejection
- rate-limit activation
- unauthorized IPC request
- privilege-boundary violation

Logs should provide enough information for investigation without exposing authentication material.

---

# 10. Authentication Logging

Do not log submitted credentials.

Good:

```text
authentication_failed
method=totp
reason=invalid_code
client_id=abc123
```

Bad:

```text
password=...
totp=123456
access_key=...
```

Authentication failures should be distinguishable by category while avoiding unnecessary account enumeration information.

---

# 11. Remote Session Correlation

Every remote session should have a unique diagnostic session identifier.

Example:

```text id="g3y0s5"
session_id = rs_01J...
```

Use this identifier to correlate:

- authentication
- authorization
- lease
- WebRTC
- GNOME agent
- display setup
- input setup
- teardown
- recovery

A session identifier must not itself grant authority.

---

# 12. Transition IDs

Every significant state-machine transaction should have a unique transition ID.

Example:

```text id="q0v9x7"
transition_id = tr_01J...
```

This allows logs to distinguish:

```text
activation attempt A
```

from:

```text
reconnect attempt B
```

even if their events overlap.

Transition IDs are especially important for race-condition debugging.

---

# 13. Security Epoch Diagnostics

The diagnostic state should show the current epoch, but never expose credentials derived from it.

Example:

```text id="m0j3qa"
Security Epoch:
    42

Active Session Epoch:
    42

Lease Epoch:
    42

Result:
    VALID
```

If values differ:

```text
Security Epoch:
    43

Session Epoch:
    42

Result:
    REVOKED
```

This makes stale-session failures much easier to diagnose.

---

# 14. Control Lease Diagnostics

Expose only safe metadata:

```text id="4j4y6m"
Lease:
    VALID

Issued:
    <timestamp>

Expires:
    <timestamp>

Client:
    <identifier>

Session:
    <identifier>

Epoch:
    42
```

Never display the actual lease credential.

---

# 15. GNOME Diagnostic Information

The local diagnostic tool should collect:

- Ubuntu version
- kernel version
- GNOME version
- Mutter version
- Wayland status
- session type
- session user
- DBus availability
- Mutter interfaces available
- ScreenCast availability
- RemoteDesktop availability
- DisplayConfig availability
- PipeWire availability
- libei/EIS availability
- GNOME session state
- virtual monitor capability
- physical display topology
- GPU information
- graphics driver information

Avoid collecting application/window content unless explicitly required for debugging.

---

# 16. Capability Report

Provide a diagnostic capability report.

Example:

```text id="r7h2kp"
Platform:
    Ubuntu 26.04
    GNOME 50.1
    Wayland

Session:
    Supported

Mutter RemoteDesktop:
    Available

Mutter ScreenCast:
    Available

RecordVirtual:
    Available

DisplayConfig:
    Available

PipeWire:
    Available

libei/EIS:
    Available

Physical display isolation:
    Supported / Unknown / Failed

Physical input isolation:
    Supported / Unknown / Failed

Emergency controller:
    Available

Overall:
    SUPPORTED
```

The report should clearly distinguish:

```text
SUPPORTED
AVAILABLE
UNKNOWN
FAILED
NOT AVAILABLE
```

Do not infer support merely because a D-Bus interface exists.

---

# 17. GNOME/Mutter Version Diagnostics

Because private/unstable Mutter APIs may change, record:

- GNOME version
- Mutter version
- relevant interface availability
- backend selected
- capability checks
- known compatibility flags

Do not simply branch on version numbers if capability detection can be used instead.

If a version-specific workaround is required, log:

```text
workaround=<identifier>
```

rather than dumping implementation details into normal logs.

---

# 18. Display Topology Diagnostics

Before remote activation, record a safe representation of the original topology.

Example:

```text id="l4e3b1"
Physical Outputs:
    HDMI-A-1
        2560x1440
        144Hz

    DP-1
        1920x1080
        60Hz

Remote Virtual Output:
    Not active
```

During remote operation:

```text
Physical Outputs:
    Disabled

Virtual Output:
    1920x1080
    60Hz
```

After restoration:

```text
Restoration:
    SUCCESS

Original topology:
    RESTORED
```

Avoid storing unnecessary EDID or hardware-identifying information in persistent logs.

---

# 19. Physical Input Diagnostics

Input isolation should expose state, not raw input events.

Good:

```text
Physical Input:
    ISOLATED
```

Bad:

```text
KEYBOARD_EVENT:
    keycode=...
    timestamp=...
```

unless a deliberately controlled low-level diagnostic mode is required.

Never log actual keyboard content.

---

# 20. Emergency Diagnostics

The emergency controller should generate an event whenever triggered.

Example:

```text id="f6q9v1"
event:
    EMERGENCY_TRIGGERED

trigger:
    PHYSICAL_HOTKEY

remote_authority:
    REVOKED

security_epoch:
    INCREMENTED

session:
    LOCKED

physical_display:
    RESTORED

physical_input:
    RESTORED

final_state:
    LOCAL_LOCKED
```

If any operation fails, the diagnostic record must identify the failure.

---

# 21. Emergency Failure Diagnostics

Example:

```text id="d4t8z2"
Emergency:
    TRIGGERED

Remote authority:
    REVOKED

Session lock:
    SUCCESS

Display restoration:
    FAILED

Input restoration:
    SUCCESS

Final state:
    FAILED_SAFE

Required action:
    MANUAL DISPLAY RECOVERY
```

The system must not report success when only some steps completed.

---

# 22. Fail-Safe Verification

After recovery, diagnostics should explicitly verify safety invariants.

Example:

```text id="b8m1xy"
Remote authority:
    REVOKED

Remote input:
    DISABLED

Session:
    LOCKED

Physical display:
    RESTORED

Physical input:
    RESTORED

Stale session:
    INVALID

Security epoch:
    CURRENT

Safety state:
    VERIFIED
```

This is much more useful than merely reporting:

```text cleanup completed
```

---

# 23. Diagnostic CLI

Provide a local diagnostic CLI or equivalent administrative diagnostic interface.

Potential commands conceptually:

```text
status
health
capabilities
session
display
input
network
security
logs
self-test
version
```

The exact command names should follow repository conventions.

The CLI must not become a generic privileged command runner.

---

# 24. Status Command

A status command should provide a concise operational summary.

Example:

```text id="3f9x1a"
Remote Console Status

Service:
    RUNNING

GNOME:
    READY

Current State:
    LOCAL_LOCKED

Remote Session:
    NONE

Physical Display:
    ACTIVE

Physical Input:
    ACTIVE

Emergency Controller:
    READY

Security Epoch:
    42

Overall:
    READY
```

---

# 25. Health Command

Health should distinguish component availability.

Example:

```text id="8h3r6m"
remote-hostd       OK
gnome-session-agent OK
remote-emergencyd  OK
PipeWire           OK
Mutter             OK
Network            OK
Gateway            OK
```

Health checks should not establish a remote session.

---

# 26. Self-Test

A local self-test should verify safe, non-destructive capabilities.

Potential checks:

- service connectivity
- IPC
- DBus
- Mutter interface discovery
- PipeWire availability
- libei availability
- session detection
- configuration validity
- certificate configuration
- permissions
- systemd unit health

Destructive operations such as disabling the physical display or blocking input should require an explicit test mode and should never be performed silently by a normal health check.

---

# 27. Safe Diagnostic Mode

Provide a diagnostic mode that can collect information without activating remote control.

It may inspect:

- system versions
- capabilities
- services
- configuration
- network reachability
- IPC
- GNOME state

It must not:

- unlock the session
- enable remote control
- disable physical input
- disable physical displays
- modify authentication
- rotate credentials

---

# 28. Diagnostic Bundle

Provide an optional diagnostic bundle for troubleshooting.

The bundle may contain:

```text id="q4v7mk"
system information
service status
capability report
recent structured logs
state history
recent failure codes
GNOME/Mutter diagnostic information
PipeWire diagnostic information
network diagnostics
configuration schema/version
```

It must NOT contain:

- passwords
- TOTP secret
- Remote Access Key
- recovery codes
- trusted-device secrets
- session tokens
- control leases
- browser cookies
- private keys
- clipboard contents
- screen captures
- keyboard contents

---

# 29. Redaction

Sensitive data should ideally never enter logs.

Redaction is a secondary defense, not the primary mechanism.

If sensitive identifiers must appear:

- hash them
- truncate them
- use stable non-secret identifiers

Example:

```text
client_id:
    8f21...b92a
```

instead of:

```text
client_credential:
    <secret>
```

---

# 30. Log Retention

Use systemd/journald or the project's established logging infrastructure.

Avoid unlimited logging.

Define:

- maximum log volume
- retention period
- debug logging behavior
- security-event retention
- diagnostic bundle size

The exact values should be configurable and documented.

---

# 31. Privacy

The remote system operates on a personal workstation and therefore diagnostics must respect privacy.

Do not collect by default:

- application names unless necessary
- window titles
- document names
- file paths unrelated to the service
- clipboard content
- screen contents
- keystrokes
- microphone/audio data

Diagnostic telemetry should be local by default.

Do not introduce cloud telemetry merely to make debugging easier.

---

# 32. Remote Diagnostics

The browser may show limited operational information:

```text
Connected
Preparing workstation
Remote session active
Network degraded
Reconnecting
Session terminated
Workstation locked
```

Do not expose detailed host diagnostics to an unauthenticated browser.

Detailed diagnostics require appropriate authorization.

---

# 33. User-Facing Error Categories

The browser should present understandable errors.

Examples:

```text
Authentication failed
Remote access key required
TOTP verification failed
Host unavailable
Remote session rejected
Workstation is busy
Remote session expired
Connection lost
Host entered safe recovery
Remote access disabled
Host environment unsupported
```

Avoid exposing internal stack traces.

---

# 34. Developer Error Codes

Internally, use stable error codes.

Examples:

```text
AUTH_INVALID_PASSWORD
AUTH_INVALID_TOTP
AUTH_ACCESS_KEY_REQUIRED
AUTH_ACCESS_KEY_INVALID
AUTH_DEVICE_REVOKED

SESSION_NOT_FOUND
SESSION_EPOCH_MISMATCH
LEASE_EXPIRED
LEASE_REVOKED

GNOME_SESSION_UNAVAILABLE
MUTTER_INTERFACE_UNAVAILABLE
VIRTUAL_MONITOR_FAILED
DISPLAY_ISOLATION_FAILED
INPUT_ISOLATION_FAILED
SESSION_LOCK_FAILED

PIPEWIRE_UNAVAILABLE
WEBRTC_FAILED
NETWORK_UNAVAILABLE

EMERGENCY_TRIGGERED
EMERGENCY_RECOVERY_FAILED
DISPLAY_RESTORE_FAILED
INPUT_RESTORE_FAILED
```

Error codes should remain stable even if implementation details change.

---

# 35. Troubleshooting Decision Tree

The diagnostic process should generally follow:

```text id="5j7x3m"
Connection failed
      |
      v
Is host reachable?
      |
   +--NO--> Network / gateway diagnosis
   |
  YES
   |
   v
Did authentication succeed?
   |
   +--NO--> Authentication diagnosis
   |
  YES
   |
   v
Did session authorization succeed?
   |
   +--NO--> Session / policy diagnosis
   |
  YES
   |
   v
Did GNOME preparation succeed?
   |
   +--NO--> GNOME / Mutter diagnosis
   |
  YES
   |
   v
Did display isolation succeed?
   |
   +--NO--> Display topology diagnosis
   |
  YES
   |
   v
Did input isolation succeed?
   |
   +--NO--> Input/seat diagnosis
   |
  YES
   |
   v
Did PipeWire/WebRTC succeed?
   |
   +--NO--> Media/network diagnosis
   |
  YES
   |
   v
REMOTE_ACTIVE
```

This should guide both human troubleshooting and automated diagnostics.

---

# 36. Common Failure Scenarios

## Scenario A — Authentication works, remote session does not start

Check:

1. GNOME session exists.
2. Mutter interfaces available.
3. virtual monitor capability.
4. display configuration.
5. input capability.
6. PipeWire.
7. logs for transition ID.

---

## Scenario B — Remote video works, remote input does not

Check:

1. libei/EIS availability.
2. RemoteDesktop authorization.
3. control lease.
4. security epoch.
5. input-isolation state.
6. GNOME session state.

Never solve this by blindly granting broader privileges.

---

## Scenario C — Remote works but physical keyboard still controls GNOME

This is a **critical safety issue**, not a cosmetic bug.

Immediately:

1. revoke remote session
2. restore safe local state
3. preserve diagnostic evidence
4. classify as P0
5. prevent release until resolved

---

## Scenario D — Remote works but physical display remains visible

Treat as a privacy/safety failure.

Check:

- DisplayConfig state
- active physical outputs
- virtual output state
- monitor hotplug
- restoration/isolation transaction
- GPU-specific behavior

Do not replace the requirement with a black fullscreen window without explicitly changing the product security model.

---

## Scenario E — Disconnect leaves workstation unlocked

Treat as a safety failure.

Expected:

```text
disconnect
    ↓
remote authority revoked
    ↓
session locked
```

---

## Scenario F — Emergency shortcut does not work

Treat as a critical operational failure.

Determine whether:

- emergency daemon is running
- hotkey listener is operational
- physical input path is available
- main daemon failure affected emergency
- GNOME session state prevented action
- service permissions are insufficient

The emergency path must not depend on the browser or network.

---

# 37. Failure Classification

Every significant failure should be classified as one of:

```text
AUTHENTICATION
AUTHORIZATION
SESSION
GNOME
MUTTER
PIPEWIRE
INPUT
DISPLAY
NETWORK
WEBRTC
SYSTEMD
PRIVILEGE
CONFIGURATION
HARDWARE
RECOVERY
SECURITY
UNKNOWN
```

`UNKNOWN` is acceptable temporarily, but should generate an investigation item.

---

# 38. Incident Timeline

For complex failures, diagnostics should be able to reconstruct:

```text id="5r4x2n"
12:01:02 AUTHENTICATED
12:01:03 SESSION_CREATED
12:01:03 LEASE_CREATED
12:01:04 VIRTUAL_DISPLAY_CREATED
12:01:04 PHYSICAL_DISPLAY_DISABLED
12:01:05 PHYSICAL_INPUT_DISABLED
12:01:06 WEBRTC_CONNECTED
12:01:06 REMOTE_ACTIVE

12:14:31 NETWORK_LOSS
12:14:33 LEASE_EXPIRED
12:14:33 REMOTE_AUTHORITY_REVOKED
12:14:33 SESSION_LOCKED
12:14:34 DISPLAY_RESTORED
12:14:34 INPUT_RESTORED
12:14:34 LOCAL_LOCKED
```

This timeline is one of the most valuable debugging artifacts in the project.

---

# 39. Crash Diagnostics

When a component crashes:

Record:

- component
- timestamp
- current state
- transition ID
- session ID
- security epoch
- last known operation
- restart count
- resulting state

Do not record sensitive process memory or credentials.

Where appropriate, integrate with standard Linux/systemd crash diagnostics rather than creating a custom crash-dump mechanism.

---

# 40. Restart Diagnostics

After a service restart, diagnostics should indicate:

```text
previous instance:
    TERMINATED

new instance:
    STARTED

active remote sessions:
    INVALIDATED

security epoch:
    CURRENT / CHANGED

recovery:
    REQUIRED / NOT REQUIRED

final state:
    LOCAL_LOCKED
```

A restarted daemon must not blindly reconstruct remote authority from stale state.

---

# 41. GNOME Crash Diagnostics

If Mutter/GNOME Shell crashes:

- identify the crash
- determine whether the remote session was active
- revoke remote authority
- determine display state
- determine input state
- verify recovery
- preserve relevant system logs

If GNOME restarts and the previous remote state cannot be trusted:

```text
REMOTE CONTROL
    ↓
REVOKE
    ↓
LOCK / SAFE RECOVERY
```

Do not attempt to preserve remote authority at the expense of safety.

---

# 42. Performance Diagnostics

Diagnostic output should optionally provide:

```text
Video:
    resolution
    FPS
    estimated latency

Input:
    input latency
    lease renewal latency

Network:
    RTT
    packet loss
    bitrate

Host:
    CPU
    memory
    GPU utilization

PipeWire:
    stream state

WebRTC:
    connection state
```

Performance diagnostics must never require capturing sensitive user content.

---

# 43. Network Diagnostics

Provide safe diagnostics for:

- local listener
- DNS
- IPv4
- IPv6
- gateway connectivity
- STUN
- TURN
- rendezvous
- WebSocket signalling
- WebRTC establishment

Do not expose authentication credentials while performing network diagnostics.

---

# 44. LAN Diagnostics

For LAN operation, diagnose:

- interface state
- IP addresses
- IPv4/IPv6 reachability
- mDNS/Avahi discovery
- host identity
- firewall reachability
- listening socket
- TLS availability

Avoid assuming that hostname or IP address is the host's cryptographic identity.

---

# 45. Firewall Diagnostics

The diagnostic tool should indicate:

```text
Expected network access:
    AVAILABLE

Listener:
    ACTIVE

Firewall:
    POSSIBLY BLOCKING
```

It should not automatically disable the user's firewall.

Any firewall modification must be explicit, documented, and reversible.

---

# 46. Configuration Diagnostics

Validate configuration and report:

```text
Configuration:
    VALID

Authentication:
    CONFIGURED

TOTP:
    CONFIGURED

Remote Access Key:
    CONFIGURED

Trusted Devices:
    2

Gateway:
    CONFIGURED

TLS:
    VALID

Emergency Controller:
    CONFIGURED
```

Never print secrets.

---

# 47. Security Audit View

Provide a local security-oriented status view.

Example:

```text id="7m3q9w"
Security Status

TOTP:
    ENABLED

Remote Access Key:
    CONFIGURED

Trusted Devices:
    2

Active Sessions:
    1

Control Lease:
    VALID

Security Epoch:
    42

Last Emergency:
    <timestamp>

Authentication Failures:
    <count>

Current Safety State:
    VERIFIED
```

---

# 48. Diagnostic Test Levels

Diagnostics should distinguish:

### Level 1 — Status

Safe, fast, non-destructive.

### Level 2 — Self-Test

Validates configuration and component availability.

### Level 3 — Integration Test

May interact with GNOME session but should remain non-destructive unless explicitly requested.

### Level 4 — Remote Session Test

Actually establishes a remote session.

### Level 5 — Hardware Safety Test

May:

- disable physical outputs
- isolate physical input
- lock the workstation

Must require explicit operator intent.

Never hide Level 5 behavior behind a generic `health` command.

---

# 49. Support Workflow

When reporting an issue, users/developers should provide:

1. Ubuntu version
2. GNOME version
3. kernel
4. GPU/driver
5. monitor topology
6. project version
7. diagnostic status
8. capability report
9. relevant error code
10. transition ID
11. session ID if available
12. diagnostic bundle if appropriate

Never request:

- passwords
- TOTP secrets
- Remote Access Keys
- recovery codes
- private keys

---

# 50. Debug Logging

Debug logging should be explicitly enabled.

Example conceptual flow:

```text
normal:
    INFO

troubleshooting:
    DEBUG

security incident:
    structured security events
```

Debug mode should have clear warnings about potentially increased metadata collection.

Even DEBUG logging must never include secrets.

---

# 51. Logging Must Not Change Security Behavior

Enabling diagnostics must not:

- extend control leases
- disable authentication
- disable rate limits
- bypass authorization
- prevent emergency recovery
- keep remote sessions alive
- disable display/input restoration

Debugging must never become an insecure mode.

---

# 52. Diagnostic API Security

If diagnostics are exposed over IPC:

- authenticate local callers
- enforce authorization
- use read-only diagnostic interfaces by default
- separate diagnostics from privileged control
- reject arbitrary command parameters

Avoid a design like:

```text
diagnostic.execute(command)
```

The diagnostic API should expose explicit operations:

```text
get_status()
get_capabilities()
get_health()
get_recent_events()
```

not arbitrary shell execution.

---

# 53. Diagnostic Data Model

Prefer structured event objects internally.

Conceptually:

```text id="4x3v2z"
DiagnosticEvent:
    timestamp
    event_type
    severity
    component
    state
    transition_id
    session_id
    security_epoch
    result
    error_code
    metadata
```

This allows:

- CLI output
- structured journald logs
- support bundles
- automated tests
- future GUI diagnostics

without duplicating logging logic.

---

# 54. Test Requirements for Observability

Observability itself must be tested.

Verify:

- every major state transition creates an event
- security events are logged
- failures have error codes
- session IDs correlate events
- transition IDs correlate transactions
- stale epochs are visible
- emergency events are visible
- secrets never appear
- diagnostic commands do not alter state
- logs remain usable after component crashes

---

# 55. Observability Acceptance Tests

Before release:

```text id="g5x1rz"
[ ] Current state is observable
[ ] Component health is observable
[ ] State transitions are logged
[ ] Security events are logged
[ ] Session IDs correlate events
[ ] Transition IDs correlate operations
[ ] Security epoch is diagnosable
[ ] Control lease is diagnosable without exposing it
[ ] GNOME capabilities are observable
[ ] Display state is observable
[ ] Input state is observable
[ ] Emergency events are observable
[ ] Recovery result is explicit
[ ] Failure codes are stable
[ ] Diagnostic bundle excludes secrets
[ ] Debug logging excludes secrets
[ ] Browser errors are understandable
[ ] CLI is non-destructive by default
```

---

# 56. Copilot Agent Instructions

When implementing observability:

1. Inspect the repository and the workflow established by `adaptive-workflow-configurator`.
2. Follow the existing logging, CLI, service, and test conventions.
3. Do not introduce a competing architecture without justification.
4. Keep diagnostic code separate from privileged control logic.
5. Make security-critical events structured and machine-readable.
6. Never log credentials.
7. Never expose arbitrary command execution through diagnostics.
8. Add tests for both observability behavior and secret exclusion.
9. Ensure diagnostics work during failure/recovery states.
10. Document every new diagnostic/error code.
11. Prefer capability detection over fragile version checks.
12. Keep user-facing errors separate from internal diagnostic detail.

Use:

```text
RESEARCH
    ↓
DESIGN OBSERVABILITY
    ↓
IMPLEMENT STRUCTURED EVENTS
    ↓
ADD SECURITY REDACTION TESTS
    ↓
ADD CLI / STATUS OUTPUT
    ↓
INTEGRATE WITH SYSTEMD/JOURNAL
    ↓
TEST FAILURE PATHS
    ↓
VERIFY NO SECRET LEAKAGE
    ↓
DOCUMENT ERROR CODES
```

---

# 57. Definition of Done

Observability is complete only when:

```text id="z4p2xc"
[ ] State is observable
[ ] State transitions are traceable
[ ] Sessions are correlatable
[ ] Transitions are correlatable
[ ] Security epoch is visible
[ ] Control lease state is visible
[ ] GNOME capabilities are visible
[ ] Display/input state is visible
[ ] Emergency state is visible
[ ] Failure reasons are classified
[ ] Critical security events are logged
[ ] Secrets are excluded
[ ] Diagnostic API is authorized
[ ] Diagnostic CLI is safe
[ ] Support bundle is safe
[ ] Crash/restart behavior is diagnosable
[ ] Fault-injection tests verify observability
[ ] Hardware failures can be investigated
```

---

# 58. Final Principle

The diagnostic system should answer:

> **“What state was the machine in, what changed, why did it change, and did the system remain safe?”**

It should not merely answer:

> “Something went wrong.”

The most important diagnostic output is therefore not raw logs.

It is the combination of:

```text
STATE
+
EVENT
+
TRANSITION
+
SESSION
+
SECURITY EPOCH
+
COMPONENT HEALTH
+
FAILURE CODE
+
RECOVERY RESULT
```

The final diagnostic invariant is:

```text
If remote control is no longer trusted,
the diagnostics must make it possible to prove that
remote authority was revoked and the workstation reached
a safe state.
```

**Observability must help prove safety, not merely explain failure.**