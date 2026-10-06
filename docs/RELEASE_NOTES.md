# Blackroom Console 0.1.0 release notes

A tablet or phone browser sees and drives this GNOME laptop while the laptop's own panel is blank and its built-in
keyboard and touchpad are grabbed. Built for one owner's own laptop; not a product.

## What is in it

- **Video and input:** WebRTC H.264 (NVENC, OpenH264 fallback) with an MJPEG fallback; pointer, keyboard, touch gestures,
  scroll; quality levels; automatic reconnect when the link drops (the session survives about 30 s of silence).
- **Connect screen and settings (new):** a NoMachine-style home page with Private and Shared modes (Shared leaves the
  laptop's screen and keyboard alone), a settings sheet saved on the laptop (blank screen, block laptop input, lock on
  disconnect, virtual monitor size, laptop pointer, laptop sound, frame rate and bitrate limits, heartbeat, idle and
  length limits, scale modes, Mac keys, text typing), in-session menu, toasts and a session timer. See
  `docs/ops/settings-guide.md`.
- **Host settings page (new):** a laptop-only page (password-protected) for what the laptop allows: modes, limits clients cannot
  exceed, sound, clipboard, typing, network, credentials (shown once, password asked again), notifications, start at login,
  and approve-each-connection (Accept or Deny on the laptop). Client choices now live on each device and stay inside those
  limits. See `docs/ops/settings-guide.md`.
- **App launcher (new):** an entry in the applications menu that starts the console, shows the top-bar icon and opens the host
  settings page; **Exit** (tray menu or the entry's right-click) stops the console and removes the icon.
- **Authenticator setup in the page (new):** scan a QR code or type the setup key, then confirm with one code from the app
  before the new secret replaces the old one.
- **Laptop top-bar indicator (new):** a GNOME extension shows whether a remote session runs (icon, timer, notices) and lets
  you disconnect it, lock the screen, open the host settings, switch remote use on the lock screen on or off, or start and
  stop the console. The client page is not offered on the laptop. See `docs/ops/README.md`.
- **Setup in the pages:** Host settings has a First steps card, a "Keyboard blocking" button (polkit password dialog on the laptop, once per boot) and ports that can be changed, with a clear refusal when another program already uses one. The session menu and End session buttons can be dragged to where you want them; the place is kept on that device.
- **Keyboard:** in a running session F8 opens the page's menu (Tab and Enter reach Disconnect there, Esc closes it); F8 is never sent to the laptop.
- **Laptop sound** to the tablet (Opus over WebRTC, off by default) and an **installable web app** (manifest, icons,
  pass-through service worker; needs a trusted https address or localhost).
- **Safety:** when a session ends (Disconnect, heartbeat loss, an owner limit, or the emergency chord while the keyboard is
  grabbed) the display is restored,
  the input grab released and, when the session's lock setting is on (default for Private, off for Shared, forceable by the
  owner), the screen locked; the grab is released after the lock. After a crash a 60 s dead-man restore timer restores the
  display and locks. A step that fails is reported as a warning on the page and in the tray, not as success.
- **Login:** Linux password (PAM) + authenticator code (or recovery code) + Remote Access Key or trusted browser; rate
  limits; server-side sessions that die with the emergency chord; remote access can be switched off from the CLI.
- **Outside the home network (guided):** `blackroom internet` (also offered as the last step of `blackroom setup`) asks whether
  your router can accept connections, then sets up a VPN (Tailscale), or direct access with a name and a real certificate, or
  with only a static IP and the console's own self-signed certificate (your explicit choice, with a fingerprint to compare). It
  checks the certificate (names, dates, key), shows the router forwards, saves only after a clean dry run and your "yes", and
  `--check` repeats the local checks any time. The settings (STUN, TURN, media ports, public name) live in `host.json`; a
  damaged `host.json` keeps the listeners on the laptop. See `docs/ops/internet-access.md`. The VPN route (Tailscale) was used
  once from a phone on mobile data (owner-reported); direct internet access was checked locally only.
- **Clipboard:** text only, both directions, explicit buttons, 256 KiB, rate limited, never logged.
- **Install and operate:** `.deb` (`docs/ops/build-deb.sh`), `blackroom setup` (QR for the authenticator, key, recovery
  codes, test login), `blackroom reset soft|security|full`, `blackroom repair`, `blackroom-grant-input`, a runbook.
- **Checks:** compatibility gate (`--allow-untested`), process figures in `/status` and the page's diagnostics panel,
  100-cycle and soak scripts, adversarial HTTP/TLS tests, a red-team report.

## Supported scope

A narrowly supported technical preview. The one authoritative support table (tested, not tested, unsupported) is at the top of
`docs/ops/compatibility-matrix.md`; in short:

- **Tested:** Ubuntu 26.04.1, GNOME Shell 50.1 on Wayland, PipeWire 1.6, NVIDIA RTX 3050 Ti with NVENC, the built-in panel as the
  only output, Chrome on an Android tablet, and the **private VPN route (Tailscale) from mobile data**, which is the recommended way
  to use it outside the home. Owner runs are owner-reported and kept without logs.
- **Tested once each:** Direct internet access with a static IP and a Let's Encrypt certificate on an own domain (owner-reported 2026-10-06, secure connection, mobile data), and with a static IP and the console's own certificate (owner-reported 2026-10-06: mobile data and Wi-Fi, WebRTC, 20+ minutes connected; the browser warns "not secure" because the certificate is self-signed).
- **Not tested:** certificate renewal, Safari and iOS (waived by the owner), Firefox, the OpenH264
  encoder in a live session, AMD/Intel GPUs, a soak of any length, laptop sleep or lid close, a reboot before anyone logs in,
  certificate renewal, start at login, real credential commands from the host page.
- **Unsupported:** X11, non-GNOME desktops, PipeWire 0.3, more than one output, a second Unix user.

## Known limits

- One display (the built-in panel; refuses to start if another output is connected), no file transfer, one controller at a time,
  GNOME on Wayland only, text-only clipboard.
- **Safety promises, exactly:** a normal stop (Disconnect, heartbeat loss, owner limit) restores the display and releases the
  grab, and locks the laptop only when the session's lock setting is on. The emergency chord (Left Ctrl + Left Shift + Left Alt +
  Esc, 2 s) works only while the keyboard grab is held, closes every remote login until you clear hostd's stop marker at the
  laptop, and does not lock by itself. Recovery steps for a stranger and for SSH:
  `docs/ops/emergency-recovery.md`. SSH is the backup route, never the only one.
- Input-device access is an operator step (`sudo blackroom-grant-input grant`, or the "Allow keyboard blocking" button, reset at
  reboot). "Come back after a restart" is the opt-in that installs a permanent udev rule for the built-in keyboard and touchpad: while it
  is on, any program running as you can read those devices. It also turns on automatic login (locked a few seconds later), needs
  an administrator's password once, and cannot help when a disk or BIOS password has to be typed at the laptop.
- hostd runs as you, without sandbox options, because the PAM check needs the setgid `unix_chkpwd` helper.
- The Remote Access Key and authenticator are not phishing-proof; a self-signed certificate relies on trust on first use.
- `style-src` still allows inline styles. See `docs/security/red-team-report.md` for the full list.
- Build and test notes: the project disk is ntfs3 and was 100% full once during this work (the debug `incremental`
  cache alone is ~7 GB); use `CARGO_INCREMENTAL=0` or clean it. Run one cargo command at a time.

## Live checks still owed by the owner

Laptop sleep and lid close with the VPN; certificate renewal; a real 30 minute session; optionally a relayed Tailscale path and
Direct on other routers. Owner-passed so far (2026-10-04 to 10-07): credential commands from the host page, the emergency chord,
Direct (self-signed on a static IP, and an own domain with Let's Encrypt), the tray icon, start at login, "Come back after a
restart" (on the build before the last review fixes; re-check on the final package) and the new-pages checks. A reboot with
nobody logged in stays unreachable until login (`Linger=no`) unless "Come back after a restart" is on. Rebuild and update with
`cargo build --release --workspace` and the `.deb`, then restart the console. Report what failed; a cell becomes PASS only after
the owner reports it (`docs/ops/compatibility-matrix.md`).

## Review status

Internal tests and one internal red-team pass, plus **one independent read-only review (GPT-6.1 Sol, 2026-10-04)**: 30
findings, 16 fixed, 3 disputed with a reason, the rest deferred (`docs/plans/plan-20261004-status.md`). The fixes were checked
by tests and the headless suites, not by a second review, and not on the real desktop. A separate read-only UI/UX review
(same date, 30 findings, 23 fixed) covered the client page, host page and tray; nothing was tried on a device.

**Release review (GPT-6.1 Sol, 2026-10-07, read-only, once):** 13 findings. Fixed: polkit now needs an administrator for both
root helpers; "off" removes the whole automatic-login block (and fails loudly if it cannot) and revokes the keyboard access it gave;
setup writes the lock first and rolls back; package removal stops if that cleanup fails or a display-restore timer is pending;
sign-in method, approval and listener changes and confirming an authenticator now need "Enable editing"; revoking a login stops
its video and sound; running password checks count against the failure limits; restart access also turns on start at login;
docs reconciled. Accepted: a Samsung monitor's serial string in the experiment evidence (a generic value, owner-approved
publication). The fixes were checked by tests and the headless suites, not by a second review.
