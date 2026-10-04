#!/usr/bin/env bash
# Runs packaging/prerm against stand-in processes in a throwaway directory. It never signals a real console: the script
# gets only the stand-in's pid through BLACKROOM_PRERM_PIDFILE.
set -u
cd "$(dirname "$0")/../.."
tmp=$(mktemp -d); trap 'kill -KILL ${stub:-} 2>/dev/null; rm -rf "$tmp"' EXIT
fail=0
check() { if [ "$2" = ok ]; then echo "ok   $1"; else echo "FAIL $1"; fail=1; fi; }

# 1. no console: nothing to do, exit 0.
: > "$tmp/pids"
BLACKROOM_PRERM_PIDFILE="$tmp/pids" sh packaging/prerm remove 2>/dev/null; [ $? = 0 ] && r=ok || r=bad
check "no console running: allowed" "$r"

# 2. a console that stops on SIGTERM: signalled, waited for, allowed.
sh -c 'trap "exit 0" TERM; while :; do sleep 0.2; done' & stub=$!
echo "$stub" > "$tmp/pids"
BLACKROOM_PRERM_PIDFILE="$tmp/pids" BLACKROOM_PRERM_WAIT=10 sh packaging/prerm upgrade 1.0 2>/dev/null; rc=$?
wait "$stub" 2>/dev/null
kill -0 "$stub" 2>/dev/null && r=bad || r=ok
check "console that obeys SIGTERM: stopped first (exit $rc)" "$([ $rc = 0 ] && echo $r || echo bad)"

# 3. a console that ignores SIGTERM: the change is refused (exit 1) within the limit.
sh -c 'trap "" TERM; while :; do sleep 0.2; done' & stub=$!
echo "$stub" > "$tmp/pids"
BLACKROOM_PRERM_PIDFILE="$tmp/pids" BLACKROOM_PRERM_WAIT=3 sh packaging/prerm remove 2>"$tmp/err"; rc=$?
kill -0 "$stub" 2>/dev/null && alive=yes || alive=no
check "console that ignores SIGTERM: package change refused (exit $rc)" "$([ $rc = 1 ] && [ $alive = yes ] && grep -q 'refused' "$tmp/err" && echo ok || echo bad)"
kill -KILL "$stub" 2>/dev/null

# 4. other maintainer actions are never blocked.
BLACKROOM_PRERM_PIDFILE="$tmp/pids" sh packaging/prerm failed-upgrade 2>/dev/null; [ $? = 0 ] && r=ok || r=bad
check "failed-upgrade is not blocked" "$r"

[ "$fail" = 0 ] && echo "PACKAGE SCRIPTS OK" || { echo "PACKAGE SCRIPTS FAILED"; exit 1; }
