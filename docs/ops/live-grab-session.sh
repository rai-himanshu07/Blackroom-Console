#!/usr/bin/env bash
# Drives one simulated gateway session over loopback from a second device (SSH), because the
# laptop's own keyboard and mouse are dead while the physical-input grab is held.
# Procedure: docs/ops/live-grab-runbook.md. Run with `bash` in ONE interactive SSH window.
# Needs curl and jq.
#
#   case revoke|silence|freeze|chord   One guided test case; prompts appear on this screen.
#   start | heartbeat [N] | watch | revoke | status
#                                      Single steps, mainly for recovery (`revoke` always works).
set -u

BASE=http://127.0.0.1:8787
DEMO_CODE=${BLACKROOM_DEMO_CODE:-SIMULATE}
DIR=${BLACKROOM_LIVE_DIR:-${XDG_RUNTIME_DIR:-/tmp}/blackroom-live}
BEAT_SECONDS=10
EXPECTED_RELEASE_SECONDS=25
CAP=120 # seconds a case waits for an answer before it ends the session itself

for tool in curl jq; do
    command -v "$tool" >/dev/null || { echo "missing tool: $tool" >&2; exit 2; }
done
umask 077
mkdir -p "$DIR" || exit 2
JAR=$DIR/cookies.txt

CODE=000
BODY=
LAST_BEAT=0
ANSWER=
FROZEN_PID=
ms() { local t=${EPOCHREALTIME/./}; echo $((t / 1000)); }
say() { echo "$(date +%T) $*"; }
secs() { printf '%d.%d s' $(($1 / 1000)) $(($1 % 1000 / 100)); }
drain() { while read -r -t 0; do read -r _; done; }

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

# report LABEL: one short line for the last reply; fails unless the gateway answered 200.
report() {
    local text
    if [ "$CODE" = 200 ]; then
        text=$(jq -r '"state=\(.state) lease=\((.lease_remaining_ms // 0) / 1000 | floor)s"' <<<"$BODY" 2>/dev/null) || text=$BODY
        say "$1 http=200 $text"
        return 0
    fi
    text=$(jq -r '"error=\(.code // "?")"' <<<"$BODY" 2>/dev/null) || text="error=$BODY"
    say "$1 http=$CODE $text"
    return 1
}

beat() {
    api /api/simulation/renew '{}'
    if [ "$CODE" != 200 ]; then
        report renew
        return 1
    fi
    LAST_BEAT=$(ms)
    return 0
}

# watch_from EPOCH: print state changes until the session leaves REMOTE_ACTIVE.
watch_from() {
    local ref=$1 seen=0 prev="" now key shown=0 limit=$((SECONDS + 120))
    while [ "$SECONDS" -lt "$limit" ]; do
        api /api/simulation
        now=$(date +%s)
        key="http=$CODE state=$(state_of)"
        if [ "$key" != "$prev" ] || [ $((now - shown)) -ge 10 ]; then
            say "+$((now - ref))s $key"
            prev=$key
            shown=$now
        fi
        if [ "$CODE" = 200 ] && [ "$(state_of)" = REMOTE_ACTIVE ]; then
            seen=1
        elif [ "$seen" = 1 ] && [ "$CODE" = 200 ]; then
            say "session ended +$((now - ref))s after the reference time"
            say "(gateway state is not the grab: check the laptop pointer)"
            return 0
        fi
        sleep 1
    done
    say "still not ended after 120 s" >&2
    return 1
}

cmd_start() {
    say "Starting. Hands OFF the laptop keyboard, mouse, touchpad."
    SECONDS=0
    api /api/simulation/start "{\"demo_code\":\"$DEMO_CODE\"}"
    report start
    local status=$?
    say "start took ${SECONDS}s"
    return $status
}

cmd_revoke() {
    api /api/simulation/revoke '{}'
    report revoke
}

cmd_heartbeat() {
    local beats=${1:-0} sent=0
    case $beats in '' | *[!0-9]*) echo "heartbeat count must be a number" >&2; return 2 ;; esac
    while :; do
        beat || { say "heartbeat stopped (renew refused)"; return 1; }
        report renew
        sent=$((sent + 1))
        if [ "$beats" -gt 0 ] && [ "$sent" -ge "$beats" ]; then
            break
        fi
        sleep "$BEAT_SECONDS"
    done
    say "LAST RENEW. Heartbeat stopped on purpose."
    watch_from "$((LAST_BEAT / 1000))"
}

# ---- guided cases ------------------------------------------------------------------------

# wait_line KEEPALIVE LIMIT: wait for Enter on this keyboard, renewing every 10 s if KEEPALIVE=1.
# 0 = a line arrived (in ANSWER), 1 = timeout, 2 = a renew was refused, 3 = input closed (SSH gone).
wait_line() {
    local keepalive=$1 limit=$2 waited=0 step rc
    ANSWER=
    while [ "$waited" -lt "$limit" ]; do
        if [ "$keepalive" = 1 ]; then
            beat || return 2
        fi
        step=$BEAT_SECONDS
        [ $((limit - waited)) -lt "$step" ] && step=$((limit - waited))
        read -r -t "$step" ANSWER
        rc=$?
        [ "$rc" -eq 0 ] && return 0
        [ "$rc" -le 128 ] && return 3
        waited=$((waited + step))
    done
    return 1
}

engage() {
    api /api/simulation
    if [ "$(state_of)" != LOCAL_LOCKED ]; then
        say "gateway state is '$(state_of)', not LOCAL_LOCKED (a session may still be active): run bk revoke first"
        return 1
    fi
    cmd_start || { say "REFUSED: nothing is grabbed. Tell the agent the time."; return 1; }
    say "GRAB SHOULD BE ON."
}

# confirm_frozen: the operator touches the laptop touchpad; y = frozen, anything else ends the case.
confirm_frozen() {
    drain
    echo
    say "==> LAPTOP: touch the touchpad now."
    echo "    pointer FROZEN -> type y, Enter"
    echo "    pointer MOVES  -> type n, Enter"
    wait_line 1 "$CAP"
    local rc=$?
    case $rc:$ANSWER in 0:y* | 0:Y*) say "recorded: pointer frozen"; return 0 ;; esac
    say "recorded: NOT frozen (answer '${ANSWER:-none}', code $rc); ending the session"
    return 1
}

# pointer_back REFERENCE_MS: the operator presses Enter the moment the pointer moves again.
pointer_back() {
    drain
    echo
    say "==> LAPTOP: keep touching the touchpad. The MOMENT the pointer moves, press Enter here."
    if wait_line 0 "$CAP"; then
        say "pointer back $(secs $(($(ms) - $1))) after the reference time"
    else
        say "no answer. If the pointer is still frozen: bk revoke, then pkill -KILL -x remote-emergenc"
        return 1
    fi
}

cleanup() {
    [ -n "$FROZEN_PID" ] && kill -CONT "$FROZEN_PID" 2>/dev/null
    cmd_revoke >/dev/null 2>&1
}

case_run() {
    [ -t 0 ] || { echo "run this in an interactive SSH window" >&2; return 2; }
    trap 'cleanup; exit 130' INT TERM HUP
    local started hostd gateway
    case $1 in
        revoke)
            engage || return 1
            confirm_frozen || { cmd_revoke; return 1; }
            cmd_revoke
            pointer_back "$(ms)"
            ;;
        silence)
            engage || return 1
            confirm_frozen || { cmd_revoke; return 1; }
            say "HEARTBEAT STOPPED (last renew $(date -d "@$((LAST_BEAT / 1000))" +%T))."
            say "Do not touch anything. Expect the pointer back at about" \
                "$(date -d "@$((LAST_BEAT / 1000 + EXPECTED_RELEASE_SECONDS))" +%T) (+23..+27 s)."
            pointer_back "$LAST_BEAT"
            ;;
        freeze)
            hostd=$(pgrep -x remote-hostd)
            gateway=$(pgrep -x remote-gateway)
            if [ "$(wc -w <<<"$hostd")" != 1 ] || [ "$(ps -o ppid= -p "$hostd" 2>/dev/null | tr -d ' ')" != "$gateway" ]; then
                echo "need exactly one remote-hostd that belongs to the gateway" >&2
                return 1
            fi
            engage || return 1
            confirm_frozen || { cmd_revoke; return 1; }
            beat
            FROZEN_PID=$hostd
            kill -STOP "$FROZEN_PID"
            started=$(ms)
            say "HOSTD FROZEN (pid $FROZEN_PID). Do not touch anything."
            say "Expect the pointer back 8..12 s from now (daemon lease 10 s)."
            say "Still frozen after 20 s: press Ctrl-C here (resumes hostd and revokes)."
            pointer_back "$started"
            kill -CONT "$FROZEN_PID"
            FROZEN_PID=
            say "hostd resumed"
            sleep 2
            api /api/simulation
            report status
            ;;
        chord)
            engage || return 1
            confirm_frozen || { cmd_revoke; return 1; }
            echo
            say "==> LAPTOP: on the built-in KEYBOARD hold Left Ctrl + Left Shift + Left Alt + Esc"
            echo "    for 2 s. Keep holding until the pointer moves, let go, then press Enter here."
            wait_line 1 "$CAP"
            say "chord step finished (code $?)"
            sleep 2
            api /api/simulation
            report status
            ;;
        *)
            echo "case needs one of: revoke silence freeze chord" >&2
            return 2
            ;;
    esac
}

case ${1:-} in
    case) case_run "${2:-}" ;;
    start) cmd_start ;;
    heartbeat) cmd_heartbeat "${2:-0}" ;;
    watch) watch_from "$(date +%s)" ;;
    revoke) cmd_revoke ;;
    status)
        api /api/simulation
        report status
        ;;
    *)
        sed -n '2,9p' "$0" | sed 's/^# \{0,1\}//' >&2
        exit 2
        ;;
esac
