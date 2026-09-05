# Security architecture — stack, naming, IPC, crypto, and crate inventory

This document records the assessment §6 decisions verbatim as project
decisions (Document 00 declines to choose a language/library stack; the user
delegated stack selection 2026-09-04), plus the Document 00 §51 crate
evaluation (security, maintenance, licence, privilege, attack surface,
compatibility, failure behaviour) for every crate named in the roadmap.
Source: `docs/plans/assessment-20260904-detailed-project-plan.md` §6.

## 1. Languages, toolchain, workflow (assessment §6.1)

| Decision | Choice |
|---|---|
| Host components (`remote-hostd`, `gnome-session-agent`, `remote-emergencyd`, `remote-gateway`, `pam-auth-helper`, optional `remote-input-helper`, `blackroom` CLI, experiment binaries, system-test harness) | **Rust** (stable, edition 2024, MSRV = distro-available 1.96) |
| Browser client | **TypeScript** with Vite; framework-light (Preact or Lit, decided Phase 13; no heavyweight framework) |
| Tests | **Rust only**: `cargo test` unit/integration; `cargo test -p blackroom-systest -- --ignored` real-GNOME, gated `BLACKROOM_SYSTEST=1` + host preflight; `vitest` + Playwright for the browser |
| Static analysis | `cargo fmt --check`, `cargo clippy -D warnings`, `cargo audit`, `cargo deny check`, `npm run lint` (eslint), `tsc --noEmit` |
| Repository layout | Cargo workspace: `crates/blackroom-core` (state machine, lease, epoch, protocol types, error codes, event schema — no I/O), `crates/blackroom-ipc`, `crates/blackroom-gnome` (`GnomeBackend` trait + Mutter impl), `crates/blackroom-store` (secrets/state), `crates/remote-hostd`, `crates/gnome-session-agent`, `crates/remote-emergencyd`, `crates/remote-gateway`, `crates/pam-auth-helper`, `crates/blackroom-cli`, `crates/blackroom-experiments`, `crates/blackroom-systest`; `web/`; `systemd/`; `polkit/`; `packaging/debian/` |

## 2. Names, identities, paths, accounts (assessment §6.2)

| Item | Decision |
|---|---|
| Product / package | `blackroom-console` (single `.deb`, v1) |
| Operator CLI | `blackroom` |
| Components (binaries = systemd units = service users) | `remote-hostd`, `remote-gateway`, `remote-emergencyd` (system units, `Type=notify`, watchdog); `gnome-session-agent` (systemd **user** unit, `PartOf=graphical-session.target`); `pam-auth-helper` (short-lived, spawned by `remote-hostd`) |
| Groups | `blackroom-admin` (privileged CLI verbs after polkit), `blackroom-session` (desktop user, agent-socket access) |
| Directories | `/etc/blackroom-console/` (`0750 root:remote-hostd`); `/var/lib/blackroom-console/` (`0750 remote-hostd`), `…/secrets/` (`0700`), `…/state/`, `…/gateway/` (`0750 remote-gateway`); `/run/blackroom-console/` sockets (`hostd.sock` `0660 remote-hostd:remote-gateway`, `admin.sock` `0660 root:blackroom-admin`, `agent.sock` `0660 remote-hostd:blackroom-session`, `emergency.sock` `0600 remote-emergencyd`); journald only, no `/var/log/` |
| Config format | TOML (`/etc/blackroom-console/config.toml`, schema-versioned); unsafe values (`require_totp=false`, `skip_input_isolation`) rejected at parse time |
| mDNS | `_blackroom._tcp` (provisional) |
| Correlation IDs | `rs_<ULID>` sessions, `tr_<ULID>` transitions, `cl_<ULID>` clients, `bc_<hex>` host id |

## 3. Interfaces, IPC and privilege crossing (assessment §6.3)

| Item | Decision |
|---|---|
| Local IPC transport | Unix domain sockets (`SOCK_STREAM`) with `SO_PEERCRED` UID/GID/PID + `sd_pid_get_unit`; length-prefixed JSON (serde + `schemars`, max 64 KiB, unknown-critical-field rejection, request IDs, per-request timeouts) |
| D-Bus usage | Only toward the platform: `gnome-session-agent` → Mutter/Shell/ScreenSaver on the **session** bus (Experiment 2 confirms which interfaces exist, `docs/gnome/api-inventory.md`); `remote-hostd`/`remote-emergencyd` → `login1`, `systemd1`, `PolicyKit1` on the **system** bus (Experiment 1/2 confirm the exact `login1` surface). No component exports a D-Bus interface in v1 |
| Admin authorisation | polkit actions `org.blackroom.console.{enable,disable,revoke,rotate,reset}`, checked by `remote-hostd` for `admin.sock`, plus `blackroom-admin` socket ACL |
| Gateway → hostd operations | `authenticate, create_session, request_control, renew_control, release_control, get_session_status, get_capabilities, disconnect_session, signal_webrtc, get_public_status` |
| Hostd → agent operations | Doc 16 §29 list (`get_session_state … verify_safe_state`), each returning `Result` + state evidence |
| Emergency → hostd | `EMERGENCY_REVOKE`, `EMERGENCY_STATUS` only |
| PAM | `pam-auth-helper` (Rust), fixed service name `blackroom-console`, `auth include common-auth` only; root requirement for `unix_chkpwd` access is a Phase 1 research item (§7.7) |

## 4. Cryptography and secrets (assessment §6.4)

| Item | Decision |
|---|---|
| Randomness | `getrandom`/`OsRng` only; `zeroize`/`secrecy` for in-memory secrets |
| Host identity | Ed25519 keypair (`ed25519-dalek`), private key `0600 remote-hostd`; fingerprint = SHA-256 of public key |
| Remote Access Key | 256-bit CSPRNG, base64url; Argon2id verifier (m=64 MiB, t=3, p=1); rotation = new generation + epoch increment |
| Recovery codes | 10 × 80-bit base32, Argon2id-hashed, single-use |
| TOTP | RFC 6238 SHA-1/6 digits/30 s, `totp-rs`; ±1 step skew; reject (step, code) reuse |
| Session credential | 256-bit opaque bearer, server-side SHA-256 hash bound to host/user/client/session/epoch/expiry. No JWT |
| Control lease | `ControlLease` signed by the host identity key; `gnome-session-agent` verifies locally per event; `remote-hostd` can still revoke synchronously |
| Trusted-device credential | Browser WebCrypto **non-extractable** `CryptoKey` (ECDSA P-256; Ed25519 when available), IndexedDB; never a bearer string in localStorage |
| TLS | `rustls` in `remote-gateway`; user-provided or setup-generated CA/host cert; no pinning |
| Persistence | Atomic `write-tmp → fsync → rename → fsync(dir)`; `rusqlite` (bundled SQLite, WAL) only if file-per-record proves insufficient; schema version on every file |

## 5. Initial numeric defaults (assessment §6.5, `blackroom_core::limits`)

| Parameter | Default |
|---|---|
| Authentication session TTL | 5 min |
| Session credential max lifetime | 12 h absolute |
| Control lease TTL / renewal / heartbeat | 30 s / 10 s / 5 s |
| `REMOTE_DEGRADED` maximum | ≤ remaining lease TTL (≤ 30 s) then `TEARING_DOWN` |
| `PREPARING_REMOTE` per-step / total | 10 s / 60 s |
| `TEARING_DOWN`/`RECOVERING` per-step / total | 10 s / 60 s, then `FAILED_SAFE` |
| Emergency chord | `Ctrl+Alt+Shift+F12` held 2000 ms, configurable |
| Auth rate limit | 5 failures / 15 min per (client_id, account) → 15 min lockout; 20/min per source IP |
| Max concurrent auth sessions | 5 |
| Max message size (WS/IPC) | 64 KiB; input events ≤ 512 B |
| Input event rate cap | 2000 events/s, coalesce pointer motion |
| Cycle tests | 10/change (dev), 50/RC build, 100/500/1000/release (hardware) |
| Soak | 30 min (CI), 1/6/12/24/48 h (release) |

---

## 6. Crate inventory (Document 00 §51 evaluation)

Evaluated 2026-09-05 via the crates.io registry API. Rows marked **in use**
are already real dependencies of `blackroom-experiments` (exact version pinned
by `Cargo.lock`, `cargo audit`/`cargo deny check` green as of the Phase 0
checkpoint). Rows marked **evaluated** are not yet dependencies — version and
last-release date are the current crates.io values fetched this session;
licence/purpose/maintenance are evaluated by documented reputation and are
**not** re-verified against a local build (Non-Goal, plan §Non-Goals: "other
crates are only evaluated and recorded").

| Crate | Version (checked 2026-09-05) | Licence | Last release | Purpose | Privilege domain | Maintenance signal |
|---|---|---|---|---|---|---|
| `zbus` | 5.19.0 **(in use)** | MIT | 2026-08-09 | D-Bus client (Mutter/logind) | unprivileged (session), system-bus caller | Active; monthly releases |
| `serde` / `serde_json` | 1.0.229 / 1.0.151 **(in use)** | MIT OR Apache-2.0 | current | Wire/config (de)serialisation | n/a | De-facto standard, very active |
| `clap` | 4.6.6 **(in use)** | MIT OR Apache-2.0 | current | CLI argument parsing | n/a | Very active |
| `time` | 0.3.55 **(in use)** | MIT OR Apache-2.0 | current | ISO-8601 UTC timestamps | n/a | Active |
| `tracing` / `tracing-subscriber` | 0.1.44 / 0.3.23 **(in use)** | MIT | current | Structured logging | n/a | Active (tokio-rs) |
| `anyhow` | 1.0.104 **(in use)** | MIT OR Apache-2.0 | current | Error handling (binaries only) | n/a | Active |
| `tracing-journald` | 0.3.2 | MIT | 2025-11-26 | journald log sink (Doc 00 §56 logging rules) | unprivileged | Active (tokio-rs org) |
| `sd-notify` | 0.5.0 | MIT OR Apache-2.0 | 2026-03-09 | systemd `Type=notify` readiness/watchdog | unprivileged (writes to `$NOTIFY_SOCKET`) | Active |
| `pipewire` | 0.10.1 | MIT | 2026-08-19 | PipeWire client bindings (`ScreenCast` media) | unprivileged (user session) | Active (freedesktop.org org) |
| `gstreamer` (+ `gstreamer-webrtc`, same repo) | 0.25.3 | MIT OR Apache-2.0 | 2026-06-29 | Media pipeline, WebRTC encode/send | unprivileged (user session, `remote-media` child) | Active (gstreamer-rs, freedesktop.org) |
| `reis` | 0.7.1 | MIT | 2026-07-30 | Pure-Rust libei/libeis protocol (remote input) | unprivileged (user session) | Active; the only maintained pure-Rust libei binding |
| `evdev` | 0.13.2 | MIT OR Apache-2.0 | 2025-09-15 | Fallback `EVIOCGRAB`/physical-input helper only if `InputCapture` (Doc 10 topic 6) proves insufficient | **privileged** (reads `/dev/input/event*`) | Active |
| `tokio` | 1.53.1 | MIT | 2026-07-20 | Async runtime for `remote-gateway`/`axum`/`hyper` | unprivileged (network-facing) | Extremely active, industry standard |
| `schemars` | 1.2.2 | MIT | 2026-07-27 | JSON-Schema generation for IPC message validation | n/a | Active |
| `rustix` | 1.1.4 | Apache-2.0 OR Apache-2.0 WITH LLVM-exception OR MIT | 2026-02-22 | Safe POSIX syscalls (`SO_PEERCRED`, fd passing) | unprivileged | Extremely active (bytecodealliance) |
| `argon2` | 0.6.0 | MIT OR Apache-2.0 | 2026-08-27 | Password/Access-Key/recovery-code hashing | **privileged** (`remote-hostd` secrets) | Active (RustCrypto); trusted-publishing enabled |
| `totp-rs` | 6.0.0 | MIT | 2026-08-06 | RFC 6238 TOTP | **privileged** (`remote-hostd` auth) | Active |
| `ed25519-dalek` | 3.0.0 | **BSD-3-Clause** (dalek-cryptography convention, already allow-listed in `deny.toml`) | 2026-07-06 | Host identity keypair, `ControlLease` signing | **privileged** (`remote-hostd`) | Active (dalek-cryptography org) |
| `rand` / `getrandom` | 0.10.2 / 0.4.3 | MIT OR Apache-2.0 | 2026-08-25 / 2026-06-17 | CSPRNG for keys/tokens | **privileged** (secret generation) | Extremely active (rust-random) |
| `zeroize` | 1.9.0 | MIT OR Apache-2.0 | 2026-06-12 | Zero secrets on drop | **privileged** | Active (RustCrypto) |
| `secrecy` | 0.10.3 | MIT OR Apache-2.0 | 2024-10-09 | Wrapper types preventing accidental secret logging | **privileged** | Maintained but slower cadence (~2 yr since last release) — re-check for a fork/replacement at Phase 15 if this gap widens |
| `rusqlite` | 0.40.2 | MIT | 2026-08-08 | Bundled SQLite (sessions/audit), only if file-per-record proves insufficient | privileged (`remote-hostd` state dir) | Very active |
| `axum` | 0.8.9 | MIT | 2026-04-14 | Gateway HTTP/WebSocket routing | **network-facing** (`remote-gateway`) | Very active (tokio-rs) |
| `hyper` | 1.11.1 | MIT | 2026-08-28 | HTTP implementation under `axum` | network-facing | Extremely active |
| `rustls` | 0.23.43 | Apache-2.0 OR ISC OR MIT | 2026-07-29 | TLS termination in `remote-gateway` | network-facing | Extremely active; memory-safe TLS (Doc 00 §51 "well-defined security properties") |
| `ulid` | 3.0.0 | MIT OR Apache-2.0 | 2026-07-16 | `rs_/tr_/cl_` correlation IDs | n/a | Active |

### Rejected: `pam` / `pam-client`; alternative `nonstick`

Both crates the roadmap originally named for PAM integration are effectively
unmaintained:

| Crate | Last release | Total versions ever | Verdict |
|---|---|---|---|
| `pam` (`1wilkens/pam`) | 2023-11-01 | 4 | **Rejected** — no release in ~3 years, does not meet Document 00 §51 "actively maintained" |
| `pam-client` (`cg909/rust-pam-client`) | 2022-07-30 | 8 | **Rejected** — no release in ~4 years |

**Alternative recorded for Phase 15/16 evaluation:** `nonstick`
(`code.pfish.zone/crates/nonstick`), current version `0.1.2`, last released
2026-03-13, first published 2025-04-15 — actively developed but young
(< 18 months old, pre-1.0). A sibling low-level crate `libpam-sys` (`0.2.0`,
released 2025-08-03) exists from the same author. **Trade-off to resolve
explicitly at Phase 15** (Host Security Authority): `nonstick` is
actively maintained but not yet battle-tested at the scale `pam`/`pam-client`
theoretically offered before stagnating; the security review at Phase 15 must
either adopt `nonstick` with extra scrutiny (code read, minimal API surface
used) or reconsider whether `pam-auth-helper` should shell out to
`pam_unix`-compatible tooling directly instead of binding libpam from Rust.
This is recorded now, not resolved now — Non-Goal for Phase 0–1.

## 7. `deny.toml` alignment

The licences above are already covered by `deny.toml`'s allow list
(`MIT, Apache-2.0, BSD-2-Clause, BSD-3-Clause, ISC, Zlib, MPL-2.0,
GPL-3.0-or-later, LGPL-2.1-or-later, Unicode-3.0`) — no new licence exceptions
are anticipated when these crates are actually added in later phases, but
`cargo deny check` must be re-run at the phase that adds each one (Non-Goal
this phase: none of these are dependencies yet, per the plan's crate
allow-list for Phase 1 binaries: `zbus, serde, serde_json, clap, tracing,
tracing-subscriber, anyhow, time`).
