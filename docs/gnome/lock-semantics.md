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
Disagreement reports `INDETERMINATE`. A 2026-09-28 read-only observation
returned false/false (`UNLOCKED_OBSERVED`); this does not prove a lock
transition or remotely controlled unlock. `ActiveChanged`, real lock/unlock
dialog control, persistence of windows/workspaces and capture/EIS continuity
across lock are still unmeasured. The session-bus ScreenSaver owner is not
independently bound to the selected login1 session beyond the agent's current
user/session context. Phase 8 and FEAS-A remain partial; no lock was called.