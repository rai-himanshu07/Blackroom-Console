# Feasibility research — GNOME 50.1 / Mutter 50.1 (Document 00 §50)

Confidence labels use Document 01 §51 vocabulary (assessment §5 C12):
`CONFIRMED` (verified directly on this host or in the exact installed source),
`LIKELY` (strong indirect evidence, not yet directly exercised),
`UNVERIFIED` (plausible but no direct evidence gathered), `UNSUPPORTED` (evidence
contradicts feasibility). Each topic names the experiment (Phase/Exp per the
roadmap) that will move `LIKELY`/`UNVERIFIED` findings to `CONFIRMED`.

Primary empirical sources for this document: Experiment 0
(`docs/experiments/evidence/exp00/2026-09-04/`), Experiment 1
(`docs/experiments/evidence/exp01/2026-09-05/`), Experiment 2
(`docs/experiments/evidence/exp02/2026-09-05/`, `docs/gnome/api-inventory.md`,
`docs/gnome/introspection/*.xml`). Secondary source: live inspection of the
**real** Mutter source at the exact installed tag, `50.1`, on
`gitlab.gnome.org/GNOME/mutter` (confirmed to exist via the project's `/-/tags`
page; the `/-/raw/<tag>/<path>` URL form returns plain source text, the
`/-/blob/` form triggers a bot-detection CAPTCHA on this network and was not
used). `apt-get source` is unavailable on this host (no `deb-src` configured,
assessment §3 risk realised) — GitLab was the fallback, as the plan anticipated.

---

## 1. Mutter private API stability

**Question:** Are the `org.gnome.Mutter.*` interfaces this project depends on
(`DisplayConfig`, `RemoteDesktop`, `ScreenCast`, `InputCapture`) present, and how
much do their private-API surfaces reasonably change between point releases?

**Sources:** Experiment 2 live introspection (`docs/gnome/introspection/mutter-*.xml`);
Mutter release notes for `50.1`–`50.4` (`gitlab.gnome.org/GNOME/mutter/-/tags`,
fetched 2026-09-05) show these interfaces receiving active bug-fix and feature
commits every point release (e.g. `50.3`: "Hide cursor while input capture is
active", `50.2`: "Allow input capture portal to integrate with clipboard") —
i.e. actively maintained, not frozen or deprecated.

**Findings:** All four interfaces are present and reachable on this host at the
paths recorded in `docs/gnome/api-inventory.md` (`CONFIRMED`). The interfaces
are explicitly documented upstream as private/unstable (Document 00 §52
"isolate GNOME-specific/private APIs" applies); `RemoteDesktop.Version=1` and
`ScreenCast.Version=4` are exposed specifically so clients can detect
capability changes across releases (`CONFIRMED` — both are real properties on
this host). Point-release changes are typically additive (new options,
bugfixes) rather than removing existing methods, based on the changelog
entries scanned — but this project must still isolate all Mutter calls behind
the `GnomeBackend` trait (assessment §6.1 repository layout) and re-verify this
inventory after any Ubuntu update.

**Confidence:** `LIKELY` (API presence and versioning are `CONFIRMED`; API
*stability across future updates* is inferred from changelog patterns, not
proven).
**Escalation:** re-run Experiment 2 after any `mutter`/`gnome-shell` package
update (Document 20 §52 "GNOME Update Detection", Phase 24 hardware/compat
matrix).

---

## 2. Virtual-monitor lifecycle (`RecordVirtual`, `ScreenCast.Session`)

**Question:** How is a virtual monitor created, and what is its lifecycle
(creation, association with a `ScreenCast` session, teardown on process death)?

**Sources:** `org.gnome.Mutter.ScreenCast` top-level interface (Experiment 2:
`CreateSession(a{sv}) -> o`, `Version=4`); Mutter changelogs mentioning
screencast internals (`50.1`: "Use fewer buffers for screencast streams",
`50.0`: "Minimize stage paints and buffer copies in screencasts", `50.rc`:
"Add HDR screen sharing support", "Fix screen sharing of monitors with no
framerate"). `RecordVirtual` itself is a method on the **session** object
returned by `CreateSession` (`org.gnome.Mutter.ScreenCast.Session` interface),
which by construction does not exist until a session is created — Phase 0–1
must not call `CreateSession` (hard rule), so this method's exact signature and
behaviour were **not** directly introspected this phase.

**Findings:** The top-level `ScreenCast.CreateSession` method exists and is
reachable (`CONFIRMED`). The existence of a dedicated `Session` sub-object
pattern matches the same pattern already `CONFIRMED` for
`org.gnome.Mutter.InputCapture` (its `CreateSession` also returns an object
path, `o`, per `docs/gnome/api-inventory.md`) and `org.freedesktop.login1`
(`Session` objects at `/org/freedesktop/login1/session/_NN`, `CONFIRMED` in
Experiment 1/2) — the "top-level interface creates a session sub-object"
pattern is consistent across every Mutter/logind interface checked this phase.
Virtual monitor creation, association with a `RecordVirtual` capture, and
automatic teardown on client-process death are all plausible based on this
pattern and the general D-Bus "session object" convention Mutter uses
throughout, but remain `UNVERIFIED` until a session is actually created.

**Confidence:** `UNVERIFIED` (mechanism exists; lifecycle behaviour unproven).
**Escalation:** Document 10 Experiment 4 (Virtual Monitor Creation) and
Experiment 15 (Main Agent Crash) — Phase 4/9.

---

## 3. DisplayConfig (all-physical-disabled configs, hybrid-GPU considerations)

**Question:** What does `GetCurrentState` report on this exact hybrid-GPU host,
and can Mutter run with zero physical monitors enabled (only a virtual one)?

**Sources:** Experiment 2 `DisplayConfig.GetCurrentState` call (live,
read-only) — full summary in `docs/gnome/api-inventory.md` and
`docs/experiments/evidence/exp02/2026-09-05/inventory.json`.

**Findings (all `CONFIRMED` — directly observed):**
- Two connectors reported: `HDMI-1` (Samsung `SAM`/"Smart M80C", 31 modes,
  **no mode currently marked `is-current`**) and `eDP-1` (AUO panel, 128 modes,
  current mode `1920x1080@120.21Hz`).
- **Only one logical monitor exists right now** (`eDP-1`, primary, position
  `(0,0)`, scale 1). `HasExternalMonitor=false`.
- **Naming mismatch, project-relevant:** Mutter's connector name (`HDMI-1`,
  `eDP-1`) differs from the kernel DRM connector name Experiment 0 read from
  `/sys/class/drm` (`card0-HDMI-A-1`, `card1-eDP-1`) — `HDMI-1` vs
  `HDMI-A-1` differ, `eDP-1` happens to match. **Any code correlating Mutter
  and kernel/DRM data must not assume these strings are interchangeable**;
  Document 05 §29's "keyed by connector+EDID serial, not index" schema should
  use Mutter's own connector string as the primary key since that is what
  `ApplyMonitorsConfig` will expect back.
- `HDMI-1` is physically `connected` (Experiment 0, `/sys/class/drm/card0-HDMI-A-1/status`)
  yet is **not currently part of any logical monitor** and has no current
  mode. The external 4K display is attached but not currently in use by the
  desktop session — this is the actual state Gate C research must account
  for; "is a physical output disabled" is not equivalent to "is a physical
  output part of the logical topology", and this host currently demonstrates
  the *disconnected-from-topology-but-electrically-connected* case, not the
  *actively-driven-then-disabled* case Gate C cares about.

**What remains unverified:** whether Mutter accepts an `ApplyMonitorsConfig`
call with **zero** physical monitors enabled when a virtual monitor exists —
`ApplyMonitorsConfig` is confirmed present on the interface (Experiment 2) but
was never called (hard rule, no mutation this phase).

**Confidence:** `LIKELY` current-state facts are `CONFIRMED`; the
zero-physical-monitor question is `UNVERIFIED`.
**Escalation:** Document 10 Experiment 5 (Virtual Monitor as Active Display)
and Experiment 6 (Physical Output Isolation) — Phase 4/5, with the safety
procedure in `docs/ops/experiment-safety.md` (SSH prerequisite currently
pending, see Blockers).

---

## 4. GNOME locking vs RemoteDesktop sessions

**Question:** Is there a distinct `org.gnome.Shell.ScreenShield` D-Bus surface,
and how does GNOME's lock state interact with a `RemoteDesktop`/`ScreenCast`
session?

**Sources:** Experiment 2 live introspection of `org.gnome.Shell.ScreenShield`
at three candidate paths (`docs/gnome/introspection/shell-screenshield-*.xml`,
`docs/gnome/introspection/screensaver.xml`); GNOME Shell `50.1` is installed
(dpkg, Experiment 0) — `js/ui/screenShield.js` was not fetched this phase
(time-boxed per the Research Stop Rule, Document 11 §43); gnome-remote-desktop
`50.2` is installed but inactive (masked for the duration of any future
RemoteDesktop experiment per `docs/ops/experiment-safety.md`).

**Findings (`CONFIRMED` — directly observed):** `org.gnome.Shell.ScreenShield`
is a real, owned D-Bus **name** (owned by the `gnome-shell` process), but it
exposes **no distinct interface** at any path tried:
`/org/gnome/Shell/ScreenShield` (empty), `/org/gnome/ScreenShield`
(nonexistent object), `/org/gnome/Shell` (only `org.gnome.Shell` and
`org.gnome.Shell.Extensions`). The only place this bus name resolves to real
interface content is `/org/gnome/ScreenSaver`, and the interface served there
is the classic `org.gnome.ScreenSaver` (`Lock`, `GetActive`, `SetActive`,
`GetActiveTime`, `ActiveChanged`, `WakeUpScreen`) — identical to what the
separate `org.gnome.ScreenSaver` bus name serves at the same path. **There is
one lock D-Bus surface on this host, not two.** This directly contradicts the
implicit assumption in some spec documents (assessment §5 C3) that
`ScreenShield` might be a separate richer interface; for this GNOME version it
is not, at least not over D-Bus.

**What remains unverified:** whether a `RemoteDesktop`+`ScreenCast(RecordVirtual)`
session created before `ScreenSaver.Lock` survives the lock (assessment §7.2);
whether EIS input reaches the unlock dialog; whether unlocking via remote
input re-enables physical outputs (it must not, per Doc 00 stop conditions).
None of these require GNOME mutation to answer conclusively — they require
Phase 8/11 experiments the plan explicitly reserves for later.

**Confidence:** `CONFIRMED` (interface topology) / `UNVERIFIED` (lock+remote
interaction behaviour) — this is the second-highest project risk (assessment
§7.2) and stays `UNVERIFIED` by design until its dedicated phase.
**Escalation:** Document 10 Experiment 11 (GNOME Lock Semantics) and
Experiment 12 (Same-Session Continuity) — Phase 8, Architecture Review #1.

---

## 5. libei / EIS

**Question:** Is `libei`/`libeis` 1.5 available, and what does Mutter's EIS
integration look like structurally?

**Sources:** Experiment 0 dpkg versions (`libei1`/`libeis1` = `1.5.0-3`,
`CONFIRMED`); Experiment 2 introspection showing `InputCapture.CreateSession`
and (per the source excerpt read this session, §"physical input isolation"
below) `MetaInputCaptureSession`'s direct use of `#include <libeis.h>` and the
`struct eis`/`eis_new`/`eis_setup_backend_fd`/`eis_seat_new_device` API family
in `meta-input-capture-session.c` (real source at the `50.1` tag).

**Findings:** `libei`/`libeis` 1.5.0 is installed and Mutter 50.1's own source
uses it directly for both seat/device emulation (keyboard + pointer + button +
scroll capabilities configured via `eis_seat_configure_capability`) and an XKB
keymap handed to the remote peer via an anonymous memfd
(`ensure_xkb_keymap_file`/`mtk_anonymous_file_new`) — `CONFIRMED` via direct
source reading against the exact installed version. `ConnectToEIS` (on
`RemoteDesktop.Session`) was not directly inspected (session-scoped, no
session created this phase) but `InputCapture`'s `ConnectToEIS` equivalent
(`handle_connect_to_eis`) follows the same pattern: it hands out an EIS
backend fd via `eis_backend_fd_add_client`, over the D-Bus method's Unix-FD
passing mechanism — the same mechanism the `reis` Rust crate (assessment §6.1
crate inventory, evaluated in `docs/security/architecture.md`) is designed to
consume.

**Confidence:** `CONFIRMED` (libei presence and Mutter's general integration
pattern, from direct source).
**Escalation:** Document 10 Experiment 8 (Remote Input) will exercise the
`reis` crate against this exact `ConnectToEIS` path — Phase 6.

---

## 6. Physical input isolation candidates

**Question:** In what order should physical-input-isolation mechanisms be
attempted, and what does the leading candidate (`InputCapture`) actually do?

**Sources:** Direct read of `meta-input-capture-session.c` at the `50.1` tag
(`gitlab.gnome.org/GNOME/mutter/-/raw/50.1/src/backends/meta-input-capture-session.c`,
fetched 2026-09-05 — full source, not a summary); Experiment 2 introspection
(`InputCapture.SupportedCapabilities=15`, `CreateSession(u) -> o` present).

**Findings — this corrects the mechanism description in assessment §7.1
candidate 1 (`CONFIRMED` from source, not previously verified against actual
code):**
- `InputCapture` is a **barrier-crossing (KVM-edge-style) trigger model, not
  an unconditional permanent grab.** State machine:
  `INIT → (Enable, installs pointer barriers) → ENABLED → (pointer crosses a
  barrier: on_barrier_hit) → ACTIVATED (EIS devices start emulating) →
  (Release/Disable) → back to ENABLED/INIT`.
- `AddBarrier` takes an axis-aligned line and `check_barrier()` **rejects** it
  unless the line is adjacent to exactly one logical-monitor edge (overlap,
  partial overlap, or adjacency to more than one monitor edge are all
  rejected with `G_IO_ERROR_INVALID_DATA`). Barriers are pointer-motion
  triggers (`MetaBarrier`); the activation path
  (`on_barrier_hit` → `meta_dbus_input_capture_session_emit_activated`) is
  driven purely by **pointer** motion.
- The source file inspected does **not** show a code path where physical
  **keyboard** events are intercepted independently of that pointer-barrier
  trigger; the decision to route a given input event to
  `meta_input_capture_session_process_event` (EIS) versus the normal focused
  surface is made in compositor/seat code outside this file, and was not
  traced this phase.
- **Project impact:** the assessment's original candidate description ("activates
  a capture ... and never releases it") describes the *desired product
  behaviour*, not the *actual API mechanism*. Achieving "physical input never
  reaches the local session during `REMOTE_ACTIVE`" with this API likely
  requires: (a) the virtual monitor being the only logical monitor so there is
  no physical desktop to leak onto in the first place (Gate C dependency), and
  (b) barriers placed to cover the entire usable pointer area so activation is
  effectively immediate — but the *keyboard* isolation path is not yet
  understood from this file alone and must not be assumed solved.
- Candidate order from assessment §7.1 remains reasonable: (1) `InputCapture`
  (`CONFIRMED` present, mechanism partially understood), (2)
  `RemoteDesktop`/`InputMapping` options (`InputMapping.GetDeviceMapping`
  `CONFIRMED` present, Experiment 2), (3) a minimal `EVIOCGRAB` helper as
  fallback (Document 06 §42–45).

**Confidence:** `LIKELY` for the barrier-crossing mechanism (`CONFIRMED` from
source) actually achieving complete isolation when configured as Blackroom
needs; `UNVERIFIED` for the keyboard-isolation path specifically. This is
**Gate E, the highest project risk** (assessment §7.1) — stays `UNVERIFIED`
until directly tested.
**Escalation:** Document 10 Experiment 9 (Physical Input Isolation, the hardest
gate) and Experiment 38 (Physical Input Verification, independent observer) —
Phase 7. If keyboard isolation cannot be confirmed via `InputCapture` alone,
re-examine candidate 3 (`EVIOCGRAB`) earlier than planned.

---

## 7. PipeWire lifecycle

**Question:** Is PipeWire available and healthy, and how does a `ScreenCast`
session's PipeWire node lifecycle work?

**Sources:** Experiment 0 (`pipewire 1.6.2-1ubuntu1.1`, `wireplumber
0.5.13-1ubuntu1`, both `CONFIRMED` installed via dpkg); Mutter `ScreenCast`
top-level interface (`CONFIRMED` present, Experiment 2). PipeWire node
creation itself happens inside a `ScreenCast.Session.RecordVirtual`/`Record*`
call, which is session-scoped and was not exercised this phase.

**Findings:** PipeWire 1.6.2 and WirePlumber 0.5.13 are installed and are
recent, actively maintained versions (`CONFIRMED`). The `pipewire` Rust crate
(assessment §6.1 crate inventory) targets this exact API generation. Node
creation, negotiation, and teardown-on-session-death are `UNVERIFIED` —
Document 10 Experiment 3 (Basic Screen Capture) is the first phase that
creates any PipeWire node at all, and is explicitly out of scope for Phase 0–1
(no GNOME mutation).

**Confidence:** `LIKELY` (versions and general architecture are sound;
lifecycle behaviour unproven).
**Escalation:** Document 10 Experiment 3 (Basic Screen Capture) — Phase 3;
Experiment 17 (PipeWire Failure) — Phase 9.

---

## 8. systemd privilege boundaries

**Question:** What are the actual privilege/control boundaries between the
system and user systemd managers on this host, and does polkit apply?

**Sources:** Experiment 0 (`systemd 259.5-0ubuntu3.4`, `CONFIRMED`); Experiment
1 (`systemctl --user list-units 'gnome-session*' 'graphical-session*'` output,
`CONFIRMED` live — 13 loaded units including `gnome-session-manager@ubuntu.service`,
`graphical-session.target`); Experiment 2 `org.freedesktop.login1.Manager`
full method/property inventory (`CONFIRMED`, `docs/gnome/api-inventory.md`) —
notably `LockSession`, `UnlockSession`, `TerminateSession`, `KillSession`,
`AttachDevice`, and the `Session` interface's own `TakeControl`/
`ReleaseControl`/`TakeDevice`/`ReleaseDevice`/`PauseDeviceComplete` methods,
which are the exact primitives Document 06's privilege-crossing design
(assessment §6.3) depends on for `remote-hostd`/`remote-emergencyd` to control
the user session from the system side.

**Findings:** `login1` exposes everything the architecture needs to lock a
session (`LockSession`/`Session.Lock`, `CONFIRMED` present) and to interrogate
session state (`Active`, `State`, `IdleHint`, all `CONFIRMED` present and
correctly read in Experiment 1) without requiring a custom system→user IPC
channel for those specific operations. What `login1` does **not** obviously
expose is a generic "start/stop this specific systemd **user** unit" call —
`systemctl --user` operations are normally only reachable from *within* the
user's own bus/session, meaning `remote-emergencyd` (running as a system
service) needs a separate, explicit mechanism to stop
`gnome-session-agent.service` in the user manager. Candidates
(`sd_bus` call to the user manager via `XDG_RUNTIME_DIR/bus`, or a `SIGTERM`
to the unit's tracked main PID via `login1`'s `Session.Leader` property, which
**is** `CONFIRMED` present) are exactly what assessment §7.5 flags as the
Phase 10 research item — not resolved by this phase.

**Confidence:** `CONFIRMED` (login1 primitives exist and were exercised
read-only); `UNVERIFIED` (the system→user unit-control path assessment §7.5
calls out, and whether polkit gates any of the admin-facing `blackroom` CLI
verbs in practice — no polkit action has been installed yet, Phase 15).
**Escalation:** Document 06 §19 system→user control matrix — Phase 10
(Emergency Controller); polkit action installation — Phase 15 (Host Security
Authority).

---

## 9. GPU-specific behaviour

**Question:** What GPU-specific behaviour must the project account for on
this hybrid NVIDIA+Intel host, and what encoders are available?

**Sources:** Experiment 0 (`CONFIRMED`, live): GPU `card0` = NVIDIA (driver
`nvidia`, PCI `10DE:25A0` = GeForce RTX 3050 Ti Mobile, drives `HDMI-1`/
`card0-HDMI-A-1`), `card1` = Intel (driver `i915`, PCI `8086:9A60` =
TigerLake-H GT1, drives the internal panel `eDP-1`); loaded kernel modules
`i915, nvidia, nvidia_drm, nvidia_modeset, nvidia_uvm,
nvidia_wmi_ec_backlight, xe` — **both** `i915` and `xe` are loaded
simultaneously (Intel's newer `xe` driver coexisting with legacy `i915`, a
real and Ubuntu-26.04-specific detail, `CONFIRMED`). Mutter `50.1`–`50.2`
changelogs (fetched 2026-09-05) explicitly reference NVIDIA-specific fixes in
this exact release range ("Fix performance regression with some nvidia driver
versions" `50.1`; "Fix freeze with nvidia driver" `50.1`; "Remove support for
legacy NVIDIA drivers" `50.2`) — this project's proprietary NVIDIA driver is
an actively-tracked compatibility concern upstream, not a neglected edge case.
`gst-inspect-1.0` encoder enumeration (Document 20 §16/§17, `gst-inspect-1.0 |
grep -E 'va|nv|x264'`) was **not** run this phase (GStreamer plugin
inspection needs no GNOME mutation and is safe read-only, but was out of
scope for the three defined Phase 1 experiments; see Blockers).

**Findings:** The hybrid GPU topology from assessment §3/§7.3 is `CONFIRMED`
exactly as recorded: NVIDIA drives the external 4K display, Intel drives the
internal panel, and (per topic 3 above) the external display is not currently
in the active logical-monitor topology at all. Whether Mutter's primary GPU
for compositing purposes is the Intel or NVIDIA device, and which GPU actually
renders the virtual monitor's PipeWire stream, is `UNVERIFIED` — this matters
directly for whether `RecordVirtual`'s output needs a cross-GPU buffer copy
(a known Mutter concern per changelog: "Handle cross GPU buffer scanout",
`51.rc`) with a performance/latency cost Document 19 cares about.

**Confidence:** `CONFIRMED` (hardware topology); `UNVERIFIED` (compositing GPU
selection, encoder availability, hardware-cursor behaviour on the NVIDIA
output specifically).
**Escalation:** `gst-inspect-1.0` encoder enumeration — trivial read-only
addition to a near-future experiment, no blocker; Document 10 Experiment 29
(GPU Matrix) and Experiment 28 (Cursor Behavior) — Phase 9/24.

---

## Feasibility blockers

None that block Phase 2 (state-machine core against the mock `GnomeBackend`,
which needs none of the above). Blockers that gate **later** phases:

1. **Phase 1 step 5 (user action, unresolved as of this report):** dev-header
   `apt install` and a verified key-based SSH login from a second device
   (`docs/ops/experiment-safety.md` §1) must complete before Document 10
   Experiment 6 (Physical Output Isolation) or Experiment 9 (Physical Input
   Isolation) may run — those are the first experiments that mutate display
   or input state. Does not block Phase 2–3.
2. **Gate E mechanism gap (topic 6 above):** `InputCapture`'s keyboard-isolation
   code path was not traced to a conclusion from the source read alone;
   Experiment 9 must resolve this before Phase 7 can claim Gate E `PASS`.
3. **Gate A/lock interaction (topic 4 above):** whether a RemoteDesktop session
   survives `ScreenSaver.Lock` is completely open; Experiment 11 (Phase 8) is
   the first phase allowed to test it.
4. **Zero-physical-monitor question (topic 3 above):** whether
   `ApplyMonitorsConfig` accepts an all-virtual topology is open; Experiment
   5/6 (Phase 4/5) resolve it.

None of these are `UNSUPPORTED` findings; none require weakening a security
requirement (Document 00 §49 stop conditions do not currently apply).
