# Compatibility matrix

The console checks three things at start and refuses an untested combination unless `--allow-untested` is given: the GNOME
Shell **major** version (tested: 50), the PipeWire **major** version (tested: 1) and a Wayland session. `blackroom-console
--check-compat` prints this host's verdict as JSON. The code that decides is `crates/blackroom-console/src/compat.rs`; the
tested list lives there. **Every other row below (distribution, GPU, encoder, minor versions, browsers) is documentation
only: the gate does not look at it**, so an UNKNOWN row there is not refused. The single-output rule is enforced
separately when a session starts.

## Support table (authoritative; the release notes and the README repeat it)

First release: a narrowly supported technical preview for the configuration below. "Tested" means a run with the evidence named
in the cell table further down; the owner's runs are owner-reported and kept without logs. No other document may claim more.

| Tested | Not tested (offered or expected to work, never observed) | Unsupported (refused or cannot work) |
|---|---|---|
| Ubuntu 26.04.1, GNOME Shell 50.1 on Wayland, PipeWire 1.6, NVIDIA RTX 3050 Ti with NVENC, built-in panel as the only output | Any other distribution, GNOME 49 or 51, PipeWire 2.x | X11 session, non-GNOME desktops, PipeWire 0.3 |
| Chrome on an Android tablet (owner) and headless Chrome (suites) | Safari and iOS (**waived by the owner, untested**), Firefox, installing the page as an app over a trusted address | |
| Shared and Private modes, sound, clipboard over https, approve-each-connection, tray menu, launcher, authenticator QR, clean `.deb` install (owner-reported 2026-10-04) | The OpenH264 software encoder in a live session; AMD or Intel GPUs | AMD/Intel hardware encoding (VA-API is not implemented) |
| **Private VPN (recommended):** Tailscale, phone on mobile data, direct path, `tailscale cert` https name (owner-reported 2026-10-04) | A relayed Tailscale path, NetBird, Headscale, any other VPN | |
| Home network (Wi-Fi) over https; **Direct internet access with a static IP and the console's own certificate** (owner-reported 2026-10-06: connected for 20+ minutes and still connected) | the browser warns "not secure" with the self-signed certificate (expected); with a Let's Encrypt certificate on an own domain it does not | |
| | A one-hour or longer soak (not run); laptop sleep or lid close; a reboot before anyone logs in; start at login and restart under systemd; real credential commands from the host page; certificate renewal; the emergency chord with the packaged console | More than one monitor, a second Unix user, multiple controllers (refused or unbuilt) |

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
| Access from mobile data through a VPN (Tailscale) | PASS (owner-reported) | 2026-10-04, Galaxy Tab S9 FE on mobile data, https name with a `tailscale cert` certificate; `tailscale status` showed a direct path; login, clipboard and video worked, a little laggier than Wi-Fi; after the browser tab went idle it needed a reconnect, which came back at once. Not covered: relayed paths, laptop sleep, reboot without a login |
| Direct internet access, static IP + the console's own self-signed certificate, set up from the host settings page | PASS (owner-reported 2026-10-06) | router ports forwarded; worked from mobile data and from Wi-Fi, transport on Auto and WebRTC in use, connected for 20+ minutes and still connected when reported; the browser showed the expected "connection not secured" warning for the self-signed certificate (opened `https://<ip>:8443`). the owner compared the browser's SHA-256 fingerprint with the laptop's and it matched. Not reported: how the session ended |
| Direct internet access, own domain (GoDaddy DNS) + Let's Encrypt certificate (manual DNS challenge) | PASS (owner-reported 2026-10-06) | the browser showed a secure connection with no warning; worked on mobile data, and a session was driven from a tablet (the owner wrote a message through it). Not reported: WebRTC or MJPEG on this run, how long it stayed up, how renewal went (manual, every 60 to 90 days) |

## How a cell becomes PASS

1. Run `docs/ops/headless-console-test.sh`, `headless-browser-test.sh`, `headless-clipboard-test.sh` and
   `headless-cycles-test.sh` through `docs/ops/headless-repro.sh` on that host.
2. Run one supervised live session (`docs/ops/live-grab-runbook.md`) and `docs/ops/soak.sh`.
3. Add the version to `TESTED_GNOME` (or `TESTED_PIPEWIRE_MAJOR`) in `compat.rs`, extend its unit test, and add the
   evidence row here in the same commit.
