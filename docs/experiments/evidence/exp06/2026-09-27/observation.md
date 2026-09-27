# Supervised eDP-only isolation observation (2026-09-27)

**Verdict:** STOP. The original eDP-only topology required a second independent
restore after exp06 stopped its ScreenCast session. Gate FEAS-C is not proven.

## Sequence

1. Preflight in `../2026-09-27-preflight.md`: eDP-1 sole active logical output;
   original GNOME Shell PID `34735`, login1 session `3`, power ON, no pending
   timer, remote desktop masked. Operator confirmed saved work, second-device
   SSH and continuous physical observation. Fresh binary hashes are recorded.
2. At 09:02:59 UTC, one exp06 `--pause-after-isolate --watchdog-seconds 45`
   run started; it printed PID `940537`, backup `backup.json`, and timer
   `blackroom-exp06-watchdog-1790499779`. Read-only GetCurrentState during
   isolation showed raw eDP-1/HDMI-1/Meta-0 but only Meta-0 active logically;
   `PowerSaveMode=3`. The original Shell and session were still active.
3. At 09:04:06 UTC, the watchdog invoked exp07. Its first
   `../../exp07/2026-09-27/report.md` reports PASS with no retry or apply
   error and a matching eDP-only topology. Independent GetCurrentState showed
   eDP-1 active and `PowerSaveMode=0`; the operator confirmed the built-in
   desktop was visible and responsive. Exp06 was still paused.
4. Only after that restore, one Enter released exp06 to clean up ScreenCast.
   It reported PARTIAL by pause-mode design, `stop_error=null`, and
   `virtual_connector_fully_gone=true`. The missing-timer warning occurred
   after the watchdog had already fired and the timer disappeared.
5. Independent GetCurrentState **after** cleanup showed both eDP-1 and HDMI-1
   active logically, not the eDP-only starting state. A second guarded exp07
   with the exact backup wrote `../../exp07/2026-09-27-2/report.md` and
   reported PASS without retry or apply error. A fresh read confirmed only
   eDP-1 active, power ON, Shell PID `34735`, session `3` active Wayland,
   Meta-0 gone and no pending timer. Remote desktop was returned to
   disabled/inactive. The operator confirmed the built-in desktop was normal.

## Finding

The first exp07 PASS was true when measured, but restoration did not remain
stable through ScreenCast owner cleanup: disabling Meta-0 reactivated HDMI-1.
Exp06 checks `all_restored` and disarms its `RestoreGuard` **before**
`Session.Stop`; its post-Stop finding checks only whether the virtual
connector disappeared. Its PARTIAL report therefore misses this final
topology mismatch. The trigger within Mutter cleanup remains unproven; do
not attribute it to a Shell crash (the Shell PID did not change).

## Generated report scope

The generated `report.md` and `findings.json` files are retained unchanged as
machine output. Exp06's PARTIAL records pause mode and Meta-0 cleanup, **not**
a final stable topology check; this observation supplies the independent
post-Stop mismatch that the exp06 report omits. Do not read that PARTIAL as
evidence of a clean restore.

Both generated exp07 reports use `topology_matches` for PASS. Their Expected
text also says no unexpected connector remains, but that is **not** the PASS
predicate. `unexpected_connectors_after_restore` counts raw connectors absent
from backup `outputs[]`; exp06 omitted HDMI-1 from `outputs[]` because that
disabled connector had no current mode. Thus the first exp07 report lists
HDMI-1 and the still-owned Meta-0, while the second lists HDMI-1; neither
list by itself proves an active HDMI display or a leaked virtual monitor.
The independent read-only GetCurrentState *between* cleanup and the second
restore established HDMI-1's temporary logical activation. Subsequent
exp07 report wording has been narrowed; these generated historical reports
have not been rewritten.

The operator has accepted the earlier brief built-in-panel flicker as a
privacy risk without repeat capture. This run has no independent zero-content
evidence and does not certify the full Gate C matrix. Do not repeat or move to
another physical-output test under the current stop conditions.