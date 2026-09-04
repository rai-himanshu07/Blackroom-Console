# 18_THREAT_DRIVEN_SECURITY_TESTING_AND_RED_TEAM_PLAN.md

## 1. Purpose

This document defines the adversarial security-testing and red-team program for the remote-access system.

The purpose is not merely to verify that expected functionality works. The purpose is to deliberately attempt to:

- bypass authentication
- bypass authorization
- reuse expired credentials
- replay stale sessions
- steal or misuse trusted-device credentials
- defeat the control lease
- defeat the security epoch
- reconnect after emergency takeover
- inject remote input after authority has been revoked
- expose the physical display
- regain physical input during remote control
- abuse local IPC
- escalate privileges
- abuse D-Bus access
- compromise the gateway
- exploit browser-side weaknesses
- exploit race conditions
- cause crashes during critical state transitions
- leave the host in an unsafe partially transitioned state
- prevent safe recovery

The red-team program must treat the following as the highest-priority security invariants:

1. **No valid authorization → no remote control.**
2. **No valid control lease → no remote input.**
3. **Stale security epoch → no remote control.**
4. **Emergency takeover → current remote authority becomes invalid.**
5. **Remote disconnect/failure → remote input is revoked.**
6. **Remote disconnect/failure → session becomes locked.**
7. **Remote disconnect/failure → physical display is restored.**
8. **Remote disconnect/failure → physical input is restored.**
9. **Remote control must never become permanent authority.**
10. **The emergency path must remain usable even if the main remote stack fails.**

This document complements:

- Document 9 — Threat Model, Security Boundaries & Abuse Cases
- Document 12 — Testing Strategy & Test Matrix

Document 9 defines **what can go wrong**.

Document 12 defines **how the overall system is tested**.

This document defines **how an attacker deliberately tries to make those failures happen**.

---

# 2. Red-Team Principles

## 2.1 Assume credentials eventually leak

Tests must assume an attacker may obtain:

- username
- password
- TOTP secret
- current session credential
- expired session credential
- Remote Access Key
- trusted-device credential
- browser cookies
- WebRTC signalling information
- host identity
- client identity

The system must remain safe according to the credential's intended authority.

Possession of one credential must not automatically imply unrestricted remote control.

---

## 2.2 Assume the network is hostile

The attacker may:

- observe traffic
- modify traffic
- replay traffic
- delay traffic
- duplicate traffic
- terminate connections
- reorder messages
- create concurrent connections
- impersonate peers
- flood endpoints
- manipulate DNS/mDNS
- operate a malicious relay
- operate a malicious client

All network-facing security decisions must therefore be independently enforced by the host security authority.

---

## 2.3 Assume the browser is hostile

The browser/client must be treated as an untrusted remote endpoint.

Never trust the browser to enforce:

- authorization
- lease expiration
- security epoch
- session ownership
- emergency revocation
- physical safety state

The host must enforce these conditions.

---

## 2.4 Assume components can fail during critical operations

Red-team testing must deliberately terminate:

- remote gateway
- host daemon
- GNOME session agent
- PipeWire connection
- WebRTC connection
- authentication connection
- control channel
- emergency daemon
- relevant helper processes

The attacker/test harness must be able to kill processes at arbitrary points in state transitions.

---

# 3. Security Severity Classification

Use the following severity levels.

### CRITICAL

An attack can:

- obtain unauthorized remote control
- bypass mandatory authentication
- bypass TOTP
- bypass Remote Access Key requirements
- maintain remote input after revocation
- reconnect after emergency invalidation
- defeat physical display privacy
- defeat physical input isolation
- prevent emergency takeover
- obtain privileged host execution
- permanently corrupt security state

A CRITICAL finding blocks release.

### HIGH

An attack can:

- materially weaken authentication
- hijack a valid session
- bypass authorization under realistic conditions
- cause unsafe recovery
- cause repeated unsafe state transitions
- expose sensitive secrets
- compromise trusted-device security
- exploit privileged IPC
- reliably defeat a security boundary

A HIGH finding blocks release unless explicitly reviewed and accepted.

### MEDIUM

An attack can:

- cause significant denial of service
- expose non-critical metadata
- degrade security controls without directly bypassing them
- cause recoverable operational problems

### LOW

Minor security weaknesses with limited practical impact.

### INFORMATIONAL

Hardening opportunities or defense-in-depth observations.

---

# 4. Red-Team Environment

Maintain a dedicated security-test environment.

The test environment should contain:

- Ubuntu 26.04 LTS
- supported GNOME version
- Wayland
- PipeWire
- supported GPU configuration
- at least one physical display
- physical keyboard
- physical mouse
- separate remote client
- browser
- LAN connectivity
- optional Internet/NAT environment

Where possible, use a separate test machine rather than the developer's primary workstation.

Never perform destructive red-team tests against a production workstation containing irreplaceable data.

---

# 5. Test Harness

Build a reusable adversarial test harness.

It should be able to:

- start/stop services
- kill processes
- terminate connections
- inject malformed protocol messages
- replay captured messages
- delay messages
- duplicate messages
- reorder messages
- create concurrent sessions
- manipulate system clock where safe
- simulate network loss
- simulate network latency
- simulate packet loss
- revoke credentials
- modify test configuration
- inspect resulting security state
- verify display topology
- verify input routing
- verify GNOME lock state
- collect logs
- collect state-transition traces
- collect security events
- produce machine-readable test results

The harness must never bypass the application's security mechanisms merely to make tests easier.

---

# 6. Attack Surface Inventory

The red-team program must cover:

```text
Internet / LAN
      |
      v
Gateway
      |
      v
Authentication
      |
      v
Host Security Authority
      |
      +--> Session Credential
      |
      +--> Control Lease
      |
      +--> Security Epoch
      |
      v
GNOME Session Agent
      |
      +--> Mutter
      +--> PipeWire
      +--> libei/EIS
      +--> DisplayConfig
      |
      v
Physical Display / Input

Emergency Controller
      |
      +--> Host Security Authority
      +--> GNOME/session recovery
```

Additional surfaces:

- configuration files
- secret storage
- systemd
- D-Bus
- Unix sockets
- browser storage
- TLS
- WebSocket
- WebRTC signalling
- STUN/TURN
- rendezvous
- mDNS/Avahi
- package installation
- upgrade process
- diagnostic interfaces
- logs
- crash dumps
- temporary files

---

# 7. Authentication Red-Team Tests

## RT-AUTH-001 — Missing Password

Attempt authentication with:

- valid username
- missing password
- valid TOTP
- valid Remote Access Key

Expected:

- authentication rejected
- no session created
- no control lease
- no GNOME state change

Severity if bypassed: CRITICAL.

---

## RT-AUTH-002 — Invalid Password

Attempt:

- valid username
- incorrect password
- valid TOTP
- valid Remote Access Key

Expected:

- rejected
- rate limiting applied
- no partial authentication state usable for control

---

## RT-AUTH-003 — Missing TOTP

Attempt:

- valid username
- valid password
- valid Remote Access Key
- no TOTP

Expected:

- rejected

A valid password must never substitute for TOTP.

Severity if bypassed: CRITICAL.

---

## RT-AUTH-004 — Invalid TOTP

Attempt:

- valid username
- valid password
- invalid TOTP
- valid Remote Access Key

Expected:

- rejected
- no session

---

## RT-AUTH-005 — Reused TOTP

Attempt to reuse a recently accepted TOTP where replay prevention is implemented.

Expected:

- behavior follows documented TOTP replay policy
- replay must not produce additional authorization beyond intended semantics

---

## RT-AUTH-006 — Missing Remote Access Key for New Device

Attempt new-device authentication using only:

- username
- password
- valid TOTP

Expected:

- rejected

Severity if bypassed: CRITICAL.

---

## RT-AUTH-007 — Invalid Remote Access Key

Expected:

- rejected
- rate limiting
- no session
- no lease

---

## RT-AUTH-008 — Trusted Device Without TOTP

Attempt trusted-device authentication while omitting TOTP.

Expected:

- rejected

Trusted status must never bypass mandatory TOTP.

---

## RT-AUTH-009 — Stolen Trusted Credential

Use a copied trusted-device credential.

Expected:

- behavior must match documented trust model
- password and TOTP remain mandatory
- credential can be revoked
- revocation takes effect according to documented semantics

---

## RT-AUTH-010 — Revoked Trusted Device

Authenticate with a previously revoked trusted device.

Expected:

- rejected
- no session
- no lease

---

## RT-AUTH-011 — Revoked Remote Access Key

Use an old access key after rotation/revocation.

Expected:

- rejected

---

## RT-AUTH-012 — Recovery Code Abuse

Attempt:

- valid username
- valid password
- recovery code
- no Remote Access Key

Expected:

- new/untrusted device authentication rejected

Recovery must not accidentally become a Remote Access Key bypass.

---

## RT-AUTH-013 — Username Enumeration

Attempt to distinguish:

- nonexistent username
- existing username

through:

- response body
- status
- timing
- error message

Expected:

- no unnecessary account enumeration

---

## RT-AUTH-014 — Authentication Flooding

Generate large numbers of authentication attempts.

Expected:

- rate limiting
- bounded resource consumption
- no service crash
- no bypass caused by concurrent attempts

---

## RT-AUTH-015 — Concurrent Authentication Race

Submit multiple authentication attempts simultaneously with different credentials.

Expected:

- state remains consistent
- no partially authenticated connection becomes authorized
- no session confusion

---

# 8. Session Credential Attacks

## RT-SESSION-001 — Expired Session Credential

Use an expired session credential.

Expected:

- rejected

---

## RT-SESSION-002 — Wrong Host

Use a valid credential issued for Host A against Host B.

Expected:

- rejected

---

## RT-SESSION-003 — Wrong Client

Use a credential bound to a different client/session context where binding is required.

Expected:

- rejected

---

## RT-SESSION-004 — Session Replay

Capture a valid session credential and replay it after:

- disconnect
- expiration
- explicit revocation
- security epoch increment

Expected:

- rejected

---

## RT-SESSION-005 — Session Credential Reuse

Attempt to use one session credential concurrently from multiple clients.

Expected:

- behavior follows explicit multi-client policy
- unauthorized additional control must not occur

---

# 9. Control Lease Attacks

## RT-LEASE-001 — No Lease, Send Input

Authenticate successfully but do not obtain a valid control lease.

Attempt:

- keyboard input
- pointer movement
- pointer clicks

Expected:

- all remote input rejected

Severity if bypassed: CRITICAL.

---

## RT-LEASE-002 — Expired Lease

Allow lease to expire and continue sending input.

Expected:

- input stops immediately

---

## RT-LEASE-003 — Lease From Previous Session

Reuse a previous lease after disconnect/reconnect.

Expected:

- rejected

---

## RT-LEASE-004 — Wrong Session ID

Use a valid-looking lease with a different session ID.

Expected:

- rejected

---

## RT-LEASE-005 — Wrong Security Epoch

Use a lease issued before the current epoch.

Expected:

- rejected

Severity if bypassed: CRITICAL.

---

## RT-LEASE-006 — Lease Renewal After Revocation

Attempt lease renewal after:

- disconnect
- emergency
- global revoke
- security epoch increment

Expected:

- rejected

---

## RT-LEASE-007 — Lease Expiration Race

Send input exactly around expiration.

Expected:

- host performs authoritative validation
- stale input cannot survive expiration

---

# 10. Security Epoch Attacks

## RT-EPOCH-001 — Stale Session After Epoch Increment

Create valid remote session.

Increment epoch.

Continue sending commands.

Expected:

- all authority from previous epoch rejected

---

## RT-EPOCH-002 — Reconnect With Stale Credential

After emergency takeover:

- reconnect browser
- reuse old session credential
- reuse old lease

Expected:

- rejected

---

## RT-EPOCH-003 — Concurrent Emergency Race

Run simultaneously:

- remote input
- lease renewal
- session reconnect
- emergency takeover

Expected:

- emergency wins
- old authority becomes invalid
- no remote input remains active

---

## RT-EPOCH-004 — Epoch Persistence

Restart host daemon after epoch increment.

Attempt old credential.

Expected:

- old authority remains invalid

The epoch must not accidentally reset to a value that revives stale authority.

---

# 11. Emergency Controller Attacks

## RT-EMERGENCY-001 — Emergency During Remote Active

Trigger emergency while remote session is fully active.

Expected order:

1. revoke remote authority
2. terminate session
3. increment security epoch
4. lock GNOME session
5. restore physical display
6. restore physical input
7. remain locked

---

## RT-EMERGENCY-002 — Emergency During Preparation

Trigger emergency while virtual display/input isolation is being configured.

Expected:

- transition aborts safely
- remote authority revoked
- physical state restored
- session locked

---

## RT-EMERGENCY-003 — Emergency During Teardown

Trigger emergency while disconnect cleanup is already running.

Expected:

- operations are idempotent
- final state is safe

---

## RT-EMERGENCY-004 — Main Daemon Hung

Intentionally hang the main remote daemon.

Trigger physical emergency shortcut.

Expected:

- emergency path still works independently

This is a CRITICAL test.

---

## RT-EMERGENCY-005 — Gateway Dead

Stop gateway.

Trigger emergency.

Expected:

- emergency still works

---

## RT-EMERGENCY-006 — Browser Maliciously Reconnects

Immediately reconnect browser after emergency using stale credentials.

Expected:

- rejected

---

## RT-EMERGENCY-007 — Repeated Emergency

Trigger emergency multiple times.

Expected:

- no crash
- no inconsistent display/input state
- final state remains locked and locally recoverable

---

# 12. Physical Display Privacy Attacks

## RT-DISPLAY-001 — Remote Active Verification

While remote control is active:

- inspect physical outputs
- inspect active display topology
- physically observe monitor

Expected:

- physical output is disabled according to documented implementation
- remote virtual display remains available

---

## RT-DISPLAY-002 — Display Re-enable Attempt

Attempt to restore physical output while remote session is active.

Expected:

- policy prevents unauthorized restoration
- physical display does not expose remote activity

---

## RT-DISPLAY-003 — Hotplug During Remote Session

Connect/disconnect physical display.

Expected:

- privacy invariant remains intact
- no accidental exposure

---

## RT-DISPLAY-004 — GNOME/Mutter Failure During Isolation

Cause failure while physical output is being disabled.

Expected:

- system transitions to safe recovery
- no uncontrolled remote authority
- physical display state is restored or otherwise proven safe

---

## RT-DISPLAY-005 — Teardown Failure

Cause failure while restoring display topology.

Expected:

- recovery retries safely
- final state does not silently remain remotely controlled

---

# 13. Physical Input Isolation Attacks

This is one of the highest-priority red-team areas.

## RT-INPUT-001 — Physical Keyboard During Remote Active

Press physical keys while remote control is active.

Expected:

- physical input cannot control the GNOME session

---

## RT-INPUT-002 — Physical Mouse During Remote Active

Move/click physical mouse.

Expected:

- physical input cannot control the remote-controlled session

---

## RT-INPUT-003 — Rapid Input Flood

Generate rapid physical input events.

Expected:

- no physical event reaches the session
- no race allows occasional events through

---

## RT-INPUT-004 — Device Hotplug

Disconnect/reconnect keyboard and mouse.

Expected:

- newly attached physical devices remain isolated while remote mode is active

---

## RT-INPUT-005 — Multiple Input Devices

Use multiple keyboards/mice.

Expected:

- no unintended physical input path remains

---

## RT-INPUT-006 — Input Isolation Teardown Race

Generate physical input during:

- disconnect
- emergency
- restoration

Expected:

- final state is locally usable
- no remote authority remains

---

## RT-INPUT-007 — Emergency Shortcut Must Still Work

Verify that physical input isolation does not prevent the emergency mechanism from receiving its trigger.

The emergency path must remain available while remote input is active.

---

# 14. Privilege Escalation Tests

## RT-PRIV-001 — Gateway Escape

Compromise/test gateway process.

Attempt to:

- access secrets
- control GNOME
- access `/dev/input`
- access host security state
- execute privileged commands

Expected:

- blocked by privilege separation

---

## RT-PRIV-002 — GNOME Agent Escape

Compromise/test user-session agent.

Attempt:

- root access
- arbitrary system command execution
- host security modification
- credential access

Expected:

- blocked by Unix/systemd boundaries

---

## RT-PRIV-003 — Host Daemon Command Injection

Send malformed or attacker-controlled strings intended to become shell commands.

Expected:

- no shell interpretation
- no arbitrary command execution

---

## RT-PRIV-004 — Arbitrary D-Bus Invocation

Attempt to use exposed D-Bus access to invoke unrelated privileged APIs.

Expected:

- rejected by policy

---

## RT-PRIV-005 — IPC Credential Confusion

Connect to local IPC using:

- wrong UID
- wrong group
- unauthorized process
- forged metadata

Expected:

- rejected

---

## RT-PRIV-006 — Privileged Helper Abuse

If an input/display helper exists, fuzz every exposed operation.

Expected:

- only explicitly supported operations are possible
- no arbitrary device/file access

---

# 15. File-System and Secret Attacks

## RT-FILE-001 — Secret File Permissions

Attempt access as:

- normal user
- gateway service user
- GNOME agent
- unrelated local user

Expected:

- only authorized component can access each secret

---

## RT-FILE-002 — Symlink Attack

Attempt to replace security-sensitive files with symlinks.

Expected:

- atomic/safe file handling prevents unintended writes

---

## RT-FILE-003 — Path Traversal

Submit paths containing:

- `../`
- absolute paths
- symlinks
- encoded traversal

Expected:

- rejected

---

## RT-FILE-004 — Configuration Injection

Place malicious configuration values into configuration files.

Expected:

- schema validation
- no arbitrary code execution
- safe failure

---

## RT-FILE-005 — Logs Containing Secrets

Search logs for:

- passwords
- TOTP secrets
- Remote Access Keys
- recovery codes
- session credentials
- trusted-device credentials

Expected:

- secrets never appear

---

# 16. Browser Security Tests

## RT-BROWSER-001 — XSS

Attempt script injection through:

- host name
- error message
- device name
- diagnostic data
- server responses

Expected:

- no executable injection

---

## RT-BROWSER-002 — CSRF

Attempt unauthorized state-changing requests from another origin.

Expected:

- rejected

---

## RT-BROWSER-003 — Clickjacking

Attempt to embed the remote console in another origin.

Expected:

- prevented according to documented browser security policy

---

## RT-BROWSER-004 — Malicious WebSocket Client

Connect without valid authentication.

Expected:

- no sensitive information
- no control commands
- no lease

---

## RT-BROWSER-005 — Malformed WebSocket Messages

Fuzz:

- message type
- length
- JSON/schema
- IDs
- enums
- nested structures
- unexpected fields

Expected:

- clean rejection
- no crash
- no state corruption

---

## RT-BROWSER-006 — Oversized Messages

Send extremely large messages.

Expected:

- bounded memory use
- request rejected

---

## RT-BROWSER-007 — Browser Refresh

Refresh during every major state.

Expected:

- stale browser state cannot regain control
- host remains authoritative

---

# 17. Network Attack Tests

## RT-NET-001 — TLS Downgrade

Attempt weaker transport/security configuration.

Expected:

- rejected

---

## RT-NET-002 — Man-in-the-Middle

Use a controlled interception environment.

Expected:

- certificate/identity verification prevents unauthorized host impersonation

---

## RT-NET-003 — Replay

Replay:

- authentication messages
- session messages
- lease requests
- control commands

Expected:

- replay rejected

---

## RT-NET-004 — Message Reordering

Reorder state-changing messages.

Expected:

- stale/out-of-order operations cannot produce unsafe state

---

## RT-NET-005 — Delayed Messages

Delay old commands until after:

- disconnect
- emergency
- epoch increment

Expected:

- stale commands rejected

---

## RT-NET-006 — Duplicate Messages

Duplicate:

- disconnect
- lease renewal
- activation
- teardown
- emergency-related requests

Expected:

- idempotent behavior

---

## RT-NET-007 — Connection Flood

Create many concurrent connections.

Expected:

- bounded resource consumption
- no watchdog starvation
- emergency path unaffected

---

# 18. Gateway Compromise Scenario

Simulate a completely compromised gateway.

Assume the attacker can:

- read gateway memory
- modify gateway responses
- terminate connections
- fabricate signalling messages
- observe metadata
- attempt to impersonate clients

Expected:

- gateway cannot directly obtain host privileges
- gateway cannot bypass host authentication
- gateway cannot manufacture valid control leases
- gateway cannot invoke arbitrary host operations
- gateway compromise does not automatically compromise host secrets

Where end-to-end encryption is part of the architecture, verify that the relay cannot decrypt remote desktop content.

---

# 19. Malicious Client Tests

A client that successfully authenticates should still be considered potentially malicious.

Attempt:

- malformed input
- excessive input rate
- invalid lease requests
- unauthorized state transitions
- concurrent sessions
- stale credentials
- oversized messages
- protocol fuzzing
- repeated reconnect
- rapid connect/disconnect
- resource exhaustion
- invalid display parameters
- unsupported capabilities

Expected:

- host remains safe
- state machine remains valid
- no privilege escalation
- no physical privacy violation

---

# 20. State-Machine Attack Testing

The state machine must be attacked as a security boundary.

For every state:

- attempt unauthorized transitions
- attempt transitions that skip authentication
- attempt transitions that skip lease creation
- attempt transitions using stale credentials
- attempt transitions after emergency
- attempt transitions during recovery
- attempt concurrent transitions

Examples:

```text
LOCAL_LOCKED
    -> REMOTE_ACTIVE
```

must never be possible without the required authentication and preparation sequence.

Likewise:

```text
REMOTE_ACTIVE
    -> LOCAL_ACTIVE
```

must never occur merely because a remote client disconnects.

The safe transition is:

```text
REMOTE_ACTIVE
    -> TEARING_DOWN
    -> RECOVERING
    -> LOCAL_LOCKED
```

---

# 21. Fault Injection During Critical Transactions

Kill components at every critical point.

Examples:

```text
AUTHENTICATING
AUTHENTICATED
PREPARING_REMOTE
VIRTUAL_DISPLAY_CREATED
PHYSICAL_OUTPUTS_DISABLED
PHYSICAL_INPUT_DISABLED
REMOTE_ACTIVE
REMOTE_DEGRADED
TEARING_DOWN
RESTORING_DISPLAY
RESTORING_INPUT
LOCKING
RECOVERING
```

For every kill point verify:

- remote authority is revoked
- stale lease cannot work
- session is eventually locked
- physical display is safe
- physical input is safe
- restoration is attempted
- recovery is idempotent

---

# 22. Race-Condition Campaign

Build explicit race tests for:

### Race A

```text
Emergency
vs
Lease Renewal
```

Emergency must win.

### Race B

```text
Disconnect
vs
Remote Input
```

No input may survive final revocation.

### Race C

```text
Reconnect
vs
Epoch Increment
```

Stale reconnect must fail.

### Race D

```text
Physical Hotplug
vs
Remote Activation
```

No physical display/input privacy violation.

### Race E

```text
GNOME Failure
vs
Emergency
```

Emergency must still reach a safe outcome.

### Race F

```text
Main Daemon Restart
vs
Existing Session
```

Old authority must not silently survive.

---

# 23. Time and Clock Attacks

Test:

- system clock moving backwards
- system clock moving forwards
- NTP adjustment
- TOTP boundary conditions
- session expiration
- lease expiration

Security-sensitive expiration must not rely solely on wall-clock time where monotonic time is appropriate.

Expected:

- expired authority does not revive
- future-dated credentials are rejected
- clock changes do not accidentally extend control leases

---

# 24. Resource Exhaustion Attacks

Attempt exhaustion through:

- authentication requests
- WebSocket connections
- WebRTC sessions
- malformed messages
- large messages
- rapid reconnect
- lease requests
- device registrations
- logging
- diagnostic requests

Verify:

- bounded memory
- bounded CPU
- bounded connection count
- bounded disk growth
- systemd limits where appropriate
- emergency daemon remains responsive

---

# 25. Persistence and Restart Attacks

Test after:

- host daemon restart
- gateway restart
- GNOME agent restart
- emergency daemon restart
- system reboot
- interrupted upgrade
- configuration migration
- power loss

Verify:

- stale sessions cannot regain control
- security epoch remains correct
- secrets remain protected
- host does not start remotely active accidentally
- physical display/input are safe
- user must perform normal unlock where required

---

# 26. Package and Upgrade Attacks

Attempt malicious or malformed package scenarios:

- modified package
- invalid signature
- dependency substitution
- interrupted installation
- interrupted upgrade
- failed migration
- downgrade
- configuration corruption

Expected:

- package authenticity is verified
- services do not start in unsafe states
- remote authority is not accidentally preserved across incompatible versions
- emergency recovery remains available

---

# 27. Diagnostic and Logging Attacks

Attempt to inject malicious data into:

- host names
- usernames
- device names
- error messages
- connection metadata
- protocol fields

Verify:

- log injection is prevented
- terminal/control characters are safely handled
- secrets are redacted
- diagnostic bundles do not contain credentials
- attacker-controlled strings cannot execute commands

---

# 28. Physical Attacker Scenarios

The following scenarios must be explicitly tested.

### Scenario 1 — Person at workstation during remote session

Expected:

- physical input cannot control session
- physical display cannot expose remote content

### Scenario 2 — Person discovers emergency shortcut

Expected:

- emergency takeover works
- remote authority is revoked
- system remains locked
- no automatic unlock

### Scenario 3 — Person reconnects keyboard/mouse

Expected:

- devices are still governed by local input-isolation policy until safe restoration.

### Scenario 4 — Person steals browser/device

Expected:

- security depends on the configured credential model
- stolen trusted credential cannot bypass mandatory password + TOTP
- revoked credentials cease to work

### Scenario 5 — Person obtains Remote Access Key

Expected:

- access key alone is insufficient
- password + TOTP remain mandatory

---

# 29. Security Invariant Verification

Create automated assertions for the following.

## INV-SEC-001

If no valid session credential exists:

```text
remote_input_allowed == false
```

## INV-SEC-002

If no valid control lease exists:

```text
remote_input_allowed == false
```

## INV-SEC-003

If lease epoch != current security epoch:

```text
remote_input_allowed == false
```

## INV-SEC-004

After emergency:

```text
remote_authority == revoked
```

## INV-SEC-005

After remote failure:

```text
session_locked == true
```

## INV-SEC-006

After remote failure:

```text
physical_input_restored == true
```

## INV-SEC-007

After remote failure:

```text
physical_display_safe == true
```

## INV-SEC-008

After emergency:

```text
old_session_cannot_reconnect == true
```

## INV-SEC-009

After host restart:

```text
remote_control == disabled_until_authenticated
```

## INV-SEC-010

Emergency must not depend on:

```text
Internet
Gateway
Browser
WebRTC
PipeWire
Main Remote Agent
```

---

# 30. Fuzzing Program

Implement fuzzing for:

- authentication messages
- protocol envelopes
- JSON/schema
- WebSocket frames
- IPC messages
- configuration
- display parameters
- capability negotiation
- device identifiers
- session identifiers
- lease identifiers
- malformed UTF-8
- oversized fields
- nested structures
- unexpected enum values
- duplicate fields
- missing fields

Fuzzing requirements:

- process must not crash
- privileged component must not execute unintended operations
- memory use must remain bounded
- malformed input must not change security state
- rejected requests must not create authority

Prioritize fuzzing of privileged components over the browser UI.

---

# 31. Penetration-Test Workflow

For each red-team campaign:

```text
1. Define attacker capability
2. Define target security boundary
3. Define expected defense
4. Execute attack
5. Capture evidence
6. Determine actual outcome
7. Compare with invariant
8. Assign severity
9. Reproduce
10. Create regression test
11. Fix
12. Re-run attack
13. Verify no regression
14. Document residual risk
```

Every confirmed vulnerability must become an automated regression test where practical.

---

# 32. Evidence Requirements

A red-team test is not complete merely because a command returned an error.

Capture sufficient evidence to prove the security property.

Useful evidence includes:

- state transition trace
- security epoch
- lease status
- session ID
- authentication result
- service status
- process status
- display topology
- physical output status
- input device status
- GNOME lock state
- relevant systemd journal entries
- security audit events
- network trace where appropriate
- browser console/network evidence
- resource usage
- timestamps
- test harness result

Never include real secrets in evidence.

---

# 33. Attack Result Format

Every red-team test should produce:

```text
Test ID:
Attack:
Attacker capability:
Target:
Preconditions:
Steps:
Expected result:
Actual result:
Security invariant tested:
Evidence:
Severity:
Reproducibility:
Automated:
Regression test:
Fix:
Retest result:
Residual risk:
```

---

# 34. Automated vs Manual Tests

Classify tests as:

### AUTOMATED

Use CI or repeatable test harnesses for:

- protocol attacks
- authentication attacks
- credential replay
- epoch attacks
- lease attacks
- malformed messages
- IPC authorization
- configuration validation
- state-machine races
- resource limits
- regression tests

### SYSTEM AUTOMATED

Run on dedicated Ubuntu/GNOME hardware:

- display isolation
- input isolation
- emergency controller
- session locking
- virtual monitor
- recovery
- GNOME/Mutter failures
- hardware hotplug

### MANUAL / PHYSICAL

Require human verification for:

- actual monitor privacy
- physical keyboard isolation
- physical mouse isolation
- emergency shortcut
- monitor power/blank behavior
- unusual GPU/display combinations

---

# 35. Red-Team Priority

Execute in this order.

## P0 — Must Pass Before Architecture Is Accepted

1. authentication bypass
2. TOTP bypass
3. Remote Access Key bypass
4. control lease bypass
5. security epoch bypass
6. stale-session replay
7. emergency invalidation
8. physical display privacy
9. physical input isolation
10. emergency independence
11. privilege escalation
12. unsafe recovery

Failure in any P0 test blocks the project.

## P1

- browser attacks
- protocol fuzzing
- IPC abuse
- resource exhaustion
- package/upgrade attacks
- race conditions

## P2

- metadata leakage
- diagnostic hardening
- advanced DoS
- unusual configuration attacks

---

# 36. Red-Team Acceptance Criteria

The security implementation is acceptable only when:

- all P0 tests pass
- no unresolved CRITICAL vulnerabilities exist
- no unresolved HIGH vulnerabilities exist without explicit security review
- stale credentials cannot regain control
- emergency invalidates existing authority
- physical input isolation is experimentally proven
- physical display privacy is experimentally proven
- privileged interfaces are narrowly scoped
- gateway compromise does not directly imply host compromise
- logs do not expose secrets
- security state survives restart
- fault injection results in safe recovery
- regression tests exist for discovered vulnerabilities
- repeated adversarial testing produces deterministic outcomes

---

# 37. Security Release Blockers

The following automatically block release:

- remote input possible without valid lease
- remote input possible with stale epoch
- emergency does not revoke existing remote authority
- stale browser can reconnect after emergency
- physical keyboard can control session while remote mode is active
- physical mouse can control session while remote mode is active
- physical display exposes remote activity
- emergency requires the main remote daemon
- authentication can be bypassed
- TOTP can be bypassed
- new-device Access Key requirement can be bypassed
- privileged IPC allows arbitrary command execution
- gateway can directly control privileged host functionality
- security secrets appear in logs
- crash during teardown leaves remote authority active
- host reboot unexpectedly restores remote authority without authentication

---

# 38. Copilot Agent Execution Instructions

GitHub Copilot Agent must treat this document as an adversarial validation specification.

Before implementing tests:

1. Inspect the repository.
2. Inspect the workflow/configuration created by `adaptive-workflow-configurator`.
3. Respect the existing workflow rather than replacing or duplicating it.
4. Inspect Documents 1–17.
5. Identify the implemented security boundaries.
6. Map each boundary to the relevant red-team tests.
7. Determine which tests can be automated safely.
8. Determine which tests require real Ubuntu/GNOME hardware.
9. Do not weaken production security merely to make a test pass.

Do not assume an implementation is secure because:

- authentication exists
- TLS exists
- tests pass
- a token is signed
- a service is running as a non-root user
- the browser hides a button
- the UI says "disconnected"

Security must be verified at the authoritative enforcement point.

---

# 39. Copilot Red-Team Implementation Order

Implement in this sequence:

```text
1. Security invariant assertions
2. State-machine adversarial tests
3. Authentication attack tests
4. Session credential tests
5. Control lease tests
6. Security epoch tests
7. Revocation tests
8. Emergency tests
9. IPC authorization tests
10. Privilege-boundary tests
11. Protocol fuzzing
12. Browser security tests
13. Network attack tests
14. Fault-injection tests
15. Race-condition tests
16. Resource-exhaustion tests
17. Physical display tests
18. Physical input tests
19. Hardware/GNOME failure tests
20. Persistence/restart tests
21. Package/upgrade tests
22. Full adversarial regression suite
```

Do not start with large-scale fuzzing before deterministic security invariants and state-machine tests exist.

---

# 40. Copilot Stop Conditions

Copilot must stop implementation and report findings rather than inventing a workaround if:

- a P0 security invariant cannot be enforced
- physical input isolation cannot be proven
- physical display privacy cannot be proven
- emergency cannot operate independently
- a privileged component requires unnecessarily broad privileges
- a protocol design allows stale authority to survive revocation
- GNOME/Mutter behavior contradicts a required security invariant
- a test requires weakening production security
- a security boundary is ambiguous
- an attack has an unresolved CRITICAL impact

The agent must clearly distinguish:

```text
IMPLEMENTED
VERIFIED
PARTIALLY VERIFIED
UNVERIFIED
FAILED
BLOCKED
```

Do not label an untested security property as verified.

---

# 41. Final Red-Team Principle

The project should be considered secure only when the system survives deliberate attempts to violate its most important invariants.

The goal is not:

> "The remote desktop works."

The goal is:

> "Even when credentials, network connections, components, browsers, and parts of the GNOME stack behave maliciously or fail unexpectedly, unauthorized remote control cannot persist and the physical workstation can reliably return to a safe locked local state."

The most important security property is therefore:

```text
REMOTE AUTHORITY
      |
      v
TEMPORARY
      |
      v
LEASE-BOUND
      |
      v
EPOCH-BOUND
      |
      v
SESSION-BOUND
      |
      v
REVOCABLE
      |
      v
FAIL-CLOSED
```

And the ultimate safety property is:

```text
ANY REMOTE FAILURE
        |
        v
REVOKE AUTHORITY
        |
        v
LOCK SESSION
        |
        v
RESTORE PHYSICAL STATE
        |
        v
VERIFY SAFE
        |
        v
LOCAL LOCKED
```

No convenience feature, performance optimization, browser behavior, network condition, or implementation shortcut may override this safety model.