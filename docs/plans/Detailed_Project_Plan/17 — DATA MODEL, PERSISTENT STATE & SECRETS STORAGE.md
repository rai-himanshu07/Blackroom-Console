# 17 — DATA MODEL, PERSISTENT STATE & SECRETS STORAGE

## 1. Purpose

This document defines the data model and storage requirements for the project.

The system has several fundamentally different categories of data:

```text id="data001"
CONFIGURATION
SECURITY MATERIAL
HOST IDENTITY
TRUSTED DEVICES
RECOVERY DATA
EPHEMERAL SESSION STATE
AUDIT / DIAGNOSTIC DATA
```

These categories must not be treated as interchangeable.

The most important principle is:

> **Persist only what must survive restart. Everything else should remain ephemeral.**

In particular, active remote-control authority should not be reconstructable merely from persistent storage.

---

# 2. Storage Principles

The storage architecture must follow these rules:

1. Minimize persistent data.
2. Separate secrets from ordinary configuration.
3. Separate durable identity from ephemeral sessions.
4. Never persist active control authority unnecessarily.
5. Never persist remote input.
6. Never persist passwords.
7. Never persist raw TOTP codes.
8. Never persist session credentials longer than required.
9. Never persist raw control leases unless absolutely required.
10. Protect host private keys.
11. Protect TOTP secrets.
12. Protect Remote Access Key material.
13. Protect trusted-device credentials.
14. Make revocation state durable.
15. Make security-critical state recoverable after restart.
16. Treat corrupted security state as a fail-safe condition.
17. Do not let the gateway become the source of truth for host security state.

---

# 3. Data Categories

The conceptual model is:

```text id="data002"
                    HOST DATA
                       |
        +--------------+--------------+
        |              |              |
        v              v              v
     DURABLE        EPHEMERAL       DIAGNOSTIC
        |              |              |
        v              v              v
 identity          sessions         events
 config            leases           logs
 credentials       transitions      health
 trusted devices   WebRTC state
 recovery
 revocation
```

---

# 4. Durable Data

The following may need persistence:

```text id="data003"
host identity
host private key
configuration
authentication configuration
TOTP secret
Remote Access Key verifier
recovery-code state
trusted-device records
security epoch
credential generation/revocation metadata
protocol/configuration schema versions
```

Only persist information that is necessary for recovery or continued operation.

---

# 5. Ephemeral Data

The following should normally remain in memory/runtime state:

```text id="data004"
active session
session credential
control lease
WebRTC connection
current input stream
current media stream
transition execution state
temporary authentication state
connection health
temporary display state
temporary input state
```

Some operational state may be represented in protected runtime files or sockets if required by the implementation, but should not automatically become durable application state.

---

# 6. Data Ownership

Each component should own only the data it needs.

Recommended:

```text id="data005"
remote-hostd
    security authority
    host identity
    authentication state
    trusted devices
    revocation state
    session authority

gnome-session-agent
    current GNOME/session state
    display topology snapshot
    input state
    runtime capability state

remote-gateway
    minimal routing/session metadata
    no authentication secrets

remote-emergencyd
    minimal emergency configuration
    no user authentication database
    no browser credentials
```

The host daemon remains the security source of truth.

---

# 7. Host Identity

Persist a cryptographic host identity.

Conceptually:

```text id="data006"
HostIdentity:
    host_id
    public_key
    private_key_reference
    created_at
    key_generation
```

The exact cryptographic algorithm should be selected based on current platform/library support.

The host identity must remain stable across:

- DHCP changes
- IP changes
- hostname changes
- network interface changes

---

# 8. Host Private Key

The host private key must be treated as a high-value secret.

Requirements:

- protected filesystem permissions
- accessible only to the required service
- never exposed to gateway
- never returned to browser
- never logged
- never included in diagnostic bundles
- never passed through command-line arguments

If a system secret store is used, document the dependency and recovery behavior.

---

# 9. Host Identity Rotation

Identity rotation must be explicit.

When rotated:

```text id="data007"
old host identity
       ↓
new host identity
```

The system should make it obvious that clients may see a new host fingerprint.

Rotation should not silently occur during ordinary package upgrades.

---

# 10. Configuration Data

Configuration should contain only operational settings.

Examples:

```text id="data008"
network settings
TLS configuration references
session policy
lease policy
display policy
input policy
emergency shortcut
logging policy
feature flags
compatibility policy
```

Do not put secrets into ordinary configuration if a protected secret store is available.

---

# 11. Configuration Schema

Configuration must have an explicit schema version.

Example:

```text id="data009"
schema_version = 1
```

The schema must support:

- validation
- migration
- rollback
- unknown-field handling
- default values

Invalid configuration must not silently activate unsafe behavior.

---

# 12. Security Configuration

Security policy should be explicit.

Conceptually:

```text id="data010"
SecurityConfig:
    require_totp
    require_access_key_for_new_devices
    trusted_device_policy
    lease_duration
    authentication_rate_limits
    recovery_policy
    session_policy
```

The application should enforce secure defaults even if configuration is missing.

---

# 13. TOTP Storage

Persist the TOTP secret securely.

Store:

```text id="data011"
TOTP:
    secret
    algorithm
    digits
    period
    configuration_version
```

The actual secret must never appear in:

- normal logs
- diagnostic output
- browser responses
- configuration dumps
- support bundles

---

# 14. TOTP Secret Access

Only the component that actually needs TOTP verification should receive access to the secret.

Prefer:

```text id="data012"
remote-hostd
    ↓
TOTP verification
```

rather than:

```text id="data013"
gateway
    ↓
TOTP secret
```

The gateway should not need the secret.

---

# 15. Remote Access Key Storage

The preferred model is to store a verifier rather than the raw Access Key.

Conceptually:

```text id="data014"
RemoteAccessKey:
    verifier
    created_at
    rotated_at
    generation
    status
```

When the user enters the Access Key:

```text id="data015"
provided key
    ↓
verification
    ↓
allow / deny
```

The raw key should not normally be recoverable from the host database.

---

# 16. Access-Key Generation

Generate the key using a cryptographically secure random source.

Prefer:

```text id="data016"
>= 256 bits entropy
```

rather than relying on human-created passwords.

The display representation may use a user-friendly encoding such as Base64url.

---

# 17. Access-Key Rotation

Rotation should update:

```text id="data017"
generation
created_at
rotated_at
verifier
status
```

The previous verifier becomes invalid.

Define explicitly whether rotation also:

- terminates active sessions
- revokes trusted devices
- increments security epoch

The default should favor security while avoiding unnecessary disruption.

Provide an explicit stronger operation such as:

```text id="data018"
rotate_and_revoke_all
```

when immediate invalidation is required.

---

# 18. Recovery Codes

Store only what is required to verify recovery codes.

Prefer storing:

```text id="data019"
hashed recovery code
used/unused status
created_at
used_at
```

rather than plaintext recovery codes.

Recovery codes should be:

- single-use
- cryptographically random
- independently revocable
- excluded from logs

---

# 19. Trusted Device Data Model

A trusted device record may contain:

```text id="data020"
TrustedDevice:
    device_id
    host_id
    public_key / credential verifier
    display_name
    created_at
    last_used_at
    revoked_at
    status
    credential_generation
```

Do not store unnecessary device information.

---

# 20. Trusted Device Privacy

Avoid collecting:

- detailed hardware fingerprints
- unrelated browser information
- IP history unless operationally necessary
- application lists
- personal browsing information

A trusted-device record should answer:

> Which client is authorized?

not:

> Everything we can learn about this device.

---

# 21. Trusted Device Revocation

When a device is revoked:

```text id="data021"
device.status = REVOKED
device.revoked_at = now
```

Existing sessions associated with that device must be evaluated.

For immediate revocation policy:

```text id="data022"
revoke device
    ↓
terminate sessions
    ↓
revoke leases
    ↓
increment security epoch where appropriate
```

---

# 22. Revoke All

The system should support global revocation.

Conceptually:

```text id="data023"
REVOKE ALL
    ↓
invalidate trusted devices
invalidate active sessions
invalidate leases
increment security epoch
```

This is a security operation and should be auditable.

---

# 23. Security Epoch Persistence

The current security epoch must survive service restart.

For example:

```text id="data024"
security_epoch = 42
```

After restart:

```text id="data025"
security_epoch >= 42
```

Never reset the epoch to an old value merely because the daemon restarted.

---

# 24. Epoch Integrity

The security epoch is security-critical state.

If storage is:

- corrupted
- unreadable
- inconsistent
- rolled back unexpectedly

the system should prefer:

```text id="data026"
REMOTE ACCESS DISABLED
```

rather than attempting to guess the correct epoch.

---

# 25. Session Storage

Active remote sessions should normally be ephemeral.

Conceptually:

```text id="data027"
RemoteSession:
    session_id
    user
    client_id
    created_at
    state
    security_epoch
```

Do not persist active sessions simply to support browser refresh.

Browser resume must require current authorization.

---

# 26. Session Restart Behavior

After `remote-hostd` restart:

```text id="data028"
old sessions
    ↓
INVALID
```

The system must not reconstruct active remote-control authority automatically.

This is especially important after:

- crash
- upgrade
- downgrade
- reboot
- security reset

---

# 27. Session Credentials

Session credentials should remain ephemeral.

If persistence is unavoidable:

- encrypt/protect them
- bind them to session
- expire them aggressively
- invalidate on restart where appropriate

The preferred model is:

```text id="data029"
daemon restart
    ↓
session credentials invalid
```

---

# 28. Control Lease Storage

Control leases should normally be in memory.

Conceptually:

```text id="data030"
Lease:
    session_id
    client_id
    epoch
    issued_at
    expires_at
```

Do not persist a valid lease across daemon restart.

After restart:

```text id="data031"
NO VALID LEASE
```

---

# 29. WebRTC State

Do not persist:

- active WebRTC connection
- ICE state
- media state
- input state
- peer connection state

These are runtime resources.

A new connection must perform fresh authorization/session validation.

---

# 30. Display Configuration Snapshot

The GNOME agent may need a temporary snapshot of the original physical display topology.

Conceptually:

```text id="data032"
DisplaySnapshot:
    topology_id
    outputs
    modes
    positions
    scale
    rotation
    enabled_state
```

This is recovery state, not user configuration.

It should exist only for the duration of a remote session or recovery operation.

---

# 31. Display Snapshot Safety

The snapshot must not be blindly applied later.

Before restoration, verify:

- hardware still exists
- output still exists
- mode is still valid
- topology is still compatible

If the monitor changed:

```text id="data033"
original topology unavailable
        ↓
safe restoration strategy
```

Never force an invalid display configuration.

---

# 32. Input State Snapshot

If the implementation changes local input routing, preserve only the minimum state required to restore it.

Example:

```text id="data034"
input isolation state
input backend
routing generation
```

Do not store raw keyboard/mouse events.

---

# 33. Recovery State

If the system crashes during a transition, a minimal recovery marker may be required.

Conceptually:

```text id="data035"
RecoveryMarker:
    transition_id
    expected_safe_action
    created_at
```

The marker must not contain credentials.

Its purpose is:

> Help the next process determine that recovery is required.

---

# 34. Recovery Marker Rules

Recovery markers must be:

- minimal
- integrity-protected where appropriate
- short-lived
- removed after successful recovery
- treated as untrusted input

If the marker is inconsistent:

```text id="data036"
REMOTE ACCESS = DISABLED
```

and run safe recovery.

---

# 35. Persistent State vs Runtime State

Maintain a clear separation:

| Data | Persistent | Runtime |
|---|---:|---:|
| Host identity | Yes | Loaded |
| Host private key | Yes | As needed |
| Configuration | Yes | Loaded |
| TOTP secret | Yes | Protected access |
| Access Key verifier | Yes | Loaded |
| Recovery codes | Yes | Loaded as needed |
| Trusted devices | Yes | Loaded |
| Security epoch | Yes | Current |
| Active session | No | Yes |
| Session credential | No | Yes |
| Control lease | No | Yes |
| WebRTC connection | No | Yes |
| Input stream | No | Yes |
| Media stream | No | Yes |
| Display snapshot | Temporary | Yes |
| Recovery marker | Temporary | Conditional |
| Raw keyboard input | Never | Never persist |
| Raw screen content | Never | Never persist |
| Clipboard | Never by default | Runtime only if feature exists |

---

# 36. What Must Never Be Persisted

The application must never persist:

```text id="data037"
Linux passwords
raw TOTP codes
raw recovery codes
raw remote input events
raw keyboard contents
screen contents
audio contents unless explicitly implementing secure recording
clipboard contents by default
WebRTC media
browser cookies
browser local authentication secrets
session credentials after expiry
control leases after expiry
```

Any future feature that requires persistence of one of these must undergo explicit security review.

---

# 37. Credential Separation

These credentials must remain distinct:

```text id="data038"
Linux password
TOTP secret
Remote Access Key
trusted-device credential
host private key
session credential
control lease
```

Do not derive one from another.

Do not reuse one credential as another credential.

---

# 38. Secret Storage Abstraction

Create a narrow secret-storage abstraction.

Conceptually:

```text id="secret001"
SecretStore:
    get_secret()
    set_secret()
    delete_secret()
    rotate_secret()
```

The implementation may use:

- protected filesystem storage
- OS secret store
- another appropriate Linux mechanism

The rest of the application should not need to know the storage implementation.

---

# 39. Secret Store Requirements

The abstraction must provide:

- access control
- atomic updates where possible
- secure permissions
- error handling
- deletion
- rotation
- versioning where required

A secret-store failure must not result in silently disabling authentication.

---

# 40. Secret Store Failure

If the TOTP secret cannot be read:

```text id="data039"
TOTP unavailable
    ↓
remote authentication unavailable
```

If the Access Key verifier cannot be read:

```text id="data040"
new-device authentication unavailable
```

Do not fall back to weaker authentication.

---

# 41. Atomic Security Updates

Security-critical state should be updated atomically where possible.

Examples:

```text id="data041"
rotate access key
update verifier
update generation
```

should not leave:

```text id="data042"
generation says NEW
verifier says OLD
```

Use transactions or atomic replacement appropriate to the storage mechanism.

---

# 42. Crash Consistency

Test crashes during:

- Access Key rotation
- trusted-device revocation
- revoke-all
- security epoch increment
- TOTP configuration
- configuration migration
- recovery-state update

After restart, the system must resolve to a safe state.

---

# 43. Security Epoch Update Ordering

Security-sensitive operations must consider ordering carefully.

For emergency/revoke-all, prefer:

```text id="data043"
invalidate authority
    ↓
increment epoch
    ↓
persist new epoch
```

or another ordering that guarantees no stale authority can become valid after restart.

The exact transaction must be formally reasoned about and tested.

Do not implement epoch updates casually.

---

# 44. Database Selection

The implementation may use:

- SQLite
- protected structured files
- another local database/storage mechanism

The choice should be based on:

- atomicity
- concurrency
- simplicity
- corruption recovery
- security
- backup behavior
- dependency footprint

Do not introduce a database merely because the project has several records.

SQLite is a reasonable candidate if transactional state becomes sufficiently complex.

---

# 45. Database Security

If SQLite or another database is used:

- restrict filesystem permissions
- separate secret material where practical
- use transactions
- validate schema
- handle corruption
- test crash recovery
- avoid storing unnecessary secrets
- do not expose the database to the gateway

A local database should never be treated as a security boundary by itself.

Filesystem and service permissions remain important.

---

# 46. Database Corruption

Simulate:

- truncated database
- invalid schema
- missing table
- partially written state
- filesystem read failure
- disk full

Expected:

```text id="data044"
REMOTE ACCESS = DISABLED
```

plus a clear diagnostic.

Do not attempt dangerous automatic reconstruction of security state.

---

# 47. Backup Policy

Backups require special care because they may contain:

- host identity
- TOTP secret
- authentication state
- trusted devices

Do not automatically upload application backups to cloud storage.

If backup/restore is supported, define explicitly:

- what is backed up
- encryption
- access requirements
- restore semantics
- credential rotation
- host identity behavior
- trusted-device behavior

---

# 48. Restore Policy

Restoring a backup onto another machine must not silently clone host identity and trust relationships.

Preferred behavior:

```text id="data045"
restore configuration
    ↓
detect different host
    ↓
require explicit identity decision
```

A restored machine should not unexpectedly impersonate the original host.

---

# 49. Host Cloning

If a VM or disk image is cloned:

- detect identity duplication where practical
- do not allow two machines to share the same cryptographic host identity unintentionally
- provide an explicit host identity regeneration process

After regeneration:

```text id="data046"
new host identity
+
new fingerprint
```

Existing clients should treat it as a different host.

---

# 50. Time Handling

Security-sensitive timestamps should use a consistent time source.

Persist:

```text id="data047"
created_at
expires_at
revoked_at
last_used_at
```

Use UTC internally where appropriate.

Do not rely on local timezone for security decisions.

TOTP requires particular care with clock skew.

---

# 51. Clock Rollback

Test system clock moving backward.

Important effects:

- TOTP validation
- session expiry
- lease expiry
- certificate validity
- diagnostic timestamps

Do not allow clock manipulation to extend remote-control authority indefinitely.

Short-lived credentials should fail safely when expiration cannot be trusted.

---

# 52. Monotonic Time

For runtime timeout decisions, prefer a monotonic clock.

Use wall-clock time for:

- human-readable timestamps
- persisted audit metadata
- TOTP according to its specification

Use monotonic time for:

- lease timers
- operation timeouts
- transition deadlines
- reconnect backoff

---

# 53. Data Retention

Define retention separately for:

```text id="data048"
security events
operational events
debug logs
trusted-device metadata
session metadata
```

Do not retain session metadata forever merely because it is easy.

---

# 54. Deletion

Provide mechanisms to delete:

- revoked trusted devices
- old diagnostic data
- old recovery state
- obsolete configuration
- temporary snapshots

Security records may need to retain minimal evidence of revocation.

For example:

```text id="data049"
device revoked
```

may remain as metadata without retaining the original credential.

---

# 55. Audit Data

Audit records should contain enough information to establish:

```text id="data050"
who
what
when
from which client
against which host
result
```

without storing secrets.

Example:

```text id="data051"
event:
    TRUSTED_DEVICE_REVOKED

client_id:
    td_8f21...

timestamp:
    ...

result:
    SUCCESS
```

---

# 56. Audit Integrity

Security audit records should be protected from ordinary service manipulation.

The gateway must not be able to rewrite host audit history.

Where practical:

- use journald
- restrict write access
- separate security events from user-generated metadata
- include sequence numbers

Do not build a complex tamper-proof ledger unless there is a real requirement.

---

# 57. Persistent State on Restart

After a normal restart:

```text id="data052"
load identity
load configuration
load authentication configuration
load trusted devices
load security epoch
validate state
```

Then:

```text id="data053"
active remote sessions:
    NONE

active control leases:
    NONE
```

The system should enter:

```text id="data054"
LOCAL_LOCKED
```

or another safe state as dictated by the runtime conditions.

---

# 58. Startup Reconciliation

Startup should reconcile:

```text id="data055"
persistent state
+
actual GNOME state
+
actual display state
+
actual input state
+
service state
```

Never assume persistent state represents reality.

For example:

```text id="data056"
database says:
    REMOTE_ACTIVE

actual GNOME:
    session gone
```

The correct result is recovery, not continuation of remote authority.

---

# 59. State Reconciliation Rules

Prefer:

```text id="data057"
actual safe state
```

over:

```text id="data058"
stale persisted state
```

If reality cannot be determined:

```text id="data059"
FAIL SAFE
```

---

# 60. Data Model Validation

Every persisted record should be validated before use.

Check:

- schema version
- required fields
- types
- bounds
- timestamps
- relationships
- host identity
- credential generation
- status values

Malformed persistent data must not trigger privileged behavior.

---

# 61. Data Model Testing

Test:

- empty database
- valid database
- old schema
- future schema
- corrupted database
- missing fields
- extra fields
- invalid enum
- invalid timestamp
- duplicate device ID
- duplicate host identity
- invalid epoch
- invalid credential generation

Expected behavior should be deterministic.

---

# 62. Data Migration

Data migration must be:

```text id="data060"
versioned
atomic
tested
reversible where practical
```

Before migration:

```text id="data061"
validate old schema
```

After migration:

```text id="data062"
validate new schema
```

Only then should the service use the new state.

---

# 63. Secret Rotation During Migration

If storage format changes:

- migrate secret references carefully
- avoid unnecessary secret re-encryption/decryption
- preserve permissions
- verify successful retrieval
- invalidate compromised/ambiguous credentials where necessary

If the migration cannot prove that a secret is correctly protected:

```text id="data063"
REMOTE ACCESS = DISABLED
```

---

# 64. Configuration vs Security Policy

Do not allow ordinary operational configuration to override mandatory security rules.

For example:

```text id="data064"
require_totp = false
```

must not be accepted if TOTP is a mandatory product requirement.

Similarly:

```text id="data065"
disable_emergency = true
```

must not silently disable a safety-critical mechanism if the product requires it.

Configuration can customize policy only within defined safe boundaries.

---

# 65. Feature Flags

Feature flags should not be used to bypass security invariants.

Acceptable:

```text id="data066"
enable_clipboard = false
```

Potentially unsafe:

```text id="data067"
skip_input_isolation = true
```

Security-critical behavior must not become an accidental feature flag.

---

# 66. Environment Variables

Environment variables may provide:

- development overrides
- diagnostic configuration
- test settings

but must not be the normal authority for:

- authentication policy
- security epoch
- credentials
- trusted devices
- active sessions

Production security state must have a controlled persistent source of truth.

---

# 67. Runtime Files

Runtime files may be appropriate for:

- PID
- sockets
- temporary state
- transition markers
- diagnostic information

They must:

- use appropriate runtime directories
- have restrictive permissions
- not contain secrets unnecessarily
- be cleaned up safely

---

# 68. No Browser Persistence of Host Secrets

The browser must never persist:

- Remote Access Key
- TOTP secret
- Linux password
- recovery codes

A trusted-device credential may be stored using an appropriate browser/platform mechanism, but must remain revocable by the host.

---

# 69. Data Model and Emergency Recovery

Emergency recovery must not require access to:

- browser storage
- gateway database
- WebRTC state
- remote session database

The emergency path should need only the minimum local state required to:

```text id="data068"
revoke authority
increment epoch
lock
restore display
restore input
verify safe state
```

---

# 70. Data Model and Package Upgrades

When upgrading:

```text id="data069"
persistent state
    ↓
validate
    ↓
migrate
    ↓
validate
    ↓
start services
```

Never allow an upgrade to interpret old data as valid remote-control authority without explicit validation.

---

# 71. Data Model and Uninstallation

During uninstall:

```text id="data070"
remote authority
    ↓
REVOKE
```

Then:

```text id="data071"
configuration
credentials
trusted devices
identity
```

may be removed according to the user's selected uninstall policy.

The distinction between:

```text remove application
```

and:

```text remove all security data
```

must be explicit.

---

# 72. Security Reset

A security reset should invalidate:

```text id="data072"
active sessions
control leases
trusted devices where selected
Remote Access Key where selected
recovery codes where selected
```

and increment the security epoch.

It should not necessarily regenerate host identity unless explicitly requested.

---

# 73. Data Access Matrix

| Data | Gateway | Hostd | GNOME Agent | Emergency |
|---|---:|---:|---:|---:|
| Host public identity | Read | Read/Write | Read | Read |
| Host private key | No | Yes | No | No |
| Linux password | No | PAM only | No | No |
| TOTP secret | No | Yes | No | No |
| Access Key verifier | No | Yes | No | No |
| Recovery verifier | No | Yes | No | No |
| Trusted devices | No | Yes | No | No |
| Security epoch | Limited | Yes | Validate | Yes |
| Active sessions | Limited | Yes | Limited | Revoke |
| Control lease | Limited | Yes | Validate | Revoke |
| Display state | No | Request | Yes | Restore |
| Input state | No | Request | Yes | Restore |
| Browser credentials | No | No | No | No |

The exact permissions may differ in implementation, but the principle must remain:

> **No component gets data merely because it is convenient.**

---

# 74. Data Security Tests

Test that:

```text id="data073"
gateway cannot read secrets
GNOME agent cannot read authentication database
emergency daemon cannot read user credentials
ordinary user cannot read protected service secrets
diagnostic bundle contains no secrets
logs contain no secrets
browser cannot obtain secrets
```

Also test filesystem permissions independently from application-level authorization.

---

# 75. Copilot Agent Instructions

When implementing the data model/storage layer:

1. Inspect the repository and `adaptive-workflow-configurator` workflow first.
2. Follow established persistence and configuration conventions where appropriate.
3. Do not introduce a database unless complexity justifies it.
4. Clearly separate durable, runtime, temporary, and never-persist data.
5. Keep the host daemon as the security authority.
6. Keep authentication secrets away from the gateway and GNOME agent.
7. Prefer verifiers over plaintext credentials where practical.
8. Use secure random generation for new secrets.
9. Use atomic updates for security-critical state.
10. Make schema versions explicit.
11. Add migration tests.
12. Add corruption tests.
13. Add crash-consistency tests.
14. Add restart/reconciliation tests.
15. Add secret-leakage tests.
16. Add permission tests.
17. Never persist remote input or screen content.
18. Never restore remote-control authority from stale persistent state.
19. Treat ambiguous security state as a reason to disable remote access.
20. Document all persistent fields and their ownership.

Implementation order:

```text id="agent03"
DATA MODEL
    ↓
SCHEMA
    ↓
CONFIGURATION
    ↓
SECRET STORAGE
    ↓
HOST IDENTITY
    ↓
TRUSTED DEVICES
    ↓
RECOVERY DATA
    ↓
SECURITY EPOCH
    ↓
RUNTIME SESSION MODEL
    ↓
PERSISTENCE TRANSACTIONS
    ↓
MIGRATION
    ↓
CRASH RECOVERY
    ↓
CORRUPTION TESTING
    ↓
SECURITY TESTING
```

---

# 76. Definition of Done

The data/storage layer is complete only when:

```text id="done003"
[ ] Durable data is explicitly defined
[ ] Runtime data is explicitly defined
[ ] Never-persist data is explicitly defined
[ ] Host identity storage works
[ ] Private-key protection works
[ ] Configuration schema works
[ ] Configuration validation works
[ ] TOTP storage is protected
[ ] Access Key verifier storage works
[ ] Recovery codes are protected
[ ] Trusted-device storage works
[ ] Revocation works
[ ] Security epoch persists correctly
[ ] Active sessions are ephemeral
[ ] Control leases are ephemeral
[ ] Stale sessions cannot be restored
[ ] Display snapshots are temporary
[ ] Recovery markers are safe
[ ] Storage updates are atomic
[ ] Crash consistency is tested
[ ] Corruption is tested
[ ] Migration is tested
[ ] Restart reconciliation is tested
[ ] Host cloning is considered
[ ] Backup/restore policy is defined
[ ] Uninstall behavior is defined
[ ] Security reset is defined
[ ] Access boundaries are tested
[ ] Secret leakage tests pass
```

---

# 77. Final Data Principle

The data model must reinforce the security architecture:

```text id="final06"
DURABLE IDENTITY
        +
DURABLE SECURITY POLICY
        +
DURABLE REVOCATION STATE
        |
        v
SAFE RESTART
```

But:

```text id="final07"
ACTIVE REMOTE AUTHORITY
        +
CONTROL LEASE
        +
WEBRTC SESSION
        |
        v
EPHEMERAL
```

Therefore:

```text id="final08"
RESTART
    ↓
OLD REMOTE AUTHORITY = INVALID
    ↓
NEW AUTHENTICATION REQUIRED
    ↓
NEW SESSION
    ↓
NEW CONTROL LEASE
```

And if persistent security state cannot be trusted:

```text id="final09"
UNKNOWN SECURITY STATE
        ↓
REMOTE ACCESS DISABLED
        ↓
LOCAL SAFE RECOVERY
```

**The database or filesystem is not the authority to control the workstation. It is only durable evidence and configuration used by the actual security authority.**

The ultimate invariant is:

> **Nothing stored on disk should be sufficient by itself to regain remote control of the workstation.**