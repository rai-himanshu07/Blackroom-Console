# Handoff: Blackroom Console

**Updated:** 2026-09-27 (implementation-first; Gate FEAS-C stopped)
**Workspace or branch:** Git repo, branch `main`
**Active plan:** docs/plans/plan-20260927-offline-phase7-11.md (Phase 6 PoC remains partial)
**Task tier:** governed
**Memory:** wing `blackroom_console`

## Current State
- Steps 1–4 complete. Watchdog path/cwd and pause-restore bugs fixed; unattended
  restore verified. PowerSaveMode blanking observed on `eDP-1` and both outputs.
- Connected-HDMI run: exp06 FAIL (pre-repair eDP+HDMI active); local reapply
  corrected logical topology; first exp07 PASS, second exp07 FAIL after raw
  HDMI vanished despite plugged cable and one re-seat. eDP/SSH usable;
  Shell/session/power stable, no timers, service disabled/inactive. Gate C STOP.
- Earlier GNOME Shell SIGSEGV/logout unexplained; old logs absent.

- Separated HTTP idle/start/input/revoke; verified hostd EOF cleanup permits
  restart at a new epoch; agent loss stays FAILED_SAFE. Scratch remnants in `/tmp`.
- Offline Phase 7–11: fake input/lock/transaction, signed lease and rotating
  Start proof, emergency epoch, idle expiry. Durable pending marker clears
  after verified fake cleanup; unverified restart reports FAILED_SAFE.

## Decisions
- Browser supports EPHEMERAL, PERSISTED and opt-in SEPARATE local processes.
  No installed service, distinct UID or live EIS authority is enabled.
- Emergency marker never auto-clears; fake lock/physical isolation are not
  live safety evidence. Phase 10 Go and Phase 11 certification remain blocked.
- All run approvals consumed; Gate C stop holds. Local reapply and versioned
  backup observed once; privacy and stable unassisted restore unproven.
- Continue adjacent approved offline work without step-by-step reapproval;
  build host, gateway and UI local paths with affected checks. Stop hook is
  disabled; doctor is manual only. Live gates still block activation, not code.

## Blockers
- Connected HDMI lost from kernel inventory during cleanup; Gate C and product
  activation stopped. FEAS-A/D/E/F/G/H remain unproven.

## Next Actions
1. Diagnose kernel HDMI loss; exp07 FAIL exit fixed but not live-retested.
