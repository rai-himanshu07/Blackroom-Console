# 08 — Browser / Remote Client UX & Interaction Flow

## 1. Purpose

This document defines the browser-based remote client experience.

It covers:

- connection discovery
- host selection
- authentication
- new vs trusted clients
- TOTP
- Remote Access Key
- session establishment
- remote-control activation
- connection status
- degraded connections
- reconnect
- disconnect
- emergency takeover feedback
- session expiration
- security errors
- browser security
- client-side state management

The browser is a **remote client**, not the authority for workstation security.

The host remains the source of truth for:

- authentication
- authorization
- session state
- trusted devices
- security epoch
- control lease
- GNOME state
- physical display/input isolation

---

# 2. Product UX Principle

The user should experience the product as:

> "Connect to my existing Ubuntu workstation securely."

Not:

> "Start another remote desktop session."

The remote client should make it clear that it is controlling the existing physical workstation session.

---

# 3. Supported Client

Initial client:

- modern desktop browser
- Chromium-based browsers
- Firefox where WebRTC/input compatibility permits
- laptop/desktop/tablet form factors

Mobile browser support may be considered, but should not block v1.

No native client is required initially.

---

# 4. Client Architecture

Conceptually:

```text id="l3jz0r"
                 Browser
                    |
          +---------+---------+
          |                   |
       HTTPS              WebRTC
          |                   |
          v                   v
      Gateway           Remote Session
          |
          |
      Signalling
```

The browser should not directly access privileged host functionality.

It communicates through authenticated host/gateway APIs.

---

# 5. Initial Connection Screen

The landing page should be minimal.

Example:

```text id="q84d7e"
Remote Console

Host
[ workstation.example.com ]

[ Connect ]

Advanced
  Connection details
```

If LAN discovery is available:

```text id="5p4c6u"
Available hosts

● workstation
  Ubuntu 26.04
  Available

[ Connect ]
```

Do not expose sensitive host information unnecessarily.

---

# 6. Host Identity

The client must distinguish a host using a cryptographic host identity.

Do not rely solely on:

```text id="qj1r30"
hostname
IP address
MAC address
```

A host may change:

- IP
- hostname
- network
- DHCP lease
- interface

while remaining the same cryptographic host.

---

# 7. First Connection

For an unknown/untrusted browser:

```text id="0n2f6r"
CONNECT
   ↓
HOST IDENTIFICATION
   ↓
USERNAME
   ↓
PASSWORD
   ↓
TOTP
   ↓
REMOTE ACCESS KEY
   ↓
AUTHENTICATE
   ↓
OPTIONALLY TRUST DEVICE
   ↓
REMOTE SESSION
```

All required factors must be completed.

---

# 8. Authentication Screen

Recommended UX:

```text id="5qdd6m"
Connect to workstation

Username
[________________]

Password
[________________]

Authenticator code
[______]

Remote Access Key
[____________________________]

[ Sign in ]
```

For a trusted client, the Remote Access Key field should not be required.

The UI should not imply that TOTP is optional.

---

# 9. Trusted Device Flow

After successful first authentication:

```text id="ly0w5c"
Trust this device?

[ ] Trust this browser on this device

This lets you connect without entering your
Remote Access Key again.

Your password and authenticator code are
still required for every session.
```

Important:

> Trusted-device status must never bypass TOTP.

The UI should explicitly communicate this.

---

# 10. Trusted Device Credential

If the user enables trust:

```text id="1h7qz4"
host
  ↓
issues cryptographic trusted-device credential
  ↓
browser stores credential securely
```

Do not store it in ordinary application data or localStorage if a safer browser mechanism is available.

The credential must be:

- cryptographically random
- scoped to the host
- revocable
- associated with a client/device identifier
- independent of the Linux password
- independent of the TOTP secret

---

# 11. New Browser Detection

The host determines whether a browser/device is trusted.

Do not allow the browser to declare:

```text id="n0cz1y"
"I am trusted"
```

The host must validate the credential.

If the credential is absent or invalid:

```text id="akb3py"
full new-device authentication
```

---

# 12. Remote Access Key

The Remote Access Key is a high-entropy credential for new/untrusted clients.

The UI should describe it as:

> Remote Access Key

Avoid exposing internal terminology such as:

> API key

The key should normally be:

- generated automatically
- long
- cryptographically random
- copyable
- revocable
- rotatable

Example display:

```text id="4v1j7m"
Remote Access Key

••••••••••••••••••••••••••••••••••••

[ Copy ]
[ Rotate ]

Keep this key in your password manager.
```

Never reveal the key unnecessarily.

---

# 13. Access-Key Rotation

When rotating:

```text id="p0x7ma"
Rotate Remote Access Key?

This may prevent new untrusted devices
from connecting with the previous key.

Existing sessions:
[Terminate existing sessions]
```

The exact semantics must be implemented consistently with the security specification.

The user should receive explicit confirmation of what rotation affects.

---

# 14. TOTP UX

TOTP uses standard authenticator applications.

Setup should occur through the host administration/setup flow.

The browser should never need access to the TOTP secret during normal login.

Normal login:

```text id="j1cy39"
Authenticator code
[ 123456 ]
```

Do not transmit or expose the TOTP secret.

---

# 15. TOTP Failure

Example:

```text id="17r3cr"
Incorrect authenticator code.

Please enter the current 6-digit code.
```

Do not reveal whether:

- username was correct;
- password was correct;
- Remote Access Key was correct.

Avoid detailed authentication-error enumeration.

---

# 16. Authentication Error UX

Prefer generic errors:

```text id="d8ib5x"
Authentication failed.

Check your credentials and try again.
```

Detailed reasons should be available only in local diagnostics/admin logs.

Do not expose sensitive authentication state to an unauthenticated browser.

---

# 17. Rate Limiting

The client should gracefully handle host-side rate limiting.

Example:

```text id="y6ps1b"
Too many authentication attempts.

Please wait before trying again.
```

The server remains responsible for enforcement.

Client-side timers are only UX.

---

# 18. Pre-Remote Session Screen

After authentication:

```text id="6i1fdd"
Preparing workstation...

Checking GNOME session
✓

Preparing virtual display
✓

Isolating physical display
✓

Isolating physical input
✓

Establishing remote connection
...
```

Do not display success until the host confirms the required safety conditions.

---

# 19. Remote Session Activation

The client must not consider the session active merely because WebRTC connected.

The authoritative sequence is:

```text id="0r7s9g"
AUTHENTICATED
      ↓
AUTHORIZED
      ↓
PREPARING_REMOTE
      ↓
DISPLAY_READY
      ↓
INPUT_READY
      ↓
REMOTE_ACTIVE
```

Only after `REMOTE_ACTIVE` is confirmed should the UI enter the normal remote desktop view.

---

# 20. Remote Desktop View

The normal UI should be intentionally minimal.

Suggested layout:

```text id="y2tx6s"
+------------------------------------------------+
| ● Connected    workstation       ⋮              |
+------------------------------------------------+
|                                                |
|                                                |
|             REMOTE GNOME DESKTOP               |
|                                                |
|                                                |
+------------------------------------------------+
| Connection: Excellent     [Disconnect]         |
+------------------------------------------------+
```

Avoid unnecessary toolbars covering the remote desktop.

---

# 21. Connection Status

Show a compact status indicator.

Suggested states:

```text id="q5w8mx"
● Connected
● Reconnecting
● Degraded
● Disconnecting
```

Do not expose implementation details such as:

```text ICE candidate state
SCTP state
PipeWire node ID
```

unless diagnostics are enabled.

---

# 22. Remote Control Lease

The browser should not directly manage lease validity.

The host controls it.

The browser should periodically communicate normally through the authenticated session.

If the host reports that the control lease has expired:

```text id="e8n4so"
Remote control ended

For your security, the workstation has been
locked and local control restored.

[ Reconnect ]
```

---

# 23. Network Interruption

Temporary interruption:

```text id="w4t9y3"
Connection interrupted

Trying to reconnect...
```

If the lease remains valid:

```text id="a8r1k2"
Connection restored
```

If the lease expires:

```text id="4sqw5z"
Remote session ended

The workstation was locked automatically.
```

Do not promise successful reconnect.

---

# 24. Reconnect Behavior

Reconnect should only happen when:

```text id="8dj0i5"
session still valid
AND
security epoch unchanged
AND
authorization remains valid
AND
host permits reconnection
```

Otherwise require a new authentication flow.

The browser must never bypass host authorization because it previously had a connection.

---

# 25. Browser Refresh

Refreshing the page must not automatically restore control.

Recommended:

```text id="xq9f4u"
browser refresh
     ↓
connection state lost
     ↓
host lease eventually expires
     ↓
remote control revoked
```

If seamless reconnect is later implemented, it must still use the host's session/lease validation.

---

# 26. Browser Tab Close

Closing the tab should result in eventual session termination.

However, the host must not depend on a clean browser event.

The control lease timeout remains authoritative.

---

# 27. Multiple Tabs

Initial implementation:

> One active remote-control browser tab per host.

If another tab attempts to control the same host:

```text id="y1v6kc"
Another remote session is already active.

[ Return ]
```

Do not silently steal control.

---

# 28. Multiple Devices

Initial implementation:

```text id="g1t9na"
one active remote controller
many registered/trusted devices
```

Trusted-device registration and active control are separate concepts.

Example:

```text id="v2d8tq"
Laptop       Trusted
Tablet       Trusted
Old Laptop   Revoked

Active controller:
Laptop
```

---

# 29. Explicit Disconnect

The remote UI should always provide an obvious disconnect action.

Recommended:

```text id="s9c3w7"
[ Disconnect ]
```

Confirmation may be appropriate:

```text id="k3d9vq"
Disconnect from workstation?

The workstation will be locked and
physical display/input restored.

[ Cancel ] [ Disconnect ]
```

---

# 30. Disconnect Completion

The browser should wait for host acknowledgement where possible.

Success:

```text id="9g2k3v"
Remote session ended.

Workstation locked.
Physical console restored.

[ Reconnect ]
```

If acknowledgement is unavailable:

```text id="v4n6cx"
Disconnect requested.

The workstation will automatically revoke
remote control if the connection is lost.
```

---

# 31. Emergency Takeover

Emergency takeover happens physically on the workstation.

The remote browser should detect termination.

Expected UI:

```text id="8q5p1n"
Remote session terminated

The workstation was taken over locally
and remote access was revoked.

You must authenticate again to reconnect.

[ Connect again ]
```

Do not expose sensitive details about the emergency trigger.

---

# 32. Emergency Security Epoch

If the browser attempts to reconnect using a stale session:

```text id="r4v2s8"
This remote session is no longer valid.

Please authenticate again.
```

The browser must discard stale session credentials where appropriate.

---

# 33. Revoked Trusted Device

If a previously trusted browser has been revoked:

```text id="m7c3x0"
This device is no longer trusted.

Authenticate as a new device to continue.
```

The client should fall back to:

```text id="7q2d9k"
username
password
TOTP
Remote Access Key
```

---

# 34. Host Remote Access Disabled

If remote access has been disabled:

```text id="n8v1r5"
Remote access is currently disabled on this
workstation.
```

Do not repeatedly reconnect.

---

# 35. Host Offline

If the host cannot be reached:

```text id="c4w7p2"
Workstation unavailable.

Check that the workstation is powered on
and connected to the network.
```

Do not reveal unnecessary infrastructure details.

---

# 36. Host Identity Warning

If a cryptographic host identity changes unexpectedly:

```text id="h5r9q1"
The identity of this workstation has changed.

For your security, the connection was stopped.

Verify the workstation before continuing.
```

Do not automatically trust the new identity.

The exact UX should support explicit user verification.

---

# 37. HTTPS Requirement

Production browser access must use HTTPS.

Do not allow production authentication over plain HTTP.

Development mode may support localhost/insecure development where explicitly configured, but the UI should make this distinction clear.

---

# 38. Browser Credential Storage

Do not store sensitive credentials in:

```text id="7y0bq2"
localStorage
URL parameters
query strings
console logs
analytics events
DOM attributes
plain IndexedDB
```

Use appropriate browser security mechanisms for trusted-device credentials.

The implementation must document the chosen mechanism and its security limitations.

---

# 39. No Secrets in URLs

Never put:

```text id="4n8j5p"
password
TOTP
Remote Access Key
session bearer token
trusted-device credential
```

in:

```text
URL
query parameter
fragment
referrer
```

---

# 40. WebRTC Security

WebRTC media/data transport should be encrypted.

The browser client should not implement custom cryptography around WebRTC unless a specific threat model requires it.

The gateway should not have access to plaintext remote-session media merely because it handles signalling.

---

# 41. Gateway Trust Boundary

The browser should conceptually communicate with:

```text id="r6s3u9"
Browser
   |
HTTPS/WebSocket
   |
Gateway
   |
authenticated host connection
   |
Host
```

The gateway should not become the authority for:

- GNOME state
- session authorization
- physical input
- display control
- security epoch

The host remains authoritative.

---

# 42. Remote Session Indicators

The remote UI should provide enough information for the user to understand:

```text id="e6t2w1"
which host
which account
whether connected
connection quality
whether reconnecting
how to disconnect
```

Optional:

```text
resolution
latency
frame rate
network path
```

should live under diagnostics rather than clutter the main interface.

---

# 43. Keyboard Input

The client must provide normal GNOME keyboard behavior.

Special browser-reserved shortcuts can interfere with remote input.

The implementation should identify unavoidable browser limitations.

Provide a mechanism such as:

```text id="f3w7z2"
Capture keyboard
```

when necessary.

The client must not claim perfect keyboard forwarding if the browser prevents it.

---

# 44. Pointer Input

Pointer input should support:

- relative movement where required;
- absolute positioning where appropriate;
- button events;
- wheel events;
- modifier keys.

Cursor synchronization must be tested against GNOME/Mutter behavior.

---

# 45. Touch / Tablet Input

Not required for the initial feasibility gate.

Treat as future capability.

Do not complicate the state machine until keyboard/pointer input is proven reliable.

---

# 46. Clipboard

Clipboard support should be independently configurable.

Potential UI:

```text id="k7p2v9"
Clipboard sharing
[ On / Off ]
```

It must not be implicitly assumed to be safe merely because remote input is enabled.

Clipboard contents can contain secrets, credentials, or sensitive documents.

---

# 47. Session Timeout

If the host session expires:

```text id="a6v3w8"
Remote session expired.

The workstation has been locked.
```

The browser must discard unusable session state.

---

# 48. Authentication Recovery

If the user loses access to their authenticator:

```text id="m8q4t2"
Use a recovery code
```

The recovery path remains:

```text id="6d3v1k"
username
password
recovery code
Remote Access Key
```

for a new/untrusted device.

Do not make recovery codes a silent TOTP bypass that eliminates the Remote Access Key requirement.

---

# 49. Trusted Device Management UI

A local management interface should expose:

```text id="w8r3p5"
Trusted devices

Laptop
Last used: ...
Status: Trusted
[ Revoke ]

Tablet
Last used: ...
Status: Trusted
[ Revoke ]

[ Revoke All Devices ]
```

Never display trusted-device secrets.

---

# 50. Active Session Management

The host management UI should expose:

```text id="s5c8q0"
Active remote session

Device: Laptop
User: user
Connected: ...
Duration: ...
Status: Active

[ Terminate Session ]
```

The user should be able to terminate the session independently of the browser.

---

# 51. Revoke All

"Revoke All" should have explicit semantics.

Recommended:

```text id="e3n7w6"
Revoke all remote access?

This will:
- terminate active remote sessions;
- revoke trusted devices;
- invalidate remote session credentials;
- prevent previous trusted clients from reconnecting.

[ Cancel ] [ Revoke All ]
```

Whether the Remote Access Key itself is rotated should be explicit.

---

# 52. Client State Model

The browser should maintain a small client-side state machine.

Suggested:

```text id="t6r2p8"
DISCONNECTED
CONNECTING
AUTHENTICATING
AUTHENTICATED
STARTING_SESSION
REMOTE_ACTIVE
RECONNECTING
DISCONNECTING
SESSION_ENDED
ERROR
```

Do not duplicate the host's authoritative state machine.

The browser state represents:

> what the client currently knows about the host session.

---

# 53. Host vs Browser State

Example:

```text id="a9p4c2"
Browser:
REMOTE_ACTIVE

Host:
TEARING_DOWN
```

The host wins.

The browser must transition to:

```text id="u3k7m1"
SESSION_ENDED
```

upon receiving authoritative termination information or detecting lease/session loss.

---

# 54. Browser Event Handling

Client events include:

```text id="v8q2s5"
CONNECT
AUTH_SUBMIT
AUTH_SUCCESS
AUTH_FAILURE
SESSION_READY
MEDIA_READY
CONTROL_READY
DISCONNECT
NETWORK_LOSS
NETWORK_RESTORED
SESSION_EXPIRED
HOST_TERMINATED
EMERGENCY_TERMINATED
HOST_UNAVAILABLE
```

These should be mapped explicitly to client states.

Avoid scattered callbacks that independently manipulate UI state.

---

# 55. Error Categories

Use stable machine-readable error categories internally.

Examples:

```text id="m3x8q1"
AUTH_FAILED
AUTH_RATE_LIMITED
ACCESS_DENIED
HOST_OFFLINE
HOST_UNTRUSTED
SESSION_EXPIRED
SESSION_REVOKED
LEASE_EXPIRED
HOST_STATE_INVALID
GNOME_UNAVAILABLE
DISPLAY_SETUP_FAILED
INPUT_SETUP_FAILED
MEDIA_FAILED
NETWORK_FAILED
EMERGENCY_TERMINATION
```

Human-facing messages should remain concise.

---

# 56. Diagnostics Mode

A diagnostics panel may expose:

```text id="q8w5n2"
Host
Session
Authentication
Control lease
Security epoch
WebRTC
Latency
Packet loss
Video codec
Resolution
PipeWire
GNOME capability
```

Never expose secrets.

Diagnostics should be safe to copy into bug reports.

---

# 57. Accessibility

The browser UI should support:

- keyboard navigation;
- visible focus;
- readable status messages;
- screen-reader-friendly labels;
- sufficient contrast;
- no color-only security indicators.

Connection state must not be communicated solely by color.

---

# 58. Responsive Design

The remote desktop viewport should adapt to:

- laptop
- desktop
- tablet

For initial implementation:

> prioritize desktop/laptop usability.

Tablet support should not complicate the core transport or security model.

---

# 59. Performance UX

The client should expose quality information only when useful.

Potential:

```text id="x7m3v9"
Excellent
Good
Poor
Reconnecting
```

Adaptive resolution/quality should be handled by the remote media system.

Do not implement a complicated client-side quality controller before the basic WebRTC pipeline is reliable.

---

# 60. Session Start UX

The preferred experience is:

```text id="d9w4p3"
1. Open remote client
2. Select host
3. Authenticate
4. Host prepares workstation
5. Browser receives remote display
6. Remote input becomes available
7. Remote session active
```

The user should not need to understand:

- Mutter
- PipeWire
- libei
- systemd
- virtual monitors
- security epochs

Those belong in diagnostics/documentation.

---

# 61. Session End UX

Normal:

```text id="z5q8w1"
Disconnect
   ↓
host tears down remote state
   ↓
GNOME locks
   ↓
physical console restored
   ↓
browser shows Session Ended
```

Abnormal:

```text id="r7m2k4"
connection loss
   ↓
lease expires
   ↓
host tears down
   ↓
GNOME locks
   ↓
browser eventually shows Session Ended
```

---

# 62. Security UX Principles

The browser should:

- never imply authentication equals control;
- never imply trusted means passwordless;
- never bypass TOTP;
- never automatically trust a new host identity;
- never expose secrets;
- never silently reconnect after security-sensitive termination;
- clearly indicate when a session has ended;
- prefer generic authentication errors;
- avoid creating a false sense of security.

---

# 63. Client Acceptance Criteria

The browser client is acceptable only if:

### A. New device works remotely

A completely new browser can authenticate while the user is physically away using:

```text
username
password
TOTP
Remote Access Key
```

### B. Trusted device works

A trusted browser can connect with:

```text
username
password
TOTP
trusted-device credential
```

### C. TOTP cannot be bypassed

Trusted status does not eliminate TOTP.

### D. Stale sessions cannot reconnect

After emergency/revocation/security-epoch change, stale browser state cannot regain control.

### E. Disconnect is clear

The user receives clear confirmation that the workstation was returned to a locked state.

### F. Network loss is safe

The client cannot retain remote control after the host lease expires.

### G. Browser refresh is safe

Refreshing the page cannot silently retain remote control.

### H. Host remains authoritative

Client UI cannot override host state.

### I. Secrets are protected

No authentication/session secret appears in URLs, logs, analytics, or ordinary browser storage.

### J. Authentication errors do not leak information

Unauthenticated clients cannot enumerate valid users/devices/credentials.

---

# 64. Implementation Instructions for GitHub Copilot Agent

When implementing this document:

1. Inspect the existing repository first.
2. Inspect the workflow/configuration created by `adaptive-workflow-configurator`.
3. Follow that workflow rather than creating a competing project structure.
4. Reuse established frontend/backend conventions where present.
5. Keep browser state separate from the authoritative host state machine.
6. Keep authentication UI separate from remote-session rendering.
7. Do not embed secrets into URLs.
8. Do not store sensitive credentials in localStorage.
9. Do not implement custom cryptography unnecessarily.
10. Use WebRTC for the remote media/data plane.
11. Treat the host as the security authority.
12. Implement explicit client states and transitions.
13. Add automated tests for authentication and client state transitions.
14. Add tests for disconnect, reconnect, revocation, emergency termination, and lease expiration.
15. Do not implement advanced features such as clipboard, touch, multi-controller support, or native clients before the core workflow is proven.

---

# 65. Implementation Order

Recommended sequence:

```text id="b6q2t9"
1. Basic host connection screen
2. Host identity handling
3. Authentication UI
4. TOTP flow
5. Remote Access Key flow
6. Trusted-device flow
7. Client state machine
8. Session-start progress UI
9. WebRTC connection
10. Remote viewport
11. Keyboard input
12. Pointer input
13. Connection health
14. Disconnect
15. Reconnect
16. Session-expiration handling
17. Emergency termination handling
18. Trusted-device management
19. Diagnostics
20. Accessibility/polish
```

---

# 66. Final UX Principle

The browser should make the system feel simple:

```text id="t8n4q6"
AUTHENTICATE
      ↓
CONNECT
      ↓
USE SAME WORKSTATION
      ↓
DISCONNECT
      ↓
WORKSTATION LOCKED
```

The complexity belongs inside the host architecture.

The browser must never compromise the host's fail-safe behavior merely to provide a smoother user experience.