# docs/security/

Security architecture and decision records for Blackroom Console.

- `architecture.md` — the stack, naming, IPC, cryptography, and numeric-default decisions
  from assessment §6, recorded verbatim as project decisions, plus the evaluated crate
  inventory (licence, last release, privilege domain) required by Document 00 §51.

- `authentication.md`, `credential-lifecycle.md`, `threat-model.md` — the Phase 11 login authority:
  what a login needs, how each credential is created, rotated and revoked, and what is and is not
  defended. `sec-gate-report.md` — gate readings SEC-A to SEC-E and SEC-J and the RT-AUTH, RT-SESSION,
  RT-EPOCH and RT-FILE matrix.

Threat model and abuse cases live in the spec
(`docs/plans/Detailed_Project_Plan/09 — Threat Model, Security Boundaries & Abuse Cases.md`);
this directory holds the project's own architecture decisions and evidence, not a restatement
of the spec.
