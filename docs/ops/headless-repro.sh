#!/usr/bin/env bash
# Runs exp13 against a throwaway --headless GNOME Shell on a private D-Bus session. The real session bus,
# display and input are never addressed (exp13 also refuses a DisplayConfig owner that is not --headless).
# Usage: docs/ops/headless-repro.sh [--no-consumer] [--keep-virtual] [--join-before-stop] [--hold-ms N]
# (BR_GDB=1 prints the Shell's crash backtrace.) Default = the crashing variant: consumer streaming, restore omits the virtual monitor.
set -u
cd "$(dirname "$0")/../.." || exit 2
BIN=${BR_BIN:-target/debug/exp13_virtual_restore}
if [ "${1:-}" != "--inner" ]; then
  [ -x "$BIN" ] || { echo "build first: cargo build -p blackroom-experiments --bin exp13_virtual_restore"; exit 2; }
  exec env -u XDG_SESSION_ID -u WAYLAND_DISPLAY -u DISPLAY GSETTINGS_BACKEND=memory BR_PRIVATE_BUS=1 \
    dbus-run-session -- "$0" --inner "$@"
fi
shift
# Never start a Shell on the real session bus (a direct --inner call or a failed dbus-run-session).
[ "${BR_PRIVATE_BUS:-}" = 1 ] && [ "${DBUS_SESSION_BUS_ADDRESS:-}" != "unix:path=${XDG_RUNTIME_DIR:-/nonexistent}/bus" ] \
  || { echo "refusing: not on a private D-Bus session"; exit 2; }
trap 'rm -f "${XDG_RUNTIME_DIR:-/nonexistent}"/blackroom-repro "${XDG_RUNTIME_DIR:-/nonexistent}"/blackroom-repro.lock' EXIT
log=$(mktemp /tmp/br-headless-shell.XXXXXX)
runner=()
[ -n "${BR_GDB:-}" ] && runner=(gdb -batch -nx -ex 'handle SIGPIPE nostop noprint pass' -ex run -ex 'bt 14' \
  -ex 'info sharedlibrary libmutter' -ex 'info registers rip rax' -ex 'x/6i $pc' --args)
shell_env=()
# For the throwaway Shell only: BR_SHELL_DATA_DIR is an extra XDG data dir (for example one holding gnome-shell/extensions);
# BR_SHELL_CONFIG_DIR is a private config dir whose glib-2.0/settings/keyfile holds its settings (for example
# enabled-extensions), instead of the empty in-memory settings.
if [ -n "${BR_SHELL_DATA_DIR:-}${BR_SHELL_CONFIG_DIR:-}" ]; then
  shell_env=(env -u JOURNAL_STREAM "XDG_DATA_DIRS=${BR_SHELL_DATA_DIR:-/nonexistent}:${XDG_DATA_DIRS:-/usr/local/share:/usr/share}")
  [ -n "${BR_SHELL_CONFIG_DIR:-}" ] && shell_env+=(GSETTINGS_BACKEND=keyfile "XDG_CONFIG_HOME=$BR_SHELL_CONFIG_DIR")
fi
timeout -k 3 "${BR_SHELL_TIMEOUT:-150}" "${runner[@]}" "${shell_env[@]}" gnome-shell --headless --wayland --no-x11 --wayland-display=blackroom-repro \
  --virtual-monitor 1920x1080 >"$log" 2>&1 &
shell=$!
export BR_SHELL_LOG=$log
ready=0
for _ in $(seq 1 80); do
  if busctl --user --no-pager list 2>/dev/null | grep -q org.gnome.Mutter.DisplayConfig; then ready=1; break; fi
  kill -0 "$shell" 2>/dev/null || break
  sleep 0.5
done
if [ "$ready" != 1 ]; then
  echo "headless shell not ready; log: $log"; tail -n 5 "$log"
  kill "$shell" 2>/dev/null; wait "$shell" 2>/dev/null; exit 3
fi
timeout -k 3 "${BR_TIMEOUT:-90}" "$BIN" "$@"
rc=$?
sleep 1
if kill -0 "$shell" 2>/dev/null; then
  echo "RESULT exp13_exit=$rc headless_shell=alive"
  kill "$shell" 2>/dev/null; wait "$shell" 2>/dev/null
else
  wait "$shell" 2>/dev/null; src=$?
  echo "RESULT exp13_exit=$rc headless_shell=EXITED status=$src (139 = SIGSEGV)"
fi
echo "shell log: $log"
[ -n "${BR_GDB:-}" ] && grep -E '^#|libmutter|^=>|^   0x|^rip|^rax|SIGSEGV' "$log" | cut -c1-200 | head -n 40
