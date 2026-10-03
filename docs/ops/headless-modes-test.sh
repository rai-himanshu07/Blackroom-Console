#!/usr/bin/env bash
# Session modes against a throwaway headless Shell (private D-Bus, never the real screen):
#   cargo build -p blackroom-console && BR_BIN=docs/ops/headless-modes-test.sh docs/ops/headless-repro.sh
# Private (blank the panel) and shared (leave the screen alone), a custom resolution, and refused options.
set -u
cd "$(dirname "$0")/../.." || exit 2
BIN=target/debug/blackroom-console
[ -x "$BIN" ] || { echo "build first: cargo build -p blackroom-console"; exit 2; }
PORT=18083
log=$(mktemp /tmp/br-modes.XXXXXX)
"$BIN" --headless --listen "127.0.0.1:$PORT" --heartbeat-secs 60 --state-dir "$(mktemp -d /tmp/br-modes-state.XXXXXX)" > "$log" 2>&1 &
srv=$!
trap 'kill "$srv" 2>/dev/null' EXIT
url=""
for _ in $(seq 1 50); do url=$(grep -o "http://127.0.0.1:$PORT/?t=[0-9a-f]*" "$log" | head -n 1); [ -n "$url" ] && break; sleep 0.2; done
[ -n "$url" ] || { echo "FAIL: no URL printed"; cat "$log"; exit 1; }
jar=$(mktemp); base="http://127.0.0.1:$PORT"; fail=0
check() { if [ "$2" = "$3" ]; then echo "ok   $1 = $2"; else echo "FAIL $1: got $2, want $3"; fail=1; fi; }
json=(-H 'Content-Type: application/json')
virtuals() { busctl --user call org.gnome.Mutter.DisplayConfig /org/gnome/Mutter/DisplayConfig org.gnome.Mutter.DisplayConfig GetCurrentState 2>/dev/null | grep -o '"Meta-[0-9]*"' | sort -u | wc -l; }
start() { curl -s -b "$jar" "${json[@]}" -X POST -d "$1" -o /tmp/br-modes-start.json -w '%{http_code}' "$base/start"; }
stop() { curl -s -b "$jar" -X POST -o /tmp/br-modes-stop.json -w '%{http_code}' "$base/stop"; }
status() { curl -s -b "$jar" "$base/status"; }
jpeg_size() { curl -s -b "$jar" --max-time 3 "$base/video" -o /tmp/br-modes.mjpeg; python3 - <<'PY'
import re
data = open("/tmp/br-modes.mjpeg", "rb").read()
start = data.find(b"\xff\xd8")
i = start + 2
while i < len(data) - 9:
    if data[i] != 0xFF:
        i += 1; continue
    marker = data[i + 1]
    if marker in (0xC0, 0xC1, 0xC2):
        h = int.from_bytes(data[i + 5:i + 7], "big"); w = int.from_bytes(data[i + 7:i + 9], "big")
        print(f"{w}x{h}"); break
    i += 2 + int.from_bytes(data[i + 2:i + 4], "big")
else:
    print("none")
PY
}
curl -s -c "$jar" -o /dev/null "$url"
v0=$(virtuals)
echo "== refused options (baseline virtual outputs: $v0)"
for bad in '{"nope":1}' '{"heartbeat_secs":1}' '{"resolution":{"width":10,"height":10}}' '{"idle_minutes":-1}' '[1]' '{"blank_panel":"yes"}'; do
  check "refused $bad" "$(start "$bad")" 400
done
check "still idle after refusals" "$(status | jq -r .phase)" idle

echo "== shared: the screen is left alone"
check "shared start" "$(start '{"blank_panel":false,"block_local_input":false,"lock_on_stop":false}')" 200
st=$(status)
check "mode" "$(echo "$st" | jq -r .mode)" shared
check "no virtual monitor was added" "$(virtuals)" "$v0"
check "capture is the monitor's own size" "$(jpeg_size)" 1920x1080
events='[{"t":"move","x":0.5,"y":0.5},{"t":"key","code":42,"down":true},{"t":"key","code":42,"down":false}]'
check "input" "$(curl -s -b "$jar" "${json[@]}" -X POST -d "$events" -o /dev/null -w '%{http_code}' "$base/input")" 204
sleep 1
check "input accepted" "$(status | jq -r '.input_accepted >= 3')" true
check "shared stop" "$(stop)" 200
check "nothing to restore" "$(jq -r '.topology_restored' /tmp/br-modes-stop.json)" null
check "no errors" "$(jq -r '.errors | length' /tmp/br-modes-stop.json)" 0
check "outputs unchanged after" "$(virtuals)" "$v0"

echo "== private with a custom resolution"
check "private start" "$(start '{"resolution":{"width":1281,"height":721}}')" 200
st=$(status)
check "mode" "$(echo "$st" | jq -r .mode)" private
check "size is even and as asked" "$(echo "$st" | jq -r '"\(.width)x\(.height)"')" 1280x720
check "virtual monitor delivers that size" "$(jpeg_size)" 1280x720
check "private stop" "$(stop)" 200
check "topology restored" "$(jq -r '.topology_restored' /tmp/br-modes-stop.json)" true
check "no errors" "$(jq -r '.errors | length' /tmp/br-modes-stop.json)" 0
check "outputs back to baseline" "$(virtuals)" "$v0"

echo "== mixed: panel stays lit, input would be blocked (no daemon here)"
check "custom start" "$(start '{"blank_panel":false,"block_local_input":true}')" 200
check "mode" "$(status | jq -r .mode)" custom
check "custom stop" "$(stop)" 200

echo "== defaults without a body are private"
check "start without a body" "$(curl -s -b "$jar" -X POST -o /dev/null -w '%{http_code}' "$base/start")" 200
check "mode" "$(status | jq -r .mode)" private
check "stop" "$(stop)" 200
[ "$fail" = 0 ] && echo "MODES OK" || echo "SOME FAILED; server log: $log"
exit "$fail"
