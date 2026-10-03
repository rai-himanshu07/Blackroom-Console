# Plan: feature completion after Phase 11 (2026-10-03)

**Tier:** compact (several crates, one release path). **Delivery:** MVP fast path; commit per chunk, one broad check at the end.

## Owner decisions (2026-10-03)
1. Access from outside the LAN: yes. 2. Packaging: a `.deb` is enough. 3. Clipboard: yes. 4. Keep the single `blackroom-console` binary
(no gateway/hostd/agent split; hostd stays the login authority).

## Non-goals
Audio, file transfer, power control (Doc 01 defers them), multi-display/HDMI isolation, AMD GPUs, multiple simultaneous controllers,
a separate gateway process, a rendezvous/relay service run by us, remote credential management from the browser (CLI only, Doc 03 s14).

## Decisions made in this plan (change any of them before testing)
- **Outside the LAN, three supported recipes, one code path.** The binary gains the pieces every recipe needs: a real TLS certificate from
  files (reloaded without restart), `Secure` cookies and security headers on https, configurable STUN/TURN with short-lived TURN
  credentials, a fixed UDP port range for media, and a `--public` interlock (refuses token/TOTP-only login, refuses plain http,
  requires a CA certificate). Recipes (docs only): **A. Tailscale/WireGuard (recommended: nothing exposed, `tailscale cert` gives a real
  certificate)**, **B. port-forward + dynamic DNS + Let's Encrypt (certbot)** with coturn for TURN, **C. VPN you already run**.
- **Clipboard is text only, explicit, and never silent.** Two buttons in the page ("Send clipboard to laptop", "Get laptop clipboard"),
  each a user gesture; 256 KiB limit; rate limited; off unless the session enabled it; content never logged (size and direction only).
  Uses Mutter's RemoteDesktop clipboard (`EnableClipboard`, `SetSelection`, `SelectionWrite`, `SelectionRead`).
- **Online guessing on the internet:** per-source limits stay; the per-account lock (which lets a stranger lock you out) is loosened
  and a valid trusted device bypasses it. Deviation from the Doc 03 numbers, recorded.
- **Dropped connections:** the page reconnects by itself (no new login while the session lives); the server's heartbeat default rises to
  30 s; the 60 s rolling restore watchdog is unchanged, so a lost client still ends in a restored, locked laptop.
- **.deb:** `cargo-deb`; installs binaries to `/usr/bin` and `/usr/lib/blackroom`, user units to `/usr/lib/systemd/user`, the PAM service
  file and polkit policy; nothing is enabled, remote access stays disabled until `blackroom setup`. Input-device access stays an
  operator step (`blackroom-grant-input`, root script; no udev rule, because a uaccess rule would let every app read your keyboard).

## Chunks (each ends with a check you can run)
- [x] **A. Internet-ready transport and hardening.** (done 2026-10-03; headless browser test passed with the CSP) `--tls-cert/--tls-key` (+ reload), `Secure` cookie on the https router only, HSTS,
  CSP, `X-Content-Type-Options`, `Referrer-Policy`, `X-Frame-Options`; `--ice-server`, `--turn-secret-file`, `/ice`; `--ice-port-range`;
  `--public`; account-lock tuning and device bypass; docs recipes. Check: server tests (headers, cookie flags, interlock), headless
  browser test still passes with the CSP.
- [x] **B. Reconnect.** (done 2026-10-03; headless browser test now drops the link for 6.5 s) Page auto-reconnects video and WebRTC with backoff; heartbeat default 30 s. Check: headless test with a dropped
  client that returns inside the window and one that does not.
- [x] **C. Clipboard.** (done 2026-10-03; `headless-clipboard-test.sh` CLIPBOARD OK both ways, browser test clicks the buttons) `blackroom-gnome` RemoteDesktop clipboard calls, `RemoteConsole::clipboard_{set,get}`, `POST/GET /clipboard`,
  page buttons. Check: headless Shell round trip both ways, limits, off-by-default.
- [x] **D. Packaging.** (done 2026-10-03; built with `docs/ops/build-deb.sh` and dpkg-deb instead of cargo-deb: no extra tool, same result, checks included) metadata, `/usr`-path units, maintainer scripts (no enable), `blackroom-grant-input`, install
  simulation (`apt-get install --simulate`, `dpkg-deb -c`). Check: package builds and lints clean; **you** install it on a clean user.
- [x] **E. First-run and repair.** (done 2026-10-03; `blackroom setup|reset|repair`, `docs/ops/runbook.md`) `blackroom setup` (enrol with terminal QR code, key, recovery codes, PAM check, start, URLs, public
  checklist), `blackroom reset {soft,security,full}`, `blackroom repair`, runbook. Check: wizard against a temp state directory.
- [ ] **F. Page quality.** Safari/iOS fallbacks (no Keyboard Lock, no fullscreen on iPhone), accessible labels, a paste-text box (uses
  clipboard), diagnostics chip. Check: headless Chrome; Safari/iOS are **your** test.
- [ ] **G. Reliability and metrics.** `/status` gains RSS, fd, thread, session counts; `docs/ops/cycle-test.sh` (N headless
  start/stop cycles with flat-resource assertion) and a soak script. Check: 100 headless cycles pass; **you** run the 1 h live soak.
- [ ] **H. Adversarial tests that need no hardware.** Browser/network/privilege cases (cookie flags, CSRF, origin, headers, TLS config,
  no setuid/no root), property tests on the JSON inputs, `docs/security/red-team-report.md`.
- [ ] **I. Compatibility gate.** Start refuses an untested GNOME/Mutter/PipeWire combination unless `--allow-untested`;
  `docs/ops/compatibility-matrix.md` with evidence per cell (this laptop PASS, everything else UNKNOWN, AMD UNKNOWN).
- [ ] **J. Release notes.** Known limits, supported scope, runbook, one independent review (you name the model).

## Owner live steps (only these need you)
1. After A and B: pick a recipe, follow `docs/ops/internet-access.md`, and log in from a phone on mobile data.
2. After C: copy text both ways over https.
3. After D/E: install the `.deb`, run `blackroom setup`, start everything from scratch.
4. After G: one hour live soak, and the Safari/iOS check from F.

## Risks
Exposing a login to the internet is the biggest risk change since the project began: the Linux password is guessed online (limits and the
three factors help, TOTP and key are not phishing-proof). CSP could break the page in a browser I cannot test (headless Chrome is the
proof; any regression shows up as a blank page, easy to see). Clipboard crosses a trust boundary by design (text pasted into a focused
laptop app is typed input by another name). A symmetric-NAT or CGNAT client needs TURN; without it media will not connect there.
