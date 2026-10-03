# docs/ops/

Operational procedures for running Blackroom Console and its feasibility experiments
safely on real hardware.

- `experiment-safety.md` — out-of-band recovery procedure required before any experiment
  that changes display or input state (assessment §8).
- `live-grab-runbook.md` + `live-grab-session.sh` — supervised live test of the physical-input
  grab through gateway, hostd and `remote-emergencyd`, driven from a second device over SSH.

## Remote console (`blackroom-console`)

Start: `docs/ops/console.sh --check` (read-only preflight), then `docs/ops/console.sh`, or the user units in
`systemd/user/` (copy to `~/.config/systemd/user/`, `systemctl --user start blackroom-console.service`; no `enable`,
no autostart; the daemon unit has a 10 s watchdog kill). Either way the operator sets the `setfacl` entries on
`/dev/input/event2-5` first and removes them afterwards, keeps a second-device SSH session open, and disables the
extension after use.

Observed live (this laptop, built-in eDP-1 panel at scale 1.0, built-in input nodes, one tablet): Start isolates
the panel and grabs the built-in keyboard and touchpad; typing, pointer, click and scroll work; Stop restores the
display, locks the screen and releases the grab; 15 s without a heartbeat ran Stop with a clean restore; Start on a
locked screen with the extension enabled beforehand showed the lock screen and the account password unlocked it.
Not observed: Safari or Android decode, iOS fullscreen, enabling the extension on an already locked screen, idle
screen blanking during a session, a `kill -9` on purpose (the dead-man restore plus lock was seen once after an unclean exit), input over the
WebRTC data channel on the tablet, a started-by-systemd run, suspend, logout, long runs, other displays or layouts.

Kill switches while a session is live: the chord (Left Ctrl + Left Shift + Left Alt + Esc, held 2 s), the Stop
button, 15 s without a browser heartbeat, and over SSH `systemctl --user stop blackroom-console.service` or
`pkill -TERM -x blackroom-conso` (runs Stop), then `pkill -KILL -x remote-emergenc` (releases the grab at once).
If the panel stays black: `exp07_restore --keep-live-virtual --lock-after --backup <state dir>/backup.json`
(the 60 s dead-man restore does this by itself and now also locks), then `loginctl unlock-session <id>`.

Locked screen: GNOME refuses remote sessions while locked. `gnome-extension/blackroom-locked-remote@blackroom.local`
lifts that so the console can show the lock screen and the account password is typed remotely (no unlock bypass).
Install once: copy the directory to `~/.local/share/gnome-shell/extensions/`, log out and in, then
`gnome-extensions enable blackroom-locked-remote@blackroom.local` while using the console and `disable` afterwards.
While the extension is enabled, locking no longer ends remote sessions: the lock screen is not a kill switch, and
any local process of your user may open a remote session on the locked screen. The kill switches above are the
real ones. Enabling it on an already locked screen lifts the existing block (new code, not yet observed live);
disabling it while locked brings the block back only once the remote sessions have ended (after Stop).

Stop order: restore the display, stop the capture, lock, and only then release the input grab, so the local
keyboard never reaches an unlocked desktop while the panel comes back. The grab lease is renewed before the lock.

State directory (`$XDG_RUNTIME_DIR/blackroom-console`): `last_stop.json` records how far the last Stop got (`step`,
and the report once done), so a Stop that dies half way is visible afterwards; `recovery.json` exists while a live
session holds the display and is removed after a verified restore. If a console dies mid-session, the next console
start restores from `backup.json` (same login session and Shell only) and locks; if that fails, Start is refused
with the file to remove after checking the panel. A live session also holds a logind `sleep:idle` block inhibitor.

Offline simulation state (never a live host): hostd appends security events to
`audit.log` in its state directory (owner-only, 1 MiB then one rotation; static
codes, epochs and principal ids only, never proofs, demo codes, input grants
or session tokens; grant issuance fails closed if it cannot be logged). The
read-only `blackroom --state-dir <abs path> status | logs [--tail N] | doctor`
CLI reports epoch and stop markers, prints valid audit lines, and checks
ownership/modes. It never creates, locks or changes state and refuses
symlinked, relative or loose-permission directories.

Runbooks for the released product (incident response, recovery) live in
`docs/plans/Detailed_Project_Plan/21_OPERATIONAL_RUNBOOK_RECOVERY_AND_INCIDENT_RESPONSE.md`;
this directory holds the project's own dev-workstation procedures.
