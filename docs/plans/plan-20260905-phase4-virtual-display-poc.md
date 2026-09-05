# Plan: Phase 4 — Virtual Display PoC

**Created:** 2026-09-05
**Status:** complete
**Approved by:** user ("approved proceed", 2026-09-05)
**Task tier:** governed

## Goal

Prove, via real (not mocked) Mutter/PipeWire mutation for the **first time in this
project** (Phase 3 was real but strictly read-only), that: (a) the existing GNOME
desktop can be captured over ScreenCast/PipeWire; (b) `RecordVirtual` creates a real,
`DisplayConfig`-visible virtual monitor at three resolutions, reproducibly destroyed;
(c) the virtual monitor can function as a usable additional active display in the same
session. Close Gate **FEAS-B**. Promote the four Phase-4-relevant capability constants
in `crates/blackroom-gnome/src/mutter/capability.rs` from live evidence, re-issue
`docs/gnome/capability-report.md`, and land the production modules
(`remote_desktop.rs`, `screencast.rs`, `virtual_monitor.rs`, `pipewire_capture.rs`)
later phases build on. No physical display/input isolation, remote input, or
encoding/WebRTC — those remain Phases 5–8 and Stage III+.

## Acceptance Criteria

- `exp03_capture.rs`: a ScreenCast (optionally RemoteDesktop-paired, per what the live
  session objects actually require — Doc 02 §9 step 2 says "if required", not
  assumed) capture of the **existing** desktop starts, real PipeWire frames arrive
  with reported dimensions/rate, and the session stops with verified cleanup (a
  follow-up call against the session object path fails as unknown/gone).
- `exp04_virtual_monitor.rs`: `RecordVirtual` creates a real virtual monitor at
  1280×720, 1920×1080, and 2560×1440 (60 Hz), each confirmed via
  `DisplayConfig.GetCurrentState` (a real monitor entry, not merely a capture
  stream) and destroyed cleanly. 50 create/destroy cycles complete with no leaked
  monitors or PipeWire nodes (`pw-dump` diff empty) and no GNOME Shell crash
  (journal evidence).
- `exp05_virtual_active.rs`: the virtual monitor renders desktop/apps/workspaces and
  is usable as an **additional** active display in the same session (existing
  windows/workspace state preserved, keyboard focus correct); the user's original
  display configuration is restored after the run. Physical monitors are **not**
  disabled or removed from the topology this phase (see Evidence #3).
- `crates/blackroom-gnome/src/mutter/{remote_desktop.rs, screencast.rs,
  virtual_monitor.rs, pipewire_capture.rs}` provide production implementations of
  the mechanics the experiments prove, in the existing `session.rs`/`capability.rs`
  style (thin D-Bus/PipeWire wrappers, pure logic unit-tested, cleanup-safe).
- `capability.rs`'s `remote_desktop_capable`, `screencast_capable`,
  `pipewire_capable`, and `virtual_display_capable` are promoted from their Phase
  0–3 values per the decision rule in Evidence #1, each with an updated doc comment
  citing the evidence; the other 3 `NOT_YET_DETERMINABLE` constants
  (`remote_input_capable`, `physical_input_isolation_capable`,
  `emergency_capable`) are untouched. `docs/gnome/capability-report.md` is
  re-issued accordingly (its own text commits to this).
- The `GnomeBackend`-assembly question (Evidence #2) is answered and recorded in a
  new `docs/gnome/virtual-display.md`, not left implicit.
- `docs/ops/experiment-safety.md` states Phase 4's masking requirement (§5, first
  real use) explicitly and records why §1–4 remain not-yet-required this phase
  (Evidence #4).
- `pipewire` is promoted evaluated→in-use in `docs/security/architecture.md` §6 and
  the relevant crates' `Cargo.toml`; `cargo deny check` stays green.
- No change to `blackroom-core` or the 13-op `GnomeBackend` trait signature in
  `backend.rs`.
- `cargo test --workspace`, `cargo fmt --check`,
  `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo check --workspace --all-targets`, `cargo deny check`, `cargo audit` all
  green; `Blackroom_Console` re-indexed in codebase-memory; independent
  Reviewer-subagent pass recorded.
- Gate **FEAS-B** decision recorded. If any experiment fails outright (Mutter
  refuses the mechanism entirely), this plan's Execution Log records a Doc 00 §49
  stop-condition report instead of silently proceeding.

## Non-Goals

- No physical display isolation, `disable_physical_outputs`/`restore_physical_outputs`,
  or any `ApplyMonitorsConfig` call that disables/removes a physical monitor from the
  topology (Phase 5, hard gate FEAS-C). `exp05` proves the virtual monitor as an
  **additional** active display only.
- No remote input, EIS, or physical input isolation (Phases 6–7).
- No GNOME lock / same-session validation (Phase 8).
- No video encoding, GStreamer, or WebRTC (Stage III+).
- No change to `blackroom-core`'s state machine, transitions, lease, or epoch logic.
- No assembled concrete `GnomeBackend` implementation (e.g. a `MutterBackend` struct)
  this phase (Evidence #2) — the other 9 of 13 Doc 05 §8 ops stay unimplemented
  until their own phases; `backend.rs` is not edited this phase.
- No `remote-hostd` (does not exist yet).
- No `docs/ops/experiment-safety.md` §1–4 procedure (SSH prerequisite / watchdog
  timer / VT fallback) — not required this phase; only §5 (masking) newly applies
  (Evidence #4).
- No permanent change to the user's display configuration: every
  `ApplyMonitorsConfig`/topology mutation this phase is temporary and is restored
  before each experiment exits.
- No new `blackroom-core` error codes (Evidence #7: existing codes already cover
  every Phase 4 failure mode).

## Evidence And Decisions

- **Sources read in full or by section:** Doc 10 §10–§12 (Experiments 3–5); Doc 02
  §8–§11 (Second–Fifth Experiments); Doc 05 §8–§9 (`GnomeBackend` interface,
  capability detection), §22–§31 (RemoteDesktop/ScreenCast/PipeWire/Virtual
  Monitor architecture, the 8-step "entering remote mode" sequence at §27,
  Original/Backup/Validation at §28–§30); Doc 19 §16–§17 (PipeWire/Mutter resource
  management); Doc 11 §9 (Phase 4 DoD: "Virtual monitor creation and teardown are
  reproducible"); assessment §4.1 (document authority ordering), §5 (conflict
  register, C2/C3/C6/C8/C13/C23/C26), §7.3/§7.6 (FEAS-B/C mechanism + WebRTC/input
  termination architecture), §8 (experiment safety plan); roadmap Phase 3/4/5
  entries; `docs/gnome/{api-inventory,feasibility-research,capability-report,
  session-discovery}.md`; `docs/security/architecture.md` §6; `docs/ops/
  experiment-safety.md`; the Phase 3 plan (precedent for structure, decisions,
  and non-goal wording); live source: `backend.rs`, `fake.rs`, `lib.rs`,
  `mutter/{mod,session,capability}.rs`, `error.rs`, both crates' `Cargo.toml`,
  `blackroom-experiments/src/{lib,cli,evidence,session}.rs`,
  `bin/exp02_mutter_inventory.rs`.
- **MemPalace:** `mempalace_status` loaded; `mempalace_diary_read` (agent
  `copilot`, wing `blackroom_console`) and `mempalace_search` show Phase 3
  complete and "next: `/plan-task` Phase 4 (virtual display PoC)" — matches
  `docs/HANDOFF.md` exactly. No contradiction, no degraded mode.
- **Code graph:** `Blackroom_Console` indexed, 4034 nodes/6474 edges, status
  `ready`, 0 skipped/parse-partial files; `check_index_coverage` over
  `crates/blackroom-gnome`, `crates/blackroom-experiments`, `docs/gnome`,
  `docs/ops` returned `no_recorded_issue` for all four scopes (fresh, matches
  Phase 3's post-implementation state). Scoped `get_architecture` on
  `crates/blackroom-gnome` shows exactly **one** `IMPLEMENTS` edge
  (`FakeGnomeBackend → GnomeBackend`) — confirms no concrete real
  `GnomeBackend` implementation exists anywhere yet, supporting Decision #2.
- **1. Capability-tier promotion is a decision rule, applied to whatever the live
  experiments actually show — not asserted in advance.** `capability.rs`'s
  `NOT_YET_DETERMINABLE` (`Unknown`) currently covers 4 constants;
  `docs/gnome/capability-report.md` separately has `remote_desktop_capable` /
  `screencast_capable` / `pipewire_capable` at `EXPERIMENTAL` (presence-only
  evidence) and `virtual_display_capable` at `UNKNOWN`. Decision: if `exp03`/
  `exp04`/`exp05` meet this plan's stated acceptance criteria (session
  create/start/stop with verified cleanup; 3 resolutions confirmed via
  `GetCurrentState`; 50/50 leak-free cycles; desktop usable on the virtual
  monitor), promote `remote_desktop_capable`, `screencast_capable`, and
  `pipewire_capable` to `Supported` (mirrors the bar `display_config_capable`
  already cleared in Phase 3: "not just present but actually called and
  returned correct... a real functional test, not mere presence"), and
  `virtual_display_capable` to `SupportedWithLimitations` — not a clean
  `Supported` — because the zero-physical-monitor question (assessment §7.3),
  GPU-specific cross-buffer-scanout behaviour, and cursor behaviour on this
  hybrid host all stay `UNVERIFIED`/escalated to Phases 5/9/24
  (`feasibility-research.md` topics 3 and 9). If any experiment only partially
  passes (e.g. 2 of 3 resolutions, occasional leak), the affected constant is
  promoted only as far as the evidence supports, with the limitation named in
  `capability-report.md`; if one fails outright, that constant stays/reverts
  toward `Unsupported`/`Unknown` and Doc 00 §49's stop-condition reporting
  applies. The actual chosen tiers are recorded in the Execution Log once run,
  not fabricated here.
- **1a. Promotion is structural, not a live call inside `detect()`.** Doc 19
  §16–17 treats repeated virtual-monitor/RemoteDesktop/ScreenCast
  creation-destruction as a first-class reliability risk ("do not assume an
  operation is safe simply because it works once"). Unlike
  `display_config_capable` (a cheap, read-only `GetCurrentState` call safe on
  every agent startup), creating/destroying a real virtual monitor and
  PipeWire session on **every** `gnome-session-agent` startup would itself be
  the repeated-cycle risk Doc 19 warns about. Decision: the 4 promoted
  constants stay **structurally fixed** in code (like `NOT_YET_DETERMINABLE`
  today) but at their new, evidence-derived tier, with a doc comment citing
  the Phase 4 evidence file — `detect()` does not gain a new mutating D-Bus
  call for these fields.
- **2. `GnomeBackend` assembly stays deferred; Phase 4 remains "modules only".**
  The roadmap's own Phase 4 file list (`docs/plans/plan-20260904-blackroom-
  console-master-roadmap.md` line 163) names exactly 4 new `mutter/*.rs`
  modules plus 3 experiment binaries — no backend-assembly file. The Phase 3
  plan's Non-Goals explicitly grouped `remote_desktop.rs` (Phase 4) with
  Phase 5's `display_config.rs`, Phase 6's `eis.rs`, and Phase 8's `lock.rs` as
  co-equal prerequisites that "eventually get assembled into one" — implying
  assembly happens once significantly more of the 13 ops are real, not after
  the first 4. No consumer exists yet (`remote-hostd` is Phase 11+) to justify
  a struct where 9 of 13 methods would be `unimplemented!()`/placeholder
  bodies, which the project's established discipline (Phase 2 declined
  `thiserror`/`schemars`/`tokio` without justified need; Phase 3 deferred
  `blackroom-ipc` extraction "until a second real consumer exists") argues
  against. Decision: **no concrete `GnomeBackend` impl (no `MutterBackend`
  struct) this phase**; `backend.rs` is not edited. Revisit once Phase 5
  (`display_config.rs`) and Phase 6 (`eis.rs`) also exist, or once a real
  caller needs one — whichever comes first — not asserted as a fixed future
  phase number here.
- **3. `exp05` proves "additional active display", not "physical monitors
  removed".** Doc 02 §11 lists "the physical monitors can be removed from the
  active topology" among the things the Fifth Experiment's PoC "must verify",
  and assessment §7.3 says Experiment 5 **and** 6 jointly establish whether
  Mutter permits an all-virtual configuration. However: the roadmap assigns
  `display_config.rs` (the `DisplayBackup`/hash-verified-restore/hotplug
  machinery Doc 05 §28–30 requires before disabling anything) to **Phase 5**,
  not Phase 4; Doc 02 §11 itself also says "do not permanently modify the
  user's display configuration"; and disabling the only other active output
  risks stranding the operator without a physical desktop mid-experiment — the
  exact failure mode `docs/ops/experiment-safety.md` §1–4 exists to cover, and
  that procedure's SSH/watchdog prerequisite is not yet exercised for this
  phase (Evidence #4). Decision: `exp05` adds the virtual monitor as an
  **additional** enabled logical monitor (temporary `ApplyMonitorsConfig`,
  physical monitor(s) stay enabled throughout) and proves app/workspace
  rendering and primary/window-placement behaviour on it. The narrower
  "zero-physical-monitor" question is explicitly left to Phase 5's Experiment 6
  under its full safety procedure. This is a scoping interpretation, not a
  document conflict — flagged here for explicit user sign-off rather than
  silently narrowed.
- **4. `docs/ops/experiment-safety.md` scope update.** The doc's header frames
  §1–4 (SSH prerequisite, watchdog, VT fallback, snapshot) as mandatory "from
  Experiment 6/9 onward"; its §5 (mask `gnome-remote-desktop`) is written
  unconditionally ("any RemoteDesktop/ScreenCast experiment") but has never
  actually applied yet (Experiment 2 only introspected properties, never
  created a session). Decision: Phase 4 is §5's first real trigger — update
  the doc's scope note (currently describes only Phase 0–1) to state this and
  confirm §1–4 stay not-required this phase per Decision #3 (physical outputs
  never disabled, input never isolated). Add one new, proportionate
  precaution for this phase specifically: a bounded per-cycle timeout on
  `exp04`'s 50-cycle reliability loop (10 s/cycle, reusing the existing
  `PREPARING_REMOTE`/`TEARING_DOWN` per-step numeric convention from
  assessment §6.5 rather than inventing a new number) so a hung cycle fails
  the run instead of hanging indefinitely.
- **5. Session sub-object method signatures are unconfirmed — discover, don't
  assume.** `feasibility-research.md` topic 2 explicitly states `RecordVirtual`
  (and, by the same reasoning, `ScreenCast`/`RemoteDesktop`'s `Start`/`Stop`/
  `RecordMonitor`-or-equivalent) live on a **session sub-object** that "by
  construction does not exist until a session is created" and was never
  directly introspected. Decision: `exp03` introspects each live session
  object's real interface (mirroring `exp02`'s technique) before calling any
  of its methods, and records the discovered signatures as evidence for
  `remote_desktop.rs`/`screencast.rs` to implement against — no signature is
  hardcoded from assumption.
- **6. Promote the introspection scanner to shared code (mechanical, not a
  redesign).** `exp02_mutter_inventory.rs`'s XML-tag scanner
  (`parse_introspection_xml`/`ParsedInterface`/…) is private to that binary
  today. `exp03`/`exp04`/`exp05` all need the same technique against
  newly-created session objects — a real, immediate 3× reuse, not speculative
  abstraction. Decision: move (not redesign) this code into a new
  `blackroom-experiments::introspect` module; `exp02` is updated to call it
  with byte-identical output, verified by re-running it.
- **7. `pipewire` crate integration is a research sub-step, not pre-specified.**
  `docs/security/architecture.md` already evaluates `pipewire` 0.10.1 (MIT,
  2026-08-19, freedesktop.org org) but no project code has used it yet; its
  event-loop/threading model is unresearched. Decision: `pipewire_capture.rs`'s
  exact design is worked out against the crate's own current docs/examples at
  implementation time (step 6), keeping its public surface minimal (frame
  count/dimensions/rate only — no encoding, matching the Non-Goals).
- **8. `blackroom-core/src/error.rs` — reviewed, no change.** `VirtualDisplayFailed`,
  `PipewireUnavailable`, and `MutterUnavailable` (already defined, already used
  by `fake.rs`) cover every Phase 4 failure mode (virtual monitor
  creation/destruction failure, PipeWire unavailable/failed, generic Mutter
  D-Bus failure). No new `ErrorCode` variants needed.
- **9. Doc 10 vs Doc 02 experiment numbering — noted, not a new conflict.**
  Both documents sit in assessment §4.1's layer 4 ("Planning/rationale"); Doc 02
  is flagged there for superseded state vocabulary but its "experiment
  procedures" remain valued. Doc 10's single "Experiment 3 — Basic Screen
  Capture" (§10) absorbs Doc 02's two separate experiments ("Second
  Experiment — RemoteDesktop" §8, "Third Experiment — ScreenCast" §9) into one
  binary — this project already uses Doc 10's numbering as the authoritative
  experiment/evidence-folder scheme (`exp00`/`exp01`/`exp02` precedent), and
  this task's own file list (`exp03_capture.rs` citing both "Doc 10 Exp 3 /
  Doc 02 §9") already resolves it the same way. No new conflict-register entry
  filed.
- **10. Evidence-folder naming.** `evidence_dir(exp_id, at)` joins
  `docs/experiments/evidence/<exp_id>/<date>/`; `exp00`/`exp01`/`exp02` each
  use their own `EXP_ID`. The roadmap's "`docs/experiments/evidence/exp03-05/`"
  is read as shorthand for the range, not a literal shared folder — `exp03`,
  `exp04`, `exp05` each get their own folder, matching precedent.

## Risks

- **GNOME Shell instability on the live daily-driver desktop.** Doc 19 §17: "Mutter
  instability must be treated as a first-class reliability concern... do not assume
  an operation is safe simply because it works once." Mitigation: `pw-dump`
  diffing, a live journal crash/restart check, and a bounded 10 s/cycle timeout on
  the 50-cycle loop (Evidence #4); run the 50-cycle loop only after the 3
  single-shot resolutions already pass.
- **Unsaved work loss if GNOME Shell crashes/restarts mid-experiment** on this
  non-disposable workstation. Mitigation: recommend saving/closing other work
  before running `exp04`'s 50-cycle loop specifically (the highest-repetition
  experiment).
- **Session sub-object signatures unconfirmed by any prior evidence** (Evidence
  #5) — risk of live trial-and-error taking longer than expected. Mitigated by
  introspecting first, consistent with the project's established pattern.
- **`pipewire` crate is entirely new to this project**; its threading/event-loop
  model is unresearched (Evidence #7). Mitigated by treating it as an explicit
  research sub-step and keeping the wrapped surface minimal.
- **Hybrid NVIDIA+Intel cross-GPU buffer scanout for `RecordVirtual`'s output is
  `UNVERIFIED`** (`feasibility-research.md` topic 9) — could cause degraded/
  incorrect frames specific to this host. Not a FEAS-B blocker (full GPU-matrix
  verification is Phase 9/24) but any observed limitation must feed the
  `SupportedWithLimitations` tier decision (Evidence #1), not be silently
  dropped.
- **Forgetting to unmask `gnome-remote-desktop`** after Phase 4's experiments
  would silently disable a GNOME feature the user may rely on. Mitigated by
  tying the unmask action to a specific step (end of step 7), not left implicit.
- **Capability-tier promotion changes real startup-gating behaviour** already
  consumed by the already-reviewed Phase 3 `gnome-session-agent` startup path.
  Mitigated by re-running Phase 3's existing capability tests plus a fresh
  grep-based blast-radius check before/after (mirrors the diligence Phase 3
  applied to its own `SessionInfo`/`Capability` change).
- **Decision #3's scoping (deferring "physical monitors removed" to Phase 5) is
  an interpretation, not a document instruction** — flagged explicitly for user
  sign-off; if rejected, `exp05`'s design and this plan's Non-Goals need revision
  before execution.

## Steps

- [x] 1. Update `docs/ops/experiment-safety.md`: extend the scope note to state
      Phase 4 triggers §5 (`gnome-remote-desktop` masking) for the first time;
      confirm and record why §1–4 remain not-required this phase (Evidence #4);
      add the 10 s/cycle bounded-timeout precaution for repeated-cycle
      experiments. Manually verify current `gnome-remote-desktop.service`
      state on this host and mask it (`systemctl --user mask --now
      gnome-remote-desktop.service`) before any experiment in steps 3/5/7 runs.
  - Files: `docs/ops/experiment-safety.md`
  - Depends on: none
  - Verify: doc reviewed against Doc 00 §51/§68's requirement it already cites;
    `systemctl --user is-active gnome-remote-desktop.service` reports
    `inactive`/masked before proceeding to step 3.

- [x] 2. Add `pipewire` to `crates/blackroom-experiments/Cargo.toml`. Promote
      `exp02_mutter_inventory.rs`'s private introspection-XML scanner
      (`parse_introspection_xml`/`ParsedInterface`/related types) into a new
      `blackroom-experiments::introspect` module (mechanical move, no redesign);
      update `exp02` to use it.
  - Files: `crates/blackroom-experiments/Cargo.toml`,
    `crates/blackroom-experiments/src/introspect.rs` (new),
    `crates/blackroom-experiments/src/lib.rs`,
    `crates/blackroom-experiments/src/bin/exp02_mutter_inventory.rs`
  - Depends on: none
  - Verify: `cargo check --workspace`; re-run `exp02_mutter_inventory` and
    diff its output/evidence against a pre-refactor run (byte-identical);
    `cargo deny check`.

- [x] 3. Implement `exp03_capture.rs` (Doc 10 Exp 3 / Doc 02 §9): introspect the
      live `RemoteDesktop`/`ScreenCast` session objects (step 2's helper) to
      discover real method signatures; capture the **existing** desktop
      (RemoteDesktop-paired only if the live session actually requires it, per
      Decision/Evidence #5); receive real PipeWire frames and report
      dimensions/rate; stop with verified cleanup. Write Doc 10 §47-format
      evidence to `docs/experiments/evidence/exp03/`.
  - Files: `crates/blackroom-experiments/src/bin/exp03_capture.rs`
  - Depends on: step 2
  - Verify: live run on this host produces real frames from the existing
    desktop (evidence `report.md`/JSON); a follow-up call against the stopped
    session's object path fails as unknown/gone; `cargo clippy --workspace
    --all-targets -- -D warnings`.

- [x] 4. Port `exp03`'s proven mechanics into production
      `crates/blackroom-gnome/src/mutter/{remote_desktop.rs, screencast.rs}`
      (thin, testable D-Bus wrappers + session handle types with cleanup-safe
      `Drop`), mirroring `session.rs`/`capability.rs`'s style (pure logic
      unit-tested with injected/fake inputs). Update `mutter/mod.rs`'s
      module doc comment (no longer "every call... is read-only").
  - Files: `crates/blackroom-gnome/src/mutter/{remote_desktop.rs,
    screencast.rs, mod.rs}`, `crates/blackroom-gnome/src/lib.rs`
  - Depends on: step 3
  - Verify: `cargo test -p blackroom-gnome`; `cargo clippy -p blackroom-gnome
    --all-targets -- -D warnings`.

- [x] 5. Implement `exp04_virtual_monitor.rs` (Doc 10 Exp 4 / Doc 02 §10):
      `RecordVirtual` at 1280×720, 1920×1080, 2560×1440 (60 Hz), each confirmed
      via `DisplayConfig.GetCurrentState`; destroy cleanly; then a 50-cycle
      create/destroy reliability loop with `pw-dump` before/after diffing, a
      GNOME Shell journal crash/restart check (exact journal filter confirmed
      live against this host, not assumed), and the 10 s/cycle bounded timeout
      (step 1). Evidence to `docs/experiments/evidence/exp04/`.
  - Files: `crates/blackroom-experiments/src/bin/exp04_virtual_monitor.rs`,
    `crates/blackroom-experiments/Cargo.toml` (if PipeWire frame receipt is
    reused here)
  - Depends on: step 3, step 1 (masking active)
  - Verify: all 3 resolutions confirmed via a real `GetCurrentState` monitor
    entry; 50/50 cycles clean (`pw-dump` diff empty, no Shell crash in
    journal); evidence recorded.

- [x] 6. Port into production `crates/blackroom-gnome/src/mutter/
      {virtual_monitor.rs, pipewire_capture.rs}`. Add `pipewire` to
      `crates/blackroom-gnome/Cargo.toml` (first production use — research its
      event-loop/threading model against current crate docs first, Evidence
      #7). Update `mutter/mod.rs` accordingly.
  - Files: `crates/blackroom-gnome/src/mutter/{virtual_monitor.rs,
    pipewire_capture.rs, mod.rs}`, `crates/blackroom-gnome/Cargo.toml`,
    `crates/blackroom-gnome/src/lib.rs`
  - Depends on: step 5
  - Verify: `cargo test -p blackroom-gnome`; `cargo clippy -p blackroom-gnome
    --all-targets -- -D warnings`; `cargo deny check`.

- [x] 7. Implement `exp05_virtual_active.rs` (Doc 10 Exp 5 / Doc 02 §11): add
      the virtual monitor as an **additional** enabled logical monitor
      (temporary `ApplyMonitorsConfig`, physical monitor(s) stay enabled —
      Decision #3); verify desktop/apps/workspaces render and are usable on
      it, primary-output/window-placement behaviour; restore the original
      topology on exit (`GetCurrentState` diff against the pre-run snapshot).
      Evidence to `docs/experiments/evidence/exp05/`. After this run, unmask
      `gnome-remote-desktop` (`systemctl --user unmask
      gnome-remote-desktop.service`; restart only if it was running before
      step 1).
  - Files: `crates/blackroom-experiments/src/bin/exp05_virtual_active.rs`
  - Depends on: step 5
  - Verify: live run + manual confirmation an app window renders on the
    virtual monitor; post-run `GetCurrentState` matches the pre-run snapshot
    exactly (physical topology unchanged); `gnome-remote-desktop` confirmed
    unmasked afterward.

- [x] 8. Update `capability.rs`: promote `remote_desktop_capable`,
      `screencast_capable`, `pipewire_capable`, `virtual_display_capable` per
      the decision rule (Evidence #1) and the actual step 3/5/7 outcomes;
      each gets its own doc comment citing the evidence (no longer sharing
      `NOT_YET_DETERMINABLE`, which stays only for the other 3 constants).
      Update/add unit tests for the new fixed values.
  - Files: `crates/blackroom-gnome/src/mutter/capability.rs`
  - Depends on: steps 3, 5, 7
  - Verify: `cargo test -p blackroom-gnome`; grep-based blast-radius check
    confirming nothing downstream assumed these 4 stay `Unknown` forever.

- [x] 9. Re-issue `docs/gnome/capability-report.md` with the promoted tiers and
      updated evidence pointers. Create `docs/gnome/virtual-display.md`
      documenting: the discovered session-sub-object signatures, the two
      resolved open questions (tier promotions + `GnomeBackend`-assembly
      deferral, Evidence #1–#2), and the Decision #3 scoping boundary.
  - Files: `docs/gnome/capability-report.md`, `docs/gnome/virtual-display.md`
  - Depends on: step 8
  - Verify: manual cross-check of every citation against the live evidence
    files it references.

- [x] 10. Update `docs/security/architecture.md` §6: `pipewire` evaluated→in-use
       with the exact version `Cargo.lock` pins.
  - Files: `docs/security/architecture.md`
  - Depends on: step 6
  - Verify: `cargo deny check` green.

- [x] 11. Update `docs/HANDOFF.md`, `/memories/repo/blackroom-console.md`, and a
       MemPalace checkpoint (wing `blackroom_console`) reflecting Phase 4
       completion, including both resolved decisions and the promoted tiers.
  - Files: `docs/HANDOFF.md`
  - Depends on: steps 1–10
  - Verify: `python3 .github/skills/project-doctor/scripts/doctor.py` reports
    0 errors/0 warnings; `docs/HANDOFF.md` stays ≤ 40 lines / 3 KB.

- [x] 12. Full workspace verification and independent review: `cargo test
       --workspace`, `cargo fmt --check`, `cargo clippy --workspace
       --all-targets -- -D warnings`, `cargo check --workspace --all-targets`,
       `cargo deny check`, `cargo audit`; re-index `Blackroom_Console`
       (`name=Blackroom_Console` explicit); independent Reviewer-agent pass
       (Phases 2–3 both found real gaps this way).
  - Files: none new
  - Depends on: steps 1–11
  - Verify: all commands exit green; Reviewer subagent verdict recorded in
    the Execution Log.

## Final Verification

- Run the configured project checks from `AGENTS.md` (`cargo test --workspace`;
  `cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings`;
  `cargo check --workspace --all-targets`).
- Confirm every acceptance criterion above with current evidence (live-host
  output, not just green tests).
- Cross-check against Doc 11 §39's GNOME-feature Definition of Done: real GNOME
  environment tested (yes); Wayland tested (already established, unaffected);
  Mutter interaction verified (yes, RemoteDesktop/ScreenCast/RecordVirtual live);
  PipeWire behavior verified (yes, real frames + `pw-dump` leak check); failure
  behavior tested (yes, cleanup-on-error paths); restoration tested (yes, topology
  restore in `exp05`); version assumptions documented (yes,
  `docs/gnome/virtual-display.md`).
- Confirm Gate **FEAS-B** decision is explicitly recorded (PASS/PARTIAL/FAIL per
  Doc 00 §69 vocabulary), not left implicit.

## Blockers

- None. Decision #3 (Risks, last item) received explicit user sign-off
  2026-09-05, after the user separately confirmed this host is a laptop
  whose inbuilt panel (`eDP-1`) can never be physically unplugged (only the
  external `HDMI-1` can) — approved "as is" with no changes requested.

## Execution Log

- 2026-09-05: Plan drafted. No code written yet.
- 2026-09-05: User approved. Steps 1–5 executed: `docs/ops/experiment-safety.md`
  updated and `gnome-remote-desktop.service` masked (step 1); `pipewire` added to
  `blackroom-experiments`, introspection scanner promoted to
  `blackroom_experiments::introspect`, `exp02` re-verified byte-identical (step 2);
  `exp03_capture.rs` run live — **PASS** (`docs/experiments/evidence/exp03/2026-09-05/`):
  `ScreenCast.CreateSession` + `RecordMonitor` captured the existing desktop
  **without** needing a `RemoteDesktop` pairing (resolves Doc 02 §9's "if
  required" — not required for basic capture), 1 real BGRx 1920×1080 frame
  received, clean teardown confirmed both same- and fresh-connection (step 3);
  `remote_desktop.rs`/`screencast.rs` ported into `blackroom-gnome` (step 4);
  `exp04_virtual_monitor.rs` run live — **PASS** (`docs/experiments/evidence/
  exp04/2026-09-05/`): all 3 resolutions (1280×720/1920×1080/2560×1440 @60Hz)
  confirmed via `GetCurrentState` and torn down cleanly; 50/50 reliability
  cycles clean (0 leaked screencast video nodes before/after, GNOME Shell PID
  unchanged throughout — Shell survived) (step 5).
- 2026-09-05: **Session interrupted by a real system crash/reboot** (`uptime`
  showed the host up only ~11 min on resumption; `journalctl -b -1` confirmed a
  clean `systemd-logind`-initiated reboot, not a kernel panic — but
  `journalctl -k -b -1` showed the kernel OOM killer fired at 17:33:52 local,
  ~61s **after** `exp04`'s own report.md was written at 17:32:51, killing VS
  Code's own process (`code`, pid 436087, oom_score_adj:300 — a designed
  low-priority target, not a crash-worthy fault in it). Root cause: this host
  has essentially no swap (149 MiB total) and was concurrently running an
  unrelated Docker stack (7 `comm-*` containers incl. `clamd` at ~1 GiB RSS)
  plus a long VS Code/agent session; the OOM most likely tipped over during a
  follow-up `cargo` command after `exp04` had already exited cleanly, not from
  a leak in this project's PipeWire/Mutter code — `exp04`'s own evidence (50/50
  clean cycles, 0 leaked nodes, Shell PID unchanged) was captured *before* the
  OOM event. `gnome-remote-desktop.service` remained correctly masked across
  the reboot (systemd mask state persists). On resumption: re-verified the full
  tree with `cargo check/test/fmt --check/clippy -D warnings --workspace` — all
  green, no file left half-written. Steps 1–5 confirmed complete from their own
  evidence files and re-verified test/lint state, checkboxes updated
  accordingly. Resuming at step 6. Lesson for `docs/ops/experiment-safety.md`/
  future phases: avoid running a `cargo build`/`test`/`clippy` concurrently with
  or immediately after a mutating experiment on this host until more swap is
  configured or the unrelated Docker stack is stopped — noted for awareness,
  not actioned (out of this phase's scope).
- 2026-09-05: Step 6 executed. `virtual_monitor.rs`/`pipewire_capture.rs`
  ported into `blackroom-gnome`, informed directly by Experiment 3/4's proven
  D-Bus/PipeWire mechanics (including the "connector only registers after a
  real PipeWire client consumes the stream" ordering finding). Also added
  `ScreenCastSession::record_virtual` to the existing `screencast.rs` (small,
  direct-reuse addition — `RecordVirtual` lives on the same session interface
  `record_monitor` already wraps, confirmed by Experiment 3's introspection;
  avoids duplicating the whole session-wrapper struct). `cargo check/test/fmt/
  clippy -D warnings -p blackroom-gnome` and `cargo deny check` (new `pipewire`
  dependency) all green.
- 2026-09-05: Step 7 executed. **Live host now has both HDMI-1 (external 4K) and
  eDP-1 active** (2 physical logical monitors — the external monitor is no
  longer in the "connected but not composited" state Phase 0-1/3 evidence
  recorded; environment changed since then). `exp05_virtual_active` — **PASS**
  on the first live run (`docs/experiments/evidence/exp05/2026-09-05/`):
  `ApplyMonitorsConfig(Temporary)` added the virtual monitor (`Meta-0`) as a
  3rd logical monitor alongside both unchanged physical ones; both physical
  connectors stayed present/enabled throughout; `RecordMonitor` on the virtual
  connector received 2 real frames (proving the compositor actually renders
  onto it, not just registers an inert entry); explicit restore succeeded and
  the RAII `RestoreGuard` was cleanly disarmed. **Independently re-verified**
  (not just trusting the binary's own report): a fresh `gdbus`
  `GetCurrentState` call before vs. after, normalized for the serial counter,
  is **byte-identical**; `gnome-shell` PID alive throughout; 0 leaked
  `Stream/Input/Video` PipeWire nodes; memory stable (~7.2GiB used, 23GiB
  available). `gnome-remote-desktop.service` unmasked afterward (was inactive
  before masking, not restarted). Physical-monitor removal deliberately not
  attempted (Decision #3, deferred to Phase 5 Experiment 6).
- 2026-09-05: Step 8 executed. `remote_desktop_capable`/`screencast_capable`
  promoted `EXPERIMENTAL→SUPPORTED` (the existing live D-Bus presence check
  is unchanged; only the "present" ceiling was raised, matching evidence);
  `pipewire_capable`'s inline logic was extracted into a new pure
  `pipewire_capable_tier()` function (matching this file's established
  pure-tier-function pattern for non-D-Bus checks) and promoted the same way.
  `virtual_display_capable` promoted `UNKNOWN→SUPPORTED_WITH_LIMITATIONS` via
  a new named fixed constant `VIRTUAL_DISPLAY_PROVEN_WITH_LIMITATIONS`
  (removed from the shared `NOT_YET_DETERMINABLE` group, whose doc comment
  now correctly describes only the 3 remaining constants). 2 new unit tests
  added. Grep-based blast-radius check: only a `gnome-session-agent` test
  fixture and an `exp02` string-label table reference these fields/names —
  neither depends on the old values. `cargo test/fmt/clippy -D warnings
  --workspace` and `cargo deny check` all green (advisories/bans/licenses/
  sources ok).
- 2026-09-05: Steps 9–10 executed. `docs/gnome/capability-report.md` re-issued
  (4 rows updated, "Overall status" corrected to 2 remaining `UNKNOWN`
  constants relevant to remote-access activation); new
  `docs/gnome/virtual-display.md` written (session-sub-object findings, both
  decisions, the Decision #3 scope boundary, the experiment-safety.md scope
  update, non-goals). `docs/security/architecture.md` §6 updated (`pipewire`
  evaluated→in-use, version confirmed from `Cargo.lock`) and §7's dependency
  list corrected (also fixed a pre-existing staleness: `rustix`'s Phase 3
  promotion was never reflected there either). `cargo deny check` green.
- 2026-09-05: Step 11 executed. `docs/HANDOFF.md` updated (Phase 4, steps
  1–11 done, step 12 pending — not claimed complete yet). `/memories/repo/
  blackroom-console.md` gained a full Phase 4 section (findings, decisions,
  the OOM incident). MemPalace `mempalace_checkpoint`: 4 drawers (3 project,
  1 general cross-project OOM-diagnosis lesson to `wing_copilot`) + 1 diary
  entry, all succeeded (no read-only lock this session).
- 2026-09-05: Step 12 executed. Full suite green (`cargo test/fmt/clippy -D
  warnings/check --workspace`, `cargo deny check`, `cargo audit` — 0
  vulnerabilities across 201 deps); `Blackroom_Console` re-indexed
  (4438 nodes/7483 edges, explicit `name=`, 0 skipped/parse-partial).
  **Independent Reviewer-subagent verdict: FAIL initially**, 3 real gaps
  (matches this project's 3-for-3 pattern of review catching what
  self-review + live-run verification missed):
  1) `exp05`'s `RestoreGuard` was armed after the virtual monitor's
     `ScreenCast` session was already created/`RecordVirtual`'d — an early
     error in that window leaked the session with no cleanup. Fixed: a
     second RAII guard (`SessionStopGuard`) armed immediately after
     `CreateSession`.
  2) `exp05`'s recorded evidence was self-contradictory (virtual connector
     still listed after "restore" with `topology_restored: true`, no
     explanation) and the PASS predicate ignored the final `Stop()`'s
     result. Fixed: reordered restore→Stop()→poll-until-gone (discovered
     live: connector removal is *also* not synchronous with `Stop()`
     returning, symmetric to Experiment 4's appearance finding), added
     `stop_error`/`virtual_connector_fully_gone` to the evidence and the
     pass predicate. Re-ran live twice more (once mid-fix, once final) —
     both PASS, final run's evidence fully self-consistent.
  3) `REMOTE_DESKTOP_CAPABLE`'s promotion to `SUPPORTED` was unsupported by
     evidence — Experiment 3 only proved `Stop()`-without-`Start()`
     correctly errors, never a successful `Start()`/verified cleanup for
     `RemoteDesktop` specifically. Reverted to `EXPERIMENTAL` in
     `capability.rs`/`capability-report.md`/`remote_desktop.rs`; only
     `screencast_capable`, `pipewire_capable`, `virtual_display_capable`
     were genuinely promoted this phase (Decision #1 corrected). All fixes
     documented in `docs/gnome/virtual-display.md`.
  Mid-review re-run also hit an unrelated transient: a user-launched QEMU
  Windows-11 VM pushed available memory to ~5.7GiB (some swap in use) —
  paused, asked the user, resumed once the VM was closed and memory
  recovered to ~22GiB available; not a regression in this phase's code.
  Re-verified full suite green after all fixes; `gnome-remote-desktop`
  re-masked/re-unmasked correctly around the re-runs. **Phase 4 genuinely
  complete.**
