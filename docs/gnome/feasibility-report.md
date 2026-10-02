# GNOME feasibility report (draft)

**Status:** DRAFT 2026-10-03. Evidence summary for the Phase 10 gate. No Go/No-Go is recorded here: the owner
records it. Nothing below is promoted beyond what the cited evidence observed. Host: this laptop (GNOME Shell and
Mutter 50.1, built-in eDP-1 at scale 1.0, built-in input nodes event2-5, one uid, one tablet client).

| Gate | Status | Basis | Main limits |
|---|---|---|---|
| FEAS-A lock semantics | PASS-WITH-LIMITS (replacement design, Review #1 = MODIFY) | exp11, exp12, `docs/gnome/lock-semantics.md`; product starts on a locked screen with the Shell extension (live runs 4, 8, 10) | Locking ends remote sessions unless the extension is enabled; with it, locking is no kill switch. No unlock bypass by decision. |
| FEAS-B virtual display and capture | PASS | exp03 to exp05; the console streams MJPEG and H.264 from a real ScreenCast stream (many live runs) | Damage-driven: an idle desktop sends about 1 frame/s; Mutter sends empty buffers on the lock screen (skipped). |
| FEAS-C physical display isolation | PASS-WITH-LIMITS | exp06/07, `docs/gnome/display-isolation.md`, product Start/Stop live | eDP-1 only, scale 1.0; the restore keeps the virtual monitor until ScreenCast stops (Mutter NULL-view crash, root-caused). HDMI unsupported. |
| FEAS-D remote input | PASS-WITH-LIMITS | exp08, product input live (keys, pointer, buttons, scroll) | Same-session EIS on the built-in layout; data-channel transport seen on the tablet only as "worked". |
| FEAS-E physical input isolation | PASS-WITH-LIMITS | exp09, daemon runs, product live | Built-in nodes only; same uid plus ACLs set by the operator; dongles and hotplug unproven. |
| FEAS-F fail-safe | OPEN | Observed: owner kill with the grab (Gate F, once); heartbeat loss Stop; dead-man restore fired and locked (run 13); headless 100 cycles, 8 race rounds, 2 SIGKILLs without leaks or Shell loss | Not observed: PipeWire and Mutter failure injection, suspend, logout, power loss, 1 h soak. Open defect: SIGHUP in a live session killed the console mid-Stop (see the plan log, run 13). |
| FEAS-G emergency | OPEN | The chord releases the grab (observed); matrix in `docs/security/emergency-matrix.md`; daemon links only libc-level libraries (tested) | No lock, epoch or marker in the product chain; chord with the network down not run in a namespace; Phase 10 live runs not done. |
| FEAS-H same-session recovery | OPEN | The dead-man restore returned the same session locked (once); recovery marker and start-up recovery written and unit-tested | Not exercised on a real crash; session survival across suspend or logout unknown. |

## Risks that remain

- Mutter instability: the Shell survived every owner loss since the fix, but owner loss still produces NULL-pointer
  assertions and cursor-update errors in the Shell journal (2026-10-03 01:43). One real crash was root-caused to a
  missing NULL check; the invariant "never apply a config without the virtual monitor while a consumer streams" stays.
- Single uid: any process of the user can use the console's authority surfaces (see `docs/security/poc-findings.md`).
- The only recovery for the built-in panel is software (watchdog, SSH); there is no unplug fallback.

## What the Go/No-Go needs from evidence

FEAS-F, FEAS-G and FEAS-H live runs (Phase 9 and 10 lists in the roadmap), the SIGHUP defect fixed and re-observed,
and the owner's acceptance of the single-uid and eDP-only limits.

**Decision (Go / No-Go / Modify), date and reason: to be recorded by the owner.**
