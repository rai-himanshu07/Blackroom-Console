# Remote Console — Project Master Plan

## 1. Project Mission

Build an open-source, secure remote-access system specifically for Ubuntu 26.04 LTS with GNOME 50+ running Wayland.

The system is intended for a normal physical Linux workstation that the owner may need to access remotely while physically away from the machine.

The defining behavior is:

> Allow remote access to the user's existing GNOME desktop session while physically isolating the workstation, and automatically fail closed by locking the session and restoring the physical console whenever remote control is lost or becomes unreliable.

This is not intended to be a generic VNC/RDP clone.

The primary product problem is:

- access the same existing GNOME session remotely
- do not create a separate desktop for normal remote use
- prevent people physically near the workstation from seeing or controlling the remote session
- make remote failure safe
- provide an independent physical emergency takeover mechanism
- allow access from a completely different device without requiring physical presence at the host

---

# 2. Initial Supported Platform

Version 1 must intentionally have a narrow platform target.

Supported:

- Ubuntu 26.04 LTS
- GNOME 50+
- Wayland
- systemd
- PipeWire
- Mutter
- standard Ubuntu GNOME desktop session
- single-user workstation

Do not initially support:

- KDE Plasma
- wlroots-based desktops
- X11
- other desktop environments
- Windows hosts
- macOS hosts
- multi-user remote sessions
- generic Linux distributions
- legacy GNOME versions
- generic RDP server compatibility
- generic VNC compatibility

Future support may be added through separate platform backends.

Do not weaken the initial architecture to support platforms outside this scope.

---

# 3. Product Principles

The implementation must follow these principles.

## 3.1 Same-session first

Remote access must attach to the existing GNOME user session.

Do not create a second independent desktop session for the normal workflow.

If the user has:

- VS Code open
- terminals open
- browser open
- applications running
- unsaved work
- existing GNOME state

the remote connection must access that same session.

After remote disconnect and local unlock, the user must return to that same session.

---

## 3.2 Fail closed

The system must prefer:

> locked and inconvenient

over:

> unlocked and remotely controllable.

The critical security invariant is:

> Remote failure must never leave the physical workstation unlocked and remotely controllable.

---

## 3.3 Physical privacy is a security property

While remote access is active:

- physical display must not expose the active remote desktop
- physical keyboard must not control the remote session
- physical mouse must not control the remote session
- remote keyboard/mouse must remain functional

Do not consider a black fullscreen window equivalent to physical privacy if the underlying display topology can safely be controlled.

Prefer disabling/removing physical outputs from the active display topology through Mutter mechanisms where technically safe.

---

## 3.4 Remote control is temporary authority

Remote control must not be treated as a permanent privilege.

Use a short-lived remote-control lease.

No valid lease means:

> no remote input authority.

The lease must expire or be revoked when:

- connection is lost
- session becomes unhealthy
- host daemon fails
- emergency takeover occurs
- remote session is explicitly terminated
- security epoch changes

---

## 3.5 Emergency recovery must be independent

The emergency recovery path must not depend on:

- browser
- WebRTC
- network
- remote gateway
- main remote daemon
- GNOME Shell extension
- remote session agent being healthy

A small independent privileged component must provide a physical emergency shortcut.

---

# 4. High-Level Architecture

Use separate privilege and responsibility boundaries.

```text
                         INTERNET / LAN
                               |
                         HTTPS / WebRTC
                               |
                               v
                  +--------------------------+
                  |     Remote Web Gateway   |
                  |       UNPRIVILEGED       |
                  +------------+-------------+
                               |
                        authenticated IPC
                               |
                               v
                  +--------------------------+
                  |       remote-hostd       |
                  |      SYSTEM SERVICE      |
                  +------------+-------------+
                               |
                        authenticated IPC
                               |
                               v
                  +--------------------------+
                  |   GNOME Session Agent    |
                  |        USER SESSION      |
                  +------------+-------------+
                               |
                 +-------------+--------------+
                 |             |              |
                 v             v              v
              Mutter       PipeWire         libei
                 |
                 v
          GNOME / Wayland Session


                  +--------------------------+
                  |   remote-emergencyd      |
                  |   MINIMAL PRIVILEGED     |
                  |   SAFETY COMPONENT       |
                  +------------+-------------+
                               |
                         Physical hotkey
                               |
                               v
                     Emergency takeover
```

---

# 5. Component Responsibilities

## 5.1 remote-hostd

System-level service.

Responsibilities:

- host identity
- authentication orchestration
- PAM authentication
- TOTP verification
- Remote Access Key validation
- trusted-client management
- session authorization
- remote-control leases
- security epoch
- active-session management
- connection lifecycle
- fail-safe state machine
- communication with GNOME session agent
- systemd watchdog
- security logging
- configuration
- rendezvous registration
- policy enforcement

It must not expose arbitrary command execution.

It must not provide an API equivalent to:

```text
execute(command)
```

---

# 6. GNOME Session Agent

The GNOME session agent runs within the user's GNOME session.

Responsibilities:

- identify active GNOME session
- communicate with Mutter
- manage remote desktop session
- manage virtual monitor
- manage PipeWire capture
- manage remote input
- manipulate temporary physical monitor topology
- restore physical display topology
- coordinate GNOME session locking
- report session state
- enforce remote-control lease state

Relevant GNOME/Mutter technologies to investigate:

- `org.gnome.Mutter.RemoteDesktop`
- `org.gnome.Mutter.ScreenCast`
- `org.gnome.Mutter.DisplayConfig`
- `RecordVirtual`
- PipeWire
- libei
- GDM 50
- `gnome-headless-session`

Private/unstable Mutter APIs must be isolated behind a dedicated abstraction.

Do not scatter private Mutter calls throughout the project.

---

# 7. Remote Web Gateway

The gateway is the Internet/LAN-facing component.

Responsibilities:

- HTTPS
- WebSocket signalling
- browser client delivery
- WebRTC signalling
- authentication request forwarding
- rate limiting
- connection lifecycle

It must be unprivileged.

It must not:

- run as root
- execute arbitrary commands
- access `/dev/input` unless independently proven necessary and isolated
- directly manipulate Mutter
- directly manipulate display configuration
- access TOTP secrets unnecessarily
- access the Remote Access Key secret unnecessarily
- have unrestricted system D-Bus access

---

# 8. Emergency Controller

Implement a separate `remote-emergencyd`.

It should be deliberately tiny.

Responsibilities:

1. detect configured physical emergency shortcut
2. revoke remote control authority
3. terminate remote sessions
4. increment security epoch
5. lock GNOME
6. restore physical display
7. restore physical input
8. optionally disable remote access

It must not contain:

- web server
- WebRTC
- codec implementation
- plugin framework
- arbitrary command execution
- remote management interface
- unnecessary filesystem access

The emergency component is a safety mechanism, not another general-purpose daemon.

---

# 9. Authentication Model

Every remote session requires:

1. Linux system username
2. Linux system password
3. TOTP authenticator code

TOTP is mandatory for every session.

A trusted device does NOT bypass TOTP.

For a new/untrusted client, additionally require:

4. Remote Access Key

Therefore:

```text
TRUSTED CLIENT

username
+
Linux password
+
TOTP
+
trusted-client credential
```

versus:

```text
NEW CLIENT

username
+
Linux password
+
TOTP
+
Remote Access Key
```

The new client may then become trusted.

---

# 10. Linux Username and Password

Use the system's normal Linux authentication mechanism/PAM rather than creating a second password database.

The Linux password:

- must never be logged
- must never be stored by the application
- must never enter WebRTC/session-agent components
- must not appear in crash reports
- must only exist for the authentication operation

Research Ubuntu 26.04 PAM behavior before implementing.

Do not make assumptions based solely on older Ubuntu versions.

---

# 11. TOTP

Implement standard TOTP.

The system must be compatible with common authenticator applications, including:

- Google Authenticator
- Microsoft Authenticator
- Authy
- Aegis
- Bitwarden
- 1Password
- other standard TOTP clients

Do not build vendor-specific integrations.

Initial setup:

```text
Remote Access Settings
        |
        v
Generate TOTP secret
        |
        v
Display QR code
        |
        v
User scans with authenticator
        |
        v
User enters current OTP
        |
        v
Verify
        |
        v
TOTP enabled
```

The TOTP secret must never be returned to remote clients after setup.

---

# 12. Remote Access Key

Generate the Remote Access Key automatically using a cryptographically secure random generator.

Prefer:

- at least 256 bits of entropy
- Base64url or similarly safe encoding
- password-manager-friendly representation

Do not require the user to invent the key.

Do not expect the user to memorize it.

The UI should explain:

> The Remote Access Key is required when connecting from a new or untrusted device. Store it securely in a password manager.

The key must be independently:

- revocable
- rotatable
- replaceable

Do not derive it from the Linux password.

---

# 13. Trusted Clients

Trusted-client authorization is a convenience layer.

It is not the root authentication mechanism.

After:

```text
username
+
password
+
TOTP
+
Remote Access Key
```

succeeds from a new browser/device, offer:

> Trust this device?

If accepted, register a cryptographic client credential.

Later:

```text
username
+
password
+
TOTP
+
trusted client credential
```

is sufficient.

The Remote Access Key is not required for that trusted client.

However:

> TOTP remains mandatory.

A different, completely unknown device must always be able to regain access using the full authentication flow.

Never design the system so that losing a trusted laptop causes permanent remote lockout.

---

# 14. Trusted Device Management

Provide a UI similar to:

```text
Trusted Devices

Laptop
  Chrome / Linux
  Last used: Today

Work Laptop
  Chrome / Windows
  Last used: Yesterday

Tablet
  Android
  Last used: 3 days ago

[Revoke]
[Revoke All]
```

Revocation must immediately prevent the revoked credential from creating new sessions.

Existing sessions associated with that credential should also be terminated.

---

# 15. Authentication Security

Do not expose detailed authentication errors.

Do not tell an unauthenticated client:

- username exists
- password was correct
- TOTP was incorrect
- Access Key was incorrect

Prefer:

> Authentication failed.

Implement:

- rate limiting
- authentication attempt throttling
- appropriate account protection
- replay resistance
- short-lived authentication/session credentials
- secure cookie handling
- CSRF protection where applicable
- strict origin validation
- secure WebSocket authentication

---

# 16. Remote Session Credential

After authentication, create a separate short-lived session credential.

Do not use the user's password as the remote session credential.

Do not use the TOTP secret as a session credential.

Do not use the Remote Access Key as a session credential.

Session credentials must be:

- short-lived
- revocable
- bound to the authenticated user
- bound to the client where possible
- bound to the current security epoch
- capability-limited

---

# 17. Remote Control Lease

Create a remote-control lease.

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

The GNOME session agent must reject remote input if:

- lease expired
- lease revoked
- session ended
- client revoked
- security epoch changed
- host entered failsafe
- emergency takeover occurred

The lease should be renewed while the connection is healthy.

---

# 18. Security Epoch

Maintain a monotonically increasing security epoch.

Example:

```text
security_epoch = 41
```

Every remote session is associated with the current epoch.

Emergency takeover:

```text
41 -> 42
```

All credentials/leases bound to epoch 41 become invalid.

This provides a simple global revocation mechanism.

---

# 19. Core State Machine

Implement the state machine explicitly.

```text
LOCAL_ACTIVE
     |
     | remote authentication
     v
REMOTE_AUTHENTICATING
     |
     | authentication successful
     v
REMOTE_PREPARING
     |
     | create virtual monitor
     | establish capture
     | establish remote input
     | isolate physical console
     v
REMOTE_ACTIVE
     |
     +-------------------------+
     |                         |
     | normal disconnect       | failure
     v                         v
REMOTE_STOPPING             FAILSAFE
     |                         |
     +-------------+-----------+
                   |
                   v
                LOCKING
                   |
                   v
       RESTORING_PHYSICAL_CONSOLE
                   |
                   v
                LOCKED
                   |
                   | local unlock
                   v
              LOCAL_ACTIVE
```

Emergency path:

```text
ANY REMOTE STATE
       |
       v
EMERGENCY
       |
       +-- revoke lease
       +-- terminate remote sessions
       +-- increment epoch
       +-- lock GNOME
       +-- restore display
       +-- restore input
       |
       v
     LOCKED
```

---

# 20. Remote Activation Sequence

The preferred sequence is:

```text
1. Authenticate user
2. Authenticate TOTP
3. Validate Access Key if new/untrusted client
4. Authorize requested capabilities
5. Acquire remote-control lease
6. Lock/secure physical console as required
7. Create/attach virtual monitor
8. Establish PipeWire capture
9. Establish remote input
10. Verify virtual session is healthy
11. Disable physical outputs
12. Disable physical input
13. Transition to REMOTE_ACTIVE
14. Start lease heartbeat
```

The system must not transition to `REMOTE_ACTIVE` until all required privacy controls are confirmed.

---

# 21. Remote Disconnect Sequence

Normal disconnect:

```text
1. Stop accepting remote input
2. Revoke control lease
3. Terminate remote transport
4. Lock GNOME session
5. Restore physical monitor configuration
6. Restore physical input
7. Destroy virtual monitor
8. Verify physical console is restored
9. Remain LOCKED
```

Only a physical local user unlocks the session.

Do not automatically unlock.

---

# 22. Unexpected Failure

For:

- network failure
- browser crash
- WebRTC failure
- gateway crash
- host daemon failure
- session agent failure
- heartbeat timeout
- lease expiration

the expected behavior is:

```text
remote input revoked
        |
        v
remote session terminated
        |
        v
GNOME locked
        |
        v
physical display restored
        |
        v
physical input restored
        |
        v
LOCKED
```

The exact implementation may vary depending on which component failed, but the security invariant must remain.

---

# 23. Physical Display Isolation

Do not use a black fullscreen application as the primary privacy mechanism.

Preferred architecture:

```text
Physical monitors
       |
       X
       |
GNOME active desktop

Virtual monitor
       |
       v
remote desktop
```

Use Mutter's display/virtual-monitor facilities to create a remote output and temporarily disable the physical outputs.

Before changing display configuration:

- capture the current topology
- capture relevant modes
- capture logical monitor layout
- capture scale/transform where relevant

On termination:

- restore the previous configuration
- use temporary configuration where possible
- do not permanently modify the user's normal display configuration

---

# 24. Physical Input Isolation

This is a critical technical requirement.

During `REMOTE_ACTIVE`:

```text
Physical keyboard -> MUST NOT control GNOME
Physical mouse    -> MUST NOT control GNOME

Remote keyboard   -> MUST control GNOME
Remote mouse      -> MUST control GNOME
```

Do not assume this works merely because the physical display is disabled.

Research and experimentally validate:

- Mutter input handling
- libei
- libinput
- uinput
- Wayland input routing
- possible session-level input gating

Avoid globally grabbing `/dev/input/event*` unless no safer mechanism exists.

If a privileged input mechanism is unavoidable, isolate it in a minimal component and document exactly why the privilege is required.

---

# 25. Emergency Takeover

Default shortcut:

```text
Ctrl + Alt + Shift + F12
```

Make it configurable.

Prefer a hold duration, initially approximately two seconds.

The emergency mechanism must work independently of the main remote agent.

Expected action:

```text
Emergency shortcut
       |
       v
Revoke remote input
       |
       v
Terminate remote session
       |
       v
Increment security epoch
       |
       v
Lock GNOME
       |
       v
Restore physical displays
       |
       v
Restore physical input
       |
       v
Remain LOCKED
```

Optional configuration:

```text
Emergency action:

[ ] Revoke current remote session
[x] Revoke current session + disable remote access
```

Do not automatically unlock the machine.

---

# 26. Browser-Based Remote Access

The primary client is a web browser.

Desired experience:

```text
Open browser
      |
      v
Find/select host
      |
      v
Authenticate
      |
      v
Remote GNOME desktop
```

The client should eventually work from:

- Linux
- Windows
- macOS
- Android
- iOS/tablets

provided the browser supports the required WebRTC APIs.

No native client is required for v1.

---

# 27. WebRTC

Use WebRTC as the primary remote data plane.

Architecture:

```text
Mutter
  |
PipeWire
  |
Video encoder
  |
WebRTC
  |
Browser
```

WebRTC provides mature mechanisms for:

- encryption
- NAT traversal
- congestion control
- low-latency transport
- media transport
- data channels

Use WebRTC rather than inventing a custom remote-desktop transport for v1.

---

# 28. Signalling

Use:

- HTTPS
- WebSocket

for signalling/control-plane communication.

Do not send desktop video through the signalling service.

The signalling service should only coordinate connection establishment.

---

# 29. NAT and Dynamic IP

Support both:

### Direct connection

```text
Browser
   |
   v
Host IP/hostname
```

and:

### Rendezvous

```text
Host
 |
 | outbound persistent connection
 v
Rendezvous server

Browser
 |
 | find host
 v
Rendezvous
 |
 | connection negotiation
 v
Host
```

Use ICE/STUN/TURN for NAT traversal.

Preferred path:

1. LAN/direct
2. IPv6/direct
3. public direct where possible
4. NAT traversal
5. TURN relay

---

# 30. TURN

TURN should relay encrypted traffic.

The relay must not need access to:

- desktop pixels in plaintext
- keyboard contents
- clipboard
- passwords
- TOTP
- Remote Access Key

Self-hosted TURN should be supported eventually.

---

# 31. LAN Discovery

Support mDNS/Avahi where appropriate.

Example:

```text
Ubuntu-PC.local
```

However, hostname/IP is not the cryptographic identity.

Use a generated device identity.

---

# 32. Host Identity

At installation generate a cryptographic host identity.

Conceptually:

```text
host_id
host_private_key
host_public_key
```

Do not use:

- MAC address
- IP address
- hostname

as the cryptographic identity.

---

# 33. Client Identity

Trusted clients should receive/store a cryptographic client credential.

Example:

```text
client_id
client_private_key
client_public_key
```

The private key must remain protected by the client/browser environment.

A trusted client credential is an authorization convenience, not a replacement for TOTP.

---

# 34. Browser Security

The web application must use:

- HTTPS only
- Secure cookies
- SameSite cookies
- CSRF protection where applicable
- strict Content Security Policy
- origin validation
- WebSocket authentication
- short-lived session credentials
- session expiration
- secure logout
- no credentials in URLs
- no password storage
- no TOTP secret storage
- no Remote Access Key in localStorage

Do not use ordinary browser localStorage as the storage location for long-lived secrets.

---

# 35. No Remote Shell

The remote protocol must not expose arbitrary command execution.

Do not implement:

```text
run_command(command)
```

Instead define explicit operations.

Examples:

```text
start_remote_session
stop_remote_session
request_control
send_keyboard_event
send_pointer_event
request_clipboard
```

Every operation must have an authorization check.

---

# 36. Capabilities

Initially support:

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

Do not implement future capabilities merely because the protocol can support them.

---

# 37. Concurrent Sessions

For v1:

- allow one controlling remote session
- reject additional controlling sessions by default

Future versions may support:

- view-only observers
- explicit controller handoff

Do not implement simultaneous controllers initially.

---

# 38. Power Management

Display sleep may remain enabled.

While remote access is active, prevent system suspend if required for the remote session.

Use systemd/logind mechanisms.

If the machine actually suspends:

```text
remote session terminates
+
machine returns to safe locked state
```

Wake-on-LAN is optional and hardware-dependent.

Do not make it a core v1 requirement.

---

# 39. Configuration

The final implementation should expose settings conceptually equivalent to:

```yaml
remote_access:
  enabled: true

authentication:
  require_linux_password: true
  require_totp: true
  require_access_key_for_new_clients: true

session:
  same_gnome_session: true
  lock_on_disconnect: true
  lock_before_remote_activation: true

privacy:
  disable_physical_display: true
  disable_physical_input: true

failsafe:
  enabled: true
  emergency_hold_seconds: 2
  invalidate_remote_sessions: true

network:
  lan_discovery: true
  rendezvous: true
  turn: true

media:
  codec: h264

power:
  keep_awake_while_remote: true
```

The exact configuration format can be decided by the implementation architecture.

---

# 40. Diagnostics

Provide a diagnostic command such as:

```bash
remote-console doctor
```

It should report:

```text
Ubuntu version
GNOME version
Wayland
PipeWire
Mutter RemoteDesktop
Mutter ScreenCast
RecordVirtual
DisplayConfig
libei
systemd
GPU
hardware acceleration
emergency controller
```

Each capability should be classified as:

```text
PASS
WARN
FAIL
UNSUPPORTED
```

Do not claim support simply because a package exists.

---

# 41. Status Command

Provide:

```bash
remote-console status
```

Example:

```text
Remote access: ENABLED
GNOME session: ACTIVE
Remote session: DISCONNECTED
Physical display: ENABLED
Physical input: ENABLED
Security epoch: 12
TOTP: CONFIGURED
Remote Access Key: CONFIGURED
Trusted clients: 3
```

Never display secrets.

---

# 42. Setup Wizard

Initial setup should verify:

```text
Ubuntu 26.04              PASS
GNOME 50+                 PASS
Wayland                   PASS
PipeWire                  PASS
Mutter RemoteDesktop      PASS
Virtual monitor           PASS
libei                     PASS
systemd                   PASS
Emergency controller      PASS
```

Then configure:

- Remote Access
- TOTP
- Remote Access Key
- emergency shortcut
- privacy policy
- power policy

Before enabling unattended remote access, strongly prefer requiring a successful emergency-recovery test.

---

# 43. Emergency Recovery Test

The setup wizard should provide:

```text
[Test Emergency Recovery]
```

It should enter a controlled remote state and ask the user to trigger the physical emergency shortcut.

Verify:

```text
Remote authority revoked       PASS
Remote session terminated      PASS
GNOME locked                   PASS
Physical display restored      PASS
Physical input restored        PASS
Security epoch incremented     PASS
```

Do not allow production/unattended mode to be marked fully configured if the emergency mechanism has not been successfully validated, unless the user explicitly overrides it.

---

# 44. Logging

Security logs may contain:

- timestamp
- event
- client identifier
- host identifier
- session identifier
- result
- reason

Never log:

- passwords
- TOTP values
- TOTP secrets
- Remote Access Keys
- private keys
- session tokens

Example:

```text
REMOTE_AUTH_SUCCESS
client=abc123
session=xyz789
method=password+totp+access_key
```

Avoid unnecessary personally identifying data.

---

# 45. Security Events

Record events such as:

- authentication success
- authentication failure
- TOTP failure
- Access Key failure
- new client registration
- client revocation
- session started
- session ended
- lease expired
- emergency takeover
- security epoch changed
- Remote Access Key rotated
- TOTP configuration changed
- remote access disabled

---

# 46. Secret Storage

Centralize secret management.

Conceptually:

```text
SecretStore
    |
    +-- host identity
    +-- Remote Access Key verifier
    +-- TOTP secret
    +-- trusted client credentials
    +-- revocation metadata
```

Do not scatter secrets throughout configuration files.

Use restrictive permissions and OS facilities where appropriate.

---

# 47. Threat Model

Create a dedicated threat model covering attackers who possess:

- host IP
- hostname
- LAN access
- intercepted network traffic
- Remote Access Key
- Linux password
- TOTP
- trusted client credential
- browser session
- temporary physical access
- compromised TURN/rendezvous server

For each scenario define:

- what the attacker can do
- what they cannot do
- which security boundary stops them
- what recovery mechanism exists

---

# 48. Example Threat Expectations

```text
Attacker knows IP
    -> cannot authenticate

Attacker knows username + password
    -> TOTP still required

Attacker knows Access Key
    -> password + TOTP still required

Attacker steals trusted client credential
    -> TOTP still required

Attacker compromises TURN
    -> sees encrypted relay traffic but not desktop contents

Attacker steals active remote session
    -> short-lived session + revocation + security epoch limit exposure

Emergency shortcut triggered
    -> remote authority revoked
       session locked
       physical console restored
```

---

# 49. Systemd Hardening

Use systemd security controls wherever practical.

Investigate:

- `NoNewPrivileges=`
- filesystem restrictions
- capability restrictions
- device restrictions
- private temporary directories
- syscall restrictions where appropriate
- network restrictions
- user/group isolation
- watchdog support

Do not run an Internet-facing service as unrestricted root.

---

# 50. Repository and Workflow Configuration

The repository will be configured using the existing `adaptive-workflow-configurator` project before implementation begins.

The implementation must respect the workflow/configuration established by that tool.

Do not:

- overwrite its configuration
- replace its workflow system
- introduce a competing agent workflow
- duplicate its instructions unnecessarily
- reorganize the repository solely for stylistic reasons

First inspect the resulting repository configuration and follow its established conventions.

The project-specific implementation plan should complement, not fight, the configured workflow.

---

# 51. Development Methodology

Work in explicit phases.

For each phase:

1. Research
2. Document assumptions
3. Implement smallest testable component
4. Add automated tests
5. Perform failure testing
6. Update documentation
7. Review security implications
8. Only then proceed

Every important technical assumption must be classified as:

```text
CONFIRMED
LIKELY
UNVERIFIED
UNSUPPORTED
```

Never silently treat an unverified GNOME behavior as confirmed.

---

# 52. Critical Development Rule

Do not start by implementing the complete product.

The first goal is proving the GNOME/Wayland architecture.

Do not spend significant effort on:

- browser UI
- Internet networking
- rendezvous
- TURN
- polished authentication UI

until the same-session/virtual-monitor/input-isolation/fail-safe behavior has been experimentally validated.

---

# 53. Development Phases

## Phase 0 — Repository and workflow inspection

Inspect:

- adaptive-workflow-configurator output
- repository structure
- agent instructions
- coding conventions
- testing conventions
- documentation conventions
- CI configuration

Do not modify workflow configuration without justification.

Deliverable:

```text
Repository/workflow assessment
```

---

## Phase 1 — GNOME feasibility research

Research current Ubuntu 26.04 / GNOME 50 source and documentation.

Focus on:

- Mutter
- GNOME Shell
- GDM
- GNOME Remote Desktop
- `gnome-headless-session`
- RemoteDesktop API
- ScreenCast API
- RecordVirtual
- DisplayConfig
- PipeWire
- libei
- libinput
- systemd/logind

Prefer current upstream source and current Ubuntu packages over old tutorials.

Deliverable:

```text
docs/gnome/feasibility-research.md
```

---

# 54. Phase 2 — GNOME PoC

Build a minimal experimental program.

No Internet.

No production authentication.

No polished UI.

The PoC must establish:

1. existing GNOME session detected
2. same session accessed
3. virtual monitor created
4. PipeWire capture established
5. remote input established
6. physical outputs disabled
7. physical input isolated
8. remote interaction works
9. connection failure detected
10. remote authority revoked
11. GNOME session locked
12. physical displays restored
13. physical input restored
14. local unlock returns to same session
15. existing applications remain

---

# 55. Phase 3 — Emergency PoC

Independently kill the main remote agent.

Trigger the physical emergency shortcut.

The emergency mechanism must still:

```text
revoke
lock
restore
```

If this cannot be made reliable, stop and revisit the architecture.

---

# 56. Phase 4 — State Machine

Implement the production state machine independently of WebRTC.

Test every transition.

No implicit state transitions.

---

# 57. Phase 5 — Local Browser Prototype

Connect:

```text
Browser
    |
    v
Local host
```

Implement:

- WebRTC
- screen streaming
- keyboard
- mouse
- connection status

No Internet rendezvous initially.

---

# 58. Phase 6 — Authentication

Implement:

- PAM
- username/password
- TOTP
- Remote Access Key
- session credentials
- trusted clients
- revocation
- security epoch
- control lease

---

# 59. Phase 7 — Internet Connectivity

Implement:

- signalling
- rendezvous
- ICE
- STUN
- TURN
- dynamic IP handling
- LAN discovery

---

# 60. Phase 8 — Hardening

Implement and test:

- systemd sandboxing
- rate limiting
- secure secret storage
- session expiration
- token revocation
- emergency recovery
- failure handling
- dependency review
- fuzzing where appropriate

---

# 61. Phase 9 — Packaging

Create:

- Debian package
- systemd units
- installation wizard
- configuration migration
- uninstall procedure
- upgrade procedure
- diagnostics

---

# 62. Testing Matrix

Test:

### Authentication

- correct username/password/TOTP
- incorrect password
- incorrect TOTP
- expired TOTP
- incorrect Access Key
- new client
- trusted client
- revoked client
- rotated Access Key
- concurrent sessions

### Remote Session

- start
- stop
- reconnect
- timeout
- lease expiration
- epoch invalidation

### Display

- single monitor
- multiple monitors
- HDMI
- DisplayPort
- high DPI
- scaling
- 60 Hz
- high refresh
- monitor unplug
- monitor replug
- display sleep/wake

### Input

- keyboard
- mouse
- modifiers
- scroll
- physical input suppression
- remote input availability

### Failure

- Wi-Fi disconnect
- Ethernet disconnect
- browser crash
- WebRTC failure
- gateway crash
- host daemon crash
- session agent crash
- PipeWire crash
- GNOME state changes
- suspend/resume

### Emergency

- main daemon running
- main daemon killed
- gateway killed
- network disconnected
- remote client malicious/stuck
- repeated emergency trigger
- recovery after emergency

---

# 63. Hardware Testing

Where hardware is available, test:

- Intel GPU
- AMD GPU
- NVIDIA GPU

Also test:

- single monitor
- multi-monitor
- high DPI
- high refresh
- monitor hotplug
- display sleep/wake

GPU-specific behavior must be documented.

---

# 64. Browser Testing

Test:

- Chromium/Chrome
- Firefox
- Safari where available
- Edge
- Android browser
- iOS browser

Record any browser-specific WebRTC/input limitations.

---

# 65. Media

Initial codec:

```text
H.264
```

Prioritize correctness and compatibility.

Later consider:

- AV1
- adaptive bitrate
- hardware encoding
- HDR
- multi-monitor
- advanced cursor transport

---

# 66. Clipboard

Treat clipboard as a later feature.

When implemented:

- explicit permission
- clearly defined direction
- avoid unexpected transfer
- never expose clipboard through arbitrary RPC

---

# 67. Files and Power

Do not implement file transfer or power control in the initial remote protocol.

They require separate threat models and authorization capabilities.

---

# 68. Production Security Criteria

Do not call the system production-ready until all of the following are satisfied:

- TOTP mandatory for every remote session
- Access Key required for new/untrusted clients
- trusted client revocation works
- no plaintext credentials in logs
- Internet-facing gateway not running unrestricted as root
- session credentials expire
- control leases expire
- emergency takeover works
- network failure safely recovers
- daemon failure safely recovers
- physical display isolation verified
- physical input isolation verified
- same-session continuity verified
- security epoch invalidation works
- Remote Access Key rotation works
- dependency/security review completed
- update mechanism is authenticated/signed

---

# 69. Final Acceptance Criteria

The project must demonstrate this complete workflow:

```text
LOCAL ACTIVE
     |
     | remote connection
     v
AUTHENTICATION
     |
     +-- username
     +-- Linux password
     +-- TOTP
     +-- Access Key if new/untrusted
     |
     v
REMOTE PREPARATION
     |
     +-- same GNOME session
     +-- virtual monitor
     +-- PipeWire
     +-- remote input
     +-- physical display isolated
     +-- physical input isolated
     |
     v
REMOTE ACTIVE
     |
     | normal disconnect OR failure
     v
REMOTE AUTHORITY REVOKED
     |
     v
GNOME LOCKED
     |
     v
PHYSICAL DISPLAY RESTORED
     |
     v
PHYSICAL INPUT RESTORED
     |
     v
LOCKED
     |
     | physical user unlocks
     v
LOCAL ACTIVE
```

Emergency:

```text
REMOTE ACTIVE
     |
     | physical emergency shortcut
     v
EMERGENCY TAKEOVER
     |
     +-- revoke remote authority
     +-- terminate remote session
     +-- increment security epoch
     +-- lock GNOME
     +-- restore physical display
     +-- restore physical input
     |
     v
LOCKED
```

The most important invariant remains:

> **At no point should loss or compromise of the remote-control path leave the physical workstation both unlocked and remotely controllable.**

---

# 70. First Instruction to Copilot Agent

Do not implement the complete product yet.

Start with:

1. Inspect the repository and the configuration produced by `adaptive-workflow-configurator`.
2. Identify and follow its established agent/workflow conventions.
3. Research Ubuntu 26.04 + GNOME 50+ + Wayland.
4. Research current Mutter RemoteDesktop, ScreenCast, RecordVirtual and DisplayConfig behavior.
5. Research PipeWire, libei, libinput and relevant session-locking mechanisms.
6. Build the smallest GNOME feasibility PoC.
7. Document every assumption as CONFIRMED, LIKELY, UNVERIFIED or UNSUPPORTED.
8. Do not proceed to Internet networking or polished application architecture until the PoC proves the critical same-session and privacy/failsafe requirements.

The first meaningful deliverable is therefore **not a remote desktop application**.

It is a technical feasibility report plus a working GNOME/Wayland PoC demonstrating that the required security and session behavior is actually achievable on Ubuntu 26.04/GNOME 50+.