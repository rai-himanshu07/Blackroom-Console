# Final implementation plan: from working MVP to a first public technical preview (2026-10-04)

Tier: governed only for the release gate (W7); everything else is mini or compact. Delivery follows the MVP fast path in
`AGENTS.md`. Written for a fresh implementation session: every step names its check and who runs it. Reviewed once by
GPT-6.1 Sol (plan and screenshots of the real UI, read-only, 2026-10-04); this version already contains its changes.
Do not call the reviewer again before the single release review.

## Where things stand
- MVP console, host settings page, tray indicator, `blackroom` CLI, `.deb`, internet access (`blackroom internet`: VPN or direct,
  certificate checks, access switch on the host page, renewal reminders) are built and gated offline.
- Owner-verified live (owner-reported, no logs): Shared/Private on the real screen, sound, tray, launcher, authenticator QR,
  clean `.deb` install, Ask, clipboard over https, **mobile data through Tailscale (direct path, https name, reconnect after an
  idle tab)**. Safari/iOS waived and the one-hour soak assumed by the owner: both stay **untested** in anything public.
- Details: `docs/plans/plan-20261004-status.md`, `docs/ops/compatibility-matrix.md`.
- Review screenshots (client connect/settings/session, host page, access card) were taken from the real pages against a throwaway
  headless console: `BR_SHOT_DIR=<dir>` on `docs/ops/headless-hostpage-test.sh` and on the headless browser suite
  (`BR_BIN=docs/ops/headless-browser-test.sh docs/ops/headless-repro.sh`).

## Position and non-goals
First release = **a narrowly supported technical preview** for the tested configuration (Ubuntu 26.04, GNOME 50, NVIDIA/NVENC,
Chrome on Android, Tailscale), with unsupported and untested combinations listed. Not: saved servers, a native client, a
project-run relay or rendezvous server, certificate auto-renewal, router changes, multiple users, IPv6 listeners, certifying
NetBird/Headscale, an APT repository, new live experiments beyond W3.

## W1. Say only what is true (docs, first, small)
1. One authoritative support table (tested / untested / unsupported) shared by the README, the matrix and the release notes;
   reconcile the documents that disagree (observed tests, self-signed Direct access, emergency-daemon installation).
2. Reconcile the safety promises: the runbook says crash recovery "locks the screen" and tells the owner to unlock a black panel;
   the client settings say the emergency chord "always" ends the session. State the exact chord, prerequisites, what is and is not
   guaranteed, and a recovery procedure that does not casually remove privacy protection.
3. A short emergency recovery page for a stranger (stuck Private session, network lost, console crashed): what happens by itself,
   what to press, what to run over SSH as the backup (SSH cannot be the only escape route).
4. All three access modes are offered and the user chooses: Home only, **Private VPN (recommended)** and Direct. The VPN comes
   first and is marked recommended in the README, the wizard and the host page; Direct shows its risks and asks "can your router
   accept incoming connections?" first, and stays listed as untested in the support table until a real router, authority and
   mobile-network run exists. Safari/iOS and the soak are written as untested.

## W2. Recovery and revocation before distribution (code, compact)
Consequence-based triage of the deferred code findings (the original texts are recovered from the review; each is read against
the live code first):
| Finding | Consequence if left | Action | Check |
|---|---|---|---|
| R8 packaged emergencyd has neither `--state-dir` (recovery latch) nor `--lock-on-emergency` | a hung console cannot lock or revoke durably after the chord | wire the latch in the packaged unit; enable `--lock-on-emergency` only after one supervised live run, else document release-without-lock | unit test of the unit file; owner live run (W3) |
| R10 `repair --fix` stops `blackroom-console-wd-*` units | cancels the last recovery timer | keep pending recovery units until recovery is verified or the operator acknowledges | setup tests |
| R25 package removal/upgrade during a Private session | stranded blank screen, helper removed | `prerm`/`postinst` refuse or shut the session down with restore first | package script test in a throwaway root; clean install/upgrade/remove check |
| R15 blocking hostd check on async workers; R16 unbounded MJPEG streams | Stop and control starve | bounded blocking admission; stream cap (about 4) | unit tests |
| R17 concurrent host password attempts | throttle bypass | reserve in-flight capacity before PAM | `tests/hostpage.rs` |
| R18 clipboard threads leak | descriptors and memory | bound in-flight transfers or close the descriptor on timeout | unit test |
| R20 `reset` leaves partial credentials | half-reset login | disable and revoke first, stay disabled unless everything completes | setup tests |
| R23 lock screen "Off" while handles remain | UI says off, access continues | show "pending off" | host page test |
| R24 tray Exit treats any error as stopped | indicator hides a live console | tell absence from uncertainty | indicator logic test |
| R26 keyboard forwarded while a button has focus | cannot reach Disconnect by keyboard | suspend forwarding while local UI owns focus | headless browser suite |
Then verify the **assembled package**'s recovery outcomes (no new mechanism experiments): browser/network loss, console crash,
emergency chord (input released, panel restored, lock outcome reported, latch handled), missing input permission after reboot.

## W3. Owner live checks (the agent cannot do these)
Each has a pass line; "observed" is only what the owner saw.
1. Real credential commands from the host page, and the next login still works.
2. Start at login and restart under systemd; reboot with nobody logged in (expected: nothing reachable until login; written down).
3. Laptop sleep/lid close with Tailscale: reachable after resume, what a running session does.
4. R8 supervised run (chord with the latch and lock wired).
5. Certificate renewal with the same `sudo tailscale cert` command: picked up within 6 h, banner and tray line clear.
6. A short real session soak (about 30 minutes, one real sitting), instead of an assumed hour.
7. Optional: Direct mode on a real router; a relayed Tailscale path. Without a Direct run it stays "untested" in the support table.

## W4. UI and accessibility (compact; screenshot review findings plus the deferred U items)
| Priority | Step | Source |
|---|---|---|
| High | Session menu shows **isolation** (screen blanked, input blocked, lock requested) apart from connection quality | design review |
| High | A compact labelled End session control stays visible in Private use with the menu closed | design review |
| High | Failed Disconnect keeps a persistent warning with Retry and the recovery instructions, not an 8 s toast | design review (and U9/U10) |
| High | Chord wording on the settings sheet exact and linked to W1.3 | design review |
| High | Host Home/VPN text must not claim "nothing is exposed": say what is configured and show the actual listener interfaces | design review |
| Medium | Host access card: label "Pending settings" vs "Currently running"; restart required stays prominent | design review |
| Medium | VPN card: working guide link and copyable host-specific commands; Direct self-signed: fingerprint steps before credentials | design review |
| Medium | Phone settings: make the Session tab discoverable; session menu: group clipboard/diagnostics/tuning behind sections | design review |
| Medium | Host page: section navigation and setup ordering (credentials are buried); devices as rows with Forget buttons | design review |
| Medium | Measure contrast of muted labels and field borders before changing the palette | design review |
| Low | Clip the pointer marker to the video and hide it under overlays | design review |
| Yes | U11 action-level errors, U14 in-flight guards, U15 minimal unsaved-change prompt | deferred U |
| Yes | U18 focus containment, U19 complete tab keyboard navigation | deferred U |
| Basic | U27 token-error page that says how to recover; U29 owner-limit text next to the clamped setting | deferred U |
Check: the headless host page and browser suites gain a check per item; capture the missing states for review and README
(sign-in with the three factors, Private session with the menu closed, isolation starting/confirmed/failed, network loss and
failed Disconnect, owner restrictions, host setup completion and a pending exposure change). README captures use a neutral
hostname and show the session safety state.

## W5. Small code items from the Tailscale run
1. Wizard reads the Tailscale name from `tailscale status --json`; refuses a short name that the certificate does not cover.
2. `blackroom doctor`/`repair` says when an older `blackroom` earlier in `PATH` hides the installed one, and when the user unit
   overrides the plain-http address to `0.0.0.0` while https is on.
3. Host page: switching to or from **Direct** asks for the laptop password (same path as the lock-screen switch).
4. Wizard menu and host page selector: VPN first with a "recommended" label and a one-line reason, Home, then Direct with its
   risk text; the setup step 9 default stays "home network only".
Check: wizard and repair unit tests, `tests/hostpage.rs`, the headless host page suite.

## W6. Open-source readiness
- **Licensing:** the workspace is GPL-3.0-or-later (see below for the root licence); `cargo deny` does not settle distribution of system GStreamer plugins, OpenH264,
  NVIDIA components or H.264 patent terms. Inventory what is bundled versus loaded, keep the notices, state it in the README. No
  legal clearance is claimed.
- **Publication privacy (owner decision 2026-10-04: publish everything, including experiment evidence, as long as no secret
  goes out):** a first pattern scan of the tracked files and all 289 commits found no private keys, access tokens, console URLs
  with tokens, TURN secrets or assigned password literals (no scanner was installed; this was `git grep`/`git log -G`). Still
  to do: run a real scanner (gitleaks or trufflehog) over the tree and the history before the first push, and add one to CI.
  Not secrets but personal, published unless the owner scrubs them: the Linux username (34 files, 17 in the evidence), a
  private LAN address (5 files), and the commit author name and gmail address on all commits (consider a noreply address
  before the first push; rewriting history is destructive and needs the owner's explicit go-ahead).
- **Licence:** `LICENSE` (GPL-3.0) is in the root and the workspace declares `GPL-3.0-or-later`: consistent. Still to do: the
  inventory of loaded and bundled third-party parts (GStreamer plugins, OpenH264, NVIDIA components, H.264 terms) in the README.
- **Exposure and privacy statement:** installed listeners, what contacts the network (STUN, the VPN), what the logs contain,
  telemetry (none, stated).
- **Lifecycle:** manual update and removal steps; the homepage placeholder in `docs/ops/build-deb.sh`.
- **Files:** concise README with screenshots, `SECURITY.md` (how to report), build instructions, `CONTRIBUTING.md`, a changelog,
  focused CI (fmt, clippy, tests, `cargo deny`, `cargo audit`, the offline headless suites that need no desktop).
- **Authenticity:** a signed checksum manifest with verification instructions and a stable signing identity.

## W7. Release gate (governed, once)
Build the distributable **last**, after the release-critical changes. Clean install, upgrade and removal on a clean machine;
full gate (`fmt`, `clippy -D warnings`, `cargo test --workspace`, `cargo deny check`, `cargo audit`, the indicator logic test, the
headless suites); the one independent read-only review; release notes with the support table and limitations; tag; checksums.

## W8. Secret cleanup before anything is published (after W7, before the first push; owner-gated)
Nothing is pushed or made public until all of this is done and the owner has said go.
1. Real scanners over the working tree **and the whole history**: gitleaks and trufflehog (`--no-update`, local only), plus the
   pattern scan already run (clean on 2026-10-04: no keys, tokens, token URLs, TURN secrets, password literals).
2. Read the experiment evidence and the plans by hand for anything a scanner cannot know: bearer or one-time URLs, recovery
   codes, Remote Access Keys, TOTP secrets, authenticator QR data, Tailscale names or auth keys, session ids, recordings.
3. Decide with the owner what to scrub: Linux username (34 files), private LAN address (5 files), the author name and Gmail
   address on all commits (offer the GitHub noreply address). A history rewrite is destructive: first a backup bundle
   (`git bundle create`) and a tag, only with the owner's explicit yes; there is no remote yet.
4. Anything found is treated as leaked: rotate or revoke it (`blackroom rotate-key`, a new authenticator, a new Tailscale key),
   then remove it from the history.
5. Re-run the scanners after any rewrite; add gitleaks to CI so it stays clean; confirm `.gitignore` covers `target/`, local
   settings and index folders; then push.

## Deferred on purpose (listed, not planned)
Phase 5 steps 7-10 and 13-14 (needs the HDMI hardware), the Phase 9 and 10 formal gates, an independent penetration test, GNOME
49 and 51, AMD/Intel GPUs (no VA-API), more than one monitor, other distributions, Firefox, IPv6 listeners, NetBird/Headscale
certification, issue templates, an update service.

## Order
W1 -> W2 and W4 and W5 (parallel sessions are fine; one cargo command at a time) -> W3 (items 1-4 as soon as W2 lands) -> W6 ->
W7 -> W8 (secret cleanup) -> publish. A new session starts with `docs/plans/plan-20261004-status.md`, this file and
`docs/HANDOFF.md`.

## Open decisions for the owner
- Decided: publish everything, evidence included, secrets excluded; licence is GPL-3.0 as in the root.
- Decided: all three access modes are offered, the VPN recommended (W1.4, W5.4).
- Still open: repository name and version scheme; whether to publish under a noreply commit identity (W8.3).
