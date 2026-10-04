#!/usr/bin/env bash
# The internet-access settings against the built binaries, in a throwaway HOME: nothing on the real desktop, in the real
# settings or on the network is touched.
#   cargo build -p blackroom-console -p blackroom-cli && docs/ops/internet-test.sh
set -u
cd "$(dirname "$0")/../.." || exit 2
C=$PWD/target/debug/blackroom-console
B=$PWD/target/debug/blackroom
[ -x "$C" ] && [ -x "$B" ] || { echo "build first: cargo build -p blackroom-console -p blackroom-cli"; exit 2; }
command -v jq > /dev/null && command -v openssl > /dev/null || { echo "needs jq and openssl"; exit 2; }
home=$(mktemp -d /tmp/br-inet.XXXXXX)
trap 'rm -rf "$home"' EXIT
export HOME=$home XDG_RUNTIME_DIR=$home/run
mkdir -p -m 700 "$XDG_RUNTIME_DIR/blackroom-hostd" "$home/.config/blackroom/tls"
# hostd's sockets only have to exist for the check.
touch "$XDG_RUNTIME_DIR/blackroom-hostd/auth.sock" "$XDG_RUNTIME_DIR/blackroom-hostd/admin.sock"
fails=0
check() { if [ "$2" = "$3" ]; then echo "ok   $1 = $2"; else echo "FAIL $1: got '$2', want '$3'"; fails=$((fails + 1)); fi; }
flags=(--hostd-dir "$XDG_RUNTIME_DIR/blackroom-hostd" --listen 127.0.0.1:8080 --tls-listen 0.0.0.0:8443)
apply() { echo "$1" | "$C" --apply-internet "${flags[@]}" "${@:2}" | tail -1; }
report() { "$C" --check-internet "${flags[@]}" | tail -1; }
profile=$home/.local/share/blackroom-console

echo "== the default is the home network"
check "access" "$(report | jq -r .report.access)" home
check "no problems" "$(report | jq -r '.report.problems | length')" 0

echo "== a name with a certificate that no authority issued is refused in public mode"
tls=$home/.config/blackroom/tls
openssl req -x509 -newkey ec -pkeyopt ec_paramgen_curve:prime256v1 -nodes -days 30 -subj "/CN=home.example.org" \
  -addext "subjectAltName=DNS:home.example.org" -keyout "$tls/key.pem" -out "$tls/cert.pem" 2> /dev/null
chmod 600 "$tls/key.pem"
patch='{"public":true,"public_name":"home.example.org","public_cert":"ca","tls_cert":"'$tls'/cert.pem","tls_key":"'$tls'/key.pem","login":"hostd"}'
out=$(apply "$patch" --dry-run)
check "the dry run names the self-signed problem" "$(echo "$out" | jq -r '.report.problems | map(select(test("self-signed"))) | length')" 1
check "a dry run saves nothing" "$([ -e "$profile/host.json" ] && echo saved || echo none)" none

echo "== a bare IP with the console's own certificate, by explicit choice"
patch='{"public":true,"public_name":"203.0.113.7","public_cert":"self_signed","tls_cert":null,"tls_key":null,"ice_ports":"50000-50100","login":"hostd"}'
out=$(apply "$patch")
check "saved without problems" "$(echo "$out" | jq -r '.report.problems | length')" 0
check "the address clients type" "$(echo "$out" | jq -r .report.url)" "https://203.0.113.7:8443/"
check "host.json is private" "$(stat -c %a "$profile/host.json")" 600
check "the router table names TCP 8443" "$(echo "$out" | jq -r '.report.forwards | map(select(startswith("TCP 8443"))) | length')" 1
check "the self-signed warning is shown" "$(echo "$out" | jq -r '.report.warnings | map(select(test("fingerprint"))) | length')" 1

echo "== rejected input changes nothing"
before=$(cat "$profile/host.json")
check "a URL is not a name" "$(apply '{"public_name":"https://evil/"}' | jq -r .ok)" false
check "an unknown key is refused" "$(apply '{"nonsense":1}' | jq -r .ok)" false
check "host.json is unchanged" "$([ "$before" = "$(cat "$profile/host.json")" ] && echo same || echo changed)" same

echo "== the blackroom command reads the same answer"
"$B" internet --check --console-bin "$C" --runtime-dir "$XDG_RUNTIME_DIR/blackroom-hostd" > "$home/cli.txt" 2>&1
check "the check passes" "$?" 0
check "it names the address" "$(grep -c 'https://203.0.113.7:8443/' "$home/cli.txt")" 1
check "it says the checks are local" "$(grep -c 'local checks only' "$home/cli.txt")" 1

echo "== a damaged host.json is a problem and is never overwritten"
echo '{ nope' > "$profile/host.json"
check "the check fails" "$("$C" --check-internet "${flags[@]}" > /dev/null; echo $?)" 1
check "apply refuses" "$(apply '{"public":false}' | jq -r .ok)" false
check "the file is untouched" "$(cat "$profile/host.json")" "{ nope"

echo "== the real start-up: a damaged file keeps the listeners on this laptop (no session is started)"
serve() { "$C" --headless --no-control --state-dir "$home/state" --cert-dir "$home/cert" --profile-dir "$profile" "$@" > "$home/serve.log" 2>&1 & pid=$!; }
echo '{ nope' > "$profile/host.json"
serve --listen 0.0.0.0:18180 --tls-listen 0.0.0.0:18443
for _ in $(seq 1 50); do grep -q "Open one of these" "$home/serve.log" && break; sleep 0.2; done
listeners=$(ss -ltnH 2> /dev/null)
check "plain http is on 127.0.0.1" "$(echo "$listeners" | grep -c '127.0.0.1:18180')" 1
check "https is on 127.0.0.1" "$(echo "$listeners" | grep -c '127.0.0.1:18443')" 1
check "nothing listens on all addresses" "$(echo "$listeners" | grep -cE '(0.0.0.0|\*):1(8180|8443)')" 0
kill "$pid" 2> /dev/null; wait "$pid" 2> /dev/null

echo "== the real start-up refuses internet mode with a gap"
rm -f "$profile/host.json"
serve --public --listen 127.0.0.1:18180 --tls-listen 0.0.0.0:18443
wait "$pid"; code=$?
check "it exits with an error" "$([ "$code" != 0 ] && echo yes || echo no)" yes
check "it says why" "$(grep -c 'refusing to start in --public mode' "$home/serve.log")" 1

[ "$fails" = 0 ] && echo "INTERNET OK" || { echo "INTERNET FAILED ($fails)"; exit 1; }
