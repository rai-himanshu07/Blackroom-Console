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
- [x] Phase 9: agent-owned fake activation/teardown transaction; signed grants
  are acknowledged only after observations; verified `LOCAL_LOCKED` or explicit
  `FAILED_SAFE` on failed restoration. Hostd/agent process-loss and idle lease
  expiry checks. A protected pending marker precedes grants; verified fake
  cleanup clears it after revoke or hostd EOF. Agent loss leaves `FAILED_SAFE`.
- [x] Phase 10: separate offline emergency executable persists a stop marker
  and epoch without hostd cooperation; atomic marker publication handles
  concurrent requests. Agent observes it; gateway denies stale control.
- [x] Phase 11 preparation: protected host identity and atomic epoch store,
  private-bootstrap-delivered, reusable process-lifetime synthetic Start proof,
  signed time-bounded control lease and persisted revocation; wrong proof and
  stale epochs refused. The proof is not one-time or user authentication.

## Open Work And Gates

- [ ] Physical-input mechanism and emergency hardware/privilege path: FEAS-E
  and FEAS-G unproven; no installed observer or distinct UID.
- [ ] Real GNOME lock, unlock, session continuity and topology restore:
  FEAS-A/C/F/H unproven. Connected-HDMI restoration remains stopped.
- [ ] Hostd-backed product identity, PAM/MFA, credential management, rate
  limits, service provisioning, explicit recovery-marker clearance and full
  durable real safety-state reconciliation; Phase 11 certification awaits
  Phase 10 Go. An agent-death marker needs independent recovery evidence;
  there is no safe automatic or manual clearance command yet.

## Validation

Focused fake, signed host/agent IPC, independent emergency, repeated grants,
EOF and cross-process death/restart tests. One broad workspace checkpoint and
one independent security/recovery review at the integration boundary.