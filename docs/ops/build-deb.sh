#!/usr/bin/env bash
# Builds target/deb/blackroom-console_<version>-1_<arch>.deb with dpkg-deb (no extra tooling) and checks it:
#   docs/ops/build-deb.sh [--no-build]
# The package installs binaries to /usr/lib/blackroom, the CLI to /usr/bin, user units (never enabled), the PAM service
# file, the polkit policy, the GNOME extension (never enabled), docs and `blackroom-grant-input`. It changes nothing
# else: remote access stays disabled until `blackroom setup`. Staging is in /tmp (the project disk is ntfs3).
set -euo pipefail
REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$REPO"
build=1; [ "${1:-}" = "--no-build" ] && build=0
VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' crates/blackroom-console/Cargo.toml | head -n 1)
ARCH=$(dpkg --print-architecture)
PKG=blackroom-console
OUT="$REPO/target/deb"
BINS=(blackroom-console remote-emergencyd remote-hostd pam-auth-helper exp07_restore)

if [ "$build" = 1 ]; then
  cargo build --release -p blackroom-console -p blackroom-cli -p remote-hostd -p pam-auth-helper -p remote-emergencyd \
    -p blackroom-experiments --bin blackroom-console --bin blackroom --bin remote-hostd --bin pam-auth-helper \
    --bin remote-emergencyd --bin exp07_restore
fi
for b in "${BINS[@]}" blackroom; do [ -x "target/release/$b" ] || { echo "missing target/release/$b" >&2; exit 1; }; done

stage=$(mktemp -d /tmp/br-deb.XXXXXX)
trap 'rm -rf "$stage"' EXIT
root="$stage/root"
install -d -m 755 "$root/usr/bin" "$root/usr/sbin" "$root/usr/lib/blackroom" "$root/usr/lib/systemd/user" \
  "$root/etc/pam.d" "$root/usr/share/polkit-1/actions" "$root/usr/share/doc/$PKG" "$root/DEBIAN" \
  "$root/usr/share/gnome-shell/extensions"
for b in "${BINS[@]}"; do install -m 755 "target/release/$b" "$root/usr/lib/blackroom/$b"; done
install -m 755 target/release/blackroom "$root/usr/bin/blackroom"
install -m 755 packaging/blackroom-grant-input "$root/usr/sbin/blackroom-grant-input"
install -m 644 packaging/units/*.service "$root/usr/lib/systemd/user/"
install -m 644 pam/blackroom-console "$root/etc/pam.d/blackroom-console"
install -m 644 polkit/org.blackroom.console.policy "$root/usr/share/polkit-1/actions/"
cp -r docs/ops/gnome-extension/blackroom-locked-remote@blackroom.local "$root/usr/share/gnome-shell/extensions/"
find "$root" -type d -exec chmod 755 {} +
find "$root/usr/share/gnome-shell/extensions" -type f -exec chmod 644 {} +
for d in docs/ops/README.md docs/ops/internet-access.md docs/security/authentication.md docs/security/credential-lifecycle.md \
  docs/security/threat-model.md docs/security/emergency-daemon.md; do
  [ -f "$d" ] && install -m 644 "$d" "$root/usr/share/doc/$PKG/$(basename "$d")"
done
for f in packaging/README.Debian; do [ -f "$f" ] && install -m 644 "$f" "$root/usr/share/doc/$PKG/$(basename "$f")"; done
install -m 644 LICENSE "$root/usr/share/doc/$PKG/copyright"

# Library dependencies from the binaries themselves (dpkg-shlibdeps), plus what is loaded at run time.
mkdir -p "$stage/debian"
printf 'Source: %s\n\nPackage: %s\nArchitecture: any\n' "$PKG" "$PKG" > "$stage/debian/control"
shlibs=$(cd "$stage" && dpkg-shlibdeps -O -e"$root/usr/bin/blackroom" $(printf -- '-e%s ' "${BINS[@]/#/$root/usr/lib/blackroom/}") 2>/dev/null \
  | sed -n 's/^shlibs:Depends=//p')
runtime="gnome-shell, systemd, acl, libpam-modules, gstreamer1.0-plugins-bad, gstreamer1.0-plugins-good, gstreamer1.0-pipewire, gstreamer1.0-nice, pipewire"
(cd "$root" && find . -type f ! -path './DEBIAN/*' -printf '%P\n' | sort | xargs md5sum) > "$root/DEBIAN/md5sums"
printf '/etc/pam.d/blackroom-console\n' > "$root/DEBIAN/conffiles"
size=$(du -sk --exclude=DEBIAN "$root" | cut -f1)
cat > "$root/DEBIAN/control" <<CONTROL
Package: $PKG
Version: $VERSION-1
Architecture: $ARCH
Maintainer: $(git config user.name) <$(git config user.email)>
Installed-Size: $size
Depends: ${shlibs:+$shlibs, }$runtime
Recommends: policykit-1 | polkitd
Suggests: coturn, tailscale
Section: utils
Priority: optional
Homepage: https://example.invalid/blackroom-console
Description: control this GNOME laptop from a browser while its panel is blank
 A tablet or phone browser sees and drives the laptop's GNOME session while the
 local panel is blank and the built-in keyboard and touchpad are grabbed.
 Login needs the Linux password, an authenticator code and a Remote Access Key
 or trusted device. Nothing is enabled or started by installing this package:
 run "blackroom setup", then "sudo blackroom-grant-input grant".
CONTROL
install -m 755 packaging/postinst "$root/DEBIAN/postinst"
install -m 755 packaging/postrm "$root/DEBIAN/postrm"
chmod 755 "$root/DEBIAN"

mkdir -p "$OUT"
deb="$OUT/${PKG}_${VERSION}-1_${ARCH}.deb"
dpkg-deb --root-owner-group --build "$root" "$deb" >/dev/null
echo "built $deb ($(stat -c %s "$deb") bytes)"

# ---- checks (read-only) ----
fail=0
check() { if eval "$2"; then echo "ok   $1"; else echo "FAIL $1"; fail=1; fi; }
contents=$(dpkg-deb -c "$deb")
check "nothing outside the expected prefixes" '! echo "$contents" | awk "{print \$6}" | grep -Ev "^\./($|usr/|etc/pam.d/)" | grep -v "^\./etc/$" | grep -q .'
check "no setuid or setgid bits" '! echo "$contents" | awk "{print \$1}" | grep -Eq "^-..[sS]|^-.....[sS]"'
check "nothing writable by group or others" '! echo "$contents" | awk "{print \$1}" | grep -Eq "^.....w|^........w"'
check "no unit has an [Install] section" '! tar -xOf <(dpkg-deb --fsys-tarfile "$deb") --wildcards "./usr/lib/systemd/user/*.service" 2>/dev/null | grep -q "^\[Install\]"'
check "units run only /usr/lib/blackroom binaries" '! tar -xOf <(dpkg-deb --fsys-tarfile "$deb") --wildcards "./usr/lib/systemd/user/*.service" 2>/dev/null | grep "^ExecStart=" | grep -v "ExecStart=/usr/lib/blackroom/" | grep -q .'
check "no path of this repository inside the units" '! tar -xOf <(dpkg-deb --fsys-tarfile "$deb") --wildcards "./usr/lib/systemd/user/*.service" 2>/dev/null | grep -q "Playground"'
check "conffile is the PAM service only" '[ "$(dpkg-deb -I "$deb" conffiles 2>/dev/null)" = "/etc/pam.d/blackroom-console" ]'
check "maintainer scripts parse" 'sh -n packaging/postinst && sh -n packaging/postrm && sh -n packaging/blackroom-grant-input'
if sim=$(apt-get -s install "$deb" 2>&1); then
  echo "ok   apt-get --simulate install: $(echo "$sim" | grep -c '^Inst') package(s) would be installed"
else
  echo "WARN apt-get --simulate could not resolve (uninstalled dependencies?):"; echo "$sim" | tail -n 5
fi
dpkg-deb -I "$deb" | sed -n '1,40p' | grep -E "Package:|Version:|Depends:|Installed-Size:" | cut -c1-200
[ "$fail" = 0 ] && echo "DEB OK" || { echo "DEB CHECKS FAILED"; exit 1; }
