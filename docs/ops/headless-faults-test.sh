#!/usr/bin/env bash
# Fault checks against a throwaway headless Shell (private D-Bus, never the real screen): the console is killed
# with SIGKILL mid-session, twice, and a new console must find the Shell alive, the virtual output gone and be
# able to Start and Stop again. (The real-session dead-man restore and lock are NOT covered: headless has none.)
#   cargo build -p blackroom-console && BR_BIN=docs/ops/headless-faults-test.sh docs/ops/headless-repro.sh
set -u
cd "$(dirname "$0")/../.." || exit 2
BIN=target/debug/blackroom-console
[ -x "$BIN" ] || { echo "build first: cargo build -p blackroom-console"; exit 2; }
PORT=18084
fail=0
note_fail() { echo "FAIL $*"; fail=$((fail + 1)); }
virtuals() { busctl --user call org.gnome.Mutter.DisplayConfig /org/gnome/Mutter/DisplayConfig org.gnome.Mutter.DisplayConfig GetCurrentState 2>/dev/null | grep -o '"Meta-[0-9]*"' | sort -u | wc -l; }
v0=$(virtuals)
shell_pid=$(pgrep -f -- "--wayland-display=blackroom-repro" | head -n 1)

launch() {
  log=$(mktemp /tmp/br-faults.XXXXXX)
  "$BIN" --headless --listen "127.0.0.1:$PORT" --heartbeat-secs 30 --state-dir "$state" > "$log" 2>&1 &
  srv=$!
  url=""
  for _ in $(seq 1 50); do url=$(grep -o "http://127.0.0.1:$PORT/?t=[0-9a-f]*" "$log" | head -n 1); [ -n "$url" ] && break; sleep 0.2; done
  jar=$(mktemp); curl -s -c "$jar" -o /dev/null "$url"
}
code() { curl -s -b "$jar" -X POST -o "${2:-/dev/null}" -w '%{http_code}' --max-time 60 "http://127.0.0.1:$PORT/$1"; }
state=$(mktemp -d /tmp/br-faults-state.XXXXXX)
trap 'kill -9 "$srv" 2>/dev/null' EXIT

for round in 1 2; do
  echo "== round $round: SIGKILL mid-session"
  launch
  [ "$(code start)" = 200 ] || note_fail "round $round: start"
  sleep 2
  [ "$(virtuals)" -gt "$v0" ] || note_fail "round $round: no virtual output while streaming ($(virtuals), baseline $v0)"
  kill -9 "$srv"; wait "$srv" 2>/dev/null
  for _ in $(seq 1 20); do [ "$(virtuals)" = "$v0" ] && break; sleep 0.5; done
  [ "$(virtuals)" = "$v0" ] || note_fail "round $round: virtual output still present after the owner died"
  kill -0 "$shell_pid" 2>/dev/null || note_fail "round $round: the Shell died"
  echo "after kill: virtual outputs $(virtuals) (baseline $v0), Shell alive: $(kill -0 "$shell_pid" 2>/dev/null && echo yes || echo NO)"
done

echo "== new console after the crashes"
launch
[ "$(code start)" = 200 ] || note_fail "start after crashes"
sleep 1
code stop /tmp/br-faults-stop.json > /dev/null
[ "$(jq -r .topology_restored /tmp/br-faults-stop.json)" = true ] || note_fail "stop after crashes: $(cat /tmp/br-faults-stop.json)"
[ "$(virtuals)" = "$v0" ] || note_fail "virtual output left after the final stop"
[ "$fail" = 0 ] && echo "ALL OK" || echo "$fail FAILURE(S)"
exit "$fail"
