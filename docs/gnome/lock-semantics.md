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
and must not be used as lock evidence. `ActiveChanged`, real lock/unlock dialog
control, workspace continuity and capture/EIS behavior across lock remain
unmeasured. Phase 8 and FEAS-A remain partial; no lock was called.