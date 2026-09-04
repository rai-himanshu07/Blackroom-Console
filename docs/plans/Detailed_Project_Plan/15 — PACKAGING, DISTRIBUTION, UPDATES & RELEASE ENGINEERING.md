# 15 — PACKAGING, DISTRIBUTION, UPDATES & RELEASE ENGINEERING

## 1. Purpose

This document defines how the project is packaged, distributed, upgraded, downgraded, verified, and released.

The packaging system is part of the security architecture because this project installs:

- system services
- privileged components
- a GNOME session agent
- an emergency recovery daemon
- authentication configuration
- networking components
- browser-facing services

A broken package upgrade must never leave the workstation in an uncertain remote-control state.

The primary principle is:

> **Every installation, upgrade, downgrade, or uninstall operation must preserve the project's fail-safe security invariants.**

---

# 2. Initial Distribution Target

The first release should target:

```text
Ubuntu 26.04 LTS
Ubuntu Desktop
GNOME 50+
Wayland
systemd
x86_64 initially
```

Architecture support should be expanded only after actual testing.

Do not claim support for:

- arbitrary Debian versions
- arbitrary Ubuntu releases
- KDE
- wlroots
- X11
- unsupported architectures

without dedicated validation.

---

# 3. Preferred Packaging Strategy

The initial production distribution should preferably use a native Debian/Ubuntu package.

Primary artifact:

```text
.deb
```

Potential future distribution mechanisms:

```text
APT repository
Ubuntu PPA
Signed release repository
Containerized development environment
Source distribution
```

Do not make containers the primary host deployment mechanism because the project directly integrates with:

- GNOME
- Mutter
- PipeWire
- systemd
- physical displays
- local input

---

# 4. Package Responsibilities

The package should install only what is required.

Conceptually:

```text id="pkgx01"
Package
 |
 +-- remote-hostd
 +-- remote-gateway
 +-- remote-emergencyd
 +-- GNOME session agent
 +-- systemd units
 +-- configuration schema
 +-- diagnostic tooling
 +-- documentation
```

The exact package/file layout must follow the repository's established packaging architecture.

Before implementing packaging, Copilot Agent must inspect the workflow produced by `adaptive-workflow-configurator`.

Do not impose an unrelated repository structure.

---

# 5. Package Identity

Use a stable project/package identity.

The package identity must not change between releases merely because internal architecture changes.

Maintain:

- package name
- version
- architecture
- maintainer metadata
- homepage/project metadata
- license metadata

The package name should remain stable across upgrades.

---

# 6. Versioning

Use a predictable semantic/versioning strategy.

A release should expose:

```text id="vrs001"
project version
package version
protocol version
configuration schema version
```

These are separate concepts.

Do not assume:

```text
application version == protocol version
```

or:

```text
application version == configuration schema version
```

---

# 7. Compatibility Metadata

Each release should declare:

```text id="vrs002"
Minimum Ubuntu version
Maximum/known GNOME compatibility
Wayland requirement
Required systemd capability
Required PipeWire capability
Required Mutter interfaces
Required libei/EIS capability
Supported GPU configurations
Supported architecture
```

For example:

```text id="vrs003"
Ubuntu:
    26.04 LTS

GNOME:
    50+

Session:
    Wayland

Architecture:
    x86_64

Status:
    Supported
```

Do not use broad compatibility claims such as "all Linux desktops."

---

# 8. Package Dependencies

Declare required dependencies explicitly.

Potential dependency categories:

- systemd
- PAM
- PipeWire
- libei/EIS
- GNOME/Mutter components
- networking/TLS dependencies
- runtime libraries

Optional functionality should use appropriate optional dependencies rather than forcing unnecessary components.

Do not rely on "it happens to be installed on Ubuntu Desktop."

---

# 9. Dependency Validation

The package should fail clearly if required dependencies are missing.

Example:

```text id="dep001"
Required dependency:
    PipeWire

Status:
    MISSING

Remote Access:
    DISABLED
```

Do not attempt unsafe fallback behavior.

---

# 10. Package Installation States

The package lifecycle should distinguish:

```text id="pkg001"
NOT_INSTALLED
INSTALLED
PARTIALLY_CONFIGURED
CONFIGURED
READY
DISABLED
FAILED_SAFE
```

The software must not confuse:

```text
package installed
```

with:

```text
remote access ready
```

---

# 11. Post-Installation Behavior

After package installation:

```text id="post001"
Services:
    Installed

Remote Access:
    DISABLED

Authentication:
    Setup Required

Emergency:
    Installed

GNOME:
    Not Yet Validated
```

The package should not automatically expose a remote endpoint before first-run security setup.

---

# 12. systemd Unit Packaging

Package the systemd units explicitly.

Expected service classes:

```text id="svc001"
remote-hostd.service
remote-gateway.service
remote-emergencyd.service
```

and appropriate user-session integration for:

```text id="svc002"
gnome-session-agent
```

The exact unit names should follow the implementation.

---

# 13. Service Enablement

Installation should distinguish:

```text id="svc003"
installed
```

from:

```text
enabled
```

and:

```text
actively accepting remote connections
```

Recommended initial state:

```text id="svc004"
Services installed:
    YES

Required safety services:
    ENABLED

Remote access:
    DISABLED
```

The emergency safety component may need to run before remote access is enabled.

---

# 14. Service Startup Ordering

Startup must ensure the system cannot enter an unsafe remote state because services started in the wrong order.

Conceptually:

```text id="svc005"
system
  ↓
remote-emergencyd
  ↓
remote-hostd
  ↓
GNOME session agent
  ↓
remote-gateway
  ↓
remote access enabled
```

Actual dependencies must be determined experimentally and according to systemd semantics.

Do not create circular dependencies.

---

# 15. Upgrade Safety

An upgrade must be treated as a controlled state transition.

Preferred conceptual sequence:

```text id="upg001"
UPGRADE REQUEST
      ↓
DISABLE NEW REMOTE CONNECTIONS
      ↓
REVOKE / TERMINATE ACTIVE REMOTE SESSIONS
      ↓
LOCK GNOME
      ↓
RESTORE PHYSICAL DISPLAY
      ↓
RESTORE PHYSICAL INPUT
      ↓
STOP OLD SERVICES
      ↓
INSTALL NEW VERSION
      ↓
MIGRATE CONFIGURATION
      ↓
VALIDATE
      ↓
START SERVICES
      ↓
RUN HEALTH CHECK
      ↓
REMOTE ACCESS REMAINS DISABLED
      ↓
EXPLICIT RE-ENABLE / READY
```

Do not allow package upgrade scripts to leave remote control active while replacing critical components.

---

# 16. Upgrade Interruption

Test interruption at every stage.

Examples:

```text id="upg002"
package installation interrupted
service restart interrupted
configuration migration interrupted
disk full
permission error
dependency failure
reboot during upgrade
```

The resulting state must remain safe.

If the system cannot prove that remote access is safe:

```text
REMOTE ACCESS = DISABLED
```

---

# 17. Configuration Migration

Configuration schemas must have explicit versions.

Example:

```text id="cfg001"
schema_version = 3
```

Migration should be:

```text id="cfg002"
OLD CONFIG
    ↓
VALIDATE
    ↓
MIGRATE
    ↓
VALIDATE NEW CONFIG
    ↓
ACTIVATE
```

Never modify configuration blindly.

---

# 18. Configuration Backup

Before a migration:

- create a recoverable backup where appropriate
- restrict its permissions
- do not duplicate secrets unnecessarily
- identify schema version
- ensure failed migration can be recovered

Backups containing secrets must be protected like the original configuration.

---

# 19. Secret Migration

Security-sensitive data requires special care.

Potential secrets:

- TOTP secret
- Remote Access Key verifier/secret
- recovery codes
- trusted-device credentials
- host private key

Migration must not:

- print secrets
- copy secrets into world-readable files
- pass secrets through command-line arguments
- place secrets in logs
- expose secrets to the gateway
- unnecessarily move secrets between privilege domains

---

# 20. Rollback

A failed upgrade must have a defined rollback strategy.

Possible levels:

### Application Rollback

Restore previous binaries/configuration.

### Package Rollback

Install previous package version.

### Configuration Rollback

Restore previous valid configuration schema.

### Security Rollback

Invalidate sessions/credentials if necessary.

The system must never assume rollback automatically restores remote authority safely.

---

# 21. Downgrade

Downgrades should be treated as potentially unsafe.

Before downgrade:

```text id="dwg001"
terminate remote sessions
revoke control leases
invalidate incompatible state
lock session
restore display
restore input
```

Then install the older version.

If configuration compatibility is uncertain:

```text
REMOTE ACCESS = DISABLED
```

until configuration is explicitly repaired and verified.

---

# 22. Protocol Compatibility

Remote clients and hosts may not always run identical versions.

Define protocol compatibility explicitly.

Conceptually:

```text id="proto001"
Client protocol 3
Host protocol 3
    -> compatible

Client protocol 2
Host protocol 3
    -> policy-dependent

Client protocol 1
Host protocol 3
    -> reject
```

Never silently downgrade security features to accommodate an older client.

---

# 23. Authentication Compatibility

A client must not be allowed to bypass newer security requirements because it uses an older protocol.

For example:

```text id="proto002"
Host requires TOTP
Old client does not support TOTP
        ↓
REJECT
```

Do not implement:

```text
"legacy client mode"
```

that weakens authentication unless there is an explicit, separately designed security model.

---

# 24. Browser Client Versioning

The browser client should expose:

```text id="web001"
client version
protocol version
host compatibility
```

When incompatible:

```text id="web002"
This client version is not compatible with the host.
Please update the client.
```

Do not expose internal stack traces.

---

# 25. WebRTC Compatibility

WebRTC negotiation should be version-tolerant where possible.

However, security and authorization must remain host-controlled.

The browser must not decide that:

```text "video connection exists"
```

means:

```text "remote input is authorized"
```

Media and control authorization remain separate.

---

# 26. Package Signing

Production packages must be cryptographically signed.

The distribution process should provide:

- signed repository metadata
- signed packages where appropriate
- release checksums
- trusted signing keys
- documented key rotation

Never instruct users to bypass package signature verification.

---

# 27. Release Signing Keys

Protect release signing credentials separately from developer workstations where practical.

Define:

- primary release key
- rotation process
- compromise response
- revocation procedure
- trusted-key update mechanism

Document how users verify legitimate releases.

---

# 28. Supply-Chain Security

The build system should account for:

- dependency pinning
- dependency review
- reproducible builds where practical
- build provenance
- signed artifacts
- source-to-binary traceability
- vulnerability scanning
- malicious dependency detection

Do not introduce dependencies solely for convenience without evaluating:

- privilege
- maintenance
- security history
- license
- attack surface

---

# 29. Build Reproducibility

Where practical, aim for reproducible package builds.

Record:

```text id="build001"
source revision
build environment
compiler/interpreter versions
dependency versions
package version
build timestamp policy
```

The goal is to make it possible to determine:

> Which source and dependencies produced this package?

---

# 30. Release Artifacts

A production release should contain:

```text id="rel001"
source archive
Debian package
checksums
signature information
release notes
compatibility information
upgrade notes
security notes
```

Optional:

```text
debug symbols
test reports
SBOM
build provenance
```

---

# 31. Software Bill of Materials

Generate an SBOM where practical.

It should identify:

- direct dependencies
- transitive dependencies
- versions
- licenses
- known vulnerabilities where tooling supports it

Do not include:

- user credentials
- workstation-specific secrets
- private configuration

---

# 32. Release Channels

Consider:

```text id="channel01"
stable
beta
nightly/development
```

The initial project may only need:

```text stable
development
```

Development builds should be clearly identified.

Do not accidentally allow a development build to silently replace a stable security-sensitive installation.

---

# 33. GNOME/Mutter Release Compatibility

GNOME/Mutter updates can affect private APIs.

Therefore each supported release should undergo compatibility validation.

Before declaring support for a new GNOME version:

```text id="gnome01"
capability discovery
virtual monitor
capture
remote input
physical display isolation
physical input isolation
lock semantics
teardown
recovery
emergency
repeated cycles
```

Passing compilation is not sufficient.

---

# 34. Compatibility Matrix

Maintain a release matrix.

Example:

| Ubuntu | GNOME | Wayland | Status |
|---|---|---|---|
| 26.04 | 50.x | Yes | Supported |
| Future | 51.x | Yes | Test required |
| Older Ubuntu | Older GNOME | Varies | Unsupported unless tested |
| KDE | N/A | Yes | Unsupported |
| X11 | N/A | No | Unsupported |

Update this matrix only from actual validation.

---

# 35. Kernel Compatibility

The project should document the kernel range actually tested.

Kernel changes may affect:

- input
- uinput
- DRM/KMS
- systemd
- security policies
- device permissions

Do not claim that all kernels are supported simply because Ubuntu boots.

---

# 36. GPU Driver Compatibility

Release testing must include relevant GPU configurations.

For each supported GPU/driver combination verify:

- virtual display
- capture
- cursor
- display isolation
- display restoration
- input behavior
- repeated sessions
- GNOME stability

A GPU-specific workaround must be documented and tested.

---

# 37. Emergency Daemon Upgrade

The emergency daemon deserves special treatment.

During upgrade:

```text id="emg001"
remote access:
    DISABLED

old emergency daemon:
    HEALTHY

new emergency daemon:
    INSTALL

new emergency daemon:
    VALIDATE

remote access:
    remains DISABLED

emergency:
    VERIFIED

remote access:
    may be re-enabled
```

Never remove the old safety mechanism before the replacement is known to be operational unless the system is already safely disabled.

---

# 38. Privileged Package Scripts

Package installation scripts should be extremely small.

Do not put complex application logic into:

- pre-install scripts
- post-install scripts
- maintainer scripts

Prefer installing files and using explicit application/setup logic.

The more code executed automatically as root, the larger the package installation attack surface.

---

# 39. No Arbitrary Shell During Upgrade

Upgrade logic must not execute arbitrary user-controlled configuration as root.

Avoid patterns where:

```text
configuration value
    ↓
shell command
```

is constructed dynamically.

Use explicit operations and validated values.

---

# 40. Service File Security

Verify packaged systemd units retain:

- correct users
- correct capabilities
- correct sandboxing
- correct filesystem access
- correct restart policy
- correct watchdog
- correct dependencies

A packaging change must not accidentally weaken runtime hardening.

---

# 41. File Permission Tests

After installation verify permissions for:

- binaries
- configuration
- credentials
- private keys
- sockets
- logs
- runtime directories

Examples of unacceptable results:

```text
TOTP secret readable by ordinary users
private key world-readable
Access Key stored in public configuration
gateway can read host authentication database
```

---

# 42. Package Upgrade Security Test

For every release candidate:

```text id="upg003"
1. Install previous stable version.

2. Configure authentication.

3. Establish remote session.

4. Verify physical display/input isolation.

5. Terminate/upgrade.

6. Verify remote authority is revoked.

7. Verify GNOME is locked.

8. Verify physical display is restored.

9. Verify physical input is restored.

10. Upgrade package.

11. Validate configuration migration.

12. Start new services.

13. Verify old session credentials are invalid.

14. Verify security epoch/session policy.

15. Verify remote access remains disabled or requires explicit re-enable where required.

16. Establish a fresh authenticated session.
```

---

# 43. Package Removal Test

Test:

```text id="uninstall01"
REMOTE_ACTIVE
    ↓
uninstall requested
    ↓
remote authority revoked
    ↓
session locked
    ↓
display restored
    ↓
input restored
    ↓
services stopped
    ↓
software removed
```

Repeat when:

- gateway is down
- host daemon is down
- GNOME agent is down
- network is disconnected

---

# 44. Interrupted Uninstall

Simulate:

- package manager interruption
- reboot during removal
- dependency failure
- permission failure
- filesystem full

The workstation must remain safe.

If necessary, the emergency/recovery component should remain until the system is definitively safe.

---

# 45. Release Candidate Checklist

Before release:

```text id="rc001"
[ ] Source reviewed
[ ] Unit tests pass
[ ] Integration tests pass
[ ] System tests pass
[ ] Hardware matrix tested
[ ] Security tests pass
[ ] Fault injection passes
[ ] Emergency recovery passes
[ ] Display restoration passes
[ ] Input restoration passes
[ ] GNOME lock behavior verified
[ ] Upgrade tested
[ ] Downgrade tested
[ ] Uninstall tested
[ ] Interrupted upgrade tested
[ ] Configuration migration tested
[ ] Package permissions verified
[ ] systemd hardening verified
[ ] No secret leakage
[ ] Package signatures verified
[ ] Checksums generated
[ ] Compatibility matrix updated
[ ] Release notes written
```

---

# 46. Release Blocking Conditions

Do not release if:

- remote authority survives a critical failure
- emergency recovery fails
- physical input cannot be reliably isolated
- physical display cannot be reliably isolated
- stale sessions reconnect
- security epoch can be bypassed
- package installation weakens privileges
- upgrade leaves remote control active unexpectedly
- secrets appear in logs/artifacts
- package permissions are unsafe
- critical GNOME compatibility behavior is unknown
- rollback leaves the system in an uncertain state

---

# 47. Security Release Process

Security-sensitive changes should receive additional review.

Examples:

- authentication
- TOTP
- Remote Access Key
- trusted devices
- session credentials
- control leases
- security epoch
- emergency mechanism
- privileged services
- IPC
- display/input isolation
- systemd sandboxing

At least one independent review should verify that the change does not weaken the safety model.

---

# 48. Security Update Process

For security vulnerabilities:

1. Determine affected versions.
2. Determine whether active sessions are affected.
3. Determine whether credentials must be revoked.
4. Determine whether security epoch must be incremented.
5. Produce patched release.
6. Publish security advisory.
7. Provide upgrade instructions.
8. Document whether users must rotate credentials.
9. Verify patched behavior.
10. Test downgrade prevention if applicable.

Do not treat a security fix as an ordinary cosmetic release.

---

# 49. Emergency Security Disable

The project should provide a documented mechanism to disable remote access immediately.

Conceptually:

```text id="disable01"
disable remote access
```

This must:

```text
stop new connections
+
revoke current remote authority
+
invalidate active leases
+
increment security epoch
+
lock session
+
restore display
+
restore input
```

The mechanism should remain available even when the browser/gateway is unavailable.

---

# 50. Release Notes

Every release should document:

- new features
- bug fixes
- security changes
- GNOME compatibility changes
- GPU compatibility changes
- configuration migrations
- protocol changes
- package changes
- known limitations
- upgrade instructions
- downgrade warnings

Security-impacting changes should be explicitly identified.

---

# 51. Known Limitations

Release notes must be honest about limitations.

Examples:

```text
NVIDIA driver X not validated
GNOME version Y not yet supported
multi-monitor restoration has known limitation
certain USB-C docks not validated
TURN deployment required for certain NAT environments
```

Do not hide compatibility limitations to make the release appear broader.

---

# 52. Development Builds

Development builds may contain:

- debug logging
- experimental Mutter backends
- diagnostic features
- incomplete compatibility
- unstable protocol changes

They must clearly indicate:

```text id="dev001"
DEVELOPMENT BUILD
```

Do not make development builds silently appear production-ready.

---

# 53. Packaging CI

CI should validate:

```text id="ci001"
source build
package build
dependency metadata
package contents
file permissions
systemd unit installation
systemd unit validation
upgrade path
uninstall path
configuration migration
artifact checksums
signatures
```

Where possible, test installation inside a clean Ubuntu environment.

---

# 54. Clean-Machine Installation Test

Before release, install the package on a clean supported Ubuntu machine.

Verify:

```text id="clean001"
package installation
preflight
first-run setup
TOTP
Remote Access Key
emergency controller
TLS
GNOME capability detection
display/input safety
remote connection
disconnect
emergency
uninstall
```

This should not rely on developer-specific files or environment variables.

---

# 55. Upgrade-Matrix Test

At minimum test:

```text id="matrix01"
N-1 → N
N-2 → N
clean install N
N → next development build
```

Where configuration migrations exist, include every supported schema transition.

---

# 56. Release Provenance

Every release should identify:

```text id="prov001"
Git revision
source archive checksum
package checksum
build environment
dependency lock/version state
release date
release channel
signing identity
```

This allows users and maintainers to determine exactly what they installed.

---

# 57. Copilot Agent Instructions

When implementing packaging/release engineering:

1. Inspect the repository.
2. Inspect `adaptive-workflow-configurator` output and follow its established workflow.
3. Do not replace its release/build conventions without a clear reason.
4. Determine existing package/build infrastructure before adding new tooling.
5. Prefer the simplest native Ubuntu/Debian packaging mechanism that satisfies the architecture.
6. Keep package maintainer scripts minimal.
7. Keep privileged installation logic minimal.
8. Test installation in a clean environment.
9. Test upgrades from supported previous releases.
10. Test interrupted upgrades.
11. Test uninstall and interrupted uninstall.
12. Test configuration migration.
13. Verify systemd hardening after packaging.
14. Verify file permissions.
15. Verify no secrets leak into package artifacts.
16. Verify signatures/checksums.
17. Verify GNOME compatibility on actual supported environments.
18. Never declare compatibility based only on compilation.

Implementation sequence:

```text id="agent01"
RESEARCH
    ↓
INSPECT EXISTING BUILD/PACKAGING WORKFLOW
    ↓
DEFINE PACKAGE CONTENTS
    ↓
IMPLEMENT PACKAGE
    ↓
INSTALL CLEAN MACHINE
    ↓
RUN PREFLIGHT
    ↓
RUN FIRST-RUN SETUP
    ↓
RUN SAFETY TESTS
    ↓
TEST UPGRADE
    ↓
TEST ROLLBACK/DOWNGRADE
    ↓
TEST UNINSTALL
    ↓
VERIFY ARTIFACT SECURITY
    ↓
DOCUMENT RELEASE
```

---

# 58. Definition of Done

Packaging and release engineering are complete only when:

```text id="done001"
[ ] Native package builds
[ ] Clean installation works
[ ] Unsupported systems are rejected
[ ] Remote access is disabled after installation
[ ] Services are correctly installed
[ ] Service privilege model is preserved
[ ] Configuration migration works
[ ] Upgrade is safe
[ ] Downgrade is safe
[ ] Interrupted upgrade is safe
[ ] Uninstall is safe
[ ] Interrupted uninstall is safe
[ ] Package permissions are correct
[ ] Secrets are protected
[ ] Package signatures work
[ ] Checksums are published
[ ] Release provenance is recorded
[ ] Compatibility matrix is validated
[ ] GNOME updates are tested
[ ] GPU matrix is tested
[ ] Security release process exists
[ ] Emergency disable path works
[ ] Release documentation is complete
```

---

# 59. Final Release Principle

The package is not merely a collection of binaries.

It is part of the remote-control security boundary.

Therefore:

```text id="final01"
INSTALL
    must be safe

UPGRADE
    must be safe

ROLLBACK
    must be safe

DOWNGRADE
    must be safe

UNINSTALL
    must be safe
```

At every point:

```text id="final02"
UNCERTAIN REMOTE AUTHORITY
        ↓
     REVOKE
        ↓
      LOCK
        ↓
    RESTORE
        ↓
   VERIFY SAFE
```

The ultimate packaging invariant is:

> **No package-management operation should ever leave the workstation remotely controllable when the system cannot prove that the remote-control stack is valid and authorized.**