# Same-Session Lock: Offline Contract

**Status:** synthetic lifecycle plus real read-only observation; FEAS-A unproven.
No live session was locked.

The agent's offline lifecycle calls the fake lock operation and separately
checks observed lock state before acknowledging activation. It checks the
same fake session identity during preparation and retains lock through
teardown. A lock call that returns success without an observed lock fails
preparation and triggers rollback; cleanup that cannot confirm lock reports
`FAILED_SAFE`. The fake never unlocks during recovery.

`gnome-session-agent --lock-observation` now rechecks the selected active
Wayland login1 session's UID, seat, type and class, then reads GNOME
ScreenSaver `GetActive` and login1 `LockedHint` without changing either.
The observer resolves the ScreenSaver D-Bus owner to a PID, then tries to
match that PID's login1 session to the selected session. A known different
session is refused; an unmapped owner or disagreeing lock signals is
`INDETERMINATE`. On 2026-09-28 the owner was a `gjs` user-bus service outside
any login1 session. Both raw lock signals were false, but the corrected
read-only verdict is **INDETERMINATE**, not proof of an unlocked session. The
earlier false/false `UNLOCKED_OBSERVED` output lacked this provenance check
and must not be used as lock evidence. Before the separately approved run
below, no lock had been called. `ActiveChanged`, remote unlock-dialog control,
workspace continuity and capture/EIS behavior across lock remain unmeasured;
Phase 8 and FEAS-A remain partial.

One separately approved lock-only diagnostic on 2026-09-28 targeted the exact
active login1 Wayland session with `loginctl lock-session 2`. ScreenSaver
became active first and login1 `LockedHint` later became true. The operator
visually observed the built-in GNOME lock screen with no desktop content,
manually unlocked and confirmed the original desktop responsive; the same
session and GNOME Shell PID survived. Raw lock signals returned to false.
The owner-provenance classification stayed `INDETERMINATE`, and no remote
unlock/capture/input continuity was tested. See the unique Experiment 11
[lock-only observation](../experiments/evidence/exp11/2026-09-28-lock-only/observation.md);
FEAS-A remains unproven. No repeat was authorized.
## Experiment 11 harness (built 2026-10-01, not yet run live)

`exp11_lock_semantics --operator-present` attaches a RemoteDesktop/EIS sender and a ScreenCast monitor
capture, locks the selected session once, injects harmless input into the lock screen, and waits for the
operator to unlock with their own password. The loopback observer page is the witness, judged at three
points: the pre-lock Shift tap arrives; while locked no key, button, pointer or wheel event reaches the
page (pointer and wheel are the real witnesses, the page has no keyboard focus while locked, and the
page's heartbeat must keep running or the run is PARTIAL); after the unlock the same EIS connection
delivers Shift, `a` and Left once each. The capture consumer stays attached and each phase must deliver a
frame (a quiet locked screen only makes that window inconclusive). It refuses to start while a grab holder
is running, aborts before locking if the pre-lock checks fail, never types or reads a password, and keeps
the physical-input grab and virtual monitor out of this first run. Lock signals (`GetActive`,
`LockedHint`, `ActiveChanged`) stay unverified for provenance as above. FEAS-A remains unproven until a
supervised run and the later same-session experiment (plan `plan-20261001-phase8-11-closure.md`).
