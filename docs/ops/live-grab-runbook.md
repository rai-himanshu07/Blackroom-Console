# Supervised live test: physical-input grab (gateway, hostd, remote-emergencyd)

One real thing is tested: the exclusive grab of this laptop's built-in keyboard, mouse and
touchpad. The session, agent, lock and desktop are simulated. Policy: `experiment-safety.md` §7
(operator present, fresh second-device SSH, exact command approved per step). Findings:
`docs/security/input-isolation-decision.md`; last run: `docs/experiments/evidence/exp09/2026-10-01-gateway-grab/observation.md`.
Nothing here installs a service or touches GNOME display config; FEAS-E/G are not promoted by it.

Step risk: **R** read-only, **M** changes host state without touching input, **G** grabs input.
Approve each M and G step with its exact command before it runs. **Chat cannot be answered while
the grab is on**, so approve a whole case first, run it, and report after the pointer works again.

## During a grab

The laptop's keyboard, mouse and touchpad are dead, so every command comes from the tablet. Use
**one** SSH window (Termux makes switching hard); each case is a single command and prompts on
that screen. Hands off the laptop until the tablet says `GRAB SHOULD BE ON` (a key held at grab
start auto-repeats into the session; the daemon waits up to 20 s for all keys up). Leave the lid
open; do not plug the dongle or press the power button (unobserved under a grab). Times you type
(`y`, Enter) include your reaction time of 1 to 5 s; the audit log gives the hostd side.

Paste once in the tablet window (uid 1000 everywhere; **never use sudo for the daemon, gateway or
script**):

```sh
export REPO='/media/user/Playground/Playground_Sys/Blackroom Console'
export XDG_RUNTIME_DIR=/run/user/1000 DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/1000/bus
export LIVE=/run/user/1000/blackroom-live STATE=/run/user/1000/blackroom-live/state
export SOCK=/run/user/1000/blackroom-live/emergency.sock
bk() { bash "$REPO/docs/ops/live-grab-session.sh" "$@"; }
cd "$REPO"
```

## State directory rules (daemon vs hostd's store)

The chord writes its stop marker through hostd's own store, which accepts a directory only if:

| Rule | Live setting |
|---|---|
| Absolute path, no symlink in any component (`openat2 NO_SYMLINKS`) | `/run/user/1000/...`, not `/var/run` |
| Owned by the calling uid | daemon runs as uid 1000, same as gateway and hostd |
| No group or other access (`mode & 077 == 0`) | `install -d -m 700` |
| `host-identity.key` and `security-epoch` exist, mode 0600 | created by hostd when the gateway starts |
| Same directory as the gateway's `--state-dir` | one `$STATE` for both |

Under `sudo` the daemon would be uid 0: hostd refuses it (`daemon_untrusted`) and the marker
would fail. The daemon also refuses to grab (`error marker_unavailable`, reason in its log) when
`--state-dir` is given but not writable by this rule, so a bad directory shows up before a grab,
not at the chord. `blackroom --state-dir "$STATE" doctor` checks the same rules read-only.

## Preflight (agent terminal unless noted)

1. **R** Fresh tablet login and a clean start: `ss -tnH state established '( sport = :22 )'`
   must show the tablet (no row = not connected); `systemctl is-active ssh.socket`;
   `systemctl --user --no-pager list-timers --all 'blackroom-*'` empty;
   `pgrep -ax remote-gateway; pgrep -ax remote-hostd; pgrep -ax remote-emergenc` print nothing
   (all names fit `pgrep`'s 15-character limit; agent: `pgrep -af 'debug/gnome-session-agent'`);
   `pgrep -x gnome-shell; loginctl --no-pager list-sessions` (write the Shell PID down). Save work.
2. **R** Nodes: `awk '/^N: Name/{n=$0} /^H: Handlers/{print n " | " $0}' /proc/bus/input/devices | grep -E 'kbd|mouse'`.
   Built-in set seen on 2026-10-01: AT keyboard (event2), PS/2 mouse (event3), `DELL0A71` Mouse
   and Touchpad (event4, event5); numbers can shift. A node without the ACL is silently skipped
   (the pointer would keep moving: invalid run, not unsafe).
3. **M** The operator runs, with those numbers (never the agent):
   `sudo setfacl -m u:user:rw /dev/input/event2 /dev/input/event3 /dev/input/event4 /dev/input/event5`
   then **R** `getfacl -p` on each shows `user:user:rw-`, and no other node is readable.
4. **M** `install -d -m 700 "$LIVE" "$STATE"`.
5. **M** Prove the kill switch on a stand-in, never first on the real daemon. `pkill -x` with a name
   longer than 15 characters matches nothing, so the timer uses `remote-emergenc`:
   ```sh
   cp /bin/dash "$LIVE/remote-emergencyd"
   setsid "$LIVE/remote-emergencyd" -c 'while :; do sleep 1; done' >/dev/null 2>&1 </dev/null &
   systemd-run --user --unit=blackroom-live-selftest --on-active=10 \
     --timer-property=AccuracySec=1s pkill -KILL -x remote-emergenc
   pgrep -ax remote-emergenc; systemctl --user --no-pager show blackroom-live-selftest.timer -p AccuracyUSec
   ```
   After about 15 s `pgrep -ax remote-emergenc` prints nothing; then `rm -f -- "$LIVE/remote-emergencyd"`.
6. **M** Arm the real kill timer, then check it (`AccuracyUSec=1s`, ExecStart `-KILL -x remote-emergenc`):
   ```sh
   systemd-run --user --unit=blackroom-live-kill --on-active=1200 \
     --timer-property=AccuracySec=1s pkill -KILL -x remote-emergenc
   systemctl --user --no-pager show blackroom-live-kill.timer -p AccuracyUSec -p ActiveState
   systemctl --user --no-pager show blackroom-live-kill.service -p ExecStart
   ```
   When it fires the daemon dies and the kernel drops the grab; a later `start` then fails closed
   (`INPUT_ISOLATION_FAILED`, nothing grabbed). Check `list-timers` for time left before each case;
   to continue, `stop` the timer, re-arm it and restart the daemon (step 8).
7. **M** Gateway first (it creates the key and epoch the marker needs); then **R** `doctor` must
   print only `OK` lines and `status` must show `"emergency_pending":false`:
   ```sh
   setsid nohup target/debug/remote-gateway --offline-sim --separate --state-dir "$STATE" \
     --hostd-bin "$PWD/target/debug/remote-hostd" --agent-bin "$PWD/target/debug/gnome-session-agent" \
     --emergency-socket "$SOCK" > "$LIVE/gateway.log" 2>&1 < /dev/null &
   ```
8. **M** Daemon (starts idle; grabs only on a Start); then **R**
   `target/debug/blackroom emergency-status --socket "$SOCK"` shows `"grabs_enabled":true,"held":0,"phase":"idle"`:
   ```sh
   setsid nohup target/debug/remote-emergencyd --state-dir "$STATE" --client-uid 1000 \
     --enable-grabs --socket "$SOCK" > "$LIVE/emergencyd.log" 2>&1 < /dev/null &
   ```

## Cases, lowest risk first (one tablet command each, G)

Every case: `start` (up to ~25 s, hands off), then touch the laptop touchpad: pointer frozen means
type `y` + Enter on the tablet, pointer moves means `n` + Enter (the case revokes and stops).
`n`, a dropped SSH window or Ctrl-C all revoke by themselves.

1. `bk case revoke`: the script revokes; press Enter when the pointer moves. Pass: back at once
   (about 3 s allowing for your reaction).
2. `bk case silence`: heartbeat stops after the `y`; do not touch anything. Expected release
   +25 s after the last renew (allow +23 to +27: 15 s without renew, then the 10 s daemon lease);
   the tablet prints the expected clock time. `logs --tail` shows `grant_revoked` cause
   `isolation_lost`. Pass: input back by +27 s. At +30 s the control lease would have ended it,
   which does not prove the daemon lease.
3. `bk case freeze`: needs exactly one hostd that is a child of the gateway. SIGSTOP of hostd only;
   the daemon's own lease lapses, expected 8 to 12 s. The script then SIGCONTs hostd (never kill
   it: that leaves `recovery-pending` and blocks restart); hostd then logs `isolation_lost`.
   Still frozen 20 s after HOSTD FROZEN: Ctrl-C (resumes hostd and revokes).
4. `bk case chord` (last: it leaves a stop marker): after the `y`, hold Left Ctrl + Left Shift +
   Left Alt + Esc for 2 s on the built-in keyboard until the pointer moves. Expected: the tablet
   ends within 10 s with `renew http=409` and `state=FAILED_SAFE`; then (R)
   `blackroom --state-dir "$STATE" status` shows `"emergency_pending":true`, `doctor` lists
   `emergency-stop` OK and `emergency stop persisted`, `emergencyd.log` has no `stop marker
   failed` line. hostd exits and keeps `recovery-pending` (it never auto-clears); the daemon
   refuses every further isolate until restarted. Check no key stays stuck (type a letter).

A refused `start` (`keys_held`, `nothing_to_grab`, `marker_unavailable`, `grab_failed`, or no
daemon) shows as `INPUT_ISOLATION_FAILED`; it closes hostd's control stream and the gateway
restarts hostd on the next start (epoch +1), which is harmless. Reasons are in `emergencyd.log`.

## FEAS-E runs A and B (Phase 7 review items; built-in layout event2-5, no gateway)

Same preflight 1-4 and 6 as above (SSH, nodes, ACLs on event2-5, directories, kill timer); no gateway and no
state dir are needed. The agent runs the experiment binaries; you act only on the page prompts.

- **A: the real daemon under the observer page.** **M** mask `gnome-remote-desktop`, start the daemon
  (`setsid nohup target/debug/remote-emergencyd --client-uid 1000 --enable-grabs --socket "$SOCK" ...`).
  **G** the agent runs `target/debug/exp08_remote_input --operator-present --daemon-socket "$SOCK"` and shows
  you a URL. Open it in Firefox, F11, keep it focused, then follow the PAGE prompts: baseline 6 s (type LETTER
  keys only, move the touchpad: the page must see it), hands off 4 s, grab on (keep typing letters and moving
  until the prompt clears (the page says about 25 s; the grab was held about 10 to 15 s) while the injected
  Shift, `a`, Left, pointer, click and scroll run), released. The verdict tally is taken while the grab is still held, so what you type after the prompt
  clears does not count. PASS needs the page to see only the injected stages, a baseline of at least 4 key
  downs and 5 moves, and the daemon's own counts (`reads` at least 20 from at least 2 nodes and at least 15
  inside the tally window, still `isolated`, no pushed release).
  Recovery: `pkill -KILL -x remote-emergenc` from the tablet. Then **M** unmask.
- **B: a frozen daemon under a supervisor.** **M** start the daemon as a unit with a watchdog:
  `systemd-run --user --unit=blackroom-live-daemon -p Type=notify -p NotifyAccess=main -p WatchdogSec=10
  -p WatchdogSignal=SIGKILL target/debug/remote-emergencyd --client-uid 1000 --enable-grabs --socket "$SOCK"`.
  **G** the agent runs `target/debug/exp09_freeze --operator-present --socket "$SOCK"`: it grabs, renews 8 s,
  SIGSTOPs only the verified daemon and measures the seconds until the connection closes (target 30 s or less,
  expected about 10 s). Touch the touchpad throughout and note when the pointer moves. If no supervisor kills
  it within 60 s the probe kills the daemon itself and the result is FAIL.

## If something is wrong (tablet, in this order)

1. `bk revoke`. 2. `pkill -KILL -x remote-emergenc` (SIGKILL releases the grab; a frozen holder
keeps it, so never rely on SIGSTOP or the lease alone). 3. The kill timer fires by itself.
4. Physical chord, then `sudo systemctl reboot` from the tablet. Do not count on SysRq or the
power button under a grab.

## Cleanup (nothing is installed)

`systemctl --user stop blackroom-live-kill.timer; pkill -TERM -x remote-gateway;
pkill -TERM -x remote-emergenc`; the pgrep checks from preflight 1 print nothing; the operator runs
`sudo setfacl -x u:user /dev/input/event2 /dev/input/event3 /dev/input/event4 /dev/input/event5`
and `getfacl -p` shows no `user` entry; after approval remove `"$LIVE"` (throwaway state on
tmpfs, including any chord marker). Record times and the audit timeline only, never key codes.
