# Networking & Browser Client Implementation Specification

## 1. Purpose

This document defines the networking architecture and browser-client requirements for the Remote Console project.

It covers:

- browser-based remote access
- HTTPS
- WebSocket signalling
- WebRTC
- LAN/direct connections
- NAT traversal
- STUN
- TURN
- rendezvous/discovery
- dynamic IP addresses
- IPv4/IPv6
- mDNS/Avahi
- host identity
- browser authentication
- session establishment
- reconnect behavior
- network failure
- relay architecture
- security boundaries
- deployment modes

The design must remain compatible with the security model defined in:

```text
03_SECURITY_AND_AUTHENTICATION.md
```

The networking layer must never weaken the authentication or fail-safe requirements.

---

# 2. Core Networking Principle

The project should use established networking protocols rather than inventing a custom remote-desktop transport.

Preferred architecture:

```text
Browser
   |
   | HTTPS
   | WebSocket signalling
   |
   v
Gateway / Rendezvous
   |
   | WebRTC negotiation
   |
   v
Host
```

After connection establishment:

```text
Browser <=====================> Host
              WebRTC
```

The goal is:

> Use the gateway to establish the connection, but move the actual interactive desktop traffic to a direct peer-to-peer WebRTC connection whenever possible.

---

# 3. Why WebRTC

Use WebRTC for the remote desktop data plane where practical.

Benefits include:

- encrypted transport
- NAT traversal
- congestion control
- packet loss handling
- connection establishment
- data channels
- mature browser support
- support for direct and relayed connectivity

Do not build a custom:

```text
TCP + custom encryption + custom congestion control
```

remote protocol.

---

# 4. High-Level Architecture

```text
                         INTERNET
                            |
              +-------------+-------------+
              |                           |
              v                           v
       Browser Client               Host Workstation
              |                           |
              | HTTPS                     | HTTPS
              | WebSocket                 | WebSocket
              |                           |
              +------------+--------------+
                           |
                           v
                  Remote Gateway
                           |
                           |
                    Signalling only
                           |
                           v
                    WebRTC negotiation
                           |
                +----------+----------+
                |                     |
             Direct                TURN
             path                  relay
                |                     |
                +----------+----------+
                           |
                           v
                       Host
```

The gateway should not become a permanent desktop-data bottleneck when direct WebRTC is possible.

---

# 5. Components

The networking system should conceptually contain:

```text
Browser Client
Remote Gateway
Rendezvous Service
STUN Service
TURN Service
Host Agent
```

These may initially be implemented together for simplicity, but their responsibilities should remain conceptually separated.

---

# 6. Browser Client

The browser client provides:

- authentication UI
- remote desktop UI
- keyboard input
- pointer input
- connection status
- security status
- device registration
- trusted-device management
- session disconnect
- recovery/error state

The browser must not have direct privileged access to the Linux workstation.

---

# 7. Browser Technology

Prefer standard web technologies.

Potential stack:

```text
TypeScript
+
React or lightweight equivalent
+
WebRTC APIs
+
WebSocket
+
HTTPS
```

The exact frontend framework is not mandated.

Do not introduce a heavyweight frontend framework merely because it is popular.

A lightweight architecture is acceptable.

---

# 8. Browser Compatibility

Initial target:

- current Chromium-based browsers
- current Firefox
- current Safari where WebRTC capabilities permit

Do not attempt to support obsolete browsers.

The application should perform capability detection rather than relying solely on browser user-agent strings.

---

# 9. Browser Capability Detection

At connection time, detect required capabilities.

Examples:

```text
WebRTC
WebSocket
WebCodecs where required
required video codec support
required input APIs
secure context
```

If a required capability is unavailable:

```text
Remote session cannot start.

Your browser does not support the required remote-control capabilities.
```

Do not attempt unsafe fallback behavior.

---

# 10. Secure Context

The production browser application must run in a secure context.

Use:

```text
HTTPS
```

not plain HTTP.

Development may use localhost HTTP where browser rules permit it.

---

# 11. WebSocket Signalling

Use WebSocket for interactive signalling.

Signalling responsibilities may include:

```text
authentication messages
session creation
SDP offer
SDP answer
ICE candidates
connection state
session lifecycle
reconnect coordination
```

Do not send desktop pixels through the signalling channel.

Do not send keyboard events through the signalling channel after the WebRTC data plane is established unless explicitly required for connection-management purposes.

---

# 12. Signalling Protocol

Define a versioned protocol.

Example:

```text
protocol_version: 1
message_type: "create_session"
request_id: "..."
payload: {...}
```

Every message should have:

- protocol version
- message type
- request identifier where appropriate
- structured payload

Do not use arbitrary JSON blobs whose schema changes implicitly.

---

# 13. Protocol Versioning

The protocol must be explicitly versioned.

Example:

```text
remote-console/v1
```

Future incompatible changes should create a new protocol version rather than silently breaking existing clients.

---

# 14. Message Validation

Treat every network message as untrusted.

Validate:

- message size
- message type
- protocol version
- required fields
- field types
- maximum lengths
- allowed values
- authorization state

Reject malformed messages.

Never pass network-provided values directly into:

- shell commands
- file paths
- systemd commands
- D-Bus method names
- executable names
- configuration files

---

# 15. Request IDs

Use request IDs for operations that require responses.

Example:

```text
request_id: 9f3a...
type: create_session
```

Response:

```text
request_id: 9f3a...
type: create_session_result
```

This makes asynchronous browser/host interactions easier to reason about.

---

# 16. Host Registration

A host should register with the rendezvous/gateway service.

Registration should use the host's cryptographic identity.

Conceptually:

```text
Host
 |
 | host_id
 | public key
 | capabilities
 | connectivity information
 v
Rendezvous
```

Do not use:

- MAC address
- hostname
- local IP

as the primary host identity.

---

# 17. Host ID

Generate a stable cryptographic host identity during installation.

Example:

```text
host_id
host_public_key
host_private_key
```

The host ID should remain stable across:

- DHCP changes
- IP changes
- hostname changes

unless the user explicitly resets the host identity.

---

# 18. Host Private Key

The host private key is highly sensitive.

It must:

- remain on the host
- never be sent to the browser
- never be sent to the rendezvous service
- never be logged
- be protected using appropriate filesystem/OS security

The host uses it to prove its identity.

---

# 19. Dynamic IP

The user must not have to know the host's current IP address.

The system should support:

```text
DHCP
dynamic public IP
NAT
IPv4
IPv6
```

The host reconnects to the rendezvous service when its network address changes.

---

# 20. Rendezvous

Rendezvous exists to answer:

> How can this browser find and negotiate a connection with this host?

It should not become the authoritative source of user authorization.

The host remains the source of truth for:

- configured user
- password authentication
- TOTP
- Remote Access Key
- trusted clients
- session authorization
- security epoch

---

# 21. Rendezvous Does Not Authenticate the User

Do not implement:

```text
Browser
→ rendezvous
→ "host says this user is valid"
```

as the security boundary.

The host must authenticate the remote user.

---

# 22. Host Online State

Rendezvous may maintain:

```text
host_id
online/offline
last_seen
connection endpoint metadata
capabilities
```

Do not expose unnecessary information publicly.

---

# 23. Host Privacy

Do not publicly expose:

- Linux username
- hostname unless explicitly intended
- internal IP addresses
- user session information
- display resolution
- active application names
- desktop metadata

unless required for the user's authenticated workflow.

---

# 24. LAN Direct Access

The project should support direct LAN connections.

Example:

```text
Laptop
   |
   | LAN
   |
   v
Ubuntu Host
```

The connection should ideally avoid the external rendezvous/relay path.

---

# 25. LAN Discovery

Consider mDNS/Avahi for local discovery.

Example:

```text
_remote-console._tcp
```

The exact service name should be defined during implementation.

LAN discovery is a convenience.

It must not be an authentication mechanism.

---

# 26. Direct LAN Connection

A discovered host should still require normal authentication:

```text
LAN discovery
     |
     v
host identified
     |
     v
username
password
TOTP
Access Key if required
```

Do not assume:

> Same Wi-Fi = trusted.

---

# 27. IPv6

Support IPv6 where available.

Do not assume IPv4-only networking.

WebRTC ICE should be allowed to discover usable IPv6 candidates.

---

# 28. NAT Traversal

Use ICE.

Preferred connection process:

```text
Host candidates
+
Browser candidates
+
STUN
+
ICE
```

Attempt to establish the most direct viable path.

---

# 29. STUN

STUN should be used for public-address discovery and NAT traversal.

STUN does not provide authentication for the remote desktop application.

Do not treat successful STUN connectivity as authorization.

---

# 30. TURN

Use TURN when direct connectivity fails.

Example:

```text
Browser
   |
   v
TURN
   |
   v
Host
```

TURN should carry encrypted WebRTC traffic.

The relay should not have access to:

- Linux password
- TOTP
- Access Key
- desktop plaintext
- keyboard plaintext

---

# 31. TURN Fallback

Preferred path:

```text
Direct LAN
   ↓
Direct public/IPv6
   ↓
NAT traversal
   ↓
TURN relay
```

The exact ICE behavior should be left to WebRTC where possible.

---

# 32. Self-Hosted Infrastructure

The project should support a self-hosted deployment.

Potential services:

```text
remote-gateway
rendezvous
STUN
TURN
```

The user should eventually be able to operate these on their own infrastructure.

Do not make the architecture permanently dependent on a proprietary cloud.

---

# 33. Initial Deployment Modes

Support at least conceptually:

### Local/LAN

```text
Browser
   |
   v
Host
```

### Internet with project gateway

```text
Browser
   |
   v
Gateway
   |
   v
Host
```

### Self-hosted

```text
Browser
   |
   v
User's Gateway
   |
   v
Host
```

---

# 34. Gateway Privilege

The gateway must not run as root.

It should not have:

- `/dev/input` access
- unrestricted D-Bus access
- access to the host's password database
- TOTP secret
- Remote Access Key
- host private key unless absolutely necessary

The gateway should communicate with `remote-hostd` through narrow authenticated IPC.

---

# 35. Gateway Architecture

Preferred:

```text
Internet
   |
   v
reverse proxy / TLS
   |
   v
remote-gateway
   |
   | authenticated IPC
   v
remote-hostd
```

The gateway is an untrusted network-facing component.

---

# 36. Gateway Compromise

Threat model:

> Assume the gateway can be compromised.

The attacker should not automatically gain:

- Linux credentials
- host root
- GNOME control
- remote input authority
- host private key

The gateway should be as close to a protocol router as practical.

---

# 37. Authentication Flow

New/untrusted browser:

```text
Browser
   |
   | connect
   v
Gateway
   |
   | establish signalling
   v
Host
   |
   | username/password/TOTP/Access Key
   v
Authentication
   |
   v
Session credential
   |
   v
WebRTC
```

The exact credential transmission must use the secure application protocol and must never expose credentials through URLs.

---

# 38. Trusted Device Flow

Trusted client:

```text
Browser
   |
   v
Gateway
   |
   v
Host
   |
   +-- username
   +-- password
   +-- TOTP
   +-- trusted client credential
   |
   v
Authenticated
```

Remote Access Key is not required.

TOTP remains mandatory.

---

# 39. Authentication Before Remote Control

Do not establish an authorized remote-control state before authentication.

Connection establishment may happen before authentication at the transport layer, but:

```text
REMOTE_CONTROL = false
```

until authentication and authorization succeed.

---

# 40. WebRTC Before Authentication

It is acceptable to establish a transport before authentication if required by the protocol.

However:

```text
WebRTC connected
≠
Remote session authorized
```

The browser must not receive desktop content or remote-control capability until authorization is complete.

---

# 41. Remote Desktop Media

The remote desktop video stream should be transported through WebRTC.

Potential architecture:

```text
Mutter
   |
   v
PipeWire
   |
   v
Video encoder
   |
   v
WebRTC
   |
   v
Browser
```

The exact codec/encoder should be selected based on GNOME/PipeWire/WebRTC capability and hardware.

---

# 42. Codec Selection

Do not prematurely hard-code one codec.

Evaluate:

- H.264
- VP8
- VP9
- AV1

based on:

- browser support
- hardware acceleration
- latency
- CPU usage
- GPU support
- GNOME/PipeWire integration

A capability-driven negotiation is preferable.

---

# 43. Hardware Acceleration

Prefer hardware encoding where safely available.

However:

> Hardware acceleration is an optimization, not a correctness requirement.

Software fallback may be acceptable if performance remains usable.

---

# 44. Remote Input

Remote input should travel through the authenticated WebRTC session.

Conceptually:

```text
Browser
   |
   | WebRTC data channel
   v
Host
   |
   v
authorized input path
   |
   v
GNOME / libei
```

Every input message must be authorized against the current control lease.

---

# 45. Input Message Design

Use structured messages.

Example:

```text
keyboard_down
keyboard_up
pointer_move
pointer_button
pointer_scroll
```

Validate:

- event type
- coordinates
- button values
- scroll range
- key identifiers

Do not accept arbitrary serialized native input structures from the browser.

---

# 46. Coordinate Validation

Pointer coordinates must be bounded by the current remote display dimensions.

Reject:

```text
NaN
Infinity
extremely large values
negative values where invalid
```

Do not trust browser-provided dimensions.

The host's actual virtual monitor configuration is authoritative.

---

# 47. Input Rate Limiting

Prevent pathological input flooding.

Apply reasonable limits to:

- pointer movement events
- scroll events
- keyboard events
- control messages

Do not introduce artificial latency that makes normal pointer movement unusable.

---

# 48. Clipboard

Clipboard is NOT part of the mandatory v1 remote-control protocol.

If implemented later:

- make it an explicit capability
- require authorization
- define directionality
- protect against huge payloads
- protect against clipboard injection
- avoid silently copying sensitive host data

Do not add it merely because remote-desktop products commonly have it.

---

# 49. File Transfer

File transfer is not part of the mandatory v1 scope.

Do not expose arbitrary filesystem access through the remote session.

This substantially reduces attack surface.

---

# 50. Remote Session Identifier

Every remote session should have a unique ID.

Example:

```text
session_id
```

It must be unpredictable enough not to act as a secret-bearing identifier.

Do not rely on session ID alone for authorization.

---

# 51. Control Lease

The networking layer must integrate with the control lease defined in the security specification.

Conceptually:

```text
WebRTC connected
      |
      v
Authenticated
      |
      v
Session created
      |
      v
Control lease
      |
      v
Remote input enabled
```

No valid lease:

```text
Remote input disabled
```

---

# 52. Connection Health

Monitor:

- WebRTC connection state
- ICE state
- signalling state
- application heartbeat
- control lease expiration

Do not rely on one signal alone.

---

# 53. Heartbeat

Use an application-level heartbeat.

Example:

```text
Browser → heartbeat
Host → heartbeat_ack
```

The heartbeat should confirm:

- session alive
- authentication still valid
- security epoch still valid
- control lease still valid

---

# 54. Connection Loss

On detected connection loss:

```text
WebRTC disconnected
        |
        v
Stop remote input
        |
        v
Lease expiration/revocation
        |
        v
Fail-safe state machine
```

Do not wait indefinitely for WebRTC to reconnect while continuing remote input.

---

# 55. Reconnection

Reconnection must be treated as a new authorization event.

Do not blindly restore old remote control after reconnect.

Preferred:

```text
connection lost
      |
      v
remote control revoked
      |
      v
GNOME locked
      |
      v
physical console restored
      |
      v
new connection
      |
      v
authentication
      |
      v
new remote session
```

If an optimized reconnect mechanism is later introduced, it must preserve equivalent security guarantees.

---

# 56. Browser Refresh

A browser refresh must not automatically produce indefinite control.

The system should either:

- resume a still-valid authenticated session securely, or
- require reauthentication.

The choice must be explicitly designed.

Do not rely on browser memory of credentials.

---

# 57. Browser Tab Close

Treat browser disappearance as a potential connection failure.

The host should eventually revoke the lease.

Do not assume:

```text browser tab closed
→ clean disconnect message always received
```

because crashes and network failures exist.

---

# 58. Multiple Browser Tabs

Prevent accidental simultaneous control from multiple tabs.

Possible behavior:

```text
Tab A = controller
Tab B = view-only / rejected
```

Only one control lease may exist.

---

# 59. Multiple Devices

Multiple authenticated devices may exist.

Example:

```text
Laptop A
Laptop B
Tablet
```

But v1 should enforce:

```text
one CONTROL lease
```

Other sessions may be:

```text VIEW
```

if view-only sessions are supported.

---

# 60. Host Offline

Browser should show:

```text
Host offline
```

Do not expose internal errors.

Possible statuses:

```text
OFFLINE
CONNECTING
AUTHENTICATING
AUTHENTICATED
STARTING_SESSION
REMOTE_ACTIVE
DISCONNECTING
RECOVERING
LOCKED
ERROR
```

---

# 61. Host Discovery Errors

Do not reveal whether a host exists to unauthenticated arbitrary users unless the deployment explicitly permits discovery.

Avoid creating a host enumeration mechanism.

---

# 62. Host Pairing

Physical pairing is not mandatory.

A completely new client can authenticate remotely using:

```text
username
password
TOTP
Remote Access Key
```

Pairing/trusting is optional after successful authentication.

---

# 63. Trusted Client Registration

After successful new-device authentication:

```text
Trust this device?

[No]
[Yes]
```

If yes:

```text
generate client key
register public key
```

Do not transmit a long-term private key through the server.

Prefer generating the client key locally in the browser/client environment where technically practical.

---

# 64. Browser Credential Protection

The trusted-client private credential is sensitive.

The implementation must research appropriate browser storage.

Avoid plaintext persistence.

Potential future approaches may include:

- WebCrypto
- non-exportable CryptoKey
- platform credential APIs
- WebAuthn/passkeys

Do not claim that ordinary browser localStorage is secure enough for a long-term private key.

---

# 65. WebAuthn / Passkeys

Passkeys are a potential future enhancement.

Do not make them mandatory for v1.

Possible future model:

```text
username
+
password
+
TOTP
+
passkey
```

or an alternative authentication policy.

Do not replace the current agreed security model without explicit design review.

---

# 66. HTTPS Certificate Handling

Production deployments must use valid TLS certificates.

Do not encourage users to disable certificate validation.

For self-hosted installations:

- support normal public certificates
- support internal CA where appropriate
- document certificate configuration

---

# 67. Host Certificate vs Host Identity

TLS identity and application host identity are separate concepts.

Do not assume:

```text
TLS certificate = host identity
```

The application should still validate its cryptographic host identity where appropriate.

---

# 68. Certificate Pinning

Do not hard-code certificate pins in the browser unless there is a compelling security reason.

Certificate rotation and self-hosted deployments make rigid pinning problematic.

Prefer normal TLS validation plus application-level host identity.

---

# 69. CSRF

Authentication/session APIs must implement appropriate CSRF protection.

Especially protect:

- session creation
- device registration
- device revocation
- Access Key rotation
- TOTP changes
- remote-access configuration
- emergency-related administrative APIs

---

# 70. CORS

Use a restrictive CORS policy.

Do not configure:

```text
Access-Control-Allow-Origin: *
```

for credentialed security-sensitive endpoints.

---

# 71. WebSocket Origin Security

Validate:

- Origin
- authenticated session
- protocol version
- host/session association

Do not accept arbitrary browser WebSocket connections merely because they can reach the port.

---

# 72. Connection Limits

Apply reasonable limits to:

- concurrent unauthenticated connections
- concurrent authentication attempts
- concurrent sessions
- signalling messages
- message sizes
- ICE candidate counts

This reduces resource exhaustion risk.

---

# 73. DoS Considerations

The gateway should protect against:

- connection floods
- oversized messages
- authentication brute force
- signalling abuse
- excessive session creation
- excessive WebRTC negotiation

The host itself must also remain protected if directly reachable.

---

# 74. LAN Exposure

If direct LAN access is enabled:

Do not assume LAN is trusted.

The host should still require full authentication.

Firewall recommendations may be provided during installation, but the application must not depend solely on firewall configuration.

---

# 75. Firewall

Provide clear documentation for required network access.

Potentially:

```text
HTTPS
WebSocket
WebRTC/ICE
TURN
```

Avoid opening broad unnecessary port ranges.

Where WebRTC requires dynamic UDP ports, document the actual requirements rather than inventing fixed assumptions.

---

# 76. IPv4 NAT

Do not require users to manually configure:

```text
port forwarding
static public IP
```

for the standard Internet workflow.

Use ICE/STUN/TURN.

Manual port forwarding can be an advanced/self-hosted option.

---

# 77. Offline LAN Mode

Consider a LAN-only mode where the system can operate without an external cloud dependency.

Example:

```text
Laptop
   |
   | local network
   v
Host
```

Authentication remains host-controlled.

This is particularly useful for privacy-sensitive deployments.

---

# 78. No Cloud Dependency in Core Security

The host must remain functional without the vendor's cloud authentication service.

The core security model must be host-local.

Do not implement:

```text
vendor cloud says user is authorized
```

as the sole authentication path.

---

# 79. Gateway Outage

If the gateway is unavailable:

LAN/direct connectivity should remain possible where configured.

The host should not become permanently unusable because a cloud rendezvous service is unavailable.

---

# 80. TURN Outage

If TURN is unavailable:

- direct connectivity may still work
- LAN connectivity may still work

The browser should report:

```text
Direct connection unavailable.
Relay connection unavailable.
```

without exposing infrastructure secrets.

---

# 81. Connection Priority

Prefer:

```text
1. Direct LAN
2. Direct IPv6/public
3. NAT-traversed direct connection
4. TURN relay
```

The exact priority should generally be left to ICE rather than manually forcing a brittle order.

---

# 82. Bandwidth Adaptation

The remote desktop should adapt to available network conditions.

Consider:

- resolution
- frame rate
- bitrate
- keyframe interval
- congestion

Do not allow the browser to demand an unlimited bitrate.

---

# 83. Latency

Remote control is interactive.

Optimize for:

```text
low latency
```

rather than maximum image quality.

Prefer a responsive pointer and keyboard experience over unnecessarily high resolution.

---

# 84. Resolution

The browser should learn the actual remote virtual monitor resolution from the host.

Do not assume the browser viewport is the monitor resolution.

---

# 85. Browser Scaling

Support device pixel ratio appropriately.

Examples:

```text
DPR 1
DPR 1.5
DPR 2
```

The remote desktop should remain usable on HiDPI clients.

---

# 86. Remote Monitor Changes

If the host changes the virtual monitor:

```text
host resolution changes
```

the browser must adapt.

Do not assume the display dimensions remain constant for the entire session.

---

# 87. Fullscreen

Fullscreen is a browser presentation feature.

It must not be confused with physical display privacy.

This project requires the host to isolate the physical display through GNOME/Mutter mechanisms.

A browser fullscreen black screen is not an acceptable substitute.

---

# 88. Physical Privacy

The networking layer must not enter `REMOTE_ACTIVE` merely because WebRTC is connected.

The host state machine must confirm:

```text
authentication valid
control lease valid
virtual display ready
physical display isolated
physical input isolated
remote stream ready
```

before declaring remote control active.

---

# 89. Network Layer and State Machine

The networking layer must report events to the host state machine.

Example:

```text
NETWORK_CONNECTED
NETWORK_AUTHENTICATED
WEBRTC_CONNECTED
MEDIA_READY
INPUT_READY
NETWORK_LOST
WEBRTC_FAILED
HEARTBEAT_TIMEOUT
```

The state machine decides what security action occurs.

The browser must not decide host security state.

---

# 90. Browser Cannot Unlock Host

The browser must never have a command such as:

```text
unlock_host()
```

unless explicitly redesigned and protected.

Normal disconnect behavior is:

```text
lock
```

not:

```text
unlock
```

---

# 91. Emergency Takeover

Emergency takeover is local.

It must not depend on:

- browser
- gateway
- WebRTC
- Internet
- TURN
- JavaScript

This is critical.

---

# 92. Emergency After Network Loss

If the network disappears:

```text
network loss
    |
    v
lease timeout
    |
    v
remote input disabled
    |
    v
lock
    |
    v
restore physical console
```

The local emergency shortcut remains an independent additional safety mechanism.

---

# 93. Session Reconnect Security

After failure:

```text
old session ≠ new session
```

A new session must obtain:

- new session ID
- valid authentication
- valid security epoch
- new control lease

unless a carefully designed secure reconnect mechanism explicitly proves equivalent authority.

---

# 94. Security Epoch and Network

The network session must include the security epoch.

Example:

```text
session:
    epoch = 21
```

Host:

```text
current_epoch = 22
```

Result:

```text
REJECT
```

This provides a strong defence against stale browser sessions.

---

# 95. Logout

Browser logout should:

1. destroy browser session state
2. request remote disconnect where possible
3. revoke control lease

Host-side cleanup must not depend on logout being received.

Lease expiration remains the safety net.

---

# 96. Network Errors

Do not expose internal stack traces to users.

User-facing:

```text
Unable to connect to host.
```

Developer diagnostics may contain:

```text
ICE failure
TURN unavailable
WebSocket closed
authentication rejected
```

without secrets.

---

# 97. Diagnostics

Provide a diagnostics page capable of showing:

```text
Browser WebRTC: ✓
WebSocket: ✓
Host reachable: ✓
Authentication: ✓
ICE: connected
Connection type: direct / relay
Video: connected
Input: connected
Remote lease: active
```

Do not display:

- credentials
- tokens
- TOTP secrets
- Access Key

---

# 98. Connection Type Display

It is useful to show:

```text
Connection:
Direct
```

or:

```text
Connection:
Relay
```

This helps diagnose performance.

Do not expose unnecessary IP addresses by default.

---

# 99. Performance Diagnostics

Allow collection of:

- RTT
- packet loss
- bitrate
- frame rate
- codec
- connection type
- CPU usage
- encoder mode

Do not collect desktop content as part of diagnostics.

---

# 100. Telemetry

The project should default to privacy-preserving behavior.

Do not send desktop telemetry to a central service without explicit user choice.

If anonymous telemetry is ever introduced:

- make it opt-in or clearly controlled
- document exactly what is collected
- never collect screen content
- never collect keystrokes
- never collect credentials
- never collect clipboard content

---

# 101. Self-Hosted Gateway Configuration

Eventually configuration may resemble:

```yaml
gateway:
  url: "https://remote.example.com"

turn:
  enabled: true
  urls:
    - "turn:remote.example.com"

lan_discovery:
  enabled: true
```

Do not hard-code these values.

Do not place secrets in source code.

---

# 102. TURN Credentials

TURN credentials must be temporary where practical.

Do not embed a permanent TURN password into browser JavaScript.

Prefer short-lived TURN credentials generated through an authenticated mechanism.

---

# 103. Gateway Authentication to Host

If gateway-to-host authentication is required, use cryptographic host identity or another narrowly scoped credential.

Do not use:

```text
Linux password
```

for gateway authentication.

---

# 104. Gateway IPC

Preferred:

```text
remote-gateway
      |
      | local authenticated IPC
      v
remote-hostd
```

The IPC protocol should be:

- authenticated
- authorized
- schema validated
- narrow
- versioned

---

# 105. No Generic Proxy

Avoid an architecture where the gateway can issue arbitrary host commands.

Bad:

```text
gateway → RPC(method, arbitrary arguments)
```

Better:

```text
gateway → CreateSession
gateway → Authenticate
gateway → SignalWebRTC
gateway → TerminateSession
```

---

# 106. Network Authentication State

The host should maintain explicit states:

```text
UNAUTHENTICATED
AUTHENTICATING
AUTHENTICATED
SESSION_CREATING
REMOTE_ACTIVE
REVOKING
DISCONNECTED
```

Integrate these with the global remote-console state machine.

---

# 107. Connection Timeout

Define timeouts for:

- initial WebSocket connection
- authentication
- session creation
- WebRTC negotiation
- ICE gathering
- media startup
- control lease
- heartbeat

Do not leave operations hanging indefinitely.

---

# 108. Authentication Timeout

If authentication is not completed within a reasonable period:

```text
terminate authentication session
```

This reduces resource exhaustion.

---

# 109. Session Startup Timeout

If the host cannot establish the complete remote state within a defined period:

```text
abort
+
cleanup
+
lock/restore as appropriate
```

Do not remain indefinitely in a half-configured state.

---

# 110. Remote Session Startup

Recommended sequence:

```text
1. Browser connects
2. TLS established
3. Signalling established
4. Host identified
5. User authenticated
6. Authorization verified
7. Remote session created
8. Control lease created
9. GNOME session prepared
10. Virtual monitor created
11. Physical display isolated
12. Physical input isolated
13. PipeWire capture established
14. WebRTC media established
15. Remote input established
16. Host enters REMOTE_ACTIVE
```

The exact ordering may change based on the GNOME PoC.

---

# 111. Do Not Declare Success Early

This is important.

```text
WebRTC connected
```

does not mean:

```text
Remote session ready
```

Likewise:

```text
video stream visible
```

does not mean:

```text
remote input authorized
```

The state machine remains authoritative.

---

# 112. Clean Disconnect

Preferred sequence:

```text
Browser disconnect
      |
      v
Host revokes lease
      |
      v
Remote input disabled
      |
      v
Remote media stopped
      |
      v
Virtual monitor destroyed
      |
      v
Physical display restored
      |
      v
Physical input restored
      |
      v
GNOME locked
```

The exact ordering must follow the GNOME feasibility PoC results.

---

# 113. Abrupt Disconnect

For:

- browser crash
- laptop shutdown
- Wi-Fi loss
- VPN failure
- router failure
- host network failure

the host must eventually reach the same safe state.

---

# 114. Network Reconnect

A network reconnect should not automatically restore remote control.

After fail-safe recovery:

```text
Host:
LOCKED
REMOTE_ACCESS_READY
```

The client must authenticate again.

---

# 115. Security Invariant

At all times:

```text
NO_VALID_LEASE
        =>
NO_REMOTE_INPUT
```

And:

```text
NETWORK_UNCERTAIN
        =>
REMOTE_INPUT_DISABLED
```

---

# 116. Host Security Invariant

The browser cannot force:

```text
REMOTE_ACTIVE
```

The host enters `REMOTE_ACTIVE` only after its own local checks pass.

---

# 117. Network Security Invariant

The gateway cannot force:

```text
REMOTE_ACTIVE
```

The gateway can only facilitate connection establishment.

---

# 118. Browser Security Invariant

The browser cannot:

- execute host commands
- access arbitrary host files
- bypass authentication
- bypass TOTP
- bypass the Access Key requirement
- bypass the control lease
- bypass security epoch
- unlock the host

---

# 119. Protocol Security Invariant

Every security-sensitive request must be validated against:

```text
authenticated session
+
authorized client
+
current security epoch
+
required capability
```

---

# 120. Testing Matrix

Test at minimum:

## LAN

```text
same subnet
IPv4
IPv6
mDNS
```

## Internet

```text
public IPv4
public IPv6
NAT
CGNAT where possible
TURN fallback
```

## Browser

```text
Chromium
Firefox
Safari where feasible
```

## Network failures

```text
Wi-Fi disconnect
Ethernet disconnect
VPN disconnect
router restart
host network restart
gateway restart
TURN unavailable
```

## Client failures

```text
browser tab close
browser crash
laptop sleep
laptop shutdown
```

---

# 121. Security Testing

Test:

```text
wrong password
wrong TOTP
wrong Access Key
expired session
expired lease
revoked client
revoked session
old security epoch
malformed WebSocket message
oversized message
unauthorized input
cross-origin WebSocket attempt
CSRF attempt
connection flood
```

---

# 122. WebRTC Testing

Test:

```text
direct connection
TURN relay
high latency
packet loss
bandwidth reduction
network switching
IPv4 → IPv6
Wi-Fi → Ethernet
```

Verify remote input remains safe during every transition.

---

# 123. Network Switching

Example:

```text
Laptop Wi-Fi
    |
    | switch network
    v
Different network
```

The existing WebRTC connection may fail.

Expected:

```text
remote input revoked
lease expires
host locks/restores
```

The client may subsequently create a new authenticated session.

---

# 124. Host IP Change

Example:

```text
Host:
192.168.1.20
   ↓
192.168.1.55
```

Expected:

- host identity remains unchanged
- rendezvous updates connectivity
- trusted client remains associated with the host
- no re-pairing solely because IP changed

---

# 125. Host Reboot

After reboot:

```text
host identity remains
security configuration remains
TOTP remains
Access Key remains
trusted clients remain
```

But:

```text
previous remote session = invalid
previous control lease = invalid
```

The user must establish a new session.

---

# 126. Host Suspend

If remote access is active, system suspend should normally be inhibited according to the main product specification.

If suspend nevertheless occurs:

```text
remote control revoked
safe recovery
```

Do not assume network connectivity survives suspend.

---

# 127. Gateway Restart

A gateway restart must not leave the host in an unsafe remote-control state.

If signalling disappears:

```text
lease eventually expires
remote input disabled
fail-safe recovery
```

---

# 128. TURN Restart

TURN failure should not bypass security.

The WebRTC connection may fail.

Expected:

```text
remote input disabled
lease expires
fail-safe recovery
```

---

# 129. Relay Privacy

The architecture should ensure that a TURN relay is treated as an untrusted transport intermediary.

It should not receive application credentials.

It should not be trusted with host authorization.

---

# 130. Logging

Network logs may include:

```text
connection ID
session ID
host ID
client ID
connection type
ICE state
WebRTC state
failure reason
timestamps
```

Never log:

```text
password
TOTP
Access Key
session bearer token
private keys
keystrokes
clipboard
desktop contents
```

---

# 131. Debug Mode

A development diagnostic mode may provide additional protocol information.

However:

> Debug mode must never automatically disable authentication, TLS, authorization, lease validation, or physical safety mechanisms.

Avoid dangerous:

```text
--disable-auth
```

shortcuts in production binaries.

---

# 132. Development vs Production

Development may use:

```text
localhost
self-signed TLS
local TURN
mock gateway
```

Production must use:

```text
valid TLS
real authentication
secure credentials
rate limiting
lease enforcement
```

---

# 133. Mocking

For unit tests, mock:

- WebRTC
- gateway
- TURN
- browser transport
- network failures

But integration tests must eventually use real WebRTC where possible.

---

# 134. End-to-End Test

Create an automated or semi-automated scenario:

```text
Ubuntu host
     |
     v
remote-hostd
     |
     v
gateway
     |
     v
browser
```

Verify:

```text
authentication
TOTP
Access Key
session creation
WebRTC
video
input
disconnect
fail-safe
reconnect
```

---

# 135. Browser UI Acceptance Test

A new client should see:

```text
Connect to Host

Username
Password
Authenticator Code
Remote Access Key

[Connect]
```

After authentication:

```text
Trust this device?

[No] [Yes]
```

A trusted client should see:

```text
Username
Password
Authenticator Code

[Connect]
```

with the trusted credential handled securely by the client.

---

# 136. Connection Status UI

Display clear states:

```text
Connecting…
Authenticating…
Starting remote session…
Connected
Connection degraded
Reconnecting…
Remote session ended
Host locked
```

Avoid exposing internal implementation details to normal users.

---

# 137. Failure UI

If connection fails:

```text
Unable to establish remote session.

The host was returned to its safe local state.
```

This is preferable to:

```text
ICE candidate 7 failed with STUN error 487
```

for ordinary users.

Detailed diagnostics may be available separately.

---

# 138. User-Controlled Disconnect

Provide an obvious:

```text
Disconnect
```

control.

Potential confirmation:

```text
Disconnect remote session?

The host will be locked and the physical console restored.

[Cancel] [Disconnect]
```

---

# 139. Emergency UI

The browser may display:

```text
If the remote session becomes unresponsive,
use the configured emergency shortcut on the host.
```

Do not make the browser emergency button the only recovery mechanism.

---

# 140. Network Architecture Summary

Final intended architecture:

```text
                    INTERNET / LAN
                           |
                           v
                    +-------------+
                    |   Browser   |
                    +------+------+
                           |
                    HTTPS/WebSocket
                           |
                           v
                  +-------------------+
                  | Remote Gateway    |
                  | Unprivileged      |
                  +---------+---------+
                            |
                       Signalling
                            |
                            v
                  +-------------------+
                  |   Host            |
                  |                   |
                  | remote-hostd      |
                  | GNOME agent       |
                  +---------+---------+
                            |
                         WebRTC
                            |
              +-------------+-------------+
              |                           |
          Direct path                 TURN relay
              |                           |
              +-------------+-------------+
                            |
                           Host
```

---

# 141. Implementation Boundaries

## Browser

Responsible for:

- UI
- WebRTC
- authentication interaction
- remote input generation
- session display

Not responsible for:

- host authorization
- host security state
- physical display
- physical input
- session locking

## Gateway

Responsible for:

- HTTPS
- WebSocket
- signalling
- rendezvous
- rate limiting
- connection coordination

Not responsible for:

- Linux authentication
- TOTP verification
- physical display
- physical input
- GNOME control

## Host daemon

Responsible for:

- authentication
- authorization
- security state
- control lease
- security epoch
- session orchestration

## GNOME agent

Responsible for:

- Mutter
- PipeWire
- libei
- virtual monitor
- display/input state

## Emergency daemon

Responsible for:

- independent emergency recovery

---

# 142. Implementation Order

Implement networking in this order.

### Phase 1 — Local WebSocket prototype

Prove:

```text
browser
↔ gateway
↔ host
```

with no remote desktop.

### Phase 2 — Authentication

Implement:

```text
username
password
TOTP
Access Key
trusted client
```

using the security specification.

### Phase 3 — WebRTC connectivity

Prove:

```text
browser ↔ host
```

using WebRTC.

### Phase 4 — Video

Connect:

```text
PipeWire → encoder → WebRTC → browser
```

### Phase 5 — Remote input

Connect:

```text
browser → WebRTC → host → libei/GNOME
```

### Phase 6 — Control lease

Enforce:

```text
valid lease
+
valid epoch
=
remote input allowed
```

### Phase 7 — Failure handling

Test:

```text
network loss
browser crash
gateway failure
TURN failure
host daemon failure
```

### Phase 8 — Production infrastructure

Add:

```text
rendezvous
STUN
TURN
LAN discovery
self-hosted deployment
```

---

# 143. Do Not Implement Yet

Until the GNOME feasibility PoC passes, do not spend substantial effort on:

- polished browser UI
- file transfer
- clipboard
- audio
- printing
- multi-user support
- mobile native clients
- cloud account system
- automatic updates
- complex device management

The first goal is proving the core architecture.

---

# 144. Final Networking Acceptance Criteria

Networking is considered successful only when all of the following work:

```text
[ ] Browser can find host
[ ] LAN direct connection works
[ ] Dynamic IP works through rendezvous
[ ] NAT traversal works
[ ] TURN fallback works
[ ] HTTPS works
[ ] WebSocket signalling works
[ ] WebRTC establishes
[ ] Video reaches browser
[ ] Remote input reaches GNOME
[ ] Authentication is enforced
[ ] TOTP is mandatory
[ ] Access Key required for new clients
[ ] Trusted client flow works
[ ] Control lease works
[ ] Security epoch works
[ ] Browser crash fails closed
[ ] Network loss fails closed
[ ] Gateway failure fails closed
[ ] Host identity survives IP changes
[ ] Host reboot invalidates old sessions
[ ] No secrets appear in logs
[ ] Gateway cannot directly control GNOME
[ ] TURN cannot decrypt desktop traffic
[ ] Browser cannot bypass host authorization
```

---

# 145. Final Copilot Agent Instruction

Implement this networking architecture conservatively.

Before coding:

1. inspect the repository
2. inspect the workflow configured by `adaptive-workflow-configurator`
3. inspect the existing GNOME feasibility PoC
4. research current WebRTC/browser support
5. research current PipeWire/WebRTC integration options
6. research STUN/TURN/ICE requirements
7. identify the minimum viable networking stack
8. document decisions before introducing major dependencies

Do not:

- invent a custom encrypted transport
- put credentials in URLs
- store permanent secrets in localStorage
- trust LAN connectivity as authentication
- make the gateway authoritative for user authentication
- run the gateway as root
- expose arbitrary host RPC
- allow browser commands to directly invoke system operations
- bypass TOTP for trusted clients
- automatically restore remote control after an ambiguous disconnect
- allow WebRTC connectivity to imply authorization
- enter `REMOTE_ACTIVE` before the host confirms all required safety conditions
- make cloud infrastructure mandatory for core local authentication
- add file transfer or arbitrary filesystem access without a separate security design

The central networking invariant is:

```text
NETWORK CONNECTED
        ≠
REMOTE CONTROL AUTHORIZED
```

The correct chain is:

```text
NETWORK
   ↓
AUTHENTICATION
   ↓
AUTHORIZATION
   ↓
SESSION
   ↓
CONTROL LEASE
   ↓
GNOME PREPARATION
   ↓
PHYSICAL DISPLAY ISOLATED
   ↓
PHYSICAL INPUT ISOLATED
   ↓
WEBRTC MEDIA READY
   ↓
REMOTE_ACTIVE
```

If any stage fails:

```text
DENY REMOTE CONTROL
+
REVOKE LEASE
+
FAIL SAFE
```

Do not optimize away these boundaries for convenience.