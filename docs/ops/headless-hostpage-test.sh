#!/usr/bin/env bash
# The laptop owner's settings page in headless Chrome against a console on a throwaway state directory (no Shell, no
# session, a stand-in password check; nothing on the real desktop or in the real settings is touched):
#   cargo build -p blackroom-console && docs/ops/headless-hostpage-test.sh
set -u
cd "$(dirname "$0")/../.." || exit 2
BIN=target/debug/blackroom-console
[ -x "$BIN" ] || { echo "build first: cargo build -p blackroom-console"; exit 2; }
state=$(mktemp -d /tmp/br-hostpage-state.XXXXXX)
log=$(mktemp /tmp/br-hostpage.XXXXXX)
printf '#!/bin/bash\nread -r account\nread -r password\n[ "$password" = "hostpass" ] && exit 0\nexit 1\n' > "$state/pam-stub"
chmod 700 "$state/pam-stub"
"$BIN" --headless --listen 127.0.0.1:18095 --host-listen 127.0.0.1:18096 --pam-helper "$state/pam-stub" --state-dir "$state" > "$log" 2>&1 &
srv=$!
trap 'kill "$srv" 2>/dev/null' EXIT
for _ in $(seq 1 50); do grep -q "Host settings" "$log" && break; sleep 0.2; done
grep -q "Host settings" "$log" || { echo "FAIL: the host page did not start"; cat "$log"; exit 1; }
node docs/ops/headless-hostpage-test.mjs "http://localhost:18096/" 9334 "$state"
rc=$?
echo "server log: $log"
exit "$rc"
