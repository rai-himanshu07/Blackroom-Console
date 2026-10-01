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
## Experiment 11 (2026-10-01): EIS and capture do not survive the lock

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

**Result (attempt 5, evidence `docs/experiments/evidence/exp11/2026-10-01-5/observation.md`):** the lock engaged in
776 ms and Mutter ended the EIS connection at once (`DeviceRemoved`, `SeatRemoved`, `Disconnected`); no injection
was possible while locked or after the unlock on the old connection. gnome-shell 50.1 calls
`remote_access_controller.inhibit_remote_access()` for every session mode that does not allow screencast, which
includes the locked `unlock-dialog` mode, and Mutter terminates all remote access sessions on that call and
refuses new ones until the call is undone. Consequences: remote input cannot drive the unlock dialog through
Mutter RemoteDesktop; the remote path must unlock the session first (logind `Unlock` after hostd-side
authentication, untested) and create the RemoteDesktop/ScreenCast sessions only while unlocked, and again after
every lock; the lock on teardown is unaffected. The capture continuity question is open (the stream delivered no
frames behind a fullscreen page). FEAS-A is **not met as originally worded**; the next experiment tests the
replacement path (plan step 2).

## Experiment 12 (2026-10-01): the replacement path works

`exp12_same_session --operator-present` tests the replacement path on the live session: an EIS sender works
before the lock; `loginctl lock-session` locks, the old EIS connection's end is recorded, a new `CreateSession`
while locked is only observed (and, if accepted, a net-zero pointer move and a Shift tap are sent), nothing may
reach the observer page while locked and the page must keep beating; the program then unlocks with
`loginctl unlock-session` (the call must exit 0 on a session that was still locked, and the session must then
report unlocked; any other route counts as a manual unlock and the run is PARTIAL); a fresh RemoteDesktop/EIS
session after the unlock must deliver Shift, `a`, Left once each; the session is disconnected, locked and
unlocked once more. The login session id, the Shell PID and the observer page instance (a served-page counter)
must be unchanged. No password is typed or read; a same-user process can unlock its own session through
logind, so the lock screen is not a boundary against such a process. FEAS-A stays open until this has run.

**Result (evidence `docs/experiments/evidence/exp12/2026-10-01/observation.md`, PASS):** a `CreateSession` while
locked is refused (`Session creation inhibited`), confirming that remote access is inhibited for the whole time
the session is locked; `loginctl unlock-session` from a process of the session's own user unlocked it in under a
second (twice, exit 0, no polkit prompt); a fresh RemoteDesktop/EIS session right after the unlock delivered
input; the login session id, Shell PID and observer page survived two lock cycles. Phase 8 Verify items:

| Roadmap item | Outcome |
|---|---|
| Session locked before activation stays attached (virtual monitor, capture, EIS) through lock | **Not achievable**: lock ends every remote session; capture and virtual monitor not exercised, stream delivered no frames behind a fullscreen page |
| Remote input can drive the unlock dialog | **Not achievable** through Mutter RemoteDesktop |
| Replacement: unlock by logind from a user process, fresh sessions after unlock | Observed twice (same uid, built-in layout) |
| After unlock physical outputs stay disabled and input stays isolated | Not tested; belongs to the Phase 9 activation transaction |
| Identifiable session state survives lock, remote, disconnect, lock | Observed for the session id, Shell PID and one window (the observer page) |
| Lock on teardown verified via `GetActive` | Observed (both signals, `ActiveChanged` events) |

Design consequences for Architecture Review #1: locking the session is a built-in kill switch for remote access
(every remote session ends in under a second and none can start while locked), which fits the emergency path;
remote unlock is a hostd-side decision (authenticate, then logind `Unlock`), not input into the unlock dialog;
RemoteDesktop and ScreenCast sessions are created after each unlock and recreated after each lock; a
different-uid hostd needs a polkit grant for `org.freedesktop.login1.lock-sessions` (Phase 11). The gate
decision is the operator's (plan step 3).
