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
# A stand-in for the blackroom command: records its arguments and prints fake secrets (the real credentials are never touched).
cat > "$state/blackroom-stub" <<STUB
#!/bin/bash
echo "\$@" >> "$state/cli-calls"
case "\$*" in
  *" status") echo "stub status: remote access enabled" ;;
  *devices*) printf 'dev-1\tChrome on tablet\tcreated=1\tlast_used=1791086225\ttrusted\ndev-0\tOld phone\tcreated=1\tlast_used=0\trevoked\n' ;;
  *rotate-key*) echo "remote access key: ABCD-EFGH-IJKL" ;;
  setup*) echo "authenticator secret: STUB-SECRET" ;;
esac
STUB
chmod 700 "$state/blackroom-stub"
# A stand-in for gnome-extensions that keeps its state in a file (the real extensions are never touched).
cat > "$state/ext-stub" <<STUB
#!/bin/bash
echo "\$@" >> "$state/ext-calls"
U=blackroom-locked-remote@blackroom.local
case "\$1 \$2" in
  "list ") echo \$U ;;
  "list --enabled"|"list --active") [ -f "$state/ext-on" ] && echo \$U ;;
  enable*) touch "$state/ext-on" ;;
  disable*) rm -f "$state/ext-on" ;;
esac
exit 0
STUB
chmod 700 "$state/ext-stub"
mkdir -p "$state/run"
printf '#!/bin/bash\necho "$@" >> "%s/systemctl-calls"\nexit 0\n' "$state" > "$state/systemctl-stub"
chmod 700 "$state/systemctl-stub"
BLACKROOM_HOSTNAME=${BR_SHOT_DIR:+my-laptop} XDG_RUNTIME_DIR="$state/run" "$BIN" --headless --no-control --gnome-extensions "$state/ext-stub" --systemctl "$state/systemctl-stub" --listen 127.0.0.1:18095 --host-listen 127.0.0.1:18096 --pam-helper "$state/pam-stub" --blackroom-cli "$state/blackroom-stub" --hostd-state-dir "$state/hostd" --state-dir "$state" > "$log" 2>&1 &
srv=$!
trap 'kill "$srv" 2>/dev/null' EXIT
for _ in $(seq 1 50); do grep -q "Host settings" "$log" && break; sleep 0.2; done
grep -q "Host settings" "$log" || { echo "FAIL: the host page did not start"; cat "$log"; exit 1; }
node docs/ops/headless-hostpage-test.mjs "http://localhost:18096/" 9334 "$state"
rc=$?
echo "server log: $log"
exit "$rc"
