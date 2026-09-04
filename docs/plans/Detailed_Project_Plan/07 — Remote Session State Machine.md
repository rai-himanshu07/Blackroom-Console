# 07 — Remote Session State Machine

## 1. Purpose

This document defines the authoritative runtime state machine for the remote-console system.

It specifies:

- runtime states
- legal state transitions
- transition guards
- actions performed during each transition
- failure handling
- rollback semantics
- timeout behavior
- concurrent-event handling
- crash recovery
- security invariants
- emergency takeover behavior
- reconnect behavior
- implementation and testing requirements

This document is a behavioral contract.

Implementation details may change, but the externally observable safety properties and state-transition semantics defined here must remain true.

The system must fail closed.

---

# 2. Product Model

The application is not primarily a remote desktop streamer.

It is a:

> **Secure remote-console state machine for an existing GNOME session.**

The host already has a normal GNOME desktop session.

A remote connection temporarily transfers interactive authority to the remote client while:

1. the same GNOME session remains in use;
2. the physical display is isolated;
3. the physical keyboard/mouse are isolated;
4. the remote client receives display output and input authority;
5. any loss of remote authority causes the system to return to a safe local locked state.

No second desktop session should be created for the normal remote-control path.

---

# 3. Scope

Initial supported environment:

- Ubuntu 26.04 LTS
- GNOME 50+
- Wayland
- systemd
- PipeWire
- Mutter
- single-user workstation

The state machine must not assume support for:

- X11
- KDE
- wlroots
- multi-user desktop switching
- separate headless login sessions
- arbitrary Linux distributions

Unsupported environments should be rejected during capability detection.

---

# 4. Authoritative States

The implementation should use explicit states rather than deriving state implicitly from scattered booleans.

Recommended states:

```text
LOCAL_ACTIVE
LOCAL_LOCKED
AUTHENTICATING
AUTHENTICATED
PREPARING_REMOTE
REMOTE_ACTIVE
REMOTE_DEGRADED
TEARING_DOWN
RECOVERING
EMERGENCY
FAILED_SAFE
```

The implementation may introduce additional internal substates, but these top-level states must remain understandable and observable.

---

# 5. State Definitions

## 5.1 LOCAL_ACTIVE

Normal local workstation state.

Expected conditions:

```text
GNOME session exists
physical display available
physical input available
remote input disabled
no active remote control lease
session locally usable
```

This is the normal state after a successful local unlock.

---

## 5.2 LOCAL_LOCKED

GNOME session is locked.

Expected conditions:

```text
GNOME session exists
session is locked
remote control lease absent
physical display restored
physical input restored
remote input disabled
```

This is the required safe state after remote disconnection.

It is also the preferred state immediately before establishing a remote session.

---

## 5.3 AUTHENTICATING

A remote client is attempting to authenticate.

Authentication may require:

```text
username
system password
TOTP
Remote Access Key OR trusted-client credential
```

The Remote Access Key requirement depends on client trust status.

TOTP is mandatory for every remote session.

No desktop control must be granted in this state.

---

## 5.4 AUTHENTICATED

Authentication succeeded.

A short-lived session credential exists.

However:

```text
remote input is NOT yet enabled
physical display is NOT yet disabled
physical input is NOT yet disabled
```

Authentication alone never grants remote control.

The system must transition through authorization and preparation first.

---

## 5.5 PREPARING_REMOTE

The host is preparing the existing GNOME session for remote control.

Typical operations:

```text
validate GNOME session
validate capabilities
validate session ownership
validate security epoch
create remote control lease
prepare virtual output
capture original display topology
activate virtual monitor
disable physical outputs
prepare remote input
disable physical input
verify resulting topology
verify input routing
verify session state
```

Preparation must be transactional.

If any mandatory operation fails, the system must roll back to a safe state.

---

## 5.6 REMOTE_ACTIVE

Remote control is fully established.

Expected conditions:

```text
remote session authenticated
remote session credential valid
control lease valid
security epoch valid
same GNOME session active
virtual display active
physical display isolated
physical input isolated
remote input active
remote media active or recoverable
```

This is the only normal state in which remote input is permitted.

---

## 5.7 REMOTE_DEGRADED

The connection is experiencing a temporary problem.

Examples:

- temporary network interruption
- WebRTC transport interruption
- media pipeline interruption
- signalling interruption
- temporary client disappearance

Remote input must not remain authorized indefinitely merely because the system considers the connection "degraded."

The implementation must use a short control-lease timeout.

If the lease cannot be renewed within the configured deadline:

```text
REMOTE_DEGRADED
        ↓
TEARING_DOWN
```

The system must eventually fail closed.

---

## 5.8 TEARING_DOWN

The remote session is being terminated.

Required order:

```text
stop accepting remote input
invalidate remote control lease
invalidate remote session authority
restore physical input
restore physical display
destroy virtual display
restore original monitor configuration
lock GNOME session
clear transient remote state
```

The exact ordering may differ where GNOME/Mutter requires a different teardown sequence, but the security invariant is:

> Remote input authority must be revoked before the system is considered safe.

---

## 5.9 RECOVERING

Used when normal teardown or preparation encountered an error.

Examples:

- Mutter operation failed
- PipeWire failed
- display restoration partially failed
- input restoration failed
- GNOME state changed unexpectedly
- main daemon restarted

Recovery must be idempotent.

Calling recovery twice must not make the state worse.

---

## 5.10 EMERGENCY

Entered by the independent emergency controller.

Emergency takeover has priority over every normal remote operation.

Required actions:

```text
revoke remote input
terminate active remote session
increment security epoch
lock GNOME
restore physical display
restore physical input
invalidate stale credentials/leases
remain locked
```

The emergency path must not depend on:

- browser
- WebRTC
- network
- signalling server
- main remote gateway
- healthy main remote daemon

---

## 5.11 FAILED_SAFE

Used when complete restoration cannot be verified.

This state must be conservative.

Examples:

```text
display restoration uncertain
input restoration uncertain
GNOME session state unknown
Mutter state inconsistent
```

The system must not silently transition to `LOCAL_ACTIVE`.

Instead it should:

- revoke remote authority
- remain locked where possible
- retry safe recovery
- expose diagnostics
- require explicit local recovery if necessary

The system must never assume recovery succeeded merely because an API call returned without an exception.

---

# 6. State Transition Diagram

Conceptually:

```text
                     +----------------+
                     |  LOCAL_ACTIVE  |
                     +-------+--------+
                             |
                           Lock
                             |
                             v
                     +----------------+
                     | LOCAL_LOCKED   |
                     +-------+--------+
                             |
                     Remote connection
                             |
                             v
                     +----------------+
                     | AUTHENTICATING |
                     +-------+--------+
                             |
                       Authentication
                             |
                             v
                     +----------------+
                     | AUTHENTICATED  |
                     +-------+--------+
                             |
                       Authorization
                             |
                             v
                     +----------------+
                     | PREPARING      |
                     +-------+--------+
                             |
                   Preparation succeeds
                             |
                             v
                     +----------------+
                     | REMOTE_ACTIVE  |
                     +---+--------+---+
                         |        |
                temporary|        |lease expires/
                 failure |        |disconnect
                         |        |
                         v        v
                  +-------------+ |
                  |   DEGRADED  | |
                  +------+------+ |
                         |        |
                    lease valid   |
                         |        |
                         +----+---+
                              |
                              v
                       TEARING_DOWN
                              |
                              v
                         LOCAL_LOCKED


Emergency can interrupt ANY state:

ANY STATE
    |
    | emergency trigger
    v
EMERGENCY
    |
    v
RECOVERING
    |
    +------> LOCAL_LOCKED
    |
    +------> FAILED_SAFE
```

---

# 7. Core Invariants

These invariants are more important than implementation convenience.

## Invariant 1 — No valid lease, no remote input

```text
remote_input_enabled == true
    ONLY IF
valid_control_lease == true
AND
security_epoch == current_security_epoch
AND
state == REMOTE_ACTIVE
```

---

## Invariant 2 — Authentication does not equal control

A successfully authenticated client must not automatically receive input authority.

```text
AUTHENTICATED != REMOTE_ACTIVE
```

---

## Invariant 3 — TOTP is always mandatory

Trusted-client status must never bypass TOTP.

```text
remote_session
    => username + password + TOTP
```

---

## Invariant 4 — New clients require Remote Access Key

Unknown/untrusted clients require:

```text
username
password
TOTP
Remote Access Key
```

---

## Invariant 5 — Emergency always revokes remote authority

Emergency takeover must invalidate current remote control regardless of network state.

---

## Invariant 6 — Old security epochs are invalid

After emergency takeover:

```text
security_epoch := security_epoch + 1
```

Any previous lease/session associated with the old epoch becomes invalid.

---

## Invariant 7 — Disconnect returns to locked local console

A remote disconnect must not simply expose the desktop.

Required result:

```text
remote disconnected
        ↓
remote authority revoked
        ↓
GNOME locked
        ↓
physical display restored
        ↓
physical input restored
        ↓
LOCAL_LOCKED
```

---

## Invariant 8 — No automatic local unlock

Remote teardown must never automatically unlock GNOME.

The user must perform normal local GNOME authentication.

---

## Invariant 9 — Recovery is idempotent

The following should be safe to call repeatedly:

```text
revoke_remote_authority()
restore_physical_input()
restore_physical_display()
destroy_virtual_monitor()
lock_session()
invalidate_sessions()
```

---

## Invariant 10 — Fail closed

If uncertain:

```text
disable remote input
invalidate lease
lock session
restore local hardware
```

Never choose:

```text
"probably okay, leave remote control enabled"
```

---

# 8. State Transition Table

| Current State | Event | Guard | Action | Next State |
|---|---|---|---|---|
| LOCAL_ACTIVE | lock | session available | lock GNOME | LOCAL_LOCKED |
| LOCAL_LOCKED | connection | remote access enabled | begin authentication | AUTHENTICATING |
| AUTHENTICATING | auth success | all required factors valid | issue session credential | AUTHENTICATED |
| AUTHENTICATING | auth failure | retry limit not exceeded | reject | AUTHENTICATING |
| AUTHENTICATING | retry limit | limit exceeded | temporary block | LOCAL_LOCKED |
| AUTHENTICATED | authorization success | policy allows access | create lease | PREPARING_REMOTE |
| AUTHENTICATED | timeout | timeout exceeded | invalidate credential | LOCAL_LOCKED |
| PREPARING_REMOTE | success | all safety checks pass | activate remote control | REMOTE_ACTIVE |
| PREPARING_REMOTE | failure | rollback possible | rollback | LOCAL_LOCKED |
| PREPARING_REMOTE | failure | rollback uncertain | fail-safe recovery | FAILED_SAFE |
| REMOTE_ACTIVE | lease renewal | valid | continue | REMOTE_ACTIVE |
| REMOTE_ACTIVE | transient failure | lease remains valid | suspend/recover | REMOTE_DEGRADED |
| REMOTE_ACTIVE | disconnect | any | teardown | TEARING_DOWN |
| REMOTE_ACTIVE | lease expiry | no renewal | teardown | TEARING_DOWN |
| REMOTE_DEGRADED | lease renewal | valid | restore connection | REMOTE_ACTIVE |
| REMOTE_DEGRADED | lease expiry | invalid | teardown | TEARING_DOWN |
| REMOTE_DEGRADED | client disconnect | any | teardown | TEARING_DOWN |
| TEARING_DOWN | success | restoration verified | lock | LOCAL_LOCKED |
| TEARING_DOWN | partial failure | recovery possible | retry recovery | RECOVERING |
| RECOVERING | success | safe state verified | lock | LOCAL_LOCKED |
| RECOVERING | failure | safety uncertain | remain conservative | FAILED_SAFE |
| FAILED_SAFE | recovery success | all checks pass | lock | LOCAL_LOCKED |
| ANY | emergency | emergency trigger | revoke + invalidate + restore | EMERGENCY |
| EMERGENCY | completed | safety verified | remain locked | LOCAL_LOCKED |
| EMERGENCY | restoration failure | uncertain | conservative recovery | FAILED_SAFE |

---

# 9. Remote Activation Transaction

Remote activation must be treated as a transaction.

Recommended conceptual transaction:

```text
BEGIN_REMOTE_ACTIVATION

1. Acquire state-machine lock
2. Confirm state == LOCAL_LOCKED
3. Confirm supported GNOME environment
4. Confirm target GNOME session
5. Confirm no conflicting remote session
6. Validate security epoch
7. Create remote session record
8. Create control lease
9. Snapshot physical display configuration
10. Snapshot input state
11. Prepare virtual monitor
12. Verify virtual monitor
13. Disable physical outputs
14. Verify physical outputs are isolated
15. Prepare remote input
16. Disable physical input
17. Verify physical input isolation
18. Verify GNOME session remains usable
19. Verify PipeWire capture
20. Verify remote input path
21. Mark REMOTE_ACTIVE
22. Release state-machine lock

COMMIT
```

If any mandatory operation fails before commit:

```text
ROLLBACK
```

---

# 10. Activation Rollback

Rollback should be performed in reverse dependency order.

Conceptually:

```text
remote input
     ↓
physical input
     ↓
virtual monitor
     ↓
physical display
     ↓
session state
```

Recommended rollback:

```text
1. Stop remote input
2. Invalidate control lease
3. Disable remote control
4. Restore physical input
5. Restore physical display topology
6. Destroy virtual monitor
7. Restore original monitor configuration
8. Lock GNOME
9. Clear transient state
10. Verify safe state
```

If verification succeeds:

```text
LOCAL_LOCKED
```

Otherwise:

```text
FAILED_SAFE
```

---

# 11. Remote Disconnect

Normal disconnect:

```text
REMOTE_ACTIVE
    |
    | client disconnect
    v
TEARING_DOWN
```

The client should not be required to cooperate for teardown.

The host must be capable of detecting disconnection independently.

The host should:

```text
invalidate lease
terminate session
stop remote input
restore local hardware
lock GNOME
verify restoration
```

Final state:

```text
LOCAL_LOCKED
```

---

# 12. Abrupt Network Failure

Network loss must be treated as potentially hostile or indistinguishable from client failure.

Do not wait indefinitely for reconnection.

Recommended mechanism:

```text
REMOTE_ACTIVE
     |
     | heartbeat / lease renewal missing
     v
REMOTE_DEGRADED
     |
     | lease timeout
     v
TEARING_DOWN
```

The timeout must be configurable.

The default should favor safety over reconnect convenience.

A stale network connection must never retain remote input authority indefinitely.

---

# 13. Control Lease

Remote input should be governed by a short-lived lease.

Lease contains at minimum:

```text
host_id
user
client_id
session_id
security_epoch
issued_at
expires_at
capabilities
```

The GNOME session agent should reject remote input when:

```text
lease expired
OR
lease revoked
OR
security epoch changed
OR
session ID invalid
OR
host state != REMOTE_ACTIVE
```

Lease renewal should require an authenticated and authorized control channel.

---

# 14. Lease Timeout

The system must distinguish:

```text
authentication lifetime
session lifetime
control lease lifetime
```

These should not be one shared timer.

For example:

```text
authentication/session credential:
    relatively long-lived

control lease:
    short-lived

heartbeat:
    frequent
```

This ensures that losing network connectivity quickly removes interactive authority.

Exact production timeout values must be established during testing rather than hard-coded prematurely.

---

# 15. Reconnect

Reconnect is permitted only if the host still considers the remote session valid.

Possible flow:

```text
REMOTE_DEGRADED
      |
      | connection restored
      v
validate session credential
      |
validate security epoch
      |
validate control lease
      |
verify GNOME state
      |
      v
REMOTE_ACTIVE
```

If validation fails:

```text
TEARING_DOWN
```

A client must never regain control merely because it reconnects to the gateway.

---

# 16. Security Epoch

The host maintains a monotonically increasing security epoch.

Example:

```text
epoch = 41
```

A remote session is issued with:

```text
session_epoch = 41
```

Emergency takeover:

```text
epoch = 42
```

The old session is now invalid:

```text
41 != 42
```

This provides cheap global invalidation without maintaining perfect per-connection revocation state.

The epoch must survive main-daemon restart where required by the threat model.

The implementation must determine whether it is persisted or regenerated in a way that guarantees stale sessions cannot survive restart.

---

# 17. Emergency Takeover

Emergency takeover is a privileged local safety path.

Trigger example:

```text
Ctrl + Alt + Shift + F12
```

held for approximately two seconds.

The exact shortcut and duration must be configurable.

The emergency daemon must operate independently of the main remote application.

Required sequence:

```text
EMERGENCY TRIGGER

        ↓

1. Disable remote input immediately

        ↓

2. Revoke active control lease

        ↓

3. Terminate remote connection

        ↓

4. Increment security epoch

        ↓

5. Lock GNOME session

        ↓

6. Restore physical display

        ↓

7. Restore physical keyboard/mouse

        ↓

8. Verify safe state

        ↓

9. Remain locked
```

It must not:

```text
unlock GNOME
restart the entire machine
execute arbitrary shell commands
depend on network availability
wait for the browser
wait for WebRTC
```

---

# 18. Emergency During PREPARING_REMOTE

If emergency occurs during activation:

```text
PREPARING_REMOTE
        |
        | emergency
        v
EMERGENCY
```

The current activation transaction must be abandoned.

Emergency takes priority over normal rollback logic.

The final target remains:

```text
LOCAL_LOCKED
```

---

# 19. Emergency During REMOTE_ACTIVE

This is the most important emergency path.

```text
REMOTE_ACTIVE
      |
      | emergency
      v
EMERGENCY
```

Remote input must be revoked first.

Do not wait for:

- network teardown
- WebRTC close
- browser acknowledgement
- gateway response
- main daemon cleanup

The emergency component should have a direct local path to the necessary safety controls.

---

# 20. Emergency During Main-Daemon Crash

The system must assume:

```text
remote-hostd crashed
```

does not necessarily mean:

```text
remote control is safely gone
```

Therefore system design must ensure the lease and GNOME agent independently fail closed.

Desired behavior:

```text
remote-hostd crash
       |
       v
lease stops renewing
       |
       v
lease expires
       |
       v
remote input revoked
       |
       v
session locked
       |
       v
physical hardware restored
```

The emergency daemon remains available as an additional local safety mechanism.

---

# 21. GNOME Shell/Mutter Failure

If Mutter/GNOME Shell becomes unavailable:

```text
REMOTE_ACTIVE
      |
      | session failure
      v
RECOVERING
```

The system must:

1. revoke remote input;
2. invalidate lease;
3. stop capture;
4. restore display topology where possible;
5. restore physical input;
6. lock session if possible;
7. verify state;
8. otherwise enter FAILED_SAFE.

Do not repeatedly restart GNOME components blindly.

Recovery loops must have limits.

---

# 22. Display Restoration Failure

If the physical display cannot be restored:

```text
TEARING_DOWN
      |
      | display restore failed
      v
RECOVERING
```

Retry using bounded/idempotent recovery.

If still uncertain:

```text
FAILED_SAFE
```

The system must not declare:

```text
LOCAL_ACTIVE
```

while physical display state is unknown.

---

# 23. Input Restoration Failure

Input restoration is safety-critical.

If physical input restoration fails:

```text
FAILED_SAFE
```

unless the system can positively establish that local input is functional.

The remote session must remain invalidated.

Do not preserve remote control merely because local input restoration failed.

---

# 24. Concurrent Events

The state machine must serialize conflicting state transitions.

Potential simultaneous events:

```text
client disconnect
lease expiry
emergency hotkey
Mutter failure
daemon restart
GNOME logout
network reconnect
```

Emergency has highest priority.

Recommended priority:

```text
1. EMERGENCY
2. SAFETY/FAILURE
3. LEASE EXPIRY
4. DISCONNECT
5. NORMAL STATE TRANSITION
6. RECONNECT
```

Example:

```text
REMOTE_ACTIVE
```

simultaneously receives:

```text
reconnect
+
emergency
```

Result must be:

```text
EMERGENCY
```

Never:

```text
REMOTE_ACTIVE
```

---

# 25. State-Machine Lock

Only one transition transaction should mutate critical state at a time.

The implementation should use an explicit state-machine synchronization mechanism.

Avoid scattered locks that can produce:

```text
display restored
while input still remote
```

or:

```text
new remote session activated
while old teardown is still running
```

Recommended concept:

```text
StateMachineLock
    |
    +-- state
    +-- active_session
    +-- active_lease
    +-- security_epoch
    +-- transition_id
```

Long-running external operations should be designed carefully so they do not create deadlocks.

---

# 26. Transition IDs

Every transition should have a unique identifier.

Example:

```text
transition_id = UUID
```

Logs can then correlate:

```text
transition started
operation
failure
rollback
verification
final state
```

This is particularly useful for debugging Mutter and hardware-dependent failures.

---

# 27. Idempotent Operations

Operations should be safe to repeat.

Examples:

```text
revoke_remote_input()
```

should be safe when remote input is already disabled.

```text
lock_session()
```

should be safe when GNOME is already locked.

```text
restore_display()
```

should be safe when the original topology is already restored.

```text
destroy_virtual_monitor()
```

should be safe when the monitor no longer exists.

This is essential because crash recovery may repeat cleanup.

---

# 28. Startup Recovery

When the system boots or `remote-hostd` starts, it must not assume that previous state was cleanly terminated.

Startup procedure:

```text
START
  |
  v
load persistent configuration
  |
inspect previous runtime state
  |
invalidate stale remote sessions
  |
invalidate stale leases
  |
verify security epoch
  |
inspect GNOME session
  |
inspect display topology
  |
inspect input state
  |
restore safe local configuration
  |
lock session where appropriate
  |
verify
  |
LOCAL_LOCKED
```

The implementation must explicitly handle power loss during REMOTE_ACTIVE.

---

# 29. Power Loss / Hard Reboot

A power failure can occur without any software cleanup.

On next boot:

```text
all old remote sessions are invalid
```

The system must not allow stale session credentials to restore remote input automatically.

The host should boot into a safe local state.

Normal remote access can then begin again through authentication.

---

# 30. Suspend / Resume

While remote control is active, system suspend should preferably be inhibited.

If suspend nevertheless occurs:

```text
REMOTE_ACTIVE
      |
      | suspend
      v
RECOVERING
```

After resume:

- validate GNOME session;
- validate display topology;
- validate PipeWire;
- validate input;
- validate lease;
- validate security epoch.

If any critical component cannot be verified:

```text
TEARING_DOWN
```

Do not blindly restore REMOTE_ACTIVE.

---

# 31. Monitor Hotplug

Physical monitor changes during remote operation must be treated as topology changes.

Examples:

```text
HDMI connected
HDMI removed
DisplayPort removed
USB-C monitor attached
laptop dock connected
```

The system must preserve the original topology snapshot.

During remote mode:

```text
new physical outputs
    -> remain isolated
```

unless explicit policy says otherwise.

On teardown:

```text
restore the topology that existed immediately before remote activation
```

rather than assuming a fixed monitor layout.

---

# 32. Original Display Configuration

Before modifying outputs, capture enough information to restore the previous configuration.

At minimum:

```text
connector/output identity
enabled/disabled state
mode
resolution
refresh rate
position
scale
transform
primary display
```

Where GNOME/Mutter exposes additional relevant state, include it as required.

The snapshot must be tied to the active remote transition.

---

# 33. Physical Input State

Before remote activation, capture relevant local-input state if necessary.

The implementation must know:

```text
which physical devices were active
which devices were isolated
how local input will be restored
```

Do not assume a static `/dev/input/event*` mapping.

Device enumeration can change across:

- reboot
- hotplug
- USB reconnection
- docking
- kernel changes

Use stable device identity where possible.

---

# 34. Remote Input Safety

Remote input must be capability-based.

The control lease should specify capabilities such as:

```text
keyboard
pointer
touch
tablet
clipboard
```

Only explicitly authorized capabilities should be enabled.

Do not give a remote session more control than required.

---

# 35. Clipboard and Additional Channels

Clipboard synchronization should not automatically be treated as equivalent to input.

If implemented:

```text
clipboard capability
```

must be independently authorized and observable.

The initial feasibility PoC should prioritize:

```text
display
keyboard
pointer
```

and defer additional channels until the core safety model is proven.

---

# 36. Remote Session Termination Conditions

The remote session must terminate when any of the following occurs:

```text
explicit disconnect
lease expiration
authentication/session invalidation
security epoch mismatch
trusted-device revocation
Remote Access Key revocation where applicable
emergency takeover
main security daemon failure
GNOME session failure
critical display failure
critical input failure
host shutdown
host reboot
policy disable
```

The exact behavior may vary by failure type, but remote authority must always be revoked.

---

# 37. Local Unlock After Teardown

After successful teardown:

```text
LOCAL_LOCKED
```

The user may physically unlock GNOME normally.

The system must not:

- automatically unlock;
- replay previous authentication;
- automatically reconnect the remote client;
- restore remote input authority.

---

# 38. Remote Reconnection After Local Unlock

If the user locally unlocks after a previous remote disconnect:

```text
LOCAL_LOCKED
      |
      | local unlock
      v
LOCAL_ACTIVE
```

A remote client cannot silently regain control.

A new remote authentication/session flow is required.

---

# 39. Trusted Client Reconnection

A trusted client still requires:

```text
username
password
TOTP
trusted-client credential
```

A trusted-client credential alone must never establish control.

If the trusted credential is revoked:

```text
existing session -> terminate
future session -> full new-client flow
```

---

# 40. Emergency and Trusted Clients

Emergency takeover must override trusted status.

Trusted devices do not receive special treatment after emergency.

The security epoch invalidates the previous remote session regardless of client trust.

---

# 41. Authentication Failure During Active Session

Authentication normally occurs before remote activation.

However, if an authenticated session later becomes invalid:

```text
REMOTE_ACTIVE
      |
      | credential/session invalidated
      v
TEARING_DOWN
```

Remote input must immediately cease.

---

# 42. Authorization Failure

Authorization can fail independently of authentication.

Examples:

```text
remote access disabled
user not permitted
device revoked
policy changed
security epoch changed
another remote controller active
```

Result:

```text
reject remote control
retain LOCAL_LOCKED
```

Do not enter PREPARING_REMOTE.

---

# 43. Single Active Remote Controller

Initial implementation should permit only one active remote control session.

If another client attempts control:

```text
REMOTE_ACTIVE
```

the host should reject the second controller.

Do not silently transfer control.

Future multi-controller support can be considered separately.

---

# 44. Control Transfer

Control transfer is explicitly out of scope for v1.

Do not implement:

```text
client A -> client B
```

while the workstation remains active.

A new remote controller should require the previous controller to terminate.

---

# 45. State Persistence

Persistent state should be limited to what is required.

Persist:

```text
host identity
authentication configuration
TOTP configuration
Remote Access Key verifier
trusted-device records
security configuration
policy
```

Do not persist transient state as if it were authoritative.

Transient state:

```text
active connection
active lease
PipeWire stream
virtual monitor
current transition
```

must be reconstructable.

---

# 46. Observability

Every state transition should generate structured logs.

Recommended fields:

```text
timestamp
host_id
transition_id
session_id
client_id
previous_state
event
next_state
result
failure_code
security_epoch
duration
```

Never log:

```text
password
TOTP secret
Remote Access Key
session bearer token
trusted-device private credential
```

---

# 47. State-Machine Diagnostics

Provide a diagnostic command/API that can report:

```text
current state
GNOME session status
remote session status
lease status
security epoch
virtual monitor status
physical output status
physical input status
PipeWire status
Mutter capability status
emergency daemon status
last transition
last recovery failure
```

Sensitive credential material must never be included.

---

# 48. Formal Transition Pseudocode

Conceptual implementation:

```python
def handle_event(event):
    with state_machine_lock:

        if event.type == EMERGENCY:
            return handle_emergency()

        if state == LOCAL_ACTIVE:
            return handle_local_active(event)

        if state == LOCAL_LOCKED:
            return handle_local_locked(event)

        if state == AUTHENTICATING:
            return handle_authentication(event)

        if state == AUTHENTICATED:
            return handle_authorized_session(event)

        if state == PREPARING_REMOTE:
            return handle_preparation(event)

        if state == REMOTE_ACTIVE:
            return handle_remote_active(event)

        if state == REMOTE_DEGRADED:
            return handle_remote_degraded(event)

        if state == TEARING_DOWN:
            return handle_teardown(event)

        if state == RECOVERING:
            return handle_recovery(event)

        if state == FAILED_SAFE:
            return handle_failed_safe(event)

        raise UnknownStateError(state)
```

The implementation should avoid allowing arbitrary callers to mutate the state directly.

Prefer:

```text
event -> transition handler -> validated state change
```

over:

```text
component -> set_state(...)
```

---

# 49. Emergency Pseudocode

Conceptual:

```python
def handle_emergency():
    transition_to(EMERGENCY)

    revoke_remote_input()

    invalidate_active_control_lease()

    increment_security_epoch()

    terminate_remote_session()

    lock_gnome_session()

    restore_physical_display()

    restore_physical_input()

    verify_safe_state()

    if safe_state_verified():
        transition_to(LOCAL_LOCKED)
    else:
        transition_to(FAILED_SAFE)
```

The emergency path must be implemented so that an exception in one cleanup operation does not prevent subsequent safety operations.

For example:

```python
try:
    revoke_remote_input()
finally:
    try:
        invalidate_lease()
    finally:
        ...
```

Use structured recovery rather than assuming every operation succeeds.

---

# 50. Fail-Safe Recovery Pseudocode

Conceptual:

```python
def recover_to_safe_state():

    invalidate_all_remote_authority()

    stop_remote_input()

    terminate_remote_session()

    try_restore_physical_input()

    try_restore_physical_display()

    try_destroy_virtual_monitor()

    try_lock_gnome()

    verified = verify_safe_state()

    if verified:
        transition_to(LOCAL_LOCKED)
    else:
        transition_to(FAILED_SAFE)
```

The verification step is mandatory.

---

# 51. Safe-State Definition

A state may only be declared safe when the system can establish:

```text
no remote input authority
AND
no valid active control lease
AND
current security epoch invalidates stale sessions where required
AND
physical display restored or otherwise positively isolated
AND
physical input restored or otherwise positively controlled
AND
GNOME session locked
```

If any condition is unknown:

```text
FAILED_SAFE
```

---

# 52. Failure Matrix

| Failure | Required Response |
|---|---|
| Client disconnect | revoke + teardown + lock |
| Network outage | degraded → lease expiry → teardown |
| WebRTC failure | revoke/teardown unless safely recoverable |
| Gateway crash | host eventually detects lease loss |
| Host daemon crash | leases expire; emergency remains available |
| GNOME agent crash | revoke + recover |
| Mutter failure | revoke + recover |
| PipeWire failure | revoke or degraded depending on media-only failure |
| Physical monitor failure | recover/failed-safe |
| Physical input failure | revoke + failed-safe |
| Emergency hotkey | immediate emergency |
| Security epoch change | invalidate current remote session |
| Trusted-device revocation | terminate associated sessions |
| Remote Access Key rotation | invalidate sessions according to policy |
| Host reboot | invalidate transient remote state |
| Power loss | recover safe state on next boot |
| Suspend/resume | revalidate everything |
| Monitor hotplug | isolate new physical output |
| GNOME logout | terminate remote session |
| User disables remote access | terminate remote sessions |
| Second remote controller | reject |

---

# 53. Testing Requirements

The state machine requires automated tests plus real hardware tests.

## Unit Tests

Test:

```text
every legal transition
every illegal transition
lease expiration
security epoch mismatch
authentication failures
authorization failures
emergency handling
rollback
idempotent cleanup
concurrent events
startup recovery
```

---

## Integration Tests

Test:

```text
GNOME session discovery
Mutter operations
virtual monitor creation
display topology changes
physical input isolation
PipeWire lifecycle
GNOME locking
restoration
systemd restart
```

---

## Failure Injection

Explicitly inject:

```text
network disconnect
gateway kill
host daemon kill
GNOME agent kill
PipeWire failure
Mutter operation failure
display restore failure
input restore failure
power interruption where practical
```

The system must demonstrate fail-closed behavior.

---

# 54. Concurrency Tests

Test combinations such as:

```text
disconnect + emergency
lease expiry + reconnect
emergency + reconnect
daemon restart + client reconnect
monitor hotplug + disconnect
Mutter failure + emergency
PipeWire failure + emergency
```

The expected outcome must always respect event priority.

---

# 55. Recovery Tests

For every critical operation:

```text
fail operation once
fail operation repeatedly
fail operation after partial completion
retry recovery
restart daemon
restart GNOME where practical
```

Verify that repeated recovery does not corrupt display or input configuration.

---

# 56. State-Machine Acceptance Criteria

The state machine is considered correct only if:

### A. No remote input without authorization

Impossible to inject remote input without a valid lease and current epoch.

### B. Disconnect is fail-safe

Remote disconnect results in locked local state.

### C. Network loss is fail-safe

Network failure cannot leave indefinite remote input authority.

### D. Emergency is independent

Emergency takeover works when the main remote application is unhealthy.

### E. Emergency invalidates stale sessions

An old remote session cannot regain control after emergency.

### F. Physical privacy is restored

The physical display is restored after remote teardown.

### G. Physical control is restored

The local keyboard/mouse are restored after remote teardown.

### H. No automatic unlock

The user must explicitly unlock GNOME.

### I. Recovery is idempotent

Repeated cleanup operations do not corrupt state.

### J. Startup is safe

Power loss or daemon restart cannot silently restore stale remote control.

---

# 57. Feasibility Gate

Before implementing the complete product, the state machine must be proven against the actual Ubuntu/GNOME environment.

The following remain hard feasibility gates:

```text
same-session reuse
virtual monitor
physical display isolation
remote input
physical input isolation
GNOME lock semantics
safe teardown
emergency takeover
reliable restoration
```

If any of these cannot be implemented safely, stop and reassess the architecture.

Do not hide an architectural incompatibility behind additional abstraction layers.

---

# 58. Implementation Rules for GitHub Copilot Agent

When implementing this document:

1. Inspect the repository before modifying it.
2. Inspect and respect the workflow/configuration created by `adaptive-workflow-configurator`.
3. Do not replace or duplicate that workflow.
4. Follow the repository's established structure and conventions.
5. Do not invent a large directory architecture if the workflow already defines one.
6. Keep state-machine logic independent from GNOME-specific implementation where practical.
7. Keep GNOME/Mutter-specific behavior behind a clearly isolated backend.
8. Keep privileged operations narrowly scoped.
9. Keep emergency handling independent from the browser/network path.
10. Do not implement the complete product before feasibility gates pass.

---

# 59. Recommended Implementation Order

Implement in this order:

```text
1. State definitions
2. Event definitions
3. Transition engine
4. State-machine locking
5. Control lease model
6. Security epoch
7. Unit tests
8. GNOME session adapter
9. Display transaction
10. Input transaction
11. Lock/unlock integration
12. Recovery engine
13. Emergency integration
14. Failure injection
15. End-to-end state-machine tests
```

Only after these are reliable should the project proceed to:

```text
authentication
networking
WebRTC
browser UI
trusted devices
production packaging
```

---

# 60. Final Engineering Principle

The most important rule in the entire project is:

> **Remote control is a temporary lease, not a permanent mode.**

The system should continuously prove that remote authority is still valid.

If that proof disappears:

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

The safe local locked console is the default recovery destination.

Never make the user depend on the network, browser, or main remote daemon to regain physical control of their workstation.