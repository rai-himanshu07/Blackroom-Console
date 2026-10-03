#!/usr/bin/env bash
# The laptop-side control channel and top-bar indicator against a throwaway headless Shell (private D-Bus, never the
# real screen or session):
#   cargo build -p blackroom-console && docs/ops/headless-indicator-test.sh
# Checks the console's D-Bus service (state, Disconnect), then loads the indicator extension into the throwaway Shell and
# reads its log lines. What it does not show: the icon on a real panel, notifications, the menu clicks.
set -u
cd "$(dirname "$0")/../.." || exit 2
UUID=blackroom-indicator@blackroom.local
if [ "${BR_INDICATOR_INNER:-}" != 1 ]; then
  data=$(mktemp -d /tmp/br-ext-data.XXXXXX)
  mkdir -p "$data/gnome-shell/extensions" "$data/config/glib-2.0/settings"
  cp -r "docs/ops/gnome-extension/$UUID" "$data/gnome-shell/extensions/"
  printf "[org/gnome/shell]\nenabled-extensions=['%s']\n" "$UUID" > "$data/config/glib-2.0/settings/keyfile"
  BR_INDICATOR_INNER=1 BR_SHELL_DATA_DIR="$data" BR_SHELL_CONFIG_DIR="$data/config" BR_BIN="$0" BR_TIMEOUT=150 \
    docs/ops/headless-repro.sh
  rc=$?; rm -rf "$data"; exit $rc
fi
BIN=target/debug/blackroom-console
[ -x "$BIN" ] || { echo "build first: cargo build -p blackroom-console"; exit 2; }
PORT=18084
log=$(mktemp /tmp/br-indicator.XXXXXX)
state=$(mktemp -d /tmp/br-indicator-state.XXXXXX)
# The owner wants to approve every connection; the headless console reads host.json from its state directory.
echo '{"approval":"ask"}' > "$state/host.json"
"$BIN" --headless --listen "127.0.0.1:$PORT" --heartbeat-secs 60 --state-dir "$state" > "$log" 2>&1 &
srv=$!
trap 'kill "$srv" 2>/dev/null' EXIT
url=""
for _ in $(seq 1 50); do url=$(grep -o "http://127.0.0.1:$PORT/?t=[0-9a-f]*" "$log" | head -n 1); [ -n "$url" ] && break; sleep 0.2; done
[ -n "$url" ] || { echo "FAIL: no URL printed"; cat "$log"; exit 1; }
jar=$(mktemp); base="http://127.0.0.1:$PORT"; fail=0; clicked=0
check() { if [ "$2" = "$3" ]; then echo "ok   $1 = $2"; else echo "FAIL $1: got $2, want $3"; fail=1; fi; }
bus() { busctl --user --json=short call org.blackroom.Console /org/blackroom/Console org.blackroom.Console1 "$@"; }
dstatus() { bus Status | jq -r '.data[0] | fromjson'; }
shell_log() { grep -c "$1" "$BR_SHELL_LOG"; }
curl -s -c "$jar" -o /dev/null "$url"

echo "== D-Bus service"
st=$(dstatus)
check "idle at first" "$(echo "$st" | jq -r .phase)" idle
check "local page address" "$(echo "$st" | jq -r .local_url)" "http://localhost:$PORT/"
check "nothing to disconnect" "$(bus Disconnect | jq -r '.data[0]')" idle
check "no secrets in the reply" "$(echo "$st" | jq -r 'keys | map(select(test("token|secret|cookie|password"))) | length')" 0

echo "== indicator extension (enabled at Shell start through its private settings)"
sleep 3
check "indicator saw the idle console" "$(shell_log 'Blackroom indicator: idle')" 1

echo "== a shared session seen from both sides"
json=(-H 'Content-Type: application/json')
shared='{"blank_panel":false,"block_local_input":false,"lock_on_stop":false}'
ask_start() { curl -s -b "$jar" "${json[@]}" -X POST -d "$shared" -o /tmp/br-ind-start.json -w '%{http_code}' --max-time 60 "$base/start" > /tmp/br-ind-code & startpid=$!; }
pending_id() { for _ in $(seq 1 30); do id=$(dstatus | jq -r '.pending.id // empty'); [ -n "$id" ] && { echo "$id"; return; }; sleep 0.5; done; }
ask_start
id=$(pending_id)
check "a start waits for the owner" "$([ -n "$id" ] && echo yes)" yes
check "the waiting request names the mode and the device" "$(dstatus | jq -r '.pending | "\(.mode) \(.device | test("^127.0.0.1 "))"')" "shared true"
check "still idle while it waits" "$(dstatus | jq -r .phase)" idle
check "a stale id is refused" "$(bus Approve t $((id + 9)) | jq -r '.data[0]')" false
sleep 2
check "the indicator raised the request" "$(shell_log 'Blackroom indicator: connection request')" 1
check "approve" "$(bus Approve t "$id" | jq -r '.data[0]')" true
wait "$startpid"
check "the start went through" "$(cat /tmp/br-ind-code)" 200
sleep 3
st=$(dstatus)
check "D-Bus phase" "$(echo "$st" | jq -r .phase)" running
check "D-Bus mode" "$(echo "$st" | jq -r .mode)" shared
check "screen not blank" "$(echo "$st" | jq -r .blank_panel)" false
if [ -n "${BR_INDICATOR_SHOT:-}" ]; then
  # The shared-mode capture is the whole monitor, top bar included (the Shell refuses its own screenshot call).
  shot() {
    curl -s -b "$jar" --max-time 4 "$base/video" -o /tmp/br-indicator.mjpeg
    python3 - "$1" <<'PY'
import sys
data = open("/tmp/br-indicator.mjpeg", "rb").read()
frames, i = [], 0
while True:
    a = data.find(b"\xff\xd8", i)
    b = data.find(b"\xff\xd9", a + 2) if a >= 0 else -1
    if a < 0 or b < 0:
        break
    frames.append(data[a:b + 2]); i = b + 2
open(sys.argv[1], "wb").write(frames[-1])
PY
  }
  shot "$BR_INDICATOR_SHOT"
  if [ -n "${BR_INDICATOR_ASK_SHOT:-}" ]; then
    # A second client asks while the first is connected: the notice and the menu rows can be seen.
    ask_start; ask_id=$(pending_id); sleep 3
    shot "$BR_INDICATOR_ASK_SHOT"
    # The icon sits a little further right while it shows "?"; open its menu and look at the Accept and Deny rows.
    curl -s -b "$jar" "${json[@]}" -X POST -d '[{"t":"move","x":0.878,"y":0.0148},{"t":"button","code":272,"down":true},{"t":"button","code":272,"down":false}]' -o /dev/null "$base/input"
    sleep 2
    shot "${BR_INDICATOR_ASK_SHOT%.jpg}-menu.jpg"
    bus Deny t "$ask_id" > /dev/null; wait "$startpid"
  fi
  if [ -n "${BR_INDICATOR_MENU_SHOT:-}" ]; then
    # Click the icon (about 1662,15 on the 1920x1080 monitor) to open its menu.
    curl -s -b "$jar" "${json[@]}" -X POST -d '[{"t":"move","x":0.8656,"y":0.0139},{"t":"button","code":272,"down":true},{"t":"button","code":272,"down":false}]' -o /dev/null "$base/input"
    sleep 2
    shot "$BR_INDICATOR_MENU_SHOT"
    # "Disconnect the remote user" is the first action row (about 1540,189 with the menu open).
    curl -s -b "$jar" "${json[@]}" -X POST -d '[{"t":"move","x":0.802,"y":0.175},{"t":"button","code":272,"down":true},{"t":"button","code":272,"down":false}]' -o /dev/null "$base/input"
    clicked=1
  fi
fi
check "indicator saw the session" "$(shell_log 'Blackroom indicator: running mode=shared')" 1
if [ "$clicked" = 1 ]; then
  echo "(the menu's Disconnect row was clicked; D-Bus Disconnect is not called)"
else
  check "disconnect from the laptop" "$(bus Disconnect | jq -r '.data[0]')" stopping
fi
for _ in $(seq 1 30); do [ "$(dstatus | jq -r .phase)" = idle ] && break; sleep 1; done
check "idle again" "$(dstatus | jq -r .phase)" idle
check "the page side agrees" "$(curl -s -b "$jar" "$base/status" | jq -r .phase)" idle
check "the stop report is kept" "$(dstatus | jq -r '.last_stop.reason | length > 0')" true
sleep 3
# The first idle reading was counted above; the indicator starts with one "off" line before the first reply.
check "indicator saw it end" "$(shell_log 'Blackroom indicator: idle mode=')" 2

echo "== deny and silence"
ask_start
id=$(pending_id)
check "deny" "$(bus Deny t "$id" | jq -r '.data[0]')" true
wait "$startpid"
check "a denied start is refused" "$(cat /tmp/br-ind-code) $(jq -r '.error' /tmp/br-ind-start.json | grep -c denied)" "403 1"
check "nothing started" "$(dstatus | jq -r .phase)" idle
if [ "${BR_INDICATOR_SLOW:-1}" = 1 ]; then
  ask_start
  wait "$startpid"
  check "no answer in 30 s is a refusal" "$(cat /tmp/br-ind-code) $(jq -r '.error' /tmp/br-ind-start.json | grep -c 'in time')" "403 1"
fi

echo "== console stop"
kill "$srv"; wait "$srv" 2>/dev/null
sleep 3
check "indicator saw the console go" "$(shell_log 'Blackroom indicator: off')" 2
check "the service name is gone with the console" "$(busctl --user --no-pager list 2>/dev/null | grep -c 'org.blackroom.Console')" 0
check "no JS errors from the extension" "$(grep -ci 'JS ERROR.*blackroom\|blackroom.*JS ERROR\|blackroom-indicator.*error' "$BR_SHELL_LOG")" 0
[ "$fail" = 0 ] && echo "INDICATOR OK" || { echo "INDICATOR FAILED"; tail -n 20 "$log"; }
exit "$fail"
