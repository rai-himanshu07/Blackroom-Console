#!/usr/bin/env bash
# HTTP end-to-end check of blackroom-console against a throwaway headless Shell (private D-Bus, never the real screen):
#   cargo build -p blackroom-console && BR_BIN=docs/ops/headless-console-test.sh docs/ops/headless-repro.sh
set -u
cd "$(dirname "$0")/../.." || exit 2
BIN=target/debug/blackroom-console
[ -x "$BIN" ] || { echo "build first: cargo build -p blackroom-console"; exit 2; }
PORT=18080
log=$(mktemp /tmp/br-console.XXXXXX)
"$BIN" --headless --listen "127.0.0.1:$PORT" --heartbeat-secs 5 --state-dir "$(mktemp -d /tmp/br-console-state.XXXXXX)" > "$log" 2>&1 &
srv=$!
trap 'kill "$srv" 2>/dev/null' EXIT
url=""
for _ in $(seq 1 50); do
  url=$(grep -o "http://127.0.0.1:$PORT/?t=[0-9a-f]*" "$log" | head -n 1)
  [ -n "$url" ] && break
  sleep 0.2
done
[ -n "$url" ] || { echo "FAIL: no URL printed"; cat "$log"; exit 1; }
jar=$(mktemp)
base="http://127.0.0.1:$PORT"
fail=0
check() { if [ "$2" = "$3" ]; then echo "ok   $1 = $2"; else echo "FAIL $1: got $2, want $3"; fail=1; fi; }
json=(-H 'Content-Type: application/json')

check "no cookie" "$(curl -s -o /dev/null -w '%{http_code}' "$base/")" 401
check "login" "$(curl -s -c "$jar" -o /dev/null -w '%{http_code}' "$url")" 303
check "page" "$(curl -s -b "$jar" -o /dev/null -w '%{http_code}' "$base/")" 200
check "start" "$(curl -s -b "$jar" -X POST -o /tmp/br-start.json -w '%{http_code}' "$base/start")" 200
echo "start reply: $(cat /tmp/br-start.json)"

curl -s -b "$jar" --max-time 5 "$base/video" -o /tmp/br-mjpeg.bin
parts=$(grep -a -c '^Content-Type: image/jpeg' /tmp/br-mjpeg.bin)
echo "mjpeg parts in 5 s: $parts ($(stat -c %s /tmp/br-mjpeg.bin) bytes)"
[ "$parts" -ge 3 ] || { echo "FAIL: fewer than 3 frames (keepalive broken?)"; fail=1; }

events='[{"t":"move","x":0.5,"y":0.5},{"t":"button","code":272,"down":true},{"t":"button","code":272,"down":false},{"t":"key","code":30,"down":true},{"t":"key","code":30,"down":false},{"t":"scroll","dx":0,"dy":10}]'
check "input" "$(curl -s -b "$jar" "${json[@]}" -X POST -d "$events" -o /dev/null -w '%{http_code}' "$base/input")" 204
check "bad input" "$(curl -s -b "$jar" "${json[@]}" -X POST -d '[{"t":"key","code":116,"down":true}]' -o /dev/null -w '%{http_code}' "$base/input")" 400
sleep 1
st=$(curl -s -b "$jar" "$base/status")
echo "status: $st"
check "input accepted" "$(echo "$st" | jq -r .input_accepted)" 6

# No heartbeat now: Stop must run by itself (5 s timeout).
sleep 8
st=$(curl -s -b "$jar" "$base/status")
check "auto stop phase" "$(echo "$st" | jq -r .phase)" idle
check "auto stop reason" "$(echo "$st" | jq -r .last_stop.reason)" "browser heartbeat lost"
check "auto stop restored" "$(echo "$st" | jq -r .last_stop.topology_restored)" true

# A client that is silent for less than the timeout is not stopped, and one heartbeat renews the window.
check "start for the gap test" "$(curl -s -b "$jar" -X POST -o /dev/null -w '%{http_code}' "$base/start")" 200
sleep 3
check "short gap keeps the session" "$(curl -s -b "$jar" "$base/status" | jq -r .phase)" running
check "heartbeat" "$(curl -s -b "$jar" "${json[@]}" -X POST -d '[]' -o /dev/null -w '%{http_code}' "$base/input")" 204
sleep 3
check "heartbeat renewed the window" "$(curl -s -b "$jar" "$base/status" | jq -r .phase)" running
check "stop after the gap test" "$(curl -s -b "$jar" -X POST -o /dev/null -w '%{http_code}' "$base/stop")" 200

check "start again" "$(curl -s -b "$jar" -X POST -o /dev/null -w '%{http_code}' "$base/start")" 200
check "manual stop" "$(curl -s -b "$jar" -X POST -o /tmp/br-stop.json -w '%{http_code}' "$base/stop")" 200
echo "stop reply: $(cat /tmp/br-stop.json)"
check "manual stop restored" "$(jq -r .topology_restored /tmp/br-stop.json)" true
check "manual stop errors" "$(jq -r '.errors | length' /tmp/br-stop.json)" 0
[ "$fail" = 0 ] && echo "ALL OK" || { echo "SOME FAILED; server log: $log"; }
exit "$fail"
