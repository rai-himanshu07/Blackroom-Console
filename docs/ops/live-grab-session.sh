#!/usr/bin/env bash
# Drives one simulated gateway session over loopback from a second device (SSH), because the
# laptop's own keyboard and mouse are dead while the physical-input grab is held.
# Procedure: docs/ops/live-grab-runbook.md. Run with `bash`; needs curl and jq on this machine.
#
#   start            Start a session (the demo code is public). Blocks up to ~30 s while hostd
#                    waits for every key to be up, then the grab is held.
#   heartbeat [N]    Renew every 10 s like the web page does. With N, stop after N renews, then
#                    watch until the session ends and report how long that took.
#   watch            Poll the state once a second until it leaves REMOTE_ACTIVE (120 s limit).
#   revoke           End the session (no cookie needed).
#   status           One state line.
set -u

BASE=http://127.0.0.1:8787
DEMO_CODE=${BLACKROOM_DEMO_CODE:-SIMULATE}
DIR=${BLACKROOM_LIVE_DIR:-${XDG_RUNTIME_DIR:-/tmp}/blackroom-live}
BEAT_SECONDS=10
EXPECTED_RELEASE_SECONDS=25

for tool in curl jq; do
    command -v "$tool" >/dev/null || { echo "missing tool: $tool" >&2; exit 2; }
done
umask 077
mkdir -p "$DIR" || exit 2
JAR=$DIR/cookies.txt

CODE=000
BODY=
# api PATH [JSON]: POST with a body, GET without. Sets CODE and BODY; keeps the grant cookie.
api() {
    local path=$1 out
    if [ $# -gt 1 ]; then
        out=$(curl -sS -m 45 -w '\n%{http_code}' -b "$JAR" -c "$JAR" \
            -H 'Content-Type: application/json' -d "$2" "$BASE$path" 2>&1)
    else
        out=$(curl -sS -m 5 -w '\n%{http_code}' -b "$JAR" -c "$JAR" "$BASE$path" 2>&1)
    fi
    CODE=${out##*$'\n'}
    BODY=${out%$'\n'*}
}

state_of() { jq -r '.state // empty' <<<"$BODY" 2>/dev/null; }

# report LABEL: one line for the last reply; fails unless the gateway answered 200.
report() {
    local text
    if [ "$CODE" = 200 ]; then
        text=$(jq -r '"state=\(.state) epoch=\(.epoch) lease_ms=\(.lease_remaining_ms // "-") session_ms=\(.session_remaining_ms // "-")"' <<<"$BODY" 2>/dev/null) || text=$BODY
        echo "$(date +%T) $1 http=200 $text"
        return 0
    fi
    text=$(jq -r '"error=\(.code // "?")"' <<<"$BODY" 2>/dev/null) || text="error=$BODY"
    echo "$(date +%T) $1 http=$CODE $text"
    return 1
}

# watch_from EPOCH: print state changes until the session leaves REMOTE_ACTIVE.
watch_from() {
    local ref=$1 seen=0 prev="" now key shown=0 limit=$((SECONDS + 120))
    while [ "$SECONDS" -lt "$limit" ]; do
        api /api/simulation
        now=$(date +%s)
        key="http=$CODE state=$(state_of)"
        if [ "$key" != "$prev" ] || [ $((now - shown)) -ge 10 ]; then
            echo "$(date +%T) +$((now - ref))s $key"
            prev=$key
            shown=$now
        fi
        if [ "$CODE" = 200 ] && [ "$(state_of)" = REMOTE_ACTIVE ]; then
            seen=1
        elif [ "$seen" = 1 ] && [ "$CODE" = 200 ]; then
            echo "$(date +%T) session ended +$((now - ref))s after the reference time"
            echo "note: the gateway state is not the grab; confirm local input physically on the laptop."
            return 0
        fi
        sleep 1
    done
    echo "$(date +%T) still not ended after 120 s" >&2
    return 1
}

cmd_start() {
    echo "$(date +%T) Starting. Hands OFF the laptop keyboard, mouse and touchpad until this returns."
    SECONDS=0
    api /api/simulation/start "{\"demo_code\":\"$DEMO_CODE\"}"
    report start
    local status=$?
    echo "start took ${SECONDS}s"
    return $status
}

cmd_heartbeat() {
    local beats=${1:-0} sent=0 last
    case $beats in '' | *[!0-9]*) echo "heartbeat count must be a number" >&2; return 2 ;; esac
    while :; do
        api /api/simulation/renew '{}'
        if ! report renew; then
            echo "heartbeat stopped: renew refused (expected after revoke, a lost grab or an emergency)"
            return 1
        fi
        last=$(date +%s)
        sent=$((sent + 1))
        if [ "$beats" -gt 0 ] && [ "$sent" -ge "$beats" ]; then
            break
        fi
        sleep "$BEAT_SECONDS"
    done
    echo "$(date +%T) LAST RENEW. Heartbeat stopped on purpose; local input should return about" \
        "$(date -d "@$((last + EXPECTED_RELEASE_SECONDS))" +%T) (+${EXPECTED_RELEASE_SECONDS}s, allow +23..+27)."
    watch_from "$last"
}

case ${1:-} in
    start) cmd_start ;;
    heartbeat) cmd_heartbeat "${2:-0}" ;;
    watch) watch_from "$(date +%s)" ;;
    revoke)
        api /api/simulation/revoke '{}'
        report revoke
        ;;
    status)
        api /api/simulation
        report status
        ;;
    *)
        sed -n '2,14p' "$0" | sed 's/^# \{0,1\}//' >&2
        exit 2
        ;;
esac
