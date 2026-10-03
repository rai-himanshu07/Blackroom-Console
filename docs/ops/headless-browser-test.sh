#!/usr/bin/env bash
# Browser end-to-end check against a throwaway headless Shell (private D-Bus, never the real screen): headless Chrome logs in
# over https, presses Start, expects WebRTC H.264 video, changes quality, falls back to MJPEG, presses Stop.
#   cargo build -p blackroom-console && BR_TIMEOUT=120 BR_BIN=docs/ops/headless-browser-test.sh docs/ops/headless-repro.sh
set -u
cd "$(dirname "$0")/../.." || exit 2
BIN=target/debug/blackroom-console
[ -x "$BIN" ] || { echo "build first: cargo build -p blackroom-console"; exit 2; }
log=$(mktemp /tmp/br-console.XXXXXX)
# A silent PipeWire sink with a tone playing into it stands in for the laptop's sound output (nothing reaches the speakers).
state=$(mktemp -d /tmp/br-console-state.XXXXXX)
audio_args=()
sink="br_test_sink_$$"
if command -v pw-cli > /dev/null && command -v pw-play > /dev/null; then
  python3 - "$state/tone.wav" <<'PY'
import math, struct, sys, wave
w = wave.open(sys.argv[1], "wb"); w.setnchannels(2); w.setsampwidth(2); w.setframerate(48000)
tone = [int(8000 * math.sin(2 * math.pi * 440 * i / 48000)) for i in range(48000 * 10)]
w.writeframes(b"".join(struct.pack("<hh", v, v) for v in tone)); w.close()
PY
  mkfifo "$state/pwcli.fifo"
  pw-cli < "$state/pwcli.fifo" > /dev/null 2>&1 &
  exec 8> "$state/pwcli.fifo"
  echo "create-node adapter { factory.name=support.null-audio-sink node.name=$sink media.class=Audio/Sink object.linger=false audio.position=[FL FR] }" >&8
  sleep 1.5
  ( while true; do pw-play --target "$sink" "$state/tone.wav" > /dev/null 2>&1; done ) 8>&- &
  tone=$!
  audio_args=(--audio-sink "$sink")
  export BR_AUDIO_TEST=1
fi
"$BIN" --headless --clipboard --listen 127.0.0.1:18080 --tls-listen 127.0.0.1:18443 --cert-dir "$state/cert" --state-dir "$state" "${audio_args[@]}" > "$log" 2>&1 8>&- &
srv=$!
trap 'kill "$srv" 2>/dev/null; [ -n "${tone:-}" ] && { kill "$tone" 2>/dev/null; pkill -f "pw-play --target $sink" 2>/dev/null; }; echo quit >&8 2>/dev/null; exec 8>&- 2>/dev/null' EXIT
url=""
for _ in $(seq 1 50); do
  url=$(grep -o "https://127.0.0.1:18443/?t=[0-9a-f]*" "$log" | head -n 1)
  [ -n "$url" ] && break
  sleep 0.2
done
[ -n "$url" ] || { echo "FAIL: no URL printed"; cat "$log"; exit 1; }
node docs/ops/headless-browser-test.mjs "$url" 8>&-
rc=$?
echo "server log: $log"
exit "$rc"
