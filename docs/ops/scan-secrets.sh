#!/usr/bin/env bash
# Secret scan before any push: gitleaks and trufflehog over the whole history and the current files. Nothing leaves this
# machine (trufflehog runs with --no-verification, so no found value is sent to a vendor). Needs both tools on PATH.
#   docs/ops/scan-secrets.sh
set -u
cd "$(dirname "$0")/../.."
for tool in gitleaks trufflehog rsync python3; do command -v "$tool" > /dev/null || { echo "missing: $tool"; exit 2; }; done
tmp=$(mktemp -d); trap 'rm -rf "$tmp"' EXIT
fail=0
# The files as they are now (tracked or not, ignored ones left out), so target/ and other build output are not scanned.
mkdir "$tmp/tree"; git ls-files -co --exclude-standard -z | rsync -a --from0 --files-from=- ./ "$tmp/tree/"
check() { if [ "$2" = 0 ]; then echo "ok   $1"; else echo "FAIL $1: $2 finding(s)"; fail=1; fi; }
count() { python3 -c 'import json,sys; print(len(json.load(open(sys.argv[1])) or []))' "$1"; }
gitleaks git . --redact --no-banner -l error -r "$tmp/gh.json" > /dev/null 2>&1; check "gitleaks, whole history" "$(count "$tmp/gh.json")"
gitleaks dir "$tmp/tree" --redact --no-banner -l error -r "$tmp/gt.json" > /dev/null 2>&1; check "gitleaks, current files" "$(count "$tmp/gt.json")"
trufflehog git file://. --no-update --no-verification --json --no-color > "$tmp/th.json" 2> /dev/null; check "trufflehog, whole history" "$(grep -c DetectorName "$tmp/th.json")"
trufflehog filesystem "$tmp/tree" --no-update --no-verification --json --no-color > "$tmp/tt.json" 2> /dev/null; check "trufflehog, current files" "$(grep -c DetectorName "$tmp/tt.json")"
[ "$fail" = 0 ] && echo "SECRET SCAN OK" || { echo "SECRET SCAN FAILED"; exit 1; }
