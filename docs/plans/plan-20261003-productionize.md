# Plan: productionize (NoMachine-style settings, audio, app feel) — 2026-10-03

Tier: compact (coupled multi-crate). Owner answers: both connect modes, laptop audio to the tablet, installable web app with a
connect screen, and the settings Resolution/scale, FPS and bitrate caps, cursor, lock on disconnect, keyboard options,
session timeouts, settings stored on the laptop.

## Non-goals
Native desktop client (the web app is the "connect window"; Tauri is a later follow-up), microphone to laptop, multi-host
launcher, multi-monitor selection UI, file transfer, remote power control, muting the laptop speakers.

## Decisions
- **Two independent switches, two presets.** `blank_panel` (virtual monitor + hide the physical panel + restore watchdog) and
  `block_local_input` (grab the built-in keyboard and touchpad). **Private** = both on (today). **Shared** = both off:
  capture the existing monitor with `RecordMonitor`, no topology change, local screen and input keep working. The
  mixed cases (shared + block input, private without blocking) are reachable from the settings sheet.
- **Safety stays attached to what is changed:** the watchdog and display restore exist only when `blank_panel`; the input
  grab lease only when `block_local_input`; heartbeat loss, idle timeout and the emergency chord end any session.
- **`lock_on_stop`** (default: on for private, off for shared). The dead-man restore after a crash still locks.
- **Settings profile lives on the laptop** (`~/.local/share/blackroom-console/profile.json`, owner-only, atomic write,
  validated ranges, unknown keys dropped). `GET/POST /settings`; Start uses it plus optional per-start overrides.
- **Audio:** PipeWire default-sink monitor (`stream.capture.sink`) -> Opus -> a second RTP stream in the existing
  `webrtcbin`; only when the browser offers an audio m-line and the setting is on. A failing audio branch must not end video.
- **Soft-keyboard text** can be injected as keysyms (`NotifyKeyboardKeysym`), which is layout independent; the key-code path
  stays the default.
- **App shell:** `page.html` is split into `index.html`, `app.css`, `app.js` (script-src `'self'`, no inline script); login
  pages keep hashed inline scripts. Manifest, icons and a pass-through service worker are public (no secrets in them);
  installing needs a trusted https origin (Tailscale/Let's Encrypt) or localhost.

## Chunks
- [x] **K. Session options and modes.** (done 2026-10-03; `headless-modes-test.sh` MODES OK) `SessionOptions` (mode, blank_panel, block_local_input, lock_on_stop, resolution,
  heartbeat, idle and max minutes), `POST /start` body, shared-mode capture and pointer mapping, status shows the mode.
  Check: headless Shell session in shared mode (video frames, no topology change, input accepted), private unchanged.
- [x] **L. Settings profile.** (done 2026-10-03; `profile.rs`, `/settings`, modes test saves and follows a profile) `profile.rs`, `GET/POST /settings`, defaults and validation. Check: unit and adversarial tests.
- [x] **N. Display, rate and cursor options.** (done 2026-10-03; cursor-mode, fps and bitrate caps, `/tuning`; headless modes test) fps cap, bitrate cap, embedded cursor, live apply. Check: headless.
- [x] **M. Audio.** (done 2026-10-03; Opus branch, `/audio`, `--audio-sink`; headless Chrome decodes a 440 Hz tone from a silent test sink) Opus branch, offer parsing, setting, page control. Check: headless Chrome receives audio from a test tone.
- [x] **O. Input options.** (done 2026-10-03 server side: keysym text typing proven in the headless Shell; Mac key mapping and text mode switch land in the page, chunk P) keysym text injection, Mac key mapping. Check: unit tests, headless browser.
- [x] **P. App shell and UI.** (done 2026-10-03; connect screen, menu, settings sheet, toasts, timer, scale modes; headless Chrome walkthrough passes; fixed the logout button blocked by the CSP) connect screen, in-session menu, settings sheet, toasts, timer, scale modes (fit, stretch,
  1:1 with follow-cursor). Check: headless Chrome walkthrough.
- [x] **Q. Installable web app.** manifest, icons, service worker, offline page, install button. Check: headless Chrome
  manifest and worker checks, curl.
- [ ] **R. Docs, gate, package, notes.** settings guide, runbook, release notes, compatibility matrix, workspace gate, `.deb`.

## Owner live steps (new)
Shared mode on the real screen; private mode with blank panel off/on; sound from the laptop to the tablet; install the web
app from a Tailscale https address; each setting on the tablet.

## Risks
Shared mode maps pointer coordinates to the logical monitor: unproven live with fractional scaling or an external monitor.
Audio capture follows the default sink and may be silent when nothing plays. A web-app install fails on a self-signed
certificate. The page rewrite can regress touch gestures: the existing headless walkthrough must keep passing.
