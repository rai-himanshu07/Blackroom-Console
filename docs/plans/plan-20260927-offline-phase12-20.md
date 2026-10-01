# Plan: Offline Phase 12–20 Increments

**Status:** Phase 12–13 and synthetic Phase 15–16 partial; product gates open (2026-09-27)
**Tier:** governed for protocol/recovery changes; no live mutation approved.

## Goal

Extend the existing offline hostd → agent → gateway → browser path where it
provides testable value. Do not activate remote mode, bind a LAN listener,
install services, provision credentials, touch GNOME/input devices, or promote
a feasibility, security, Go/No-Go or release gate from simulation.

## Implemented Offline

- [x] Phase 12: loopback HTTP Start/revoke/status/input with strict JSON, fixed
  Host header and allow-listed supplied Origin (Origin-less CLI is accepted),
  shared 64-KiB message and 512-byte input bounds.
  Each acknowledged fake grant gets a random input ID; the gateway carries it
  in an HttpOnly SameSite=Strict cookie, never in status JSON, and input
  requires that cookie and the exact next sequence before the fake sink is
  called. Revoke clears the cookie.
  Invalid, replayed, gapped and stale envelopes leave the event log unchanged.
  Separated hostd also requires the active signed lease epoch and its own
  increasing sequence before it forwards fake input to the agent.
- [x] Phase 13 slice: browser consumes the current grant/sequence in memory,
  disables control before initial status and on missing binding, ignores
  status responses older than a local command, and renders locked/failed-safe/
  disconnected states without a live desktop preview.
- [x] Phases 15–16 synthetic slice: a real offline hostd/agent child-process
  test refuses duplicate input and a prior grant after revoke/restart; focused
  adversarial HTTP tests cover oversized requests without authority changes.
- [x] Phase 18 read-only preparation: an explicit one-shot agent command
  reports current logind/GNOME capability tiers and the Phase 3 session gate
  while always leaving product remote mode disabled. This does not measure
  display privacy, real input, recovery or compatibility matrix support.
- [x] Phase 11/15 observability slice: hostd writes an append-only, owner-only
  `audit.log` (host start, refused auth, grant issued, renew refused, grant
  revoked with cause) with no secret-bearing fields; grant issuance fails
  closed when it cannot be logged. Tests grep the log for proofs, demo code and
  input grants. A read-only `blackroom` CLI (`status`, `logs`, `doctor`) reads
  the offline state directory without creating or locking anything.
- [x] Phase 12 slice: separated hostd owns an in-memory, epoch-bound, 5-minute
  authentication-session registry (`remote-hostd::auth`). Only a
  `CredentialVerifier` adapter can open a session; the sole adapter is the
  fake demo-code/rotating-proof one. `start_for` takes the lease user/client
  from the session, caps expiry at the session's and refuses stale-epoch or
  expired sessions before any recovery marker is written. Revoke, agent-refused
  input and the abuse limit end the session. Session-bound leases last the
  documented 30 s (`CONTROL_LEASE_TTL`); a `Renew` control command re-signs
  the active lease for the same epoch and session only for the grant holder,
  the gateway forwards a browser heartbeat every 10 s (separated mode only),
  and silence lets the lease lapse (real-process test, ~70 s). Revoke is the
  logout (it ends the session and grant); the 5-minute session end is capped
  into the lease expiry, so no separate logout command exists. Accepted Start
  and Renew replies carry display-only deadlines; the gateway shows remaining
  time to the holder only and the page counts down. The session
  credential never leaves
  hostd. Hostd also mints the input grant (returned on Start, echoed on every
  Input, dead with the session) instead of the gateway in SEPARATE mode; the
  control protocol gained that field. In-process modes keep gateway-issued IDs.

- [x] Phase 12 credential adapter (2026-10-01, not wired into the simulation service):
  `remote-hostd::totp` is a `CredentialVerifier` for RFC 6238 TOTP (HMAC-SHA1 checked against the
  RFC vectors, 6 digits, 30 s, one step of skew) with replay protection persisted across restarts,
  per-(account, client) failure lockout with doubling delay (documented 5 failures / 15 min policy),
  an escalating per-account lockout across clients, uniform refusals for unknown accounts with
  bounded limiter tables, zeroised secrets, and an owner-only `totp-credentials` file (0600, no
  symlinks, unknown or duplicate entries refused) whose read-modify-write is serialised by a
  `totp-credentials.lock` flock (CLI and host). Independently reviewed; all confirmed findings fixed. `blackroom enroll --account <name>` adds an account and prints
  the secret once; `blackroom accounts` lists names. Not done: wiring this into the hostd service
  and gateway login flow, PAM, device keys, and any real enrollment on this host.
- [x] Isolation wiring (2026-10-01, opt-in, simulation host only): hostd holds the daemon lease via
  `--emergency-socket` (see docs/security/emergency-daemon.md "Who holds the lease"); proven with
  in-test fake daemon/agent processes and against the real daemon loop with fake devices. Not done:
  gateway flag and longer Start deadline, agent-side verification, any live run.
- [x] Phase 10 component (2026-10-01): `remote-emergencyd` (see
  `docs/security/emergency-daemon.md`), with `blackroom emergency-status --socket <path>`.

## Open Dependencies

- [ ] Phase 12: product credential verification (PAM/TOTP/device key),
  per-account rate limits, device revocation commands,
  privilege separation, hardened LAN HTTPS/WebSocket/IPC and CSRF/origin
  enforcement. In-process, persisted and one-shot `--offline-sim-host` paths
  still issue a fixed synthetic lease without a session. The public demo code
  and the loopback cookie (not port-isolated) are not credentials.
- [ ] Phase 13: real browser identity, reconnect, trusted device and remote
  interaction flows require the authenticated product protocol; synthetic
  controls are not a desktop client.
- [ ] Phase 14: WebRTC media, data channels and network traversal require
  proven live capture, input, authorization and recovery on a supported host.
  No fake media stream is represented as a working remote desktop.
- [ ] Phases 15–16: full live lifecycle, physical privacy/input recovery,
  cross-user privilege tests and red-team gates remain unverified.
- [ ] Phases 17–18: performance and compatibility measurements require a real
  supported path. Unknown hardware/display configurations stay unsupported;
  AMD remains UNKNOWN for v1.
- [ ] Phases 19–20: no service installation, setup, upgrade, signed package or
  release candidate until supported gates and their operational evidence exist.

## Verification And Next Boundary

Run focused gateway/web checks per change; one workspace checkpoint only for
cross-crate integration and one independent review for a new high-risk
mechanism. Live system tests stay ignored. Remaining hostd-side work needs a
real credential verifier (PAM/TOTP/device key), whose design, privilege
separation and per-(client, account) rate limits need an explicit operator
decision on credential provisioning; do not add a fake limiter first. Offline
candidates meanwhile: sessions for the in-process modes and Phase 13 UI
state. FEAS-C remains
STOP after the HDMI connector-loss incident; no unattended live experiment is
authorized by this plan.