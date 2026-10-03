#!/usr/bin/env bash
# Clipboard round trip through a throwaway headless Shell (private D-Bus, never the real screen or clipboard):
#   cargo build -p blackroom-console && cargo build -p blackroom-experiments --bin clip_peer \
#     && BR_BIN=docs/ops/headless-clipboard-test.sh docs/ops/headless-repro.sh
# A second RemoteDesktop session (clip_peer) stands in for a laptop application that copies or pastes.
set -u
cd "$(dirname "$0")/../.." || exit 2
BIN=target/debug/blackroom-console
PEER=target/debug/clip_peer
[ -x "$BIN" ] && [ -x "$PEER" ] || { echo "build first: blackroom-console and clip_peer"; exit 2; }
PORT=18082
log=$(mktemp /tmp/br-clip.XXXXXX)
"$BIN" --headless --clipboard --listen "127.0.0.1:$PORT" --heartbeat-secs 60 --state-dir "$(mktemp -d /tmp/br-clip-state.XXXXXX)" > "$log" 2>&1 &
srv=$!
trap 'kill "$srv" 2>/dev/null' EXIT
url=""
for _ in $(seq 1 50); do
  url=$(grep -o "http://127.0.0.1:$PORT/?t=[0-9a-f]*" "$log" | head -n 1)
  [ -n "$url" ] && break
  sleep 0.2
done
[ -n "$url" ] || { echo "FAIL: no URL printed"; cat "$log"; exit 1; }
jar=$(mktemp); base="http://127.0.0.1:$PORT"; fail=0
check() { if [ "$2" = "$3" ]; then echo "ok   $1 = $2"; else echo "FAIL $1: got $2, want $3"; fail=1; fi; }
text=(-H 'Content-Type: text/plain; charset=utf-8')

check "login" "$(curl -s -c "$jar" -o /dev/null -w '%{http_code}' "$url")" 303
check "idle clipboard refused" "$(curl -s -b "$jar" "${text[@]}" -X POST -d x -o /dev/null -w '%{http_code}' "$base/clipboard")" 409
check "start" "$(curl -s -b "$jar" -X POST -o /dev/null -w '%{http_code}' "$base/start")" 200
check "status offers the clipboard" "$(curl -s -b "$jar" "$base/status" | jq -r .clipboard)" true
sleep 1

# Browser -> laptop: a laptop application pastes (the peer reads through Mutter's selection).
want="tablet to laptop: grüße ✓ $(date +%s)"
check "set from the browser" "$(curl -s -b "$jar" "${text[@]}" -X POST --data-binary "$want" -o /dev/null -w '%{http_code}' "$base/clipboard")" 204
got=$("$PEER" read 2>/tmp/br-clip-peer.err)
check "laptop paste gets the browser text" "$got" "$want"
sleep 1
check "second paste gets it too" "$("$PEER" read 2>/dev/null)" "$want"

# Laptop -> browser: a laptop application copies (the peer owns the selection).
sleep 1
"$PEER" serve "laptop to tablet: ünïcode ✓" 20 > /tmp/br-clip-serve.out 2>&1 &
peer=$!
for _ in $(seq 1 50); do grep -q serving /tmp/br-clip-serve.out && break; sleep 0.2; done
sleep 1
check "get the laptop clipboard" "$(curl -s -b "$jar" "$base/clipboard")" "laptop to tablet: ünïcode ✓"
kill "$peer" 2>/dev/null; wait "$peer" 2>/dev/null

# Limits.
sleep 1
big=$(head -c 262145 /dev/zero | tr '\0' x)
check "over 256 KiB refused" "$(printf %s "$big" | curl -s -b "$jar" "${text[@]}" -X POST --data-binary @- -o /dev/null -w '%{http_code}' "$base/clipboard")" 413
sleep 1
max=$(head -c 262144 /dev/zero | tr '\0' y)
check "exactly 256 KiB accepted" "$(printf %s "$max" | curl -s -b "$jar" "${text[@]}" -X POST --data-binary @- -o /dev/null -w '%{http_code}' "$base/clipboard")" 204
sleep 1
check "256 KiB reaches the laptop" "$("$PEER" read 2>/dev/null | head -c 262144 | wc -c)" 262144
sleep 1
curl -s -b "$jar" "${text[@]}" -X POST -d a -o /dev/null "$base/clipboard"
check "second request inside 500 ms is rate limited" "$(curl -s -b "$jar" "${text[@]}" -X POST -d b -o /dev/null -w '%{http_code}' "$base/clipboard")" 429
check "cross-origin refused" "$(curl -s -b "$jar" -H 'Origin: http://evil.example' "${text[@]}" -X POST -d b -o /dev/null -w '%{http_code}' "$base/clipboard")" 403
check "no cookie refused" "$(curl -s -o /dev/null -w '%{http_code}' "$base/clipboard")" 401

# Nothing is retained after Stop, and a stopped console refuses.
check "stop" "$(curl -s -b "$jar" -X POST -o /dev/null -w '%{http_code}' "$base/stop")" 200
sleep 1
check "clipboard after stop" "$(curl -s -b "$jar" -o /dev/null -w '%{http_code}' "$base/clipboard")" 409
if grep -q "tablet to laptop\|laptop to tablet" "$log"; then echo "FAIL: clipboard text reached the server log"; fail=1; else echo "ok   clipboard text never logged"; fi
kill "$srv" 2>/dev/null; wait "$srv" 2>/dev/null

# Off unless started with --clipboard.
"$BIN" --headless --listen "127.0.0.1:$PORT" --state-dir "$(mktemp -d /tmp/br-clip-state.XXXXXX)" > "$log" 2>&1 &
srv=$!
url=""
for _ in $(seq 1 50); do url=$(grep -o "http://127.0.0.1:$PORT/?t=[0-9a-f]*" "$log" | head -n 1); [ -n "$url" ] && break; sleep 0.2; done
curl -s -c "$jar" -o /dev/null "$url"
check "off by default: set" "$(curl -s -b "$jar" "${text[@]}" -X POST -d x -o /dev/null -w '%{http_code}' "$base/clipboard")" 403
check "off by default: status" "$(curl -s -b "$jar" "$base/status" | jq -r .clipboard)" false
[ "$fail" = 0 ] && echo "CLIPBOARD OK" || echo "SOME FAILED; server log: $log"
exit "$fail"
