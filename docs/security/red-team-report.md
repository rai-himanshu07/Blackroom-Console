# Red-team report (hardware-free) — 2026-10-03

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

## Not tested here (needs hardware or a person)

Real phone on mobile data, Safari and iOS behaviour, a second Unix user on the same laptop, the one-hour live soak, the
packaged install on a clean account, a CGNAT/symmetric-NAT client without TURN, and any test by someone other than the
author. An independent review is still planned for the release (plan chunk J).
