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
- **Laptop sound** to the tablet (Opus over WebRTC, off by default) and an **installable web app** (manifest, icons,
  pass-through service worker; needs a trusted https address or localhost).
- **Safety:** when a session ends (Disconnect, heartbeat loss, the emergency chord, an owner limit) the display is restored,
  the input grab released and, when the session's lock setting is on (default for Private, off for Shared, forceable by the
  owner), the screen locked; the grab is released after the lock. After a crash a 60 s dead-man restore timer restores the
  display and locks. A step that fails is reported as a warning on the page and in the tray, not as success.
- **Login:** Linux password (PAM) + authenticator code (or recovery code) + Remote Access Key or trusted browser; rate
  limits; server-side sessions that die with the emergency chord; remote access can be switched off from the CLI.
- **Outside the home network:** real certificate files (reloaded without restart), `Secure` cookies, STUN/TURN,
  a fixed media port range, and a `--public` interlock; recipes in `docs/ops/internet-access.md` (Tailscale recommended).
- **Clipboard:** text only, both directions, explicit buttons, 256 KiB, rate limited, never logged.
- **Install and operate:** `.deb` (`docs/ops/build-deb.sh`), `blackroom setup` (QR for the authenticator, key, recovery
  codes, test login), `blackroom reset soft|security|full`, `blackroom repair`, `blackroom-grant-input`, a runbook.
- **Checks:** compatibility gate (`--allow-untested`), process figures in `/status` and the page's diagnostics panel,
  100-cycle and soak scripts, adversarial HTTP/TLS tests, a red-team report.

## Supported scope

One host proven: Ubuntu 26.04.1, GNOME Shell 50.1 on Wayland, PipeWire 1.6, NVIDIA RTX 3050 Ti, the built-in panel as the
only output, Chrome on the tablet. Everything else is in `docs/ops/compatibility-matrix.md` as UNKNOWN or NO.

## Known limits

- One display (the built-in panel; refuses to start if another output is connected), no file transfer, one
  controller at a time, GNOME on Wayland only, text-only clipboard.
- Safari and iOS, Firefox, a phone on mobile data, a CGNAT client without TURN, AMD/Intel GPUs, the OpenH264 path in a
  live session, a second Unix user on the laptop, and a one-hour live soak have **not** been observed.
- Input-device access is an operator step (`sudo blackroom-grant-input grant`, reset at reboot): there is no udev rule on
  purpose.
- hostd runs as you, without sandbox options, because the PAM check needs the setgid `unix_chkpwd` helper.
- The Remote Access Key and authenticator are not phishing-proof; a self-signed certificate relies on trust on first use.
- `style-src` still allows inline styles. See `docs/security/red-team-report.md` for the full list.
- Build and test notes: the project disk is ntfs3 and was 100% full once during this work (the debug `incremental`
  cache alone is ~7 GB); use `CARGO_INCREMENTAL=0` or clean it. Run one cargo command at a time.

## Your live steps

0a. Host settings page: open it from the indicator, try a limit (for example Private only) and approval Ask from the tablet,
   restart under systemd, start at login, and a credential change with the real `blackroom` command.
0. New in this version (also: enable the top-bar indicator and try its menu on the real desktop): Shared mode on the real screen (check the pointer lands correctly with fractional scaling or an
   external monitor), Private mode with the blank/block switches each off and on, sound from the laptop's real default
   output, each setting on the tablet, installing the web app over a trusted https address.

1. Rebuild and update the installed pieces: `cargo build --release --workspace`, `docs/ops/install-security.sh --update`
   (or install the new `.deb`: `sudo apt install ./target/deb/blackroom-console_0.1.0-1_amd64.deb`), restart the console.
2. Clipboard over https: send text to the laptop and fetch the laptop's text from the tablet.
3. Internet: pick a recipe in `docs/ops/internet-access.md`, log in from a phone on mobile data.
4. Clean-install check: `.deb` on a clean user, `blackroom setup`, `sudo blackroom-grant-input grant`, start from scratch.
5. One-hour soak: `docs/ops/soak.sh 60` in a second terminal during a live session.
6. Safari/iOS: open the page, check login, video, touch, the diagnostics chip.
7. Report what failed; add a PASS cell to the matrix for anything that works.

## Review status

Internal tests and one internal red-team pass, plus **one independent read-only review (GPT-6.1 Sol, 2026-10-04)**: 30
findings, 16 fixed, 3 disputed with a reason, the rest deferred (`docs/plans/plan-20261004-status.md`). The fixes were checked
by tests and the headless suites, not by a second review, and not on the real desktop. A separate read-only UI/UX review
(same date, 30 findings, 23 fixed) covered the client page, host page and tray; nothing was tried on a device.
