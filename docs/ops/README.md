# docs/ops/

Operational procedures for running Blackroom Console and its feasibility experiments
safely on real hardware.

- `experiment-safety.md` — out-of-band recovery procedure required before any experiment
  that changes display or input state (assessment §8).

Offline simulation state (never a live host): hostd appends security events to
`audit.log` in its state directory (owner-only, 1 MiB then one rotation; static
codes, epochs and principal ids only, never proofs, demo codes, input grants
or session tokens; grant issuance fails closed if it cannot be logged). The
read-only `blackroom --state-dir <abs path> status | logs [--tail N] | doctor`
CLI reports epoch and stop markers, prints valid audit lines, and checks
ownership/modes. It never creates, locks or changes state and refuses
symlinked, relative or loose-permission directories.

Runbooks for the released product (incident response, recovery) live in
`docs/plans/Detailed_Project_Plan/21_OPERATIONAL_RUNBOOK_RECOVERY_AND_INCIDENT_RESPONSE.md`;
this directory holds the project's own dev-workstation procedures.
