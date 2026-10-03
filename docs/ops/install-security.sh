#!/usr/bin/env bash
# Installs the Phase 11 security pieces for the current user. Run from anywhere:
#   docs/ops/install-security.sh --check       read-only report (also proves the PAM stack loads)
#   docs/ops/install-security.sh --install     build, copy binaries, install the unit; asks sudo for TWO files
#   docs/ops/install-security.sh --uninstall   stop and remove the unit and the two root files (keeps your credentials)
# Options: --no-build (reuse target/release), --polkit (let `blackroom enable` need a local active session).
# Root is used for exactly: /etc/pam.d/blackroom-console and /usr/share/polkit-1/actions/org.blackroom.console.policy.
set -euo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
LIB="$HOME/.local/lib/blackroom"
BIN="$HOME/.local/bin"
STATE="$HOME/.local/share/blackroom-console/hostd"
UNIT_DIR="$HOME/.config/systemd/user"
PAM_FILE=/etc/pam.d/blackroom-console
POLKIT_FILE=/usr/share/polkit-1/actions/org.blackroom.console.policy
mode=""; build=1; polkit=0
for arg in "$@"; do
  case "$arg" in
    --check|--install|--uninstall) mode="$arg" ;;
    --no-build) build=0 ;;
    --polkit) polkit=1 ;;
    *) echo "unknown argument: $arg" >&2; exit 2 ;;
  esac
done
[ -n "$mode" ] || { sed -n '2,7p' "${BASH_SOURCE[0]}"; exit 2; }
[ "$(id -u)" -ne 0 ] || { echo "run as your normal user, not root (sudo is asked only where needed)" >&2; exit 2; }

ok()   { echo "OK   $*"; }
bad()  { echo "FAIL $*"; failed=1; }
warn() { echo "WARN $*"; }

check() {
  failed=0
  [ -x "$LIB/remote-hostd" ] && ok "remote-hostd installed" || bad "remote-hostd missing in $LIB (run --install)"
  [ -x "$LIB/pam-auth-helper" ] && ok "pam-auth-helper installed" || bad "pam-auth-helper missing in $LIB"
  [ -x "$BIN/blackroom" ] && ok "blackroom CLI installed" || warn "blackroom CLI missing in $BIN"
  if [ -d "$STATE" ] && [ "$(stat -c %a "$STATE")" = 700 ]; then ok "state dir $STATE is 0700"; else bad "state dir $STATE missing or not 0700"; fi
  [ -f "$PAM_FILE" ] && ok "$PAM_FILE present" || bad "$PAM_FILE missing"
  [ -f "$POLKIT_FILE" ] && ok "polkit policy present" || warn "polkit policy missing (only needed with --polkit)"
  [ -f "$UNIT_DIR/remote-hostd.service" ] && ok "user unit installed" || bad "user unit missing"
  if [ -x "$LIB/pam-auth-helper" ] && [ -f "$PAM_FILE" ]; then
    # A deliberately wrong password: exit 1 (rejected) proves the PAM stack loads; 2 means it could not decide.
    set +e; printf '%s\nthis-is-not-the-password\n' "$USER" | env -i "$LIB/pam-auth-helper" --service blackroom-console; code=$?; set -e
    [ "$code" = 1 ] && ok "PAM stack answers (wrong password rejected)" || bad "PAM self-test returned $code (expected 1)"
  fi
  if [ -d "$STATE" ]; then "$BIN/blackroom" --state-dir "$STATE" doctor 2>/dev/null | sed 's/^/     /' || true; fi
  return $failed
}

case "$mode" in
--check) check; exit $? ;;
--install)
  if [ "$build" = 1 ]; then
    echo "== building (needs libpam0g-dev and libclang for the PAM bindings)"
    (cd "$REPO" && cargo build --release -p remote-hostd -p blackroom-cli \
      && cargo build --release --manifest-path crates/pam-auth-helper/Cargo.toml --target-dir target/pam-helper)
  fi
  HELPER="$REPO/target/pam-helper/release/pam-auth-helper"
  for f in "$REPO/target/release/remote-hostd" "$REPO/target/release/blackroom" "$HELPER"; do [ -x "$f" ] || { echo "missing $f" >&2; exit 1; }; done
  install -d -m 700 "$LIB" "$STATE"; install -d -m 755 "$BIN" "$UNIT_DIR"
  install -m 755 "$REPO/target/release/remote-hostd" "$HELPER" "$LIB/"
  install -m 755 "$REPO/target/release/blackroom" "$BIN/blackroom"
  unit="$UNIT_DIR/remote-hostd.service"
  if [ "$polkit" = 1 ]; then
    sed 's#--pam-helper \(.*\)pam-auth-helper$#--pam-helper \1pam-auth-helper --polkit#' "$REPO/systemd/user/remote-hostd.service" > "$unit"
  else
    install -m 644 "$REPO/systemd/user/remote-hostd.service" "$unit"
  fi
  systemctl --user daemon-reload
  echo "== root step: two files (you will be asked for your password by sudo)"
  echo "   $PAM_FILE"; cat "$REPO/pam/blackroom-console" | sed 's/^/     | /'
  echo "   $POLKIT_FILE (policy for the optional --polkit gate)"
  sudo install -o root -g root -m 0644 "$REPO/pam/blackroom-console" "$PAM_FILE"
  sudo install -o root -g root -m 0644 "$REPO/polkit/org.blackroom.console.policy" "$POLKIT_FILE"
  echo "== installed. Next:"
  echo "   $BIN/blackroom --state-dir $STATE enroll --account $USER          # authenticator secret, shown once"
  echo "   $BIN/blackroom --state-dir $STATE rotate-key --account $USER      # Remote Access Key, shown once"
  echo "   $BIN/blackroom --state-dir $STATE recovery-codes --account $USER  # optional, shown once"
  echo "   systemctl --user start remote-hostd.service"
  echo "   $BIN/blackroom --state-dir $STATE login-check --account $USER     # real password+code+key test"
  echo "   docs/ops/install-security.sh --check"
  ;;
--uninstall)
  systemctl --user stop remote-hostd.service 2>/dev/null || true
  rm -f "$UNIT_DIR/remote-hostd.service"; systemctl --user daemon-reload
  rm -f "$LIB/remote-hostd" "$LIB/pam-auth-helper" "$BIN/blackroom"
  echo "== root step: removing the two root files"
  sudo rm -f "$PAM_FILE" "$POLKIT_FILE"
  echo "removed. Your credentials in $STATE were kept; delete that directory yourself to forget them."
  ;;
esac
