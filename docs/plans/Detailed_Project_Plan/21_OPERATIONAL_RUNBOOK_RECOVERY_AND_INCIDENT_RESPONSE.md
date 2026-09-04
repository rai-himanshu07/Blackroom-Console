# 21_OPERATIONAL_RUNBOOK_RECOVERY_AND_INCIDENT_RESPONSE.md

## 1. Purpose

This document defines the operational procedures for running, recovering, disabling, upgrading, troubleshooting, and responding to security incidents involving the remote-access system.

The system controls an existing physical workstation and therefore operational mistakes can have consequences beyond a normal remote application.

The operational model must always prioritize:

1. revoke remote authority
2. protect the physical console
3. lock the GNOME session
4. restore physical display/input
5. verify safe state
6. investigate
7. recover normal operation

Never prioritize restoring remote connectivity over proving that the workstation is safe.

---

# 2. Operational Safety Principle

The most important operational rule is:

> **When in doubt, disable remote control and return the workstation to a locked local state.**

The preferred failure path is:

```text id="8a7p2k"
PROBLEM
   |
   v
REVOKE REMOTE AUTHORITY
   |
   v
LOCK SESSION
   |
   v
RESTORE PHYSICAL DISPLAY
   |
   v
RESTORE PHYSICAL INPUT
   |
   v
VERIFY
   |
   v
LOCAL_LOCKED
```

Only after this state is verified should troubleshooting continue.

---

# 3. Operational States

Administrators and diagnostics must distinguish:

```text id="2jz1xm"
LOCAL_ACTIVE
LOCAL_LOCKED
REMOTE_ACTIVE
REMOTE_DEGRADED
TEARING_DOWN
RECOVERING
EMERGENCY
FAILED_SAFE
DISABLED
INCOMPATIBLE
```

Do not describe all failures simply as:

> "Remote connection failed."

The operational state should identify what actually happened.

---

# 4. Normal Operating Model

Normal lifecycle:

```text id="l7w3n0"
LOCAL_ACTIVE
      |
      v
LOCAL_LOCKED
      |
      v
AUTHENTICATION
      |
      v
REMOTE PREPARATION
      |
      v
REMOTE_ACTIVE
      |
      v
DISCONNECT
      |
      v
RECOVERY
      |
      v
LOCAL_LOCKED
```

The remote session is temporary.

The local GNOME session remains the user's underlying workstation session.

---

# 5. Before Enabling Remote Access

Verify:

- Ubuntu version supported
- GNOME version supported
- Wayland active
- systemd operational
- required GNOME capabilities available
- PipeWire available
- virtual display capability available
- remote input capability available
- physical input isolation verified
- physical display isolation verified
- emergency controller functional
- authentication configured
- TOTP configured
- Remote Access Key configured
- recovery codes securely stored
- host identity known
- TLS/network configuration valid

Do not enable remote access if a safety-critical capability is unknown.

---

# 6. Initial Readiness Check

The system should expose a readiness check.

Conceptually:

```text id="q17p0a"
remote-access status
```

should report:

```text
Host:
  Identity: OK

Authentication:
  Password/PAM: OK
  TOTP: OK
  Remote Access Key: OK
  Recovery: OK

GNOME:
  Session: OK
  Wayland: OK
  Mutter: OK
  PipeWire: OK

Display:
  Virtual display: OK
  Physical isolation: OK
  Restoration: OK

Input:
  Remote input: OK
  Physical isolation: OK
  Restoration: OK

Emergency:
  Emergency controller: OK

Network:
  Gateway: OK
  TLS: OK
  Rendezvous: OK/Not configured
  TURN: OK/Not configured

Overall:
  READY
```

Any critical failure must result in:

```text
NOT_READY
```

rather than best-effort activation.

---

# 7. Normal Remote Connection Procedure

When the user wants to connect remotely:

1. confirm host is reachable
2. authenticate
3. verify current security policy
4. create authenticated session
5. issue control lease
6. verify GNOME session
7. snapshot display topology
8. create virtual display
9. prepare remote capture
10. disable physical outputs
11. isolate physical input
12. verify both
13. start media transport
14. verify remote input
15. enter `REMOTE_ACTIVE`

Do not report "connected" before safety preparation is complete.

---

# 8. Normal Disconnect Procedure

On intentional disconnect:

1. stop accepting new remote input
2. revoke control lease
3. terminate remote media
4. terminate remote session
5. lock GNOME
6. restore physical outputs
7. restore physical input
8. destroy temporary virtual display
9. verify state
10. return to `LOCAL_LOCKED`

The ordering must preserve the safety invariant.

---

# 9. Unexpected Network Failure

If network connectivity disappears:

1. detect failure
2. stop remote input according to lease/failure policy
3. transition to `REMOTE_DEGRADED`
4. attempt bounded recovery if appropriate
5. if recovery threshold is reached:
   - revoke authority
   - terminate remote session
   - lock session
   - restore display
   - restore input
   - verify
6. enter `LOCAL_LOCKED` or `FAILED_SAFE`

Do not wait indefinitely for the client to reconnect.

---

# 10. Client Browser Crash

If the browser crashes or the device loses power:

- host must detect session failure
- control lease must eventually expire/revoke
- remote authority must be terminated
- GNOME must be locked
- physical display restored
- physical input restored

A client crash must never leave the workstation permanently remotely controlled.

---

# 11. Gateway Failure

If the gateway fails:

Expected behavior:

```text id="3ghx1k"
Gateway failure
      |
      v
Existing remote session loses connectivity
      |
      v
Lease/failure policy
      |
      v
Safe teardown
```

The host must not depend on gateway availability to revoke authority.

The emergency controller must continue functioning.

---

# 12. Host Daemon Failure

If `remote-hostd` crashes:

1. systemd should restart it where appropriate
2. active remote authority must not automatically become valid again
3. existing leases must be invalidated if required
4. security epoch/session state must be reconciled
5. GNOME agent must enter safe recovery
6. physical display/input must be restored
7. session must be locked

After restart:

```text id="8n9t4w"
REMOTE_CONTROL = DISABLED
```

until a fresh authenticated session is established.

---

# 13. GNOME Agent Failure

If the GNOME session agent crashes:

1. host authority detects loss
2. remote control is revoked
3. remote session terminates
4. GNOME/session recovery is attempted
5. physical display is restored
6. physical input is restored
7. session is locked
8. state is verified

The host must not continue sending remote input to an unavailable or unknown agent.

---

# 14. PipeWire Failure

If PipeWire fails during remote mode:

```text id="q4b0yd"
capture unavailable
```

must not mean:

```text
remote authority remains active indefinitely
```

Depending on the defined recovery policy:

- attempt bounded media recovery
- otherwise revoke remote control
- lock
- restore physical state

Media availability and authorization must remain separate.

---

# 15. Mutter/GNOME Failure

If required Mutter functionality becomes unavailable:

- stop remote input
- revoke authority
- terminate remote session
- lock
- restore physical state
- enter recovery/failed-safe

Do not attempt increasingly invasive operations against a malfunctioning compositor.

---

# 16. Display Restoration Failure

This is a critical operational condition.

If physical displays cannot be restored:

1. remote authority must already be revoked
2. remote input must remain disabled
3. retry restoration within bounded limits
4. collect diagnostics
5. enter a known safe state
6. require local administrative intervention if necessary

Do not automatically reactivate remote access.

---

# 17. Physical Input Restoration Failure

If physical input cannot be restored:

1. revoke remote authority
2. lock session
3. retry restoration
4. verify device state
5. provide a clear diagnostic
6. require recovery action if necessary

The system must never resolve this by re-enabling remote control.

---

# 18. Emergency Takeover

The emergency shortcut is the primary local safety mechanism.

The intended behavior is:

```text id="l0fsby"
PHYSICAL EMERGENCY SHORTCUT
          |
          v
REVOKE REMOTE AUTHORITY
          |
          v
INVALIDATE CURRENT EPOCH
          |
          v
TERMINATE REMOTE SESSION
          |
          v
LOCK GNOME
          |
          v
RESTORE PHYSICAL DISPLAY
          |
          v
RESTORE PHYSICAL INPUT
          |
          v
VERIFY
          |
          v
LOCAL_LOCKED
```

The emergency action must not unlock the workstation.

The user must perform normal GNOME authentication.

---

# 19. Emergency Shortcut Requirements

The shortcut should be:

- configurable
- physically accessible
- difficult to trigger accidentally
- independent of browser
- independent of network
- independent of WebRTC
- independent of the main remote daemon

A hold duration may be used to reduce accidental activation.

The default shortcut must be documented clearly during setup.

---

# 20. Emergency Procedure for the User

If the user suspects that remote control is behaving incorrectly:

1. trigger the emergency shortcut
2. wait for the recovery indication
3. verify physical display is restored
4. verify physical keyboard/mouse are working
5. perform normal GNOME unlock
6. inspect connection/session status
7. rotate credentials if compromise is suspected
8. review security events

Do not attempt to continue the remote session merely because the browser still appears connected.

---

# 21. Emergency When Main Software Is Frozen

If the browser or remote application appears frozen:

- use the physical emergency shortcut
- do not depend on the browser UI
- do not depend on `remote-hostd`
- do not depend on the network

The emergency path must remain operational.

---

# 22. Suspected Credential Compromise

If any of the following may have been exposed:

- password
- TOTP secret
- Remote Access Key
- trusted-device credential
- recovery codes
- host private key

immediately:

1. trigger emergency takeover if remote access is active
2. disable remote access
3. revoke active sessions
4. increment/invalidate security epoch
5. revoke affected trusted devices
6. rotate affected credentials
7. regenerate Remote Access Key if required
8. regenerate TOTP if its secret is compromised
9. regenerate recovery codes if required
10. inspect security logs
11. re-enable remote access only after verification

---

# 23. Suspected Stolen Trusted Device

If a trusted laptop/browser/device is lost:

1. disable/revoke its trusted credential
2. terminate its active sessions
3. invalidate affected sessions/epoch if appropriate
4. verify remaining trusted devices
5. optionally rotate broader credentials
6. keep remote access disabled until verification if compromise is suspected

The user must not need physical access to the workstation merely to revoke a remote trusted device.

---

# 24. Suspected Remote Access Key Compromise

If the Remote Access Key is suspected compromised:

1. disable remote access if necessary
2. revoke active remote sessions
3. rotate the Remote Access Key
4. invalidate affected session credentials
5. verify trusted devices
6. verify TOTP state
7. inspect security events
8. re-enable only after verification

The old key must immediately cease to authorize new-device authentication.

---

# 25. Suspected TOTP Secret Compromise

If the TOTP secret is compromised:

1. revoke active remote sessions
2. invalidate current remote authority
3. disable remote access
4. replace TOTP secret
5. regenerate recovery codes
6. verify authentication
7. re-enable remote access

Do not rely on the compromised TOTP secret remaining secret.

---

# 26. Suspected Host Compromise

If the host operating system itself is suspected compromised:

Do not trust the remote-access application to prove its own integrity.

Procedure:

1. trigger emergency if possible
2. disable remote access
3. isolate the workstation/network as appropriate
4. preserve relevant diagnostics
5. investigate system integrity
6. rotate credentials from a trusted device
7. reinstall/recover the host if required
8. regenerate host identity where appropriate
9. regenerate Remote Access Key
10. regenerate TOTP/recovery credentials where necessary
11. revalidate the environment
12. only then re-enable remote access

A compromised host cannot be made trustworthy merely by restarting the remote service.

---

# 27. Security Incident Timeline

Security events should support reconstruction of:

```text id="3by7s1"
authentication attempt
→ authentication success/failure
→ session creation
→ lease issuance
→ remote activation
→ input authority
→ network degradation
→ disconnect/failure
→ revocation
→ emergency
→ epoch change
→ restoration
```

Use monotonic timing for local ordering where possible and wall-clock timestamps for human correlation.

---

# 28. Incident Evidence

Collect:

- host state
- session ID
- client ID
- host ID
- authentication result
- lease state
- security epoch
- state transitions
- component versions
- systemd status
- relevant journal entries
- GNOME/Mutter diagnostics
- PipeWire diagnostics
- network information
- display topology
- input topology
- emergency events

Do not collect or export:

- passwords
- TOTP secrets
- Remote Access Keys
- recovery codes
- private keys
- valid session credentials

---

# 29. Security Log Handling

Security logs should be:

- structured
- timestamped
- redacted
- rate limited
- rotated
- access controlled

Security events should identify:

- event type
- result
- session/client identifier where appropriate
- reason
- component
- timestamp

Do not log raw authentication material.

---

# 30. Remote Session Investigation

For a suspicious session, answer:

1. Which client connected?
2. Which host identity was targeted?
3. When did authentication succeed?
4. Which authentication factors were used?
5. Was the device trusted?
6. Which session was created?
7. Which lease was issued?
8. Which security epoch was active?
9. When did remote mode become active?
10. When was authority revoked?
11. Was an emergency action triggered?
12. Was the physical display restored?
13. Was physical input restored?
14. Did the session return to `LOCAL_LOCKED`?

---

# 31. Remote Session Kill Procedure

Provide an administrative/local mechanism to terminate active remote access.

Conceptually:

```text
remote-access sessions
```

and:

```text
remote-access revoke-session <session>
```

or equivalent implementation-specific interfaces.

The exact CLI/API is up to the repository architecture.

The important requirements are:

- explicit authorization
- idempotent behavior
- session revocation
- lease revocation
- no arbitrary command execution
- audit event
- safe physical recovery

---

# 32. Revoke-All Procedure

Provide a secure mechanism to revoke all remote sessions.

Expected:

1. increment/invalidate security epoch
2. revoke all active leases
3. terminate remote sessions
4. lock GNOME
5. restore physical display
6. restore physical input
7. verify safe state

Afterward:

```text id="lry9ut"
REMOTE_ACCESS = disabled or requires fresh authentication
```

according to policy.

---

# 33. Disable Remote Access

The system must support explicit remote-access disablement.

Disabling remote access should:

- prevent new authentication
- terminate active sessions
- revoke leases
- invalidate appropriate session authority
- preserve local workstation functionality
- leave the workstation safe

Disablement must not require uninstalling the application.

---

# 34. Safe Mode

Provide a local safe mode where appropriate.

Safe mode should:

- disable remote access
- revoke sessions
- invalidate leases
- lock session
- restore physical display
- restore physical input
- prevent automatic remote activation

This is useful when investigating compatibility or recovery problems.

---

# 35. Recovery After Failed Upgrade

If an upgrade fails:

1. do not reactivate remote access automatically
2. verify service versions
3. verify configuration
4. verify security state
5. verify GNOME compatibility
6. verify emergency controller
7. verify display/input recovery
8. run readiness check
9. only re-enable remote access after validation

If rollback is required:

- preserve security state safely
- invalidate incompatible session credentials
- verify service versions
- rerun compatibility checks

---

# 36. Recovery After Power Loss

After power loss:

1. boot normally
2. verify system services
3. verify GNOME session
4. verify display topology
5. verify physical input
6. verify remote-access state
7. verify emergency controller
8. verify security epoch/session state
9. ensure no remote session is active
10. require fresh authentication for future remote access

Expected default:

```text
LOCAL_LOCKED
```

or another explicitly documented safe state.

---

# 37. Recovery After Kernel/GPU Update

If a kernel or GPU driver changes:

1. detect environment change
2. mark compatibility for review
3. run compatibility check
4. test virtual display
5. test physical display isolation
6. test physical input isolation
7. test teardown
8. test emergency
9. test restoration

Do not automatically assume previous validation remains valid.

---

# 38. Recovery After GNOME Update

After a GNOME/Mutter update:

1. detect version change
2. disable remote access until compatibility validation if required by policy
3. run capability detection
4. test virtual display
5. test input
6. test display isolation
7. test locking
8. test teardown
9. test emergency
10. run full readiness check

Only then return to supported operation.

---

# 39. Manual Recovery When Services Are Unavailable

The documentation must provide local administrator procedures for:

- stopping remote services
- disabling remote access
- restoring display configuration
- restoring input devices
- locking GNOME
- checking service state
- collecting diagnostics

These procedures must be explicit and safe.

Avoid recommending arbitrary shell commands when a dedicated administrative operation can provide the same result.

---

# 40. Recovery Command Design

If a CLI is provided, commands should be narrowly scoped.

Good examples:

```text id="x4yq8f"
status
doctor
sessions
revoke-session
revoke-all
disable
enable
emergency-status
compatibility
diagnostics
```

Avoid generic interfaces such as:

```text id="8q7j1r"
run-command
exec
shell
dbus-proxy
```

The management interface must not become a privileged command-execution framework.

---

# 41. Operational Troubleshooting Order

When remote access fails, investigate in this order:

```text id="v44j2z"
1. Is host reachable?
2. Is authentication working?
3. Is host authorization working?
4. Is a session created?
5. Is a control lease valid?
6. Is security epoch current?
7. Is GNOME session available?
8. Is virtual display available?
9. Are physical outputs safely isolated?
10. Is physical input safely isolated?
11. Is PipeWire working?
12. Is WebRTC working?
13. Is browser input working?
14. Is teardown/recovery healthy?
```

Do not immediately modify GNOME or system configuration before determining which layer failed.

---

# 42. Diagnostic Decision Tree

### Cannot connect

Check:

```text
network
→ TLS
→ host discovery
→ gateway
→ authentication
```

### Authentication fails

Check:

```text
username/password
→ TOTP
→ device trust/access key
→ rate limiting
→ account state
```

### Authentication succeeds but remote mode does not start

Check:

```text
session
→ capability
→ lease
→ virtual display
→ display isolation
→ input isolation
```

### Remote display works but input does not

Check:

```text
control lease
→ browser input
→ WebRTC data channel
→ libei/EIS
→ GNOME input path
```

### Input works but physical input is not isolated

Treat as:

```text
CRITICAL
```

Remote mode must not remain active.

### Disconnect leaves screen/input wrong

Treat as a recovery failure.

Immediately:

```text
revoke
→ lock
→ restore
→ verify
```

---

# 43. Operational Metrics

Monitor:

- active remote sessions
- authentication failures
- lease expirations
- session failures
- recovery failures
- emergency activations
- display restoration failures
- input restoration failures
- component crashes
- watchdog restarts
- compatibility failures
- credential revocations

Metrics must not contain secrets.

---

# 44. Maintenance Windows

For planned maintenance:

1. disable new remote sessions
2. terminate active sessions
3. revoke leases
4. lock session
5. restore physical display/input
6. verify safe state
7. perform maintenance
8. run compatibility/readiness checks
9. re-enable remote access explicitly

Do not upgrade the system while remote control is active unless the upgrade procedure explicitly guarantees safe behavior.

---

# 45. Planned Credential Rotation

Credential rotation should follow:

```text id="w2l6bc"
prepare replacement
→ validate replacement
→ revoke old authority
→ increment/invalidate affected sessions
→ terminate old sessions
→ verify
```

Do not create a period where both old and new credentials have broader authority than intended.

---

# 46. Trusted Device Maintenance

Regularly review trusted devices.

For each device record:

- device identifier
- user
- created time
- last-used time if retained
- status
- revocation state

Remove devices that are:

- unused
- lost
- compromised
- no longer needed

Do not expose sensitive credential material through the management UI.

---

# 47. Host Identity Changes

If host identity/private key is regenerated:

- invalidate assumptions about the previous host
- require explicit client verification
- invalidate sessions where appropriate
- warn about identity change
- never silently trust the new identity

This prevents a host identity reset from becoming an impersonation opportunity.

---

# 48. Remote Access Disablement During Incident

If compromise is suspected, the fastest safe operational response is:

```text id="gjx9ma"
DISABLE REMOTE ACCESS
        |
        v
REVOKE ALL
        |
        v
SECURITY EPOCH INVALIDATION
        |
        v
LOCK
        |
        v
RESTORE PHYSICAL STATE
        |
        v
INVESTIGATE
```

Do not require the administrator to diagnose the root cause before revoking authority.

---

# 49. Recovery Verification Checklist

Before declaring recovery complete:

### Security

- [ ] No active unauthorized sessions
- [ ] All stale leases invalid
- [ ] Current security epoch verified
- [ ] Compromised credentials revoked
- [ ] Remote access disabled if necessary

### GNOME

- [ ] Correct session identified
- [ ] Session locked
- [ ] No unexpected remote session
- [ ] GNOME/Mutter healthy

### Display

- [ ] Physical displays restored
- [ ] Original topology restored where possible
- [ ] No remote content exposed

### Input

- [ ] Physical keyboard works
- [ ] Physical mouse works
- [ ] No remote input accepted

### Services

- [ ] host daemon healthy
- [ ] gateway healthy if enabled
- [ ] GNOME agent healthy
- [ ] emergency daemon healthy

### Compatibility

- [ ] Environment supported
- [ ] Required capabilities verified
- [ ] No unresolved critical compatibility issue

Only after all required checks pass should remote access be re-enabled.

---

# 50. Operational Recovery Levels

Use:

## LEVEL 0 — Normal

Everything healthy.

## LEVEL 1 — Degraded

Remote functionality degraded but safety invariants intact.

## LEVEL 2 — Recovery

Remote authority revoked; system restoring safe state.

## LEVEL 3 — Emergency

Local emergency takeover activated.

## LEVEL 4 — Failed Safe

Remote authority revoked and system is in a known safe state, but some convenience functionality requires intervention.

## LEVEL 5 — Security Incident

Possible compromise of credentials, host, or remote infrastructure.

---

# 51. Incident Severity

### CRITICAL

- unauthorized remote control
- physical input bypass
- physical display privacy bypass
- emergency takeover failure
- privilege escalation
- stale session successfully reactivated

### HIGH

- credential compromise
- trusted-device compromise
- security epoch failure
- unsafe recovery
- significant IPC vulnerability

### MEDIUM

- denial of service
- metadata leakage
- diagnostic exposure without credentials

### LOW

- operational inconvenience
- minor logging/UX issue

---

# 52. Incident Response Procedure

For a security incident:

```text id="n88prc"
1. CONTAIN
2. REVOKE
3. LOCK
4. RESTORE
5. VERIFY
6. PRESERVE EVIDENCE
7. INVESTIGATE
8. ROTATE CREDENTIALS
9. PATCH
10. TEST
11. REVALIDATE
12. RE-ENABLE EXPLICITLY
```

Never skip containment because the incident appears minor.

---

# 53. Post-Incident Review

Every significant security incident should produce:

- timeline
- root cause
- affected versions
- affected environments
- attacker capability
- violated invariant
- detection method
- containment method
- recovery result
- corrective action
- regression test
- compatibility impact
- release impact

If the incident exposed a missing invariant, update the relevant design document.

---

# 54. Operational Documentation Requirements

The project documentation must contain:

- installation guide
- first-run guide
- normal connection guide
- emergency procedure
- credential recovery procedure
- trusted-device revocation
- Remote Access Key rotation
- TOTP rotation
- remote-access disablement
- upgrade procedure
- rollback procedure
- troubleshooting
- diagnostics
- security incident procedure
- compatibility recovery

These procedures must remain synchronized with the actual implementation.

Do not document commands or behavior that the software does not implement.

---

# 55. Copilot Agent Instructions

GitHub Copilot Agent must:

1. Inspect the repository.
2. Inspect the workflow/configuration created by `adaptive-workflow-configurator`.
3. Respect the existing workflow.
4. Read Documents 1–20.
5. Identify existing operational interfaces.
6. Reuse existing state/diagnostic infrastructure.
7. Avoid creating duplicate administrative interfaces.
8. Implement explicit safe recovery operations.
9. Ensure emergency operations remain independent.
10. Ensure management interfaces cannot become generic privileged execution mechanisms.
11. Add tests for every documented recovery procedure.
12. Keep documentation synchronized with actual behavior.

---

# 56. Copilot Implementation Order

Implement operational capabilities in this order:

```text id="w7tx9q"
1. status/readiness
2. state visibility
3. active session visibility
4. session revocation
5. revoke-all
6. remote-access disablement
7. emergency status
8. compatibility/doctor
9. diagnostics
10. recovery orchestration
11. credential rotation workflows
12. trusted-device management
13. upgrade recovery
14. incident evidence collection
15. operational documentation
16. recovery regression tests
```

---

# 57. Copilot Stop Conditions

Stop and report rather than inventing an operational workaround if:

- a recovery action requires arbitrary shell execution
- emergency depends on the main remote stack
- revocation cannot be guaranteed
- stale sessions can survive recovery
- physical display restoration cannot be verified
- physical input restoration cannot be verified
- the system cannot establish a known safe state
- an upgrade can leave remote authority ambiguous
- credentials cannot be safely rotated
- diagnostics expose secrets
- a documented operational procedure does not match implementation behavior

---

# 58. Definition of Done

This document is implemented when:

- readiness/status exists
- active sessions can be inspected
- sessions can be revoked
- revoke-all exists
- remote access can be disabled
- emergency state is observable
- compatibility state is observable
- diagnostics are available
- safe recovery is bounded
- credential rotation is supported
- trusted-device revocation is supported
- upgrade recovery is documented
- incident response is documented
- security events support investigation
- recovery procedures have automated/manual tests
- documentation matches actual implementation

---

# 59. Final Operational Invariant

Every operational procedure must answer:

```text id="8n1s5u"
How do we stop remote control?
How do we lock the session?
How do we restore physical display?
How do we restore physical input?
How do we invalidate stale authority?
How do we verify the result?
```

If an operational procedure cannot answer these questions, it is incomplete.

The preferred operational response is always:

```text id="b3e9qs"
STOP
  ↓
REVOKE
  ↓
LOCK
  ↓
RESTORE
  ↓
VERIFY
  ↓
INVESTIGATE
  ↓
RECOVER
```

The system must make the safe path easier than the unsafe path.

---

# 60. Final Principle

The product is not complete when remote access works.

It is complete when an administrator or user can confidently answer:

> **"Something went wrong. How do I make this workstation safe?"**

The answer must be deterministic, documented, testable, and independent of the failing remote component.

The ultimate operational guarantee is:

> **No operational failure, maintenance operation, credential incident, compatibility change, or recovery procedure may leave the workstation with ambiguous remote authority.**