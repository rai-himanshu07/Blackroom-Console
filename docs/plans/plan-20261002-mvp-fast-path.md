# MVP fast path (2026-10-02)

Owner decision: stop validating, build the product. Goal: from the tablet, open a URL, see this laptop's desktop live while
the local panel is blank and the local keyboard and touchpad are grabbed, type, move and click, press Stop, and get the
local desktop back. Same session, owner's own use, home LAN. The roadmap order is overridden by this list.

## Accepted as-is (do not reopen)
FEAS-C eDP-only restore, FEAS-D remote input, FEAS-E built-in grab, FEAS-A replacement design, the integrated probe
(`exp06 --integrated-probe`, 2026-10-02 runs), the Mutter NULL-view fix (keep the virtual monitor until Stop).
Limits stay recorded in `docs/gnome/display-isolation.md`; none is a work item unless it breaks the MVP.

## Decisions (2026-10-02, from the plan docs and this machine)
- Doc 04 s41-s44 recommends WebRTC for video (codec chosen by capability, hardware encode preferred, software fallback
  acceptable) and a WebRTC data channel for input. That is also the smoothest option: about 3-8 Mbit/s at 1080p30 against
  25-50 for MJPEG, congestion control, hardware decode on tablets, every browser including Safari.
- This machine has what it needs without any apt install: GStreamer 1.28 with `pipewiresrc`, `nvh264enc` (RTX 3050 Ti NVENC),
  `openh264enc`, `rtph264pay`, `webrtcbin`, libnice, DTLS and SRTP, plus gstreamer-rs 0.25 (LGPL runtime, allowed).
- Clients: any browser on any machine or tablet. MJPEG works everywhere with no client code; WebRTC receive-only works on
  plain http on the LAN. Full keyboard capture (Ctrl+W, Alt+Tab, F11) needs Chromium in fullscreen over HTTPS (Keyboard Lock
  API), so HTTPS with a self-signed certificate comes with the polish milestone; touch devices get an on-screen modifier bar.
- Order: MJPEG first as the guaranteed baseline for the first real tablet demo (milestone 1 is done), then WebRTC H.264
  behind the same video interface. The session library hands out the PipeWire node id, so the consumer is pluggable.

## Milestones (each ends with a working demo, not a document)
1. **Video, MJPEG**: PipeWire frames to JPEG for an `<img>`. **Done** (see log).
2. **Session library** (`RemoteConsole { start, stop, input }`): lift the `exp06 --integrated-probe` flow out of the
   experiment crate: RemoteDesktop/EIS session, virtual monitor and capture, isolate, emergencyd grab, restore keeping the
   virtual monitor, Stop, lock on stop. Arms the `exp07 --keep-live-virtual` watchdog and releases the grab on socket EOF.
   Needs input primitives the EIS layer lacks: key down/up, button down/up, absolute pointer.
3. **Server and page**: one `blackroom-console` binary (axum): `/` page (embedded HTML+JS), `/video` (MJPEG with keepalive),
   `POST /input`, `POST /start`, `POST /stop`. One random token, HttpOnly cookie, constant-time compare. Fail closed: no
   heartbeat for 15 s runs Stop (the panel is blank, so a lost client must restore it).
4. **Tablet run** on the real session under the standing approval: see the desktop, type, move, click, Stop.
5. **WebRTC H.264**: `pipewiresrc` -> `nvh264enc` (fallback `openh264enc`) -> `webrtcbin`, signalling over the existing HTTP
   endpoints, MJPEG kept as the fallback; input over a data channel afterwards. Then polish: scaling, quality knobs,
   systemd user unit, HTTPS.

## Deferred until the MVP works
TOTP login and the separate hostd/gateway/agent process split (kept in the tree, off the MVP path), WebRTC, adversarial and
soak testing, HDMI and multi-display, packaging and install, compatibility matrix, emergency-daemon extras (chord audit),
independent reviews (one at release).

## Rules while building
Mini plans, smallest affected check per slice, commit and continue without asking. Live runs follow the standing approval
in `docs/ops/experiment-safety.md`; log one line per run below.

## Log
- 2026-10-02: plan adopted; workflow policy amended (AGENTS.md, WORKFLOW_CONFIG.md, copilot-instructions.md,
  experiment-safety.md "Standing approval").
- 2026-10-02 M1 done: `blackroom_gnome::mutter::video` (`stream_jpeg`, `JpegSlot`, rate limit, BGRx/RGBx) with 4 unit tests;
  `exp14_mjpeg_probe` on the throwaway headless Shell produced a correct 1920x1080 JPEG (about 45 KB, colours right) from a
  real Mutter ScreenCast stream; an idle desktop yields about 1 frame/s (damage driven), so the server must resend the last
  frame as a keepalive. New dependency `jpeg-encoder` (licence IJG allowed in deny.toml).
- 2026-10-02 M2 done: crate `blackroom-console` (`RemoteConsole` start/stop/input on one actor thread; `eis_support` moved in, absolute pointer bound; rolling 60 s dead-man restore watchdog refreshed every 20 s; grab lease 10 s renewed every 2 s; Stop = release grab, release held keys, stop EIS, restore keeping the virtual monitor, join video, ScreenCast Stop, verify/repair topology, lock). Headless proof (`crates/blackroom-console/tests/headless.rs` via `BR_BIN=<test bin> docs/ops/headless-repro.sh --ignored`): 2 start/stop rounds, JPEG frames, 9/9 input events accepted incl. absolute pointer, topology restored, Shell alive; silent client auto-stop. Not covered: real session, daemon grab, lock, watchdog firing.
- 2026-10-02 M3 done: axum server + embedded page (`/`, `/video`, `/status`, `/input`, `/start`, `/stop`), one random token, HttpOnly SameSite=Strict cookie, constant-time compare, same-origin check, 15 s heartbeat stop. `docs/ops/headless-console-test.sh` (via headless-repro.sh): login, MJPEG with keepalive, input, auto-stop, restart, manual stop all OK. Page script only syntax-checked (node), not exercised in a browser.
