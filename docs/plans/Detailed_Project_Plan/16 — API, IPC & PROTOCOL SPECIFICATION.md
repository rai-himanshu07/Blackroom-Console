# 16 — API, IPC & PROTOCOL SPECIFICATION

## 1. Purpose

This document defines the communication contracts between:

```text
Browser Client
      |
      | HTTPS / WebSocket / WebRTC
      v
Remote Gateway
      |
      | authenticated local IPC
      v
Remote Host Daemon
      |
      | authenticated local IPC
      v
GNOME Session Agent

Remote Emergency Daemon
      |
      | independent local safety path
      v
Remote Host Daemon / GNOME Session
```

The goal is to establish clear boundaries between:

- authentication
- authorization
- session management
- control authority
- media transport
- GNOME operations
- privileged operations
- emergency recovery

The protocol must make it impossible to confuse:

```text
connected
```

with:

```text
authorized to control
```

---

# 2. Protocol Design Principles

The protocol must follow these principles:

1. Authentication and authorization are separate.
2. Session credentials are separate from login credentials.
3. Control authority is temporary.
4. Every remote input operation requires current authorization.
5. Security epoch changes invalidate old authority.
6. Protocol messages are explicitly typed.
7. Unknown messages are rejected safely.
8. Invalid messages never cause privileged operations.
9. Browser state is never authoritative.
10. Host state is authoritative.
11. Media transport does not grant control authority.
12. Gateway does not receive unnecessary secrets.
13. Privileged IPC is narrow and explicit.
14. No arbitrary command execution.
15. Emergency operations remain independent from the browser/network path.
16. Protocol version changes must not silently weaken security.

---

# 3. Communication Planes

The system should conceptually separate four planes.

## Plane A — Authentication

```text
Browser
   |
HTTPS / WebSocket
   |
Gateway
   |
Host
```

Responsible for:

- username
- password
- TOTP
- Remote Access Key
- trusted-device credential

---

## Plane B — Session Control

Responsible for:

- creating remote sessions
- authorization
- security epoch
- control lease
- lifecycle
- disconnect
- recovery

---

## Plane C — Media/Input

Responsible for:

- WebRTC media
- remote keyboard
- remote pointer

Media/input transport must still be governed by the control lease.

---

## Plane D — Local Safety

Responsible for:

- emergency trigger
- remote revocation
- session termination
- GNOME locking
- display restoration
- input restoration

This plane must not depend on the Internet.

---

# 4. External Protocol Architecture

The browser-facing architecture should be:

```text id="ext001"
Browser
   |
   | HTTPS
   v
Gateway
   |
   | authenticated application protocol
   v
Host Daemon
```

WebRTC is used for the real-time media/input channel.

WebSocket is used for signalling/control where appropriate.

Do not put authentication credentials inside WebRTC data channels.

---

# 5. Transport Security

All network-facing application communication must use encrypted transport.

Production requirement:

```text id="tls001"
HTTPS
WSS
WebRTC with authenticated DTLS/SRTP
```

Do not allow production authentication over plaintext HTTP.

Development/test environments may use explicit test certificates.

---

# 6. Host Identity

Every host must have a cryptographic host identity.

The identity must be independent of:

- hostname
- IP address
- MAC address
- username

A host identity should contain:

```text id="host001"
host_id
public_key
fingerprint
```

The private key remains on the host.

---

# 7. Client Identity

Clients should have a stable cryptographic identity where practical.

A trusted client may have:

```text id="client001"
client_id
public_key
device_metadata
credential
created_at
last_used_at
status
```

Client identity is not equivalent to authentication.

A client must still satisfy the host's authentication policy.

---

# 8. Protocol Version

Every protocol connection should negotiate a protocol version.

Example:

```text id="proto001"
client_protocol:
    3

host_protocol:
    3

negotiated:
    3
```

If incompatible:

```text id="proto002"
PROTOCOL_INCOMPATIBLE
```

Do not silently downgrade security requirements.

---

# 9. Capability Negotiation

After protocol negotiation, the host may advertise supported capabilities.

Examples:

```text id="cap001"
virtual_display
remote_input
physical_display_isolation
physical_input_isolation
emergency_control
reconnect
multiple_clients
clipboard
audio
```

Only capabilities actually verified on the current host should be advertised.

---

# 10. Authentication Contract

The browser must authenticate according to the host policy.

## New/Untrusted Device

Required:

```text id="auth001"
username
password
totp
remote_access_key
```

---

## Trusted Device

Required:

```text id="auth002"
username
password
totp
trusted_device_credential
```

TOTP remains mandatory.

---

# 11. Authentication Request

Conceptually:

```text id="auth003"
AuthenticateRequest:
    protocol_version
    host_id
    username
    password
    totp
    access_key_or_device_credential
    client_id
```

Actual implementation must protect credentials in transit.

Do not log this message.

Do not persist it unnecessarily.

---

# 12. Authentication Response

Success:

```text id="auth004"
AuthenticateResponse:
    result = SUCCESS
    authentication_session
    host_id
    protocol_version
    capabilities
```

Failure:

```text id="auth005"
AuthenticateResponse:
    result = DENIED
    error_code
```

Do not reveal unnecessary information about which credential component failed.

---

# 13. Authentication Session

The authentication session is temporary.

It must have:

```text id="auth006"
authentication_session_id
issued_at
expires_at
host_id
user
client_id
authentication_strength
```

It is not itself a remote-control lease.

---

# 14. Session Creation

After successful authentication:

```text id="sess001"
CreateSession
```

The host verifies:

- authentication session
- user authorization
- host state
- current security epoch
- current active session policy
- GNOME availability
- emergency controller health

If acceptable:

```text id="sess002"
RemoteSession:
    session_id
    host_id
    user
    client_id
    security_epoch
    state
```

---

# 15. Remote Session States

Protocol-visible states should correspond to the authoritative state machine.

Example:

```text id="sess003"
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

The browser must not invent additional authoritative states.

---

# 16. Session Credential

After session creation, issue a short-lived session credential.

It should be:

- cryptographically strong
- scoped to host
- scoped to session
- scoped to client
- short-lived
- revocable

It must not be equivalent to:

- Linux password
- TOTP secret
- Remote Access Key
- trusted-device credential

---

# 17. Control Lease

Remote input requires a separate control lease.

Conceptually:

```text id="lease001"
ControlLease:
    session_id
    client_id
    host_id
    security_epoch
    issued_at
    expires_at
    capabilities
```

The actual lease credential is secret and must never be displayed in diagnostics.

---

# 18. Lease Lifecycle

```text id="lease002"
SESSION_AUTHORIZED
       ↓
LEASE_CREATED
       ↓
LEASE_ACTIVE
       ↓
LEASE_RENEWED
       ↓
LEASE_EXPIRED / REVOKED
```

No valid lease:

```text id="lease003"
REMOTE INPUT = DENIED
```

---

# 19. Lease Renewal

The authorized client may periodically request renewal.

The host verifies:

- session still exists
- client still authorized
- security epoch unchanged
- session state permits control
- lease has not been revoked
- connection remains healthy

If any check fails:

```text id="lease004"
RENEWAL = DENIED
```

---

# 20. Security Epoch

Every control-related request should be associated with the security epoch.

Conceptually:

```text id="epoch001"
Request:
    session_id
    epoch = 42

Host:
    current_epoch = 42

Result:
    ACCEPT
```

If:

```text id="epoch002"
request_epoch = 42
current_epoch = 43
```

then:

```text id="epoch003"
REJECT
```

This provides fast invalidation of stale authority.

---

# 21. Epoch Increment Events

The security epoch should be incremented for security-critical events such as:

- emergency takeover
- revoke-all
- security reset
- service restart where appropriate
- explicit credential/security reset

The exact event list must be defined by implementation and tested.

---

# 22. Session Termination

A session can terminate because of:

```text id="sess004"
CLIENT_DISCONNECT
NETWORK_TIMEOUT
LEASE_EXPIRY
USER_REQUEST
EMERGENCY
AUTHORIZATION_REVOCATION
GNOME_FAILURE
HOST_SHUTDOWN
UPGRADE
SECURITY_RESET
```

Every termination path must enter the safe teardown workflow.

---

# 23. Disconnect Contract

Conceptually:

```text id="disc001"
DisconnectRequest:
    session_id
    session_credential
```

Host response:

```text id="disc002"
DisconnectResponse:
    result
    final_state
```

The host must revoke authority before considering disconnect complete.

---

# 24. Emergency Contract

Emergency control is fundamentally different from normal protocol operations.

It should not require:

- browser
- Internet
- WebRTC
- gateway
- active client

The emergency event is locally generated.

Conceptually:

```text id="emg001"
EmergencyTrigger:
    source
    timestamp
```

The emergency subsystem then initiates:

```text id="emg002"
REVOKE
→
EPOCH++
→
TERMINATE SESSION
→
LOCK
→
RESTORE DISPLAY
→
RESTORE INPUT
→
VERIFY
```

---

# 25. Emergency Acknowledgement

The emergency daemon should receive an explicit result.

Example:

```text id="emg003"
EmergencyResult:
    authority_revoked
    epoch_incremented
    session_terminated
    session_locked
    display_restored
    input_restored
    final_state
```

Partial success must be represented explicitly.

---

# 26. Local IPC Architecture

Recommended logical channels:

```text id="ipc001"
remote-gateway
      |
      v
remote-hostd

remote-hostd
      |
      v
gnome-session-agent

remote-emergencyd
      |
      v
remote-hostd
```

The exact IPC mechanism may be:

- Unix domain sockets
- D-Bus
- another authenticated local IPC mechanism

Selection should be based on the required security and GNOME integration.

Do not create a generic IPC proxy.

---

# 27. IPC Authentication

Every privileged IPC request must authenticate its caller.

Possible controls include:

- Unix UID
- peer credentials
- Unix socket permissions
- systemd socket activation
- D-Bus policy
- cryptographic authentication where necessary

The host daemon must not trust a caller merely because it can reach the socket.

---

# 28. Gateway → Host IPC

The gateway should request only application-level operations.

Allowed concepts:

```text id="ipc002"
authenticate
create_session
renew_lease
disconnect_session
get_public_status
```

It should NOT have generic operations such as:

```text id="ipc003"
execute
run_command
dbus_call
shell
write_file
```

---

# 29. Host → GNOME Agent IPC

The host daemon should request explicit session operations.

Examples:

```text id="ipc004"
get_session_state
get_capabilities
prepare_remote
create_virtual_display
isolate_physical_display
isolate_physical_input
start_capture
enable_remote_input
lock_session
teardown_remote
restore_display
restore_input
verify_safe_state
```

These are conceptual operations; exact APIs should follow implementation.

---

# 30. GNOME Agent Response

Every operation should return an explicit result.

Example:

```text id="ipc005"
Operation:
    isolate_physical_display

Result:
    SUCCESS

State:
    ISOLATED

Evidence:
    topology_id=...
```

Failure:

```text id="ipc006"
Result:
    FAILED

Error:
    DISPLAY_ISOLATION_FAILED
```

Do not infer success merely from the absence of an IPC error.

---

# 31. Emergency Daemon → Host IPC

The emergency daemon should have a much smaller API.

Prefer something conceptually similar to:

```text id="ipc007"
EMERGENCY_REVOKE
EMERGENCY_STATUS
```

It should not expose:

```text id="ipc008"
authentication
network
browser
WebRTC
configuration
arbitrary shell
```

The emergency component should contain as little functionality as possible.

---

# 32. IPC Message Structure

Use a common envelope.

Conceptually:

```text id="msg001"
Message:
    protocol_version
    message_type
    request_id
    timestamp
    sender
    session_id
    transition_id
    security_epoch
    payload
```

Not every field is required for every message.

Validation must occur before processing the payload.

---

# 33. Request IDs

Every request should have a unique request ID.

Example:

```text id="msg002"
request_id:
    req_01J...
```

Responses must reference the request.

This enables:

- correlation
- timeout handling
- duplicate detection
- race debugging

---

# 34. Idempotency

Safety-critical operations should be idempotent where possible.

Examples:

```text id="idemp001"
lock_session()
restore_display()
restore_input()
revoke_session()
terminate_session()
```

Calling them twice should not create an unsafe state.

---

# 35. Duplicate Requests

Test:

```text id="idemp002"
restore_display
restore_display
restore_display
```

Expected:

```text id="idemp003"
safe final state
```

not:

```text id="idemp004"
error because restoration was already performed
```

where an idempotent operation is intended.

---

# 36. Stale Requests

A stale request must not overwrite newer state.

Example:

```text id="stale001"
Transition A:
    activation

Transition B:
    emergency

Late response from A
```

The host must reject any result that no longer applies.

Use:

- transition IDs
- request IDs
- generation counters
- security epoch

as appropriate.

---

# 37. Timeout Semantics

Every IPC operation that can block must have a timeout.

On timeout:

```text id="timeout001"
unknown operation state
        ↓
do not assume success
        ↓
enter recovery / fail-safe
```

Never interpret timeout as success.

---

# 38. Partial Failure

Example:

```text id="fail001"
create_virtual_display
    SUCCESS

disable_physical_display
    SUCCESS

disable_physical_input
    TIMEOUT
```

The system must not continue to:

```text id="fail002"
REMOTE_ACTIVE
```

It must enter the appropriate rollback/recovery path.

---

# 39. Browser Control Protocol

The browser should communicate through explicit commands.

Conceptual message types:

```text id="web001"
HELLO
AUTHENTICATE
CREATE_SESSION
GET_SESSION_STATUS
REQUEST_CONTROL
RENEW_CONTROL
RELEASE_CONTROL
DISCONNECT
GET_CAPABILITIES
```

The browser must not directly command privileged GNOME operations.

---

# 40. Browser Event Stream

The host/gateway may send events:

```text id="web002"
AUTHENTICATION_COMPLETE
SESSION_CREATED
SESSION_STATE_CHANGED
CONTROL_GRANTED
CONTROL_REVOKED
LEASE_EXPIRING
NETWORK_DEGRADED
RECOVERY_STARTED
RECOVERY_COMPLETE
SESSION_TERMINATED
ERROR
```

The browser uses these for UI.

The host remains authoritative.

---

# 41. Control Request

If explicit control acquisition is used:

```text id="control001"
RequestControl:
    session_id
```

Host checks:

- authentication
- authorization
- session state
- active client policy
- security epoch
- existing controller
- safety state

Then:

```text id="control002"
ControlGranted:
    lease_metadata
```

The actual secret lease credential must be protected.

---

# 42. Single Controller Policy

The preferred initial model is:

```text id="controller001"
ONE ACTIVE REMOTE CONTROLLER
```

This avoids ambiguous simultaneous keyboard/pointer control.

If another client requests control:

```text id="controller002"
CLIENT A:
    CONTROL

CLIENT B:
    REQUEST CONTROL
```

Policy should be explicit:

```text id="controller003"
DENY
```

or:

```text id="controller004"
EXPLICIT TAKEOVER
```

Do not silently split control between clients.

---

# 43. Media Session

WebRTC media establishment should occur only after session authorization.

Conceptually:

```text id="media001"
authenticated
    ↓
session authorized
    ↓
control/session policy established
    ↓
GNOME prepared
    ↓
WebRTC negotiation
    ↓
media connected
```

Media connection does not itself grant remote control.

---

# 44. Input Data Plane

Remote keyboard/pointer data must be accepted only while:

```text id="input001"
session valid
+
epoch valid
+
lease valid
+
GNOME input state valid
```

Otherwise:

```text id="input002"
DROP
```

Do not queue unauthorized input for later execution.

---

# 45. Input Ordering

Remote input events should have:

- sequence number
- session ID
- appropriate lease association

The host/GNOME agent should reject:

- stale sequence data
- invalid session
- invalid lease
- invalid epoch

Do not replay old input after reconnect.

---

# 46. Input Failure

If remote input becomes unreliable:

```text id="input003"
REMOTE_ACTIVE
    ↓
INPUT_FAILURE
```

The system should not leave an uncontrolled half-state.

Depending on the failure policy:

```text id="input004"
REMOTE_DEGRADED
```

or:

```text id="input005"
TEARING_DOWN
```

must eventually lead to safe recovery.

---

# 47. Display State Contract

The GNOME agent should report:

```text id="display001"
physical_display_state
virtual_display_state
topology_id
isolation_state
```

Example:

```text id="display002"
Physical:
    ISOLATED

Virtual:
    ACTIVE

Topology:
    REMOTE_ONLY
```

---

# 48. Input State Contract

The GNOME agent should report:

```text id="input006"
physical_input_state
remote_input_state
input_backend
```

Example:

```text id="input007"
Physical Input:
    ISOLATED

Remote Input:
    ENABLED

Backend:
    libei
```

---

# 49. Safety Verification API

The GNOME agent should expose an explicit verification operation.

Conceptually:

```text id="safe001"
verify_safe_state()
```

It should verify:

```text id="safe002"
remote authority
physical display
physical input
GNOME lock
virtual display
session state
```

The result should be explicit.

---

# 50. Final Safe-State Contract

A successful recovery should report:

```text id="safe003"
Remote Authority:
    REVOKED

Remote Input:
    DISABLED

GNOME:
    LOCKED

Physical Display:
    RESTORED

Physical Input:
    RESTORED

Stale Session:
    INVALID

Security Epoch:
    CURRENT

Final State:
    LOCAL_LOCKED

Safety:
    VERIFIED
```

---

# 51. Error Model

Use stable error categories and codes.

Examples:

```text id="err001"
AUTH_INVALID
AUTH_RATE_LIMITED
AUTH_TOTP_REQUIRED
AUTH_ACCESS_KEY_REQUIRED
AUTH_DEVICE_REVOKED

HOST_UNAVAILABLE
HOST_UNSUPPORTED

SESSION_NOT_FOUND
SESSION_REVOKED
SESSION_EPOCH_MISMATCH

LEASE_EXPIRED
LEASE_REVOKED
LEASE_INVALID

GNOME_SESSION_UNAVAILABLE
MUTTER_UNAVAILABLE
VIRTUAL_DISPLAY_FAILED
DISPLAY_ISOLATION_FAILED
INPUT_ISOLATION_FAILED
SESSION_LOCK_FAILED

PIPEWIRE_UNAVAILABLE
WEBRTC_FAILED
NETWORK_FAILED

IPC_UNAUTHORIZED
IPC_INVALID_MESSAGE
IPC_TIMEOUT

EMERGENCY_TRIGGERED
EMERGENCY_RECOVERY_FAILED

DISPLAY_RESTORE_FAILED
INPUT_RESTORE_FAILED
RECOVERY_FAILED
```

---

# 52. Error Response Structure

Conceptually:

```text id="err002"
ErrorResponse:
    code
    category
    retryable
    user_message
    diagnostic_id
```

Never include:

- password
- TOTP
- Access Key
- session secret
- stack trace

in a normal browser-facing error.

---

# 53. Retry Semantics

Each error must specify whether retry is appropriate.

Examples:

```text id="retry001"
NETWORK_FAILED
    retryable = true

AUTH_INVALID
    retryable = false until new credentials supplied

SESSION_REVOKED
    retryable = false

GNOME_UNAVAILABLE
    retryable = conditional

DISPLAY_ISOLATION_FAILED
    retryable = no automatic retry unless recovery logic explicitly permits it
```

Do not blindly retry safety-critical operations indefinitely.

---

# 54. Authentication vs Authorization Errors

Keep these concepts distinct internally.

Authentication:

```text id="auth007"
Who are you?
```

Authorization:

```text id="auth008"
Are you allowed to perform this operation?
```

Session validity:

```text id="auth009"
Is your current session still valid?
```

Control authority:

```text id="auth010"
Do you currently possess the right to control input?
```

These must not be collapsed into one boolean.

---

# 55. Browser Reconnect Protocol

After network interruption:

```text id="reconn001"
NETWORK_LOSS
    ↓
connection lost
    ↓
lease eventually invalid
    ↓
reconnect
```

The browser must not assume its old authority remains valid.

On reconnect:

```text id="reconn002"
validate session
validate epoch
validate authentication
validate lease
validate GNOME state
```

If any fail:

```text id="reconn003"
NEW AUTHORIZATION REQUIRED
```

---

# 56. Session Resume

Session resume may be supported only when safe.

A resume request must contain:

```text id="resume001"
session_id
session_credential
client_id
security_epoch
```

The host independently verifies all fields.

Do not trust browser memory of:

```text id="resume002"
"I was connected before."
```

---

# 57. Browser Refresh

A browser refresh must not accidentally create duplicate control sessions.

Expected:

```text id="refresh001"
refresh
    ↓
session reconciliation
    ↓
existing authority validated
```

If validation fails:

```text id="refresh002"
session terminated / reauthentication required
```

---

# 58. Browser Tab Duplication

A duplicated tab should not automatically create a second controller.

Policy should be explicit.

Recommended:

```text id="tabs001"
same session
+
same client
+
second tab
    ↓
read-only / control denied
```

or require explicit control transfer.

---

# 59. Protocol Replay Protection

Requests must not be replayable indefinitely.

Use appropriate combinations of:

- short-lived credentials
- timestamps
- nonces
- request IDs
- sequence numbers
- security epoch
- session binding
- lease binding

Replay of an old control request must not cause input or state changes.

---

# 60. Message Validation

Every message must validate:

- protocol version
- message type
- required fields
- field types
- field lengths
- allowed values
- session binding
- host binding
- client binding
- epoch
- authorization

Malformed input must be rejected before privileged processing.

---

# 61. Size Limits

Every network and IPC message should have bounded size.

Reject:

- oversized messages
- deeply nested structures
- malformed serialization
- excessive metadata
- unexpected binary payloads

This protects against simple resource-exhaustion attacks.

---

# 62. Serialization

Use a well-defined serialization format.

Potential choices include:

- JSON for browser-facing control/signalling
- a binary protocol where performance or strict schemas justify it

The final choice should be made based on:

- browser compatibility
- schema validation
- security
- implementation complexity
- debugging requirements

Do not invent an unnecessary custom serialization format.

---

# 63. Schema Versioning

Messages should support schema evolution.

Example:

```text id="schema001"
message_type:
    SESSION_STATUS

schema_version:
    2
```

New optional fields should not break older compatible clients.

Security-critical fields must not become optional merely for compatibility.

---

# 64. Protocol State Validation

The host must validate protocol requests against the authoritative state machine.

Example:

```text id="schema002"
REQUEST_CONTROL
```

while:

```text id="schema003"
state = LOCAL_LOCKED
```

may be valid only after the appropriate authenticated session flow.

But:

```text id="schema004"
SEND_REMOTE_INPUT
```

while:

```text id="schema005"
state = AUTHENTICATING
```

must always be rejected.

---

# 65. Privileged Operation Policy

The protocol must never expose a generic operation such as:

```text id="priv001"
execute_privileged_action(name, args)
```

Instead expose narrowly defined operations:

```text id="priv002"
lock_session()
restore_display()
restore_input()
revoke_remote()
verify_safe_state()
```

Each operation should have:

- explicit authorization
- bounded arguments
- predictable effects
- audit event
- tests

---

# 66. Gateway Security Boundary

The gateway must assume it may eventually be compromised.

Therefore a compromised gateway must NOT automatically gain:

- Linux password
- TOTP secret
- Remote Access Key
- host private key
- arbitrary host commands
- unrestricted GNOME control

The host must enforce authentication and authorization independently.

Where practical, end-to-end sensitive operations should minimize what the gateway can observe.

---

# 67. Browser Security Boundary

The browser is untrusted client software.

Never rely on browser-side logic for:

- authorization
- session validity
- lease validity
- security epoch
- safety state

The browser can display:

```text id="browser001"
REMOTE_ACTIVE
```

but the host decides whether remote input is actually permitted.

---

# 68. Protocol Audit Trail

Record important protocol events:

```text id="audit001"
AUTHENTICATION_SUCCESS
AUTHENTICATION_FAILURE
SESSION_CREATED
SESSION_TERMINATED
LEASE_CREATED
LEASE_RENEWED
LEASE_EXPIRED
LEASE_REVOKED
EPOCH_CHANGED
EMERGENCY_TRIGGERED
CONTROL_GRANTED
CONTROL_REVOKED
RECOVERY_STARTED
RECOVERY_COMPLETED
```

Never log secrets.

---

# 69. Protocol Testing

Every message type requires tests for:

- valid request
- missing fields
- invalid types
- invalid values
- oversized payload
- stale session
- wrong host
- wrong client
- wrong epoch
- expired credential
- unauthorized caller
- replay
- duplicate request
- timeout
- concurrent request

---

# 70. Protocol Security Test

Attempt:

```text id="ptest001"
old session
+
old epoch
+
old lease
```

against:

```text id="ptest002"
current host
```

Expected:

```text id="ptest003"
REJECT
```

Attempt:

```text id="ptest004"
valid session
+
invalid lease
```

Expected:

```text id="ptest005"
remote input rejected
```

Attempt:

```text id="ptest006"
valid authentication
+
unauthorized privileged IPC
```

Expected:

```text id="ptest007"
IPC rejected
```

---

# 71. Emergency Race Test

Run:

```text id="race001"
REMOTE_ACTIVE
```

Then simultaneously:

```text id="race002"
client sends input
+
lease renewal
+
network disconnect
+
emergency trigger
```

Expected final condition:

```text id="race003"
remote authority:
    REVOKED

remote input:
    DISABLED

GNOME:
    LOCKED

physical display:
    RESTORED

physical input:
    RESTORED

stale lease:
    INVALID
```

The order of events must not produce an unsafe final state.

---

# 72. Protocol Timeout Test

Simulate:

```text id="timeout002"
GNOME agent stops responding
```

while:

```text id="timeout003"
host daemon is waiting for display/input preparation
```

Expected:

```text id="timeout004"
timeout
    ↓
do not assume operation succeeded
    ↓
recovery
    ↓
safe state
```

---

# 73. Protocol Restart Test

Restart:

- gateway
- host daemon
- GNOME agent

during active protocol operations.

Verify:

- stale sessions are rejected
- stale leases are rejected
- state is reconciled
- recovery is performed
- browser receives a meaningful state/error
- no stale remote input is accepted

---

# 74. Protocol Compatibility Test

Test:

```text id="compat001"
same protocol
older compatible client
newer client
unsupported client
unknown message
unknown capability
```

The host must reject unsupported security-sensitive combinations.

---

# 75. Protocol Documentation Requirements

Every public/internal protocol message should document:

```text id="doc001"
message name
purpose
sender
receiver
required fields
optional fields
authentication requirement
authorization requirement
state requirements
epoch requirement
lease requirement
expected response
error codes
retry semantics
security implications
```

---

# 76. Copilot Agent Instructions

When implementing protocol/API/IPC:

1. Inspect the repository and `adaptive-workflow-configurator` workflow first.
2. Follow established protocol and serialization conventions where appropriate.
3. Do not invent unnecessary abstractions.
4. Define schemas before implementing handlers.
5. Keep browser, gateway, host, GNOME agent, and emergency responsibilities separate.
6. Never expose arbitrary privileged operations.
7. Validate every field before processing.
8. Implement authentication and authorization independently.
9. Bind sessions to host, user, client, and security epoch.
10. Enforce control leases at the point where remote input is accepted.
11. Make safety-critical operations idempotent.
12. Add timeout handling.
13. Add replay protection.
14. Add concurrency/race tests.
15. Add compatibility/version tests.
16. Test stale sessions and stale leases.
17. Ensure emergency operations do not depend on the browser/network.
18. Never log protocol secrets.
19. Document every error code.
20. Do not consider protocol implementation complete until failure and security tests pass.

Implementation order:

```text id="agent02"
PROTOCOL MODEL
    ↓
SCHEMAS
    ↓
VERSION NEGOTIATION
    ↓
AUTHENTICATION
    ↓
SESSION
    ↓
SECURITY EPOCH
    ↓
CONTROL LEASE
    ↓
LOCAL IPC
    ↓
GNOME OPERATIONS
    ↓
BROWSER CONTROL
    ↓
WEBRTC SIGNALLING
    ↓
RECOVERY
    ↓
RACE TESTING
    ↓
SECURITY TESTING
```

---

# 77. Definition of Done

The protocol layer is complete only when:

```text id="done002"
[ ] Protocol versions defined
[ ] Capability negotiation defined
[ ] Host identity defined
[ ] Client identity defined
[ ] Authentication contract defined
[ ] Session contract defined
[ ] Session credential defined
[ ] Control lease defined
[ ] Security epoch enforced
[ ] Browser protocol defined
[ ] Gateway/host IPC defined
[ ] Host/GNOME IPC defined
[ ] Emergency IPC defined
[ ] Message validation implemented
[ ] Size limits implemented
[ ] Replay protection implemented
[ ] Timeout behavior implemented
[ ] Idempotency implemented
[ ] Error codes defined
[ ] Retry semantics defined
[ ] Protocol compatibility tested
[ ] Stale sessions rejected
[ ] Stale leases rejected
[ ] Race conditions tested
[ ] Privilege boundaries tested
[ ] Secrets excluded from logs
[ ] Emergency remains network-independent
[ ] Documentation complete
```

---

# 78. Final Protocol Invariant

The most important protocol rule is:

```text id="final03"
NETWORK CONNECTION
    ≠
AUTHENTICATION

AUTHENTICATION
    ≠
AUTHORIZATION

AUTHORIZATION
    ≠
REMOTE CONTROL

REMOTE CONTROL
    ≠
PERMANENT AUTHORITY
```

Instead:

```text id="final04"
AUTHENTICATION
      ↓
AUTHORIZATION
      ↓
SESSION
      ↓
CURRENT SECURITY EPOCH
      ↓
CURRENT CONTROL LEASE
      ↓
VALID GNOME STATE
      ↓
REMOTE INPUT ALLOWED
```

And at any point:

```text id="final05"
AUTHORITY LOST
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

**The protocol must enforce the security model; it must never merely transport commands from the browser to the workstation.**