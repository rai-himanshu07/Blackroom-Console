# Owner live checks (W3)

Only you can run these: they need the real desktop, your password or your devices. Nothing here is claimed as passed until you
report it. Each item has the exact steps, a pass line and what to send back (a sentence is enough; logs help but are optional).
Keep a second device with an SSH login ready for 4, and save your work first.

Before you start: install the newest build (`sudo apt install ./target/deb/blackroom-console_0.1.0-1_amd64.deb` after
`docs/ops/build-deb.sh`), then `systemctl --user restart blackroom-console.service` so the console, the grab daemon and the
extensions are the new ones. The tray extension needs a log out and in once to load its new code.

## 1. Real credential commands from the host page, and the next login still works

1. Tray menu, Host settings (or `http://127.0.0.1:8090/` on the laptop). Sign in with your laptop password.
2. In "Login and credentials": type your password, press **New recovery codes**. Store the codes.
3. Type the password again, press **New remote access key**. Store the key. Leave "Also forget trusted browsers" off.
4. From the tablet, in a private tab (so no trusted browser applies), sign in with laptop password + authenticator code + the **new** key.
5. Try the **old** key the same way.
6. Back on the host page, "Trusted browsers": press **Forget** on the tablet's row (password first), reload the tablet page: it must ask for the key again.

**Pass:** each action says "Done." and shows its secret once; the new key logs in; the old key is refused; after Forget the
tablet needs the key. **Send back:** which step, if any, failed, with the message shown inside the card.

## 2. Start at login, restart under systemd, a reboot with nobody logged in

1. Host settings, "This laptop": switch on "Start the console when you log in". Log out and in.
2. `systemctl --user is-active blackroom-console.service` and `cat "$XDG_RUNTIME_DIR/blackroom-console/url"`.
3. Open the address on the tablet. Then, with no session running: `systemctl --user restart blackroom-console.service`; reload the tablet.
4. Restart the laptop and stop at the login screen without logging in. From the tablet, open the address.

**Pass:** 2 prints `active` and a URL, the tablet logs in; 3 comes back within about 10 s with no new setup; 4 is expected to
**not answer** (a user service starts only after login; `loginctl show-user $USER -p Linger` says `no`). **Send back:** whether 4
answered, and the Linger value, so the support table can say it in one line.

## 3. Laptop sleep and lid close with Tailscale

1. Tailscale connected on both devices, console running, **no** session. Close the lid for about 2 minutes (or `systemctl suspend`), open it.
2. On the tablet (mobile data), open the https name. Note how long it took to answer after the resume.
3. Repeat with a Private session running: close the lid for about 2 minutes, open it, look at the tablet and at the laptop.

**Pass for 1 and 2:** the page answers within about a minute of the resume without any command. **For 3 there is no pass line:**
write down what happened (session ended or survived, what the page said, whether the laptop's screen and keyboard were back and
locked). **Send back:** both observations.

## 4. The emergency chord with the packaged console (supervised)

The packaged daemon now writes hostd's stop marker on the chord and does not lock by itself. Do this with SSH from a second device
open (`systemctl --user stop blackroom-console.service` ready to type), your work saved, and not as the first thing after a
display change.

1. Private session from the tablet (blank screen and "Block this laptop's keyboard and touchpad" on, "Lock the laptop when I disconnect" on).
2. On the laptop hold **Left Ctrl + Left Shift + Left Alt + Esc** for 2 seconds.
3. Look at the laptop and the tablet; try to sign in again from the tablet.
4. Clear the marker as in `docs/ops/emergency-recovery.md` ("After the chord"), restart the console, sign in again.
5. Repeat 1 and 2 with "Lock the laptop when I disconnect" **off** (Shared sessions have no grab: the chord does nothing there; try it once to see that).

**Pass:** within about 2 s the keyboard and touchpad work again, the panel is restored, the tablet's session ends, a new tablet
login is refused until step 4, and after step 4 it works. In 1 to 3 the laptop is locked because the lock setting was on; in 5 it is not.
**Send back:** each of those, and anything unexpected (a frozen Shell, a black panel, a stuck key). Locking from the daemon itself
(`--lock-on-emergency`) stays off until you decide, after this run, to try it once more the same way.

## 5. Certificate renewal

1. Run the same command you used to make the certificate: `sudo tailscale cert --cert-file ~/.config/blackroom/tls/cert.pem --key-file ~/.config/blackroom/tls/key.pem <name>`, then the two `chown`/`chmod` lines from `docs/ops/internet-access.md`.
2. Wait up to 6 hours, or `systemctl --user restart blackroom-console.service` to see it at once. Open Host settings, "Currently running".

**Pass:** "Currently running" shows the new number of days left; the tablet's browser shows no certificate warning. The renewal
banner and the tray line appear only in the last 30 days of a certificate, so unless yours is that old you cannot see them clear:
say which case you are in. **Send back:** the days left before and after.

## 6. A real 30 minute session

1. Press Start in the tablet's page, then in a second terminal on the laptop run `docs/ops/soak.sh 30`. Use the tablet for a real sitting of about 30 minutes (not idle).
2. When it finishes, read its last line.

**Pass:** `SOAK OK`. **Send back:** that line, or what it says drifted, and whether you noticed any lag or drop yourself.
This replaces the assumed hour: the support table keeps "soak: not run" until you report it.

## 7. Optional

Direct mode with a name and a real certificate, and a relayed (not direct) Tailscale path. Direct with a static IP and the self-signed certificate was reported working on 2026-10-06.
