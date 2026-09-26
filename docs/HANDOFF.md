# Handoff: Blackroom Console

**Updated:** 2026-09-26 (Phase 5 stopped for reassessment; Gate FEAS-C unproven)
**Workspace or branch:** Git repo, branch `main`
**Active plan:** docs/plans/plan-20260905-phase5-physical-display-isolation.md
**Task tier:** governed
**Memory:** wing `blackroom_console`

## Current State

- Steps 1–4 complete. Watchdog path/cwd and pause-restore bugs fixed; unattended
  restore verified. PowerSaveMode blanking observed on `eDP-1` and both outputs.
- HDMI-only routine restore succeeded. Two requested kill windows (45s,
  90s) expired without SIGKILL; the 90s run lacked a visual observer.
  Both watchdogs restored; Step 5 crash-recovery remains untested.
- Earlier GNOME Shell SIGSEGV/logout remains unexplained; old logs absent.
  One later non-isolating owner-loss run removed Meta-0 without a crash
  (Shell PID stable; operator confirmed desktop usable). Gate C unproven.

## Checks

- 2026-09-26: routine exp06 restore and two no-kill attempts; Shell stable.
  Workspace cargo gates green; no 50-cycle proof; audit unchanged.
- `Blackroom_Console` indexed (fast); experiment binaries excluded from graph.

## Decisions

- Keep exp06/exp07 independent; defer GnomeBackend. No weakened Gate FEAS-C.

## Blockers

- Doc 00 §49 / Doc 10 §49 stop-and-report applies to reported Mutter instability.
  No more live isolation here without a new safety review and approval.

## Next Actions

1. Never require chat while blank; operator reports after restore. The
  opt-in machine-gated diagnostic is prepared offline, not live-approved.
2. Investigate old crash; obtain safety approval for any live test. Phase 6
  and Gate FEAS-C remain blocked.
