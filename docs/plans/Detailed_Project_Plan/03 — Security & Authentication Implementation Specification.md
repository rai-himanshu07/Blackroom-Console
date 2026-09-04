# Security & Authentication Implementation Specification

## 1. Purpose

This document defines the security architecture and implementation requirements for the Remote Console project.

It covers:

- Linux username/password authentication
- PAM
- mandatory TOTP
- Remote Access Key
- trusted clients
- client credentials
- session authentication
- remote-control leases
- security epochs
- revocation
- emergency takeover
- secret storage
- authentication rate limiting
- session lifecycle
- authorization
- capability control
- security logging
- recovery

This document is normative for the implementation.

If another design conflicts with this document, do not silently choose one. Document the conflict and resolve it explicitly.

---

# 2. Security Objective

The system provides remote interactive control over a real logged-in Linux desktop session.

This makes the system security-sensitive.

The most important security objective is:

> A remote failure, expired credential, compromised client session, or ambiguous system state must never leave the workstation both unlocked and remotely controllable.

The system must fail closed.

---

# 3. Authentication Model

Every remote session requires:

1. Linux system username
2. Linux system password
3. TOTP authenticator code

A new or untrusted client additionally requires:

4. Remote Access Key

Therefore:

## Trusted client

```text
Linux username
+
Linux password
+
TOTP
+
trusted-client credential
```

## New/untrusted client

```text
Linux username
+
Linux password
+
TOTP
+
Remote Access Key
```

TOTP is mandatory in both cases.

A trusted client must never bypass TOTP.

---

# 4. Why the Credentials Are Separate

The credentials serve different purposes.

## Linux username

Identifies the Linux account.

## Linux password

Proves knowledge of the Linux account credential through the system's authentication mechanism.

## TOTP

Provides a separate authentication factor.

## Remote Access Key

Explicitly authorizes remote access from a new/untrusted client.

## Trusted client credential

Identifies a previously authorized client.

These credentials must not be merged or derived from each other.

---

# 5. Credential Independence

Do not implement:

```text
Remote Access Key = hash(username + password)
```

Do not implement:

```text
TOTP secret = derived from password
```

Do not implement:

```text
client credential = derived from Remote Access Key
```

Generate each credential independently.

---

# 6. PAM

Use Linux PAM for system-account authentication.

The application should not create a second Linux password database.

Conceptually:

```text
Browser
   |
   v
Authentication service
   |
   v
PAM
   |
   v
Linux account
```

Research the correct Ubuntu 26.04 PAM integration before implementation.

Do not assume PAM behavior from older Ubuntu releases.

---

# 7. Password Handling

The Linux password is highly sensitive.

Requirements:

- never log it
- never persist it
- never cache it
- never send it to the GNOME session agent
- never send it through WebRTC
- never store it in the session database
- never put it in URLs
- never put it in browser storage
- never include it in error messages
- never include it in crash reports

The password should exist only for the minimum time required for authentication.

---

# 8. PAM Helper Architecture

Prefer isolating PAM interaction.

Potential architecture:

```text
remote-hostd
     |
     | authenticated local IPC
     v
PAM authentication helper
     |
     v
PAM
```

The exact architecture may differ after research.

If a helper is used:

- minimize privileges
- minimize lifetime
- minimize API surface
- do not allow arbitrary PAM configuration from remote clients
- do not expose PAM directly over the network

---

# 9. TOTP

Use standard TOTP.

The implementation must be compatible with standard authenticator applications.

Examples:

- Google Authenticator
- Microsoft Authenticator
- Authy
- Aegis
- Bitwarden
- 1Password
- other standards-compliant TOTP clients

Do not implement vendor-specific APIs.

---

# 10. TOTP Setup

TOTP configuration should happen through a secure local setup flow.

Example:

```text
Remote Access Security

Two-factor authentication

Status: NOT CONFIGURED

[Configure authenticator]
```

Generate:

```text
TOTP secret
```

Display:

```text
QR code
```

User scans it.

User enters the current TOTP.

Server verifies it.

Only then:

```text
TOTP: CONFIGURED
```

---

# 11. TOTP Secret Protection

The TOTP secret is more sensitive than an individual OTP value.

Never log:

- TOTP secret
- QR payload
- provisioning URI

Never return the secret after initial configuration unless an explicit secure recovery/reset workflow requires it.

Store it securely.

Evaluate:

- root-only filesystem permissions
- OS keyring/secret service where appropriate
- encryption at rest
- secure backup behavior

Do not automatically choose an encryption mechanism without understanding how unattended boot and service startup will work.

---

# 12. TOTP Verification

Every remote session must validate a current TOTP.

Do not accept:

- cached OTPs
- previously used session OTPs
- a TOTP from an old authentication session

Use a small acceptable clock-skew window appropriate for TOTP.

Document the chosen window.

Do not make the window unnecessarily large.

---

# 13. TOTP Replay

Protect against replay where practical.

An OTP that has already successfully authenticated a login should not be accepted repeatedly within the same TOTP time step if doing so would materially increase replay risk.

Document the exact implementation.

---

# 14. TOTP Reset

Resetting TOTP is a sensitive operation.

Do not allow:

```text
remote session
→ disable TOTP
```

without strong additional authorization.

A TOTP reset must require an appropriate recovery process.

The recovery mechanism must not become an easy bypass of the Remote Access Key.

---

# 15. Remote Access Key

The Remote Access Key is the dedicated secret for authorizing new/untrusted remote clients.

Generate it automatically.

Do not ask the user to create it.

Use a cryptographically secure random generator.

Target:

```text
>= 256 bits of entropy
```

Encode using a safe representation such as Base64url.

---

# 16. Remote Access Key UI

Use the term:

> Remote Access Key

Do not call it an API key in the primary user interface.

Explain:

> The Remote Access Key is required when connecting from a new or untrusted device. Store it securely in a password manager.

The user does not need to memorize it.

---

# 17. Remote Access Key Storage

Prefer storing a secure verifier rather than plaintext where practical.

The design must support verification without exposing the plaintext secret to unrelated components.

Never:

- log it
- place it in URLs
- put it in cookies
- place it in browser localStorage
- return it through a status API
- send it to the GNOME session agent
- expose it to the WebRTC data plane

---

# 18. Remote Access Key Rotation

Provide:

```text
[Rotate Remote Access Key]
```

Rotation invalidates the previous key.

New/untrusted clients must use the new key.

Trusted clients do not necessarily need to be revoked when the key rotates.

Provide a separate operation:

```text
[Rotate Key + Revoke All Clients]
```

for users who want a complete reset.

---

# 19. Trusted Clients

Trusted clients are a convenience mechanism.

They are not the root authentication mechanism.

A trusted client should have a cryptographic client credential.

Conceptually:

```text
Trusted Client

client_id
public_key
credential metadata
created_at
last_used_at
status
```

The client private key must remain protected on the client side.

---

# 20. New Client Registration

New client flow:

```text
New browser
    |
    v
Username
    +
Password
    +
TOTP
    +
Remote Access Key
    |
    v
Authentication successful
    |
    v
Offer:
"Trust this device?"
    |
    +---- No ----> temporary session
    |
    +---- Yes ---> register client credential
```

Physical access to the Ubuntu host is not required.

This is critical.

A user must be able to access the machine from a completely different laptop while physically away.

---

# 21. Trusted Client Authentication

Trusted client:

```text
client credential
+
username
+
password
+
TOTP
```

The Remote Access Key is not required.

TOTP remains mandatory.

Do not implement:

```text
trusted client
→ credential only
→ access
```

---

# 22. Unknown Device Recovery

If the user is on a completely new laptop:

```text
New laptop
    |
    v
username
password
TOTP
Remote Access Key
    |
    v
Authenticated
    |
    v
Optional:
Trust this device
```

No physical interaction with the Ubuntu host is required.

This must work through the rendezvous/network architecture.

---

# 23. Device Revocation

Provide:

```text
Trusted Devices

Laptop A
Laptop B
Tablet

[Revoke]
[Revoke All]
```

Revocation must:

1. prevent new sessions from using the credential
2. invalidate active sessions belonging to the credential
3. invalidate its remote-control lease
4. remove/disable its authorization state

---

# 24. Session Credentials

Authentication credentials and remote-session credentials must be separate.

Do not use:

- Linux password
- TOTP secret
- Remote Access Key

as the session token.

After successful authentication, issue a short-lived session credential.

---

# 25. Session Token Properties

A remote session credential should be:

- unpredictable
- short-lived
- revocable
- associated with host
- associated with user
- associated with client
- associated with session
- associated with security epoch
- capability-limited

Do not expose session tokens unnecessarily.

---

# 26. Browser Session Storage

Do not store permanent secrets in:

- URL
- query string
- ordinary localStorage
- plaintext IndexedDB
- logs

Use secure browser mechanisms appropriate to the implementation.

Short-lived session state may be held using secure cookies or an equivalent secure mechanism.

Research the browser architecture before deciding the exact mechanism.

---

# 27. Authentication Session vs Remote Session

Maintain a distinction.

```text
Authentication
      |
      v
Authenticated principal
      |
      v
Remote session
      |
      v
Control lease
```

Authentication does not automatically grant indefinite remote control.

---

# 28. Remote-Control Lease

Create an explicit control lease.

Conceptually:

```text
ControlLease
    session_id
    host_id
    user_id
    client_id
    security_epoch
    issued_at
    expires_at
    capabilities
```

The lease is temporary authority.

---

# 29. Lease Renewal

While the remote connection is healthy:

```text
client
  |
  | heartbeat
  v
host
  |
  | renew lease
  v
active
```

If the heartbeat fails:

```text
lease expires
    |
    v
remote input disabled
```

Do not depend solely on TCP/WebRTC connection state.

The application-level lease is an additional safety boundary.

---

# 30. Lease Expiration

When a lease expires:

1. stop accepting remote input
2. invalidate remote control
3. terminate or transition the remote session
4. initiate fail-safe recovery
5. lock GNOME
6. restore physical display
7. restore physical input

The exact cleanup order must be validated against the GNOME PoC.

---

# 31. Security Epoch

Maintain:

```text
security_epoch
```

Example:

```text
security_epoch = 17
```

Every remote session and control lease records the current epoch.

---

# 32. Epoch Invalidation

Emergency takeover:

```text
epoch 17
   |
   v
epoch 18
```

All credentials/leases bound to epoch 17 become invalid.

This must happen before the system considers the emergency action complete.

---

# 33. Why Epochs Are Useful

They provide a simple global revocation mechanism.

Instead of tracking every possible remote session individually:

```text
Is session valid?

session.epoch == current_security_epoch
```

If false:

```text
REJECT
```

The implementation may additionally maintain explicit per-session revocation.

---

# 34. Authorization

Authentication answers:

> Who are you?

Authorization answers:

> What are you allowed to do?

Do not combine these concepts.

---

# 35. Initial Capabilities

v1 should support:

```text
VIEW
CONTROL
```

Future capabilities may include:

```text
CLIPBOARD
FILES
POWER
SETTINGS
```

Do not implement future capabilities prematurely.

---

# 36. Control Authorization

Only a client with the `CONTROL` capability and a valid control lease may send:

- keyboard input
- pointer movement
- pointer buttons
- scrolling

A `VIEW` client must not be able to send input.

---

# 37. No Generic Remote Command

Never expose:

```text
execute(command)
```

or equivalent.

The remote protocol should contain explicit operations.

Example:

```text
send_keyboard_event
send_pointer_event
request_control
release_control
start_remote_session
stop_remote_session
```

Every operation must pass authorization checks.

---

# 38. One Controller in v1

Only one remote client may have `CONTROL` at a time.

If another client requests control:

```text
Existing controller
        |
        v
New controller request
```

Default behavior:

```text
REJECT
```

Future versions may implement explicit handoff.

---

# 39. Authentication Error Handling

Do not expose detailed authentication failure information.

Avoid:

```text
username exists
password correct
TOTP wrong
```

Return a generic error:

> Authentication failed.

Internally log enough information for diagnosis without exposing secrets.

---

# 40. Rate Limiting

Rate-limit:

- username/password attempts
- TOTP attempts
- Remote Access Key attempts
- device registration attempts
- session establishment attempts

Use increasing delays or other appropriate protection.

Do not make legitimate users permanently unable to recover access because of overly aggressive rate limits.

---

# 41. Brute Force Protection

Consider rate limits at multiple levels:

```text
IP
+
host/device
+
account
+
client identity
```

Do not rely only on IP-based throttling because NAT can put many legitimate users behind one IP.

Likewise, do not rely only on username-based throttling because attackers can distribute usernames.

---

# 42. Session Revocation

Implement:

```text
Revoke Session
Revoke Device
Revoke All Sessions
Rotate Access Key
Disable Remote Access
Emergency Takeover
```

Each operation must immediately invalidate remote-control authority.

---

# 43. Emergency Revocation

Emergency takeover is the strongest local revocation operation.

It must:

1. revoke active control lease
2. terminate remote sessions
3. increment security epoch
4. restore physical console
5. lock GNOME
6. restore physical input

It must not depend on the remote client cooperating.

---

# 44. Emergency Does Not Mean Unlock

After emergency takeover:

```text
LOCKED
```

not:

```text
LOCAL_ACTIVE
```

The physical user must perform normal GNOME authentication/unlock.

---

# 45. Optional Emergency Disable

Allow configuration:

```text
Emergency behavior

( ) Revoke current remote session
(x) Revoke session + disable remote access
```

If remote access is disabled:

- terminate current remote session
- invalidate leases
- increment epoch
- lock session
- restore physical console
- prevent new remote connections

Re-enable should require deliberate local action.

---

# 46. Remote Access Disable

When the user locally disables remote access:

```text
Remote Access: DISABLED
```

the host must:

1. reject new connections
2. terminate active remote sessions
3. revoke control leases
4. increment security epoch
5. restore physical console
6. lock if remote mode was active

---

# 47. Credential Changes

Changing:

- Linux password
- TOTP
- Remote Access Key
- trusted client list

must have explicitly defined effects.

Document them.

Recommended defaults:

### Linux password changed

Existing remote sessions:

- preferably terminate
- require fresh authentication

Trusted clients:

- remain trusted

### TOTP changed/reset

Terminate existing remote sessions.

Require fresh TOTP.

### Access Key rotated

Invalidate old Access Key.

Trusted clients:

- remain trusted unless explicitly revoked

### Trusted client revoked

Terminate its active sessions.

---

# 48. Session Binding

Where practical, bind sessions to:

- host identity
- user identity
- client identity
- security epoch

Avoid bearer credentials that can be freely transferred between contexts.

---

# 49. Transport Security

All remote communication must use secure transport.

The architecture should use:

```text
HTTPS/WebSocket
+
WebRTC
```

WebRTC should provide encrypted media/data transport.

Do not implement custom cryptography.

Do not invent a custom encryption protocol.

Use established cryptographic libraries/protocols.

---

# 50. Rendezvous Security

The rendezvous server should not receive:

- Linux password
- TOTP
- TOTP secret
- Remote Access Key
- private keys
- desktop pixels
- keyboard contents
- clipboard contents

It should primarily coordinate discovery and connection establishment.

---

# 51. TURN Security

TURN relays should transport encrypted traffic.

The TURN server should not be able to decrypt the remote desktop content.

Treat TURN as untrusted infrastructure.

---

# 52. Host Identity

Generate a cryptographic host identity during installation.

Conceptually:

```text
host_id
host_private_key
host_public_key
```

Do not use:

- IP address
- MAC address
- hostname

as the cryptographic identity.

---

# 53. Client Identity

Trusted clients receive:

```text
client_id
client_public_key
```

The private key must remain on the client.

The host stores only what is necessary to validate the client credential.

---

# 54. Secret Storage Architecture

Centralize secrets.

Example abstraction:

```text
SecretStore

    Host identity
    TOTP secret
    Remote Access Key verifier
    Trusted client credentials
    Revocation metadata
```

Do not scatter security-sensitive material through arbitrary configuration files.

---

# 55. Secret Access Boundaries

Not every component should have access to every secret.

Example:

```text
remote-gateway
    |
    | no access to secrets
    v

remote-hostd
    |
    +-- authentication secrets
    +-- client authorization
    |
    v

GNOME session agent
    |
    | no password/TOTP/Access Key
```

The GNOME session agent should not need the user's password or TOTP secret.

---

# 56. Password Manager Compatibility

The Remote Access Key should be convenient to store in:

- password managers
- secure notes
- enterprise secret managers

Avoid formats that are difficult to copy safely.

Do not use ambiguous characters unnecessarily if the user may need to manually transcribe it.

However, copy/paste is the preferred workflow.

---

# 57. Recovery Codes

Provide recovery codes for loss of the TOTP authenticator.

Recovery codes must be:

- generated securely
- shown only during setup/recovery
- stored securely by the user
- single-use
- invalidated after use
- never logged

Recovery codes must not create an unintended bypass of the Remote Access Key for a new/untrusted client.

---

# 58. Recovery Flow

A potential recovery flow:

```text
Username
+
Linux password
+
Recovery code
+
Remote Access Key
```

Then:

```text
authenticated
    |
    v
reset/configure new TOTP authenticator
```

The exact workflow must be designed carefully before implementation.

---

# 59. TOTP Device Loss

If the user loses their authenticator device:

```text
Recovery code
+
Linux password
+
Remote Access Key
```

may be used to establish recovery.

After recovery:

1. invalidate used recovery code
2. configure new TOTP
3. terminate old sessions
4. optionally revoke trusted clients
5. increment security epoch

---

# 60. Security Logging

Security logs may contain:

- timestamp
- event
- host ID
- client ID
- session ID
- account identifier where appropriate
- result
- reason
- security epoch

Never log:

- password
- TOTP value
- TOTP secret
- Remote Access Key
- private key
- session token

---

# 61. Security Events

Log events including:

```text
AUTH_SUCCESS
AUTH_FAILURE
PASSWORD_FAILURE
TOTP_FAILURE
ACCESS_KEY_FAILURE
CLIENT_REGISTERED
CLIENT_REVOKED
SESSION_CREATED
SESSION_TERMINATED
LEASE_CREATED
LEASE_EXPIRED
LEASE_REVOKED
ACCESS_KEY_ROTATED
TOTP_CHANGED
REMOTE_ACCESS_ENABLED
REMOTE_ACCESS_DISABLED
EMERGENCY_TAKEOVER
SECURITY_EPOCH_CHANGED
```

Avoid excessive logging of normal input events.

Do not log every keyboard key.

---

# 62. Privacy of Logs

Remote desktop systems handle potentially sensitive information.

Do not log:

- keystrokes
- clipboard contents
- screen contents
- typed passwords
- arbitrary application data

Logs should contain metadata only.

---

# 63. Browser Security

The browser application must enforce:

- HTTPS
- strict Content Security Policy
- secure cookies
- SameSite cookies
- appropriate CSRF protection
- strict WebSocket origin validation
- session expiration
- secure logout
- no credentials in URL
- no permanent secrets in localStorage

---

# 64. Origin Validation

WebSocket/WebRTC signalling endpoints must validate the expected browser origin.

Do not accept arbitrary cross-origin control connections.

Do not assume TLS alone solves browser-origin attacks.

---

# 65. Clickjacking

Protect the authentication/control UI from clickjacking.

Use appropriate HTTP response headers and Content Security Policy.

---

# 66. Session Expiration

Remote sessions must have an expiration mechanism.

The exact duration should be configurable.

However:

- the control lease should expire independently
- authentication session should not automatically imply indefinite control

---

# 67. Explicit Disconnect

When the user clicks:

```text
Disconnect
```

the host should:

1. revoke control lease
2. terminate remote input
3. terminate remote session
4. lock GNOME
5. restore physical display
6. restore physical input
7. destroy virtual monitor
8. remain locked

---

# 68. Browser Crash

If browser disappears:

```text
WebRTC connection lost
```

The host must not wait indefinitely.

Lease expiration must eventually trigger fail-safe recovery.

---

# 69. Network Failure

If network connectivity disappears:

```text
network failure
    |
    v
heartbeat fails
    |
    v
lease expires
    |
    v
remote input revoked
    |
    v
GNOME locked
    |
    v
physical console restored
```

The exact timeout must be configurable but conservative.

Document the default.

---

# 70. Main Daemon Failure

If `remote-hostd` crashes:

The system must not remain in an unsafe remote state.

Use:

- systemd supervision
- watchdog
- lease expiration
- independent emergency controller

The design must ensure recovery even if the main daemon is unavailable.

---

# 71. Emergency Daemon Failure

The emergency daemon itself is security-critical.

Minimize its dependencies.

It should not depend on:

- network
- browser
- WebRTC
- main application
- remote gateway

Document what happens if the emergency daemon itself crashes.

Provide another safety mechanism where technically feasible.

---

# 72. Privilege Separation

Never make the whole application privileged.

Preferred model:

```text
Web Gateway
    unprivileged

remote-hostd
    limited system privileges

GNOME agent
    user session

emergencyd
    minimal required privileges
```

Any privileged operation must have a documented reason.

---

# 73. D-Bus Security

Do not give the Internet-facing gateway unrestricted D-Bus access.

The gateway should communicate with `remote-hostd` through a narrow authenticated IPC interface.

The host daemon should expose only explicit operations.

---

# 74. IPC Security

Define explicit IPC messages.

Example:

```text
AuthenticateRequest
AuthenticateResult

CreateRemoteSession
RemoteSessionCreated

AcquireControlLease
LeaseGranted

RenewControlLease
LeaseRenewed

RevokeLease
LeaseRevoked

EmergencyTakeover
EmergencyCompleted

RestorePhysicalConsole
PhysicalConsoleRestored
```

Do not expose a generic RPC mechanism that can invoke arbitrary methods.

---

# 75. Remote Input Authorization

Every remote input operation should effectively pass:

```text
session valid?
AND
client valid?
AND
security epoch valid?
AND
control lease valid?
AND
CONTROL capability present?
AND
system in REMOTE_ACTIVE state?
```

If any condition is false:

```text
REJECT
```

---

# 76. Input Fail-Closed

When uncertain:

```text
Is remote input authorization valid?
```

The answer must default to:

```text
NO
```

Do not default to accepting input when authorization state is unavailable.

---

# 77. Display Isolation Fail-Closed

If the system cannot confirm that physical display isolation has succeeded:

Do not enter:

```text
REMOTE_ACTIVE
```

Remain in preparation/failure state.

The user must never unknowingly enter remote mode while the physical screen is still displaying the remote desktop.

---

# 78. Activation Transaction

Remote activation should behave like a transaction.

Resources:

```text
control lease
remote session
virtual monitor
PipeWire stream
remote input
physical display isolation
physical input isolation
```

Only transition to `REMOTE_ACTIVE` after all required resources are ready.

---

# 79. Activation Failure

Example:

```text
Remote session created
Virtual monitor created
PipeWire started
Remote input started
Physical display isolation FAILED
```

The system must:

1. revoke lease
2. disable remote input
3. destroy PipeWire stream
4. destroy virtual monitor
5. restore physical display
6. restore physical input
7. lock if required
8. return to safe state

Never leave partial state behind.

---

# 80. Recovery Must Be Idempotent

The following operations must be safe if executed multiple times:

```text
revoke_remote_session()
restore_physical_console()
lock_session()
increment_epoch()
disable_remote_input()
```

Emergency recovery should not become less safe if triggered twice.

---

# 81. Security State Persistence

Persist only what is necessary.

Persist:

- host identity
- TOTP configuration
- Access Key verifier
- trusted clients
- security epoch
- relevant configuration

Do not persist:

- passwords
- active keyboard input
- screen data
- unnecessary session secrets

---

# 82. Security Epoch Persistence

Security epoch should survive daemon restarts.

Otherwise:

```text
epoch = 42

emergency

epoch = 43

daemon restart

epoch = 42
```

could accidentally resurrect previously invalidated credentials.

The exact persistence mechanism must prevent rollback.

---

# 83. Clock Considerations

TOTP depends on time.

The system should detect severe clock problems.

Document:

- acceptable clock skew
- behavior when system clock changes significantly
- behavior during suspend/resume
- behavior when NTP adjusts time

Do not silently broaden TOTP validity to compensate for broken clocks.

---

# 84. Concurrent Authentication

Define behavior if two clients authenticate simultaneously.

Example:

```text
Client A → authentication
Client B → authentication
```

Both may authenticate if authorized.

However, only one should receive `CONTROL` in v1.

The control lease decides who has control.

---

# 85. Existing Session Ownership

Remote authentication must map to a valid Linux user/session.

Do not allow:

```text
authenticated user A
→ control user B's GNOME session
```

unless explicitly designed and authorized.

For v1:

> Remote access is for the authenticated local user and that user's existing GNOME session.

---

# 86. Multi-User Restrictions

Do not implement multi-user remote desktop access in v1.

If multiple users exist on the host, clearly determine which account is configured for remote access.

Only the explicitly configured account should be remotely controllable.

---

# 87. Remote Access Enablement

Remote access must be explicitly enabled.

Example:

```text
Remote Access
[ ON ]
```

Installation should not silently expose the machine to the network.

---

# 88. Default Security Posture

Recommended defaults:

```text
Remote Access: OFF until setup completed
Linux password: REQUIRED
TOTP: REQUIRED
Remote Access Key: REQUIRED for new clients
Trusted clients: optional
Physical display isolation: ON
Physical input isolation: ON
Lock on disconnect: ON
Emergency recovery: ON
Session lease: ON
```

---

# 89. First-Time Setup

The setup wizard should guide the user through:

1. enable remote access
2. configure TOTP
3. generate Remote Access Key
4. configure emergency shortcut
5. test emergency recovery
6. optionally register first trusted client
7. verify diagnostic checks
8. explicitly confirm remote access

---

# 90. Remote Access Key Display

The key should be displayed securely.

Provide:

```text
Copy
Regenerate
```

Avoid:

```text
Share
```

or other UI that could accidentally expose it.

After leaving the setup screen, don't display the plaintext key again unless explicitly regenerating it.

---

# 91. Remote Access Key Regeneration

Regeneration should require strong local confirmation.

Example:

```text
Regenerate Remote Access Key?

This will invalidate the existing key for new/untrusted devices.

[Cancel] [Regenerate]
```

Do not silently rotate the key.

---

# 92. Trusted Client Naming

When registering a client, allow a human-readable name:

```text
Work Laptop
Personal Laptop
Tablet
```

Do not use the name as a security identity.

The cryptographic client ID remains authoritative.

---

# 93. Client Fingerprint

Display a short fingerprint for trusted clients.

Example:

```text
Work Laptop

Client ID:
abc123...

Fingerprint:
AB12 CD34 EF56 ...
```

This can help identify/revoke clients.

Do not rely on fingerprints as authentication by themselves.

---

# 94. Remote Session UI

The browser should show:

```text
Connected
User: <account>
Device: <client name>
Capabilities: View + Control
Session duration: ...
```

Do not expose secrets.

---

# 95. Security Status UI

Provide:

```text
Security

TOTP
✓ Configured

Remote Access Key
✓ Configured

Trusted Devices
3

Active Sessions
1

Emergency Recovery
✓ Tested

Security Epoch
12
```

Never display the TOTP secret or Access Key plaintext in normal status views.

---

# 96. "Revoke All Sessions"

Provide an explicit operation:

```text
[Revoke All Sessions]
```

It should:

1. terminate remote sessions
2. revoke leases
3. increment security epoch
4. restore physical console if required
5. lock GNOME if remote mode was active

This is a user-facing emergency security control.

---

# 97. "Disable Remote Access"

Provide:

```text
[Disable Remote Access]
```

This is different from simply disconnecting the current client.

It prevents new remote connections.

---

# 98. Security Auditability

Create a security event log that allows the user to answer:

- when was remote access used?
- from which client?
- which sessions were active?
- was a client revoked?
- was emergency takeover triggered?
- was the Access Key rotated?
- was TOTP changed?

Do not log private desktop activity.

---

# 99. Threat Model

Create:

```text
docs/security/threat-model.md
```

Analyze at least:

### Attacker knows host IP

Expected:

```text
Cannot authenticate.
```

### Attacker knows username

Expected:

```text
Still requires password + TOTP.
```

### Attacker knows username + password

Expected:

```text
Still requires TOTP.
```

### Attacker knows Remote Access Key

Expected:

```text
Still requires username + password + TOTP.
```

### Attacker steals trusted client credential

Expected:

```text
TOTP still required.
```

### Attacker compromises TURN server

Expected:

```text
Cannot decrypt desktop contents.
```

### Attacker obtains active session token

Expected:

```text
Limited by expiration, revocation, lease and epoch.
```

### Attacker has physical access

Analyze separately.

### Attacker triggers emergency shortcut

Expected:

```text
Remote access revoked.
Machine locked.
Physical console restored.
```

---

# 100. Physical Attacker Considerations

The emergency mechanism is not intended to defend against a fully compromised operating system.

If an attacker has:

- root
- kernel compromise
- physical disk access
- firmware compromise

application-level controls cannot provide absolute guarantees.

Document this clearly.

The emergency mechanism is primarily designed for:

- remote software failure
- stuck remote connection
- accidental remote state
- unwanted remote control
- local user takeover

---

# 101. Supply Chain Security

Remote access software has extremely high impact.

Future release infrastructure must support:

- signed releases
- authenticated update metadata
- secure update transport
- dependency review
- vulnerability disclosure
- SBOM
- rollback protection where practical

Do not build an automatic self-updater before its security model is defined.

---

# 102. Cryptography Rules

Do not implement cryptographic primitives yourself.

Use mature libraries for:

- random number generation
- hashing
- signatures
- TOTP
- key handling
- secure transport

Do not invent:

- custom encryption
- custom authentication handshake
- custom password hashing
- custom key exchange

---

# 103. Randomness

All security-sensitive random values must use a cryptographically secure OS-backed RNG.

This includes:

- Remote Access Key
- session IDs
- nonces
- client credentials
- recovery codes
- host identity keys

Never use:

- timestamps
- predictable counters
- `rand()`
- UUIDs as secrets unless their randomness properties are explicitly appropriate

---

# 104. Memory Handling

Where practical:

- minimize credential lifetime
- avoid unnecessary copies
- avoid logging sensitive structures
- clear sensitive buffers where appropriate
- use secure secret abstractions

Do not claim guaranteed memory wiping unless the implementation can actually guarantee it.

---

# 105. Error Handling

Security-sensitive errors must fail closed.

Examples:

```text
Cannot validate lease
    → reject input

Cannot validate client
    → reject connection

Cannot determine session state
    → do not enter REMOTE_ACTIVE

Cannot restore display
    → remain in failsafe and attempt recovery

Cannot verify authorization
    → deny
```

---

# 106. No Security by Obscurity

Do not rely on:

- hidden URLs
- random port numbers
- obscure device names
- secret hostnames
- hidden API endpoints

as authentication mechanisms.

The Remote Access Key, password and TOTP are actual credentials.

---

# 107. Security Documentation

Maintain:

```text
docs/security/
    architecture.md
    threat-model.md
    authentication.md
    credential-lifecycle.md
    incident-response.md
    security-testing.md
```

Keep documentation synchronized with implementation.

---

# 108. Security Testing

Automated tests should cover:

## Authentication

- correct credentials
- wrong password
- wrong TOTP
- wrong Access Key
- expired TOTP
- revoked client
- rotated Access Key
- invalid session
- expired session

## Authorization

- view-only client cannot control
- unauthorized client cannot create control lease
- expired lease cannot send input
- old epoch cannot send input
- revoked client cannot reconnect

## Revocation

- session revoke
- device revoke
- revoke all
- emergency takeover
- Access Key rotation

## Failure

- gateway crash
- host daemon crash
- session agent crash
- network failure
- browser failure
- PipeWire failure
- emergency operation

---

# 109. Security Fuzzing

Where practical, fuzz:

- authentication protocol parsing
- WebSocket messages
- WebRTC signalling messages
- IPC messages
- remote input messages
- configuration parsing
- serialized session/lease structures

Never fuzz against a production system.

---

# 110. Dependency Security

Use appropriate tooling such as:

```text
cargo audit
```

and dependency review.

Track:

- Rust crates
- JavaScript packages
- system libraries
- WebRTC dependencies
- PipeWire dependencies
- GNOME libraries

Minimize unnecessary dependencies.

---

# 111. Privileged Code Review

Every privileged function must have:

- clear purpose
- minimal input
- explicit authorization
- explicit failure behavior
- test coverage
- documentation

Particular scrutiny is required for:

- emergencyd
- input isolation
- display configuration
- PAM helper
- systemd integration

---

# 112. Security Review Checklist Before Release

Verify:

```text
[ ] TOTP mandatory for every session
[ ] Access Key required for new clients
[ ] Trusted clients cannot bypass TOTP
[ ] Linux password never stored
[ ] TOTP secret protected
[ ] Access Key protected
[ ] Session credentials expire
[ ] Control leases expire
[ ] Epoch revocation works
[ ] Device revocation works
[ ] Session revocation works
[ ] Emergency takeover works
[ ] Network failure fails closed
[ ] Daemon failure fails closed
[ ] Physical input isolation works
[ ] Physical display isolation works
[ ] No arbitrary command execution
[ ] Gateway not unrestricted root
[ ] Secrets absent from logs
[ ] Browser secrets protected
[ ] Rate limiting implemented
[ ] Dependencies reviewed
```

---

# 113. Security Acceptance Test

The following scenario must pass before production readiness.

## Setup

Host:

```text
Ubuntu 26.04
GNOME 50+
Wayland
Remote Access enabled
TOTP configured
Remote Access Key configured
Trusted Laptop A
```

## Test A — trusted device

Laptop A connects.

Authentication:

```text
username
password
TOTP
trusted client
```

Expected:

```text
Remote Access Granted
```

Access Key must not be required.

---

# 114. Security Acceptance Test — New Device

Laptop B has never connected.

Authentication:

```text
username
password
TOTP
Remote Access Key
```

Expected:

```text
Remote Access Granted
```

Optionally register Laptop B as trusted.

---

# 115. Security Acceptance Test — Wrong TOTP

Laptop A:

```text
username ✓
password ✓
TOTP ✗
```

Expected:

```text
DENIED
```

The trusted-client credential must not bypass TOTP.

---

# 116. Security Acceptance Test — Wrong Access Key

Laptop B:

```text
username ✓
password ✓
TOTP ✓
Access Key ✗
```

Expected:

```text
DENIED
```

---

# 117. Security Acceptance Test — Revocation

Laptop A is connected.

Revoke Laptop A.

Expected:

```text
active session terminated
control lease revoked
future authentication denied
```

---

# 118. Security Acceptance Test — Emergency

Remote session active.

Press emergency shortcut.

Expected:

```text
remote input revoked
remote session terminated
security epoch incremented
GNOME locked
physical display restored
physical input restored
```

Laptop must not be able to reconnect using its previous session.

---

# 119. Security Acceptance Test — Emergency With Main Daemon Failure

Remote session active.

Kill the main remote daemon.

Trigger emergency shortcut.

Expected:

```text
emergency controller still works
remote authority revoked
physical console restored
GNOME locked
```

This is a hard production gate.

---

# 120. Security Acceptance Test — Old Lease

Create a valid control lease.

Increment security epoch.

Attempt remote input using old lease.

Expected:

```text
REJECT
```

---

# 121. Security Acceptance Test — Different Laptop While Physically Away

Host remains unattended.

Laptop A is unavailable.

Laptop B connects from a different network.

Laptop B has no trusted credential.

User supplies:

```text
username
password
TOTP
Remote Access Key
```

Expected:

```text
authentication successful
remote session established
```

No physical interaction with the host is required.

This is a mandatory product requirement.

---

# 122. Security Design Summary

The final security model is:

```text
                      REMOTE CLIENT
                            |
                            v
                    Transport Security
                            |
                            v
                    Authentication
                            |
          +-----------------+----------------+
          |                 |                |
       Username          Password          TOTP
          |                 |                |
          +-----------------+----------------+
                            |
                    New client?
                       /          \
                     YES           NO
                      |             |
               Remote Access Key   Trusted
                      |             |
                      +------+------+
                             |
                             v
                      Authorization
                             |
                             v
                      Session Credential
                             |
                             v
                       Control Lease
                             |
                             v
                     Security Epoch
                             |
                             v
                       GNOME Session
```

The critical security invariant remains:

> **No valid authentication + authorization + control lease = no remote input.**

And:

> **If remote control becomes ambiguous or unhealthy, the system must fail closed by revoking remote authority, locking the GNOME session, restoring the physical console, and restoring local input.**

---

# 123. Implementation Order

Implement this security architecture in the following order:

### Step 1

Define security data structures.

### Step 2

Implement host identity.

### Step 3

Implement secure secret storage.

### Step 4

Implement PAM authentication.

### Step 5

Implement TOTP setup and verification.

### Step 6

Implement Remote Access Key generation/storage/rotation.

### Step 7

Implement authentication state machine.

### Step 8

Implement trusted client credentials.

### Step 9

Implement session credentials.

### Step 10

Implement security epoch.

### Step 11

Implement control leases.

### Step 12

Integrate authorization with remote input.

### Step 13

Implement revocation.

### Step 14

Integrate emergency takeover.

### Step 15

Test failure paths.

Only after these are working should they be connected to the production WebRTC/browser layer.

---

# 124. Final Copilot Agent Instruction

Implement this specification conservatively.

Before coding:

1. inspect the repository
2. inspect the workflow configured by `adaptive-workflow-configurator`
3. inspect existing architecture
4. research current Ubuntu 26.04/PAM/GNOME 50 behavior
5. identify security-sensitive dependencies
6. document implementation decisions

Do not:

- weaken mandatory TOTP
- make trusted clients bypass TOTP
- make device pairing mandatory for all access
- store Linux passwords
- expose secrets to the browser unnecessarily
- expose secrets to the GNOME session agent unnecessarily
- implement custom cryptography
- create arbitrary remote command execution
- run the entire application as root
- silently change security semantics for convenience

The intended user experience is:

```text
Previously trusted device:

Username
+
Linux Password
+
TOTP
+
Trusted Client Credential
        |
        v
Remote Access
```

and:

```text
New/untrusted device:

Username
+
Linux Password
+
TOTP
+
Remote Access Key
        |
        v
Remote Access
        |
        v
Optional:
Trust This Device
```

TOTP is mandatory in both cases.

A user must be able to access their configured Ubuntu workstation from a completely different device while physically away from the workstation.

The security architecture must always prioritize:

```text
DENY
+
LOCK
+
RESTORE
```

over:

```text
ALLOW
+
HOPE
```

Do not declare the security implementation complete until the acceptance tests in this document pass.