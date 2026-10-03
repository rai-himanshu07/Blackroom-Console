# Threat model: the security authority (Phase 11)

Scope: the login authority, its credential files, its sockets and its operator verbs. The full spec
is Doc 09; this records what Phase 11 actually defends, the decisions that shaped it, and what it
does not.

## Boundaries

| Boundary | Control |
|---|---|
| Network to host | none directly: the authority listens on two Unix sockets (0600, same-uid peer check) in a 0700 runtime directory; a gateway must connect from your account |
| Authority to PAM | fixed command line, password on stdin only, cleared environment, 15 s timeout then kill, helper sets no-dump and prints nothing |
| Authority to disk | one 0700 directory, files 0600, atomic replacement, no symlink followed, names from a fixed alphabet, size and schema checked |
| Operator to authority | `admin.sock`, owner uid only; `enable` can additionally need polkit (`--polkit`) |

## Attackers (Doc 09 §11)

- **Internet / LAN**: cannot reach the sockets; flooding the future gateway is bounded by the limiter
  and costs no password check once a client or the table is saturated.
- **Credential attacker** (some factors): every factor is checked, in an order that never discloses
  which one failed before the password passed; a wrong key does not burn an authenticator code.
- **Stolen device**: the credential alone opens nothing (password and code still needed); `revoke-device`
  kills it and its live sessions at once. The device credential is a bearer secret, not a hardware key.
- **Stolen Remote Access Key**: useless without password and code; `rotate-key` replaces it.
- **Malicious local user (other uid)**: cannot enter the state or runtime directory (0700) nor connect to
  the sockets. Tests assert the modes and that foreign uids are refused; they do not run as a second user.
- **Faulty software**: damaged credential files fail closed; crash leftovers change nothing; sessions
  die with the process; the epoch only moves forward.

## Decisions

- **hostd runs as you, not as a dedicated user.** `pam_unix` can verify only the calling user's own
  password without root, through the setgid `unix_chkpwd`. A dedicated `remote-hostd` user would need
  root or the `shadow` group to check your password, which is a larger privilege than the problem
  needs. The cost is that a process running as you can read the credential files; see below.
- **No NoNewPrivileges on the unit**, for the same reason (it blocks `unix_chkpwd`). Seccomp-based
  sandbox options are also off, because they imply it.
- **Polkit is a gate for re-opening only.** Closing access (`disable`, `revoke-all`) never asks;
  `enable` can require a local, active session. It is off by default because a same-uid attacker can
  edit the files anyway; it protects against a mistaken or remote-shell `enable`, not against malware
  running as you.
- **No generic secret encryption at rest.** Verifiers are hashed; the TOTP secret must be readable
  to compute codes, so it is protected by file permissions only.

- **PAM binding: `pam-client` 0.5.0, a deviation from `architecture.md`.** That record rejected
  `pam`/`pam-client` as unmaintained and named `nonstick` (0.1.2, March 2026, young) as the alternative.
  `pam-client` was used because it builds and works today, and the whole binding is one ~150-line file in
  its own Cargo workspace (`crates/pam-auth-helper/src/main.rs`), so replacing it is a one-file change.
  `cargo audit` and `cargo deny` are clean for it. Owner decision needed: keep, or switch to `nonstick`.

## Accepted (Doc 09 §101) and known limits

- Malware running as your user, a compromised kernel or root can read the TOTP secret and the files.
- TOTP uses HMAC-SHA-1 (RFC 6238 default; collision resistance is not needed).
- Sessions are in memory; a hostd restart logs everyone out (by design, plus the epoch moves).
- A shared per-account lock means an attacker who knows your account name can lock you out for 15 min
  by guessing (availability, not access). `blackroom` on the local machine still works.
- Timing: unknown and known accounts take the same code path and a PAM call; PAM's own failure delay
  applies to both. Not measured statistically.
- The unit carries no seccomp-based option: in a user manager each one (checked live: `MemoryDenyWriteExecute`,
  `RestrictAddressFamilies`, `LockPersonality`) turns `NoNewPrivileges` on and the PAM helper then cannot
  decide (exit 2, login answers `HOST_UNAVAILABLE`). Sandboxing needs a design that keeps `unix_chkpwd` working.

## Gate J (no arbitrary privileged execution)

Nothing network-reachable executes a caller-chosen command: the helper and `pkcheck` are started with
fixed arguments (the polkit process argument is built from a numeric pid and uid), no component runs
with elevated privilege, and the two root-owned files installed by `install-security.sh` are static.
