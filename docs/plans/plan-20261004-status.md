# Status and review round (2026-10-04)

Tier: mini (status note). Delivery: MVP fast path.

## Where the project stands
The MVP console is built end to end: client page (Private and Shared sessions, WebRTC H.264 + Opus, MJPEG fallback, clipboard,
per-device settings), owner limits and approve-each-connection, the laptop-only host page, the `blackroom` credential CLI, the
top-bar indicator, the lock-screen switch, the launcher and the `.deb`. Roadmap Phases 0-8 are done; Phase 5 steps 7-10 and 13-14
stay deferred; the Phase 9 and 10 formal gates are open; Phases 11-20 are largely superseded by the console (annotated in the
roadmap). Release notes: `docs/RELEASE_NOTES.md`.

## One independent review (GPT-6.1 Sol, once, read-only)
30 findings (8 High, 20 Medium, 2 Low), all read against the code before triage.
- **Fixed:** the video stream and the WebRTC input channel now stop when their login ends (R1); the login is re-checked after the
  owner answers an approval (R5); a negotiation cannot attach to a later session (R6); input queued for the actor is capped (R7);
  the restore timer stays when the lock failed (R4); the restore helper locks before its verification poll and fails when the
  lock fails (R11); a private state directory is required, no `/tmp` fallback (R12); a damaged `host.json` makes every
  connection ask the owner (R13); unique temp file per `host.json` save (R14); rejected input text no longer reaches the debug
  log (R19); authenticator note (R21); the CLI no longer says "no live sessions" when hostd merely did not answer (R22);
  compatibility wording (R27); a cancelled touch never clicks (R28); device reset keeps the laptop's defaults (R29); host
  sign-in recovers from a network error (R30).
- **Disagreed, with reason:** R2 (startup recovery runs about 5 s after a crash, far beyond the roughly 50 ms Mutter teardown that
  crashed in run 19), R3 (the retry that drops `--keep-live-virtual` is deliberate and commented), R9 (the marker is written
  before the display is touched and the 60 s timers remain the backstop).
- **Deferred:** R8 (`--lock-on-emergency` in the packaged unit: a lock during teardown is an unobserved composition, needs a live
  run first), R10, R15-R18 (need an authenticated or same-user caller), R20, R23-R26 (documented behaviour or product choice).
- Not covered by a test: the data-channel gate itself (only the gate type and the unchanged data-channel path in headless Chrome),
  the negotiation check (R6), `pointercancel` (R28) and the teardown order against a real lock (R4: only the decision function).

## One UI/UX review (GPT-6.1 Sol, once, read-only, on request)
30 findings (U1-U30) on the client page, host page, tray and docs; none was run on a device.
- **Fixed (23):** a failed restore, lock or input release is shown as a warning on the page and in the tray instead of being
  left out (U1); the owner's forced lock choice survives a preset click (U2); a hand-set limit with no dropdown entry shows as
  itself and an unrelated save keeps it (U3); Restart sends `confirm` only after the owner agreed (U4); the tray says
  "starting" until isolation is done and describes screen and input separately (U5); the tray tells a silent console from an
  absent one and Exit keeps the icon when unsure (U6); lock-screen off wording and a note under the tray switch (U7, U28);
  runbook, release notes and README no longer promise an unconditional lock (U8); Disconnect failure and stale status are
  reported (U9, U10); the host page shows the damaged-`host.json` warning (U12); secrets and passwords are cleared on sign-out
  and the authenticator QR expires on screen (U13); browser zoom allowed and 44 px targets (U16, U17); gesture hint, short
  two-finger scroll, pointer marker, right-click flag and clipboard text cleared between sessions (U20-U24); login page label,
  one-time-code autocomplete and a real Forget button (U25, U26); settings notes tell live from next-connect (U30).
- **Deferred:** U11 (action-level error handling on every host call), U14 (in-flight guards), U15 (unsaved-change prompts),
  U18 and U19 (focus trap, tab roles), U27 (branded token error page), U29 (owner-limit text inside the sheet).
- **Checked by:** `indicator-logic-test.mjs` (new notices), the headless indicator, browser and host page suites (three new host
  page checks). **Not exercised:** gestures (U21, U23), the pointer marker (U22) and touch target sizes on a real tablet.
- U28 differs from the finding: the tray shows the lock-screen consequence in an always-visible line under the switch instead
  of a confirmation dialog.

## Verified by the owner on 2026-10-04 (owner-reported, no logs kept)
Shared mode on the real screen incl. pointer mapping, Private blank and block toggles, laptop sound from the real output, tray
lock-screen switch, tray Exit, the applications-menu launcher, an authenticator app scanning the QR, a clean `.deb` install,
and the newest changes (tray notices, Disconnect with the network off, tablet gestures and targets, logout/revoke ending a live
stream, start from the user unit). Compatibility-matrix rows updated accordingly. Later the same day: Ask on the real desktop and
clipboard over https also verified by the owner; Safari/iOS waived by the owner (browser-based, expected to work, not tested); the
one-hour soak assumed fine by the owner, not run.

## Internet access rework (owner request, 2026-10-04)
The owner pointed out that the recipes did not serve a static-IP user and that outside access was not part of `blackroom setup`.
One design review by GPT-6.1 Sol (read-only) shaped it. Built: `blackroom internet` (guided; `--check`), offered as step 9 of
`setup`; the console binary owns validation (`--check-internet`, `--apply-internet [--dry-run]`) and start-up uses the same
preflight; `host.json` gained `stun`, `turn`, `turn_secret_file`, `turn_ttl_secs`, `ice_ports`, `public_name`, `public_cert`;
real certificate checks (`certcheck.rs`: names, dates, key match, fingerprint; x509-parser is already in the tree through rcgen);
the TURN secret is read as an owner-only regular file without following links; a renewed certificate is reloaded only if valid;
a damaged `host.json` keeps the listeners on 127.0.0.1; the host page shows the state read-only and refuses to save an internet
mode the next start would refuse; `docs/ops/internet-access.md` rewritten around who can reach whom.
- **Owner decisions that differ from the reviewer:** a bare static IP may use the console's own self-signed certificate in internet
  mode (explicit choice, fingerprint shown, no HSTS, trust-on-first-use notice); the reviewer advised insisting on a name.
- **Left out on purpose:** automatic CGNAT detection, certificate issuance, router or firewall changes, TURN by default, a fresh
  password for exposure changes on the host page (it refuses unsafe saves instead), IPv6 listeners, a `turns:` relay.
- **Tested:** unit tests (host.json validation, certificate checks, preflight, patching, the wizard against a fake console, start-up
  merge), `docs/ops/internet-test.sh` (real binaries, throwaway HOME, real start-up with a damaged file and with a refused public
  mode), the host page tests. **Not tested:** a real router, a real certificate authority, Tailscale, a phone on mobile data.

## Still the owner's
Real PAM and credential commands from the host page, and start at login and restart under systemd.

Owner-verified 2026-10-04 (reported, no logs): login from mobile data through Tailscale with the `tailscale cert` https name
(direct path, clipboard, authentication, reconnect after an idle tab). Not covered: a relayed path, laptop sleep or lid close, a reboot
before anyone logs in, certificate renewal.
