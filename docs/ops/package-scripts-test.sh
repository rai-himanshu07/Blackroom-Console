#!/usr/bin/env bash
# Runs packaging/prerm against stand-in processes in a throwaway directory. It never signals a real console: the script
# gets only the stand-in's pid through BLACKROOM_PRERM_PIDFILE.
set -u
cd "$(dirname "$0")/../.."
tmp=$(mktemp -d); trap 'kill -KILL ${stub:-} 2>/dev/null; rm -rf "$tmp"' EXIT
fail=0
check() { if [ "$2" = ok ]; then echo "ok   $1"; else echo "FAIL $1"; fail=1; fi; }

# The real restart-access helper is never run here: a stand-in that succeeds, and one that fails (case 4b).
printf '#!/bin/sh\nexit 0\n' > "$tmp/helper-ok"; printf '#!/bin/sh\nexit 4\n' > "$tmp/helper-bad"; chmod +x "$tmp/helper-ok" "$tmp/helper-bad"
export BLACKROOM_PRERM_HELPER="$tmp/helper-ok"

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

# 4a. A pending restore timer holds removal back (and an upgrade is not held).
printf 'Wed 2026-10-07 01:00:00 IST 30s left  blackroom-console-wd-1.timer\n' > "$tmp/timers"
: > "$tmp/pids"
BLACKROOM_PRERM_TIMERFILE="$tmp/timers" BLACKROOM_PRERM_TIMER_WAIT=2 BLACKROOM_PRERM_PIDFILE="$tmp/pids" sh packaging/prerm remove 2> "$tmp/err4a"; rc=$?
check "removal is refused while a restore timer is pending (exit $rc)" "$([ $rc = 1 ] && grep -q 'restore timer' "$tmp/err4a" && echo ok || echo bad)"
BLACKROOM_PRERM_TIMERFILE="$tmp/timers" BLACKROOM_PRERM_PIDFILE="$tmp/pids" sh packaging/prerm upgrade 2>/dev/null; [ $? = 0 ] && r=ok || r=bad
check "an upgrade is not held by the timer" "$r"
: > "$tmp/timers"
BLACKROOM_PRERM_TIMERFILE="$tmp/timers" BLACKROOM_PRERM_PIDFILE="$tmp/pids" sh packaging/prerm remove 2>/dev/null; [ $? = 0 ] && r=ok || r=bad
check "removal goes ahead once no timer is pending" "$r"

# 4b. If the automatic login it set up cannot be removed, removal stops (upgrade is not affected).
: > "$tmp/pids"
BLACKROOM_PRERM_HELPER="$tmp/helper-bad" BLACKROOM_PRERM_PIDFILE="$tmp/pids" sh packaging/prerm remove 2> "$tmp/err4"; rc=$?
check "removal stops when automatic login cannot be removed (exit $rc)" "$([ $rc = 1 ] && grep -q 'automatic login' "$tmp/err4" && echo ok || echo bad)"
BLACKROOM_PRERM_HELPER="$tmp/helper-bad" BLACKROOM_PRERM_PIDFILE="$tmp/pids" sh packaging/prerm upgrade 2>/dev/null; [ $? = 0 ] && r=ok || r=bad
check "an upgrade does not touch automatic login" "$r"

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
# A block whose lines were separated by hand is still removed whole, and the result is checked.
helper on > /dev/null 2>&1
python3 - "$ra/etc/gdm3/custom.conf" <<'PY'
import sys; p=sys.argv[1]; s=open(p).read().replace("AutomaticLoginEnable=true","\n# note\nAutomaticLoginEnable=true"); open(p,"w").write(s)
PY
helper off > /dev/null 2>&1; rc=$?
check "off removes a block whose lines moved apart (exit $rc)" "$([ $rc = 0 ] && ! grep -q '^AutomaticLogin' "$ra/etc/gdm3/custom.conf" && ! grep -q 'blackroom-console: automatic' "$ra/etc/gdm3/custom.conf" && echo ok || echo bad)"
printf '[daemon]\nAutomaticLoginEnable=true\nAutomaticLogin=other\n' > "$ra/etc/gdm3/custom.conf"; cp "$ra/etc/gdm3/custom.conf" "$tmp/gdm-own"
helper on > /dev/null 2> "$tmp/err3"; rc=$?
check "an automatic login the owner set up is left alone (exit $rc)" "$([ $rc = 3 ] && cmp -s "$ra/etc/gdm3/custom.conf" "$tmp/gdm-own" && echo ok || echo bad)"
BLACKROOM_TEST_ROOT="$ra" BLACKROOM_TEST_USER='bad;name' sh packaging/blackroom-restart-access on > /dev/null 2>&1; rc=$?
check "a user name with odd characters is refused (exit $rc)" "$([ $rc = 2 ] && echo ok || echo bad)"

# 7. Lock at login: locks the user's graphical session after an automatic login only, with the marker on, and never needs XDG_SESSION_ID.
lk="$tmp/lk"; mkdir -p "$lk/bin" "$lk/data/blackroom-console"
cat > "$lk/bin/loginctl" <<'STUB'
#!/bin/sh
echo "$@" >> "$LK_CALLS"
case "$1" in
  show-user) echo 7 ;;
  show-session) echo "$LK_SERVICE" ;;
esac
STUB
chmod +x "$lk/bin/loginctl"
lock_run() { : > "$lk/calls"; env -u XDG_SESSION_ID PATH="$lk/bin:$PATH" XDG_DATA_HOME="$lk/data" LK_CALLS="$lk/calls" LK_SERVICE="$1" BLACKROOM_LOCK_DELAY=0 sh packaging/blackroom-lock-at-login; }
lock_run gdm-autologin
check "no marker: nothing is locked" "$(grep -q lock-session "$lk/calls" && echo bad || echo ok)"
: > "$lk/data/blackroom-console/lock-at-autologin"
lock_run gdm-password
check "a login with a typed password is not locked" "$(grep -q lock-session "$lk/calls" && echo bad || echo ok)"
lock_run gdm-autologin
check "an automatic login locks the user's graphical session (7)" "$(grep -qx 'lock-session 7' "$lk/calls" && echo ok || echo bad)"

[ "$fail" = 0 ] && echo "PACKAGE SCRIPTS OK" || { echo "PACKAGE SCRIPTS FAILED"; exit 1; }
