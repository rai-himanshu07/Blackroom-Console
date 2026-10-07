# Red-team report (hardware-free) — 2026-10-03

**Current follow-up review: 2026-10-07**, recorded at the end of this document. Earlier dated sections are historical,
not a list of defects still open. No review establishes that every possible bypass has been excluded.

Scope: what a hostile browser tab, a hostile page on another origin, a network peer, a malformed or oversized request, or a
non-root local user can do to the console without any display, input device or Shell. This is an internal review by the
author of the code with automated tests; it is **not** an independent penetration test. Items that need hardware, a
second Unix user, or a real phone are listed at the end as untested.

## What was run

| Check | Where | Result |
|---|---|---|
| Every route refuses an anonymous caller (5 GET, 6 POST) | `crates/blackroom-console/tests/adversarial.rs` | pass |
| 11 near-miss cookies (truncated, extended, upper-case, quoted, wrong name, empty); token in URL exchanged only when right and never echoed; cookie is `HttpOnly; SameSite=Strict; Path=/`, no `Domain`, `Secure` only on https | same | pass |
| 9 hostile `Origin` values (foreign, `null`, look-alike hosts, other port, `file://`, empty) on all 6 state-changing routes, and on the reads (`/status`, `/clipboard`, `/ice`, `/video`) | same | pass: all 403; the real origin is not refused |
| No `Access-Control-*` header on any response; `OPTIONS`/`TRACE`/`CONNECT`/`PUT`/`DELETE`/`PATCH` get 4xx | same | pass |
| Security headers on 200, 401, 404 and 405 responses: CSP, `nosniff`, `X-Frame-Options: DENY`, `no-referrer`, CORP and COOP `same-origin`, permissions policy, `Cache-Control: no-store`; HSTS only when asked | same | pass (after fixes below) |
| Body limits (64 KiB input, quality, WebRTC offer; 256 KiB clipboard) enforced before parsing | same | pass |
| 515 hostile bodies × 4 routes: deterministic mutations of valid input, truncations, 100 000-deep nesting, `1e999`, `-1`, `2^32`, wrong types, unknown event kinds, NUL bytes, 100 random binary blobs | same | pass after one fix (below): never a 5xx, never accepted |
| 20 000 generated input events: whatever `validate` accepts has a key code in 1..=0x2ff and not power/sleep/wake/suspend, a button in 0x110..=0x117, finite positions, scroll within ±1000 | same | pass |
| Path tricks (`//`, `/../`, `%73`, `%00`, case) never reach a handler anonymously | same | pass |
| Real process: runs as the invoking user, no setuid bit, private state directory | `docs/ops/adversarial-net-test.sh` | pass |
| TLS: 1.3 and 1.2 served; 1.1, 3DES-only and anonymous/export clients refused; HTTP/2 negotiated; plain http on the TLS port gets no page | same | pass |
| https cookie is `Secure`, plain-http cookie is not, no HSTS on a self-signed certificate, wrong token sets no cookie | same | pass |
| CSP script hashes equal SHA-256 of each page's inline script, computed independently in Python; no `unsafe-inline` for scripts | same | pass |
| A script injected into the page DOM is blocked by the CSP (real Chrome) | `docs/ops/headless-browser-test.mjs` | pass |
| A stalled half-request and a 64 KiB header-less blob do not stop other requests; a 100 KB request line is refused (414); 100 × 1 KB headers never give a 5xx | `adversarial-net-test.sh` | pass |
| Package: no setuid/setgid, nothing group- or world-writable, units run only `/usr/lib/blackroom` binaries, no `[Install]` section | `docs/ops/build-deb.sh` | pass |

## Added with the settings and web-app work (2026-10-03)

- Start options and the saved profile are validated on the laptop with hard ranges and `deny_unknown_fields`; invalid
  start options get 400, a body that is not a JSON object is refused (a bare array used to deserialize into a struct).
- The web-app files (`/manifest.webmanifest`, `/sw.js`, icons) are public next to the login page: static, no secrets, GET only,
  with the usual security headers (`install_files_are_public_static_and_still_hardened`). The service worker caches
  nothing and only answers failed page loads.
- Found by the browser walkthrough: the logout button's inline handler was silently blocked by the hash-based CSP, and
  typing in page text fields was swallowed by the global key handler. Both fixed; a unit test refuses inline handlers.
- A second local entry point, the session-bus service `org.blackroom.Console` for the top-bar indicator: it returns a small fixed set of
  facts about the session (a test pins the exact key set; no credentials), can end a running session and can answer a
  connection that waits for approval. It cannot start a session or change a setting. Any process of the owner's user can call it, like `pkill` or `systemctl --user stop`.
- **The host settings page** (`hostpage.rs`) is a third entry point: a loopback-only listener behind the laptop account's
  password (PAM helper, 5 failures lock it for a minute), a session cookie (HttpOnly, SameSite=Strict, 15 minutes idle, 1 hour
  at most), a loopback-only `Host` check against DNS rebinding, an `Origin` that must be the page's own on every change, and a
  script-only CSP. Credential changes ask for the password again and run the `blackroom` command with fixed arguments (the only
  browser text that reaches a command line is a device id of letters, digits and `_.-`); new secrets are returned once and not
  stored or logged. Tests: `tests/hostpage.rs` and `docs/ops/headless-hostpage-test.sh`. Residual risk: anything running as the
  owner on the laptop can reach the loopback port and try the password; the unlocked laptop is the owner's trust boundary.
- **Authenticator setup from the page**: needs the password; the new secret is generated and held in memory (zeroized) until
  one right code from the app is typed, so a failed scan never locks the owner out; five wrong codes or ten minutes drop it;
  it is shown on screen as a QR code and a text key and removed from the page when confirmed, cancelled or signed out; the
  confirming code's step is recorded so that code cannot be replayed to sign in. Tests: `totp.rs` (RFC vector, skew, replace,
  replay) and `tests/hostpage.rs`. A screenshot or shoulder-surfer during setup sees the key: it is the same exposure as the
  terminal's setup output.
- **Lock-screen access** is a switch (host page, tray) that enables the existing lock-screen extension. The host page asks for
  the password to turn it on and warns that locking then no longer ends a remote session; the tray switch needs a person at
  the unlocked laptop. **Sign-in method** (`host.json` `login`): saving "hostd" is refused while the login authority is not
  running. The original startup token fallback was removed in the 2026-10-07 follow-up below: configured three-factor login
  now fails closed when the authority is absent.
- **Limits are enforced by the laptop**, not the page: a start outside the owner's modes is refused (403), numbers are
  clamped, sound and text typing are refused when switched off (`tests/adversarial.rs`).
- **Approve each connection** (Ask): only the laptop's D-Bus service or the host page can answer; a stale or invented id
  does nothing; one request waits at a time; no answer in 30 seconds is a Deny; the device text shown is plain ASCII.
- New settings can lower the safety margin on purpose (Shared mode, no lock on disconnect, longer or unlimited timeouts):
  they are the owner's choices and the defaults stay safe. The emergency chord, the Stop button and heartbeat loss
  still end the session; with "lock on disconnect" off the screen is left unlocked afterwards.

## Findings and fixes

1. **Invalid clipboard text was answered 502 (a server error) instead of 400.** Found by the hostile-body test (a body of
   NUL bytes). The text check returned the "transfer failed" error class. Fixed: a separate `Invalid` error maps to 400.
2. **Scripts were allowed `'unsafe-inline'` in the CSP.** The page needs its one inline script, so the policy now lists
   the SHA-256 hash of each page's script instead (computed at start from the embedded pages). An injected script does not
   run. `style-src` still allows `'unsafe-inline'` (see residual risks).
3. **Responses were cacheable by default** apart from a few that set `no-store`. All responses now carry
   `Cache-Control: no-store` unless a handler sets its own.
4. **No cross-origin isolation headers.** `Cross-Origin-Resource-Policy: same-origin` and
   `Cross-Origin-Opener-Policy: same-origin` are now sent (the video stream cannot be embedded by another origin).
5. **Nothing stopped the console, the login authority or the input daemon from running as root.** All three now refuse.
   Root adds nothing they need (access to input nodes comes from ACLs for the user) and would enlarge the damage of a bug.

## Residual risks (known, accepted or open)

- **Online guessing of the Linux password** once the login is reachable from the internet. Per-source limits, the account
  lock and the two other factors bound it; the Remote Access Key and the authenticator are **not phishing-proof** (a
  look-alike page can relay them in real time). Recommended: Tailscale or another private network, not a public port.
- **Token mode** (the default of the bare binary: a 192-bit token in the URL) is for a trusted LAN only: the URL is a bearer
  secret and is printed to the log. `--public` refuses it. The installed unit uses the three-factor login.
- **Self-signed certificate:** a first visit relies on the owner accepting it (trust on first use). Use a real certificate
  for anything outside the home network.
- **`style-src 'unsafe-inline'`:** the pages style elements inline. Injected CSS cannot run script, but it can restyle. Moving
  the styles to hashed blocks is possible; not done.
- **Key filtering is a short deny-list** (power, sleep, wake, suspend). A logged-in client could still send other function
  keys (for example the airplane-mode key, which would cut the network and end its own session). The client is already fully
  trusted to type anything on the laptop; the list protects against accidents, not against a logged-in adversary.
- **The clipboard crosses a trust boundary by design:** text sent to the laptop and pasted into a focused application is
  typed input by another name.
- **Everything runs as the owner.** A process of the same Unix user can read the hostd sockets' directory and the owner's
  files; the design assumes the owner's account is not already compromised.
- **ICE/UDP media ports** (the `--ice-port-range` you open) were not fuzzed; they accept DTLS only from a negotiated peer.

## Independent review round (2026-10-04)

One read-only review by another model (30 findings, read against the code before triage). Security-relevant fixes: the video
stream and the WebRTC input channel stop when their login ends (they used to outlive a logout, revocation or expiry); the
login is checked again after the owner answers an approval; a damaged `host.json` now makes every connection ask the owner
instead of opening the defaults; the state directory must be private and owned by the user (no shared `/tmp` fallback);
rejected input text no longer reaches the debug log; the CLI no longer reports "no live sessions" when hostd merely failed to
answer. Open from that review (deferred, see `docs/plans/plan-20261004-status.md`): password attempts in flight at the same
time on the loopback host page, the blocking pool shared by media readers and control calls, clipboard worker threads after a
timeout, the order of a security reset, the lock-screen switch while remote handles exist, and the packaged emergency daemon
without its own lock action. None of the fixes was observed on the real desktop.

## Internet access set-up (2026-10-04)

`--public` used to check only that certificate paths were given. It now reads the certificate and key: the certificate must be in
date, cover the name clients type, belong to the key and not be self-signed (unless the owner chose the console's own
self-signed certificate for a bare IP, which is allowed deliberately, never for a name, and shown with its fingerprint); the key
file must be an owner-only regular file opened without following a link; a renewed pair is reloaded only when it passes the same
checks. A damaged `host.json` no longer silently drops saved public settings while the listeners stay open: the console starts
with its listeners on 127.0.0.1 only. The TURN secret file is read with the same rules and is never written to `host.json`, the
page or the logs. The host page refuses to save an internet mode the next start would refuse. Open: the host page still changes
network settings behind its session login alone (no fresh password); a self-signed certificate on a bare IP is
trust-on-first-use; nothing was tried against a real router, authority or mobile network.

## Not tested here (needs hardware or a person)

Real phone on mobile data, Safari and iOS behaviour, a second Unix user on the same laptop, the one-hour live soak, the
packaged install on a clean account, a CGNAT/symmetric-NAT client without TURN, and any test by someone other than the
author. The one independent review of the release was the read-only round above (2026-10-04); its fixes were not re-reviewed.

## Final security follow-up (2026-10-07)

Requested after the earlier release review and fixes. This is a source review with hardware-free tests, not an independent
external penetration test. It covers the shipped console's HTTP/control routes, login authority, host-page editing,
credential store, media authorization callbacks, startup exposure checks and packaged privileged helpers. The old
simulation gateway is not the installed entry point. GNOME isolation results already accepted with limits were not reopened.

### Confirmed findings and fixes

**High: a configured three-factor login could downgrade at startup.** In
[main.rs](../../crates/blackroom-console/src/main.rs), saved `login=hostd` plus missing authority sockets cleared `hostd_dir`
and served a token URL instead. Public-mode preflight refused that downgrade, but LAN/VPN startup did not. A token holder
could therefore use a weaker login than the owner selected; missing sockets alone did not give an anonymous caller access.

Fixed: startup refuses before creating network listeners when the selected authority is missing. It explains how to start
the authority or run local setup, and does not issue a token URL. Explicit token mode remains an opt-in development mode;
it is not a substitute chosen automatically for configured three-factor login.

The real-process regression in [internet-test.sh](../ops/internet-test.sh) failed on the old binary (token URL offered,
process stayed running) and passed after the change (prompt startup error, no token URL). The test uses a throwaway HOME,
missing authority path and headless mode; it never starts a desktop session.

**Medium: startup recovery advice removed the screen lock.** The runtime message still suggested
`loginctl unlock-session` after a black panel, despite the runbook warning not to do that. Following it remotely could
reveal the desktop to someone beside the laptop; it was unsafe recovery advice, not an anonymous remote-login exploit.
Fixed: the message warns against an SSH unlock and points to the recovery guide. The same real-process script confirms
that the unlock command is absent and the warning is present. No unlock was performed during this review.

### Boundaries checked

| Boundary | Evidence in this run |
|---|---|
| Anonymous requests, near-miss cookies, hostile origins, body limits and malformed input | Workspace console/adversarial tests passed |
| Host-page login, five-minute edit permission, protected saves, credential changes, authenticator confirmation and sign-out | Host-page Rust and isolated Chrome tests passed |
| Concurrent password reservations, independent factors, recovery-code limits, replay, revocation and emergency latch | Login-authority and credential-lifecycle tests passed |
| Owner-only credential files, no-follow reads, atomic writes and socket peer UID checks | Live-source inspection and existing store/authority tests passed |
| MJPEG and WebRTC/data-channel authorization | Source inspection of gates and generation checks, plus existing gate tests; no new end-to-end revoked WebRTC peer test |
| TLS, Secure/HttpOnly/SameSite cookies, CSP, stale/hostile HTTP clients | `adversarial-net-test.sh`: NET OK |
| Administrator requirement, GDM cleanup, lock-first setup and package-removal safeguards | Polkit/source inspection, host-page tests and `package-scripts-test.sh`: PACKAGE SCRIPTS OK |
| Dependency security and lint | `cargo clippy --workspace --all-targets -- -D warnings`, `cargo deny check`, `cargo audit`: passed |

The workspace test checkpoint was `cargo test --workspace --exclude gnome-session-agent`, then
`cargo test -p gnome-session-agent`; both passed, as did `cargo fmt --check`. Real-GNOME ignored tests stayed ignored.
Graph discovery was checked against live source; coverage metadata is best-effort and not proof of completeness.

### Residual risks and limits

- No further authentication bypass was confirmed in the reviewed paths. This is a bounded result, **not a guarantee of no
  bypass**. No fresh UDP/DTLS/SCTP fuzzing, exhaustive race exploration, sustained denial-of-service test or external
  penetration test was performed. Media revocation is polled and is not instantaneous.
- The owner account is the trust boundary. A compromised same-user program can reach the owner's files and sockets; this
  application does not isolate remote work from malicious programs already running as that user.
- **Automatic login is not a fail-closed boot privacy boundary.** The optional restart-access feature shows the desktop
  before a later lock and grants persistent input access. Leave it off when that trade-off is unacceptable. A failed boot
  lock on real hardware was not exercised here.
- **Lock-screen remote access changes GNOME's lock semantics.** While enabled, locking alone is not a remote-session kill
  switch. This does not remove the Linux password requirement, but same-user processes can also open remote handles.
- Self-signed certificates require independent fingerprint comparison, and the login factors are not phishing-resistant.
  Prefer a private VPN and a trusted certificate. Home/VPN labels do not enforce a firewall boundary.
- The emergency chord releases blocked input and closes remote login, but does not lock by itself; it is only available
  while the keyboard grab is active. Physical privacy/recovery was not newly observed in this run.
- A new clean-machine install/upgrade/remove, sleep/lid-close test, certificate renewal, long live soak, second Unix user
  and untested browsers remain outside this run. The installed console was not restarted or replaced.
