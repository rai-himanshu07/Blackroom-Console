# Same-Session Lock: Offline Contract

**Status:** synthetic only; FEAS-A unproven. No live session was locked.

The agent's offline lifecycle calls the fake lock operation and separately
checks observed lock state before acknowledging activation. It checks the
same fake session identity during preparation and retains lock through
teardown. A lock call that returns success without an observed lock fails
preparation and triggers rollback; cleanup that cannot confirm lock reports
`FAILED_SAFE`. The fake never unlocks during recovery.

GNOME ScreenSaver `GetActive`/`ActiveChanged`, logind `LockedHint`, remote
unlock-dialog control, persistence of actual windows/workspaces and capture/EIS
continuity across lock are **not** implemented or measured here. The offline
agent mode never invokes the real GNOME agent startup path. Phase 8 and the
same-session gate remain partial until those real observations are authorized.