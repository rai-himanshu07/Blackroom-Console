#!/usr/bin/env bash
# The menu launcher (packaging/blackroom-app) against stand-in systemctl, gnome-extensions, xdg-open and notify-send, with a
# fake console that listens on a port; nothing real is started, enabled or opened:
#   docs/ops/launcher-test.sh
set -u
cd "$(dirname "$0")/../.." || exit 2
PORT=18097
tmp=$(mktemp -d /tmp/br-launcher.XXXXXX); bin="$tmp/bin"; mkdir "$bin"
trap 'kill $(cat "$tmp/listener.pid" 2>/dev/null) 2>/dev/null; rm -rf "$tmp"' EXIT
fail=0
check() { if [ "$2" = "$3" ]; then echo "ok   $1"; else echo "FAIL $1: got [$2], want [$3]"; fail=1; fi; }
for tool in xdg-open notify-send gnome-extensions; do
  printf '#!/bin/bash\necho "%s $*" >> "%s/calls"\nexit "${STUB_%s_FAIL:-0}"\n' "$tool" "$tmp" "$(echo "$tool" | tr 'a-z-' 'A-Z_')" > "$bin/$tool"
done
cat > "$bin/systemctl" <<STUB
#!/bin/bash
echo "systemctl \$*" >> "$tmp/calls"
case "\$2" in
  start)
    [ "\${STUB_START_FAIL:-0}" = 1 ] && { echo "Unit not found." >&2; exit 5; }
    ( sleep 0.6; python3 -c 'import socket,time; s=socket.socket(); s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1); s.bind(("127.0.0.1",$PORT)); s.listen(); time.sleep(60)' & echo \$! > "$tmp/listener.pid" ) > /dev/null 2>&1 & ;;
  stop)
    [ "\${STUB_BY_HAND:-0}" = 1 ] || kill \$(cat "$tmp/listener.pid" 2>/dev/null) 2>/dev/null ;;
esac
exit 0
STUB
chmod 755 "$bin"/*
run() { PATH="$bin:$PATH" BR_APP_HOST_PORT=$PORT "$@"; }
calls() { cat "$tmp/calls" 2>/dev/null; }

run packaging/blackroom-app > /dev/null 2>&1; rc=$?
check "the launcher succeeds" "$rc" 0
check "it turns the top-bar icon on" "$(calls | grep -c '^gnome-extensions enable blackroom-indicator@blackroom.local')" 1
check "it starts the console service" "$(calls | grep -c '^systemctl --user start blackroom-console.service')" 1
check "it opens the host settings page, not the client page" "$(calls | grep '^xdg-open')" "xdg-open http://localhost:$PORT/"
check "the icon is enabled before the page opens" "$(calls | grep -n 'gnome-extensions enable\|xdg-open' | cut -d: -f1 | tr '\n' ' ')" "1 3 "

: > "$tmp/calls"
run packaging/blackroom-app exit > /dev/null 2>&1; rc=$?
check "exit succeeds" "$rc" 0
check "exit stops the console" "$(calls | grep -c '^systemctl --user stop blackroom-console.service')" 1
check "exit removes the top-bar icon" "$(calls | grep -c '^gnome-extensions disable blackroom-indicator@blackroom.local')" 1
check "exit says so" "$(calls | grep -c '^notify-send.*closed')" 1

: > "$tmp/calls"
STUB_BY_HAND=1 run packaging/blackroom-app > /dev/null 2>&1
: > "$tmp/calls"
STUB_BY_HAND=1 run packaging/blackroom-app exit > /dev/null 2>&1; rc=$?
check "a console started by hand is left running and keeps its icon" "$rc $(calls | grep -c '^gnome-extensions disable')" "1 0"
kill $(cat "$tmp/listener.pid" 2>/dev/null) 2>/dev/null; sleep 0.3

: > "$tmp/calls"
STUB_START_FAIL=1 run packaging/blackroom-app > /dev/null 2>&1; rc=$?
check "a console that cannot start is reported and nothing is opened" "$rc $(calls | grep -c '^notify-send.*Could not start') $(calls | grep -c '^xdg-open')" "1 1 0"

: > "$tmp/calls"
STUB_GNOME_EXTENSIONS_FAIL=1 run packaging/blackroom-app > /dev/null 2>&1; rc=$?
check "a missing top-bar extension is reported but the app still opens" "$rc $(calls | grep -c '^notify-send.*top-bar icon') $(calls | grep -c '^xdg-open')" "0 1 1"
kill $(cat "$tmp/listener.pid" 2>/dev/null) 2>/dev/null

check "an unknown option is refused" "$(run packaging/blackroom-app nonsense 2> /dev/null; echo $?)" 2
desktop-file-validate packaging/blackroom-console.desktop && echo "ok   the menu entry is a valid desktop file"
grep -q '^Exec=/usr/bin/blackroom-app$' packaging/blackroom-console.desktop && echo "ok   it runs /usr/bin/blackroom-app" || fail=1
[ "$fail" = 0 ] && echo "LAUNCHER OK" || echo "LAUNCHER FAILED"
exit "$fail"
