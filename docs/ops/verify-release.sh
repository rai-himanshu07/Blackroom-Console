#!/usr/bin/env bash
# Checks a downloaded release: the signature on SHA256SUMS against the project's public key, then the package against SHA256SUMS.
#   docs/ops/verify-release.sh <dir with the .deb, SHA256SUMS and SHA256SUMS.sig> [public key file, default docs/release-signing.pub]
# Compare the key's fingerprint with the one printed in the release notes before trusting it.
set -eu
dir=${1:?usage: verify-release.sh <dir> [public key]}
pub=${2:-$(dirname "$0")/../release-signing.pub}
[ -f "$pub" ] || { echo "no public key at $pub"; exit 2; }
signers=$(mktemp); trap 'rm -f "$signers"' EXIT
printf 'blackroom-console %s\n' "$(cut -d' ' -f1,2 "$pub")" > "$signers"
ssh-keygen -lf "$pub"
ssh-keygen -Y verify -f "$signers" -I blackroom-console -n blackroom-console -s "$dir/SHA256SUMS.sig" < "$dir/SHA256SUMS"
(cd "$dir" && sha256sum -c SHA256SUMS)
echo "RELEASE OK"
