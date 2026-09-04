# Systemd Services & Privilege Model

## 1. Purpose

This document defines the Linux process, systemd, privilege, sandboxing, startup, watchdog, IPC, and recovery architecture for the Remote Console project.

It covers:

- `remote-hostd`
- `gnome-session-agent`
- `remote-gateway`
- `remote-emergencyd`
- systemd service lifecycle
- systemd user services
- privilege separation
- capabilities
- filesystem permissions
- D-Bus access
- IPC
- watchdogs
- startup ordering
- shutdown
- crash recovery
- sandboxing
- resource limits
- emergency recovery independence

Target platform:

```text
Ubuntu 26.04 LTS
GNOME 50+
Wayland
systemd
single-user workstation
```

---

# 2. Core Principle

The project must never run the entire remote-access stack as root.

The intended privilege model is:

```text
                    INTERNET
                       |
                       v
              remote-gateway
              UNPRIVILEGED
                       |
                authenticated IPC
                       |
                       v
                remote-hostd
              SYSTEM SERVICE
                       |
                authenticated IPC
                       |
                       v
            gnome-session-agent
              USER SESSION
```

And independently:

```text
              Physical Keyboard
                     |
                     v
             remote-emergencyd
              MINIMAL PRIVILEGE
```

---

# 3. Process Responsibilities

## `remote-gateway`

Network-facing process.

Responsible for:

- HTTPS
- WebSocket
- signalling
- WebRTC coordination
- connection management
- rate limiting
- browser serving

It should be unprivileged.

---

## `remote-hostd`

System-level security/orchestration daemon.

Responsible for:

- authentication
- PAM coordination
- TOTP
- Remote Access Key
- trusted clients
- authorization
- security epoch
- session credentials
- control leases
- remote state
- fail-safe orchestration
- communication with the GNOME agent

---

## `gnome-session-agent`

User-session process.

Responsible for:

- GNOME session discovery
- Mutter
- ScreenCast
- RemoteDesktop
- PipeWire
- virtual monitor
- physical display isolation
- libei/EIS
- physical input isolation
- GNOME lock
- display restoration

It must run as the target user, not root.

---

## `remote-emergencyd`

Minimal local safety daemon.

Responsible for:

- emergency hotkey
- immediate remote-input revocation
- emergency state transition
- security epoch invalidation
- triggering session lock
- physical display restoration
- physical input restoration

It must have no network-facing functionality.

---

# 4. Privilege Domains

Use four explicit domains:

```text
DOMAIN A
Network
remote-gateway
UNPRIVILEGED

DOMAIN B
Security
remote-hostd
SYSTEM SERVICE

DOMAIN C
Desktop
gnome-session-agent
USER SESSION

DOMAIN D
Emergency
remote-emergencyd
MINIMAL PRIVILEGE
```

No component should automatically inherit another component's privileges.

---

# 5. Why the Gateway Must Be Unprivileged

The gateway is exposed to hostile network traffic.

Therefore:

> The most exposed process should have the least privilege.

A compromise of `remote-gateway` must not directly provide:

- root
- `/dev/input` access
- GNOME control
- PAM access
- TOTP secret
- Remote Access Key
- arbitrary system commands

---

# 6. Gateway → Host IPC

The gateway should communicate with `remote-hostd` using a narrow authenticated IPC interface.

Conceptually:

```text
remote-gateway
      |
      | authenticated IPC
      v
remote-hostd
```

The gateway should not have unrestricted access to the host daemon's socket.

---

# 7. No Generic RPC

Do not expose:

```text
call(method, arbitrary_arguments)
```

as the host IPC interface.

Instead expose explicit operations such as:

```text
CreateAuthenticationSession
Authenticate
CreateRemoteSession
SignalConnection
RequestControl
ReleaseControl
DisconnectSession
```

Every operation should have explicit authorization requirements.

---

# 8. GNOME Agent IPC

The GNOME agent should have a separate IPC interface.

```text
remote-hostd
      |
      | authenticated local IPC
      v
gnome-session-agent
```

The interface should contain only GNOME operations required by the application.

---

# 9. No D-Bus Proxy

Do not implement:

```text
remote-hostd
   |
   v
gnome-session-agent
   |
   v
arbitrary D-Bus call
```

such as:

```text
call(service, object, method, arguments)
```

This would effectively create a privileged D-Bus tunnel.

---

# 10. Explicit GNOME Operations

Use operations such as:

```text
GetSessionState
GetDisplayState
CreateVirtualMonitor
DestroyVirtualMonitor
StartCapture
StopCapture
IsolatePhysicalDisplays
RestorePhysicalDisplays
EnableRemoteInput
DisableRemoteInput
LockSession
GetRecoveryState
```

The exact protocol may differ.

---

# 11. Systemd Service Layout

The conceptual deployment is:

```text
/etc/systemd/system/
    remote-hostd.service
    remote-gateway.service
    remote-emergencyd.service
```

and potentially:

```text
~/.config/systemd/user/
    gnome-session-agent.service
```

The exact installation paths may differ depending on packaging.

---

# 12. `remote-hostd.service`

This is a system service.

Conceptually:

```ini
[Unit]
Description=Remote Console Host Service
After=network-online.target
Wants=network-online.target

[Service]
ExecStart=...
Restart=on-failure

[Install]
WantedBy=multi-user.target
```

Do not copy this blindly into production.

Research appropriate systemd hardening and dependencies first.

---

# 13. Gateway Service

The gateway should preferably run separately.

Conceptually:

```ini
[Unit]
Description=Remote Console Gateway
After=network-online.target
Wants=network-online.target

[Service]
ExecStart=...
User=remote-gateway
Group=remote-gateway
Restart=on-failure
```

It should have no unnecessary system privileges.

---

# 14. Emergency Service

Conceptually:

```ini
[Unit]
Description=Remote Console Emergency Controller

[Service]
ExecStart=...
Restart=always
```

The exact startup dependencies must be minimized.

The emergency service should not depend on:

```text
remote-gateway
Internet
WebRTC
PipeWire
browser
```

---

# 15. Emergency Independence

This is a hard requirement.

If:

```text
remote-gateway = DEAD
remote-hostd = DEAD
WebRTC = DEAD
PipeWire = DEAD
GNOME agent = DEAD
```

the emergency mechanism should still have the best possible chance of operating.

This may require a carefully designed local fallback path.

---

# 16. Emergency Architecture

Preferred:

```text
Physical keyboard
       |
       v
remote-emergencyd
       |
       +----------------------+
       |                      |
       v                      v
local safety action     minimal IPC
                              |
                              v
                       recovery components
```

Do not require the full application stack.

---

# 17. Emergency Daemon Cannot Be Fully Independent

Some recovery operations may inherently require GNOME/session APIs.

For example:

```text
GNOME lock
Mutter display restore
```

Therefore independence means:

> The emergency trigger must not depend on the normal remote-control/network/browser path.

It does not mean that it must reimplement every GNOME subsystem itself.

---

# 18. Emergency Fallback

If `remote-hostd` is alive:

```text
emergencyd
    |
    v
remote-hostd
```

may coordinate the recovery.

If `remote-hostd` is unavailable:

```text
emergencyd
    |
    v
direct minimal recovery mechanism
```

should be used where technically possible.

Document exactly which operations require which component.

---

# 19. Emergency Design Review

Before implementation, explicitly map:

```text
Operation
    |
    +-- can emergencyd perform directly?
    |
    +-- requires GNOME agent?
    |
    +-- requires remote-hostd?
```

Do not assume that an IPC dependency makes the emergency mechanism independent.

---

# 20. Systemd Watchdog

Use systemd watchdog support where appropriate.

Conceptually:

```text
systemd
   |
   | watchdog
   v
remote-hostd
```

The daemon must regularly notify systemd that it is healthy.

---

# 21. Watchdog Does Not Equal Security

A watchdog only detects process health.

It does not prove:

```text
remote input is disabled
physical output is isolated
```

Those states must be independently verified.

---

# 22. Host Daemon Failure

If `remote-hostd` crashes:

```text
systemd
   |
   v
restart remote-hostd
```

But before remote control can resume:

```text
security state
+
epoch
+
GNOME state
+
lease state
```

must be reconciled.

Do not automatically restore stale remote control.

---

# 23. Daemon Restart Policy

Use:

```text
Restart=on-failure
```

or another deliberate policy.

Avoid an infinite rapid restart loop.

Use appropriate:

```text
RestartSec
StartLimitIntervalSec
StartLimitBurst
```

after testing.

---

# 24. Startup Recovery

On startup, `remote-hostd` should inspect:

```text
persisted security state
GNOME session state
remote session state
display state
input state
```

before accepting remote control.

---

# 25. Startup Must Fail Closed

If the daemon cannot determine whether the previous remote state was safely cleaned up:

```text
REMOTE_CONTROL = DISABLED
```

until the state is reconciled.

---

# 26. Security Epoch on Restart

The persisted security epoch must remain valid.

Consider incrementing the epoch during certain recovery conditions.

Any strategy must ensure that stale sessions cannot become valid again after restart.

---

# 27. Service Ordering

A possible dependency chain:

```text
network-online
       |
       v
remote-hostd
       |
       v
gnome-session-agent
```

However:

> Do not make the host daemon hard-dependent on a particular GNOME session startup order unless required.

GNOME sessions can start later.

---

# 28. GNOME Agent Startup

The user-session agent should start when the user's GNOME session is actually available.

Possible mechanism:

```text
systemd --user
```

with appropriate GNOME/session dependencies.

---

# 29. Agent Startup Before GNOME

If the agent starts too early:

```text
GNOME unavailable
```

It should:

- wait/retry
- report unavailable
- avoid unsafe assumptions

It should not fail permanently just because GNOME took longer to initialize.

---

# 30. Agent Logout

When the GNOME user session ends:

```text
gnome-session-agent
        |
        v
shutdown
```

Remote sessions must be invalidated.

---

# 31. User Session Lifecycle

Handle:

```text
session start
session active
session locked
session unlocked
session idle
session logout
session crash
```

Do not treat `active` and `unlocked` as identical.

---

# 32. systemd User Service

The GNOME agent should preferably run under the user's systemd user manager.

Benefits:

- user ownership
- lifecycle integration
- restart handling
- resource controls
- environment integration

---

# 33. User Service Security

The user-session agent should not gain additional privileges simply because it controls GNOME.

It should remain within the user's normal account privileges.

---

# 34. Host Daemon User

Create a dedicated service account if practical.

For example:

```text
remote-host
```

The exact username is not important.

Avoid running the daemon as:

```text
root
```

unless a specific operation genuinely requires it.

---

# 35. Host Daemon Privileges

Evaluate whether `remote-hostd` actually requires:

- root
- specific capabilities
- PAM access
- systemd interaction
- device access

Grant only the minimum.

---

# 36. PAM Privilege

PAM may require privileged access depending on the chosen integration.

Do not give the entire daemon unrestricted root access solely because PAM exists.

Prefer isolating the PAM operation if practical.

---

# 37. PAM Helper

Potential architecture:

```text
remote-hostd
      |
      v
pam-auth-helper
      |
      v
PAM
```

The helper should have:

- minimal lifetime
- minimal privileges
- minimal IPC
- no network
- no GNOME access

---

# 38. PAM Helper Security

The helper must never accept:

```text
arbitrary PAM service
arbitrary module
arbitrary configuration
```

from remote clients.

The application chooses the expected PAM service/configuration.

---

# 39. Capabilities

Do not add Linux capabilities preemptively.

For each capability:

```text
CAP_*
```

document:

1. why required
2. which process requires it
3. which code path uses it
4. whether it can be removed
5. what happens if it is removed

---

# 40. No `CAP_SYS_ADMIN` by Default

`CAP_SYS_ADMIN` is extremely broad.

Do not use it merely because it makes implementation easier.

If it becomes necessary for a particular fallback implementation:

- isolate it
- document it
- minimize the helper
- consider whether the design can avoid it

---

# 41. Device Access

Do not give `remote-gateway` access to:

```text
/dev/input/*
/dev/uinput
```

unless a future architecture explicitly requires it.

---

# 42. Input Helper

If physical input isolation eventually requires a privileged helper:

```text
remote-hostd
     |
     v
remote-input-helper
```

keep it separate from:

```text
remote-gateway
```

and the media stack.

---

# 43. Input Helper Responsibilities

A potential privileged input helper may only:

- identify approved physical devices
- apply the approved isolation operation
- restore the approved state
- report success/failure

It must not:

- interpret arbitrary commands
- execute programs
- expose raw input streams to the network
- provide generic `/dev/input` access

---

# 44. Device Allowlisting

If a privileged helper must interact with input devices, use explicit device identification.

Do not blindly operate on:

```text
/dev/input/event*
```

without understanding hotplug and seat semantics.

---

# 45. Hotplug

The privilege model must account for devices appearing after remote mode begins.

A helper should not automatically grant itself broad access to every future device.

---

# 46. Filesystem Sandbox

Use systemd sandboxing where compatible.

Potential controls include:

```text
ProtectSystem
ProtectHome
PrivateTmp
PrivateDevices
NoNewPrivileges
RestrictSUIDSGID
LockPersonality
RestrictNamespaces
ProtectKernelTunables
ProtectKernelModules
ProtectControlGroups
```

The exact set must be tested against actual runtime requirements.

---

# 47. Do Not Copy Sandbox Flags Blindly

Some GNOME/PipeWire/PAM functionality may legitimately require access that aggressive sandboxing blocks.

Therefore:

1. start restrictive
2. test
3. identify exact failure
4. loosen only the required boundary
5. document the exception

---

# 48. `NoNewPrivileges`

Use `NoNewPrivileges=true` where compatible.

If a service genuinely needs privilege transitions, document why.

---

# 49. Filesystem Access

`remote-hostd` should access only:

- its configuration
- its state directory
- required system authentication interfaces
- required IPC sockets

It should not have broad read access to user home directories.

---

# 50. Gateway Filesystem

The gateway should access only:

- application files
- static browser assets
- gateway configuration
- necessary TLS material
- temporary runtime data

It should not access user home directories.

---

# 51. GNOME Agent Filesystem

The session agent should access:

- its application configuration
- necessary user runtime resources
- GNOME/Mutter/PipeWire interfaces

It should not have arbitrary system administration access.

---

# 52. Private Temporary Files

Use private temporary directories where possible.

Do not place authentication material in `/tmp`.

---

# 53. Runtime Directories

Prefer appropriate:

```text
/run
/run/user/<uid>
```

runtime locations with restrictive permissions.

Do not create world-writable sockets.

---

# 54. Unix Socket Permissions

IPC sockets must use restrictive ownership and permissions.

Example conceptual policy:

```text
remote-gateway socket
    accessible only to remote-gateway + remote-hostd

GNOME agent socket
    accessible only to expected user/service
```

Exact mechanism should use systemd socket activation or another robust method where appropriate.

---

# 55. Socket Activation

Consider systemd socket activation for local services where useful.

Advantages:

- controlled ownership
- controlled permissions
- lifecycle management
- reduced idle process surface

Do not use it solely for aesthetics.

---

# 56. D-Bus Policy

Where D-Bus is required:

- define explicit services
- restrict callers
- restrict methods
- restrict object paths
- avoid broad `own`/`send` permissions

Review the resulting D-Bus policy.

---

# 57. System Bus vs User Bus

Use the appropriate bus.

Potentially:

```text
system operations
→ system D-Bus

GNOME session operations
→ user/session D-Bus
```

Do not expose the user session bus to network-facing components.

---

# 58. Gateway D-Bus Access

Prefer:

```text
remote-gateway
    |
    X
D-Bus
```

The gateway should not need direct D-Bus access.

---

# 59. Host Daemon D-Bus Access

`remote-hostd` may need carefully scoped system services.

Avoid unrestricted D-Bus access.

---

# 60. GNOME Agent D-Bus Access

The GNOME agent will naturally need GNOME/Mutter/session interfaces.

Limit access to expected services.

---

# 61. Network Namespace

Do not put `remote-hostd` into an isolated network namespace if it genuinely needs host networking.

However, the gateway can be isolated from unnecessary interfaces if practical.

---

# 62. Gateway Network Exposure

Only expose required network ports.

Do not bind administrative IPC sockets to:

```text
0.0.0.0
```

unless explicitly required.

---

# 63. Local IPC Must Stay Local

Security-sensitive IPC should use:

```text
Unix domain socket
```

or another authenticated local mechanism.

Do not expose it over TCP merely because TCP is convenient.

---

# 64. Service Account Separation

Prefer:

```text
remote-gateway user
remote-host service user
target GNOME user
```

as separate identities.

Do not run multiple security domains under one account unnecessarily.

---

# 65. Environment Variables

Do not use environment variables as security credentials.

Avoid:

```text
REMOTE_ACCESS_KEY=...
TOTP_SECRET=...
SESSION_TOKEN=...
```

in long-lived service environments.

---

# 66. Secrets in systemd Units

Do not place:

```text
Environment=REMOTE_ACCESS_KEY=...
```

into service files.

Secrets should come from the secure secret-storage mechanism.

---

# 67. Secrets in Command Lines

Never invoke:

```text
program --password=...
```

or:

```text
program --totp=...
```

Command-line arguments may be visible through process inspection.

---

# 68. Secrets in Process Environment

Avoid passing long-lived secrets through environment variables.

If a short-lived secret must be passed to a helper, use a safer IPC mechanism or file descriptor where appropriate.

---

# 69. Logging Configuration

systemd/journald integration is useful.

Configure logs so that sensitive values cannot accidentally be written.

---

# 70. Standard Input/Output

Services should not depend on interactive terminals.

Use structured logging and explicit IPC.

---

# 71. Resource Limits

Apply reasonable limits:

- memory
- CPU
- number of processes
- file descriptors
- network connections
- IPC message sizes

Do not choose limits so low that normal remote sessions fail.

---

# 72. Gateway Resource Limits

The gateway is internet-facing and therefore should have especially strong:

- connection limits
- request limits
- message limits
- memory limits

---

# 73. Host Daemon Resource Limits

`remote-hostd` should have conservative limits but enough resources for:

- authentication
- state management
- signalling
- multiple failed connections

---

# 74. GNOME Agent Resource Limits

Do not impose restrictive memory limits that break:

- PipeWire
- media capture
- Mutter communication

If video encoding runs in another process, isolate that resource separately.

---

# 75. WebRTC Process Isolation

If possible, keep browser/gateway networking separate from system-security logic.

Do not allow WebRTC parsing vulnerabilities to directly compromise the security daemon.

---

# 76. Media Encoder Boundary

If a codec/encoder process is required:

```text
GNOME/PipeWire
      |
      v
media process
      |
      v
WebRTC
```

Consider isolating the media process because media parsers/encoders are complex attack surfaces.

---

# 77. Media Process Privileges

Media processing should be unprivileged.

It must not have:

- root
- input device access
- PAM access
- security database access

---

# 78. Gateway and Media Separation

Where practical:

```text
gateway
media
security daemon
```

should be separate failure/privilege domains.

---

# 79. systemd Restart Storm

Ensure that repeated failures do not create:

```text
start
crash
start
crash
start
crash
```

at high frequency.

Use systemd rate limiting.

---

# 80. Graceful Shutdown

On normal shutdown:

```text
stop accepting connections
        |
        v
revoke remote leases
        |
        v
disable remote input
        |
        v
stop capture
        |
        v
restore physical display
        |
        v
lock if appropriate
        |
        v
terminate services
```

---

# 81. Shutdown Timeout

Do not wait indefinitely for cleanup.

If graceful cleanup exceeds a defined timeout:

```text
force safe recovery
```

---

# 82. SIGTERM

Services should handle SIGTERM gracefully.

Do not treat SIGTERM as equivalent to:

```text
continue remote operation
```

The secure default is to revoke remote control.

---

# 83. SIGKILL

SIGKILL cannot be handled.

Therefore:

> The architecture must remain safe even when a component is killed without cleanup.

This is one reason control leases, systemd supervision, and independent recovery are required.

---

# 84. Host Power Loss

If power disappears:

```text
everything stops
```

On next boot:

```text
old remote session invalid
physical display normal
physical input normal
```

The system must not attempt to resume a previous remote-control session.

---

# 85. Kernel Crash

The application cannot guarantee application-level cleanup after kernel panic.

After reboot, however:

- stale remote sessions must be invalid
- physical display should return to normal hardware behavior
- remote control should require new authentication

---

# 86. systemd Boot Recovery

On startup:

```text
remote-hostd
    |
    v
load persistent security state
    |
    v
verify GNOME environment
    |
    v
reconcile stale state
    |
    v
READY
```

Do not accept remote control before reconciliation.

---

# 87. Emergency Boot State

`remote-emergencyd` should start early enough to be available when the graphical session becomes usable, but not so early that it depends on GNOME components that do not yet exist.

The exact ordering must be tested.

---

# 88. Emergency Shortcut Detection

The implementation must determine the safest way to observe the configured physical emergency shortcut.

Potential mechanisms must be evaluated against:

- Wayland
- GNOME
- libinput
- seat ownership
- hotplug
- lock screen
- session crashes

Do not assume a GNOME keyboard shortcut is sufficient because the GNOME shell may be the failing component.

---

# 89. Emergency Shortcut While Locked

Determine whether the shortcut should work when:

```text
LOCAL_LOCKED
```

and:

```text
REMOTE_ACTIVE
```

The emergency path should remain useful even if GNOME is unresponsive.

---

# 90. Emergency Shortcut Collision

The shortcut must be configurable.

Avoid interfering with common:

- terminal shortcuts
- accessibility shortcuts
- desktop shortcuts
- application shortcuts

---

# 91. Emergency Hold Detection

Use a deliberate hold interval.

Example:

```text
Ctrl + Alt + Shift + F12
held ≥ 2 seconds
```

Do not trigger on a single accidental key event.

---

# 92. Emergency Confirmation

Do not require an on-screen confirmation.

If GNOME is frozen or the physical display is disabled, confirmation may be impossible.

The emergency action should be immediate after the configured hold threshold.

---

# 93. Emergency Feedback

Provide a local feedback mechanism where possible, such as:

- keyboard LED change
- system notification after restoration
- journal event

Do not depend on the remote UI.

---

# 94. Emergency Event Logging

Log:

```text
EMERGENCY_TRIGGERED
EMERGENCY_RECOVERY_STARTED
EMERGENCY_RECOVERY_COMPLETED
EMERGENCY_RECOVERY_PARTIAL
```

Never log the keyboard sequence itself as raw input data.

---

# 95. No Keylogging

The emergency daemon must not become a general keyboard logger.

It should detect only the minimum state needed to recognize the configured emergency sequence.

---

# 96. Input Device Access and Privacy

If raw input access is unavoidable:

- isolate the process
- minimize retained state
- never transmit input
- never log input
- never persist input
- monitor only required devices/events

---

# 97. Emergency Daemon Sandboxing

Apply systemd hardening aggressively, subject to testing.

Potential controls:

```text
NoNewPrivileges=true
PrivateTmp=true
ProtectSystem=strict
ProtectHome=true
RestrictSUIDSGID=true
LockPersonality=true
ProtectKernelModules=true
ProtectKernelTunables=true
ProtectControlGroups=true
```

Device access must be explicitly granted if required.

---

# 98. Device Access Exceptions

If `PrivateDevices=true` prevents required emergency input detection, do not simply disable sandboxing globally.

Instead:

1. identify required device access
2. isolate the helper
3. use the narrowest device permission possible
4. document it
5. test hotplug behavior

---

# 99. `remote-emergencyd` Networking

Do not give it network access unless absolutely unavoidable.

Preferred:

```text
PrivateNetwork=true
```

if compatible with the chosen IPC/recovery mechanism.

---

# 100. Emergency IPC

If emergencyd communicates with `remote-hostd`, use local authenticated IPC.

It should expose only predefined recovery operations.

---

# 101. Emergency IPC Failure

If IPC fails:

```text
emergencyd
   |
   X
remote-hostd
```

it must still perform whatever local safety actions it can.

It should not simply report:

```text
remote-hostd unavailable
```

and stop.

---

# 102. Recovery Escalation

Potential model:

```text
Level 1
remote-hostd recovery

Level 2
direct GNOME/session recovery

Level 3
local display/input recovery

Level 4
remain locked and deny remote control
```

Exact implementation depends on the GNOME PoC.

---

# 103. Fail-Safe State

Define:

```text
FAIL_SAFE
```

as a first-class state.

Properties:

```text
remote input = disabled
remote lease = invalid
new remote control = denied
physical display = being restored
physical input = being restored
GNOME = locked or being locked
```

---

# 104. Fail-Safe Persistence

If necessary, persist:

```text
recovery_required=true
```

so that a restart can continue recovery.

Do not persist sensitive desktop content.

---

# 105. Fail-Safe Startup

If:

```text
recovery_required=true
```

on boot/session startup:

Do not accept remote connections until:

```text
physical console safe
+
security state reconciled
```

---

# 106. systemd Dependency Failure

If one service is unavailable:

```text
gateway unavailable
```

the others should not become insecure.

Examples:

```text
gateway failure
→ no new remote connections

host daemon failure
→ remote control revoked

GNOME agent failure
→ remote control revoked

emergency daemon failure
→ investigate independent fallback
```

---

# 107. Service Isolation Matrix

Target architecture:

| Component | Network | Root | GNOME/D-Bus | Input devices | Secrets |
|---|---|---|---|---|---|
| remote-gateway | Yes | No | No | No | Minimal |
| remote-hostd | Limited/required | Avoid if possible | Narrow | No/limited | Authentication secrets |
| gnome-session-agent | No inbound network | No | GNOME session | Session mechanism | None |
| remote-emergencyd | No | Minimal only if required | Narrow/local | Minimal if required | None |
| media/encoder | Required only as needed | No | No | No | None |

This table is a design target, not permission to grant broad access where a narrower implementation is possible.

---

# 108. Secrets by Process

## remote-gateway

Should not possess:

```text
Linux password
TOTP secret
Remote Access Key
private host key
```

## remote-hostd

May possess/access:

```text
TOTP secret
Access Key verifier
trusted client metadata
host identity
security epoch
```

## GNOME agent

Should possess:

```text
none of the above
```

## emergencyd

Should possess:

```text
none of the above
```

---

# 109. Host Private Key

Keep host identity private key in the security domain.

Do not give it to:

```text
remote-gateway
gnome-session-agent
remote-emergencyd
```

unless a specific cryptographic operation requires it and can be safely isolated.

---

# 110. Authentication Secret Access

The PAM/TOTP/Access Key implementation should live as close as possible to `remote-hostd`.

The network-facing components should forward authentication requests but not own authentication secrets.

---

# 111. Configuration Separation

Separate:

```text
system security configuration
GNOME session configuration
gateway configuration
emergency configuration
```

Do not put everything into one world-readable configuration file.

---

# 112. Configuration Permissions

Security-sensitive configuration/state should be readable only by the required service identity.

For example:

```text
root:remote-host
0600
```

or another appropriately restrictive model.

---

# 113. Configuration Validation

Reject:

- unknown security settings
- invalid paths
- unexpected commands
- unsafe permissions
- malformed capability values

Do not silently ignore security-related configuration errors.

---

# 114. No Arbitrary Executables

Configuration must not contain user-controlled executable paths for security operations unless explicitly designed and validated.

Avoid:

```yaml
emergency_command: "..."
```

or:

```yaml
restore_command: "..."
```

that gets passed to a shell.

---

# 115. No Shell Invocation

Do not use:

```text
system()
popen()
sh -c
bash -c
```

for network-derived or configuration-derived operations.

Prefer direct library/system APIs.

---

# 116. Systemd Commands

If the application needs to control systemd:

Prefer a narrow D-Bus interface or predefined systemd operation.

Do not allow the browser to specify:

```text
unit_name
action
```

arbitrarily.

---

# 117. Privileged Helper Protocol

If a helper is unavoidable:

```text
request:
    operation = RESTORE_DISPLAY
```

not:

```text
request:
    command = "/usr/bin/something --whatever"
```

---

# 118. Helper Allowlist

Helpers should implement a fixed set of operations:

```text
ISOLATE_INPUT
RESTORE_INPUT
RESTORE_DISPLAY
LOCK_SESSION
EMERGENCY_REVOKE
```

No arbitrary operation execution.

---

# 119. IPC Authentication

Even local IPC should not automatically be trusted.

Use:

- Unix socket permissions
- peer credentials
- systemd credentials
- cryptographic authentication where appropriate

depending on the interface.

---

# 120. Peer Identity

Where possible, verify:

```text
UID
GID
PID/peer credentials
service identity
```

before accepting privileged IPC.

Do not trust a client-provided identity field.

---

# 121. IPC Authorization

Even after identifying the caller, authorize each operation.

For example:

```text
gateway
→ may request session creation

gateway
→ may NOT request emergency takeover
```

---

# 122. Emergency Authorization

Emergency operations should have a dedicated authorization path.

Do not expose emergency functions to the browser.

---

# 123. Remote Gateway Compromise

If gateway is compromised:

Expected maximum impact should be approximately:

```text
connection/signalling abuse
+
authentication attempt abuse
+
DoS
```

not:

```text
root compromise
+
GNOME control
+
credential extraction
```

---

# 124. GNOME Agent Compromise

If the GNOME agent is compromised under the user account:

The attacker may have user-level desktop access.

However, the architecture should prevent automatic escalation to:

```text
root
```

or:

```text
security database
```

---

# 125. Host Daemon Compromise

`remote-hostd` is a high-impact component.

Therefore it requires:

- aggressive input validation
- dependency minimization
- restricted privileges
- security-focused testing
- fuzzing
- careful IPC

---

# 126. Emergency Daemon Compromise

Because emergencyd has local privileged access, keep its code extremely small.

Prefer:

```text
small
boring
deterministic
```

over:

```text
feature-rich
```

---

# 127. Codebase Separation

Keep emergency functionality separate from:

- WebRTC
- HTTP
- authentication UI
- media
- browser
- complex business logic

Ideally it should have a tiny dependency graph.

---

# 128. Build Separation

Consider compiling emergencyd as a separate executable/binary.

Benefits:

- smaller attack surface
- easier auditing
- easier privilege analysis
- fewer runtime dependencies

---

# 129. Dependency Separation

The emergency binary should not link against:

```text
WebRTC
browser framework
video codecs
large HTTP stack
```

unless absolutely necessary.

---

# 130. Static Analysis

Run appropriate tools against privileged components.

Examples:

```text
cargo clippy
cargo audit
cargo deny
```

and appropriate C/C++ tooling if any native helpers are used.

---

# 131. Fuzzing Privileged IPC

Fuzz:

```text
remote-hostd IPC
GNOME agent IPC
emergency IPC
```

especially malformed requests.

---

# 132. Service Security Tests

Automate checks that verify:

```text
gateway cannot access secrets
gateway cannot access privileged sockets
GNOME agent cannot access host secrets
emergency daemon has no network
```

---

# 133. Permission Tests

Installation tests should verify:

```text
config permissions
state permissions
socket permissions
service users
device permissions
systemd sandbox settings
```

---

# 134. Service Startup Test

After installation:

```text
reboot
```

Verify:

```text
remote-hostd starts
gateway starts if configured
emergencyd starts
GNOME agent starts with user session
```

---

# 135. Reboot Safety Test

Start remote session.

Reboot host.

Expected after reboot:

```text
old session invalid
old lease invalid
physical outputs normal
physical input normal
remote session unavailable until new authentication
```

---

# 136. Main Daemon Crash Test

During remote session:

```text
kill -9 remote-hostd
```

Expected:

```text
remote control eventually revoked
safe recovery
```

Do not rely on graceful cleanup.

---

# 137. GNOME Agent Crash Test

During remote session:

```text
kill -9 gnome-session-agent
```

Expected:

```text
remote control revoked
safe recovery
```

---

# 138. Gateway Crash Test

During remote session:

```text
kill -9 remote-gateway
```

Expected:

```text
existing session eventually loses lease
remote input revoked
safe recovery
```

Directly connected sessions should still obey the same lease model.

---

# 139. Emergency Test

During remote session:

```text
main daemon healthy
gateway healthy
GNOME agent healthy
```

Trigger emergency.

Expected:

```text
remote control revoked
epoch incremented
GNOME locked
physical display restored
physical input restored
```

---

# 140. Emergency During Main Daemon Failure

During remote session:

```text
kill -9 remote-hostd
```

Then trigger emergency.

Expected:

```text
emergency path remains operational
```

to the maximum extent defined by the architecture.

This is a mandatory acceptance test.

---

# 141. Emergency During GNOME Agent Failure

Kill the GNOME agent.

Trigger emergency.

Expected:

```text
remote control already unavailable or revoked
emergency daemon remains responsive
physical recovery proceeds where technically possible
```

Document any GNOME operation that cannot be performed without a healthy session agent.

---

# 142. Input Isolation Failure

Simulate inability to isolate physical input.

Expected:

```text
REMOTE_ACTIVE = FORBIDDEN
```

The system must not "try anyway."

---

# 143. Display Isolation Failure

Simulate inability to disable physical output.

Expected:

```text
REMOTE_ACTIVE = FORBIDDEN
```

---

# 144. Restoration Failure

Simulate display restoration failure.

Expected:

```text
remote input remains disabled
session remains safe/locked
recovery continues/retries
```

Do not return to unrestricted remote control.

---

# 145. systemd Sandbox Regression

Every update to service sandbox settings should run the integration test suite.

A seemingly harmless setting such as:

```text
ProtectHome=true
```

can break legitimate operation.

Do not weaken sandboxing permanently just to make tests pass.

---

# 146. Packaging

The package should install:

```text
systemd units
configuration
executables
GNOME session agent
emergency daemon
```

with correct ownership and permissions.

---

# 147. Installation Security

Installation must not:

- overwrite unrelated system configuration
- replace GNOME configuration unexpectedly
- disable firewall security
- modify PAM broadly without explicit configuration
- enable remote access silently

---

# 148. Uninstallation

Uninstall should:

1. stop services
2. terminate remote sessions
3. restore physical display/input if necessary
4. remove service units
5. remove application state according to user choice
6. avoid deleting unrelated GNOME configuration

---

# 149. Upgrade

During upgrade:

- do not leave old and new daemons simultaneously active
- invalidate incompatible sessions
- preserve required security configuration
- preserve host identity
- preserve TOTP unless explicitly reset
- preserve trusted clients where compatible

---

# 150. Upgrade Safety

If an upgrade changes the GNOME integration:

```text
REMOTE_ACTIVE
```

should not survive an incompatible component upgrade.

Prefer:

```text
terminate
+
restore
+
lock
```

before upgrading critical components.

---

# 151. Package Rollback

If an upgrade fails:

The host must return to a safe local state.

Do not preserve remote control across an uncertain software version transition.

---

# 152. Service Version Compatibility

`remote-hostd` and `gnome-session-agent` should negotiate protocol versions.

Example:

```text
hostd protocol = 3
agent protocol = 2
```

Expected:

```text
incompatible
→ remote mode disabled
```

not:

```text
guess behavior
```

---

# 153. IPC Protocol Version

Every internal IPC protocol should include:

```text
protocol_version
message_type
request_id
```

Unknown versions should fail safely.

---

# 154. State Version

Persisted state should be versioned.

Example:

```text
state_version: 1
```

On upgrade:

- migrate explicitly
- validate migrated state
- do not silently reinterpret old security state

---

# 155. Security Epoch Migration

When migrating persisted state:

The security epoch must never decrease.

If uncertain:

```text
increment epoch
invalidate active sessions
```

---

# 156. Final Privilege Matrix

The intended architecture is:

```text
                         PRIVILEGE

Least
  |
  v
+----------------------------+
| remote-gateway             |
| Internet-facing            |
| unprivileged               |
+----------------------------+
             |
             v
+----------------------------+
| media/encoder              |
| unprivileged               |
+----------------------------+
             |
             v
+----------------------------+
| gnome-session-agent        |
| normal GNOME user          |
+----------------------------+
             |
             v
+----------------------------+
| remote-hostd               |
| narrowly privileged        |
| system security domain     |
+----------------------------+
             |
             v
+----------------------------+
| emergencyd                 |
| minimal exceptional        |
| local privilege            |
+----------------------------+
```

The ordering is conceptual, not a claim that every component has a strictly increasing privilege level.

---

# 157. Final Service Architecture

```text
                         systemd
                            |
            +---------------+---------------+
            |               |               |
            v               v               v
     remote-gateway   remote-hostd   remote-emergencyd
     unprivileged      security         emergency
                            |
                            | IPC
                            v
                   GNOME user session
                            |
                            v
                  gnome-session-agent
                            |
             +--------------+--------------+
             |              |              |
             v              v              v
          Mutter        PipeWire         libei
```

---

# 158. Critical Failure Paths

## Gateway fails

```text
gateway crash
    ↓
existing lease eventually expires
    ↓
remote input disabled
    ↓
safe recovery
```

## Host daemon fails

```text
hostd crash
    ↓
leases invalidated / expire
    ↓
remote input disabled
    ↓
safe recovery
```

## GNOME agent fails

```text
agent crash
    ↓
remote state invalid
    ↓
remote input disabled
    ↓
safe recovery
```

## Emergency daemon triggered

```text
hotkey
    ↓
immediate remote revocation
    ↓
epoch increment
    ↓
lock
    ↓
display/input restore
```

---

# 159. Hard Security Requirements

The implementation must satisfy:

```text
[ ] Entire application is NOT root
[ ] Gateway is unprivileged
[ ] GNOME agent runs as target user
[ ] Emergency daemon is minimal
[ ] No arbitrary privileged command execution
[ ] No arbitrary D-Bus proxy
[ ] IPC is authenticated
[ ] IPC is authorization-controlled
[ ] Secrets are isolated
[ ] Linux password is never persisted
[ ] TOTP secret is not exposed to gateway/agent
[ ] Access Key is not exposed to gateway/agent
[ ] Network-facing component has no input-device access
[ ] Privileged helpers are narrowly scoped
[ ] systemd watchdog is configured appropriately
[ ] restart loops are controlled
[ ] stale sessions cannot survive restart
[ ] security epoch survives restart
[ ] service sandboxing is tested
[ ] emergency path does not depend on Internet
[ ] emergency path does not depend on browser
[ ] emergency path does not depend on WebRTC
```

---

# 160. Final Copilot Agent Instruction

Before implementing systemd/service architecture:

1. inspect the repository
2. inspect the configuration generated by `adaptive-workflow-configurator`
3. read the previous architecture/security/GNOME documents
4. inspect the current Ubuntu 26.04 systemd behavior
5. inspect actual PAM requirements
6. inspect GNOME user-session lifecycle
7. inspect PipeWire/libei runtime permissions
8. determine which operations truly require elevated privilege
9. minimize privileges before writing service files
10. document every privilege exception

Do not start by writing permissive `root` systemd units and hardening them later.

Start from:

```text
NO PRIVILEGE
```

and add only what is demonstrably required.

For every privileged operation, document:

```text
operation
component
required privilege
reason
attack impact
alternative considered
```

The implementation must not use:

```text
root
+
arbitrary shell
+
arbitrary D-Bus
```

as a shortcut.

---

# 161. Most Important Architectural Rule

The system must preserve this separation:

```text
NETWORK
    ↓
remote-gateway

SECURITY
    ↓
remote-hostd

DESKTOP
    ↓
gnome-session-agent

EMERGENCY
    ↓
remote-emergencyd
```

A compromise or crash in one domain must not automatically compromise the others.

---

# 162. Final Safety Principle

The entire system should be designed around:

> **The component with the most dangerous privilege should contain the least functionality possible.**

Therefore:

```text
remote-emergencyd
    = tiny

privileged helpers
    = tiny

remote-hostd
    = security-focused

gnome-session-agent
    = GNOME-focused

remote-gateway
    = network-focused

browser
    = UI-focused
```

Do not create a single "god process" that handles networking, authentication, GNOME, input, display, and system administration.

---

# 163. Completion Criteria

The systemd/privilege implementation is complete only when:

```text
[ ] Services install correctly
[ ] Services start correctly
[ ] GNOME agent starts with user session
[ ] Gateway runs unprivileged
[ ] Host daemon has minimum required privilege
[ ] Emergency daemon has minimum required privilege
[ ] Service sandboxing passes integration tests
[ ] IPC permissions are verified
[ ] D-Bus permissions are verified
[ ] Secrets are isolated
[ ] Daemon crash recovery works
[ ] Gateway crash recovery works
[ ] GNOME agent crash recovery works
[ ] Emergency takeover works
[ ] Reboot invalidates stale sessions
[ ] Security epoch persists correctly
[ ] Display state recovers
[ ] Input state recovers
[ ] No arbitrary command execution exists
[ ] No unrestricted D-Bus proxy exists
[ ] Privileged helper code has been separately reviewed
```

Do not mark the architecture complete merely because all systemd units successfully start.

The real acceptance criterion is:

```text
REMOTE FAILURE
      ↓
REMOTE AUTHORITY REVOKED
      ↓
LOCAL CONSOLE RECOVERED
      ↓
SYSTEM LOCKED
```

without requiring cooperation from the remote client.