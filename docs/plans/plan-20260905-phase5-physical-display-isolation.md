# Plan: Phase 5 — Physical Display Isolation

**Created:** 2026-09-05
**Status:** stopped: eDP-1 cleanup reactivated HDMI-1 after watchdog PASS; Gate FEAS-C unproven
**Approved by:** user ("approved proceed", 2026-09-05)
**Task tier:** governed

The recorded one-run approvals are spent and Gate FEAS-C remains stopped. For
future diagnostic work, use the prospective run-specific route in
`docs/ops/experiment-safety.md` §7: preserve the product stop and the failed
connected-HDMI observation, but do not require another independent review for
each same-mechanism supervised diagnostic with unchanged recovery controls.
Offline implementation may continue independently; no live run is approved by
this plan alone.
The matrix, hotplug/mode variants, and cycle counts below are the original
comprehensive acceptance targets, not prerequisites for building the app
offline. Select the minimum live evidence for a *supported* display layout
before enabling isolation there; leave connected-HDMI support disabled while
its final restoration remains unreliable. Defer other runs until a named
failure or expanded support claim calls for them. Do not mark FEAS-C PASS
from the unplugged-HDMI diagnostic or from incomplete privacy/restore proof.

## Goal

Prove, with real (not mocked, not black-window) Mutter mutation, that every physical
display output on this host can be disabled while only the virtual monitor stays
active — a genuine **zero-physical-monitor** topology, not merely "an additional
display" (Phase 4's proven, narrower case) — verified both via
`DisplayConfig.GetCurrentState` and by direct human observation/photo of the
physical panels, and that the exact original topology (connector, mode, scale,
transform, position, primary) is restored reliably, including across a monitor
hotplug and an **ungraceful** (process-killed) termination. Close the hard gate
**FEAS-C** (Doc 10 §48: "Physical display cannot expose the active desktop during
remote mode"). Land `crates/blackroom-gnome/src/mutter/display_config.rs` — the
`DisplayBackup` snapshot/disable/restore/hotplug module every later phase's real
`GnomeBackend` ops will build alongside.

This is the first phase where the operator must be **physically present and
actively participating** for the whole execution session (arming/confirming the
restore watchdog over the out-of-band SSH channel, physically pressing
`Ctrl+Alt+F3`, taking photos of the panels) — unlike Phase 0–1's safe overnight,
read-only run. See Risks.

## Acceptance Criteria

- With only the virtual monitor enabled, every original physical connector is
  present in `GetCurrentState`'s top-level `monitors[]` inventory (the hardware
  reference persists) but absent from every `logical_monitors[].monitors[]` entry
  (Doc 05 §33/§34: "disabled" = not part of the active desktop topology, not
  necessarily "panel electronics powered off") **and** the panels show no desktop
  content to a human observer (photo evidence; standby/no-signal/blank behaviour
  noted per output, per Doc 02 §13's three-way distinction).
- Exact original topology (connector, mode, scale, transform, position, primary)
  is restored and compared by hash (`DisplayBackup.configuration_hash`) plus
  field-by-field comparison, for every matrix row.
- A monitor hotplug (HDMI-1 connect/disconnect — the only physically
  disconnectable output on this host) during isolation keeps the reconnected
  output isolated (does not silently reappear as an active logical monitor) or
  triggers safe teardown; original topology can still be restored afterward.
- Matrix rows: `eDP-1` alone, `HDMI-1` alone, both together — each recording
  GPU/driver/kernel/GNOME/Mutter versions and per-row virtual-monitor/capture/
  teardown/restore results (Doc 10 §36's exact field list), satisfying
  Experiment 29 without a dedicated binary (Decision 5). AMD stays `UNKNOWN`
  (no hardware available, assessment §11 item 3, unchanged this phase).
- 50 isolate/restore cycles complete clean (machine-verified via `GetCurrentState`
  + hash, bounded per-cycle timeout per `experiment-safety.md` §6) — see Decision
  8 for why these are not 50 operator-witnessed photo sessions.
- The restore watchdog (`experiment-safety.md` §2) is implemented and proven to
  actually fire and restore at least once via a deliberate ungraceful-termination
  scenario; `experiment-safety.md` §3's previously-`UNVERIFIED` VT-fallback
  question for a DPMS-off/disabled `eDP-1` is resolved empirically before any
  real isolate run (Decision 7).
- Assessment §7.3's two open questions are answered with direct evidence, not
  assumption: (a) whether Mutter's `ApplyMonitorsConfig` accepts a configuration
  with zero physical monitors enabled; (b) what Mutter restores automatically
  when the owning process dies (the ungraceful-termination scenario above).
- `docs/gnome/capability-report.md`'s `VIRTUAL_DISPLAY_CAPABLE` and
  `DISPLAY_CONFIG_CAPABLE` rows have their "all-physical-disabled edge case
  remains open" notes resolved one way or the other, with `capability.rs` updated
  to match per the evidence-driven promotion rule (not asserted here).
- Gate **FEAS-C** recorded PASS, **or** — if Mutter refuses a zero-physical
  configuration or restoration proves unreliable — the plan's Execution Log
  records a Doc 00 §49 / Doc 10 §49 stop-and-report instead of a workaround
  (no black-window fallback, no silent scope-narrowing; Doc 05 §32).
- `cargo test --workspace`, `cargo fmt --check`,
  `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo check --workspace --all-targets`, `cargo deny check`, `cargo audit` all
  green; `Blackroom_Console` re-indexed; independent Reviewer-subagent pass
  recorded.

## Non-Goals

- No remote input, EIS, or `reis` (Phase 6, `eis.rs`).
- No physical input isolation (Phase 7, Gate E).
- No GNOME lock / same-session validation (Phase 8).
- No change to `blackroom-core`'s state machine, transitions, lease, or epoch
  logic, and no new `ErrorCode` variants (Decision 9).
- No assembled concrete `GnomeBackend` implementation (no `MutterBackend`
  struct); `backend.rs` is not edited this phase (Decision 2 — the user's
  second explicit question, answered below).
- No shared-types refactor across `capability.rs`/`virtual_monitor.rs`/
  `display_config.rs`, and no new dependency from `blackroom-experiments` on
  `blackroom-gnome` (Decision 10).
- No HiDPI/scale-factor matrix (Doc 05 §38), no cursor-behaviour verification
  (Doc 05 §39–40, Doc 10 Experiment 28) — both fall inside the read range but
  outside this task's cited experiment list; they stay deferred to Phase 9/24
  per `capability-report.md`'s existing `GPU_CAPABLE`/cursor deferral
  (Decision 11).
- No testing of restoration after system reboot, PipeWire failure, Mutter
  failure, or emergency takeover (Doc 20 §24's broader trigger list) — those
  map to already-roadmapped Experiments 17/18/19/21–24 (Phase 9/10), not this
  phase's cited scope (Decision 11).
- No lid-close testing (Doc 20 §21) — closing the lid risks an uncontrolled
  systemd-logind suspend mid-experiment, an orthogonal mechanism not needed to
  prove Gate C (Decision 11).
- No `MUTTER_DEBUG_DISABLE_HW_CURSORS=1` applied speculatively — only if a real,
  measured hardware-cursor problem is actually observed during HDMI-1 (NVIDIA)
  isolation.
- No permanent `monitors.xml`/`gsettings` change — every `ApplyMonitorsConfig`
  call stays `method=Temporary`.
- No dock/undock testing (Doc 10 §33) — no dock hardware on this host; documented
  as not-applicable, not silently skipped (Decision 4).

## Evidence And Decisions

- **Sources read in full or by exact section this session:** roadmap Phase
  4/5/6/7 entries (`docs/plans/plan-20260904-blackroom-console-master-roadmap.md`
  lines 163–222); Doc 00 §49 (Stop Conditions); Doc 10 §13–14 (Experiment 6/7),
  §33–34 (Experiment 26/27), §36 (Experiment 29 GPU Matrix), §44 (Experiment 37),
  §48–49 (Hard Feasibility Gates, Stop Conditions); Doc 02 §12–13 (Sixth
  Experiment, Important Physical Display Caveat), §37–38 (Physical Monitor
  Configuration Backup, Temporary Display Configuration); Doc 05 §28–40
  (Original Display Configuration → Cursor Requirements), §60–62 (Physical
  Console Restoration/Verification/Failure); Doc 11 §10 (Phase 5 DoD); Doc 20
  §20–24 (Display Hardware → Display Restoration Compatibility, not previously
  read this project); assessment §7.3 (FEAS-B/C open questions), §8 (safety
  plan), §11 (AMD unavailable); `docs/gnome/api-inventory.md` (DisplayConfig
  interface table — `ApplyMonitorsConfig`, `GetCurrentState`, `PowerSaveMode`,
  `ApplyMonitorsConfigAllowed`, `MonitorsChanged` signal, all confirmed present
  by Experiment 2 introspection, not assumed); `docs/ops/experiment-safety.md`
  (full); `docs/gnome/{capability-report,virtual-display}.md`; live source in
  full: `backend.rs`, `mutter/mod.rs`, `mutter/capability.rs`,
  `mutter/session.rs`, `mutter/remote_desktop.rs`, `mutter/screencast.rs`,
  `mutter/virtual_monitor.rs`, `bin/exp05_virtual_active.rs`,
  `blackroom-experiments/src/evidence.rs`, `blackroom-core/src/error.rs`, both
  crates' `Cargo.toml`; the Phase 4 plan (precedent for structure/decisions/
  non-goal wording and Evidence depth).
- **Citation check:** every section title the task cited was verified to match
  the live document exactly (Doc 10 §13/§14/§33/§34/§36/§44, Doc 02 §12/§13/
  §37/§38, Doc 05 §28–§40/§60–§62, Doc 20 §20–§24) — no numbering discrepancy,
  no new conflict-register entry needed (contrast Phase 2's C26).
- **MemPalace:** `mempalace_status` loaded (17,015 drawers, `blackroom_console`
  wing present, 109 drawers); `mempalace_diary_read` (agent `copilot`, wing
  `blackroom_console`, last 6) and `mempalace_search` both show Phase 4 complete
  + reviewed, "next: `/plan-task` Phase 5" — matches `docs/HANDOFF.md` and
  `/memories/repo/blackroom-console.md` exactly. No contradiction, no degraded
  mode, no Chronicle fallback needed.
- **Code graph:** `Blackroom_Console` status `ready`, 4438 nodes / 7483 edges,
  0 skipped/parse-partial files, `generation_matches: true` (indexed
  2026-09-05T13:44:06Z, same day, post-Phase-4). `check_index_coverage` over
  `crates/blackroom-gnome`, `crates/blackroom-experiments`, `docs/ops`,
  `docs/gnome`, and `error.rs` all returned `no_recorded_issue`. Scoped
  `get_architecture` on `crates/blackroom-gnome` shows exactly **one**
  `IMPLEMENTS` edge (`FakeGnomeBackend → GnomeBackend`) — confirms no concrete
  real `GnomeBackend` implementation exists yet, supporting Decision 2.

- **Decision 1 (user's first explicit question) — restructure into two
  binaries, reuse exp05's proven sub-mechanics, don't hand-wave "adapt vs
  fresh".** `exp05_virtual_active.rs` (read in full) already has, live-proven:
  `read_state`/`to_write_side`/`apply_monitors_config`/`current_mode_id`/
  `mode_width`/`is_current_mode` (GetCurrentState read + ApplyMonitorsConfig
  write-side reconstruction), `poll_for_new_connector`/`poll_for_connector_gone`
  (Experiment 4/5's "not synchronous with Start()/Stop()" finding), and the
  `RestoreGuard`/`SessionStopGuard` RAII idempotency pattern. All of this is
  mechanism-identical for the "disable" case — only step 3 ("build the new
  logical-monitors array") differs: exp05 appends the virtual monitor to the
  *original* entries, exp06 must build an array containing **only** the virtual
  monitor's entry (zero physical entries — literally assessment §7.3's open
  question). However, the roadmap's own file list names **two** binaries
  (`exp06_isolate_outputs.rs`, `exp07_restore.rs`), not one, unlike exp05's
  single-binary isolate+restore. Reason this is not just a style choice:
  assessment §7.3's second open question ("what does Mutter restore
  automatically when the owning process dies") cannot be exercised by a
  single-process design where the same process that isolates always also
  restores — that question requires a fresh, independent process to observe
  and restore state a first process may have abandoned. Decision: reuse exp05's
  proven sub-mechanics verbatim (adapted for the zero-physical array), but (a)
  `exp06` persists its snapshot as a `DisplayBackup` JSON sidecar into its own
  evidence directory (Doc 05 §29 schema, Decision 3) instead of keeping it only
  in an in-process guard; (b) `exp07` is a **separate** binary that loads a
  `DisplayBackup` JSON (its own most recent, or a path argument to a specific
  prior run's) and independently restores + verifies, provably decoupled from
  whether the isolating process is still alive. The watchdog-arming helper
  (`experiment-safety.md` §2) is written directly inside `exp06` rather than
  extracted to `blackroom-experiments`'s shared library: unlike Phase 4's
  introspection-scanner extraction (a real, immediate 3× same-phase reuse
  across exp03/04/05), only `exp06` needs it this phase — Phase 7's `exp09`
  is a different, not-yet-active phase, so extracting now would be the same
  speculative-abstraction mistake the project already declined once (Phase 3:
  "`blackroom-ipc` extraction deferred until a second real consumer exists").
- **Decision 2 (user's second explicit question) — GnomeBackend/MutterBackend
  assembly stays deferred, with a precise count, not "6 of 13".** Doc 05 §8's
  trait (`backend.rs`, read in full) has 13 ops. As of Phase 4: `session.rs`
  really implements `discover_session`; `virtual_monitor.rs` really implements
  `create_virtual_monitor`/`destroy_virtual_monitor`; `screencast.rs`'s
  `ScreenCastSession::start`/`stop` really back `start_capture`/`stop_capture`.
  `get_display_state` does **not** yet have a dedicated function returning the
  trait's exact `DisplayState { connectors, virtual_monitor_active }` shape —
  only ad hoc, per-file-duplicated `Vec<String>`/`bool` helpers
  (`capability.rs`'s `display_config_get_current_state_ok`,
  `virtual_monitor.rs`'s private `connectors()`). This phase's
  `display_config.rs` snapshot function is the natural place that finally
  produces one, and adds `disable_physical_outputs`/`restore_physical_outputs`
  for real. So after Phase 5, **up to 8 of 13** ops have real backing modules
  (not the task prompt's approximate "6") — correcting the count explicitly
  rather than silently repeating it, same pattern as Phase 2's C26 conflict
  handling. The remaining 5 (`enable_remote_input`/`disable_remote_input` →
  Phase 6 `eis.rs`; `lock_session` → Phase 8 `lock.rs`; `get_cursor_state` → no
  known backing D-Bus interface at all, an open Phase 3 research gap per
  `backend.rs`'s own doc comment; `restore_session` → needs display+input+lock
  all wired) stay unimplemented. Phase 4's own stated pre-condition for
  revisiting ("until Phase 5 **and** Phase 6 also exist") is only half-met
  after Phase 5 — Phase 6 still will not exist. No caller exists yet regardless
  (`remote-hostd` is Phase 11+; `gnome-session-agent`'s startup path calls
  `session::discover_session` directly today, not a `GnomeBackend` trait
  object). Assembling a struct with 5 of 13 methods still
  `unimplemented!()`/placeholder violates this project's own established
  discipline (Phase 2 declined `thiserror`/`schemars`/`tokio` without
  justified need; Phase 3/4 both deferred extractions "until a second real
  consumer exists"; `WORKFLOW_CONFIG.md`'s maintenance-first contract: "do not
  add speculative abstractions... or extension points"). Decision: **no
  concrete `GnomeBackend` impl this phase; `backend.rs` is not edited.**
  Realistically revisit once Phase 6 **and** Phase 7 or 8 land (closer to all
  13 ops being real), not fixed to one specific future phase number here.
- **Decision 3 — `DisplayBackup` schema, keyed by connector+EDID serial.** Doc
  05 §29's schema is `timestamp, session, outputs[], topology, primary_output,
  configuration_hash`; Doc 05 §28 additionally requires capturing
  enabled/disabled, mode, resolution, refresh, position, scale, rotation,
  primary, "HDR-related state where relevant", "color state where relevant".
  Assessment §7.3 and Doc 02 §37 ("do not assume connector names remain stable
  after hotplug") both require keying by connector **and** EDID serial, not
  array index. Concretely: `outputs: Vec<{connector, vendor, product, serial,
  mode_id, width, height, refresh_rate, enabled}>` (an output is `enabled` iff
  it appears in some `topology[].monitors` entry — this is the exact signal
  the acceptance criteria's "disabled in GetCurrentState" checks against);
  `topology: Vec<{x, y, scale, transform, primary, monitors: Vec<(connector,
  serial)>}>`; `primary_output: (connector, serial)`; `configuration_hash: u64`
  via `std::hash::DefaultHasher` over `outputs` sorted by `(connector,
  serial)` — an **integrity/equality check for same-process, same-build
  comparison, explicitly not a security control** (no new hashing crate
  dependency needed for that scope). This host has no HDR and no non-zero
  rotation currently in use; those two Doc 05 §28 fields are named but left
  unpopulated/optional this phase (no evidence of need) — flagged here as a
  scoped simplification, not silently dropped.
- **Decision 4 — hotplug scope: verified signal, HDMI-1-only physical test.**
  `org.gnome.Mutter.DisplayConfig.MonitorsChanged` (no args) is confirmed
  present by Experiment 2 introspection (`api-inventory.md` line 37) — Doc 20
  §7 "No API Guessing" satisfied, not assumed. `exp06`/`exp26` subscribe to it
  before triggering any change, mirroring `screencast.rs`'s already-proven
  "arm the signal listener before the triggering call" pattern
  (`PipeWireStreamAdded` before `Start()`). Repo memory confirms this host is a
  laptop where `HDMI-1` is the only physically disconnectable output — the
  live hotplug test (Doc 10 §33) disconnects/reconnects `HDMI-1` only.
  `eDP-1` hotplug and "dock/undock laptop" have no exercisable hardware on
  this host; both are documented as not-applicable in the evidence report,
  not silently skipped.
- **Decision 5 — Experiment 29 (GPU Matrix) is data recorded inside exp06/07/27,
  not a 6th binary.** The task's own in-scope file list names exactly 5
  binaries (`exp06/07/26/27/37`) — no `exp29_*.rs`. Doc 10 §36 asks for
  GPU/driver/kernel/GNOME/Mutter/virtual-monitor/capture/input/teardown
  results per GPU; this host only has Intel (`eDP-1`) and NVIDIA (`HDMI-1`,
  proprietary) — no AMD (assessment §11 item 3, `UNKNOWN` for v1, unchanged).
  The roadmap's own Phase 5 verify text ("matrix rows: internal panel (Intel),
  HDMI 4K (NVIDIA), both") is exactly Doc 10 §36's matrix in different words.
  Decision: record GPU/driver/kernel/Mutter-version plus per-row results as
  extra `Findings` fields inside `exp06`/`exp07`/`exp27`'s existing evidence
  structs for each of the three topology rows, rather than a separate binary.
- **Decision 6 — `exp37_privacy_check.rs` is semi-manual by design, not an
  automation gap.** Doc 10 §44 explicitly asks to "test using an independent
  camera/observer rather than the same software stack being evaluated" — a
  photo *is* the evidence; a self-check by the same D-Bus stack being tested
  would not satisfy the experiment's own stated purpose. Decision: `exp37`
  drives the isolate/verify-by-`GetCurrentState` steps and prints an explicit
  operator prompt (what to look at, what to photograph); the photo and the
  operator's reported observation (desktop visible? notifications visible?
  which of Doc 02 §13's three states — pixels not routed / panel standby /
  hardware message — occurred per output) are recorded as structured evidence
  fields the binary cannot assert on its own. The plan's Steps mark this
  explicitly as a manual operator action, not a `cargo test`-assertable one.
- **Decision 7 — resolve `experiment-safety.md` §3's VT-fallback question
  empirically, before any real isolate, via the lowest-risk mechanism
  available.** Repo memory flags this as open: does `Ctrl+Alt+F3` show
  anything on a DPMS-off/mode-disabled `eDP-1`, or would it be invisible too
  (making SSH the *only* trusted recovery path for `eDP-1` specifically, not
  just a backup to the VT fallback)? A full `ApplyMonitorsConfig` disable is
  not the lowest-risk way to test this. `api-inventory.md` confirms
  `DisplayConfig.PowerSaveMode` (`readwrite i`) exists — a much cheaper,
  reversible-by-property-write probe that plausibly exercises the same
  physical DPMS-off state without touching topology/logical-monitor state at
  all. Decision: probe `PowerSaveMode` first (its exact enum values confirmed
  from Mutter's own source/docs at implementation time, not assumed — Doc 20
  §7 again), have the operator physically try `Ctrl+Alt+F3` and report what
  is visible, record the result directly in `experiment-safety.md` §3 (an
  operational-safety fact, not a new conflict-register entry), and only then
  proceed to the first real `ApplyMonitorsConfig`-based isolate.
- **Decision 8 — risk-ascending order for the matrix rows and a bounded,
  machine-verified 50-cycle loop.** The roadmap lists matrix rows without
  specifying order; repo memory records the decisive host-topology fact:
  `eDP-1` (laptop internal panel) has **no** physical unplug/replug fallback
  if software restore misbehaves, `HDMI-1` does. Decision: test `HDMI-1` alone
  first (lowest risk, physical fallback exists, plus the deliberate
  ungraceful-termination/crash-recovery scenario for assessment §7.3 is run
  here first too), then `eDP-1` alone (only once the watchdog and VT-fallback
  groundwork are proven), then both together last (highest risk). Separately:
  "50 isolate/restore cycles clean" is a reliability criterion, verified via
  `GetCurrentState` + hash comparison per cycle (no human needed per cycle,
  mirroring Doc 19 §16–17/`experiment-safety.md` §6's existing bounded
  per-cycle-timeout convention from Phase 4's 50-cycle PipeWire test);
  operator photo evidence (Decision 6) is gathered for a small representative
  subset (once per matrix row = 3 photos), not for all 50 cycles — 50 real,
  operator-witnessed screen-blanking events on the operator's own daily-driver
  laptop would be a disproportionate, undue live-disruption burden the
  acceptance criteria do not actually require.
- **Decision 9 — no new `blackroom-core` error codes.**
  `ErrorCode::DisplayIsolationFailed` (already `Retryable::No`, explicit
  match arm) and `ErrorCode::DisplayRestoreFailed` (already present, defaults
  to `Retryable::No`) were both defined in Phase 2 and already cover every
  Phase 5 failure mode — `DisplayIsolationFailed`'s existing `No` retry
  semantics already match Doc 05 §62's "do not silently return to
  `LOCAL_ACTIVE` if physical state is unknown" requirement, even though
  wiring that into the real state machine stays a non-goal this phase.
  `error.rs` is reviewed, not edited.
- **Decision 10 — no shared-types refactor; duplication convention
  continues.** `blackroom-experiments/Cargo.toml` has **zero** path dependency
  on `blackroom-gnome` or `blackroom-core` today (confirmed by reading it) —
  experiments duplicate their own `GetCurrentState`/`ApplyMonitorsConfig`
  zvariant types per binary by established, deliberate precedent (`exp05`
  duplicated rather than importing from `virtual_monitor.rs`, even though both
  live in the same repo). `display_config.rs` (production) will define its own
  `DisplayBackup`/read-write types independently; `exp06/07/26/27/37`
  (experiments) will each define their own JSON-serializable equivalent
  locally, exactly like `exp05` did. Decision: keep this convention — no new
  cross-crate dependency, no shared-types extraction, even though 3 files
  would now have near-identical GetCurrentState types; that dedup refactor is
  out of scope for this phase (not requested, and touches already-shipped,
  reviewed Phase 3/4 files).
- **Decision 11 — explicitly out of scope despite being inside the read
  ranges.** Doc 05 §38 (HiDPI/scale factors) and §39–40 (cursor behaviour,
  which is Doc 10's separate Experiment 28) sit between §28 and §40 but are not
  among this task's cited experiments (6/7/26/27/29/37) — `capability-
  report.md` already defers GPU/cursor caveats to Phase 9/24, unchanged here.
  Doc 20 §21 (laptop lid-close) is named as its own non-goal (suspend-on-lid
  risk, orthogonal to Gate C). Doc 20 §24's broader restoration-trigger list
  (reboot, PipeWire failure, Mutter failure, emergency takeover, service
  restart) maps to Experiments 17/18/19/21–24, already roadmapped for Phase
  9/10 — not re-attempted here under Phase 5's narrower, already-large scope.
  All four are named explicitly in Non-Goals rather than silently absent.

## Risks

- **Operator must be physically present for the entire execution session** —
  arming/disarming the watchdog and confirming recovery over the out-of-band
  SSH channel (a *second* device, not the terminal running the experiment),
  physically pressing `Ctrl+Alt+F3`, and taking photos of the physical panels.
  This cannot be run unattended/overnight like Phase 0–1's read-only work; the
  Steps below flag every operator-required sub-action explicitly.
- **Mutter may simply refuse a zero-physical-monitor configuration**
  (`ApplyMonitorsConfigAllowed=false`, or the `ApplyMonitorsConfig` call itself
  errors) — this is an anticipated, acceptance-criteria-relevant *possible
  outcome*, not a bug to code around. If so: Gate C fails, and the Execution
  Log records a Doc 00 §49 / Doc 10 §49 stop-and-report. No workaround (black
  window, partial disable, weakened requirement) may be substituted.
- **The deliberate ungraceful-termination (SIGKILL) scenario for the
  crash-recovery cross-check is the single riskiest step in this plan** — its
  entire point is that the outcome (does Mutter auto-restore?) is unknown
  going in. It must only be attempted on the `HDMI-1`-alone row, with the
  watchdog armed and the operator standing by on the SSH channel, never on the
  `eDP-1`-alone or both-together rows first.
- **Topology-hash mismatches from legitimate Mutter nondeterminism** (e.g.
  mode IDs renumbered across calls) are possible; `exp05` already established
  that `mode_id` must be freshly cross-referenced from current state at
  restore time, never cached from the original snapshot — `exp07` must re-read
  fresh state rather than trust `exp06`'s persisted `mode_id` values verbatim
  for anything beyond identifying which mode to look up again.
- **NVIDIA hardware-cursor/explicit-sync issues could surface unprompted**
  during `HDMI-1` isolation even though cursor behaviour itself is out of
  scope (Decision 11); if a real, measured problem is observed, document it
  per Doc 05 §39 without applying `MUTTER_DEBUG_DISABLE_HW_CURSORS=1`
  speculatively.
- **This is the operator's actual daily-driver laptop, not a disposable test
  box** (assessment §8's own framing) — any Phase 5 misstep has real
  consequences. The risk-ascending row order and the VT-fallback-before-any-
  real-isolate sequencing (Decisions 7–8) exist specifically to bound this,
  not as arbitrary step ordering.
- **50 real isolate/restore cycles is still a tangible, repeated disruption**
  even bounded to machine verification (Decision 8) — each cycle briefly
  changes the operator's own active display topology; confirm the operator is
  not mid-task on the affected outputs before starting the loop.

## Steps

- [x] 1. Resolve `docs/ops/experiment-safety.md` open safety items before any
      physical-output mutation: confirm §1 (SSH) needs no re-verification;
      document the §2 watchdog's concrete mechanism
      (`systemd-run --user --on-active=45s <restore-command>`, default
      `N = 45s`, matching the doc's existing text); empirically resolve §3's
      VT-fallback question via the `DisplayConfig.PowerSaveMode` probe
      (Decision 7) — **operator-required**: operator physically presses
      `Ctrl+Alt+F3` and reports what is visible on `eDP-1`; record the result
      inline in §3. Confirm §5 (mask `gnome-remote-desktop`)/§6 (bounded
      per-cycle timeout) continuity from Phase 4.
  - Files: `docs/ops/experiment-safety.md`
  - Depends on: none
  - Verify: doc updated with a dated "Status as of 2026-09-05" entry under §3
    recording the operator's direct observation; no other section's meaning
    changed.
- [x] 2. Implement `crates/blackroom-gnome/src/mutter/display_config.rs`:
      `DisplayBackup`/`OutputBackup`/`LogicalMonitorBackup` types (Decision 3),
      a snapshot function (`GetCurrentState` → `DisplayBackup`, also the first
      real candidate for the trait's `get_display_state` shape), a
      `disable_physical_outputs`-shaped function (checks
      `ApplyMonitorsConfigAllowed` first per Doc 05 §30, builds a
      zero-physical logical-monitors array from an already-created virtual
      monitor's connector, applies `Temporary`), a
      `restore_physical_outputs`-shaped function (idempotent, re-applies a
      `DisplayBackup`, verifies by hash + field comparison), and a
      `MonitorsChanged` signal-subscription helper (Decision 4). Register the
      module in `mutter/mod.rs`. Unit-test the pure logic (hash computation,
      enabled/disabled derivation, backup-to-write-side reconstruction) with
      injected fixtures, matching `capability.rs`'s existing test style.
  - Files: `crates/blackroom-gnome/src/mutter/display_config.rs`,
    `crates/blackroom-gnome/src/mutter/mod.rs`
  - Depends on: step 1 (safety groundwork must exist first)
  - Verify: `cargo test -p blackroom-gnome` green; `cargo clippy -p
    blackroom-gnome --all-targets -- -D warnings` clean.
- [x] 3. Implement `crates/blackroom-experiments/src/bin/exp06_isolate_outputs.rs`
      (Decision 1): snapshot + persist `DisplayBackup` JSON to its own evidence
      dir; create+confirm a virtual monitor (duplicated exp05-style inline
      logic, per Decision 10 — no import from `virtual_monitor.rs`); check
      `ApplyMonitorsConfigAllowed`; arm the watchdog inline (not extracted to
      the shared library, per Decision 1); build and apply the zero-physical
      logical-monitors array; verify every original connector is present in
      `monitors[]` but absent from every `logical_monitors[].monitors[]`
      entry; support a `--cycles=N` flag for the bounded-timeout reliability
      loop (Decision 8) and a `--kill-before-restore` mode for the deliberate
      ungraceful-termination scenario.
  - Files: `crates/blackroom-experiments/src/bin/exp06_isolate_outputs.rs`
  - Depends on: step 2
  - Verify: `cargo build -p blackroom-experiments --bin exp06_isolate_outputs`
    clean; `cargo clippy` clean. No live run yet.
- [x] 4. Implement `crates/blackroom-experiments/src/bin/exp07_restore.rs`
      (Decision 1): standalone binary, loads a `DisplayBackup` JSON (path
      argument, default = most recent `exp06` evidence directory),
      independently re-applies + verifies by hash and field comparison, polls
      until the virtual connector is fully gone (mirroring `exp05`'s
      `poll_for_connector_gone` precedent), and implements Doc 05 §62's
      restoration-failure handling explicitly in its `Findings`/report (retry
      once safely; never claim success if physical state is unknown; report
      diagnostic state).
  - Files: `crates/blackroom-experiments/src/bin/exp07_restore.rs`
  - Depends on: step 2
  - Verify: `cargo build -p blackroom-experiments --bin exp07_restore` clean;
    `cargo clippy` clean. No live run yet.
- [x] 5. Live-run `exp06`+`exp07` for the `HDMI-1`-alone row first (Decision 8,
      lowest risk) — **operator-required** throughout: confirm recovery over
      the SSH channel after the first normal cycle; then run the deliberate
      `--kill-before-restore` scenario once (watchdog armed, operator standing
      by on SSH), observe via a plain `GetCurrentState` read what Mutter did
      automatically, then run `exp07` fresh to restore and verify — this
      directly answers assessment §7.3's crash-recovery question with
      evidence.
  - Files: `docs/experiments/evidence/exp06/`, `docs/experiments/evidence/exp07/`
  - Depends on: steps 3, 4
  - Verify: normal restore and independent exp07 report PASS (or record
    FAIL/BLOCKED and root cause). For intentional SIGKILL, exp06 necessarily
    ends with a PARTIAL pre-kill report; independent journal/GetCurrentState
    and exp07 evidence must establish the post-kill outcome. Record the
    privacy observation separately; this step does not promote Gate FEAS-C.
- [ ] 6. Live-run `exp06`+`exp07` for the `eDP-1`-alone row — **operator-
      required** — only after step 5 is fully green and the watchdog has been
      proven to fire correctly at least once.
  - Files: `docs/experiments/evidence/exp06/`, `docs/experiments/evidence/exp07/`
  - Depends on: step 5
  - Verify: same as step 5; additionally confirm `Ctrl+Alt+F3` behaves as step
    1 predicted (or record the discrepancy).
- [ ] 7. Live-run `exp06`+`exp07` for the both-together row (highest risk,
      last) — **operator-required**.
  - Files: `docs/experiments/evidence/exp06/`, `docs/experiments/evidence/exp07/`
  - Depends on: step 6
  - Verify: same as step 5.
- [ ] 8. Implement and live-run `exp26_hotplug.rs` (Decision 4): subscribe to
      `MonitorsChanged` before triggering; **operator-required**: physically
      disconnect/reconnect `HDMI-1` during isolation. Document `eDP-1`
      hotplug and dock/undock as not-applicable (no hardware) rather than
      omitting them silently.
  - Files: `crates/blackroom-experiments/src/bin/exp26_hotplug.rs`,
    `docs/experiments/evidence/exp26/`
  - Depends on: step 5
  - Verify: `report.md` shows the reconnected output stayed isolated (or
    triggered documented safe teardown) and that restoration still succeeded
    afterward.
- [ ] 9. Implement and live-run `exp27_modes.rs` (Doc 10 §34): resolution/
      refresh-rate matrix (1920×1080, 2560×1440, 3840×2160 @ 60 Hz where
      supported; high-refresh where available) across the three topology
      rows, folding in Decision 5's GPU-matrix fields per row. Uses a smaller
      per-mode cycle count than step 10's dedicated reliability loop (matrix
      breadth over repetition depth).
  - Files: `crates/blackroom-experiments/src/bin/exp27_modes.rs`,
    `docs/experiments/evidence/exp27/`
  - Depends on: step 7
  - Verify: `report.md` records creation time/stability/capture quality/
    teardown/restoration reliability per Doc 10 §34's exact field list, for
    every mode actually supported (unsupported modes documented, not silently
    dropped).
- [ ] 10. Run the 50-cycle bounded-timeout reliability loop via `exp06
      --cycles=50` on one representative row/resolution (Decision 8) —
      machine-verified via `GetCurrentState` + hash per cycle, no per-cycle
      operator photo.
  - Files: `docs/experiments/evidence/exp06/`
  - Depends on: step 7
  - Verify: 50/50 cycles report `topology_restored=true`, 0 cycles exceed the
    per-cycle timeout, 0 leaked virtual connectors.
- [ ] 11. Implement and live-run `exp37_privacy_check.rs` (Decision 6):
      **operator-required** photo evidence for a representative subset (once
      per matrix row = 3 photos, not all 50 cycles), recording which of Doc 02
      §13's three states occurred per output.
  - Files: `crates/blackroom-experiments/src/bin/exp37_privacy_check.rs`,
    `docs/experiments/evidence/exp37/`
  - Depends on: step 7
  - Verify: `report.md` links the photo evidence file(s) and records the
    operator's explicit observation per output; no bare `PASS` without it.
- [ ] 12. Update `docs/gnome/capability-report.md` (resolve `VIRTUAL_DISPLAY_
      CAPABLE`/`DISPLAY_CONFIG_CAPABLE`'s open "all-physical-disabled" notes)
      and add `docs/gnome/display-isolation.md` (Phase 4's `virtual-display.md`
      precedent) recording the findings and both explicit decisions (1 and 2)
      from this plan. Update `crates/blackroom-gnome/src/mutter/capability.rs`
      to promote `virtual_display_capable`/`display_config_capable` per the
      actual evidence gathered in steps 5–11 (promotion is a decision rule
      applied to real results, not asserted here — if any matrix row failed
      outright, the affected constant is not promoted and the stop-condition
      report is cited instead).
  - Files: `docs/gnome/capability-report.md`, `docs/gnome/display-isolation.md`,
    `crates/blackroom-gnome/src/mutter/capability.rs`
  - Depends on: steps 5–11
  - Verify: `cargo test -p blackroom-gnome` green (capability tier tests
    updated to match); doc content matches the actual recorded evidence, not
    a template assumption.
- [ ] 13. Final verification: full workspace checks; `Blackroom_Console`
      re-indexed in codebase-memory; independent Reviewer-subagent pass;
      Execution Log entry recording Gate **FEAS-C** PASS or a Doc 00 §49
      stop-and-report.
  - Files: (none — verification only)
  - Depends on: step 12
  - Verify: `cargo test --workspace && cargo fmt --check && cargo clippy
    --workspace --all-targets -- -D warnings && cargo check --workspace
    --all-targets && cargo deny check && cargo audit` all green.
- [ ] 14. Update `docs/HANDOFF.md`, `/memories/repo/blackroom-console.md`, and
      checkpoint MemPalace (wing `blackroom_console`) with this phase's
      findings and the two explicit decisions, per `docs/MEMORY_PROTOCOL.md`'s
      session-end protocol.
  - Files: `docs/HANDOFF.md`
  - Depends on: step 13
  - Verify: `docs/HANDOFF.md` stays ≤ 40 lines / 3 KB (project doctor check).

## Final Verification

- Run the configured project checks from `AGENTS.md`/`docs/WORKFLOW_CONFIG.md`:
  `cargo test --workspace`, `cargo fmt --check`, `cargo clippy --workspace
  --all-targets -- -D warnings`, `cargo check --workspace --all-targets`,
  `cargo deny check`, `cargo audit`.
- Confirm every acceptance criterion above against the actual `report.md`
  evidence produced in steps 5–11, not against this plan's expectations.
- Confirm Gate **FEAS-C**'s verdict is recorded plainly in the Execution Log —
  PASS with evidence, or a Doc 00 §49 stop-and-report — before Phase 6 planning
  begins.

## Blockers

- Live Phase 5 work is stopped under Doc 00 §49 / Doc 10 §49 after the
  reported GNOME Shell crash. The trigger and abnormal-termination restore
  outcome are unknown. The separately approved 2026-09-27 eDP-only diagnostic
  found a new restoration failure: HDMI-1 became active after exp06 stopped
  Meta-0, despite the watchdog's earlier exp07 PASS. A second guarded exp07
  restored eDP-only, but this violates stable restoration; stop further
  isolation under the same gate until cleanup/final-topology verification is
  reassessed. No other rows or product activation are authorized.

## Execution Log

- 2026-09-05: Steps 2–4 implemented (code only, no live Mutter mutation).
  `display_config.rs` landed with `DisplayBackup`/`OutputBackup`/
  `LogicalMonitorBackup`, `snapshot`/`disable_physical_outputs`/
  `restore_physical_outputs`/`verify_restored`/`wait_for_monitors_changed`,
  8 unit tests (hash order-independence, enabled/disabled derivation, primary
  detection, hash excludes timestamp/session, write-side reconstruction incl.
  fail-closed on an inconsistent backup, zero-physical-config shape) — all
  green. `exp06_isolate_outputs.rs`/`exp07_restore.rs` implemented per
  Decision 1 (two independent binaries, JSON-persisted `DisplayBackup`,
  `exp06` supports `--cycles`/`--pause-after-isolate` for the reliability
  loop and the deliberate ungraceful-termination scenario; `exp07` requires
  an explicit `--backup` path, no auto-discovery, on purpose). `mod.rs`
  registers the new module; `experiment-safety.md` §2 now documents the
  concrete implemented watchdog command; §3 records the VT-fallback question
  as still pending empirical resolution. Full workspace green: `cargo test
  --workspace`, `cargo fmt --check`, `cargo clippy --workspace --all-targets
  -- -D warnings`, `cargo check --workspace --all-targets`, `cargo deny
  check` (advisories/bans/licenses/sources ok), `cargo audit` (201 deps, 0
  vulnerabilities — unchanged from Phase 4, no new dependency added).
  **Step 1's live part (operator presses `Ctrl+Alt+F3` during a
  `PowerSaveMode` probe) and steps 5–14 (every live Mutter mutation) are NOT
  done** — they require the operator's real-time physical presence and were
  deliberately not started without an explicit go-ahead (see Risks: this is
  the first phase that mutates the operator's own live display).
- 2026-09-05 (later same day): operator confirmed VM stopped; re-checked
  memory (22GiB available, up from 5.8GiB; 0 qemu processes) before any live
  action. Ran step 1's live VT-fallback probe: read `PowerSaveMode` (`0`),
  set to `3` (DPMS off, standard value, armed with a 30s/60s throwaway
  auto-restore each time — not the real exp06 watchdog, a smaller one-off
  safety net for this probe only). Both panels visibly blanked; mouse
  movement did NOT wake them; `Ctrl+Alt+F3` showed a real, visible `tty3`
  login console (operator-confirmed directly); `Ctrl+Alt+F2` returned
  cleanly, `gnome-shell` PID unchanged (no crash). Recorded in
  `experiment-safety.md` §3 with the scope caveat that this tested
  `PowerSaveMode`/DPMS, not yet the real `ApplyMonitorsConfig` zero-physical
  mechanism — expected to generalize (VT-switch is below the compositor)
  but not independently proven for Experiment 6's exact mechanism yet. Step
  1 now fully complete. Steps 5–14 still not started; re-evaluating live
  matrix-row order before the first real `ApplyMonitorsConfig` disable (see
  next Decision to be recorded once agreed with the operator).
- 2026-09-05 (first real Experiment 6 run — INCIDENT, root-caused, fixed):
  proposed and ran "both together" first instead of the plan's literal
  `HDMI-1`-alone-first order, because a single-output row requires manually
  disabling the other output via GNOME Settings first, which sacrifices a
  passive-fallback screen before the real mechanism was ever proven —
  operator agreed. Ran `exp06_isolate_outputs --pause-after-isolate` live:
  `apply_monitors_config_allowed: true` and the zero-physical apply
  succeeded — **directly answers assessment §7.3's "does Mutter accept
  zero physical monitors" question: yes.** `Ctrl+Alt+F3` again showed a
  visible `tty3` console under this real mechanism too. **`Ctrl+Alt+F2` did
  NOT restore the GUI this time** (unlike the `PowerSaveMode` probe) —
  correctly explained by the fact that Mutter's own compositor config
  genuinely had zero physical monitors active, so regaining its VT does not
  make it display something it believes should not be displayed; a real
  `ApplyMonitorsConfig` restore is required, not a VT switch. The operator
  had to restore manually over SSH from the tablet (correctly identifying
  they could equally have run it from the `tty3` shell directly). Root
  cause: TWO real bugs in the safety nets meant to handle exactly this,
  found immediately afterward by reading the code and the watchdog's own
  `journalctl` log: (1) `exp06`'s `--pause-after-isolate` branch never
  actually called restore after the operator's Enter — it fell through, and
  `all_restored`'s unmutated `true` default wrongly disarmed `RestoreGuard`
  too; (2) the watchdog's `systemd-run`-launched restore command was passed
  a **relative** `--backup` path, which fails once the transient unit runs
  in a different working directory (`journalctl --user -u
  blackroom-exp06-watchdog-*.service` showed `Error: No such file or
  directory (os error 2)`, unit status `FAILURE`). Both fixed same session:
  `backup_path` made absolute via `std::path::absolute` at construction
  (fixes both the printed operator instructions and the watchdog argument);
  the pause branch now actually calls `apply_monitors_config` with
  `original_write_side` and verifies via the same
  `physical_connectors_active`-count check `run_one_cycle` uses, setting
  `all_restored` correctly instead of leaving it at its default. Full
  workspace re-verified green after the fix (build/clippy/fmt/test).
  Post-incident health check: `gnome-shell` same PID throughout (no crash),
  memory stable (22GiB available), topology confirmed fully restored
  (`eDP-1`+`HDMI-1` both present in `monitors[]` and `logical_monitors[]`,
  virtual connector `Meta-0` fully gone). LESSON: the operator's manual
  SSH-based recovery was not a backup precaution that happened to be
  unnecessary — it was the ONLY thing that actually worked; both automated
  restore paths had real bugs live testing caught immediately. Do not trust
  a safety-net code path just because it looks right on read-through;
  Doc 00 §49/Doc 10 §49's "restoration is unreliable" stop condition was
  seriously in play here, but root-caused to fixable harness bugs, not a
  fundamental Mutter/GNOME limitation (the underlying zero-physical
  mechanism itself worked correctly both times it was actually invoked
  correctly). Not yet re-verified live after the fix — see Next Actions.
- 2026-09-05 (fix re-verified live, same day): fresh isolate run, watchdog
  window (45s) deliberately left untouched (no Enter). Watchdog fired
  **completely unattended** and restored correctly — `journalctl` showed
  `exp07_restore` invoked with the correct absolute path this time,
  `Result: PASS`; independently confirmed via a fresh `GetCurrentState`
  read (`eDP-1`+`HDMI-1` both active) and `gnome-shell` health (same PID).
  One loose end: `Meta-0` (virtual monitor) still present in the raw
  inventory since its owning process (still paused) had not yet stopped its
  `ScreenCast` session. Sent Enter to that stale process: it correctly
  performed a harmless redundant re-apply (state already restored) then
  cleaned up `Meta-0` (`virtual_connector_fully_gone: true`). Final state
  fully clean. `exp07_restore`'s own evidence (from the watchdog's
  invocation) correctly flagged `Meta-0` as an "unexpected connector" both
  before and immediately after its own restore — expected, since restoring
  logical topology doesn't stop a `ScreenCast` session it doesn't own.
  **Both bugs now considered genuinely fixed and live-proven, not just
  code-reviewed.** `experiment-safety.md` §2 updated to reflect the
  watchdog is now trustworthy. Not yet done: a dedicated clean test of
  "Enter pressed while still isolated" (today's Enter-path exercise was a
  redundant re-apply after the watchdog had already restored, not a first
  restore) — optional follow-up, deferred pending operator preference,
  since the higher-priority zero-operator-action watchdog path is now fully
  proven.
- 2026-09-05 (Gate-C-relevant finding — `eDP-1` frozen-frame, fix
  implemented, NOT yet live-tested): operator reported, both live isolate
  runs, that `eDP-1` did not blank when disabled — it showed a **frozen,
  fully visible last frame** (a mid-reflow, partially-cropped VS Code
  window from GNOME reassigning windows off the disappearing physical
  monitors onto the sole remaining virtual one). `HDMI-1` appeared to blank
  correctly, but only because that monitor's own firmware independently
  powers down on "no signal" — masking whether the same underlying issue
  would occur on it too. Root cause: `ApplyMonitorsConfig` alone only
  changes the *logical* topology; it does not guarantee the physical panel
  stops displaying whatever it last received (exactly Doc 05 §34's
  caveat). **This directly fails Gate C's actual requirement** ("the active
  desktop must not be visible") — a frozen frame is still visible desktop
  content, and this failure mode is invisible to `GetCurrentState`-only
  verification; only Exp 37's human/photo check caught it, validating that
  design decision precisely. Fix implemented (not yet live-tested, per
  operator instruction): `disable_physical_outputs`/`restore_physical_
  outputs` (`display_config.rs`) and the matching `exp06`/`exp07` code now
  pair every `ApplyMonitorsConfig` disable/restore with an explicit
  `DisplayConfig.PowerSaveMode` blank/un-blank (already proven live twice to
  force a real hardware blank independent of logical topology). Ordering:
  blank **before** changing topology on disable (never let the reflow
  transition become visible at all), restore topology **before** un-blanking
  on restore (never reveal a transitional frame on the way back); best-effort
  un-blank if the disable's topology change itself fails, so a failure never
  leaves the panel dark with an unchanged, fully-active topology. Full
  workspace re-verified green (build/clippy/fmt/test) — no live run
  performed. Next: live-test this fix, ideally isolating `eDP-1` alone
  (disable `HDMI-1` via GNOME Settings first) so `HDMI-1`'s own auto-sleep
  can't mask whether the fix actually resolves the freeze.
- 2026-09-05 (fix live-verified, `eDP-1` alone): operator physically
  unplugged `HDMI-1` (confirmed via `GetCurrentState`: absent from
  `logical_monitors[]`, only `eDP-1` active). Ran `exp06
  --pause-after-isolate` with the fix. Operator confirmed `eDP-1` now goes
  cleanly **blank/black**, not frozen. Restore independently verified after
  Enter: topology back to `eDP-1` alone, `PowerSaveMode` back to `0` (ON),
  `gnome-shell` healthy, `Meta-0` fully cleaned up. **The frozen-frame Gate
  C finding is resolved for `eDP-1`.** Minor non-urgent observation:
  `disarm_watchdog()` targets the watchdog's `.service` unit, which is not
  instantiated until the `.timer` actually fires — a very-fast graceful
  restore (well inside the 45s window) may not actually cancel the pending
  timer, only harmlessly no-op on a not-yet-existing unit; a later
  redundant watchdog fire is idempotent and safe, just imprecise. Not fixed
  yet, not urgent.
- Next: resume the matrix (both-together clean pass → `HDMI-1`
  alone → `eDP-1` alone, risk-ascending order per Decision 8) once the
  operator confirms readiness to continue — `eDP-1` alone is effectively
  done via the fix verification above; `HDMI-1` alone and both-together
  with the fix still remain.
- 2026-09-06 (both-together row, Gate-C fix re-verified live): `HDMI-1`
  replugged, both displays joined via Super+P (confirmed live: GNOME's
  `switch-monitor` keybinding, the Linux equivalent of Windows' Win+P,
  applies through the same `ApplyMonitorsConfig` call `exp06` uses). Ran
  `exp06 --pause-after-isolate` live: isolate succeeded; both `eDP-1` and
  `HDMI-1` confirmed by direct operator observation to go cleanly
  blank/black, not frozen — the Gate-C fix holds on both-together too.
  `Ctrl+Alt+F3` again showed `tty3`; `Ctrl+Alt+F2` again did not return to
  the GUI until the real topology was restored (consistent with the
  earlier both-together finding). The 45s watchdog fired unattended and
  restored correctly — independently confirmed via `journalctl`
  (`exp07_restore` `Result: PASS`) and a fresh `GetCurrentState` (both
  real outputs back, `gnome-shell` same PID, no crash). Enter was then
  sent to the still-paused `exp06` process for its own cleanup
  (`ScreenCast` session stopped, `Meta-0` fully gone) — its own result was
  `PARTIAL` by design (pause-mode always defers restore certification to
  `exp07`), not a failure.
- 2026-09-06 (evidence-misplacement bug found + fixed while verifying the
  above): the watchdog-triggered `exp07_restore`'s evidence write was
  traced to `$HOME/docs/experiments/evidence/exp07/2026-09-05/` instead of
  the repo. Root cause: `arm_watchdog()`'s `systemd-run --user` call never
  set `--working-directory`, so the transient unit's cwd defaulted to
  `$HOME`, and `evidence_dir()`'s relative path resolved against that
  instead of the repo root. Confirmed via the stray directory's content
  (matched `journalctl`'s timestamp and `Result: PASS` exactly, just in
  the wrong location) — the restore mechanism itself was unaffected, an
  audit-trail-only bug. Every prior watchdog-triggered restore's evidence
  was silently misplaced the same way and is not recoverable (overwritten
  by this run). Fixed by passing `--working-directory=<cwd>` (`systemd-run
  --help` confirmed the flag live). Today's both-together evidence
  (`exp06`'s isolate side plus the recovered `exp07` restore side)
  archived to `docs/experiments/evidence/{exp06,exp07}/
  2026-09-05-both-together/` ahead of being superseded by the next run.
  Full workspace re-verified green after the fix (`cargo test/fmt/clippy/
  check`, 0 new deps; `deny`/`audit` not re-run this session).
- Next: `HDMI-1`-alone row — routine isolate/restore check, then the
  still-outstanding deliberate `--kill-before-restore` crash-recovery
  scenario (Decision 8: must land on this row only, never `eDP-1`-alone or
  both-together). Then steps 8–14.
- 2026-09-26 (resume of 2026-09-06 session): `HDMI-1`-alone routine
  isolation was operator-observed blank, and the watchdog restored before
  the proposed deliberate kill. The date-keyed exp06 report/backup now show
  both outputs active before isolation, while exp07's report records an
  HDMI-only backup; the earlier isolate evidence was overwritten, so these
  files do not form a matching evidence pair for this row. Subsequently the
  operator reported a blank screen, a third display in Settings, and a
  logout. The prior session reported GNOME Shell SIGSEGV from journal and
  apport near virtual-monitor removal, but ended without an answer to its
  question of whether exp06 had been killed or the terminal closed first.
  The historical journal and crash artifact are not available on resume;
  the current GNOME Shell is running. **Step 5 remains open; no crash-recovery
  PASS or causal attribution is established.** Pause live mutations pending
  the operator's sequence of events and a stop-condition assessment; do not
  rerun the kill scenario or proceed to steps 8–11 on this daily-driver host
  by assuming the crash was harmless.
- 2026-09-26 (operator clarification): desktop is usable on the built-in
  panel only; operator no longer recalls whether exp06 was killed/its
  terminal closed before the logout. The sequence cannot be established
  from the current evidence. Keep live mutations paused and decide the
  stop condition before proposing any new recovery experiment.
- 2026-09-26 (stop-and-report assessment): Doc 00 §49 and Doc 10 §49 both
  require a stop if Mutter becomes unstable; the reported GNOME Shell
  SIGSEGV and fresh login during the Phase 5 work meet the threshold for
  stopping further live experiments on this daily-driver host. The prior
  session reported seeing the crash in journal and apport, but neither
  historical artifact is available now; whether exp06 was killed first,
  whether our code caused the crash, and whether abnormal termination
  restores the original topology remain **unknown**. The observed blanking
  on the matrix rows does not prove the full Gate FEAS-C restoration and
  reliability criteria: **no PASS verdict**. Steps 5 and 8–13 remain open;
  Phase 6/product implementation must not proceed on an assumed Gate C.
  Reassess architecture and investigate the compositor crash from retained
  diagnostics or on a separate prepared host before proposing any new live
  experiment, with an explicit safety review and operator approval. No
  black-window substitute, automatic retry, or capability-tier promotion.
- 2026-09-26 (local checkpoint): `cargo test --workspace`, `cargo fmt
  --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo check --workspace --all-targets`, `cargo deny check`, and
  `cargo audit` passed. Real-GNOME system tests remained ignored. These
  static/unit checks do not discharge the stop condition or Gate FEAS-C.
- 2026-09-26 (offline harness hardening; no live GNOME tests): reserve each
  experiment run's evidence directory atomically; preserve existing
  date-keyed reports with numbered same-day siblings and record the actual
  path in exp06/exp07 reports. Refuse to isolate outputs if the watchdog
  cannot arm, and cancel its pending timer (not its inactive service) after
  verified restoration. This prevents recurrence of the documented evidence
  overwrite and closes two harness safety gaps; it does not explain the
  reported compositor crash or lift the stop condition. The retained journal
  starts 2026-09-13, after the incident, so the prior crash timeline still
  cannot be independently reconstructed from this host's current logs.
  `cargo test --workspace`, `cargo fmt --check`, `cargo clippy --workspace
  --all-targets -- -D warnings`, `cargo check --workspace --all-targets`,
  and the project doctor passed; real-GNOME tests remained ignored. No new
  dependency was added (deny/audit last passed at the preceding checkpoint).
- 2026-09-26 (operator returned; supervised reassessment proposed, not run):
  no separate GNOME host was created in earlier sessions. Operator is at
  the workstation with second-device SSH ready and accepts the possibility
  of a crash on the daily-driver laptop. `ssh.socket` is active;
  `gnome-remote-desktop.service` is disabled/inactive; no exp06 timer is
  pending. Existing exp04 `--skip-cycles` still tests three virtual-monitor
  sizes, but leaves physical outputs enabled. The recovery branches and
  limits are now written in `docs/ops/experiment-safety.md` §7. No GNOME
  mutation, SIGKILL, Gate FEAS-C reclassification, or new live-test approval
  has yet occurred; obtain approval for the exact diagnostic command first.
- 2026-09-26 (bounded diagnostic approval): operator confirmed a fresh
  second-device SSH login works and explicitly approved **one**
  `exp04_virtual_monitor --skip-cycles` run on this laptop after reviewing
  §7's recovery branches and possible GNOME crash/logout. This approval
  excludes exp06, physical-output isolation, SIGKILL, and any repeat run.
  A passing exp04 result will not lift the Phase 5 stop or prove Gate FEAS-C.
- 2026-09-26 (single approved diagnostic completed): exp04 returned `PASS`;
  all three virtual monitors (1280x720, 1920x1080, 2560x1440) were detected,
  yielded two frames each, and reported clean Stop/removal. Independently,
  Shell PID stayed `34735`, journal showed three normal Meta-0 removals with
  no matched crash signature, `PowerSaveMode=0`, and no Meta connector was
  found in a fresh GetCurrentState. The competing service was unmasked back
  to its prior disabled/inactive state. **Qualification:** `--skip-cycles`
  ran zero reliability cycles, but exp04's old code printed `cycles_clean`,
  `no_leaked_nodes`, and `shell_survived` as `true` from empty/None inputs;
  the generated report's PASS must therefore be read as a partial
  three-resolution observation, not a 50-cycle reliability pass. The
  separate PID check above did verify Shell continuity for this one run.
  Source now represents unmeasured fields as None and reserves PASS for
  full measured reliability; a focused regression test passed. Original
  generated evidence was retained unchanged. This run neither reproduced
  the earlier crash nor proves abrupt-session loss or zero-physical display
  teardown safe. **Phase 5 stop and Gate FEAS-C unproven status remain.**
- 2026-09-26 (owner-loss diagnostic design, offline only): the operator chose
  to **prepare**, not run, a single non-isolating owner-loss probe next.
  zbus 5.19's blocking `Connection::close(self)` explicitly closes its
  D-Bus socket; unlike SIGKILL, the diagnostic process can persist evidence
  first. Proposed bounded mode: create and confirm one virtual monitor at
  1280x720 with physical outputs still active, keep a Stop guard armed for
  every early failure, persist a pre-close report, then intentionally skip
  `ScreenCast.Session.Stop` and close only that session-owner connection.
  Independent read-only GetCurrentState, Shell PID and journal checks are
  required afterward; no PASS can be asserted solely by the probe. This
  tests owner disappearance without zero-physical topology but could still
  crash GNOME. **No live invocation, retry, exp06, or SIGKILL is approved.**
- 2026-09-26 (offline preparation): implemented the explicit exp04
  `--probe-owner-loss` mode, exclusive with `--skip-cycles`. It requires
  active physical display state with no pre-existing Meta connector, one
  confirmed 1280x720 virtual connector and captured frames, and a Shell PID
  baseline. The Stop guard stays armed through the pre-close evidence write;
  after that write succeeds, the mode disarms it and calls zbus 5.19's
  `Connection::close(self)` on its own session-bus connection. It reports
  PARTIAL and leaves removal/crash verification to an independent observer.
  Direct Stop-on-write-failure behavior has not been exercised with a live
  D-Bus session; source ordering and offline CLI/compile checks are not proof
  of runtime recovery. No live owner-loss probe has been run or approved.
- 2026-09-26 (bounded owner-loss approval): operator again confirmed being
  at the workstation with a working fresh second-device SSH session and
  explicitly approved **one** `exp04_virtual_monitor --probe-owner-loss`
  run. This closes one D-Bus session-owner connection after persisting
  pre-close evidence, with physical displays left active; GNOME Shell may
  crash/logout. Approval excludes exp06, display isolation, SIGKILL, and
  repeat runs. Follow `docs/ops/experiment-safety.md` §7 on any failure.
- 2026-09-26 (single owner-loss probe completed): pre-close PARTIAL evidence
  at `docs/experiments/evidence/exp04/2026-09-26-2/` records a confirmed
  1280x720 Meta-0 virtual monitor and two frames. The probe explicitly
  closed its own D-Bus connection without `ScreenCast.Session.Stop`.
  Independent journal shows both `Removed virtual monitor Meta-0` and
  `D-Bus client with active sessions vanished` at 22:38:38 local time;
  GNOME Shell PID stayed 34735, a fresh GetCurrentState call succeeded
  without Meta, PowerSaveMode remained 0, and the operator confirmed the
  built-in desktop visible and responsive. `gnome-remote-desktop` was
  restored to disabled/inactive. This **does not reproduce** the earlier
  crash in the non-isolating configuration, but neither establishes
  reliability nor rules out zero-physical/hybrid-GPU teardown as the
  trigger. Gate FEAS-C and the Phase 5 stop remain unresolved. The one-run
  approval has been used; exp06 and any repeat require separate review.
- 2026-09-26 (HDMI-only routine test review, no isolation): operator
  reconnected the external display and selected second-screen-only;
  reported HDMI desktop visible and built-in panel off. Read-only exp02
  inventory at `docs/experiments/evidence/exp02/2026-09-26/` confirms raw
  connectors `eDP-1` and `HDMI-1`, with only `HDMI-1` in logical monitors
  (primary, scale 1.25). **Evidence limitation:** exp02's executable was
  still the 2026-09-05 build, predating the per-run evidence-directory fix;
  its second invocation reused the date path and overwrote the earlier
  built-in-only snapshot. That first result was seen in the session but no
  longer exists as a file. Rebuilt exp02 against the current helper without
  another inventory run; restored historical `api-inventory.md` content
  after the generator's incidental session-ID/topology rewrite. Current
  HDMI-only evidence is valid; no exp06, SIGKILL, or further physical-state
  mutation has been approved or performed by this review.
- 2026-09-26 (routine HDMI-only path reviewed, not run): the standalone
  exp06/exp07 binaries dated September 5/6, before current watchdog/evidence
  fixes; both were rebuilt from current source. Proposed one-run sequence:
  with HDMI-1 the only active display and fresh second-device SSH, mask the
  competing service, start `exp06 --pause-after-isolate`, observe external
  blanking, leave its process untouched until the 45-second watchdog fires,
  independently confirm exp07's restoration and the physical desktop over
  SSH, then send Enter to the still-paused exp06 for ScreenCast cleanup.
  Do not choose its printed `kill -9` suggestion. Exp06's normal graceful
  branch can disarm its watchdog after only an internal topology check,
  before out-of-band confirmation (experiment-safety §2); this procedure
  instead exercises the unattended watchdog. This remains a harness gap
  for future general use. It does not test abrupt owner loss under
  zero-physical topology or resolve the earlier crash. No live exp06 run,
  SIGKILL, or Phase 5 gate promotion is approved by this review.
- 2026-09-26 (bounded routine-isolation approval): operator reconfirmed
  HDMI is the only visible desktop, is physically present, and a fresh
  second-device SSH login works. Explicit approval covers **one**
  `exp06_isolate_outputs --pause-after-isolate` run with the 45-second
  watchdog left to restore the HDMI-only topology. No Enter or process
  termination while isolated; SSH and operator independently verify
  restoration before Enter cleans up the original ScreenCast session.
  The operator accepts possible Shell crash/logout and reviewed the §7
  recovery procedure. This approval excludes SIGKILL, owner-loss under
  zero-physical topology, repeat runs, and any Gate FEAS-C promotion.
- 2026-09-26 (one supervised HDMI-only routine run completed): exp06
  `--pause-after-isolate` armed the 45-second timer, persisted its backup,
  and isolated the sole active HDMI-1 output. Operator saw HDMI cleanly
  black/blank with no desktop; the already-disabled built-in panel lit up
  but stayed blank. Operator switched VT with Ctrl+Alt+F2 and later saw the
  GUI again on HDMI; the watchdog journal independently shows exp07 started
  at 22:51:47 local and reported PASS at 22:51:48, so the VT switch alone
  is **not** credited with restoring Mutter's topology. Exp07 found the
  backed-up HDMI-only topology matching with no retry or apply error;
  its `unexpected_connectors_after_restore` still included eDP-1 (disabled)
  and Meta-0 (exp06 still paused), as expected. After SSH verification,
  Enter let exp06 redundantly re-apply its backup and stop ScreenCast;
  its pause-mode PARTIAL report records `stop_error=None` and
  `virtual_connector_fully_gone=true`. The expired timer was no longer
  loaded when exp06 tried to stop it, producing a harmless unit-not-loaded
  warning. Independent checks: Shell PID 34735 throughout, PowerSaveMode 0,
  no Meta connector/timer remained, `gnome-remote-desktop` restored to its
  prior disabled/inactive state, and operator confirmed HDMI desktop
  responsive with built-in off. Fresh read-only exp02 evidence at
  `docs/experiments/evidence/exp02/2026-09-26-2/` exactly matches the
  preflight HDMI-only logical topology (connector, x/y, scale, transform,
  primary). The earlier exp02 snapshot was preserved. No SIGKILL or abrupt
  owner loss was tested in zero-physical state. Step 5's crash-recovery
  criterion, the earlier compositor-crash uncertainty, and Gate FEAS-C stop
  remain unresolved; this single routine PASS does not promote a gate.
- 2026-09-26 (next risk decision, no new test): operator requested a
  supervised HDMI-only process-kill investigation. Recovery and stop steps
  are recorded in `docs/ops/experiment-safety.md` §7: save work, fresh
  second-device SSH, exact PID/backup/timer, one kill after observing blank,
  immediate read-only state check, then independent watchdog/recovery check.
  If GNOME crashes into a new login, do not apply an old backup. A request
  is not execution approval; obtain a separate confirmation that unsaved
  work is safe, SSH/HDMI-only preconditions hold, and this exact one-run
  SIGKILL risk is accepted. No automatic retries or gate promotion.
- 2026-09-26 (explicit process-kill authorization): operator confirmed all
  unsaved work is safe, accepted possible GNOME crash/logout, confirmed
  HDMI-only desktop and a fresh second-device SSH command, and approved
  **one** exp06 `--pause-after-isolate` followed by `kill -9` of only its
  printed PID after visually confirming blanking. Permit its already-armed
  watchdog to fire and inspect state from SSH; no Enter before the kill,
  no retry, no old-backup restore into a new GNOME login, and no automatic
  Gate FEAS-C promotion. Record outcome even if Shell crashes or state is
  unknown.
- 2026-09-26 (approved attempt, **no SIGKILL performed**): exp06 paused
  with PID 377437, exact backup in `exp06/2026-09-26-2/`, timer
  `blackroom-exp06-watchdog-1790443804` armed. Operator observed HDMI
  blank/black and the disabled built-in panel light up blank, then switched
  VT (F3 showed tty3; F2 initially failed and later returned to the HDMI
  GUI). By the time isolation could be independently checked, the timer
  was inactive and exp07 had already run at 23:00:56 local with PASS:
  original HDMI-only topology matched, no retry/apply error. In accordance
  with the one-run approval, **the printed PID was not killed and no repeat
  was started**. After verifying the original session was restored, Enter
  let the paused exp06 redundantly apply its backup and stop Meta-0;
  pause-mode PARTIAL and timer-already-unloaded warning are expected.
  Independent final checks found Shell PID 34735 unchanged, PowerSaveMode
  0, no Meta connector/timer, stop_error=None, and original service state
  disabled/inactive; operator confirmed HDMI desktop responsive, built-in
  off. Evidence: `docs/experiments/evidence/{exp06,exp07}/2026-09-26-2/`.
  No automatic-restore-after-process-death observation was made. The
  earlier Shell crash remains unexplained; Step 5 and Gate FEAS-C stop
  remain open. Safety §7 now requires confirming the timer and isolated
  state are still active immediately before any future kill attempt.
- 2026-09-26 (offline timing preparation, no run approved): operator asks
  for clear instructions before another attempt. During the blank window,
  observe HDMI and reply immediately from the second device; do not switch
  VT, press Enter, or send a kill. Use VT/SSH only for recovery. A 45-second
  window was insufficient during the previous observation. Prepare an
  optional `--watchdog-seconds` for **pause mode only**, bounded to 45-120,
  leaving the default 45 seconds and repeated-cycle timing unchanged.
  A proposed 90-second window gives the observer more time but also extends
  the unattended blank if recovery is needed. Before any future SIGKILL,
  verify the named timer remains active and original session still isolated.
  This is not approval to run or repeat exp06, nor a Gate FEAS-C promotion.
- 2026-09-26 (offline timing option complete, still no live approval): exp06
  now accepts `--watchdog-seconds` only with `--pause-after-isolate`, bounded
  to 45-120; absent an override, pause mode uses 45 regardless of `--cycles`.
  The normal repeated-cycle calculation is unchanged. Focused parsing and
  duration tests, workspace cargo test/fmt/clippy/check, project doctor,
  fresh binary build and `--help` all passed. A 90-second supervised retry
  remains a proposal: saving work, fresh SSH, HDMI-only preflight and exact
  one-run SIGKILL approval must be reconfirmed before any execution.
- 2026-09-26 (explicit bounded retry approval): operator reconfirmed saved
  work, HDMI-only visible desktop and a fresh second-device SSH command;
  explicitly approved **one** exp06 `--pause-after-isolate
  --watchdog-seconds 90` attempt with possible Shell crash/logout. After
  visual blanking, independently verify the printed timer is still active
  and GetCurrentState contains no active physical logical output; only then
  send one SIGKILL to the printed exp06 PID. If either check fails or the
  timer restores first, abort kill and clean up. No Enter/VT switch while
  isolated, no repeat, no old-backup restore into a new GNOME login, and no
  automatic FEAS-C promotion. This is separate from the prior spent
  45-second approval.
- 2026-09-26 (one 90-second attempt, **no SIGKILL performed**): exp06
  paused at PID 403830 with timer `blackroom-exp06-watchdog-1790446557`
  and backup `exp06/2026-09-26-3/backup.json`. The process reported
  isolation, but the operator answered "already restored / unsure", not
  a timely confirmation of clean blanking. Before independent timer and
  isolated-state checks could justify a kill, the timer fired at 23:47:40
  local and exp07 reported PASS with the exact original HDMI-only topology
  matching, no retry or apply error. Per the approval's abort condition,
  **no kill was sent and no repeat was started**. Enter after restoration
  let exp06 redundantly apply its backup and Stop its ScreenCast session;
  its pause-mode PARTIAL and timer-not-loaded warning are expected. Shell
  PID stayed 34735, PowerSaveMode 0, GetCurrentState returned with no
  Meta connector, no timer remained, service returned to disabled/inactive,
  and the operator confirmed HDMI desktop responsive with built-in off.
  Generated exp06/exp07 evidence is in `2026-09-26-3` under each experiment.
  This is another safe watched restore, **not** a visual privacy attestation
  for this run or an abnormal-termination observation. The one-run 90s
  approval is consumed. Do not lengthen/retry automatically on the
  daily-driver host; Step 5 and FEAS-C stop remain open.
- 2026-09-26 (operator correction after the 90-second attempt): operator
  clarified they were **not physically present to watch** during that
  isolation window and can watch now. The earlier "already restored /
  unsure" response therefore cannot attest to HDMI blanking, absence of
  desktop content, or the kill window. The recorded exp07 topology PASS
  remains a valid machine result, but the test lacked the mandatory
  continuous human observer. No SIGKILL occurred. Safety §7 now requires
  the operator to confirm physical presence **before launch**, remain
  through recovery, and abort any kill if they step away. This correction
  does not authorize a new run or lift the Mutter-instability stop.
- 2026-09-26 (communication-path correction): operator is able to watch
  the HDMI screen but cannot send a VS Code chat message while the GUI is
  blank; they can report their observation only after restoration. Asking
  for a timely in-blank chat reply was an impossible requirement, even
  with a present observer. Do not repeat timed chat-mediated attempts or
  lengthen the blank window. A separate machine-gated owner-loss diagnostic
  may be designed offline: require armed watchdog and read-only verified
  zero-physical logical topology, persist pre-death evidence, then trigger
  one owner exit without relying on an operator message. Such checks do
  **not** prove the physical panel blank: operator observation is collected
  afterward and cannot serve as a pre-kill veto. New design, risk review,
  and explicit approval are required before any implementation or live
  execution. Phase 5 stop/Gate FEAS-C status are unchanged.
- 2026-09-26 (offline diagnostic design authorized, no live approval):
  operator clarified they can watch without interacting and report only
  after the screen returns; requested an option selected before launch.
  Design a distinct opt-in auto-owner-loss mode, leaving manual/default
  paths unchanged. It requires `--pause-after-isolate` and an explicit
  45-120s watchdog duration, an original HDMI-1-only logical layout,
  persisted backup, the same GNOME Shell PID, active named timer, DPMS OFF,
  exactly one virtual-only logical monitor and HDMI-1 still in raw
  inventory, all within 10s of beginning watchdog arming. Persist a
  `PARTIAL` pre-kill record before signaling only its own PID once. Any
  failed precondition or write error returns normally through restore
  guards; missing timer or elapsed window must never trigger a kill.
  A machine check is **not** a visual-privacy attestation; the operator
  reports observed physical screen state after recovery. Validate CLI
  exclusions, fail-closed predicate, workspace Rust gates, and review the
  binary offline. Do not run this mode until separate approval covering
  GNOME crash/logout, no pre-kill visual veto, and SSH recovery.
- 2026-09-26 (opt-in diagnostic implemented offline, NOT executed): exp06
  adds `--auto-kill-after-isolate`, requiring explicit pause mode and an
  explicit bounded watchdog duration. Default/manual behavior is unchanged.
  The automatic branch checks HDMI-1-only original topology before
  mutation, retains RAII restore guards through backup/pre-kill evidence,
  and verifies original Shell PID, timer active, DPMS OFF, one virtual-only
  logical monitor and raw HDMI-1 within 10 seconds of watchdog arming.
  A second structured state/timer/PID check follows evidence write and
  stdout flush immediately before `/usr/bin/kill` targets only its own PID.
  Any precondition/write/command failure returns through the restore guard;
  `pre_kill.json` and its report are PARTIAL, not proof that a signal was
  delivered or a physical panel blanked. Synthetic CLI/exclusion and
  individual precondition-failure tests passed; actual timer race, Shell
  survival and watchdog restoration after SIGKILL remain **unverified**.
  No live invocation or Gate FEAS-C change has been approved.
- 2026-09-26 (independent safety review correction, offline): a read-only
  Reviewer found the initial `pgrep -xo gnome-shell` PID comparison could
  select a greeter or another session's Shell, so it was a blocker to live
  use. Replaced it with `org.freedesktop.DBus.GetConnectionUnixProcessID`
  for the exact `org.gnome.Mutter.ScreenCast` service on exp06's own user
  bus connection, before and after isolation; missing owner or changed PID
  now aborts through the restore guard. The read-only D-Bus query returned
  PID 34735, matching this host's Shell. Focused compile, format and
  fail-closed tests passed after the fix. No live use or Gate promotion.
- 2026-09-26 (final offline check): full workspace test/fmt/clippy/check,
  project doctor and freshly rebuilt exp06 `--help` passed after the
  bus-owner correction. A second independent read-only Reviewer pass found
  no further concrete blocker in the opt-in auto-kill branch. It identified
  runtime-only residual risks: a slow run can safely abort at the 10s gate;
  post-SIGKILL Shell survival, watchdog restoration, and physical privacy
  remain unverified until a separately approved operator-witnessed run.
  No GNOME mutation or auto-kill invocation occurred during preparation.
- 2026-09-26 (recovery read-through fix, offline): the opt-in pre-kill
  message now includes the exact absolute backup path alongside the
  watchdog unit and PID, and flushes that output before its final
  state/timer check and self-signal. This lets SSH recovery use the
  correct snapshot if the original Shell survives but the watchdog fails;
  never apply it after a new GNOME login. Focused compile/format/tests
  passed; no auto-kill run or Gate FEAS-C reclassification.
- 2026-09-26 (explicit automatic diagnostic approval): operator confirmed
  they are physically watching HDMI throughout, can report observations
  after recovery, saved their work, and verified HDMI-only plus a fresh
  second-device SSH command. They explicitly approved **one**
  `exp06 --pause-after-isolate --watchdog-seconds 45
  --auto-kill-after-isolate` run with automatic SIGKILL only if the
  machine-gated preconditions pass, accepting possible GNOME Shell
  crash/logout and no pre-kill human visual veto. No repeat, no default
  behavior change, and no automatic FEAS-C promotion. If the original
  Shell lives but watchdog fails, use only the exact printed backup;
  never restore an old backup into a new GNOME login.
- 2026-09-26 (one automatic owner-death run completed): the explicitly
  approved exp06 self-SIGKILL produced PARTIAL pre-kill evidence in
  `docs/experiments/evidence/exp06/2026-09-26-4/`. It records HDMI-1 as
  the original sole logical output, Meta-0 as the sole active output,
  `PowerSaveMode=3`, the timer active, the same ScreenCast owner PID 34735,
  and 826ms since watchdog arming. The 00:10:41 local journal reports
  both `Removed virtual monitor Meta-0` and `D-Bus client with active
  sessions vanished`; exp06 no longer ran afterward. **Before the
  watchdog**, read-only exp02 evidence at
  `docs/experiments/evidence/exp02/2026-09-26-3/` found HDMI-1 already
  active logically with eDP-1 still disabled in logical topology, but
  `PowerSaveMode` remained 3 (OFF). This directly answers the automatic
  restoration question: Mutter re-enabled the original logical output
  on owner loss but did not physically unblank it. The timer fired at
  00:11:32; exp07 reported PASS at 00:11:33, matched the exact backup
  without retry, and set `PowerSaveMode=0`. Final independent checks:
  Shell PID unchanged at 34735; no Meta connector or timer; service
  restored to disabled/inactive. Operator touched nothing during the run:
  HDMI went blank until the UI returned; built-in panel flickered and
  then blanked, but the flicker was **too brief to determine whether any
  desktop content appeared**. Operator confirmed HDMI showed no content
  during isolation. Step 5's process-kill and watched-restore diagnostic
  is complete (pre-kill PARTIAL by design; exp07 PASS). This single clean
  run does not explain the earlier SIGSEGV, prove repeat reliability, or
  prove physical privacy on the built-in panel. Gate FEAS-C remains
  **UNPROVEN** and the Mutter-instability stop remains in force; no repeat
  or later-phase live work is approved by this result.
- 2026-09-27 (operator privacy acceptance and evidence addendum): operator
  explicitly accepts the brief built-in flicker as privacy-acceptable for
  this run. They remain unsure whether readable content appeared during
  the flicker; approval does not change that observation into proof of no
  desktop exposure. Independent journal chronology, the saved pre-watchdog
  exp02 inventory and the contemporaneous (not separately persisted) DPMS
  reading are attributed in
  `docs/experiments/evidence/exp06/2026-09-26-4/observation.md`. Step 5's
  verification text now recognizes that the deliberately killed exp06 can
  write only PARTIAL pre-kill evidence while the independent exp07 restore
  reports PASS. Full Gate FEAS-C acceptance still needs its remaining
  matrix/privacy/reliability criteria and resolution of earlier instability.
- 2026-09-27 (proposed diagnostic deviation, NO-GO; no run): operator explicitly
  requested an override of the Phase 5 live-test stop on the daily-driver
  laptop and accepted that a compositor crash may require a restart. §7 of
  the experiment-safety procedure records a proposed single eDP-1-only,
  45-second watchdog-backed pause observation with no kill or repeat. The
  earlier Shell SIGSEGV and panel privacy question remain unresolved; this
  risk acceptance is not evidence for Gate C or permission for product
  activation. Independent review rejected a live go path: fresh approval and
  SSH cannot substitute for a credible crash reassessment and persisted
  pre/postflight evidence. No isolation
  command was launched.
- 2026-09-27 (operator clarification, no run): the operator considers the
  brief built-in flicker privacy-acceptable and does not want a repeat capture
  for the proposed diagnostic. Record that as explicit risk acceptance, not
  evidence that no content appeared. The diagnostic's capture prerequisite is
  removed; the earlier Shell instability and Gate FEAS-C stop remain open.
- 2026-09-27 (one-run operator override, preflight pending): operator reiterated
  authorization to run the eDP-1-only isolation diagnostic on this laptop and
  to update the protocol, acknowledging the earlier review's no-go warning.
  §7 now permits one 45-second watchdog-backed pause with no intentional kill,
  only after saved work, eDP-only topology, fresh SSH, unique preflight evidence
  and exact-command confirmation. No run or gate promotion has occurred yet.
- 2026-09-27 (watchdog safety repair, no run): independent preflight review
  found exp07 would apply an old backup to a restarted Shell/session. New exp06
  backups persist the selected active Wayland session ID and original Shell PID;
  exp07 now refuses missing/mismatched identity before restore writes or retry.
  Pause-mode exp06 also prints the actual watchdog unit and timeout. Focused
  offline tests/build passed; exp06 now also refuses launch unless remote desktop
  is masked and inactive. On watchdog identity refusal, the named service journal
  is the required durable evidence (exp07 does not produce a normal PASS report).
  The sanitized preflight remains pending service masking and immediate operator
  confirmation. No GNOME mutation occurred.
- 2026-09-27 (one eDP-only no-kill run; STOP): operator confirmed saved work,
  live second-device SSH, eDP observation and the exact command. Fresh binary
  checks and masking passed. During isolation only Meta-0 was active logically
  and power was OFF. The 45-second watchdog exp07 PASS restored eDP-only and
  power ON with Shell PID 34735 unchanged; operator saw a responsive desktop.
  After Enter cleaned up exp06's ScreenCast session, Meta-0 disappeared but
  HDMI-1 became active alongside eDP-1. A second exact-backup exp07 PASS
  restored eDP-only without retry; final Shell/session, timer, power, service
  and physical desktop were independently confirmed normal. Evidence:
  `docs/experiments/evidence/exp06/2026-09-27/observation.md`. Exp06 checks
  restoration before `Session.Stop` and has no final topology check afterward.
  The first PASS was transient, not end-to-end stable restoration. Gate FEAS-C
  remains unproven; do not repeat or proceed to another live matrix row.
- 2026-09-27 (new unplugged-HDMI diagnostic requested, no run): operator
  physically disconnected HDMI-1; fresh GetCurrentState shows only eDP-1 in
  raw and logical topology. That removes the prior reactivation's physical
  precondition, but does not repair the missing post-Stop check. Exp06 now
  compares final logical topology against its backup and persists raw/logical
  outputs, power, Shell/session and timer status; a mismatch changes pause
  output from PARTIAL to FAIL. The watchdog remains armed through cleanup
  until final checks succeed. Focused offline regression reproduces rejection
  of synthetic eDP-plus-HDMI after Stop. A fresh preflight, independent safety
  review and new exact-command approval are required before this distinct
  hardware-configuration diagnostic. Gate C remains stopped.
- 2026-09-27 (cleanup recovery amendment, no run): reviewer found that a
  pause test consumes its first timer before Enter, so deferring disarm did
  not cover Session.Stop. Exp06 now checks original Shell PID/login session
  before local restore and in RestoreGuard, arms a separate 45-second
  identity-guarded exp07 timer before cleanup, and leaves it armed if final
  topology fails. Findings include its unit, final raw/logical connectors,
  DPMS, Shell/session, timer/service state and probe errors. Focused offline
  tests pass; this still needs independent review and operator preflight
  before any second diagnostic.
- 2026-09-27 (one unplugged-HDMI diagnostic; no repeat): operator confirmed
  saved work, second-device SSH, physical observation and the exact command;
  raw inventory contained only eDP-1. One 45-second no-kill exp06 run reached
  Meta-0-only logical topology with DPMS OFF. First watchdog exp07 PASS
  restored eDP-only/DPMS ON. After operator-visible confirmation, one Enter
  armed a separate 45-second cleanup timer before restore/ScreenCast Stop.
  Exp06's generated pause-mode PARTIAL includes a final-state match: raw and
  logical eDP-1 only, DPMS ON, Shell PID 34735/session 3 unchanged, Meta-0
  gone, no probe errors. The cleanup timer was disarmed; service returned
  disabled/inactive; operator confirmed normal desktop. Evidence:
  `docs/experiments/evidence/exp06/2026-09-27-2/observation.md`. This did
  not reproduce the earlier HDMI reactivation because HDMI was physically
  absent; the connected-HDMI restoration failure, prior Shell crash and
  unfinished matrix still block Gate FEAS-C. No further run authorized.
- 2026-09-27 (evidence contract gap, no run): independent review confirmed
  exp06's local `backup.json` contains session ID, Shell PID, outputs and
  logical topology but not the full `blackroom-gnome::DisplayBackup` fields
  (`primary_output`, `configuration_hash`). Exp07 checks logical fields, not
  the plan's required topology hash. The generated artifacts are preserved
  unchanged; the hash-plus-field acceptance criterion remains **unmet** and
  must be reconciled before any Gate FEAS-C determination.
- 2026-09-27 (one connected-but-inactive HDMI diagnostic; STOP, no repeat):
  operator approved the exact 45-second no-kill pause command with fresh
  second-device SSH, saved work and eDP observation. The first watchdog
  exp07 PASS restored eDP-only while Meta-0 remained raw. After the operator
  confirmed desktop and SSH, Enter armed a separate 45-second cleanup timer
  before ScreenCast Stop. Exp06's final findings report FAIL: HDMI-1 became
  logically active beside eDP-1 after Stop, matching the previous failure;
  Shell PID 34735/session 3 and power ON were stable. The second watchdog
  exp07 PASS restored the original eDP-only logical topology; Meta-0 was
  gone, timers disarmed, service restored disabled/inactive and operator
  confirmed normal desktop. See `docs/experiments/evidence/exp06/2026-09-27-3/observation.md`.
  This run supports a reproducible connected-HDMI post-Stop mismatch, not
  its cause or Gate C PASS. Original generated artifacts remain unchanged;
  historical Shell crash and backup-hash acceptance gap remain. No new run
  is authorized by this result.
- 2026-09-27 (offline post-Stop guard repair, no run): exp06 no longer disarms
  its identity-checked RestoreGuard before ScreenCast Stop. It disarms the
  guard and exact-backup timer only when the pre-Stop restore, Stop result,
  connector removal and final topology all verify; a mismatch still reports
  FAIL and retains both fallback paths. Focused synthetic tests and lint pass;
  this changed fallback has not been observed live. The canonical
  `blackroom-gnome` hash covers sorted output records (including enabled
  state), not the full logical topology. Exp06/exp07's historical JSON schema
  omits enabled/primary/hash, so a compatible shared-schema migration and
  field-plus-hash verification remain separate work before Gate C assessment.
- 2026-09-27 (offline future-backup schema, no run): new exp06 snapshots include
  per-output enabled state, primary connector/serial and the existing
  `blackroom-gnome` output hash. Exp07 continues to read old backups with
  identity plus exact logical-field checks; it rejects incomplete or corrupt
  new metadata before any GNOME write and requires current backed-output hash
  **and** exact logical topology for a new-backup PASS. Raw virtual connectors
  left by a still-paused exp06 are excluded from the physical-output hash;
  disabled outputs with no current mode retain only their saved mode in that
  comparison. Focused synthetic exp06/exp07/canonical-hash tests and lint pass.
  The review identified two additional recovery risks: the original
  `DefaultHasher` value could change across builds, and disabled raw HDMI
  without a current mode was omitted from the snapshot. The offline repair
  now uses a pinned version-one SHA-256-derived output hash and includes raw
  disabled outputs with explicit no-mode fields. Exp07 falls back to exact
  identity/topology checks for old, unversioned or unknown-version backups;
  it enforces hash plus fields for version one. The hash is not a security
  control or a full topology hash. Cross-build fixture, disabled-HDMI and
  old-backup tests pass, but the new path has not been observed live. Old
  generated evidence is unchanged and Gate FEAS-C stays stopped.
- 2026-09-27 (synthetic post-Stop local repair, no run): after ScreenCast Stop
  and virtual connector removal, exp06 makes at most one exact-backup reapply
  when final topology/hash is wrong, the original Shell/session is unchanged,
  the second watchdog timer is active and its service is inactive. It skips
  local reapply if that service is already starting/running, reobserves the
  final state and records the attempt/error, and still reports FAIL even if
  local reapply succeeds.
  The second timer remains armed for independent restoration on any such
  attempt; on failed verification the identity-checked Drop guard also remains
  armed and restores both topology and power. Focused synthetic checks cover
  refusal on stale identity, missing timer or active watchdog service,
  timer, failed Stop, unreadable final state and false-PASS prevention. This
  changed recovery envelope needs its own safety review and exact-command
  operator approval before any live diagnostic; Gate FEAS-C remains STOP.
- 2026-09-27 (one approved guarded post-Stop diagnostic; STOP, no repeat):
  connected HDMI-1 was raw but inactive, eDP-1 the sole active output. The
  new version-one backup captured disabled HDMI without a current mode.
  First watchdog exp07 PASS restored eDP-only. After operator confirmation,
  Enter armed the second 45-second cleanup timer before ScreenCast Stop.
  Exp06 reported FAIL and `post_stop_restore_attempted=true`; the local
  identity-checked reapply returned no error and its final eDP-only topology,
  output hash, power and original Shell/session verified. The timer remained
  armed and independently invoked exp07 PASS. Final physical desktop/SSH,
  original session, exact eDP-only active topology, zero timers and disabled/
  inactive service were independently confirmed. The *initial post-Stop*
  mismatch before local reapply was not persisted, so this run alone cannot
  identify its changed field. See
  `docs/experiments/evidence/exp06/2026-09-27-4/observation.md`. This is
  observed assisted recovery, not reliable unassisted restoration, physical
  privacy proof, or Gate C PASS. No more runs are authorized.
- 2026-09-27 (synthetic evidence-gap repair, no run): future exp06 findings
  retain `post_stop_pre_repair_state` before any local reapply so a recovered
  final state cannot erase the triggering raw/logical outputs, hash result,
  identity, power or watchdog observation. A focused synthetic serialization
  test passes. The earlier `2026-09-27-4` generated report remains unchanged
  and its missing initial state cannot be reconstructed. No live repeat is
  authorized by this instrumentation.