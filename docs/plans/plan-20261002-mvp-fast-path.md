# MVP fast path (2026-10-02)

Owner decision: stop validating, build the product. Goal: from the tablet, open a URL, see this laptop's desktop live while
the local panel is blank and the local keyboard and touchpad are grabbed, type, move and click, press Stop, and get the
local desktop back. Same session, owner's own use, home LAN. The roadmap order is overridden by this list.

## Accepted as-is (do not reopen)
FEAS-C eDP-only restore, FEAS-D remote input, FEAS-E built-in grab, FEAS-A replacement design, the integrated probe
(`exp06 --integrated-probe`, 2026-10-02 runs), the Mutter NULL-view fix (keep the virtual monitor until Stop).
Limits stay recorded in `docs/gnome/display-isolation.md`; none is a work item unless it breaks the MVP.

## Milestones (each ends with a working demo, not a document)
1. **Video**: PipeWire frames to JPEG, served as MJPEG (`multipart/x-mixed-replace`, shown by a plain `<img>`; no new
   client code, works through cookies). Latest-frame slot, old frames dropped. Developed against the throwaway headless
   Shell (`docs/ops/headless-repro.sh` pattern) so the real screen is not touched. Dependency: one pure-Rust JPEG
   encoder (cargo deny must pass).
2. **Session library**: lift the `exp06 --integrated-probe` flow into a library `RemoteConsole { start, stop, input }`:
   RemoteDesktop/EIS session, virtual monitor and capture, isolate, emergencyd grab, restore keeping the virtual monitor,
   Stop, lock. Arms the `exp07 --keep-live-virtual` watchdog and releases the grab on socket EOF, as today.
3. **Server**: one binary `blackroom-console` (axum, already a workspace dependency): `/` page (embedded HTML+JS), `/video`,
   `POST /input` (keys, pointer, buttons, wheel), `POST /start`, `POST /stop`. Auth: one random token printed at start, set as
   an HttpOnly cookie, constant-time compare. Fail closed: no heartbeat from the browser for 15 s runs Stop (the panel is
   blank, so a lost client must restore it).
4. **Tablet run**: the demo above on the real session, using the standing approval.
5. **Make it usable**: scaling for the tablet screen, frame rate and quality knobs, on-screen modifier keys, clean error
   page, systemd user unit. Then HTTPS (self-signed) before using it away from the home LAN.

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
