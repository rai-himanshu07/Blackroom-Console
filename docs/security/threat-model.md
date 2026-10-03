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

- **PAM binding: `nonstick` 0.1.2**, as `architecture.md` recorded (`pam-client` was rejected as unmaintained;
  it was used for the first build and replaced on the owner's decision). Only `TransactionBuilder` and
  `authenticate` are used, in one ~120-line file (`crates/pam-auth-helper/src/main.rs`); the crate is young
  and was checked by its API surface and `cargo audit`/`cargo deny`, not by a full code audit.

## Accepted (Doc 09 §101) and known limits

- Malware running as your user, a compromised kernel or root can read the TOTP secret and the files.
- TOTP uses HMAC-SHA-1 (RFC 6238 default; collision resistance is not needed).
- Sessions are in memory; a hostd restart logs everyone out (by design, plus the epoch moves).
- A shared per-account lock means an attacker who knows your account name can lock you out for 15 min
  by guessing (availability, not access). `blackroom` on the local machine still works.
- Timing: unknown and known accounts take the same code path and a PAM call; PAM's own failure delay
  applies to both. Not measured statistically.
- The unit carries no sandbox option at all. Probed live with a wrong password: every seccomp-based option
  (`MemoryDenyWriteExecute`, `RestrictAddressFamilies`, `LockPersonality`) and every namespace-based one
  (`PrivateTmp`, `ProtectSystem`, `ProtectHome`, `ProtectKernelTunables`, `ProtectControlGroups`) made the PAM
  helper exit 2 (cannot decide, login answers `HOST_UNAVAILABLE`); `PrivateDevices`, `ProtectKernelModules`
  and `ProtectClock` refuse to start (218/CAPABILITIES) in a user manager. A sandbox needs a different design
  (for example a root-owned system unit with a separate PAM helper), which is not planned.
- The trusted-browser credential lives in `localStorage`, readable by any script on the page origin. The page
  loads no third-party script; a cross-site-scripting bug would expose it (still useless without password
  and code).
- Enumeration timing was measured once (6 wrong-password tries each, real PAM helper): your account 1.5 to 2.4 s
  (median 1.6 s), a nonexistent account 1.5 to 2.6 s (median 2.0 s). The ranges overlap and PAM's own random
  failure delay (about 2 s plus or minus half) swamps any difference; not a statistical proof.

## Gate J (no arbitrary privileged execution)

Nothing network-reachable executes a caller-chosen command: the helper and `pkcheck` are started with
fixed arguments (the polkit process argument is built from a numeric pid and uid), no component runs
with elevated privilege, and the two root-owned files installed by `install-security.sh` are static.

## Internet exposure (docs/ops/internet-access.md)

- Recommended: a mesh VPN (Tailscale) so nothing faces the internet. `--public` exists for a port-forwarded setup and refuses weak
  settings instead of warning: login through hostd, https from a real certificate, plain http on loopback only.
- What a stranger can reach: the login page and `/login` (body limit 4 KiB, per-source and per-account limits in hostd) and nothing else; every
  other route answers 401 before parsing a body. Headers: Content-Security-Policy (no third-party loads, no framing), `nosniff`, no referrer, HSTS and
  `Secure` cookies.
- New residual risks: online guessing of the Linux password (needs the code and the key as well, but the first factor is attackable from anywhere);
  a live phishing relay; a stranger locking the account for 5 minutes at a time (a trusted browser is exempt); the TURN relay is another public
  service to keep patched (credentials expire in 1 hour by default and the relay denies private ranges in the sample config).
