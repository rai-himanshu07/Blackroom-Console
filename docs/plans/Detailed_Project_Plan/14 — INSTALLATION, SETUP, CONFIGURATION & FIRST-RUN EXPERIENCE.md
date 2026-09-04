# 14 — INSTALLATION, SETUP, CONFIGURATION & FIRST-RUN EXPERIENCE

## 1. Purpose

This document defines how the project is installed, configured, initialized, upgraded, and verified on a supported workstation.

The setup experience must make the system:

- easy to install
- explicit about privileges
- secure by default
- understandable to a technically competent Linux user
- recoverable if setup fails
- safe before the first remote connection

The first-run experience must not silently enable remote access before authentication and safety mechanisms are configured.

The core principle is:

> **A workstation must never become remotely controllable merely because the software was installed.**

Installation and remote-access enablement are separate stages.

---

# 2. Supported Initial Environment

The initial release targets only:

```text
Ubuntu 26.04 LTS
Ubuntu Desktop
GNOME 50+
Wayland
systemd
Single-user workstation
```

The installer/setup system must explicitly detect:

- Ubuntu version
- desktop environment
- GNOME version
- Wayland
- systemd
- current user
- session state
- required runtime dependencies
- GPU/graphics environment
- required GNOME interfaces

Do not claim compatibility with unsupported environments.

---

# 3. Installation Lifecycle

The overall lifecycle should be:

```text
INSTALL
   ↓
ENVIRONMENT CHECK
   ↓
SERVICE INSTALLATION
   ↓
CONFIGURATION INITIALIZATION
   ↓
SECURITY SETUP
   ↓
GNOME SESSION VALIDATION
   ↓
EMERGENCY CONTROLLER VALIDATION
   ↓
NETWORK/TLS SETUP
   ↓
SELF-TEST
   ↓
EXPLICIT ENABLE REMOTE ACCESS
   ↓
READY
```

Installation alone should result in:

```text
REMOTE ACCESS = DISABLED
```

until setup is completed.

---

# 4. Installation Principles

The installer must:

1. Minimize privileges.
2. Explain every privileged operation.
3. Avoid running the complete application as root.
4. Create only required system users/groups.
5. Install only required files.
6. Set restrictive permissions.
7. Install systemd units safely.
8. Validate configuration.
9. Start only required services.
10. Avoid opening network access unnecessarily.
11. Never generate or display secrets unnecessarily.
12. Provide a clean uninstall path.
13. Preserve user data only when appropriate.
14. Fail safely if installation is interrupted.

---

# 5. Installation Components

Installation may provision:

```text
remote-hostd
remote-gateway
remote-emergencyd
gnome-session-agent
```

and required supporting resources.

The exact filesystem/package layout must follow the repository/package architecture established by the project.

Do not force a directory structure merely because this document names components.

The agent must first inspect the workflow and packaging conventions established by `adaptive-workflow-configurator`.

---

# 6. Privilege Requirements

Installation may require administrator privileges.

Runtime privileges should remain separated.

Target model:

```text
INSTALLER
    |
    +-- creates system services
    +-- creates restricted service identities
    +-- installs configuration
    +-- installs permissions
    |
    +-- does NOT run remote desktop stack as root
```

Runtime:

```text
remote-gateway
    UNPRIVILEGED

remote-hostd
    NARROW SYSTEM PRIVILEGE

gnome-session-agent
    NORMAL USER SESSION

remote-emergencyd
    MINIMAL PRIVILEGED SERVICE
```

---

# 7. Preflight Environment Check

Before modifying the system, run a preflight check.

Report:

```text
Platform:
    Ubuntu 26.04 LTS       PASS

Desktop:
    GNOME                  PASS

GNOME Version:
    50.x                   PASS

Session:
    Wayland                PASS

systemd:
    Available              PASS

User Session:
    Active                 PASS

Mutter:
    Required interfaces    PASS/FAIL

PipeWire:
    Available              PASS/FAIL

libei/EIS:
    Available              PASS/FAIL

Graphics:
    Detected               PASS

Overall:
    INSTALLABLE / UNSUPPORTED
```

The check should distinguish:

- required dependency missing
- optional capability missing
- unknown capability
- unsupported environment

---

# 8. Do Not Automatically Repair Everything

The installer should not aggressively modify the workstation.

For example, it should not automatically:

- change the user's firewall policy
- replace GPU drivers
- change GNOME settings unrelated to the application
- disable security software
- change display configuration
- alter login behavior unnecessarily
- modify unrelated systemd services

If a dependency is missing, explain what is required.

Where automatic installation is safe and conventional, it may be offered explicitly.

---

# 9. Installation Modes

Support conceptually:

## Interactive Installation

For normal users.

Provides:

- preflight
- explanations
- confirmation
- setup wizard
- final verification

## Automated Installation

For administrators/developers.

Must support:

- non-interactive installation
- explicit configuration
- predictable exit codes
- no secret leakage
- no interactive assumptions

## Development Installation

May install development/test components, but must not silently enable production remote access.

---

# 10. First-Run State

Immediately after installation:

```text
Installation:
    COMPLETE

Remote Access:
    DISABLED

Authentication:
    NOT CONFIGURED

TOTP:
    NOT CONFIGURED

Remote Access Key:
    NOT CONFIGURED

Trusted Devices:
    NONE

Emergency Controller:
    INSTALLED / NOT VERIFIED

TLS:
    NOT CONFIGURED

Host:
    NOT READY
```

The system should clearly communicate this state.

---

# 11. First-Run Wizard

The first-run wizard should guide the user through:

```text
1. Environment verification
2. Host identity
3. Authentication setup
4. TOTP setup
5. Remote Access Key generation
6. Recovery-code setup
7. Emergency controller setup
8. Network/TLS setup
9. Optional trusted-device setup
10. Safety self-test
11. Explicit remote-access enablement
```

The exact UI technology should follow the project architecture.

A simple native Linux UI or CLI is acceptable for initial releases.

Do not make a heavyweight desktop application a prerequisite for the core host.

---

# 12. Host Identity

Generate a cryptographic host identity during setup.

The identity must be independent of:

- IP address
- hostname
- MAC address
- username

The host identity should remain stable across:

- IP changes
- DHCP changes
- hostname changes

unless the user explicitly resets/regenerates it.

---

# 13. Host Identity Display

Show a human-friendly fingerprint.

Example:

```text
Host:
    Workstation

Host ID:
    rc_8f21...

Fingerprint:
    AB73-19D4-...
```

The fingerprint is for recognition.

Do not treat a short fingerprint as the actual authentication secret.

---

# 14. Authentication Setup

The setup wizard must establish the mandatory authentication model.

Every remote session requires:

```text
Linux username
+
Linux password
+
TOTP
```

New/untrusted devices additionally require:

```text
Remote Access Key
```

Trusted devices use:

```text
Linux username
+
Linux password
+
TOTP
+
trusted device credential
```

TOTP remains mandatory for trusted devices.

---

# 15. TOTP Setup

The wizard should:

1. Generate a cryptographically secure TOTP secret.
2. Display a QR code.
3. Display the manual setup key if necessary.
4. Tell the user to add it to a compatible authenticator.
5. Ask for a current TOTP code.
6. Verify the code.
7. Confirm successful enrollment.

The TOTP secret must never be sent over the remote network during normal operation.

---

# 16. TOTP Setup Failure

If verification fails:

```text
TOTP setup
    ↓
verification failed
    ↓
retry
```

Do not enable remote access.

The user should be able to restart TOTP enrollment safely.

---

# 17. TOTP Secret Protection

The TOTP secret must be protected using an appropriate local secret-storage mechanism.

At minimum:

- restrictive file permissions
- service identity separation
- no world-readable configuration
- no normal log output
- no browser exposure

If a stronger system secret store is used, document its dependency and recovery implications.

---

# 18. Recovery Codes

Generate recovery codes during first-run setup.

Recovery codes should be:

- cryptographically random
- single-use
- stored securely
- displayed only during setup
- explicitly acknowledged by the user
- invalidated after use

Suggested flow:

```text
Normal:
    password + TOTP

Authenticator lost:
    password + recovery code + Remote Access Key
```

A recovery code must not silently become a permanent TOTP replacement.

---

# 19. Remote Access Key

Generate the Remote Access Key automatically.

Do not ask the user to invent one.

Prefer at least:

```text
256 bits of cryptographic entropy
```

The UI should communicate:

> This key is an additional credential required for new/untrusted devices.

The key should be:

- independently revocable
- rotatable
- treated as a secret
- stored securely
- shown only when explicitly requested
- excluded from logs

---

# 20. Remote Access Key Backup

During setup, provide a clear instruction:

```text
Save your Remote Access Key in a password manager.
```

The application should not encourage users to store it in:

- screenshots
- plain text notes
- browser localStorage
- URLs
- source code

The setup wizard should not repeatedly display the secret.

---

# 21. Access-Key Rotation

Provide an explicit rotation operation.

When rotated:

```text
old key
    ↓
INVALID
```

New devices must use the new key.

Define whether existing trusted-device credentials remain valid.

The preferred model is:

- Remote Access Key rotation invalidates the old Access Key.
- Existing trusted devices remain trusted unless explicitly revoked.
- Active sessions continue only until their existing authorization/lease expires, unless policy requires immediate revocation.
- Provide an explicit “rotate and revoke all” security operation.

---

# 22. Trusted Device Setup

Trusted devices are optional.

After initial authentication from a browser:

```text
Register this device as trusted?
```

If accepted:

- generate device credential
- bind it to host identity
- store it using browser/platform-protected storage where possible
- register it on the host
- show device name
- record registration timestamp

Trusted status must never bypass TOTP.

---

# 23. Trusted Device Management

Provide:

```text
Trusted Devices

1. Laptop
   Added: <date>
   Last Used: <date>
   Status: Active

2. Tablet
   Added: <date>
   Last Used: <date>
   Status: Active
```

Actions:

```text
Revoke
Rename
Revoke All
```

Revocation must invalidate the device credential.

---

# 24. Emergency Controller Setup

The emergency controller is a safety-critical feature.

Setup should verify:

```text
remote-emergencyd:
    installed

service:
    running

input monitoring:
    available

emergency shortcut:
    configured

shortcut:
    Ctrl + Alt + Shift + F12

hold duration:
    2 seconds

test:
    REQUIRED
```

The exact shortcut and hold duration should be configurable.

---

# 25. Emergency Shortcut Safety

The shortcut should avoid accidental activation.

Prefer:

- multi-key combination
- deliberate hold duration
- configurable shortcut
- no single-key trigger

A normal short press should not trigger emergency recovery.

The shortcut should remain functional even when:

- browser is closed
- network is unavailable
- gateway is down
- host daemon is unhealthy
- remote client is disconnected

---

# 26. Emergency Test

During setup, the user should explicitly test the emergency mechanism.

The test must clearly warn:

> This test will lock the workstation and may temporarily change the display/input state.

After activation:

```text
remote authority:
    REVOKED

security epoch:
    INCREMENTED

session:
    LOCKED

physical display:
    RESTORED

physical input:
    RESTORED
```

The user then performs normal GNOME unlock.

---

# 27. Emergency Failure During Setup

If the emergency controller cannot be verified:

```text
Remote Access:
    MUST REMAIN DISABLED
```

The setup wizard must not declare the workstation ready.

This is a hard safety dependency.

---

# 28. Network Setup

Support at least:

```text
LAN
Direct IP
IPv4
IPv6
```

and eventually:

```text
mDNS discovery
Rendezvous
STUN
TURN
NAT traversal
```

Network setup must not assume a static IP.

---

# 29. LAN Mode

LAN mode should support direct workstation access where possible.

Conceptually:

```text
Laptop
   |
LAN
   |
Workstation
```

The user should be able to discover the host using:

- known host identity
- mDNS where enabled
- direct address

Do not use hostname/IP as the cryptographic identity.

---

# 30. Internet Access

For remote Internet access:

```text
Browser
   |
HTTPS / WebSocket
   |
Gateway / Rendezvous
   |
WebRTC
   |
Workstation
```

The networking architecture should prefer direct peer connectivity where possible and use relay infrastructure when required.

The setup wizard should make the distinction clear:

```text
LAN:
    Direct access may be sufficient.

Internet:
    NAT traversal / rendezvous / relay may be required.
```

---

# 31. TLS

The gateway must use HTTPS.

The setup process must clearly establish:

- certificate configuration
- hostname/domain where applicable
- certificate validity
- private-key permissions
- renewal strategy

Never silently downgrade authentication to plain HTTP for convenience.

Development environments may use explicit test certificates.

Production configuration must be clearly distinguished.

---

# 32. Firewall

The installer should detect potential firewall blocking.

Example:

```text
Gateway:
    Listening

Firewall:
    May block required traffic
```

It may provide instructions or an explicit configuration option.

It must not silently disable the firewall.

Any automatically created firewall rule must be:

- minimal
- documented
- reversible

---

# 33. Remote Access Enablement

Remote access should be enabled only after all mandatory safety checks pass.

Conceptually:

```text
Environment:
    PASS

Authentication:
    PASS

TOTP:
    PASS

Remote Access Key:
    PASS

Emergency Controller:
    PASS

GNOME Capability:
    PASS

Display Safety:
    PASS

Input Safety:
    PASS

Recovery Test:
    PASS

--------------------------------
ENABLE REMOTE ACCESS
--------------------------------
```

The final enable operation should require explicit user confirmation.

---

# 34. Readiness State

Once setup is complete:

```text
Remote Access:
    ENABLED

Authentication:
    READY

TOTP:
    ENABLED

Remote Access Key:
    CONFIGURED

Emergency Controller:
    VERIFIED

GNOME:
    SUPPORTED

Display Isolation:
    VERIFIED

Input Isolation:
    VERIFIED

Recovery:
    VERIFIED

Overall:
    READY
```

---

# 35. First Remote Connection

The first remote connection should deliberately use the full new-device authentication flow.

Example:

```text
Browser
   ↓
Host Identity
   ↓
Username
   ↓
Password
   ↓
TOTP
   ↓
Remote Access Key
   ↓
Host authorization
   ↓
Session credential
   ↓
Control lease
   ↓
GNOME preparation
   ↓
Remote session
```

Do not automatically trust the first browser.

---

# 36. First Connection Safety Check

Before activating the remote session, the host should verify:

```text
GNOME session exists
Mutter capabilities available
virtual monitor possible
physical display isolation possible
physical input isolation possible
emergency controller healthy
```

If any critical check fails:

```text
REMOTE ACCESS DENIED
```

The local session should remain safe.

---

# 37. First Remote Session UX

The browser should communicate progress.

Example:

```text
Authenticated

Preparing workstation...

Creating remote display...

Securing physical display...

Securing physical input...

Establishing remote media...

Remote session ready.
```

Do not expose implementation-specific D-Bus or internal error details to ordinary users.

---

# 38. Configuration Model

Configuration should separate:

## Security

```text
authentication
TOTP
Remote Access Key
trusted devices
recovery policy
session policy
lease policy
```

## Remote Session

```text
resolution
frame rate
cursor
input policy
display policy
```

## Network

```text
gateway
TLS
LAN
mDNS
rendezvous
STUN
TURN
```

## Safety

```text
emergency shortcut
hold duration
disconnect behavior
suspend policy
```

## Diagnostics

```text
log level
retention
diagnostic settings
```

Avoid mixing secrets and ordinary preferences in the same storage mechanism if the architecture allows separation.

---

# 39. Safe Defaults

Default behavior should favor safety.

Recommended defaults:

```text
Remote Access:
    DISABLED until setup complete

TOTP:
    REQUIRED

Remote Access Key:
    REQUIRED for new devices

Trusted Device:
    OPTIONAL

Control Lease:
    SHORT-LIVED

Emergency Controller:
    ENABLED

Disconnect:
    LOCK

Network failure:
    FAIL CLOSED

Physical display:
    RESTORE

Physical input:
    RESTORE

Auto-unlock:
    NEVER

Suspend during remote session:
    INHIBITED by default if required for session stability

Clipboard:
    DISABLED initially unless explicitly implemented and secured
```

---

# 40. Disconnect Policy

The default disconnect policy must be:

```text
REMOTE DISCONNECT
      ↓
REVOKE REMOTE AUTHORITY
      ↓
LOCK GNOME
      ↓
RESTORE PHYSICAL DISPLAY
      ↓
RESTORE PHYSICAL INPUT
      ↓
LOCAL_LOCKED
```

There must be no default auto-unlock.

---

# 41. Reboot Behavior

After reboot:

```text
remote sessions:
    INVALID

control leases:
    INVALID

previous browser sessions:
    INVALID

physical console:
    normal GNOME login/unlock flow

remote access:
    AVAILABLE ONLY AFTER NORMAL AUTHENTICATION
```

Do not automatically resume remote control.

---

# 42. Upgrade Behavior

Package upgrades must be designed so that:

- services stop safely
- active remote sessions are terminated safely
- display is restored
- input is restored
- GNOME is locked
- stale control leases become invalid
- services restart only after configuration validation

Never upgrade the remote stack while leaving an uncertain remote-control state.

---

# 43. Configuration Migration

Configuration formats may evolve.

For every upgrade:

```text
old config
    ↓
validate
    ↓
migrate
    ↓
validate new config
    ↓
activate
```

If migration fails:

```text
REMOTE ACCESS
    DISABLED
```

Preserve a recoverable backup where appropriate.

Never silently discard security configuration.

---

# 44. Uninstallation

Uninstallation must clearly distinguish:

```text
Remove software
```

from:

```text
Remove configuration and credentials
```

Before removing:

- terminate remote sessions
- revoke authority
- lock GNOME
- restore physical display
- restore physical input
- stop services
- disable service startup

Then remove application components.

Sensitive credentials should be deleted securely according to the storage mechanism's guarantees.

---

# 45. Uninstall Safety

If uninstallation fails halfway:

- remote control must not remain active indefinitely
- system should remain in the safest achievable state
- services should not continue with missing dependencies
- display/input configuration must be recoverable

The uninstall process must not assume every component is healthy.

---

# 46. Recovery From Broken Installation

Provide a recovery path.

For example:

```text
diagnose
repair
disable
reset
uninstall
```

The exact commands should follow the project's CLI conventions.

The most important emergency operation is:

```text
disable remote access
```

which should revoke remote authority and prevent new remote sessions.

---

# 47. Configuration Reset

A reset operation should distinguish:

## Soft Reset

Reset operational configuration while preserving identity/credentials where appropriate.

## Security Reset

Invalidate:

- active sessions
- leases
- trusted devices
- Remote Access Key where selected
- security epoch

## Full Reset

Return the application to an unconfigured state.

A full reset should require explicit confirmation.

---

# 48. Host Recovery Command

There should be a local recovery mechanism equivalent to:

```text
disable remote access
```

It should:

1. stop accepting new remote sessions
2. revoke active remote authority
3. increment security epoch
4. terminate active sessions
5. lock GNOME
6. restore physical display
7. restore physical input
8. verify safe state

This is separate from the physical emergency shortcut but provides another recovery path.

---

# 49. Setup Diagnostics

At the end of setup, produce a concise report:

```text
Remote Console Setup

Platform:
    PASS

GNOME:
    PASS

Wayland:
    PASS

Mutter:
    PASS

PipeWire:
    PASS

Remote Input:
    PASS

Physical Input Isolation:
    PASS

Physical Display Isolation:
    PASS

Emergency Controller:
    PASS

Authentication:
    PASS

TOTP:
    PASS

Remote Access Key:
    PASS

TLS:
    PASS

Recovery:
    PASS

Remote Access:
    ENABLED

Overall:
    READY
```

Do not report READY if a critical safety gate is unknown.

---

# 50. Setup Failure States

Setup should explicitly identify:

```text
NOT_CONFIGURED
CONFIGURATION_REQUIRED
UNSUPPORTED
PARTIALLY_CONFIGURED
SAFETY_CHECK_FAILED
READY
REMOTE_ACCESS_DISABLED
```

Avoid vague states such as:

```text
ERROR
```

without a meaningful explanation.

---

# 51. Configuration Security

Configuration files must use restrictive permissions.

Security-sensitive configuration should be isolated from:

- ordinary users
- gateway
- browser
- GNOME agent unless specifically required

The GNOME agent should not receive authentication secrets merely because it participates in the session lifecycle.

---

# 52. No Secret Passing Through Environment Variables

Do not use environment variables as the normal long-term storage mechanism for:

- Remote Access Key
- TOTP secret
- session credentials
- trusted-device secrets

Environment variables can accidentally leak through diagnostics/process inspection.

Use appropriate protected storage and authenticated IPC.

---

# 53. No Secrets in Command-Line Arguments

Do not support patterns such as:

```text
remote-tool --access-key SECRET
```

for normal operation.

Command-line arguments may appear in process listings or diagnostic tools.

Use protected input mechanisms where secret entry is necessary.

---

# 54. Setup Audit Trail

Record security-relevant setup events:

```text
installation completed
TOTP configured
Remote Access Key generated
recovery codes generated
trusted device registered
emergency controller verified
remote access enabled
remote access disabled
security reset performed
```

Never record the actual secret values.

---

# 55. Installer Exit Codes

Non-interactive installation should use predictable exit codes.

At minimum distinguish:

```text
SUCCESS
UNSUPPORTED_PLATFORM
MISSING_DEPENDENCY
PERMISSION_ERROR
CONFIGURATION_ERROR
SECURITY_SETUP_REQUIRED
SAFETY_CHECK_FAILED
INSTALLATION_FAILED
```

The exact numeric values should follow repository conventions.

---

# 56. First-Run Acceptance Test

A clean supported machine must pass:

```text
1. Install package.

2. Verify remote access is initially disabled.

3. Run preflight.

4. Complete TOTP setup.

5. Generate Remote Access Key.

6. Generate recovery codes.

7. Configure emergency controller.

8. Verify emergency controller.

9. Configure TLS/networking.

10. Run capability checks.

11. Run display/input safety tests.

12. Enable remote access explicitly.

13. Connect from a new browser.

14. Authenticate with:
       username
       password
       TOTP
       Remote Access Key

15. Register browser as trusted if desired.

16. Establish remote session.

17. Verify remote input.

18. Verify physical input isolation.

19. Verify physical display isolation.

20. Disconnect.

21. Verify:
       authority revoked
       session locked
       display restored
       input restored

22. Reconnect.

23. Trigger emergency takeover.

24. Verify stale session cannot reconnect.

25. Unlock locally.

26. Verify original GNOME session remains intact.
```

This is a full first-run acceptance test.

---

# 57. Setup Security Gates

Remote access must remain disabled unless all applicable gates pass:

```text
GATE A:
Supported platform

GATE B:
Valid GNOME session

GATE C:
Authentication configured

GATE D:
TOTP verified

GATE E:
Remote Access Key configured

GATE F:
Emergency controller verified

GATE G:
Virtual display capability verified

GATE H:
Physical display isolation verified

GATE I:
Remote input verified

GATE J:
Physical input isolation verified

GATE K:
Recovery verified

GATE L:
Configuration valid
```

Any blocker:

```text
REMOTE ACCESS = DISABLED
```

---

# 58. Copilot Agent Instructions

When implementing installation/setup:

1. Inspect the repository first.
2. Inspect the workflow/configuration created by `adaptive-workflow-configurator`.
3. Follow its package, build, configuration, test, and release conventions.
4. Do not replace or duplicate its workflow.
5. Do not impose a directory layout merely because this document describes components.
6. Identify the minimum required privileged installation operations.
7. Keep runtime privilege separation intact.
8. Make installation idempotent.
9. Make configuration migration explicit.
10. Make uninstall safe.
11. Make remote-access enablement explicit.
12. Ensure a fresh install cannot accidentally expose remote control.
13. Add automated tests for configuration and installer logic.
14. Add system tests for actual service installation.
15. Test interrupted installation and partial configuration.
16. Test upgrade and rollback paths.
17. Never place secrets in logs, command-line arguments, or unsafe configuration.
18. Do not silently modify unrelated system configuration.

Implementation sequence:

```text
RESEARCH
    ↓
PREFLIGHT DETECTION
    ↓
PACKAGE/SERVICE INSTALLATION
    ↓
CONFIGURATION MODEL
    ↓
SECURITY INITIALIZATION
    ↓
EMERGENCY CONTROLLER
    ↓
GNOME CAPABILITY VALIDATION
    ↓
NETWORK/TLS SETUP
    ↓
SELF-TEST
    ↓
EXPLICIT ENABLE
    ↓
FIRST REMOTE CONNECTION
    ↓
UPGRADE/UNINSTALL TESTING
```

---

# 59. Definition of Done

Installation and setup are complete only when:

```text
[ ] Fresh installation works
[ ] Unsupported platforms are rejected
[ ] Installation is idempotent
[ ] Remote access starts disabled
[ ] Required services are correctly installed
[ ] Runtime privileges are separated
[ ] Authentication setup works
[ ] TOTP setup works
[ ] Recovery codes work
[ ] Remote Access Key generation works
[ ] Access-key rotation works
[ ] Trusted-device setup works
[ ] Trusted-device revocation works
[ ] Emergency controller is verified
[ ] GNOME capability detection works
[ ] Display safety checks work
[ ] Input safety checks work
[ ] TLS setup works
[ ] Network configuration works
[ ] Explicit enablement works
[ ] First remote connection works
[ ] Disconnect recovery works
[ ] Emergency recovery works
[ ] Upgrade path works
[ ] Configuration migration works
[ ] Uninstall is safe
[ ] Broken-install recovery works
[ ] No secrets leak
[ ] Diagnostics are available
```

---

# 60. Final Principle

Installation should create the software.

Setup should establish trust.

Verification should establish safety.

Only then should remote access become available.

The desired lifecycle is:

```text
INSTALL
   ↓
CONFIGURE
   ↓
VERIFY
   ↓
EXPLICITLY ENABLE
   ↓
REMOTE ACCESS
```

Never:

```text
INSTALL
   ↓
REMOTE ACCESS AVAILABLE
```

The most important first-run invariant is:

> **A freshly installed workstation must be safer than a partially configured workstation, never more remotely exposed.**