# Same-Session Lock: Offline Contract

**Status:** synthetic lifecycle, real read-only observation and three supervised live lock runs (2026-09-28
lock-only; exp11 and exp12 on 2026-10-01). FEAS-A is **not met as originally worded**; a replacement path
is observed with limits (see the Experiment 12 section); the gate decision (2026-10-01, MODIFY) is recorded at the end.

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
`LockedHint`, `ActiveChanged`) stay unverified for provenance as above. (Superseded by the results below.) FEAS-A remained unproven until a
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
logind, so the lock screen is not a boundary against such a process. (Superseded: the run happened, see the result below.)

**Result (evidence `docs/experiments/evidence/exp12/2026-10-01/observation.md`, PASS):** a `CreateSession` while
locked is refused (`Session creation inhibited`), consistent with remote access being inhibited while the
session is locked (observed once, about 0.7 s after the first lock; the source says for the whole locked mode); `loginctl unlock-session` from a process of the session's own user unlocked it in under a
second (twice, exit 0, no polkit prompt); a fresh RemoteDesktop/EIS session created after the unlock (accepted at 58.97 s, 3.7 s after
`ActiveChanged(false)`; no earlier attempt, so the earliest allowed moment is unmeasured) delivered input; the login session id, Shell PID and observer page survived two lock cycles. Phase 8 Verify items:

| Roadmap item | Outcome |
|---|---|
| Session locked before activation stays attached (virtual monitor, capture, EIS) through lock | **Not achievable**: the EIS connection ended at the lock (observed); ScreenCast/virtual-monitor sessions are expected to end too (source reading, not observed; the capture stream delivered no frames behind a fullscreen page) |
| Remote input can drive the unlock dialog | **Not achievable** through Mutter RemoteDesktop |
| Replacement: unlock by logind from a user process, fresh sessions after unlock | Observed twice (same uid, built-in layout) |
| After unlock physical outputs stay disabled and input stays isolated | **Not achievable as worded** (a virtual monitor needs an unlocked session, so activation must unlock first and the window between unlock and output blanking is a Phase 9 design item); not tested |
| Identifiable session state survives lock, remote, disconnect, lock | Observed for the session id, Shell PID and one window (the observer page) |
| Lock on teardown verified via `GetActive` | Both signals agreed and `ActiveChanged` arrived, but without provenance (the ScreenSaver owner is not mapped to the login1 session); the shipped observer still classifies this `INDETERMINATE` and `remote_mode_allowed=false` |

Design consequences for Architecture Review #1: locking the session is a built-in kill switch for remote access
(the EIS connection ended in under a second and new sessions are refused while locked; ScreenCast follows from the source reading), which fits the emergency path;
remote unlock is a hostd-side decision (authenticate, then logind `Unlock`), not input into the unlock dialog;
RemoteDesktop and ScreenCast sessions are created after each unlock and recreated after each lock; a
different-uid hostd needs a polkit grant for `org.freedesktop.login1.lock-sessions` (Phase 11). The gate
decision was taken on 2026-10-01: **FEAS-A PASS-WITH-LIMITS under the replacement design (original wording not met),
Architecture Review #1 = MODIFY**; amendment and conflict C27: `docs/plans/amendment-20261001-lock-inhibits-remote-access.md`.

**Limits of the replacement path (review 2026-10-01):** one run on eDP-1, one uid; unlock is possible for any
process of the session's own user (logind authorises the session's uid; observed from inside the session, not
over SSH), so the lock screen is no boundary against such a process; no capture or virtual monitor after the
unlock; one window checked (the observer page), no workspaces; lock signals without provenance; no lock under a
physical grab or display isolation (the unobserved owner-loss case of Gate F); unlock-to-ready latency not
measured; the kill-switch property needs `org.gnome.desktop.lockdown disable-lock-screen=false`.

**Documents that assume what this finding invalidates (to amend after the gate decision):** Doc 02 (locked
console with remote control), Doc 07 section 9 (activation order has no unlock step and an exposure window
between unlock, virtual monitor and output disable) and section 10 (rollback must lock first), the "only a
physical user unlocks" statements in Docs 01, 04, 05 and 14 (now an explicit exception: hostd-authenticated
logind unlock), Doc 10 Exp 12 (remote view of the same state), Doc 06 (a polkit rule for hostd's uid), and the
roadmap Phase 8 Verify items 1 to 3.
