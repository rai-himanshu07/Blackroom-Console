# Handoff: Blackroom Console

**Updated:** 2026-09-27 (Phase 6 offline PoC exception; Gate FEAS-C unproven)
**Workspace or branch:** Git repo, branch `main`
**Active plan:** docs/plans/plan-20260926-phase6-remote-input-readiness.md
**Task tier:** governed
**Memory:** wing `blackroom_console`

## Current State

- Steps 1–4 complete. Watchdog path/cwd and pause-restore bugs fixed; unattended
  restore verified. PowerSaveMode blanking observed on `eDP-1` and both outputs.
- HDMI-only SIGKILL: Shell survived; Mutter restored HDMI logically but
  DPMS stayed OFF until watchdog exp07 unblanked. Brief built-in flicker
  accepted without repeat capture; content unknown. Gate C open.
- Earlier GNOME Shell SIGSEGV/logout remains unexplained; old logs absent.
  One later non-isolating owner-loss run removed Meta-0 without a crash
  (Shell PID stable; operator confirmed desktop usable). Gate C unproven.

## Checks

- Phase 6 fake input/lease time checked; exp07 restore identity guarded offline.
- `Blackroom_Console` indexed (fast); experiment binaries excluded from graph.

## Decisions

- Phase 6 offline only: authority is caller-supplied (no hostd), no live EIS.
  One eDP-only diagnostic is operator-authorized, pending exact preflight;
  prior review NO-GO noted. Product stop/Gate C unchanged; no run yet.

## Blockers

- Doc 00 §49 / Doc 10 §49 still block live/product progress after Mutter
  instability. FEAS-D and Phase 7 also remain unproven.

## Next Actions

1. Bind the offline input authorization snapshot and verifier to trusted
  agent state without startup wiring; fake-test refusal on state changes.
2. Separate review/approval required before live GNOME EIS or input.
