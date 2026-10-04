# Compatibility matrix

The console checks three things at start and refuses an untested combination unless `--allow-untested` is given: the GNOME
Shell **major** version (tested: 50), the PipeWire **major** version (tested: 1) and a Wayland session. `blackroom-console
--check-compat` prints this host's verdict as JSON. The code that decides is `crates/blackroom-console/src/compat.rs`; the
tested list lives there. **Every other row below (distribution, GPU, encoder, minor versions, browsers) is documentation
only: the gate does not look at it**, so an UNKNOWN row there is not refused. The single-output rule is enforced
separately when a session starts.

Cells: **PASS** = run end to end with evidence, **UNKNOWN** = never run (refused only where the gate above checks it),
**NO** = cannot work (an X11 session or a missing GNOME Shell is refused always; the other NO rows are not checked).

| Cell | Status | Evidence |
|---|---|---|
| Ubuntu 26.04.1 LTS, GNOME Shell 50.1, Wayland, PipeWire 1.6, NVIDIA RTX 3050 Ti (driver 595.91.07, NVENC), Dell G15 5511, built-in 1920x1080 panel | **PASS** | live integrated runs (`docs/plans/plan-20261002-mvp-fast-path.md` log), 100 headless cycles with clipboard, headless Chrome WebRTC test, owner's tablet login |
| Same host, OpenH264 software encoder (no NVENC) | UNKNOWN | `webrtc.rs` falls back to it when NVENC fails its probe, but a session has only ever run on NVENC here |
| GNOME Shell 49 or older, Mutter 49 or older | UNKNOWN | RemoteDesktop/ScreenCast clipboard and virtual-monitor behaviour differ between releases |
| GNOME Shell 51 or newer | UNKNOWN | the gate refuses until someone runs the headless suite and a live session on it |
| PipeWire 0.3.x | NO | too old for the capture path (gate: unsupported) |
| PipeWire 2.x | UNKNOWN | |
| Other Linux distributions with GNOME 50 (Fedora, Arch) | UNKNOWN | the package is a `.deb`; the binaries should run but nothing else was tried |
| AMD or Intel GPU | UNKNOWN | only the OpenH264 path could apply; NVENC is NVIDIA-only; VA-API is not implemented |
| X11 session | NO | a Wayland session is required (gate: unsupported when `XDG_SESSION_TYPE` says otherwise) |
| KDE, XFCE, other desktops | NO | the console drives Mutter's D-Bus interfaces; there is no GNOME Shell to find |
| More than one monitor, HDMI attached | UNKNOWN | the console refuses to start unless the built-in panel is the only output |
| Browser: Chrome/Chromium on Linux, Android Chrome | PASS (Chrome headless on this laptop); tablet Chrome by the owner | headless browser test; owner's tablet |
| Shared mode (panel and input left alone) on the real screen | PASS (owner-reported 2026-10-04) | owner ran it on the real screen incl. pointer mapping on the built-in panel; no log or recording kept; fractional scaling and an external monitor are still not covered |
| Laptop sound to the browser | PASS (owner-reported 2026-10-04) | owner heard the laptop's real output on the client; headless also decodes a 440 Hz tone |
| Laptop top-bar indicator on the real desktop | PASS (owner-reported 2026-10-04) | owner checked the real top bar, its notices and menu; headless Shell test also passes |
| Host settings page with the real password check and the real `blackroom` commands | UNKNOWN | headless Chrome with a stand-in password check and stand-in command; the real PAM helper, credential verbs, start-at-login and restart-by-systemd were not run from the page |
| Approve each connection (Ask) on the real desktop | PASS (owner-reported 2026-10-04) | owner approved and denied a real request on the real desktop; headless Shell also covers Accept, Deny and no answer |
| Tray switch for the lock-screen extension and the host page's lock-screen and sign-in settings on the real desktop | UNKNOWN | the tray switch was checked on the real desktop (owner-reported 2026-10-04); the host page's lock-screen and sign-in settings still used stand-in commands |
| Authenticator setup from the host page with real authenticator apps | PASS (owner-reported 2026-10-04) | owner scanned the QR code with an authenticator app and confirmed with a code; the algorithm is also checked against RFC 6238 |
| Applications-menu launcher and tray **Exit** on the real desktop | PASS (owner-reported 2026-10-04) | owner used the real menu entry and the real Exit click |
| Installing the page as an app | UNKNOWN | headless Chrome finds no installability problem; a tablet over a trusted https address was not tried |
| Browser: Safari, iOS Safari | UNKNOWN | fallbacks exist (no Keyboard Lock, prefixed fullscreen, blocked storage) and are exercised with a stub only |
| Browser: Firefox | UNKNOWN | |

## How a cell becomes PASS

1. Run `docs/ops/headless-console-test.sh`, `headless-browser-test.sh`, `headless-clipboard-test.sh` and
   `headless-cycles-test.sh` through `docs/ops/headless-repro.sh` on that host.
2. Run one supervised live session (`docs/ops/live-grab-runbook.md`) and `docs/ops/soak.sh`.
3. Add the version to `TESTED_GNOME` (or `TESTED_PIPEWIRE_MAJOR`) in `compat.rs`, extend its unit test, and add the
   evidence row here in the same commit.
