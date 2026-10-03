#!/usr/bin/env bash
# Browser end-to-end check against a throwaway headless Shell (private D-Bus, never the real screen): headless Chrome logs in
# over https, presses Start, expects WebRTC H.264 video, changes quality, falls back to MJPEG, presses Stop.
#   cargo build -p blackroom-console && BR_TIMEOUT=120 BR_BIN=docs/ops/headless-browser-test.sh docs/ops/headless-repro.sh
set -u
cd "$(dirname "$0")/../.." || exit 2
BIN=target/debug/blackroom-console
[ -x "$BIN" ] || { echo "build first: cargo build -p blackroom-console"; exit 2; }
log=$(mktemp /tmp/br-console.XXXXXX)
state=$(mktemp -d /tmp/br-console-state.XXXXXX)
"$BIN" --headless --clipboard --listen 127.0.0.1:18080 --tls-listen 127.0.0.1:18443 --cert-dir "$state/cert" --state-dir "$state" > "$log" 2>&1 &
srv=$!
trap 'kill "$srv" 2>/dev/null' EXIT
url=""
for _ in $(seq 1 50); do
  url=$(grep -o "https://127.0.0.1:18443/?t=[0-9a-f]*" "$log" | head -n 1)
  [ -n "$url" ] && break
  sleep 0.2
done
[ -n "$url" ] || { echo "FAIL: no URL printed"; cat "$log"; exit 1; }
node docs/ops/headless-browser-test.mjs "$url"
rc=$?
echo "server log: $log"
exit "$rc"
