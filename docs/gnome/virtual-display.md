# Virtual display PoC — RemoteDesktop, ScreenCast, PipeWire, virtual monitor (Phase 4)

Turns Phase 0-1's read-only research and Phase 3's read-only session/capability
discovery into the project's **first mutating** GNOME/Mutter/PipeWire
production code (assessment C2: mock-first ends here for RemoteDesktop/
ScreenCast/virtual-monitor mechanics). Every call added this phase creates,
starts, stops, or destroys real Mutter/PipeWire state — still no physical
display/input isolation, no remote input, and no encoding/WebRTC (Phases 5–8,
Stage III+).

## Production modules

- `crates/blackroom-gnome/src/mutter/remote_desktop.rs` —
  `RemoteDesktopSession` (`CreateSession`/`Start`/`Stop`, `Drop`-safe cleanup;
  `Stop()` on an unstarted session is idempotent-by-construction rather than
  surfacing the real `Session not started` D-Bus error).
- `crates/blackroom-gnome/src/mutter/screencast.rs` — `ScreenCastSession`
  (`CreateSession`/`Start`/`Stop`/`record_monitor`/`record_virtual`) and
  `ScreenCastStream` (`start_and_wait_for_pipewire_node`). `RecordVirtual` and
  `RecordMonitor` live on the **same** session interface (Experiment 3
  introspection), so `record_virtual` was added directly to the existing
  `ScreenCastSession` rather than a duplicate session-wrapper type.
- `crates/blackroom-gnome/src/mutter/virtual_monitor.rs` — `VirtualMonitor`:
  orchestrates `record_virtual` → drive a bounded PipeWire capture → confirm
  via `DisplayConfig.GetCurrentState` → return a handle whose `destroy()` (or
  `Drop`) stops the underlying session.
- `crates/blackroom-gnome/src/mutter/pipewire_capture.rs` — `capture_frames`:
  a bounded (frame-count-or-timeout) PipeWire stream connect/receive, no
  encoding (Non-Goal). First production use of the `pipewire` crate
  (evaluated → in-use, `docs/security/architecture.md` §6).

## Experiments (evidence in `docs/experiments/evidence/exp0{3,4,5}/`)

- **Experiment 3 — Basic Screen Capture** (Doc 10 §10 / Doc 02 §9): captured
  the *existing* desktop via `ScreenCast.CreateSession` + `RecordMonitor`,
  **without** pairing a `RemoteDesktop` session (Doc 02 §9 step 2's "if
  required" resolves to "not required" for plain monitor capture — resolved
  by live introspection, not assumed). Result: PASS.
- **Experiment 4 — Virtual Monitor Creation** (Doc 10 §11 / Doc 02 §10):
  `RecordVirtual` at 1280×720/1920×1080/2560×1440@60Hz, each confirmed via
  `GetCurrentState` and destroyed cleanly; 50-cycle reliability loop (Doc 19
  §16–17) with 0 leaked PipeWire nodes and no GNOME Shell crash. Result: PASS.
- **Experiment 5 — Virtual Monitor as Active Display** (Doc 10 §12 / Doc 02
  §11): `ApplyMonitorsConfig(method=Temporary)` added the virtual monitor as
  an **additional** logical monitor alongside the host's existing physical
  monitors (this host currently runs both `HDMI-1`, an external 4K display,
  and `eDP-1`, the internal panel — both stayed enabled/unchanged throughout);
  `RecordMonitor` on the new virtual connector received real frames, proving
  the compositor actually renders onto it; original topology explicitly
  restored and independently re-verified byte-identical (serial-normalized)
  against a pre-run `GetCurrentState` snapshot taken outside the experiment
  binary. Result: PASS.

## Key findings (session-sub-object mechanics, previously unconfirmed)

`feasibility-research.md` topic 2 flagged that `RecordVirtual` (and sibling
session-sub-object methods) could not be introspected before a session
existed. Now confirmed live:

- `org.gnome.Mutter.ScreenCast.Session` exposes `Start`, `Stop`,
  `RecordMonitor`, `RecordWindow`, `RecordArea`, `RecordVirtual`.
- `org.gnome.Mutter.RemoteDesktop.Session` exposes `Start`, `Stop`,
  `ConnectToEIS`, keyboard/pointer/touch injection, and clipboard methods
  (unused until Phase 6).
- **The virtual monitor's connector does not appear in
  `DisplayConfig.GetCurrentState` immediately after `Start()`/
  `PipeWireStreamAdded`** — it only appears once a real PipeWire client
  actually consumes the stream. Production code (`virtual_monitor.rs`) drives
  a bounded capture *before* polling for confirmation, not after.
- **`RecordVirtual`'s actual resolution is driven by the negotiated PipeWire
  video format (the `SPA_PARAM_EnumFormat`/`Format` pod), not by the
  properties dict passed to `RecordVirtual` itself** — the dict's exact key
  schema is undocumented and this project's client-proposed
  `width`/`height`/`framerate` keys may or may not be read by Mutter; the
  PipeWire format negotiation is what actually determines the delivered
  resolution.
- **Removal from the raw connector inventory is *also* not synchronous with
  `Stop()` returning** — symmetric to the appearance finding above. An
  immediate post-`Stop()` `GetCurrentState` snapshot could still show the
  virtual connector present; `exp05` polls until it disappears (or a bound
  elapses) rather than checking once.
- `ApplyMonitorsConfig`'s `method` argument: `0` = Verify, `1` = Temporary
  (used throughout this phase — never persists to `monitors.xml`), `2` =
  Persistent (never used by this project, Doc 02 §11).
- Writing `ApplyMonitorsConfig`'s logical-monitors array requires a
  **different** per-monitor shape (`connector`, `mode_id`, properties) than
  reading `GetCurrentState` provides (`connector`, `vendor`, `product`,
  `serial` — no mode id); the mode id must be cross-referenced from the
  top-level `monitors` array's `is-current` mode for that connector.

## Decisions required by this phase (both evidence-cited, not defaults)

### 1. Capability tier promotion

`screencast_capable`, `pipewire_capable` promoted `EXPERIMENTAL → SUPPORTED`;
`virtual_display_capable` promoted `UNKNOWN → SUPPORTED_WITH_LIMITATIONS` (not
a clean `SUPPORTED`: the zero-physical-monitor question, Phase 5's job, and
GPU-specific/cursor behaviour on this hybrid host, Phase 9/24's job, stay
open). **`remote_desktop_capable` was *not* promoted** — an independent review
of this phase found that Experiment 3's only recorded `RemoteDesktop.Session
.Stop()` call was made without a prior `Start()` and correctly errored
("Session not started"); no successful `Start()`/verified cleanup was ever
exercised for `RemoteDesktop` specifically (unlike `ScreenCast`, whose full
lifecycle Experiments 3–5 did prove). It stays `EXPERIMENTAL` pending Phase 6
(Experiment 8), which will actually call `ConnectToEIS` and needs a real
`Start()`. *(Update 2026-09-30: Experiment 8 supplied that lifecycle and
`remote_desktop_capable` is now `SUPPORTED_WITH_LIMITATIONS`; see
`capability-report.md`.)* All promotions are structurally fixed in `capability.rs` (not
computed by a live mutating call inside `detect()`) — Doc 19 §16–17 treats
repeated virtual-monitor/RemoteDesktop/ScreenCast creation as a first-class
reliability risk, so creating one on every `gnome-session-agent` startup would
itself be that risk. See `docs/gnome/capability-report.md` for the re-issued
table.

### 2. `GnomeBackend` assembly stays deferred

No concrete `GnomeBackend` implementation (e.g. a `MutterBackend` struct) was
assembled this phase; `backend.rs` is unchanged. The roadmap's own Phase 4
file list names exactly the four modules above, not a backend-assembly file;
Phase 3's plan already grouped `remote_desktop.rs` with Phase 5's
`display_config.rs` and Phase 6's `eis.rs` as co-equal prerequisites for a
future assembly, implying it happens once significantly more of the 13 Doc 05
§8 ops are real — not after the first four. No caller exists yet
(`remote-hostd` is Phase 11+) to justify a struct where 9 of 13 methods would
be placeholders. Revisit once Phase 5/6's modules also exist, or once a real
caller needs one.

## Scope boundary: physical-monitor removal deferred to Phase 5

Doc 02 §11 lists "the physical monitors can be removed from the active
topology" among what Experiment 5 "must verify". This phase interprets that
narrowly: Experiment 5 adds the virtual monitor as an *additional* active
display and proves rendering on it, but deliberately does **not** attempt to
disable or remove any physical monitor — that requires the `DisplayBackup`/
hash-verified-restore/hotplug machinery the roadmap assigns to Phase 5's
`display_config.rs`, plus the SSH/watchdog safety procedure
(`docs/ops/experiment-safety.md` §1–4) that Phase 4 does not yet need. This is
a scoping interpretation flagged for user sign-off, not a document
instruction.

## `docs/ops/experiment-safety.md` scope update

Phase 4 is the first real trigger of §5 (`gnome-remote-desktop` masking) —
Experiment 2 (Phase 0-1) never created a session, so it never applied before
now. §1–4 (SSH prerequisite, watchdog timer, VT fallback) remain not-required
this phase: no physical output was disabled and no input was isolated, so the
"operator stranded without physical control" failure mode those sections
exist for does not apply. The new risk this phase (Mutter/PipeWire resource
leaks, Doc 19 §16–17) was instead addressed via `pw-dump`-equivalent node
counting, a GNOME Shell PID liveness check, and a bounded 10s/cycle timeout.

## Independent review findings (step 12) — all fixed, re-verified live

An independent Reviewer pass (this project's established practice — Phases
2–3 both had review catch real gaps) found 3 real issues before this phase
could be marked complete:

1. **exp05's cleanup guard was armed too late.** `RestoreGuard` (topology
   restore) was only constructed after the virtual monitor's `ScreenCast`
   session was already created and `RecordVirtual`'d — an early error in
   between (e.g. `PipeWireStreamAdded` timing out) would have leaked that
   session with no cleanup at all. Fixed by adding a second RAII guard,
   `SessionStopGuard`, armed immediately after `CreateSession` succeeds.
2. **exp05's recorded evidence was internally inconsistent and its pass
   predicate too weak.** The stored `physical_connectors_after_restore`
   still listed the virtual connector with `topology_restored: true` and no
   explanation, and the PASS predicate never checked whether the final
   `Stop()` call succeeded. Fixed by reordering (restore → `Stop()` →
   *poll* until the connector disappears from the raw inventory, since its
   removal is not synchronous with `Stop()` returning either — see above),
   adding `stop_error`/`virtual_connector_fully_gone` fields to the
   evidence, and requiring both in the pass predicate. Re-run live after the
   fix: still PASS, now with fully self-consistent evidence.
3. **`REMOTE_DESKTOP_CAPABLE`'s promotion to `SUPPORTED` was not backed by
   evidence.** Experiment 3's only recorded `RemoteDesktop.Session.Stop()`
   call was made without a prior `Start()` and correctly errored — no
   successful `Start()`/verified cleanup was ever exercised for
   `RemoteDesktop` specifically (unlike `ScreenCast`). Reverted to
   `EXPERIMENTAL` in `capability.rs`, `capability-report.md`, and
   `remote_desktop.rs`'s module doc comment; only `screencast_capable`,
   `pipewire_capable`, and `virtual_display_capable` were genuinely promoted
   this phase (Decision #1, corrected).

The mid-review re-run also required re-masking `gnome-remote-desktop`
(unmasked once already after the first exp05 pass) and hit an unrelated
transient condition worth recording: a user-launched QEMU Windows-11 VM
briefly pushed available memory down to ~5.7 GiB with some swap in use —
paused before re-running the experiment, resumed once the VM was closed and
memory recovered to ~22 GiB available. Unrelated to this phase's code; noted
for awareness of this host's environment.

## Non-goals reaffirmed

No physical display isolation or `ApplyMonitorsConfig` call disabling a
physical monitor (Phase 5), no remote input/EIS/physical input isolation
(Phases 6–7), no GNOME lock/same-session validation (Phase 8), no video
encoding/GStreamer/WebRTC (Stage III+), no change to `blackroom-core`'s state
machine, and no assembled concrete `GnomeBackend` (see Decision #2 above).
