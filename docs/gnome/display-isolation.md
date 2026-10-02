# Physical display isolation findings (Phase 5, Gate FEAS-C)

**Outcome (2026-10-01):** FEAS-C is recorded **PASS-WITH-LIMITS only for the declared single-display layout: the
built-in `eDP-1` panel alone, no external output connected**. An independent read-only review reached the same
verdict. Connected HDMI, hotplug during isolation, a mode matrix, 50 cycles and abnormal-termination recovery
are **not claimed**. **One bounded eDP-only owner-kill with the input grab was observed on 2026-10-01 (Gate F, one run, see below);
physical-isolation activation stays disabled for the other Phase 9 to 11 items.** Plan:
`docs/plans/plan-20260905-phase5-physical-display-isolation.md`; supported-layout decision 2026-09-28 in the
roadmap. Procedure and recovery: `docs/ops/experiment-safety.md` sections 2, 3 and 7.

## What was proven on the declared layout

| Claim | Evidence |
|---|---|
| Mutter accepts a topology with zero physical monitors active (only the virtual monitor) | `ApplyMonitorsConfig` (`Temporary`) accepted in every eDP-only run; raw `eDP-1` stays in `GetCurrentState` monitors, absent from every logical monitor. The isolated state was read back by hand during the runs, not persisted by `exp06` pause mode |
| "Disabled in the logical topology" is not "blank": a disabled `eDP-1` can keep a frozen last frame | 2026-09-05 live finding (Phase 5 plan log; no evidence artifact); fixed by pairing every disable with `PowerSaveMode` OFF (blank before the topology change, un-blank after restore) |
| The panel shows no desktop content during isolation | operator observation: clean black, not frozen (2026-09-05 log; 2026-10-01 run: fully black, phone photo kept by the operator, not in the repository, no hash or capture time). Brief flicker at the transition was accepted by the operator and not captured |
| The original topology is restored, by hash and fields | 2026-10-01 only: `exp07` PASS with `topology_matches` and `configuration_hash_matches` true on a version-one backup, and the `exp06` post-Stop `final_state`. The earlier eDP-only run (2026-09-27-2) had no hash; the 2026-09-27 hash results are connected-HDMI runs |
| The restore watchdog fires unattended and restores | 2026-09-05, 09-26, 09-27 and 2026-10-01 runs. Timer accuracy was 1 minute until 2026-10-01 (`d36f201`), so earlier firing could be late (67 s once); on 2026-10-01 it fired 46 s after arming |
| Owner cleanup (ScreenCast Stop) leaves the original topology | 2026-09-27-2 and 2026-10-01: `final_state` raw and logical `eDP-1` only, `Meta-0` gone, no post-Stop repair needed |

Assessment section 7.3: (a) Mutter accepts zero physical monitors: yes on this layout. (b) What Mutter does when
the owner dies: observed once on an HDMI-only layout (Mutter restored HDMI logically, DPMS stayed OFF until the
watchdog), not on eDP-only. On a sole panel that outcome would be a blank screen until the watchdog fires.

## Limits and what is not claimed

- **Connected HDMI is unsupported.** In four connected-HDMI runs (2026-09-27 to 2026-09-27-5) HDMI-1 became
  active after `Stop` (persisted in two runs, observed in the others) and in the last the connector vanished
  from Mutter and the kernel (still reported disconnected after a re-seat and a reboot). Cause unknown. A
  connected or newly detected output must block activation or trigger safe teardown; that policy exists only in
  the offline fake and was not observed live. The "no external output" precondition rests on a port that failed
  after our own tests, so the layout may be hardware-forced.
- **Hotplug during isolation, mode matrix, HiDPI, 50 cycles, other GPUs (AMD), frames captured from the virtual
  monitor while isolated on this layout**: not run, not claimed.
- **Privacy depends on `PowerSaveMode` staying OFF.** The black panel comes from DPMS; the 2026-09-05 frozen
  frame shows the logical disable alone does not blank it. Only a ~45 s window was observed, with no input,
  idle, lid or lock interaction.
- **Abnormal termination on eDP-only** (owner killed while isolated, with the real input grab held): observed once on
  2026-10-01, see the section below. Recovery relies on the armed watchdog and SSH.
- **Unexplained historical Shell SIGSEGV** near virtual-monitor removal (reported before 2026-09-26; the
  operator cannot say whether exp06 was killed first) was not reproduced in about 12 later supervised runs
  (two on this layout, none abnormal). That does not bound recurrence: about five clean isolation runs preceded
  it. Recording PASS-WITH-LIMITS is an explicit, scoped acceptance of that risk by the operator; Mutter
  stability stays a Go/No-Go criterion (Phase 10).
- This layout is a hybrid-GPU laptop whose panel is driven by Intel; a single-monitor desktop or an NVIDIA/AMD
  primary output cannot inherit this evidence.

## Decisions

1. **Scope:** FEAS-C is closed per declared layout, not globally. Any other layout needs its own evidence.
2. **Backup contract:** `exp06` writes a version-one backup (outputs with enabled state, primary, pinned
   SHA-256-derived hash of identity, mode and enabled state); `exp07` requires the hash and exact logical fields
   (position, scale, transform, primary) for it and falls back to exact identity and topology checks for older
   backups. Only the original live Shell and session may use a backup.
3. **Recovery envelope:** watchdog timer plus identity-guarded restore plus a second cleanup timer before
   ScreenCast Stop; `PowerSaveMode` is restored last.

## Evidence that would strengthen this

A committed or hashed photo with capture time; a persisted isolated-state snapshot (raw, logical,
`PowerSaveMode`); one eDP-only run that captures frames from `Meta-0` while isolated and has the operator use the
keyboard and touchpad with `PowerSaveMode` watched; later, one eDP-only self-kill under its own approval.

Evidence: `docs/experiments/evidence/exp06/` (2026-09-26-4, 2026-09-27-2 to 5, 2026-10-01) and `exp07/`.

## Gate F: eDP-only owner-kill with the input grab (2026-10-01, one run)

Evidence `docs/experiments/evidence/exp06/2026-10-01-2/observation.md`. The probe isolated eDP-1 (virtual-only
`Meta-0`, DPMS 3), the real `remote-emergencyd` grabbed event2-5, then the probe SIGKILLed itself. Mutter removed
the virtual monitor and put eDP-1 back in the logical topology within the same second; the daemon had released the
grab by the first check 15 s later (socket closing, not the 60 s lease); the panel stayed dark (DPMS 3) until the
45 s watchdog ran `exp07_restore` (PASS, topology and hash match). Shell PID unchanged, no crash line. The operator
reported no desktop content during the blank, the picture back within about a minute, working input and an intact
desktop. Limits: one run, same uid, the owner was the exp06 probe (not the product agent), no remote input, capture
or lock in the run, the recovery time is the watchdog interval (about 45 s of dark panel), exact release times not
recorded, no repeat, no other layout.

## Shell SIGSEGV reproduced (2026-10-01 23:02, integrated probe) - stop condition

The integrated probe (remote session and observer page, virtual monitor with a capture consumer, isolation, input grab,
then the orderly restore) crashed GNOME Shell with SIGSEGV inside the restore `ApplyMonitorsConfig`
(`meta_monitor_manager_apply_monitors_config` -> `meta_monitor_manager_rebuild` -> a `monitors-changed` handler), one
second after the remote session's EIS socket closed; the session was lost. Evidence and stack summary:
`docs/experiments/evidence/exp06/2026-10-01-3/observation.md`. The earlier statement that the historical SIGSEGV had
not been reproduced no longer holds. The Mutter-instability stop (Doc 00 section 49) applies again: no further live
display, input or session experiments until the cause is investigated and the operator approves a bounded bisect.
The root cause is reproduced (5/5) on a throwaway headless Shell with high confidence for this Mutter build (50.1-0ubuntu2.4): the
ScreenCast virtual-stream `monitors-changed-internal` handler dereferences a NULL stage view when the stream is enabled
(a PipeWire consumer is streaming) and the virtual monitor has no logical monitor, which is exactly what a physical-only
restore config produces (same on upstream main). Earlier clean runs had no streaming consumer at restore. Rule: while a
virtual-monitor stream may be enabled, every applied config must keep the virtual monitor (extra logical monitor), and
the session is stopped afterwards; verified safe 8/8 on the headless Shell (scale 1.0 only) and once on the real session
(2026-10-02 integrated probe: consumer streaming through isolation and restore, no crash, original topology restored, lock
and logind unlock fine; `docs/experiments/evidence/exp06/2026-10-02/observation.md`; that run injected no input because the
observer page lost focus at isolation, so remote input under isolation is still untested);
`exp07_restore --keep-live-virtual` (passed by both watchdogs of `exp06 --integrated-probe`) applies the same rule to the
watchdog restore (unit-tested only; plain `exp07_restore` without the flag does not, so manual recovery with a live owner
and a streaming consumer needs the flag). See the
observation's Headless reproduction section.
