# Handoff: Blackroom Console

**Updated:** 2026-09-26 (Phase 5 stopped for reassessment; Gate FEAS-C unproven)
**Workspace or branch:** Git repo, branch `main`
**Active plan:** docs/plans/plan-20260905-phase5-physical-display-isolation.md
**Task tier:** governed
**Memory:** wing `blackroom_console`

## Current State

- Steps 1–4 complete. Watchdog path/cwd and pause-restore bugs fixed; unattended
  restore verified. PowerSaveMode blanking observed on `eDP-1` and both outputs.
- On the `HDMI-1`-alone routine run, the operator saw the external panel
  blank; the watchdog restored before a deliberate kill. Date-keyed exp06
  evidence now conflicts with exp07's HDMI-only report. Step 5 remains open.
- Earlier GNOME Shell SIGSEGV/logout remains unexplained; old logs absent.
  One later non-isolating owner-loss run removed Meta-0 without a crash
  (Shell PID stable; operator confirmed desktop usable). Gate C unproven.

## Checks

- 2026-09-26: one approved owner-loss probe returned PARTIAL, no crash;
  workspace cargo gates green. No exp06 or 50-cycle proof; audit unchanged.
- `Blackroom_Console` indexed (fast); experiment binaries excluded from graph.

## Decisions

- Keep exp06/exp07 independent; defer GnomeBackend. No weakened Gate FEAS-C.

## Blockers

- Doc 00 §49 / Doc 10 §49 stop-and-report applies to reported Mutter instability.
  No more live isolation here without a new safety review and approval.

## Next Actions

1. Investigate the compositor crash from retained diagnostics or a separately
  prepared host; preserve the uncertainty about whether exp06 was killed.
2. Reassess architecture and obtain safety review/approval before any new
  live test. Do not advance Phase 6 or promote Gate FEAS-C.
