# Plan: Offline Phase 7–11 Integrated Slice

**Status:** implemented in part; live gates unchanged (2026-09-27)
**Tier:** governed (security and recovery); execution-first amendment in master roadmap.

## Goal

Keep the hostd → agent → gateway → browser path observable without live
GNOME/device mutation. Do not activate a product remote mode, install services,
grab devices, inject input, or promote any feasibility/security gate.

## Implemented Offline

- [x] Phase 7: fake physical-input isolation including hotplug inheritance,
  remote-channel separation, emergency visibility and rollback. Research
  records why no real mechanism is selected.
- [x] Phase 8: synthetic lock observation and same fake-session check;
  false-success lock calls block activation.
- [x] Phase 8 read-only preparation: real selected-session identity is checked
  again before querying ScreenSaver `GetActive` and login1 `LockedHint`.
  Owner PID provenance is checked; this host's user-bus owner has no login1
  session, so false/false is INDETERMINATE. FEAS-A remains open.
- [x] Phase 8 bounded supervised lock-only diagnostic: one login1-targeted
  `Lock` returned, ScreenSaver active preceded login1 LockedHint true; operator
  saw the local lock screen without desktop content and manually recovered the
  same session. No remote unlock, capture/EIS continuity or FEAS-A PASS.
- [x] Phase 9: agent-owned fake activation/teardown transaction; signed grants
  are acknowledged only after observations; verified `LOCAL_LOCKED` or explicit
  `FAILED_SAFE` on failed restoration. Hostd/agent process-loss and idle lease
  expiry checks. A protected pending marker precedes grants; verified fake
  cleanup clears it after revoke or hostd EOF. Agent loss leaves `FAILED_SAFE`.
- [x] Phase 10: separate offline emergency executable persists a stop marker
  and epoch without hostd cooperation; atomic marker publication handles
  concurrent requests. Agent observes it even with synthetic hostd SIGSTOPped;
  gateway denies stale control.
- [x] Phase 11 preparation: protected host identity and atomic epoch store,
  private-bootstrap-delivered synthetic Start proof rotated only after an
  acknowledged grant, signed time-bounded lease and persisted revocation;
  wrong and previously used proofs and stale epochs are refused. This is not
  user authentication or a distinct-UID service boundary.
- [x] Offline authority abuse slice: five wrong/replayed hostd Start proofs
  block further Starts for that process; an active fake grant is revoked,
  with a durable stop on unverified agent acknowledgement. The loopback
  gateway requires a public fake demo code and caps bad codes per process;
  separated hostd independently requires the code alongside its private
  proof before signing a grant. The browser exposes blocked, recovery-required
  and disconnected states. Neither check is production authentication.

## Open Work And Gates

- [ ] Physical-input mechanism and emergency hardware/privilege path: FEAS-E
  and FEAS-G unproven; no installed observer or distinct UID.
- [ ] Real GNOME lock, unlock, session continuity and topology restore:
  FEAS-A/C/F/H unproven. Connected-HDMI restoration remains stopped.
- [ ] Hostd-owned product authentication/session binding, PAM/MFA, credential
  management, durable per-identity abuse policy, service provisioning,
  explicit recovery-marker clearance and full real safety-state
  reconciliation; Phase 11 certification awaits Phase 10 Go. The demo-code
  counter resets on gateway restart, and the proof counter on hostd restart;
  neither constitutes a production rate limit. An agent-death marker needs
  independent recovery evidence; no safe clearance command exists yet.

## Validation

Focused fake, signed host/agent IPC, independent emergency, repeated grants,
EOF and cross-process death/restart tests. One broad workspace checkpoint and
one independent security/recovery review at the integration boundary.
2026-09-27 offline abuse increment: focused hostd replay/failure tests,
separated-process gateway test, one workspace cargo test checkpoint, web
typecheck/build and a synthetic browser Start/input/revoke/disconnect check
passed. Touched-crate Clippy passed with three verified baseline warnings
suppressed; plain `-D warnings` still fails on those existing warnings.