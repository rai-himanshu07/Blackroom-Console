# GNOME session discovery and capability detection (Phase 3)

Turns Phase 0-1's read-only research (`exp01_session_discovery`,
`exp02_mutter_inventory`, `docs/gnome/{api-inventory,feasibility-research,
capability-report}.md`) into real, still non-mutating production code. Every
call this phase adds is read-only (`Introspect`/`Get`/`ListSessions`/
`GetCurrentState`-style) — never `CreateSession`, `RecordVirtual`,
`ApplyMonitorsConfig`, or `ConnectToEIS` (those remain Phase 4+, assessment
C2).

## Session discovery (`crates/blackroom-gnome/src/mutter/session.rs`)

`session::discover_session()` ports the algorithm proven in
`blackroom-experiments::session::discover()` (Experiment 1) as real
production code: it calls `login1.Manager.ListSessions` then, for each
candidate, reads the `Session` object's `Type`/`Class`/`Seat`/`User`/`Active`
properties, and selects the **unique** session matching
`Type=wayland ∧ Class=user ∧ Seat=seat0 ∧ User=<uid> ∧ Active=true` — failing
closed (never "the first session") on zero or ambiguous (more than one)
matches (Doc 05 §12–14).

The current process's own uid is read via `rustix::process::getuid()`, not an
environment variable (Doc 05 §12 explicitly forbids identity-by-environment-
variable) and not a shell-out (unlike Experiment 1's `id -u`, which existed
specifically to avoid a new Phase 1 dependency). Doc 05 §14's Wayland/
XWayland corroboration is implemented as a secondary, non-fatal check
(`XDG_SESSION_TYPE`/`WAYLAND_DISPLAY` in the agent's own process environment,
logged via `tracing::warn!` on mismatch) — the `login1`-based selection above
remains the sole source of truth.

**Live-verified on this host** (which has two sessions on `seat0`, Phase 0-1
finding): `discover_session()` correctly selects session `"2"`, not `"1"`,
proving the disambiguation logic — not "the first match" — is what runs.

## Capability detection (`crates/blackroom-gnome/src/mutter/capability.rs`)

`capability::detect()` produces all 16 Doc 20 §8 constants using the Doc 00
§35 five-tier vocabulary (`SUPPORTED, SUPPORTED_WITH_LIMITATIONS,
EXPERIMENTAL, UNSUPPORTED, UNKNOWN` — `UNKNOWN` never activates). D-Bus-
derived constants use targeted `zbus::blocking::Proxy` calls against the
exact interfaces/paths `docs/gnome/api-inventory.md` already confirmed exist
(not Experiment 2's generic introspection-XML scanner, which was a
Phase-1-only technique for *discovering* unknown interfaces). Non-D-Bus facts
(`OS_SUPPORTED`, `GNOME_SUPPORTED`, `SYSTEMD_SUPPORTED`, `GPU_CAPABLE`) port
Experiment 0's proven techniques: `/etc/os-release`, `dpkg-query`, and
`/proc/modules` reads.

`DISPLAY_CONFIG_CAPABLE` calls the real `DisplayConfig.GetCurrentState`
method (not just a property `Get`) using the exact typed signature
Experiment 2 already proved live against Mutter 50.1:
`ua((ssss)a(siiddada{sv})a{sv})a(iiduba(ssss)a{sv})a{sv}`.

**Live-verified on this host: every one of the 16 classifications exactly
reproduces `docs/gnome/capability-report.md`'s existing table**, and the
roadmap's Phase 3 gate (`OS_SUPPORTED, GNOME_SUPPORTED, WAYLAND_SUPPORTED,
SYSTEMD_SUPPORTED, SESSION_FOUND` all `SUPPORTED`) passes.

## `SessionInfo`/`Capability` revision (evidence-cited, `GnomeBackend` unchanged)

The Phase 2 placeholder `Capability` enum (`VirtualDisplay, RemoteInput,
PhysicalInputIsolation`) and `SessionInfo.capabilities: Vec<Capability>` are
removed. A presence-only `Vec` cannot express Doc 00 §35's five tiers —
`UNSUPPORTED` and `UNKNOWN` both must not activate but mean different things.
Replaced by `CapabilityTier`/`CapabilityReport` above. Capability detection is
**not** a 14th `GnomeBackend` trait method: Doc 05 §8's interface lists
exactly 13 operations and Doc 05 §9 treats capability detection as a separate
startup-time concern, so the already-reviewed Phase 2 trait is untouched.
`SessionInfo` gained `uid: u32` and `active: bool` (Doc 05 §12's explicit
field list; `discover_session` already computed both). Blast radius of the
removal was confirmed zero via repo-wide grep before making the change
(`blackroom_gnome::Capability`/`SessionInfo` were used only inside
`blackroom-gnome` itself; `blackroom-core`'s own `lease::Capability{View,
Control}` is a separate, unrelated enum per conflict C6).

## `gnome-session-agent` and `AgentState`

The new `crates/gnome-session-agent` runs as the logged-in user (never root,
Doc 05 §16). Its startup sequence (`startup::start`) calls
`session::discover_session()` then `capability::detect()`, transitioning its
own `AgentState` (Doc 05 §20, 11 values: `SessionUnknown, SessionReady,
Preparing, VirtualDisplayReady, PhysicalDisplayIsolated,
PhysicalInputIsolated, RemoteReady, RemoteActive, Restoring, Restored,
Failed`) to `SessionReady` when the Phase 3 gate passes, or failing closed to
`Failed` on a non-Wayland/non-GNOME session or a failed gate — never a silent
fallback. If GNOME is not yet ready (`SessionUnknown`), the agent retries with
a bounded backoff (Doc 06 §29) instead of failing permanently.

**`AgentState` is `gnome-session-agent`'s own local-subsystem readiness
state; `blackroom_core::state::State` (Doc 07 §4–5, Phase 2, 11 canonical
cross-host states) remains the sole cross-host authority (owned by
`remote-hostd`, Phase 11+), and `AgentState` never substitutes for it.** Doc
05 §21's informal `LOCAL_ACTIVE → PREPARING_REMOTE → REMOTE_ACTIVE →
RECOVERING → LOCKED` global-state sketch is already covered by conflict **C3**
(assessment §5); no new conflict number is filed for it.

## `agent.sock` (`crates/gnome-session-agent/src/ipc.rs`)

Reuses the already-decided IPC design (architecture.md §3): a Unix domain
socket verified via `SO_PEERCRED`. `std::os::unix::net::UnixStream::
peer_cred()` was directly compile-tested against the pinned toolchain
(`rustc 1.96.0`) and confirmed **still gated behind the unstable
`peer_credentials_unix_socket` feature** — the standard library
documentation page appeared to list it as stabilized at `1.10.0`, but that
was misleading (nightly-built docs); this was verified empirically, not
assumed. `rustix::net::sockopt::socket_peercred` is used instead, preserving
`#![forbid(unsafe_code)]`.

The authorization check (`ipc::peer_authorized`) currently accepts only the
exact expected uid (the agent's own). No `remote-hostd` exists yet to extend
this to architecture.md §6.2's group-based ACL (`agent.sock` owned
`remote-hostd:blackroom-session`), and no message protocol exists yet either
— accepting and immediately closing a verified connection proves the
mechanism without inventing protocol semantics early. **No password, TOTP
value, Access Key, or recovery code is ever parsed here** (Doc 05 §18).

Until a packaging/setup phase creates `/run/blackroom-console/` with the
right ownership, the socket binds under the agent's own `$XDG_RUNTIME_DIR`.

**Live-verified end to end**: a real `socat` connection from the same user is
accepted (peer uid logged) and closed; a wrong-uid peer is rejected (unit
test, since simulating a different real uid needs a second user account).

## systemd user unit (`systemd/user/gnome-session-agent.service`)

`PartOf=graphical-session.target` was already decided (assessment §6.2),
which also flagged the exact `After=` target ("`gnome-session-wayland.target`/
Ubuntu equivalent") for Phase 3 to verify. Live `systemctl --user list-units`
output on this host shows **no unit literally named
`gnome-session-wayland.target`**; the closest desktop-environment-agnostic,
standard systemd/freedesktop convention target is `graphical-session.target`
("Current graphical user session"), which is active here — used instead.
`Type=simple`, not `notify` (the binary does not call `sd_notify` yet; the
`sd-notify` crate remains evaluated-only). The unit was live-smoke-tested via
a temporary `systemctl --user start` (binary path substituted, unit structure
unchanged) and reached `active (running)` under `session.slice`; cleaned up
afterward.

## Live-host verification (`crates/blackroom-systest`)

Instantiates the already-declared `AGENTS.md` convention ("Real-GNOME system
tests live in `crates/blackroom-systest`, are `#[ignore]`d, and run only with
`BLACKROOM_SYSTEST=1` on a prepared host") for the first time — Phase 3 is
the first phase needing a live-host check. Two tests, both host-preflighted
(`BLACKROOM_SYSTEST=1` + `XDG_SESSION_TYPE=wayland`): session selection
(demonstrating the two-seat0-session disambiguation on this host) and the
Phase 3 capability gate. Plain `cargo test --workspace` runs zero of these
tests; omitting `BLACKROOM_SYSTEST=1` fails closed even with `--ignored`.

## Non-goals reaffirmed

No virtual display/capture (Phase 4), no physical display isolation (Phase
5), no remote input or physical input isolation (Phases 6–7), no GNOME lock/
same-session validation (Phase 8), no `remote-hostd` (does not exist yet), no
`blackroom-ipc` crate extraction, and no complete concrete `GnomeBackend`
implementation — later phases' `mutter/*.rs` modules (`remote_desktop.rs`,
`display_config.rs`, `eis.rs`, `lock.rs`) are what eventually assemble into
one.
