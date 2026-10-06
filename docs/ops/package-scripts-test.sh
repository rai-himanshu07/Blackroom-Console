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

# 5. Through pkexec the grant helper refuses to act for another user.
PKEXEC_UID=$(id -u) sh packaging/blackroom-grant-input grant --user someone-else 2> "$tmp/err2" > /dev/null; rc=$?
check "the grant helper refuses --user through pkexec (exit $rc)" "$([ $rc = 2 ] && grep -q 'not allowed through pkexec' "$tmp/err2" && echo ok || echo bad)"

# 6. The restart-access helper edits a throwaway copy of the GDM config only, and takes back exactly what it added.
ra="$tmp/ra"; mkdir -p "$ra/etc/gdm3"
printf '[daemon]\n#  AutomaticLoginEnable = true\n\n[security]\n' > "$ra/etc/gdm3/custom.conf"; cp "$ra/etc/gdm3/custom.conf" "$tmp/gdm-orig"
helper() { BLACKROOM_TEST_ROOT="$ra" BLACKROOM_TEST_USER=someone sh packaging/blackroom-restart-access "$@"; }
helper on > /dev/null 2>&1; helper on > /dev/null 2>&1   # twice: still one block
check "restart access on: one automatic-login block and a udev rule" "$([ "$(grep -c '^AutomaticLogin=someone$' "$ra/etc/gdm3/custom.conf")" = 1 ] && grep -q 'u:someone:rw' "$ra/etc/udev/rules.d/90-blackroom-input.rules" && helper status | grep -qx autologin=ours && echo ok || echo bad)"
helper off > /dev/null 2>&1
check "restart access off: the GDM file is back to the original and the rule is gone" "$(cmp -s "$ra/etc/gdm3/custom.conf" "$tmp/gdm-orig" && [ ! -e "$ra/etc/udev/rules.d/90-blackroom-input.rules" ] && echo ok || echo bad)"
printf '[daemon]\nAutomaticLoginEnable=true\nAutomaticLogin=other\n' > "$ra/etc/gdm3/custom.conf"; cp "$ra/etc/gdm3/custom.conf" "$tmp/gdm-own"
helper on > /dev/null 2> "$tmp/err3"; rc=$?
check "an automatic login the owner set up is left alone (exit $rc)" "$([ $rc = 3 ] && cmp -s "$ra/etc/gdm3/custom.conf" "$tmp/gdm-own" && echo ok || echo bad)"
BLACKROOM_TEST_ROOT="$ra" BLACKROOM_TEST_USER='bad;name' sh packaging/blackroom-restart-access on > /dev/null 2>&1; rc=$?
check "a user name with odd characters is refused (exit $rc)" "$([ $rc = 2 ] && echo ok || echo bad)"

[ "$fail" = 0 ] && echo "PACKAGE SCRIPTS OK" || { echo "PACKAGE SCRIPTS FAILED"; exit 1; }
