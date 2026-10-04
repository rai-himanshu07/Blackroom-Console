# Blackroom Console runbook

For the person who owns the laptop. Everything here is a command you type; nothing runs by itself.

## First time

```
blackroom setup                         # authenticator (QR), Remote Access Key, recovery codes, starts the login authority, test login
sudo blackroom-grant-input grant        # once per boot: lets the console grab the built-in keyboard and touchpad
systemctl --user start blackroom-console.service
cat $XDG_RUNTIME_DIR/blackroom-console/url        # the https address for the tablet
```

`setup` is safe to run again: it keeps what exists and prints a secret only when it creates one. Store the key and the
recovery codes in a password manager when they appear; they are never shown again. The first time a browser opens the
address it shows a certificate warning (a self-signed certificate on your own laptop); continue, then log in with your
Linux password, an authenticator code and the key. Tick "trust this browser" to skip the key next time.

From outside your home network (mobile data, another city): `blackroom internet`, a guided choice between a VPN such as
Tailscale (works for everyone) and direct access (needs a router that accepts connections: a name with a real certificate, or
only a static IP with the console's own certificate). `setup` offers it as its last step. Details and the router checklist:
`docs/ops/internet-access.md`.

## Daily use

1. Phone or tablet: open the address, log in, pick Private or Shared and press **Connect**. In Private mode the laptop panel goes
   blank and its keyboard and touchpad stop; in Shared mode they stay as they are.
2. **Disconnect** in the page's menu ends the session: a blanked screen is restored, blocked input is released, and the laptop
   is locked when the session's lock setting is on (the default for Private, off for Shared; the owner can force it either way
   in Host settings). Closing the page or losing the connection does the same after the configured silence timeout (30 seconds
   by default), so a lost client does not leave a blank panel. After a crash of the console the restore timer acts within about
   60 seconds and locks the screen.
3. On the laptop itself, hold Left Ctrl + Left Shift + Left Alt + Esc for 2 seconds: the emergency chord ends the session
   and every browser login.

Clipboard: the two buttons under the keyboard row send text to the laptop or fetch the laptop's text. Text only, 256 KiB.

## When something is wrong

| Symptom | Do |
|---|---|
| Anything odd | `blackroom repair` (read-only report), then `blackroom repair --fix` for the safe fixes |
| Panel stays black, no tablet | Wait 60 s (the restore timer runs). Still black: from another device over SSH run `systemctl --user stop blackroom-console.service`, then `loginctl unlock-session "$(loginctl show-user $USER -p Display --value)"` |
| "no rw access" or the grab does not start | `sudo blackroom-grant-input status`, then `sudo blackroom-grant-input grant` (the ACLs reset at reboot) |
| Login says the authority is unavailable | `systemctl --user status remote-hostd.service`, `journalctl --user -u remote-hostd -n 30`; `blackroom repair --fix` |
| Login refused after typos | Five failures from one address lock it for 15 minutes (doubling to 4 hours). Wait, or use a trusted browser |
| Lost phone or authenticator | `blackroom reset security` on the laptop (new authenticator, key and recovery codes; every trusted browser is dropped) |
| Lost the key only | `blackroom --state-dir ~/.local/share/blackroom-console/hostd rotate-key --account $USER --revoke-devices` |
| Lost both the key and a trusted browser | Same as lost key: you need the laptop (or SSH) for a new key |
| Stolen or lost tablet | `blackroom --state-dir ~/.local/share/blackroom-console/hostd devices --account $USER`, then `revoke-device <id>`; or `reset security` |
| "emergency stop latched" | An emergency chord ended the last session. Stop the console unit, start it again |
| Turn remote access off now | `blackroom --state-dir ~/.local/share/blackroom-console/hostd disable` (ends every session; `enable` re-opens it) |

State directory: `~/.local/share/blackroom-console/hostd` (owner only). Audit trail: `blackroom --state-dir <that> logs`.
A diagnostics bundle without secrets: `blackroom --state-dir <that> diagnostics`.

## Reset levels

| Command | Effect |
|---|---|
| `blackroom reset soft` | Ends every session and stops the services. Credentials kept. Start again with `setup` |
| `blackroom reset security` | New authenticator, key and recovery codes; trusted browsers and sessions dropped. Prints the new secrets once |
| `blackroom reset full` | Forgets every credential and the host identity (asks you to type RESET, or pass `--yes`). Refuses while a safety latch is set. Files that are not Blackroom's are left alone |

## Check that it holds up

`docs/ops/soak.sh 60` (in a second terminal, while a session is running from the tablet) samples the console for an
hour and prints SOAK OK or what drifted (memory, file descriptors, threads, a dropped session). The page's status chip
(tap it) shows the same figures live. `docs/ops/headless-cycles-test.sh` repeats Start/Stop 100 times against a throwaway
Shell without touching your screen.

## Update and remove

```
sudo apt install ./blackroom-console_<version>_amd64.deb     # update: credentials and state are untouched
blackroom reset full                                          # only if you want the credentials gone
sudo apt remove blackroom-console                             # keeps your state directory (purge also removes the PAM file)
```

## Outside your home network

See `internet-access.md`: Tailscale (nothing exposed, recommended), or a port-forward with a real certificate and
`--public`. `--public` refuses to start without a certificate from a real authority and without the three-factor login.

## Known limits (see also the release notes)

One display only (the built-in panel), no file transfer, one controller at a time. GNOME on Wayland only.
Safari and iOS are not yet observed. A session survives a network drop for about 30 seconds.
