#!/usr/bin/env bash
# Integrated core-path live run (Phase 9): RemoteDesktop/EIS, virtual monitor, isolate eDP-1, real
# input grab, remote Shift/a/Left judged by the observer page, orderly teardown, lock and unlock.
#
# Run it from the repo root IN YOUR OWN TERMINAL with the tablet SSH session open:
#   docs/ops/live-integrated-run.sh --check    # read-only preflight, changes nothing
#   docs/ops/live-integrated-run.sh            # the live run
#
# Needs temporary ACLs on event2-5 (you run sudo, never the script):
#   sudo setfacl -m u:user:rw /dev/input/event2 /dev/input/event3 /dev/input/event4 /dev/input/event5
# and afterwards: sudo setfacl -x u:user /dev/input/event2 /dev/input/event3 /dev/input/event4 /dev/input/event5
set -u
cd "$(dirname "$0")/../.." || exit 2
if [ "${BLACKROOM_STOP_LIFTED:-}" != "1" ]; then
  echo "STOPPED: the 2026-10-01 run crashed GNOME Shell (SIGSEGV in the restore ApplyMonitorsConfig, session lost)."
  echo "Cause and fix: docs/experiments/evidence/exp06/2026-10-01-3/observation.md. Run only with the"
  echo "operator's approval for this run: BLACKROOM_STOP_LIFTED=1 docs/ops/live-integrated-run.sh"
  exit 3
fi
export XDG_RUNTIME_DIR=/run/user/1000
LIVE=/run/user/1000/blackroom-live
SOCK=$LIVE/emergency.sock
SESS=$(loginctl show-user "$(id -un)" -p Display --value)
EV=docs/experiments/evidence/exp06
fail=0
bad() { echo "PREFLIGHT FAIL: $*"; fail=1; }

preflight() {
  for n in 2 3 4 5; do
    { [ -r "/dev/input/event$n" ] && [ -w "/dev/input/event$n" ]; } || bad "no rw access to /dev/input/event$n (run the setfacl)"
  done
  [ -n "$(ss -tnH state established '( sport = :22 )')" ] || bad "no second-device SSH session is connected"
  connected=$(grep -l '^connected$' /sys/class/drm/card*-*/status 2>/dev/null | sed 's|/sys/class/drm/||; s|/status||')
  [ "$connected" = "card1-eDP-1" ] || bad "connected outputs are not exactly the built-in panel: $connected"
  [ -z "$(systemctl --user list-timers --all --no-legend 'blackroom-*' 2>/dev/null)" ] || bad "a blackroom-* timer is pending"
  for p in remote-emergenc remote-hostd remote-gateway exp09_grab_prob exp09_freeze exp08_remote_in exp06_isolate_o exp11_lock_sema exp12_same_sess; do
    pgrep -x "$p" > /dev/null && bad "$p is running"
  done
  [ "$(systemctl --user is-active gnome-remote-desktop.service)" != "active" ] || bad "gnome-remote-desktop is active"
  [ "$(gsettings get org.gnome.desktop.lockdown disable-lock-screen)" = "false" ] || bad "disable-lock-screen is not false"
  [ "$(loginctl show-session "$SESS" -p LockedHint --value)" = "no" ] || bad "graphical session $SESS is locked or unknown"
  for b in target/debug/exp06_isolate_outputs target/debug/exp07_restore target/debug/remote-emergencyd target/debug/blackroom; do
    [ -x "$b" ] || bad "missing $b (cargo build -p blackroom-experiments -p remote-emergencyd -p blackroom-cli)"
  done
  echo "shell pid $(pgrep -x gnome-shell); AC: $(cat /sys/class/power_supply/AC*/online 2>/dev/null | head -n 1)"
}

preflight
if [ "$fail" -ne 0 ]; then echo "Not starting."; exit 1; fi
if [ "${1:-}" = "--check" ]; then echo "Preflight OK (nothing changed)."; exit 0; fi

cat <<EOF
About to run the integrated live test. Have you: saved work, closed other windows, plugged in AC,
kept the lid open, and opened the tablet SSH session?
Sequence: this terminal prints a URL; open it in Firefox, press F11, then hands off keyboard, touchpad,
lid and power button. The panel goes black for about a minute, the lock screen shows for a few
seconds, then everything returns by itself.
Recovery from the tablet: pkill -KILL -x remote-emergenc ; exp07_restore --keep-live-virtual --backup <path in the evidence dir> ;
loginctl unlock-session $SESS. The restore watchdog fires 120 s after it is armed.
EOF
read -r -p "Type START to begin: " answer
[ "$answer" = "START" ] || { echo "Aborted."; exit 1; }

cleanup() {
  systemctl --user stop blackroom-live-kill.timer > /dev/null 2>&1
  pkill -x remote-emergenc > /dev/null 2>&1
  systemctl --user unmask gnome-remote-desktop.service > /dev/null 2>&1
  systemctl --user disable --now gnome-remote-desktop.service > /dev/null 2>&1
  rm -f -- "$SOCK" "$LIVE/emergencyd.log"
  rmdir "$LIVE" 2> /dev/null
}
trap cleanup EXIT

install -d -m 700 "$LIVE" || exit 2
systemd-run --user --unit=blackroom-live-kill --on-active=900 --timer-property=AccuracySec=1s \
  pkill -KILL -x remote-emergenc || exit 2
systemctl --user mask --now gnome-remote-desktop.service || exit 2
setsid nohup target/debug/remote-emergencyd --client-uid 1000 --enable-grabs --socket "$SOCK" \
  > "$LIVE/emergencyd.log" 2>&1 < /dev/null &
sleep 1
target/debug/blackroom emergency-status --socket "$SOCK" || { echo "daemon not answering"; exit 2; }

target/debug/exp06_isolate_outputs --pause-after-isolate --watchdog-seconds 120 \
  --integrated-probe --grab-socket "$SOCK" --hold-secs 25
code=$?
echo "exp06 exit code: $code"
latest=$(ls -td "$EV"/*/ 2> /dev/null | head -n 1)
if [ -f "${latest}integrated.json" ]; then
  python3 - "${latest}integrated.json" <<'PY'
import json, sys
j = json.load(open(sys.argv[1]))
print("PASS" if j.get("pass") else "NOT PASS", sys.argv[1])
for k in ("grab_nodes", "grab_refused", "daemon_phase_during", "released_early", "grab_restored",
          "daemon_phase_after", "page_ready_after_isolation", "injections", "tally_notes",
          "non_key_events", "capture_frames_in_hold", "capture_error", "notes"):
    print(f"  {k}: {j.get(k)}")
lock = j.get("lock_teardown") or {}
print("  lock:", lock.get("engaged_after_ms"), (lock.get("unlock") or {}).get("method"))
PY
else
  echo "no integrated.json written; read $latest"
fi
echo "Remember: sudo setfacl -x u:user /dev/input/event2 /dev/input/event3 /dev/input/event4 /dev/input/event5"
