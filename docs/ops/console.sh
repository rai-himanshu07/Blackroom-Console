#!/usr/bin/env bash
# Starts the remote console: the input-grab daemon, then the web app. Run it from the repo root IN YOUR OWN
# TERMINAL (the tablet needs SSH to this laptop open as well):
#   docs/ops/console.sh --check     read-only preflight, changes nothing
#   docs/ops/console.sh --cleanup   after a crash or logout that skipped the cleanup: stops the kill timer, restore
#                                   timers and guard, the orphaned daemon, and unmasks gnome-remote-desktop; refuses
#                                   while a console is running; keeps recovery.json (the next start acts on it)
#   docs/ops/console.sh             start; prints the URL to open on the tablet; Ctrl-C stops everything
# Env: BR_TARGET=target/release (default) or target/debug; BR_PORT=8080 (http); BR_TLS_PORT=8443 (https, self-signed);
# BR_KILL_SECS=7200 (daemon kill timer); BR_TOKEN_FILE=<abs path> keeps the URL token across restarts (tests only); BR_AUTH_DIR=<abs dir> [BR_ACCOUNT=<name>] logs in with a TOTP code instead of the URL token; BR_TLS_CERT/BR_TLS_KEY/BR_PUBLIC/BR_STUN/BR_TURN/BR_TURN_SECRET_FILE/BR_ICE_PORTS enable internet access (docs/ops/internet-access.md); BR_HOSTD=1 logs in through remote-hostd (Linux password + authenticator + key or trusted browser; start remote-hostd.service first; see README). Use the https URL on a Chromium laptop for full keyboard capture.
#
# Needs temporary ACLs on the built-in input nodes (you run sudo, never this script):
#   sudo setfacl -m u:user:rw /dev/input/event2 /dev/input/event3 /dev/input/event4 /dev/input/event5
# and afterwards: sudo setfacl -x u:user /dev/input/event2 /dev/input/event3 /dev/input/event4 /dev/input/event5
#
# Recovery from the tablet over SSH if the panel stays black:
#   pkill -KILL -x remote-emergenc ; cd <repo> ; $BR_TARGET/exp07_restore --keep-live-virtual --lock-after --backup /run/user/1000/blackroom-console/backup.json
#   loginctl unlock-session "$(loginctl show-user $USER -p Display --value)"
set -u
cd "$(dirname "$0")/../.." || exit 2
T=${BR_TARGET:-target/release}
PORT=${BR_PORT:-8080}
TLS_PORT=${BR_TLS_PORT:-8443}
KILL_SECS=${BR_KILL_SECS:-7200}
export XDG_RUNTIME_DIR=/run/user/$(id -u)
LIVE=$XDG_RUNTIME_DIR/blackroom-live
SOCK=$LIVE/emergency.sock
SESS=$(loginctl show-user "$(id -un)" -p Display --value)
fail=0
bad() { echo "PREFLIGHT FAIL: $*"; fail=1; }

cleanup_leftovers() {
  if pgrep -x blackroom-conso > /dev/null; then
    echo "A console is running: stop it first (Stop in the page, or pkill -TERM -x blackroom-conso)."
    exit 1
  fi
  systemctl --user stop blackroom-live-kill.timer > /dev/null 2>&1
  systemctl --user stop 'blackroom-console-wd-*' > /dev/null 2>&1
  systemctl --user reset-failed 'blackroom-console-wd-*' 'blackroom-live-kill*' > /dev/null 2>&1
  pkill -x remote-emergenc > /dev/null 2>&1
  rm -f -- "$SOCK" "$LIVE/emergencyd.log"
  rmdir "$LIVE" 2> /dev/null
  systemctl --user unmask gnome-remote-desktop.service > /dev/null 2>&1
  systemctl --user disable --now gnome-remote-desktop.service > /dev/null 2>&1
  echo "timers left: $(systemctl --user list-timers --all --no-legend 'blackroom-*' 2> /dev/null | wc -l)"
  echo "daemon running: $(pgrep -x remote-emergenc > /dev/null && echo yes || echo no)"
  echo "gnome-remote-desktop: $(systemctl --user is-enabled gnome-remote-desktop.service 2>&1)"
  [ -e "$XDG_RUNTIME_DIR/blackroom-console/recovery.json" ] \
    && echo "recovery.json is kept: the next console start restores from it if it still applies."
  exit 0
}
case "${1:-}" in
  --cleanup) cleanup_leftovers ;;
  "" | --check) ;;
  *) echo "unknown argument: $1 (use --check or --cleanup)"; exit 2 ;;
esac

preflight() {
  for n in 2 3 4 5; do
    { [ -r "/dev/input/event$n" ] && [ -w "/dev/input/event$n" ]; } || bad "no rw access to /dev/input/event$n (run the setfacl)"
  done
  [ -n "$(ss -tnH state established '( sport = :22 )')" ] || bad "no second-device SSH session is connected"
  connected=$(grep -l '^connected$' /sys/class/drm/card*-*/status 2>/dev/null | sed 's|/sys/class/drm/||; s|/status||')
  [ "$connected" = "card1-eDP-1" ] || bad "connected outputs are not exactly the built-in panel: $connected"
  [ -z "$(systemctl --user list-timers --all --no-legend 'blackroom-*' 2>/dev/null)" ] || bad "a blackroom-* timer is pending"
  for p in remote-emergenc blackroom-conso remote-gateway exp06_isolate_o exp09_grab_prob; do
    pgrep -x "$p" > /dev/null && bad "$p is running"
  done
  # The login authority (remote-hostd --auth-service) is meant to run; only the offline simulation host is not.
  pgrep -f 'remote-hostd --offline-sim' > /dev/null && bad "a remote-hostd offline simulation is running"
  [ "$(systemctl --user is-active gnome-remote-desktop.service)" != "active" ] || bad "gnome-remote-desktop is active"
  [ "$(gsettings get org.gnome.desktop.lockdown disable-lock-screen)" = "false" ] || bad "disable-lock-screen is not false"
  if [ "$(loginctl show-session "$SESS" -p LockedHint --value)" != "no" ]; then
    gnome-extensions list --enabled --active | grep -qx blackroom-locked-remote@blackroom.local \
      || bad "session $SESS is locked and the blackroom-locked-remote extension is not enabled (docs/ops/README.md)"
  fi
  for b in blackroom-console exp07_restore remote-emergencyd blackroom; do
    [ -x "$T/$b" ] || bad "missing $T/$b (cargo build --release --workspace)"
  done
  echo "shell pid $(pgrep -x gnome-shell); AC: $(cat /sys/class/power_supply/AC*/online 2>/dev/null | head -n 1); session $SESS"
}

preflight
if [ "$fail" -ne 0 ]; then echo "Not starting."; exit 1; fi
if [ "${1:-}" = "--check" ]; then echo "Preflight OK (nothing changed)."; exit 0; fi

cat <<EOF
About to start the remote console. Have you: saved work, plugged in AC, kept the lid open, opened the tablet SSH session?
Pressing Start on the page blanks this panel and grabs the built-in keyboard and touchpad until you press Stop, the
chord (Left Ctrl+Left Shift+Left Alt+Esc, 2 s) is held, or the tablet sends no heartbeat for 15 s. Stop restores the
display and locks the screen; the grab is released after the lock. If the screen is locked at Start (extension
enabled), the page shows the lock screen: type the account password there; it is never bypassed. While the
extension is enabled, locking does not end remote sessions. Not yet observed: Safari/Android decode, iOS fullscreen,
idle blanking, a crashed console (the 60 s dead-man restore also locks; seen once).
EOF
read -r -p "Type START to begin: " answer
[ "$answer" = "START" ] || { echo "Aborted."; exit 1; }

app=0
cleanup() {
  [ "$app" != 0 ] && kill -TERM "$app" 2> /dev/null && wait "$app" 2> /dev/null
  systemctl --user stop blackroom-live-kill.timer > /dev/null 2>&1
  pkill -x remote-emergenc > /dev/null 2>&1
  systemctl --user unmask gnome-remote-desktop.service > /dev/null 2>&1
  systemctl --user disable --now gnome-remote-desktop.service > /dev/null 2>&1
  rm -f -- "$SOCK" "$LIVE/emergencyd.log"
  rmdir "$LIVE" 2> /dev/null
}
trap cleanup EXIT
trap 'exit 130' INT TERM HUP

install -d -m 700 "$LIVE" || exit 2
# 15 chars: the kernel truncates comm, a longer name would silently match nothing.
systemd-run --user --unit=blackroom-live-kill --on-active="$KILL_SECS" --timer-property=AccuracySec=1s \
  pkill -KILL -x remote-emergenc > /dev/null 2>&1 || exit 2
systemctl --user mask --now gnome-remote-desktop.service || exit 2
setsid nohup "$T/remote-emergencyd" --client-uid "$(id -u)" --enable-grabs --socket "$SOCK" \
  > "$LIVE/emergencyd.log" 2>&1 < /dev/null &
sleep 1
"$T/blackroom" emergency-status --socket "$SOCK" || { echo "daemon not answering"; exit 2; }

extra=()
[ -n "${BR_TOKEN_FILE:-}" ] && extra=(--token-file "$BR_TOKEN_FILE")
[ -n "${BR_AUTH_DIR:-}" ] && extra+=(--auth-dir "$BR_AUTH_DIR")
[ -n "${BR_HOSTD:-}" ] && extra+=(--hostd-dir "${XDG_RUNTIME_DIR:?}/blackroom-hostd")
[ -n "${BR_ACCOUNT:-}" ] && extra+=(--account "$BR_ACCOUNT")
# Internet access (docs/ops/internet-access.md): BR_TLS_CERT + BR_TLS_KEY (real certificate), BR_PUBLIC=1 (needs BR_HOSTD=1),
# BR_STUN / BR_TURN (space separated URLs), BR_TURN_SECRET_FILE, BR_ICE_PORTS=MIN-MAX.
[ -n "${BR_TLS_CERT:-}" ] && extra+=(--tls-cert "$BR_TLS_CERT" --tls-key "${BR_TLS_KEY:?BR_TLS_KEY is needed with BR_TLS_CERT}")
for url in ${BR_STUN:-}; do extra+=(--stun "$url"); done
for url in ${BR_TURN:-}; do extra+=(--turn "$url"); done
[ -n "${BR_TURN_SECRET_FILE:-}" ] && extra+=(--turn-secret-file "$BR_TURN_SECRET_FILE")
[ -n "${BR_ICE_PORTS:-}" ] && extra+=(--ice-port-range "$BR_ICE_PORTS")
listen_host=0.0.0.0
if [ -n "${BR_PUBLIC:-}" ]; then extra+=(--public); listen_host=127.0.0.1; fi
"$T/blackroom-console" --grab-socket "$SOCK" --listen "$listen_host:$PORT" --tls-listen "0.0.0.0:$TLS_PORT" "${extra[@]}" &
app=$!
wait "$app"
echo "console exited with $?"
