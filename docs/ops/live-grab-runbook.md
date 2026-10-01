# Supervised live test: physical-input grab (gateway, hostd, remote-emergencyd)

One real thing is tested: the exclusive grab of this laptop's built-in keyboard, mouse and
touchpad. The session, agent, lock and desktop are simulated. Policy: `experiment-safety.md` §7
(operator present, fresh second-device SSH, exact command approved per step); evidence and
findings: `docs/security/input-isolation-decision.md`. Nothing here installs a service or touches
GNOME display config. FEAS-E/G are not promoted by one run.

Step risk: **R** read-only, **M** changes host state without touching input, **G** grabs input.
The agent asks for approval of the exact command before every M and G step.

## What the laptop does during a session

Its keyboard, mouse and touchpad are dead until the grab ends, so all commands come from the
tablet over SSH. Do not touch the laptop's input at all during `start` (a key held at grab start
auto-repeats into the session; the daemon waits up to 20 s for all keys up). Leave the lid open,
do not plug or unplug the dongle (hotplug is unobserved), do not press the power button.
Observation: the pointer (frozen or moving) checks the mouse and touchpad nodes; click into a plain
text window on the laptop before `start` and press one letter key after the pointer freezes: nothing
may appear (that checks the keyboard node).

## Tablet windows

Open three SSH windows and paste this block into each (uid 1000 everywhere; **never use sudo for
the daemon, gateway or script**):

```sh
export REPO='/media/user/Playground/Playground_Sys/Blackroom Console'
export XDG_RUNTIME_DIR=/run/user/1000 DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/1000/bus
export LIVE=/run/user/1000/blackroom-live STATE=/run/user/1000/blackroom-live/state
export SOCK=/run/user/1000/blackroom-live/emergency.sock
bk() { bash "$REPO/docs/ops/live-grab-session.sh" "$@"; }
cd "$REPO"
```

- **A**: commands, kill and recovery. **B**: `bk start && bk heartbeat ...` (the page-less
  stand-in for the browser heartbeat; start it right after A is ready, the first renew must come
  within 15 s of the grab). **C** (optional): `tail -f "$LIVE/emergencyd.log" "$LIVE/gateway.log"`.
- The script prints state, never the grant. `state` is the gateway's view, not the grab: confirm
  local input on the laptop.

## State directory rules (daemon vs hostd's store)

The chord writes its stop marker through hostd's own store, which accepts a directory only if:

| Rule | Live setting |
|---|---|
| Absolute path, no symlink in any component (`openat2 NO_SYMLINKS`) | `/run/user/1000/...`, not `/var/run` |
| Owned by the calling uid | daemon runs as uid 1000, same as the gateway and hostd |
| No group or other access (`mode & 077 == 0`) | `install -d -m 700` |
| `host-identity.key` and `security-epoch` exist, mode 0600 | created by hostd when the gateway starts |
| Same directory as the gateway's `--state-dir` | one `$STATE` variable for both |

`sudo` would make the daemon uid 0: hostd then refuses it (`daemon_untrusted`) and the marker
would fail. The daemon now refuses to grab (`error marker_unavailable`, reason in its log) when
`--state-dir` is given but not writable by this rule, instead of finding out at the chord.
Offline tests prove the marker lands in a directory hostd holds open
(`the_real_marker_lands_in_a_state_directory_that_hostd_holds`).

## Preflight (window A unless noted)

1. **R** Tablet SSH is a fresh login, and no earlier run is left:
   `ss -tn state established '( sport = :22 )'` shows the tablet's address;
   `systemctl is-active ssh.socket`; `systemctl --user --no-pager list-timers --all 'blackroom-*'`
   is empty; `pgrep -ax remote-gateway; pgrep -ax remote-hostd; pgrep -ax remote-emergenc` print nothing
   (all three names fit `pgrep`'s 15-character limit; for the agent use
   `pgrep -af 'debug/gnome-session-agent'`).
2. **R** Baseline: `pgrep -x gnome-shell; loginctl --no-pager list-sessions` (write down the
   Shell PID) and save all work on the laptop.
3. **R** Nodes: `awk '/^N: Name/{n=$0} /^H: Handlers/{print n " | " $0}' /proc/bus/input/devices | grep -E 'kbd|mouse'`.
   Expected built-in set (numbers can shift): AT keyboard (`sysrq kbd`, event2), PS/2 mouse (event3),
   `DELL0A71` Mouse and Touchpad (event4, event5). No dongle. Power button, Intel HID, Dell WMI,
   Video Bus nodes are not grabbed.
4. **M** Operator on the laptop or tablet, with the node numbers from step 3 (the operator runs
   sudo, never the agent):
   `sudo setfacl -m u:user:rw /dev/input/event2 /dev/input/event3 /dev/input/event4 /dev/input/event5`
   then **R** `getfacl -p /dev/input/event2 | grep user`. A node without the ACL is silently
   skipped, so the pointer would keep moving: that run is invalid, not unsafe.
5. **M** Directories: `install -d -m 700 "$LIVE" "$STATE"`.
6. **M** Prove the kill switch on a stand-in (never on the real daemon first). `pkill -x` with a name
   longer than 15 characters matches nothing, so the timer uses `remote-emergenc`:
   ```sh
   cp /bin/dash "$LIVE/remote-emergencyd"
   setsid "$LIVE/remote-emergencyd" -c 'while :; do sleep 1; done' >/dev/null 2>&1 < /dev/null &
   pgrep -ax remote-emergenc
   systemd-run --user --unit=blackroom-live-selftest --on-active=10 \
     --timer-property=AccuracySec=1s pkill -KILL -x remote-emergenc
   systemctl --user --no-pager show blackroom-live-selftest.timer -p AccuracyUSec
   ```
   After about 15 s: `pgrep -ax remote-emergenc` prints nothing. Then
   `rm -f -- "$LIVE/remote-emergencyd"`. If the stand-in survives, stop: the kill switch is broken.
7. **M** Arm the real kill timer (25 min, 1 s accuracy) and verify what it will run:
   ```sh
   systemd-run --user --unit=blackroom-live-kill --on-active=1500 \
     --timer-property=AccuracySec=1s pkill -KILL -x remote-emergenc
   systemctl --user --no-pager show blackroom-live-kill.timer -p AccuracyUSec -p ActiveState
   systemctl --user --no-pager show blackroom-live-kill.service -p ExecStart
   ```
   Expect `AccuracyUSec=1s` and `-KILL -x remote-emergenc`. When it fires the daemon dies, the
   kernel drops the grab, and the test simply ends; re-arm for a restart.
8. **M** Gateway first (it creates the key and epoch the marker needs):
   ```sh
   setsid nohup target/debug/remote-gateway --offline-sim --separate --state-dir "$STATE" \
     --hostd-bin "$PWD/target/debug/remote-hostd" --agent-bin "$PWD/target/debug/gnome-session-agent" \
     --emergency-socket "$SOCK" > "$LIVE/gateway.log" 2>&1 < /dev/null &
   ```
   Then **R** `target/debug/blackroom --state-dir "$STATE" doctor` must print only `OK` lines and
   `status` must show `"emergency_pending":false`. Any other result: stop, this is the ownership
   rule failing.
9. **M** Daemon (starts idle; grabbing begins only on a Start):
   ```sh
   setsid nohup target/debug/remote-emergencyd --state-dir "$STATE" --client-uid 1000 \
     --enable-grabs --socket "$SOCK" > "$LIVE/emergencyd.log" 2>&1 < /dev/null &
   ```
   **R** `target/debug/blackroom emergency-status --socket "$SOCK"` shows
   `"grabs_enabled":true,"held":0,"phase":"idle"` (this only works while no session is engaged).

## Cases, lowest risk first

Hands off the laptop from `start` until the case says otherwise. After each case the grab must be
gone and the pointer must move before the next case.

**1. Revoke (G).** B: `bk start && bk heartbeat`. Expect `start http=200 state=REMOTE_ACTIVE` in a
few seconds. Operator: pointer and touchpad dead. A: `bk revoke` expects `http=200 state=LOCAL_LOCKED`;
pointer moves within about 3 s; B prints `renew http=409` and exits.
Pass: dead during, back within 3 s of revoke.

**2. Heartbeat stops (G).** B: `bk start && bk heartbeat 3` (renews at +0, +10, +20 s, then stops
and watches). B prints the time of the last renew and the expected release (+25 s, allow +23 to
+27: 15 s without a fresh renew, then the 10 s daemon lease). Operator: note when the pointer moves.
A afterwards: `target/debug/blackroom --state-dir "$STATE" logs --tail 4` shows
`grant_revoked` with cause `isolation_lost`.
Pass: input back by +27 s. At +30 s or later the 30 s control lease ended it, which does not
prove the daemon lease: record FAIL for that claim (still safe).

**3. hostd frozen (G).** B: `bk start && bk heartbeat`. A, only when exactly one hostd exists:
```sh
test "$(pgrep -x remote-hostd | wc -l)" = 1 && kill -STOP "$(pgrep -x remote-hostd)" && date +%T
```
The daemon lease (10 s, last renewed at most 2 s earlier) lapses by itself: expect input back
8 to 12 s after the STOP. `bk status` is not an indicator here (the gateway cannot reach hostd and
reports LOCAL_LOCKED at once); B's heartbeat exits on its first refused renew, which is expected.
Then A: `pkill -TERM -x remote-gateway` (the gateway kills its
children), `pgrep -ax remote-hostd` must print nothing, and repeat preflight step 8 for a fresh
gateway. Pass: input back within 12 s.

**4. Chord (G, last: it leaves a stop marker).** B: `bk start && bk heartbeat`. Operator on the
laptop: hold Left Ctrl + Left Shift + Left Alt + Esc for 2 s on the built-in keyboard, then let go
only after the pointer moves. Expect input back about 2 s into the hold. A afterwards:
`bk status` (not REMOTE_ACTIVE), `target/debug/blackroom --state-dir "$STATE" status` shows
`"emergency_pending":true`, `doctor` reports `emergency stop persisted`, `bk start` is refused,
and `cat "$LIVE/emergencyd.log"` has no `stop marker failed` line. The daemon now refuses every
isolate until restarted. Pass: input back, marker present.

## If something is wrong (from the tablet, in this order)

1. `bk revoke`. 2. `pkill -KILL -x remote-emergenc` (SIGKILL releases the grab; a frozen holder
keeps it, so never rely on SIGSTOP or the lease alone). 3. The kill timer fires by itself.
4. Physical chord on the laptop, then `sudo systemctl reboot` from the tablet. SysRq and the power
button under a grab are unobserved: do not count them.
If the pointer keeps moving while a session is Active, the ACLs or node numbers are wrong: revoke
and redo preflight 3 and 4. A `start` that fails with `INPUT_ISOLATION_FAILED` is the fail-closed path
(`keys_held`, `nothing_to_grab`, `marker_unavailable`, `grab_failed`); the reason is in the audit
log only as `isolation_failed`, the daemon log has details for `marker_unavailable`.

## Cleanup (nothing is installed)

`systemctl --user stop blackroom-live-kill.timer`; `pkill -TERM -x remote-gateway`;
`pkill -TERM -x remote-emergenc`; the pgrep checks from preflight 1 print nothing; the operator
runs `sudo setfacl -x u:user /dev/input/event2 /dev/input/event3 /dev/input/event4 /dev/input/event5`
and `getfacl -p` shows no `user` entry; after approval remove `"$LIVE"` (throwaway state,
including the chord marker, on tmpfs). Record per case: time of start, last renew, input back,
and the Shell PID unchanged. Counts and times only, never key codes.
