# Amendment: a locked session ends remote access (conflict C27)

**Date:** 2026-10-01 · **Status:** decided MODIFY (Architecture Review #1, operator) · **Tier:** governed
**Evidence:** `docs/experiments/evidence/exp11/2026-10-01-5/`, `docs/experiments/evidence/exp12/2026-10-01/`,
`docs/gnome/lock-semantics.md` (outcome table, limits). Source: gnome-shell 50.1 `js/ui/main.js` `_sessionUpdated`,
`js/ui/sessionMode.js`; Mutter 50.1 `meta-remote-access-controller.c`.

## Finding

While the session is locked, gnome-shell inhibits remote access: every RemoteDesktop/EIS connection ends (observed,
under 1 s) and `CreateSession` is refused (`Session creation inhibited`). Remote input therefore cannot drive the
unlock dialog and no remote session can stay attached through a lock. A process of the session's own user can
unlock it through logind in under a second (observed twice); fresh sessions work right after.

## Decision

- Remote sessions exist only while the session is unlocked. **Locking is the kill switch** (emergency and teardown).
- RemoteDesktop and ScreenCast sessions are created after each unlock and recreated after each lock.
- **Option A (default, consistent with Docs 01/04/05/14):** remote activation requires a session the local user
  left unlocked; there is no remote unlock. If the session is locked, remote access is unavailable until the local
  user unlocks it.
- **Option B (open decision D1, not adopted):** hostd unlocks through logind after full remote authentication.
  It needs an explicit exception to "only a physical local user unlocks", a threat-model update (Docs 03/09) and a
  polkit grant for hostd's uid on `org.freedesktop.login1.lock-sessions` (Phase 11).
- New activation order (Doc 07 §9): preconditions (unlocked session, no conflicting session) → RemoteDesktop/EIS
  → virtual monitor → blank/disable physical outputs → physical-input grab → `REMOTE_ACTIVE`. The window between
  an unlock and output blanking is a design item for Phase 9 (the session is unlocked and visible).
- Rollback (Doc 07 §10) keeps "destroy sessions and virtual monitor, restore outputs and input, then lock" for a
  normal teardown. An emergency may lock first as the kill switch; that ends the virtual monitor's owner without
  an orderly stop, which is exactly the unobserved owner-loss case of Gate F (Phase 9 must observe it on eDP-only).

## Documents to amend (source documents are not edited by this note)

| Document | Section | Assumption now invalid | Amendment |
|---|---|---|---|
| 02 GNOME Feasibility PoC | locked-console / lock section (about L555) | locked console with remote control; "may terminate remote access" is the observed rule | state it as observed, remove any requirement for remote control while locked |
| 07 State Machine | §9 activation (about L626), §10 rollback (about L685) | no unlock step; lock after restore; sessions survive a lock | new order above; lock = kill switch; `LOCAL_LOCKED` precondition wording |
| 01, 04 §90, 05 §98, 14 | "only a physical local user unlocks", "no automatic unlock" | stays true under option A; option B needs an explicit exception | keep, add the option B note |
| 10 Experiment 12 | §19 same-session continuity | remote view of the same state across a lock | replace by exp12 as run (session id, Shell PID, window, fresh sessions) |
| 06 Services and privileges | hostd polkit | none needed under option A | add the `lock-sessions` grant only if option B is adopted |
| Roadmap Phase 8 Verify | items 1 to 3 | attached through lock, remote unlock dialog, isolation after unlock | replaced by the exp12 outcome table |

## Consequences for Phases 9 to 11

- Phase 9: activation transaction follows the new order; the real backend creates sessions after the unlock check
  and treats a lock as `REMOTE_ACTIVE` -> teardown (sessions end by themselves); exp13/exp14 cover lock during a
  session; Gate F owner-loss on eDP-only is now also the emergency-lock case.
- Phase 10: the emergency lock is a built-in revoke of every remote session; `emergencyd` still persists the stop
  marker and epoch first.
- Phase 11: polkit and a hostd unlock verb only under option B.

## Limits carried from the evidence

One run on eDP-1, one uid; any same-user process can unlock the session (the lock screen is no boundary against
it); capture and virtual monitor were not exercised after unlock; one window checked; lock signals without
provenance; no lock under a physical grab or isolation; SSH unlock unobserved; unlock-to-ready latency unmeasured;
the kill switch needs `org.gnome.desktop.lockdown disable-lock-screen=false`.

## Addendum 2026-10-02: the product's locked-screen mode (extension)

Operator decision: no unlock bypass, but the console may start on a locked screen and the account password is typed
remotely on the lock screen shown in the page. That needs the Shell extension
`docs/ops/gnome-extension/blackroom-locked-remote@blackroom.local`, which makes `inhibit_remote_access` and
`uninhibit_remote_access` no-ops (and lifts an existing block when enabled on an already locked screen).

Consequence for the kill-switch list above: while the extension is enabled, locking the screen no longer ends remote
sessions, so the lock is not a kill switch and the "lock = kill switch" lines in this amendment hold only with the
extension disabled. Any local process of the same user may also open a remote session on the locked screen. The real
kill switches then are: the chord (Left Ctrl + Left Shift + Left Alt + Esc, 2 s), the Stop button, 15 s without a
browser heartbeat, and over SSH a SIGTERM to `blackroom-console` (`systemctl --user stop blackroom-console.service`)
or `pkill -KILL -x remote-emergenc` plus `exp07_restore --keep-live-virtual --lock-after`. Stop locks the screen
after restoring the display and releases the input grab only after that lock. The extension is enabled only while
the console is in use and disabled afterwards; enabling it after the lock and disabling it while locked are
unobserved live.
