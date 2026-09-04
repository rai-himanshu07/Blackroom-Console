# 09 — Threat Model, Security Boundaries & Abuse Cases

## 1. Purpose

This document defines the security model for the remote-console system.

It specifies:

- security objectives
- assets
- trust boundaries
- attacker capabilities
- authentication threats
- session threats
- remote-input threats
- display-privacy threats
- browser threats
- network threats
- local privilege threats
- credential theft scenarios
- replay scenarios
- emergency takeover requirements
- denial-of-service considerations
- abuse cases
- security invariants
- security test requirements

This document is intended to guide implementation, code review, penetration testing, and release acceptance.

The system must fail closed.

---

# 2. Security Objective

The primary security objective is:

> Allow an authorized remote user to control the existing GNOME workstation while ensuring that loss, theft, interruption, or compromise of remote connectivity cannot silently leave the workstation under unauthorized remote control.

The system has two distinct security goals:

### Confidentiality

An unauthorized person must not be able to:

- view the workstation screen;
- access remote media;
- obtain clipboard contents;
- observe user activity.

### Integrity

An unauthorized person must not be able to:

- inject keyboard input;
- inject pointer input;
- control the GNOME session;
- bypass authentication;
- retain control after session termination;
- regain control after emergency takeover.

Physical privacy is also a first-class requirement.

---

# 3. Security Model

The system should be considered secure only when all of these are true:

```text id="q7j4m2"
REMOTE ACCESS
      ↓
STRONG AUTHENTICATION
      ↓
AUTHORIZATION
      ↓
SHORT-LIVED CONTROL LEASE
      ↓
VALID SECURITY EPOCH
      ↓
SAME GNOME SESSION
      ↓
PHYSICAL DISPLAY ISOLATED
      ↓
PHYSICAL INPUT ISOLATED
      ↓
REMOTE CONTROL
```

Breaking any mandatory link must prevent or revoke remote control.

---

# 4. Assets

The following are security-sensitive assets.

## 4.1 User Credentials

- Linux username
- Linux password
- TOTP secret
- recovery codes

---

## 4.2 Remote Access Key

The Remote Access Key is equivalent to a high-value authentication credential.

Possession must be treated as sensitive.

---

## 4.3 Trusted-Device Credentials

Trusted-device credentials authorize a device to skip the Remote Access Key.

They must therefore be:

- cryptographically strong;
- scoped;
- revocable;
- protected from casual browser extraction.

They do not replace password + TOTP.

---

## 4.4 Session Credentials

Short-lived remote session credentials grant access to an authenticated session.

They must not be equivalent to the long-term Remote Access Key.

---

## 4.5 Control Lease

The control lease is particularly sensitive because it authorizes interactive input.

A valid session without a valid control lease must not permit remote keyboard/pointer control.

---

## 4.6 Security Epoch

The security epoch is the global revocation mechanism for remote sessions.

A stale epoch must never be accepted.

---

## 4.7 GNOME Session

The existing physical user's GNOME session is an asset.

The system must not accidentally:

- expose it to unauthorized users;
- unlock it;
- switch it to an unintended session;
- destroy it unnecessarily.

---

## 4.8 Physical Display

The display can expose:

- documents;
- passwords;
- source code;
- browser sessions;
- internal applications;
- notifications.

Remote mode must isolate the physical display.

---

## 4.9 Physical Input

Physical keyboard and mouse provide local control authority.

During remote operation they must not unexpectedly control the session.

---

## 4.10 Host Identity

The cryptographic identity of the workstation protects against:

- DNS spoofing;
- IP changes;
- hostname changes;
- malicious rendezvous entries;
- connecting to the wrong machine.

---

# 5. Trust Boundaries

Primary architecture:

```text id="g4j8r0"
                UNTRUSTED NETWORK
                       |
                       v
              +------------------+
              | Browser Client   |
              +--------+---------+
                       |
                  HTTPS/WebRTC
                       |
                       v
              +------------------+
              | Remote Gateway   |
              | Unprivileged     |
              +--------+---------+
                       |
                 Authenticated IPC
                       |
                       v
              +------------------+
              | remote-hostd     |
              | Privileged       |
              +--------+---------+
                       |
                 Authenticated IPC
                       |
                       v
              +------------------+
              | GNOME Agent      |
              | User Session     |
              +--------+---------+
                       |
              +--------+---------+
              |                  |
              v                  v
           Mutter            PipeWire/libei
```

Emergency path:

```text id="g5f2q1"
Physical keyboard
       |
       v
remote-emergencyd
       |
       +--> revoke remote authority
       +--> lock GNOME
       +--> restore display
       +--> restore input
```

The emergency path must remain independent of the network path.

---

# 6. Security Boundary: Browser

The browser must be considered untrusted.

Do not assume:

```text id="0f6m4k"
browser JavaScript
browser storage
browser extension environment
browser network
```

is trustworthy.

The host must enforce authorization independently.

---

# 7. Security Boundary: Gateway

The gateway is exposed to potentially hostile network traffic.

Assume attackers can:

- connect repeatedly;
- send malformed requests;
- open many WebSockets;
- send invalid authentication attempts;
- send oversized payloads;
- attempt protocol confusion;
- attempt resource exhaustion.

The gateway must therefore be:

- unprivileged;
- strongly sandboxed;
- minimal;
- rate-limited;
- unable to execute arbitrary host commands.

---

# 8. Security Boundary: remote-hostd

`remote-hostd` is the primary security authority.

It must control:

- authentication;
- authorization;
- session credentials;
- control leases;
- security epoch;
- trusted devices;
- Remote Access Key;
- policy;
- session lifecycle.

It should expose narrowly defined operations.

It must not become a generic privileged RPC service.

---

# 9. Security Boundary: GNOME Agent

The GNOME agent operates inside the user's session.

It should have access to:

- Mutter;
- PipeWire;
- libei/EIS;
- GNOME session facilities.

It should not possess unnecessary system privileges.

The agent must validate commands received from `remote-hostd`.

---

# 10. Security Boundary: Emergency Daemon

`remote-emergencyd` has exceptional privilege.

Therefore it must have extremely little functionality.

It should not contain:

- WebRTC;
- HTTP;
- authentication UI;
- browser logic;
- codec handling;
- general shell execution;
- arbitrary D-Bus forwarding;
- plugin loading.

Principle:

> The component with the most dangerous privilege must contain the least functionality.

---

# 11. Attacker Classes

The system should consider at least the following attackers.

## A. Internet attacker

Can send arbitrary network traffic.

---

## B. LAN attacker

Can observe or manipulate local network traffic and attempt host discovery.

---

## C. Credential attacker

Possesses some but not necessarily all authentication factors.

---

## D. Stolen-device attacker

Obtains a trusted browser/device.

---

## E. Stolen Remote Access Key attacker

Possesses the Remote Access Key but not necessarily the password/TOTP.

---

## F. Malicious local user

Has access to the workstation but is not authorized for remote control.

---

## G. Compromised browser

The browser environment has been compromised by:

- malicious extension;
- malware;
- injected JavaScript;
- stolen browser profile.

---

## H. Compromised gateway

Assume the gateway may be compromised.

The host should remain protected by its own authentication and authorization model.

---

## I. Malicious authenticated user

An authenticated user may attempt to abuse remote control beyond intended capabilities.

---

## J. Faulty software

Not all threats are malicious.

The system must protect against:

- crashes;
- races;
- network loss;
- partial failures;
- unexpected GNOME behavior;
- hardware changes.

---

# 12. Authentication Threat Model

Required normal authentication:

```text id="7m4p8s"
username
+
Linux password
+
TOTP
```

For new/untrusted clients:

```text id="6d2w9q"
username
+
Linux password
+
TOTP
+
Remote Access Key
```

For trusted clients:

```text id="0x5c7k"
username
+
Linux password
+
TOTP
+
trusted-device credential
```

No single credential should be sufficient for remote control.

---

# 13. Password Attacks

Threats:

- brute force;
- credential stuffing;
- password spraying;
- leaked Linux password;
- reused password.

Mitigations:

- PAM authentication;
- rate limiting;
- temporary authentication throttling;
- generic errors;
- audit logging;
- TOTP;
- Remote Access Key for new clients.

Do not implement custom password verification if PAM can be used correctly.

---

# 14. TOTP Attacks

Threats:

- brute force;
- replay within validity window;
- stolen authenticator secret;
- phishing.

Mitigations:

- standard RFC 6238 TOTP;
- server-side validation;
- rate limiting;
- limited clock-skew window;
- no secret exposure;
- no secret logging.

Do not build vendor-specific Google/Microsoft authenticator integration.

---

# 15. TOTP Replay

TOTP naturally has a short validity window.

The implementation should consider preventing repeated use of the same time-step/code during a single authentication flow where practical.

Do not expand the accepted clock window unnecessarily.

---

# 16. Remote Access Key Threat

The Remote Access Key is a high-value credential.

An attacker possessing it should still be unable to connect without:

```text id="7v1z2p"
username
password
TOTP
```

for a new device.

The key should therefore be an additional factor rather than the sole authentication mechanism.

---

# 17. Trusted Device Theft

If a trusted browser/device is stolen:

The attacker may possess the trusted-device credential.

However, they should still require:

```text id="x8k4q0"
username
password
TOTP
```

to establish a new remote session.

The trusted credential should not be treated as a replacement for MFA.

---

# 18. Trusted Device Revocation

The user must be able to revoke an individual trusted device.

Revocation must invalidate:

- future authentication using that credential;
- associated active sessions where applicable.

The host remains authoritative.

---

# 19. Revoke All

"Revoke All" must terminate active sessions and invalidate trusted-device credentials.

A security epoch increment is recommended.

Conceptually:

```text id="m5q2w8"
REVOKE_ALL
     |
     v
security_epoch++
     |
     +--> terminate sessions
     +--> invalidate leases
     +--> revoke trusted devices
```

---

# 20. Session Hijacking

Threat:

An attacker steals a valid remote session credential.

Mitigations:

- TLS;
- short-lived session credentials;
- control leases;
- security epoch;
- host-side authorization;
- no credentials in URLs;
- secure browser storage;
- session revocation.

The host must not assume possession of a session credential automatically grants indefinite control.

---

# 21. Control-Lease Theft

Even if an attacker obtains stale session information, remote input must require a valid current control lease.

The lease must include:

```text id="2t8f6p"
session ID
client ID
security epoch
expiration
capabilities
```

---

# 22. Replay Attack

An attacker may replay:

- authentication requests;
- session requests;
- control requests;
- old lease messages;
- old trusted-device credentials.

Mitigations:

- TLS;
- server-side session state;
- nonces/challenge mechanisms where appropriate;
- expiration;
- security epoch;
- unique session IDs;
- strict request validation.

Do not design a stateless control protocol where an old message can re-enable input.

---

# 23. Security-Epoch Attack

Threat:

An attacker retains an old remote session and waits for the user to disconnect/emergency-stop.

After emergency:

```text id="0g9v4d"
epoch_old != epoch_current
```

The old session must be rejected.

This is mandatory.

---

# 24. Emergency Race Condition

Threat:

The attacker attempts to send input immediately while the user triggers emergency takeover.

Required behavior:

```text id="q4m6r8"
emergency
    >
normal remote input
```

Emergency must have higher priority.

After emergency starts:

> No new remote input may be accepted.

---

# 25. Reconnect Race

Threat:

A stale remote client reconnects while teardown is occurring.

Example:

```text id="3p9x7c"
TEARING_DOWN
     |
     +-- stale reconnect
```

The reconnect must be rejected.

The system must not permit:

```text id="n4c8v2"
teardown old session
+
activate same session
```

simultaneously.

---

# 26. Display Privacy Threat

Threat:

Remote user disconnects, but the physical display remains showing the desktop.

Required:

```text id="1y6v9k"
remote ends
    ↓
GNOME locked
    ↓
physical display restored
```

The exact physical-display behavior must be verified experimentally.

A black fullscreen application is not considered sufficient.

---

# 27. Display Restoration Attack

Threat:

An attacker attempts to retain visual access by preventing display restoration.

The system should ensure:

- remote media stops;
- remote authority is revoked;
- GNOME locks;
- display restoration is attempted;
- restoration is verified.

If restoration cannot be verified:

```text id="7b5m1q"
FAILED_SAFE
```

---

# 28. Physical Input Threat

Threat:

Physical keyboard/mouse remain active while remote control is active.

This could allow:

- local user interference;
- accidental input;
- unauthorized local takeover;
- unpredictable state.

The system must positively verify the physical input-isolation mechanism.

This is a hard feasibility gate.

---

# 29. Physical Input Restoration

After remote termination:

```text id="9d2k7m"
remote input disabled
physical input restored
GNOME locked
```

The implementation must verify restoration.

---

# 30. Emergency Shortcut Attack

The emergency shortcut must not be trivially triggered by accidental input.

Recommended:

```text id="r7m3x9"
specific key combination
+
approximately 2-second hold
```

The exact combination and duration should be configurable.

The implementation should avoid making it too easy for arbitrary applications to spoof the trigger.

---

# 31. Emergency Shortcut Reliability

The emergency mechanism must work even when:

- GNOME is frozen;
- browser is frozen;
- network is disconnected;
- WebRTC is broken;
- `remote-gateway` has crashed;
- `remote-hostd` is unhealthy.

This is a core safety requirement.

---

# 32. Emergency Shortcut Denial of Service

An attacker with physical access may deliberately trigger the emergency shortcut.

This is not considered an authorization bypass.

The result is:

```text id="n7c2p4"
remote access terminated
workstation locked
```

This is preferable to allowing remote control to continue.

---

# 33. Privilege Escalation

The privileged components must not provide generic execution capabilities.

Forbidden APIs include concepts such as:

```text id="g0m6y2"
execute(command)
run_as_root(command)
dbus_proxy(method, ...)
shell(...)
```

Do not expose arbitrary command execution to the browser, gateway, or GNOME agent.

---

# 34. D-Bus Abuse

D-Bus access must be narrowly scoped.

Do not expose a generic D-Bus proxy from:

```text id="2x7p9m"
network
→ gateway
→ hostd
→ D-Bus
```

Only explicit operations should be exposed.

---

# 35. File-System Abuse

Privileged services should have minimal filesystem access.

They must not unnecessarily read:

```text id="k4w1z8"
user home directories
SSH keys
browser profiles
documents
source repositories
```

except where explicitly required.

---

# 36. Credential Storage

Sensitive values must be stored using appropriate host-side mechanisms.

At minimum:

```text id="3p7v2m"
password
    -> PAM / system authentication

TOTP secret
    -> protected local secret storage

Remote Access Key
    -> preferably verifier/hash rather than plaintext

trusted-device credentials
    -> protected credential storage
```

Never store secrets in application logs.

---

# 37. Browser Storage Threat

Do not store long-lived secrets in:

```text id="z8r3m5"
localStorage
URL
query string
page source
analytics
console logs
```

Use an appropriate secure browser mechanism for trusted-device credentials.

Document residual browser-security limitations.

---

# 38. XSS Threat

A cross-site scripting vulnerability in the remote client could expose:

- session credentials;
- trusted-device credentials;
- remote control capabilities.

Therefore:

- strict Content Security Policy;
- avoid unsafe HTML;
- avoid dynamic script injection;
- sanitize untrusted values;
- avoid unnecessary third-party scripts;
- keep dependencies minimal.

---

# 39. CSRF Threat

State-changing HTTP operations must use appropriate CSRF protection where applicable.

Particularly:

- authentication;
- device registration;
- device revocation;
- Remote Access Key rotation;
- session termination;
- configuration changes.

Do not assume WebRTC encryption solves HTTP authorization threats.

---

# 40. Clickjacking

The management/authentication interface should prevent embedding by untrusted sites.

Use appropriate:

- CSP;
- frame-ancestors policy;
- related browser protections.

---

# 41. WebSocket Abuse

WebSocket connections must enforce:

- authentication;
- authorization;
- origin validation where applicable;
- message-size limits;
- rate limits;
- connection limits;
- idle timeouts;
- explicit protocol state.

Do not assume a WebSocket connection is trusted merely because TLS is enabled.

---

# 42. WebRTC Abuse

WebRTC negotiation must be authorized.

An attacker should not be able to:

- allocate unlimited sessions;
- force expensive media pipelines;
- consume unlimited resources;
- receive media without authorization.

---

# 43. TURN / Relay Threat

TURN infrastructure may observe:

- traffic metadata;
- connection timing;
- IP information.

Where architecture permits, the relay should not have access to decrypted remote-session content.

The relay must not be treated as a trusted host authority.

---

# 44. Rendezvous Server Compromise

If the rendezvous service is compromised:

It may attempt to:

- redirect clients;
- provide false host addresses;
- observe metadata;
- perform denial of service.

Cryptographic host identity and authenticated transport must prevent silent impersonation.

---

# 45. DNS Attack

DNS may be compromised or manipulated.

The system must not treat:

```text id="c7q2n9"
hostname == trusted host
```

as sufficient identity.

Host identity must be cryptographically verifiable.

---

# 46. LAN Discovery Threat

mDNS/Avahi can be used for convenience.

However, discovery information is not authentication.

The client must still verify host identity and authenticate.

---

# 47. Man-in-the-Middle

Required protections:

```text id="w5j8r2"
TLS
+
host identity verification
+
authenticated session
```

Do not implement custom encryption instead of TLS/WebRTC security.

---

# 48. Denial of Service

The system cannot guarantee availability against a sufficiently capable network attacker.

It must, however, prevent simple resource exhaustion.

Controls should include:

- connection limits;
- authentication rate limits;
- request-size limits;
- WebSocket limits;
- session limits;
- WebRTC allocation limits;
- timeouts;
- bounded subprocesses;
- systemd resource limits.

---

# 49. Authentication Enumeration

Unauthenticated clients must not be able to determine:

- whether a username exists;
- whether a trusted device exists;
- whether a Remote Access Key is valid;
- whether a particular account is configured.

Use generic authentication failures.

---

# 50. Session Enumeration

Session IDs must be cryptographically unpredictable.

Never use:

```text id="9x4b2p"
incrementing integer
timestamp
username
hostname
```

as the sole session identifier.

---

# 51. Host Enumeration

The gateway should not reveal unnecessary information about hosts.

Avoid exposing:

- OS details;
- usernames;
- internal IPs;
- installed packages;
- GNOME version;
- hardware information

before authentication unless explicitly needed.

---

# 52. Information Leakage

Security-sensitive information must not leak through:

- HTTP status differences;
- error messages;
- timing differences where practical;
- logs exposed to clients;
- browser console;
- URLs;
- WebSocket messages;
- diagnostics endpoints.

Perfect side-channel resistance is not required, but obvious enumeration paths must be avoided.

---

# 53. Malicious Authenticated User

An authenticated user should still be constrained to the intended capabilities.

Do not expose:

```text id="z1n6m3"
shell
terminal
arbitrary process execution
filesystem browsing
privileged D-Bus
```

unless separately designed and explicitly authorized.

The remote console is for controlling the GNOME session, not for becoming a generic privileged administration channel.

---

# 54. Clipboard Exfiltration

If clipboard support is implemented, treat it as a separate security capability.

Potential threats:

- password exfiltration;
- API-key exfiltration;
- confidential document exfiltration.

Initial implementation should consider leaving clipboard disabled until the core remote-control system is secure.

---

# 55. File Transfer

File transfer should be out of scope for the initial security model.

It significantly expands the attack surface.

Do not add it merely because remote-desktop products commonly provide it.

---

# 56. Browser Extension Threat

Browser extensions may have broad access to pages and storage.

The system cannot fully protect a compromised browser profile.

Mitigations:

- minimize long-lived client secrets;
- require password + TOTP every session;
- allow trusted-device revocation;
- provide "Revoke All";
- never place secrets in URLs.

---

# 57. Stolen Browser Profile

If an attacker copies a browser profile:

The attacker may obtain trusted-device material depending on the storage mechanism.

They should still require:

```text id="b6v9r4"
username
password
TOTP
```

to establish control.

Users should be able to revoke the device.

---

# 58. Stolen Remote Access Key

Possession of the Remote Access Key must not be sufficient.

Required new-device flow:

```text id="x9q4s7"
Remote Access Key
+
username
+
password
+
TOTP
```

If the key is believed compromised:

```text id="g8m2p5"
rotate key
+
revoke affected sessions
```

according to the documented policy.

---

# 59. Lost Authenticator

Recovery should require:

```text id="h4k7q1"
username
password
recovery code
Remote Access Key
```

for an untrusted device.

Recovery codes must be:

- high entropy;
- one-time;
- revocable/regeneratable;
- stored securely;
- never logged.

---

# 60. Lost Remote Access Key

If the user loses the key:

A local authenticated administrator/setup flow should allow rotation.

Do not provide a remote unauthenticated mechanism to recover the key.

---

# 61. Emergency After Credential Theft

Suppose an attacker currently controls a remote session.

The user triggers emergency takeover.

Required:

```text id="p2x8n5"
revoke remote input
+
terminate session
+
increment epoch
+
lock
+
restore physical display
+
restore physical input
```

The attacker must not be able to immediately reconnect using the stale session.

Normal authentication is required for a new session.

---

# 62. Emergency After Main-Daemon Compromise

Even if `remote-hostd` is malfunctioning, the emergency daemon should remain able to perform its safety function.

This requires careful privilege separation.

The emergency daemon should not trust arbitrary commands from `remote-hostd`.

---

# 63. Compromised GNOME Agent

If the user-session agent becomes compromised, the attacker may potentially affect the GNOME session.

The architecture should limit what the agent can do outside its user-session scope.

The system daemon should not give the agent unnecessary privileged operations.

---

# 64. Compromised Gateway

If the gateway is compromised:

It must not be sufficient to:

- bypass host authentication;
- issue valid control leases;
- increment/decrement security epoch;
- execute host commands;
- access secrets.

The gateway should function as a transport/signalling component rather than a security authority.

---

# 65. Compromised Host

If the entire Ubuntu host is compromised with root-level malware:

> This system cannot provide meaningful security guarantees.

The threat model assumes the underlying operating system and kernel are trustworthy.

This limitation must be explicitly documented.

---

# 66. Physical Attacker

A person with unrestricted physical access to the workstation may be able to:

- unplug monitors;
- disconnect keyboards;
- reboot the machine;
- modify hardware;
- access storage.

This is outside the application's complete protection boundary.

The emergency mechanism is intended for:

> an authorized local user needing to regain control from an active remote session.

It is not a physical-security system.

---

# 67. Lock-Screen Security

After remote teardown:

```text id="n5x7c2"
GNOME LOCKED
```

must be the expected local state.

Remote access must not automatically unlock the session.

---

# 68. Notifications

GNOME notifications can leak sensitive information onto the physical display.

The physical display should be isolated during remote operation.

After remote termination, the screen should be locked.

The implementation should consider whether notifications are visible on the virtual remote display and document the behavior.

---

# 69. Remote Display Confidentiality

The remote client receives the contents of the GNOME session.

Therefore an authenticated remote user effectively has screen-level access.

This is intentional.

The system must ensure the remote stream is not accidentally delivered to:

- unauthenticated clients;
- another active session;
- another host;
- unauthorized gateway users.

---

# 70. Session Confusion

A particularly dangerous class of bugs is attaching to the wrong GNOME session.

Before activation, verify:

```text id="k9m3r1"
target user
target UID
target session ID
Wayland display
GNOME session
seat
Mutter instance
```

The system must never blindly select "the first available session."

---

# 71. Session Confusion After Logout

If the original user logs out while a remote session is active:

```text id="w2q7m8"
terminate remote session
invalidate lease
restore safe state
```

Do not attach automatically to another user's session.

---

# 72. Multi-User Safety

Initial implementation is single-user.

If multiple desktop users exist on the machine, the system should explicitly reject unsupported conditions rather than guessing which session to control.

---

# 73. Security Logging

Log security-relevant events:

```text id="q4m8x2"
authentication success/failure
trusted-device registration
trusted-device revocation
Remote Access Key rotation
session creation
session termination
lease expiration
security epoch change
emergency takeover
state-machine failures
recovery failures
```

Do not log credentials.

---

# 74. Audit Correlation

Use:

```text id="f6r2q8"
host_id
session_id
client_id
transition_id
security_epoch
```

to correlate security events.

This allows investigation without logging secret material.

---

# 75. Secret-Handling Rules

Never log or expose:

```text id="w1q5k7"
Linux password
TOTP secret
TOTP recovery codes
Remote Access Key
trusted-device private credential
session bearer credential
control lease token
```

Even debug builds should avoid printing them.

---

# 76. Dependency Security

The project should:

- minimize dependencies;
- pin/constraint versions appropriately;
- monitor known vulnerabilities;
- avoid unnecessary JavaScript packages;
- avoid abandoned authentication libraries;
- prefer mature cryptographic libraries.

Do not implement cryptographic primitives manually.

---

# 77. Supply-Chain Security

Production builds should eventually support:

- reproducible or auditable builds where practical;
- dependency lockfiles;
- dependency vulnerability scanning;
- signed release artifacts;
- source provenance.

These are release-hardening requirements, not prerequisites for the initial feasibility PoC.

---

# 78. Systemd Security

System services should use appropriate sandboxing.

Potential controls include:

```text id="m8q2x5"
NoNewPrivileges
PrivateTmp
ProtectSystem
ProtectHome
RestrictAddressFamilies
RestrictNamespaces
RestrictSUIDSGID
CapabilityBoundingSet
DeviceAllow
MemoryMax
TasksMax
```

Do not copy security settings blindly.

Every restriction must be tested against actual functionality.

---

# 79. Capability Minimization

Do not grant:

```text id="g3r7m2"
CAP_SYS_ADMIN
CAP_SYS_PTRACE
CAP_NET_ADMIN
```

or similar powerful capabilities unless demonstrably required.

If a capability is unavoidable:

- isolate it;
- document why;
- minimize the process holding it;
- test its scope;
- avoid combining it with unrelated functionality.

---

# 80. Emergency Privilege

If emergency handling requires privileged access to physical input or other system facilities:

The privileged helper must be extremely small.

Preferred architecture:

```text id="j5v8n2"
remote-emergencyd
    |
    +-- physical trigger
    +-- fixed safety operations
```

Not:

```text id="x3k9p6"
remote-emergencyd
    |
    +-- plugin system
    +-- shell
    +-- network
    +-- HTTP
    +-- arbitrary D-Bus
```

---

# 81. Abuse Case: Remote Input Without Authentication

Expected:

```text id="v5m2q8"
DENY
```

No display or input authority should be granted.

---

# 82. Abuse Case: Correct Password, Wrong TOTP

Expected:

```text id="p7x4n1"
DENY
```

No remote session.

---

# 83. Abuse Case: Correct Password + TOTP, Wrong Remote Access Key

For new/untrusted client:

```text id="r8q3m6"
DENY
```

---

# 84. Abuse Case: Trusted Credential Without TOTP

Expected:

```text id="k2w7p4"
DENY
```

---

# 85. Abuse Case: Stale Session After Emergency

Expected:

```text id="x6m9q2"
DENY
```

because the security epoch changed.

---

# 86. Abuse Case: Reconnect During Teardown

Expected:

```text id="n4p8r1"
DENY
```

until a valid new session is established.

---

# 87. Abuse Case: Gateway Attempts Privileged Command

Expected:

```text id="j7q3v9"
DENY
```

The gateway has no generic privileged execution path.

---

# 88. Abuse Case: Malformed IPC

Expected:

```text id="c5m8x2"
reject request
log security event
keep service alive
```

Malformed IPC must not crash the privileged service where practical.

---

# 89. Abuse Case: Flood Authentication Endpoint

Expected:

```text id="q9v4m7"
rate limiting
bounded resource consumption
generic error
```

---

# 90. Abuse Case: Flood WebRTC Sessions

Expected:

```text id="r2k8p5"
connection/session limits
resource quotas
authorization
```

---

# 91. Abuse Case: Malicious Browser Origin

Expected:

```text id="m7x3q9"
reject unauthorized origin where applicable
```

The server must not trust arbitrary browser origins.

---

# 92. Abuse Case: Host Identity Changed

Expected:

```text id="v8n4k2"
warn user
stop connection
require explicit verification
```

Do not silently accept a new host identity.

---

# 93. Abuse Case: Main Daemon Crash

Expected:

```text id="x5q8m1"
remote lease expires
remote input revoked
session locked
local hardware restored
```

---

# 94. Abuse Case: Emergency During Main-Daemon Crash

Expected:

```text id="p3r7k9"
emergency still works
```

This is one of the most important integration tests.

---

# 95. Abuse Case: Physical Display Restoration Failure

Expected:

```text id="q8m4v2"
remote authority revoked
recovery attempted
FAILED_SAFE if uncertain
```

Never silently return to LOCAL_ACTIVE.

---

# 96. Abuse Case: Physical Input Restoration Failure

Expected:

```text id="k5x9p3"
remote authority revoked
safe recovery attempted
FAILED_SAFE if local control cannot be verified
```

---

# 97. Abuse Case: Power Loss During Remote Mode

After reboot:

```text id="n2v7m4"
old remote sessions invalid
safe local state
no automatic remote control
```

---

# 98. Abuse Case: GNOME Session Logout

Expected:

```text id="x7q3m8"
terminate remote session
invalidate lease
invalidate session state
```

Do not attach to another session.

---

# 99. Security Test Matrix

The implementation must eventually test at least:

| Scenario | Expected Result |
|---|---|
| No credentials | Denied |
| Wrong password | Denied |
| Wrong TOTP | Denied |
| Wrong Access Key | Denied |
| Trusted without TOTP | Denied |
| Revoked device | Denied |
| Expired session | Denied |
| Expired lease | Remote input revoked |
| Old security epoch | Denied |
| Emergency takeover | Immediate remote revocation |
| Reconnect after emergency | New authentication required |
| Browser refresh | No silent control restoration |
| Network loss | Lease expires safely |
| Gateway crash | Host eventually fails closed |
| Host daemon crash | Lease expires |
| GNOME agent crash | Remote control revoked |
| Mutter failure | Safe recovery |
| Display restore failure | FAILED_SAFE |
| Input restore failure | FAILED_SAFE |
| Power loss | Safe startup |
| Monitor hotplug | Safe topology handling |
| Malformed IPC | Rejected |
| Authentication flood | Rate limited |
| WebSocket flood | Bounded |
| Unauthorized D-Bus request | Denied |
| Gateway command execution attempt | Impossible |

---

# 100. Hard Security Gates

The following are release blockers.

## Gate A — Authentication

All required factors are enforced.

## Gate B — TOTP

TOTP cannot be bypassed for trusted clients.

## Gate C — Session Isolation

A valid authenticated session without a control lease cannot inject input.

## Gate D — Lease Expiration

Lost connectivity eventually revokes remote input.

## Gate E — Security Epoch

Emergency/revocation invalidates stale sessions.

## Gate F — Physical Input Isolation

Physical keyboard/mouse cannot control the remote session while remote mode is active.

## Gate G — Physical Display Isolation

Physical display does not expose the active desktop during remote mode.

## Gate H — Emergency Independence

Emergency takeover works independently of the main remote application.

## Gate I — Safe Teardown

Every normal/abnormal termination reaches a locked local state.

## Gate J — No Arbitrary Privileged Execution

No network-accessible component can execute arbitrary privileged commands.

---

# 101. Threats Explicitly Accepted

The initial version does not attempt to protect against:

- fully compromised Linux kernel;
- malicious root user;
- physically destructive attacks;
- compromised physical hardware;
- compromised authenticator device;
- completely compromised client operating system;
- availability loss caused by powerful network attackers.

These limitations must be documented.

---

# 102. Threats Not Accepted

The product must not knowingly accept:

- remote input without valid authentication;
- trusted-device bypass of TOTP;
- stale session reuse after revocation;
- stale session reuse after emergency;
- indefinite remote control after network loss;
- arbitrary privileged command execution;
- unauthenticated access to the GNOME session;
- silent attachment to the wrong user's session;
- automatic unlock after disconnect;
- emergency takeover dependent on the network;
- physical display remaining exposed after remote activation where isolation is claimed;
- physical input remaining uncontrolled where isolation is claimed.

---

# 103. Security Review Checklist

Before release, reviewers should ask:

```text id="w9m3k6"
[ ] What authenticates the user?
[ ] What authenticates the device?
[ ] What authorizes remote control?
[ ] What expires remote control?
[ ] What invalidates stale sessions?
[ ] What happens if the network disappears?
[ ] What happens if remote-hostd crashes?
[ ] What happens if GNOME crashes?
[ ] What happens if Mutter fails?
[ ] What happens if display restoration fails?
[ ] What happens if input restoration fails?
[ ] Can the emergency path work independently?
[ ] Can an old browser session reconnect after emergency?
[ ] Can the gateway execute privileged operations?
[ ] Can the browser obtain long-lived secrets?
[ ] Can the wrong GNOME session be selected?
[ ] Can remote input occur without a valid lease?
[ ] Can the system ever automatically unlock?
```

Every answer must be demonstrable from code and tests.

---

# 104. Implementation Instructions for GitHub Copilot Agent

When implementing this document:

1. Inspect the repository first.
2. Inspect the workflow/configuration created by `adaptive-workflow-configurator`.
3. Respect that workflow and do not replace it.
4. Treat this document as the adversarial/security contract.
5. Implement security boundaries before convenience features.
6. Keep privileged code minimal.
7. Keep the emergency path independent.
8. Do not expose generic privileged RPC.
9. Do not implement custom cryptographic primitives.
10. Use established cryptographic/TLS/PAM mechanisms.
11. Add negative/security tests before declaring features complete.
12. Add failure-injection tests for every critical state transition.
13. Never weaken a security invariant to make a test pass.
14. If a requirement cannot be safely implemented, stop and report the limitation rather than silently degrading it.

---

# 105. Final Security Principle

The system must assume that:

> **Remote connectivity is unreliable and remote credentials may eventually be exposed.**

Security therefore cannot depend on a single connection staying alive or a single secret remaining secret forever.

Instead, the system must continuously enforce:

```text id="r5k8m2"
AUTHENTICATION
      +
AUTHORIZATION
      +
CURRENT SESSION
      +
CURRENT SECURITY EPOCH
      +
CURRENT CONTROL LEASE
      +
VALID GNOME STATE
```

If any required condition disappears:

```text id="x2q7n4"
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

The safest state is the normal recovery state.

Remote control must always be temporary, explicitly authorized, continuously validated, and immediately revocable.