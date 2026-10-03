# Blackroom Console 0.1.0 release notes

A tablet or phone browser sees and drives this GNOME laptop while the laptop's own panel is blank and its built-in
keyboard and touchpad are grabbed. Built for one owner's own laptop; not a product.

## What is in it

- **Video and input:** WebRTC H.264 (NVENC, OpenH264 fallback) with an MJPEG fallback; pointer, keyboard, touch gestures,
  scroll; quality levels; automatic reconnect when the link drops (the session survives about 30 s of silence).
- **Safety:** the display is restored and the screen locked on Stop, on heartbeat loss, on the emergency chord
  (Left Ctrl+Shift+Alt+Esc held 2 s), and by a 60 s dead-man restore timer; the physical input grab is released after the lock.
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

- One display (the built-in panel; refuses to start if another output is connected), no audio, no file transfer, one
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

1. Rebuild and update the installed pieces: `cargo build --release --workspace`, `docs/ops/install-security.sh --update`
   (or install the new `.deb`: `sudo apt install ./target/deb/blackroom-console_0.1.0-1_amd64.deb`), restart the console.
2. Clipboard over https: send text to the laptop and fetch the laptop's text from the tablet.
3. Internet: pick a recipe in `docs/ops/internet-access.md`, log in from a phone on mobile data.
4. Clean-install check: `.deb` on a clean user, `blackroom setup`, `sudo blackroom-grant-input grant`, start from scratch.
5. One-hour soak: `docs/ops/soak.sh 60` in a second terminal during a live session.
6. Safari/iOS: open the page, check login, video, touch, the diagnostics chip.
7. Report what failed; add a PASS cell to the matrix for anything that works.

## Review status

Internal tests and one internal red-team pass only. **One independent review of the release is still owed** (you name the
reviewer model); nothing here has been reviewed by anyone but its author.
