#!/usr/bin/env bash
# Start/Stop cycles and races against a throwaway headless Shell (private D-Bus, never the real screen):
# leaks (fds, threads, RSS, virtual monitors) and the Start/Stop/input race matrix.
#   cargo build -p blackroom-console && BR_CYCLES=100 BR_SHELL_TIMEOUT=1500 BR_TIMEOUT=1400 \
#     BR_BIN=docs/ops/headless-cycles-test.sh docs/ops/headless-repro.sh
set -u
cd "$(dirname "$0")/../.." || exit 2
BIN=target/debug/blackroom-console
[ -x "$BIN" ] || { echo "build first: cargo build -p blackroom-console"; exit 2; }
CYCLES=${BR_CYCLES:-20}
PORT=18082
log=$(mktemp /tmp/br-cycles.XXXXXX)
state=$(mktemp -d /tmp/br-cycles-state.XXXXXX)
"$BIN" --headless --clipboard --listen "127.0.0.1:$PORT" --heartbeat-secs 5 --state-dir "$state" > "$log" 2>&1 &
srv=$!
trap 'kill "$srv" 2>/dev/null' EXIT
url=""
for _ in $(seq 1 50); do url=$(grep -o "http://127.0.0.1:$PORT/?t=[0-9a-f]*" "$log" | head -n 1); [ -n "$url" ] && break; sleep 0.2; done
[ -n "$url" ] || { echo "FAIL: no URL printed"; cat "$log"; exit 1; }
jar=$(mktemp); base="http://127.0.0.1:$PORT"; fail=0
curl -s -c "$jar" -o /dev/null "$url"
json=(-H 'Content-Type: application/json')
note_fail() { echo "FAIL $*"; fail=$((fail + 1)); }
code() { curl -s -b "$jar" -X POST -o "${2:-/dev/null}" -w '%{http_code}' --max-time 60 "$base/$1"; }
phase() { curl -s -b "$jar" "$base/status" | jq -r .phase; }
virtuals() { busctl --user call org.gnome.Mutter.DisplayConfig /org/gnome/Mutter/DisplayConfig org.gnome.Mutter.DisplayConfig GetCurrentState 2>/dev/null | grep -o '"Meta-[0-9]*"' | sort -u | wc -l; }
fds() { ls "/proc/$srv/fd" | wc -l; }
threads() { awk '/^Threads:/ {print $2}' "/proc/$srv/status"; }
rss_kb() { awk '/^VmRSS:/ {print $2}' "/proc/$srv/status"; }

cycle() {
  local start stop
  start=$(code start /tmp/br-cyc-start.json)
  [ "$start" = 200 ] || { note_fail "cycle $1 start http $start: $(cat /tmp/br-cyc-start.json)"; return; }
  curl -s -b "$jar" "${json[@]}" -X POST -d '[{"t":"move","x":0.4,"y":0.4},{"t":"key","code":42,"down":true},{"t":"key","code":42,"down":false}]' -o /dev/null "$base/input"
  sleep 0.3
  # The clipboard path (its signal handling and pipes) is part of every cycle.
  [ "$(curl -s -b "$jar" -H 'Content-Type: text/plain' -X POST -d "cycle $1" -o /dev/null -w '%{http_code}' "$base/clipboard")" = 204 ] \
    || note_fail "cycle $1 clipboard set"
  stop=$(code stop /tmp/br-cyc-stop.json)
  [ "$stop" = 200 ] || note_fail "cycle $1 stop http $stop"
  [ "$(jq -r .topology_restored /tmp/br-cyc-stop.json)" = true ] || note_fail "cycle $1 topology: $(cat /tmp/br-cyc-stop.json)"
  [ "$(jq -r '.errors | length' /tmp/br-cyc-stop.json)" = 0 ] || note_fail "cycle $1 errors: $(jq -c .errors /tmp/br-cyc-stop.json)"
}

v0=$(virtuals)   # the headless Shell itself exposes one virtual output
echo "== race matrix (baseline virtual outputs: $v0)"
[ "$(code stop /tmp/br-race.json)" = 200 ] && [ "$(jq -r .reason /tmp/br-race.json)" = "not running" ] || note_fail "stop while idle: $(cat /tmp/br-race.json)"
[ "$(code start)" = 200 ] || note_fail "first start"
second=$(code start /tmp/br-race.json)
{ [ "$second" != 200 ] && grep -q "not idle" /tmp/br-race.json; } || note_fail "double start: http $second $(cat /tmp/br-race.json)"
[ "$(phase)" = running ] || note_fail "phase after double start: $(phase)"
code stop > /dev/null
[ "$(phase)" = idle ] || note_fail "phase after stop: $(phase)"
for i in $(seq 1 8); do
  code start > /dev/null & p1=$!
  code stop > /dev/null & p2=$!
  code start > /dev/null & p3=$!
  curl -s -b "$jar" "${json[@]}" -X POST -d '[{"t":"move","x":0.5,"y":0.5}]' -o /dev/null "$base/input" & p4=$!
  wait "$p1" "$p2" "$p3" "$p4"
  code stop > /dev/null
  [ "$(phase)" = idle ] || note_fail "race round $i left phase $(phase)"
  [ "$(virtuals)" = "$v0" ] || note_fail "race round $i left $(virtuals) virtual outputs (baseline $v0)"
done

echo "== $CYCLES cycles"
for i in 1 2 3; do cycle "warmup$i"; done
base_fds=$(fds); base_threads=$(threads); base_rss=$(rss_kb)
echo "baseline: fds=$base_fds threads=$base_threads rss_kb=$base_rss"
started=$(date +%s)
for i in $(seq 1 "$CYCLES"); do
  cycle "$i"
  if [ $((i % 10)) = 0 ]; then echo "cycle $i: fds=$(fds) threads=$(threads) rss_kb=$(rss_kb) virtuals=$(virtuals) failures=$fail"; fi
done
echo "cycles took $(( $(date +%s) - started )) s"
end_fds=$(fds); end_threads=$(threads); end_rss=$(rss_kb)
echo "end: fds=$end_fds threads=$end_threads rss_kb=$end_rss virtuals=$(virtuals)"
[ $((end_fds - base_fds)) -le 4 ] || note_fail "fd leak: $base_fds -> $end_fds"
[ $((end_threads - base_threads)) -le 2 ] || note_fail "thread leak: $base_threads -> $end_threads"
[ $((end_rss - base_rss)) -le 40000 ] || note_fail "rss growth: $base_rss -> $end_rss kB"
[ "$(virtuals)" = "$v0" ] || note_fail "virtual monitor left behind"
[ "$(phase)" = idle ] || note_fail "not idle at the end"
res=$(curl -s -b "$jar" "$base/status" | jq -c .resources)
echo "status resources: $res"
[ "$(echo "$res" | jq '.sessions_started == .sessions_stopped and .sessions_started >= '"$CYCLES")" = true ] \
  || note_fail "session counters do not agree: $res"
[ "$(echo "$res" | jq '.open_fds - '"$end_fds"' | fabs <= 2')" = true ] || note_fail "status fds disagree with /proc: $res vs $end_fds"
if [ "$fail" = 0 ]; then echo "ALL OK"; else echo "$fail FAILURE(S); server log: $log"; fi
exit "$fail"
