#!/usr/bin/env bash
# Writes SHA256SUMS for the built package(s) and signs it with an SSH key (the release identity):
#   docs/ops/sign-release.sh <path to the private signing key> [dir with the .deb, default target/deb]
# Publish SHA256SUMS, SHA256SUMS.sig and the .deb together, and keep the matching public key in the repository
# (docs/release-signing.pub, one line) so anyone can run docs/ops/verify-release.sh.
set -eu
key=${1:?usage: sign-release.sh <private key> [dir]}
dir=${2:-target/deb}
cd "$dir"
ls ./*.deb > /dev/null 2>&1 || { echo "no .deb in $dir (run docs/ops/build-deb.sh)"; exit 2; }
sha256sum ./*.deb | sed 's|\./||' > SHA256SUMS
rm -f SHA256SUMS.sig
ssh-keygen -Y sign -f "$key" -n blackroom-console SHA256SUMS > /dev/null
echo "wrote $dir/SHA256SUMS and $dir/SHA256SUMS.sig"
cat SHA256SUMS
