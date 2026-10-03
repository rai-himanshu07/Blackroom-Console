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
| FEAS-F fail-safe | PASS-WITH-LIMITS (proposed) | Observed: owner kill with the grab (Gate F, once); heartbeat loss Stop; dead-man restore fired and locked (run 13); SIGKILL mid-session twice with the crash guard: locked in milliseconds, display restored by the timer after 61 s, same Shell and session (runs 18, 20; run 19 crashed the Shell before the guard was made lock-only); headless 100 cycles, 8 race rounds, 2 SIGKILLs without leaks or Shell loss | PipeWire restart during a session aborts the Shell (run 21): session lost, fails safe to the locked login screen, not same-session recoverable. Not observed: Mutter failure injection, suspend, logout, power loss, 1 h soak. SIGHUP in a live session once killed the console mid-Stop (run 13); after registering the signal handlers first, one live re-run stopped cleanly (run 14). |
| FEAS-G emergency | PASS-WITH-LIMITS (proposed) | The chord releases the grab (observed, also with Wi-Fi off, run 22); the daemon latch refused a stale Start (observed, run 22); matrix in `docs/security/emergency-matrix.md`; daemon links only libc-level libraries (tested) | No lock, epoch or marker in the product chain; chord with the network down not run in a namespace; Phase 10 live runs not done. |
| FEAS-H same-session recovery | PARTIAL (proposed) | The dead-man restore returned the same session locked (once); recovery marker and start-up recovery written and unit-tested | Not exercised on a real crash; session survival across suspend or logout unknown. |

## Risks that remain

- Mutter instability: a second real Shell crash happened on 2026-10-03 13:52 (run 19): applying a monitors config 0.4 s after a killed console's virtual monitor vanished crashed libmutter (`apply_monitors_config` -> `rebuild` -> NULL deref); the session was lost. The first crash (2026-10-01) was the same bug class. Both came from applying a display config while a virtual monitor was being torn down. The rule is now: after an owner dies, never touch the display for the first seconds; only the 60 s timers restore. Owner loss alone (no config applied) has not crashed the Shell in four observed kills, but it still logs NULL-pointer assertions.
- Single uid: any process of the user can use the console's authority surfaces (see `docs/security/poc-findings.md`).
- The only recovery for the built-in panel is software (watchdog, SSH); there is no unplug fallback.

## Proposed reading for the owner (2026-10-03)

Status words in the table are proposals; only the owner's decision below counts.

- **Works and observed live:** Start and Stop on the real session, remote input, H.264 and MJPEG video, Start on a
  locked screen with the extension, the chord (also with Wi-Fi off), heartbeat loss, SIGHUP/SIGTERM Stop, a console
  killed with SIGKILL (locked in milliseconds, display restored by the 60 s timer, same Shell and session), the
  start-up recovery marker, the systemd units, a stale Start refused by the emergency latch.
- **Known limits:** eDP-1 only at scale 1.0, built-in input nodes, one uid with ACLs set by the operator, a PipeWire
  restart or crash during a session aborts the Shell (session lost, fails safe to the locked login screen), two real
  Mutter crashes so far (both applying a display config while a virtual monitor was torn down; both fixed by rule),
  suspend, power loss, the 1 h soak and non-built-in devices not observed.
- **Not yet built (Phase 11):** real authentication (today one LAN token), a dedicated uid, PAM and polkit. Do not
  expose the console beyond a trusted LAN before Phase 11.
- **Fit:** a Go for single-owner use on this laptop over a trusted LAN looks supported by the evidence; a Go for
  anything wider does not.

**Decision (Go / No-Go / Modify), date and reason: to be recorded by the owner.**
