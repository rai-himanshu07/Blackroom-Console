# Compatibility matrix

The console refuses to start on a combination it has not been proven on (`--allow-untested` overrides an *untested*
verdict; an *unsupported* one is never allowed). `blackroom-console --check-compat` prints this host's verdict as JSON.
The code that decides is `crates/blackroom-console/src/compat.rs`; the tested list lives there.

Cells: **PASS** = run end to end with evidence, **UNKNOWN** = never run (refused unless `--allow-untested`),
**NO** = cannot work (refused always).

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
| Shared mode (panel and input left alone) on the real screen | UNKNOWN | proven only on the headless Shell; pointer mapping with fractional scaling or an external monitor not observed |
| Laptop sound to the browser | UNKNOWN | headless: a 440 Hz tone arrives and decodes at 439 Hz; the real default output was not tried |
| Laptop top-bar indicator on the real desktop | UNKNOWN | headless: the extension loads in a throwaway Shell, shows the session and its menu's Disconnect ends it; the real top bar and notifications were not observed |
| Installing the page as an app | UNKNOWN | headless Chrome finds no installability problem; a tablet over a trusted https address was not tried |
| Browser: Safari, iOS Safari | UNKNOWN | fallbacks exist (no Keyboard Lock, prefixed fullscreen, blocked storage) and are exercised with a stub only |
| Browser: Firefox | UNKNOWN | |

## How a cell becomes PASS

1. Run `docs/ops/headless-console-test.sh`, `headless-browser-test.sh`, `headless-clipboard-test.sh` and
   `headless-cycles-test.sh` through `docs/ops/headless-repro.sh` on that host.
2. Run one supervised live session (`docs/ops/live-grab-runbook.md`) and `docs/ops/soak.sh`.
3. Add the version to `TESTED_GNOME` (or `TESTED_PIPEWIRE_MAJOR`) in `compat.rs`, extend its unit test, and add the
   evidence row here in the same commit.
