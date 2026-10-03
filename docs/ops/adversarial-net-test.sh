#!/usr/bin/env bash
# Network-level adversarial checks against a real console process (no session is ever started: no Shell, no display,
# no input device is touched). Needs curl, jq, python3 and a built target/debug/blackroom-console:
#   cargo build -p blackroom-console && docs/ops/adversarial-net-test.sh
set -u
cd "$(dirname "$0")/../.." || exit 2
BIN=${BR_BIN:-target/debug/blackroom-console}
[ -x "$BIN" ] || { echo "build first: cargo build -p blackroom-console"; exit 2; }
HTTP=18091; TLS=18092
state=$(mktemp -d /tmp/br-net-state.XXXXXX); log=$(mktemp /tmp/br-net.XXXXXX)
"$BIN" --headless --listen "127.0.0.1:$HTTP" --tls-listen "127.0.0.1:$TLS" --state-dir "$state" > "$log" 2>&1 &
srv=$!
trap 'kill "$srv" 2>/dev/null; rm -rf "$state"' EXIT
url=""
for _ in $(seq 1 50); do url=$(grep -o "http://127.0.0.1:$HTTP/?t=[0-9a-f]*" "$log" | head -n 1); [ -n "$url" ] && break; sleep 0.2; done
[ -n "$url" ] || { echo "FAIL: no URL printed"; cat "$log"; exit 1; }
token=${url##*t=}
fail=0
check() { if [ "$2" = "$3" ]; then echo "ok   $1"; else echo "FAIL $1: got '$2', want '$3'"; fail=1; fi; }
refused() { if ! "${@:2}" > /dev/null 2>&1; then echo "ok   $1"; else echo "FAIL $1: the request was accepted"; fail=1; fi; }
accepted() { if "${@:2}" > /dev/null 2>&1; then echo "ok   $1"; else echo "FAIL $1: the request failed"; fail=1; fi; }
https="https://127.0.0.1:$TLS"

echo "== process"
check "console runs as an ordinary user" "$(ps -o euid= -p "$srv" | tr -d ' ')" "$(id -u)"
check "console binary has no setuid or setgid bit" "$(stat -c %A "$BIN" | grep -c '[sS]')" 0
check "state directory is private" "$(stat -c %a "$state")" 700

echo "== TLS"
accepted "TLS 1.3 is served" curl -sk --tlsv1.3 -o /dev/null "$https/status"
accepted "TLS 1.2 is served" curl -sk --tlsv1.2 --tls-max 1.2 -o /dev/null "$https/status"
refused "TLS 1.1 is refused" curl -sk --tlsv1.0 --tls-max 1.1 -o /dev/null "$https/status"
refused "a 3DES-only client is refused" curl -sk --tlsv1.2 --tls-max 1.2 --ciphers 'DES-CBC3-SHA' -o /dev/null "$https/status"
refused "an anonymous or export-grade client is refused" curl -sk --tlsv1.2 --tls-max 1.2 --ciphers 'aNULL:eNULL:EXPORT' -o /dev/null "$https/status"
check "HTTP/2 is negotiated" "$(curl -sk --http2 -o /dev/null -w '%{http_version}' "$https/status")" 2
refused "plain http on the TLS port gets no page" curl -s --max-time 3 -f "http://127.0.0.1:$TLS/status"
if command -v openssl > /dev/null; then
  chain=$(echo | openssl s_client -connect "127.0.0.1:$TLS" -tls1_3 2>/dev/null | grep -c "BEGIN CERTIFICATE")
  [ "$chain" -ge 1 ] && echo "ok   a certificate is presented" || { echo "FAIL no certificate presented"; fail=1; }
fi

echo "== cookies and headers over https"
headers=$(curl -sk -D - -o /dev/null "$https/?t=$token")
echo "$headers" | grep -qi '^set-cookie:.*Secure' && echo "ok   the https cookie is Secure" || { echo "FAIL https cookie lacks Secure"; fail=1; }
echo "$headers" | grep -qi '^set-cookie:.*HttpOnly.*SameSite=Strict\|^set-cookie:.*SameSite=Strict.*HttpOnly' && echo "ok   HttpOnly and SameSite=Strict" || { echo "FAIL cookie flags"; fail=1; }
echo "$headers" | grep -qi '^strict-transport-security' && { echo "FAIL HSTS is set for a self-signed certificate"; fail=1; } || echo "ok   no HSTS without a real certificate"
headers=$(curl -s -D - -o /dev/null "http://127.0.0.1:$HTTP/?t=$token")
echo "$headers" | grep -qi '^set-cookie:.*Secure' && { echo "FAIL the plain http cookie is Secure (never sent back)"; fail=1; } || echo "ok   the plain http cookie is not Secure"
check "the wrong token sets no cookie" "$(curl -s -D - -o /dev/null "http://127.0.0.1:$HTTP/?t=wrong" | grep -ci '^set-cookie')" 0

echo "== CSP script hashes match the page scripts (computed independently)"
csp=$(curl -s -D - -o /dev/null "http://127.0.0.1:$HTTP/status" | grep -i '^content-security-policy' | tr -d '\r')
for page in page login login_full; do
  hash=$(python3 - "crates/blackroom-console/src/$page.html" <<'PY'
import base64, hashlib, re, sys
text = open(sys.argv[1], encoding="utf-8").read()
m = re.search(r"<script>(.*?)</script>", text, re.S)
print("sha256-" + base64.b64encode(hashlib.sha256(m.group(1).encode()).digest()).decode())
PY
)
  echo "$csp" | grep -q "'$hash'" && echo "ok   $page.html hash is in the policy" || { echo "FAIL $page.html hash missing from the policy"; fail=1; }
done
echo "$csp" | grep -q "unsafe-inline" && echo "$csp" | grep -o "script-src[^;]*" | grep -q "unsafe-inline" && { echo "FAIL script-src allows unsafe-inline"; fail=1; } || echo "ok   script-src has no unsafe-inline"

echo "== a stalled or hostile client does not stall others"
exec 3<> "/dev/tcp/127.0.0.1/$HTTP"; printf 'GET /stat' >&3
exec 4<> "/dev/tcp/127.0.0.1/$HTTP"; head -c 65536 /dev/zero | tr '\0' 'A' >&4 2>/dev/null
check "a normal request is still answered while those hang" "$(curl -s --max-time 3 -o /dev/null -w '%{http_code}' "http://127.0.0.1:$HTTP/status")" 401
exec 3>&- 4>&-
check "a request line of 100 KB is refused, not served" "$(curl -s --max-time 5 -o /dev/null -w '%{http_code}' "http://127.0.0.1:$HTTP/$(head -c 100000 /dev/zero | tr '\0' a)")" 414
check "100 headers of 1 KB each are refused or ignored, never a 5xx" "$(args=(); for i in $(seq 1 100); do args+=(-H "X-A$i: $(head -c 1000 /dev/zero | tr '\0' b)"); done; code=$(curl -s --max-time 5 -o /dev/null -w '%{http_code}' "${args[@]}" "http://127.0.0.1:$HTTP/status"); [ "$code" -lt 500 ] && echo ok)" ok

echo "== compatibility gate (a fake gnome-shell on PATH stands in for other desktops)"
check "this host is supported" "$("$BIN" --check-compat | jq -r .verdict.verdict)" supported
shim=$(mktemp -d /tmp/br-shim.XXXXXX)
for fake in "GNOME Shell 99.2:untested" "GNOME Shell 50.1:supported"; do
  printf '#!/bin/sh\necho "%s"\n' "${fake%%:*}" > "$shim/gnome-shell"; chmod +x "$shim/gnome-shell"
  check "fake '${fake%%:*}' is ${fake##*:}" "$(PATH="$shim:$PATH" "$BIN" --check-compat | jq -r .verdict.verdict)" "${fake##*:}"
done
printf '#!/bin/sh\necho "GNOME Shell 99.2"\n' > "$shim/gnome-shell"
refusal=$(PATH="$shim:$PATH" timeout 5 "$BIN" --headless --listen 127.0.0.1:18093 --state-dir "$state" 2>&1); code=$?
check "an untested desktop refuses to start" "$code" 1
echo "$refusal" | grep -q "allow-untested" && echo "ok   the refusal names the override" || { echo "FAIL refusal text: $refusal"; fail=1; }
PATH="$shim:$PATH" timeout 3 "$BIN" --headless --allow-untested --listen 127.0.0.1:18093 --state-dir "$state" > /dev/null 2>&1; code=$?
check "--allow-untested starts it (stopped by the timeout)" "$code" 124
printf '#!/bin/sh\nexit 1\n' > "$shim/gnome-shell"
PATH="$shim:$PATH" timeout 5 "$BIN" --headless --allow-untested --listen 127.0.0.1:18093 --state-dir "$state" > /dev/null 2>&1; code=$?
check "a host without GNOME Shell is refused even with --allow-untested" "$code" 1
rm -rf "$shim"

echo "== the console still answers and logged no secret"
check "still serving" "$(curl -s -o /dev/null -w '%{http_code}' "http://127.0.0.1:$HTTP/status")" 401
grep -q "$token" "$log" && echo "NOTE the one-time URL (with its token) is printed at start by design" || true
[ "$fail" = 0 ] && echo "NET OK" || echo "NET CHECKS FAILED (log: $log)"
exit "$fail"
