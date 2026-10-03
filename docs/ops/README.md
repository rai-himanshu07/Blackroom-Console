# docs/ops/

Operational procedures for running Blackroom Console and its feasibility experiments
safely on real hardware.

- `experiment-safety.md` — out-of-band recovery procedure required before any experiment
  that changes display or input state (assessment §8).
- `live-grab-runbook.md` + `live-grab-session.sh` — supervised live test of the physical-input
  grab through gateway, hostd and `remote-emergencyd`, driven from a second device over SSH.

## Remote console (`blackroom-console`)

Start: `docs/ops/console.sh --check` (read-only preflight), then `docs/ops/console.sh`, or the user units in
`systemd/user/` (copy to `~/.config/systemd/user/`, `systemctl --user start blackroom-console.service`; no `enable`,
no autostart; the daemon unit has a 10 s watchdog kill). Either way the operator sets the `setfacl` entries on
`/dev/input/event2-5` first and removes them afterwards, keeps a second-device SSH session open, and disables the
extension after use.

Observed live (this laptop, built-in eDP-1 panel at scale 1.0, built-in input nodes, one tablet): Start isolates
the panel and grabs the built-in keyboard and touchpad; typing, pointer, click and scroll work; Stop restores the
display, locks the screen and releases the grab; 15 s without a heartbeat ran Stop with a clean restore; a console killed with SIGKILL mid-session was locked within a few milliseconds by the crash guard and the display restored by the 60 s timer (run 20); Start on a
locked screen with the extension enabled beforehand showed the lock screen and the account password unlocked it.
Not observed: Safari or Android decode, iOS fullscreen, enabling the extension on an already locked screen, idle
screen blanking during a session, input over the
WebRTC data channel on the tablet, suspend, logout, long runs, other displays or layouts.

Known failure: restarting PipeWire (or a PipeWire crash) while a session streams aborts the GNOME Shell (run 21): you land on the login screen, the session is lost, and `console.sh` leaves the daemon, its kill timer and a masked `gnome-remote-desktop` behind (run `docs/ops/console.sh --cleanup`: it stops the timers, guard and orphaned daemon and unmasks the service; it refuses while a console runs). Do not restart PipeWire during a session.

Kill switches while a session is live: the chord (Left Ctrl + Left Shift + Left Alt + Esc, held 2 s), the Stop
button, 15 s without a browser heartbeat, and over SSH `systemctl --user stop blackroom-console.service` or
`pkill -TERM -x blackroom-conso` (runs Stop), then `pkill -KILL -x remote-emergenc` (releases the grab at once).
If the panel stays black: `exp07_restore --keep-live-virtual --lock-after --backup <state dir>/backup.json`
(the 60 s dead-man restore does this by itself and now also locks), then `loginctl unlock-session <id>`.

Locked screen: GNOME refuses remote sessions while locked. `gnome-extension/blackroom-locked-remote@blackroom.local`
lifts that so the console can show the lock screen and the account password is typed remotely (no unlock bypass).
Install once: copy the directory to `~/.local/share/gnome-shell/extensions/`, log out and in, then
`gnome-extensions enable blackroom-locked-remote@blackroom.local` while using the console and `disable` afterwards.
You can switch it from the tray menu (**Remote use on the lock screen**) or the host settings page (asks for your password to
turn it on); the console's own hint when the screen is locked points there. While the extension is enabled, locking no longer ends remote sessions: the lock screen is not a kill switch, and
any local process of your user may open a remote session on the locked screen. The kill switches above are the
real ones. Enabling it on an already locked screen lifts the existing block (new code, not yet observed live);
disabling it while locked brings the block back only once the remote sessions have ended (after Stop).

Stop order: restore the display, stop the capture, lock, and only then release the input grab, so the local
keyboard never reaches an unlocked desktop while the panel comes back. The grab lease is renewed before the lock.

State directory (`$XDG_RUNTIME_DIR/blackroom-console`): `last_stop.json` records how far the last Stop got (`step`,
and the report once done), so a Stop that dies half way is visible afterwards; `recovery.json` exists while a live
session holds the display and is removed after a verified restore. If a console dies mid-session, the next console
start restores from `backup.json` (same login session and Shell only) and locks; if that fails, Start is refused
with the file to remove after checking the panel. A live session also holds a logind `sleep:idle` block inhibitor.

Offline simulation state (never a live host): hostd appends security events to
`audit.log` in its state directory (owner-only, 1 MiB then one rotation; static
codes, epochs and principal ids only, never proofs, demo codes, input grants
or session tokens; grant issuance fails closed if it cannot be logged). The
read-only `blackroom --state-dir <abs path> status | logs [--tail N] | doctor`
CLI reports epoch and stop markers, prints valid audit lines, and checks
ownership/modes. It never creates, locks or changes state and refuses
symlinked, relative or loose-permission directories.

Runbooks for the released product (incident response, recovery) live in
`docs/plans/Detailed_Project_Plan/21_OPERATIONAL_RUNBOOK_RECOVERY_AND_INCIDENT_RESPONSE.md`;
this directory holds the project's own dev-workstation procedures.

### The app launcher (applications menu)

The `.deb` adds **Blackroom Console** to the applications menu (`/usr/bin/blackroom-app`, a desktop entry and an icon). Opening
it turns the top-bar icon on, starts `blackroom-console.service` and opens the host settings page (`http://localhost:8090/`);
its right-click actions are **Host settings** and **Exit Blackroom Console**. `blackroom-app exit` (and **Exit** in the tray menu)
stops the console, ending any remote session the normal way (display restored, screen locked), and removes the top-bar icon;
a console that was started by hand is not touched and keeps its icon. Proof without touching the real desktop:
`docs/ops/launcher-test.sh`. Not observed: the entry in the real applications menu and the real **Exit** click.

### Laptop indicator (top bar)

`gnome-extension/blackroom-indicator@blackroom.local` puts an icon in the GNOME top bar: dim when the console is off, normal
when it is ready, orange with a timer while a remote session runs. Its menu shows the mode (private or shared), how long the
session has run and whether laptop sound is sent, and offers **Disconnect the remote user**, **Lock this screen now**,
**Host settings...** (the laptop-only settings page; the client page is deliberately not offered on the laptop),
a **Remote use on the lock screen** switch (turns the lock-screen extension below on or off), **Start/Stop the console**, and **Exit** (stops the console and removes the icon until the launcher is used again) (`systemctl --user start|stop blackroom-console.service`, so the
user unit must be installed). A notification appears when a session starts and when it ends. Install like the other
extension (copy the directory to `~/.local/share/gnome-shell/extensions/`, log out and in; the `.deb` installs it under
`/usr/share/gnome-shell/extensions/`), then `gnome-extensions enable blackroom-indicator@blackroom.local`. Unlike the
locked-remote extension it may stay enabled: it changes nothing about access.

It talks to the console over the session D-Bus (`org.blackroom.Console`, `Status` and `Disconnect`). That service can only
report state and end a session; any process of your own user can call it, which is no more than `pkill` or
`systemctl --user stop` already allow. In Private mode the panel is blank, so the icon is for Shared mode and for before and
after a session. Proof without hardware: `docs/ops/headless-indicator-test.sh` (the console's D-Bus service, the extension
loaded into a throwaway Shell, a click on its menu's Disconnect) and `node docs/ops/indicator-logic-test.mjs`. Not observed:
the real top bar, notifications, Lock and Start/Stop from the menu.

### Host settings page and approval

The console also serves a settings page for you, on `127.0.0.1:8090` only (`--host-listen`; with `--headless` only when
given). It needs the laptop account's password (the PAM helper `pam-auth-helper`, `--pam-helper`), changes `host.json`
(allowed modes, limits, sound, clipboard, network, indicator options, approval) and manages the credentials through the
`blackroom` command, asking for the password again before each change. Open it from the indicator menu. With approval set to
Ask, a client's Start waits up to 30 seconds for Accept or Deny on the laptop (indicator notification and menu, or this
page); no answer is a Deny. Proof without hardware: `docs/ops/headless-hostpage-test.sh` (Chrome against a stand-in password
check and a stand-in `blackroom` command, so the real credentials are never touched) and
`docs/ops/headless-indicator-test.sh` (approve, deny and no answer, seen in a throwaway Shell). Not observed: the real PAM
check, the real `blackroom` verbs from the page, start-at-login and the restart button under systemd, notifications on the
real desktop. See `docs/ops/settings-guide.md`.

The authenticator is set up from the page too (QR code or manual key, then one confirming code); `blackroom setup` in a terminal
still works and prints a text QR code without a confirming step.

### Modes, settings and the web app

The page opens on a connect screen with Private and Shared modes and a settings sheet whose choices are saved on the
laptop (`--profile-dir`, default `~/.local/share/blackroom-console`; `--audio-sink <name>` picks the sound source
instead of the default output). Choices are kept on each device (the laptop's profile only supplies defaults) and limited by the host settings. The page can be installed as an app. See `docs/ops/settings-guide.md`. Proof without
hardware: `BR_BIN=docs/ops/headless-modes-test.sh docs/ops/headless-repro.sh` (modes, options, audio, reset) and
`BR_BIN=docs/ops/headless-browser-test.sh docs/ops/headless-repro.sh` (the whole page in Chrome, audio tone, install
checks). Not observed live: Shared mode on the real screen, laptop sound from the real output, install on a tablet.

### Clipboard (text only)

`console.sh` starts the console with `--clipboard` (set `BR_CLIPBOARD=0` to leave it out; the binary alone has it off).
While a session runs, the page shows a text box with two buttons: **Send to laptop** puts the box text (or this
device's clipboard when the box is empty) on the laptop clipboard, and **Get from laptop** shows the laptop's clipboard
text in the box and copies it on this device. Each is one explicit tap: nothing is synced in the background.
Limits: text only, 256 KiB, one request per 500 ms, the same login and same-origin checks as input. The text is held in
memory only while the session runs and is never logged (the log has sizes). Reading this device's clipboard needs https
or localhost and the browser may ask first; the box is the fallback. Proof without hardware:
`BR_BIN=docs/ops/headless-clipboard-test.sh docs/ops/headless-repro.sh` (both directions through a real Mutter selection).

### TOTP login (optional, Phase 11 slice 1)

Default is the URL token. To log in with an authenticator code instead:

1. Enrol once into a private, persistent directory (not `$XDG_RUNTIME_DIR`, it is tmpfs):
   `mkdir -p -m 700 ~/.local/share/blackroom-console/auth && target/release/blackroom --state-dir ~/.local/share/blackroom-console/auth enroll --account <name>`
   (prints the otpauth URI once; add it to the authenticator app).
2. Start with `BR_AUTH_DIR=~/.local/share/blackroom-console/auth BR_ACCOUNT=<name> docs/ops/console.sh`
   (or `blackroom-console --auth-dir <dir> --account <name>`). The printed URLs carry no token.
3. The page asks for the 6-digit code. A code is accepted once; repeated failures lock the account.
   Sessions end after 30 min idle, 12 h absolute, and at any emergency stop (chord). Sessions are
   in memory only, so a console restart logs everyone out.

Not covered: logout button, rate limits per client address (one shared counter), live use.

### Login authority (Phase 11): password + authenticator + key or trusted device

Separate from the console's own TOTP mode above. Install and check with `docs/ops/install-security.sh`
(`--check` is read-only; `--install` builds, copies binaries to `~/.local/lib/blackroom`, installs the
user unit and, with sudo, `/etc/pam.d/blackroom-console` and the polkit policy; `--uninstall` reverses it).
Then, once: `blackroom --state-dir ~/.local/share/blackroom-console/hostd enroll --account $USER`,
`rotate-key --account $USER`, optionally `recovery-codes --account $USER`; start with
`systemctl --user start remote-hostd.service`; test the real chain with `blackroom ... login-check --account $USER`
(it asks for the password, the authenticator code and the key on the terminal and prints only the verdict).
Operator verbs: `status`, `sessions`, `revoke-session <id>`, `revoke-all`, `disable [--reason ...]`, `enable`,
`rotate-key [--revoke-devices]`, `devices`, `revoke-device <id>`, `doctor`, `diagnostics`, `compatibility`.
`disable` works with hostd stopped. Details: `docs/security/authentication.md`, `credential-lifecycle.md`.

Console login through hostd: start `remote-hostd.service`, then `BR_HOSTD=1 docs/ops/console.sh` (or
`blackroom-console --hostd-dir $XDG_RUNTIME_DIR/blackroom-hostd`). The page asks for account, Linux password,
authenticator code and Remote Access Key; "Trust this browser" remembers a device credential so later logins skip the
key. After a code change run `docs/ops/install-security.sh --update` (no sudo) to replace the installed binaries.
