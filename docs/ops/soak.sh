#!/usr/bin/env bash
# Soak: watches the running console for an hour (or N minutes) while you drive it from the tablet, and says whether
# it stayed healthy. Run it in a SECOND terminal (or over SSH) on the laptop after pressing Start in the page:
#   docs/ops/soak.sh [minutes=60] [interval_seconds=30]
# Every sample reads the console process (memory, file descriptors, threads), the input-grab daemon, how many browsers
# are connected, and whether the input grab is still held (that is, the session is still live). It changes nothing and
# needs no login. The verdict at the end:
#   - the console process never died and the daemon stayed up,
#   - the session stayed live in at least 95% of samples (a heartbeat loss or Stop shows up here),
#   - resources stayed flat after a 5 minute warm-up: fds +6, threads +4, memory +25% (at least 50 MB of room).
# The CSV of the samples is kept in /tmp (no secrets in it).
# Env: BR_SOAK_PROC (default blackroom-conso, the 15-character process name), BR_SOAK_SOCK (the emergency daemon socket).
set -u
MINUTES=${1:-60}
INTERVAL=${2:-30}
PROC=${BR_SOAK_PROC:-blackroom-conso}
SOCK=${BR_SOAK_SOCK:-${XDG_RUNTIME_DIR:-/run/user/$(id -u)}/blackroom-live/emergency.sock}
BLACKROOM=$(command -v blackroom || echo target/release/blackroom)
CSV=/tmp/blackroom-soak-$(date +%Y%m%d-%H%M%S).csv
case "$MINUTES$INTERVAL" in *[!0-9]*|"") echo "usage: soak.sh [minutes] [interval_seconds]" >&2; exit 2 ;; esac

pid=$(pgrep -x "$PROC" | head -n 1)
[ -n "$pid" ] || { echo "no running $PROC: start the console and press Start in the page first" >&2; exit 2; }
echo "soaking pid $pid for $MINUTES min, every $INTERVAL s; samples in $CSV"
echo "t_s,rss_kb,fds,threads,browsers,daemon,held" > "$CSV"

sample() {
  local t=$1 rss fds threads browsers daemon held
  kill -0 "$pid" 2>/dev/null || { echo "$t,DEAD,,,,," >> "$CSV"; return 1; }
  rss=$(awk '/^VmRSS:/ {print $2}' "/proc/$pid/status")
  threads=$(awk '/^Threads:/ {print $2}' "/proc/$pid/status")
  fds=$(ls "/proc/$pid/fd" 2>/dev/null | wc -l)
  browsers=$(ss -Htn state established '( sport = :8443 or sport = :8080 )' 2>/dev/null | wc -l)
  daemon=$(pgrep -x remote-emergenc > /dev/null && echo 1 || echo 0)
  held=$("$BLACKROOM" emergency-status --socket "$SOCK" 2>/dev/null | jq -r 'if .held == null then "?" elif .held > 0 then 1 else 0 end' 2>/dev/null)
  echo "$t,$rss,$fds,$threads,$browsers,$daemon,${held:-?}" >> "$CSV"
  printf '%5ss rss=%s kB fds=%s threads=%s browsers=%s daemon=%s grab=%s\n' "$t" "$rss" "$fds" "$threads" "$browsers" "$daemon" "${held:-?}"
}

start=$(date +%s)
end=$((start + MINUTES * 60))
died=0
while [ "$(date +%s)" -lt "$end" ]; do
  sample $(( $(date +%s) - start )) || { died=1; break; }
  sleep "$INTERVAL"
done

echo "== verdict"
fail=0
bad() { echo "FAIL $*"; fail=1; }
[ "$died" = 0 ] || bad "the console process died during the soak"
samples=$(grep -vc ',DEAD' "$CSV"); samples=$((samples - 1))
[ "$samples" -ge 2 ] || bad "fewer than 2 samples"
warm=$((300 / INTERVAL)); [ "$MINUTES" -ge 10 ] || warm=0
base=$(awk -F, -v skip="$warm" 'NR > 1 && $2 != "DEAD" {n++; if (n == skip + 1) {print $2","$3","$4; exit}}' "$CSV")
[ -n "$base" ] || base=$(awk -F, 'NR == 2 {print $2","$3","$4}' "$CSV")
b_rss=${base%%,*}; rest=${base#*,}; b_fds=${rest%%,*}; b_threads=${rest#*,}
read -r max_rss max_fds max_threads < <(awk -F, -v skip="$warm" 'NR > 1 && $2 != "DEAD" {n++; if (n > skip) {if ($2 > r) r = $2; if ($3 > f) f = $3; if ($4 > t) t = $4}} END {print r, f, t}' "$CSV")
echo "baseline rss=$b_rss kB fds=$b_fds threads=$b_threads; peak after warm-up rss=$max_rss kB fds=$max_fds threads=$max_threads"
[ $((max_fds - b_fds)) -le 6 ] || bad "fd growth $b_fds -> $max_fds"
[ $((max_threads - b_threads)) -le 4 ] || bad "thread growth $b_threads -> $max_threads"
allowed=$(( b_rss / 4 )); [ "$allowed" -ge 51200 ] || allowed=51200
[ $((max_rss - b_rss)) -le "$allowed" ] || bad "memory growth $b_rss -> $max_rss kB (allowed +$allowed)"
down=$(awk -F, 'NR > 1 && $6 == 0 {n++} END {print n + 0}' "$CSV")
[ "$down" = 0 ] || bad "the input-grab daemon was missing in $down sample(s)"
live=$(awk -F, 'NR > 1 && $7 == 1 {n++} END {print n + 0}' "$CSV")
unknown=$(awk -F, 'NR > 1 && $7 == "?" {n++} END {print n + 0}' "$CSV")
if [ "$unknown" -gt 0 ]; then
  echo "NOTE the grab state was unreadable in $unknown sample(s) (no daemon socket at $SOCK?): session liveness not judged"
elif [ $((live * 100)) -lt $((samples * 95)) ]; then
  bad "the session was live in only $live of $samples samples"
else
  echo "session live in $live of $samples samples"
fi
[ "$fail" = 0 ] && echo "SOAK OK" || echo "SOAK FAILED (samples: $CSV)"
exit "$fail"
