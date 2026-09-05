# Plan: Phase 0–1 — Repository Bootstrap, Environment Inventory and GNOME Research

**Created:** 2026-09-04
**Status:** approved
**Approved by:** user (Himanshu), 2026-09-05
**Task tier:** governed
**Roadmap position:** Stage I, Phases 0 and 1 of
[plan-20260904-blackroom-console-master-roadmap.md](plan-20260904-blackroom-console-master-roadmap.md).
Decisions and conflict resolutions referenced below live in
[assessment-20260904-detailed-project-plan.md](assessment-20260904-detailed-project-plan.md).

## Goal

Turn the documentation-only workspace into a buildable Rust workspace under Git, and
produce the Phase 1 deliverables required by Document 00 §47 (capability report, research
findings, feasibility-blocker list) using **read-only** experiments 0–2 from Document 10 on
this workstation — without touching display, input, lock state, or any privileged system
configuration.

## Acceptance Criteria

- `git log` shows an initial commit containing the existing scaffolding and plan documents,
  followed by small commits per step; `.gitignore` excludes `target/`, `node_modules/`,
  `.codebase-memory/`, editor files.
- `cargo check --workspace --all-targets`, `cargo fmt --check`,
  `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`,
  `cargo audit` and `cargo deny check` all pass on the bootstrapped workspace.
- Three binaries `exp00_environment`, `exp01_session_discovery`, `exp02_mutter_inventory`
  run as the desktop user, change no system state, and write redacted evidence under
  `docs/experiments/evidence/exp00-02/` in the Document 10 §47 result format.
- `docs/gnome/api-inventory.md` lists every interface, method, signal and property actually
  exposed on this host for `org.gnome.Mutter.*`, `org.gnome.Shell.ScreenShield`,
  `org.gnome.ScreenSaver`, `org.freedesktop.login1.{Manager,Session}`, with captured
  introspection XML.
- `docs/gnome/feasibility-research.md` answers every mandatory research topic in
  Document 00 §50 for GNOME 50.1 / Mutter 50.1 with sources and a
  `CONFIRMED / LIKELY / UNVERIFIED / UNSUPPORTED` label per finding.
- `docs/gnome/capability-report.md` classifies each Document 20 §8 capability constant for
  this host using the Document 00 §35 tiers; anything not yet experimentally shown is
  `UNKNOWN`.
- `docs/security/architecture.md` records the stack, naming, IPC, crypto and numeric-default
  decisions (assessment §6) and a crate inventory (name, version, licence, purpose,
  privilege domain, maintenance signal).
- `docs/ops/experiment-safety.md` documents the out-of-band recovery procedure
  (assessment §8) and SSH from the second device has been verified once.
- A Phase 0 and a Phase 1 report in Document 00 §68 format are appended to the Execution
  Log and summarised in `docs/HANDOFF.md`; the Phase 1 "feasibility blockers" list is
  written even if empty.

## Non-Goals

- No state-machine code, no GNOME mutation (no RemoteDesktop/ScreenCast sessions, no
  `ApplyMonitorsConfig`, no EIS, no lock calls) — those start in Phases 2–4.
- No crates beyond what Phase 1 binaries need (`zbus`, `serde`, `serde_json`, `clap`,
  `tracing`, `tracing-subscriber`, `anyhow`, `time`); other crates are only *evaluated* and
  recorded.
- No systemd units, packaging, browser code, network code, or CI pipeline definitions.
- No edits to generated workflow files other than `docs/HANDOFF.md` and the already
  approved `AGENTS.md` command table.

## Evidence And Decisions

- Evidence: assessment §2 (no Git repo, no code), §3 (host inventory: GNOME 50.1, Mutter
  50.1, missing `-dev` packages, hybrid GPU, `InputCapture` present), §8 (safety plan).
- Evidence: Document 10 Exp 0 (Environment Discovery), Exp 1 (GNOME Session Discovery),
  Exp 2 (Mutter Capability Inventory) — all defined as non-destructive; Document 02 §7
  diagnostic-tool fields; Document 00 §50 research list; Document 20 §8 capability
  constants; Document 05 §12–§14 session-discovery rules (no `$DISPLAY`/process-name
  guessing, `XDG_SESSION_TYPE=wayland`, owner check).
- Decision: Rust workspace with `edition = "2024"`, `rust-toolchain.toml` pinned to
  `stable` (1.96 installed), `resolver = "3"`; only `crates/blackroom-core` (empty lib
  with a doc comment) and `crates/blackroom-experiments` are created now (lean-change
  contract; other crates are added by the phases that need them).
- Decision: evidence files redact the Unix username and hostname by default
  (`--no-redact` to disable) — Document 10 §46 "avoid collecting secrets", Document 13 §31
  minimal collection.
- Decision: `cargo-deny` and `cargo-audit` are installed with `cargo install --locked`
  into `~/.cargo/bin` (developer tooling, not project dependencies).

## Risks

- Installing `-dev` packages and `openssh-server` requires `sudo`; the executor must ask
  the user to run the exact command and must not attempt privilege escalation.
- `apt source` for Mutter/gnome-shell/gnome-remote-desktop may be unavailable without
  `deb-src`; fall back to reading the GNOME GitLab tags matching the installed versions
  (`50.1`, `50.2`) — record which was used.
- zbus introspection of `org.gnome.Shell` can be large; write XML to files, never to the
  chat transcript.
- Two logind sessions exist on seat0; Exp 1 must show *why* one was selected.
- Do not run experiments while `gnome-remote-desktop.service` (user) is active; Exp 0
  records its state, nothing stops it in this phase.

## Steps

- [x] 1. Initialise Git and commit the current tree
  - Files: `.gitignore`, `.gitattributes` (`* text=auto`), initial commit.
  - Depends on: none.
  - Verify: `git status --porcelain` empty after commit; `git log --oneline | wc -l` = 1.

- [x] 2. Create the Rust workspace skeleton
  - Files: `Cargo.toml` (workspace members `crates/*`, `[workspace.package]` license
    `GPL-3.0-or-later` (user decision 2026-09-05), `[workspace.lints]` with
    `unsafe_code = "forbid"` for `blackroom-core`), `LICENSE` (GPL-3.0 text),
    `rust-toolchain.toml`,
    `crates/blackroom-core/{Cargo.toml,src/lib.rs}` (empty lib, `#![forbid(unsafe_code)]`),
    `crates/blackroom-experiments/{Cargo.toml,src/lib.rs}` (shared `evidence` module:
    result-format writer, redaction, ISO-8601 UTC timestamps), `deny.toml` (advisories deny
    unmaintained/vulnerable; licenses allow MIT/Apache-2.0/BSD/ISC/Zlib/MPL-2.0/
    GPL-3.0-or-later/LGPL-2.1+; bans wildcard versions), `Cargo.lock`.
  - Depends on: step 1.
  - Verify: `cargo check --workspace --all-targets && cargo fmt --check && cargo clippy
    --workspace --all-targets -- -D warnings && cargo test --workspace`; `cargo install
    --locked cargo-deny cargo-audit` then `cargo deny check && cargo audit` pass.

- [x] 3. Create documentation directories and the safety procedure
  - Files: `docs/gnome/README.md`, `docs/security/README.md`, `docs/experiments/README.md`
    (evidence layout `evidence/expNN/<date>/{report.md,*.json,*.xml}`), `docs/protocol/
    README.md`, `docs/ops/README.md`, `docs/ops/experiment-safety.md` (assessment §8:
    SSH prerequisite, armed restore watchdog rule, VT fallback limits, snapshot rule,
    `gnome-remote-desktop` masking rule).
  - Depends on: step 1.
  - Verify: files exist; links resolve (`grep -o '](\S*)' | check`); commit.

- [x] 4. Run the project doctor and update the handoff for Phase 0
  - Files: `docs/HANDOFF.md` (Active plan = this file; stopping point; checks run).
  - Depends on: steps 1–3.
  - Verify: `python3 .github/skills/project-doctor/scripts/doctor.py` reports no missing
    required file; Phase 0 report appended to the Execution Log below.

- [x] 5. Ask the user to install prerequisites (no sudo by the agent)
  - Files: none (record the command and result in the Execution Log).
  - Depends on: step 4.
  - Verify: user runs `sudo apt install libei-dev libeis-dev libpipewire-0.3-dev
    libspa-0.2-dev libgstreamer1.0-dev libgstreamer-plugins-base1.0-dev
    libgstreamer-plugins-bad1.0-dev gstreamer1.0-nice libpam0g-dev libinput-dev
    libinput-tools libudev-dev libclang-dev evtest` (all names verified against the 26.04
    archive on 2026-09-05; `openssh-server` already installed); agent confirms with
    `pkg-config --modversion libei-1.0 libeis-1.0 libpipewire-0.3 gstreamer-1.0
    gstreamer-webrtc-1.0 pam` and `systemctl is-active ssh.socket`; user confirms
    `ssh <user>@192.168.1.50` works from the tablet/phone with a **key** installed in
    `~/.ssh/authorized_keys` (currently empty; password auth is still enabled and should be
    turned off in `/etc/ssh/sshd_config.d/` once the key works — record the choice in
    `docs/ops/experiment-safety.md`).

- [x] 6. Experiment 0 — environment discovery (read-only)
  - Files: `crates/blackroom-experiments/src/bin/exp00_environment.rs`; evidence
    `docs/experiments/evidence/exp00/<date>/`.
  - Depends on: step 2.
  - Verify: binary collects OS release, kernel, `gnome-shell --version`, dpkg versions of
    `mutter`, `gnome-shell`, `gnome-remote-desktop`, `pipewire`, `wireplumber`, `libei1`,
    `libeis1`, `xdg-desktop-portal-gnome`, `systemd`, GPU list from `/sys/class/drm` +
    `lspci -nn`, loaded DRM/GPU modules, `XDG_SESSION_TYPE`, `gnome-remote-desktop`
    unit state, connected outputs (from `/sys/class/drm/*/status`), input devices from
    `/proc/bus/input/devices` (names and bus, no serials) — and writes `report.md` +
    `environment.json`; running it twice produces identical content except timestamp; no
    file outside the evidence directory changes (`inotifywait`/`git status` check).

- [x] 7. Experiment 1 — GNOME session discovery (read-only, system bus)
  - Files: `crates/blackroom-experiments/src/bin/exp01_session_discovery.rs`; evidence
    `docs/experiments/evidence/exp01/<date>/`.
  - Depends on: step 6.
  - Verify: via `org.freedesktop.login1.Manager.ListSessions` and `Session` properties
    (`Type`, `Class`, `Seat`, `Active`, `State`, `User`, `Display`, `Desktop`,
    `LockedHint`, `Scope`), the binary lists all sessions, selects exactly one with
    `Type=wayland`, `Class=user`, `Seat=seat0`, `User=<current uid>`, `Active=true`, and
    prints the selection rationale; asserts `XDG_SESSION_TYPE=wayland` and that
    `WAYLAND_DISPLAY`/`DBUS_SESSION_BUS_ADDRESS` are set for the current process (values
    redacted); reports `Desktop` and the user-manager unit for the session (`systemctl
    --user list-units 'gnome-session*' 'graphical-session*'`); fails closed with a
    `SESSION_NOT_FOUND`/`WAYLAND_UNAVAILABLE` code when run under `env -u
    WAYLAND_DISPLAY` (negative test recorded).

- [x] 8. Experiment 2 — Mutter/Shell/logind capability inventory (read-only, introspection)
  - Files: `crates/blackroom-experiments/src/bin/exp02_mutter_inventory.rs`;
    `docs/gnome/introspection/*.xml`; `docs/gnome/api-inventory.md`; evidence
    `docs/experiments/evidence/exp02/<date>/`.
  - Depends on: step 7.
  - Verify: for each name — `org.gnome.Mutter.DisplayConfig`, `.RemoteDesktop`,
    `.ScreenCast`, `.InputCapture`, `.InputMapping`, `.ServiceChannel`, `.IdleMonitor`,
    `org.gnome.Shell.ScreenShield`, `org.gnome.ScreenSaver`, `org.freedesktop.login1`
    (Manager + the selected Session object) — `Introspect` is called, XML saved, and the
    inventory table lists interface/method/signature/signals/properties, plus the
    `Version` properties of `RemoteDesktop`/`ScreenCast`, `DisplayConfig.GetCurrentState`
    summary (connectors, current modes, primary; EDID serials hashed), and whether
    `RecordVirtual`, `ConnectToEIS`, `ApplyMonitorsConfig`, `InputCapture.CreateSession`
    exist; each capability in Document 20 §8 gets a provisional `AVAILABLE / NOT
    AVAILABLE / UNKNOWN` presence flag (presence ≠ support, Doc 13 §16); no session or
    object is created (verify with `busctl --user tree org.gnome.Mutter.ScreenCast` before
    and after).

- [x] 9. Research findings — Document 00 §50 topics for GNOME 50.1 / Mutter 50.1
  - Files: `docs/gnome/feasibility-research.md`.
  - Depends on: step 8.
  - Verify: one section per topic with question, sources (Mutter 50.1 source paths, e.g.
    `src/backends/meta-screen-cast-session.c` RecordVirtual, `meta-remote-desktop-session.c`
    ConnectToEIS, `meta-input-capture-session.c`, `meta-monitor-manager.c`/
    `meta-monitor-config-manager.c` ApplyMonitorsConfig validation and zero-monitor rules,
    `meta-monitor-manager-dummy`/virtual monitor handling; gnome-shell 50.1
    `js/ui/screenShield.js`; gnome-remote-desktop 50.2 headless/screen-share and lock
    handling; libei 1.5 docs; systemd 259 `pam_systemd`/`unix_chkpwd`; Ubuntu 26.04
    `gnome-session` systemd user targets), findings, confidence label, and the experiment
    (Phase/Exp) that will turn `LIKELY`/`UNVERIFIED` into `CONFIRMED`. Topics: Mutter
    private API stability; virtual-monitor lifecycle; DisplayConfig (incl. all-physical-
    disabled configs, hybrid-GPU considerations); GNOME locking vs RemoteDesktop
    sessions; libei/EIS; physical input isolation candidates (`InputCapture` first, then
    RemoteDesktop/InputMapping, then `EVIOCGRAB` helper); PipeWire lifecycle; systemd
    privilege boundaries (user-unit startup, system→user control options, polkit); GPU-
    specific behaviour (NVIDIA proprietary + Intel `xe`/`i915`, hardware cursors, encoders
    via `gst-inspect-1.0 | grep -E 'va|nv|x264'`).

- [x] 10. Capability report and architecture decisions
  - Files: `docs/gnome/capability-report.md`; `docs/security/architecture.md`.
  - Depends on: steps 8–9.
  - Verify: capability report classifies every Document 20 §8 constant with the Doc 00
    §35 tiers and an evidence pointer (Exp 0–2 or "pending Phase N"); overall status is
    `UNKNOWN → activation blocked` (expected at this stage). Architecture document records
    assessment §6 decisions verbatim as project decisions, the crate inventory (evaluated
    via crates.io metadata: `zbus`, `pipewire`, `gstreamer` + `gst-plugins-rs` webrtc,
    `reis`, `evdev`, `tokio`, `serde`/`serde_json`/`schemars`, `rustix`, `argon2`,
    `totp-rs`, `ed25519-dalek`, `rand`/`getrandom`, `zeroize`, `secrecy`, `rusqlite`,
    `axum`, `hyper`, `rustls`, `tracing`, `tracing-journald`, `sd-notify`, `pam`/
    `pam-client`, `clap`, `ulid`), each with licence and last-release date, and marks
    any crate that fails the Document 00 §51 evaluation as rejected with the alternative.

- [x] 11. Phase 1 report, blockers list, handoff, memory
  - Files: this file (Execution Log), `docs/HANDOFF.md`, `docs/gnome/feasibility-research.md`
    ("Feasibility blockers" section).
  - Depends on: steps 5–10.
  - Verify: Document 00 §68 report present with `Verified / Partially Verified /
    Unverified / Blocked` lists; blockers list explicit (empty allowed); `docs/HANDOFF.md`
    names the next plan to create (`/plan-task` for Phase 2); MemPalace checkpoint written
    to wing `blackroom_console` with the research synthesis; `cargo test --workspace`,
    `cargo clippy`, `cargo fmt --check`, `cargo deny check`, `cargo audit` green.

## Final Verification

- Run the configured project checks from `AGENTS.md`
  (`cargo test --workspace`; `cargo fmt --check && cargo clippy --workspace --all-targets
  -- -D warnings`; `cargo check --workspace --all-targets`) plus `cargo audit` and
  `cargo deny check`.
- Confirm every acceptance criterion with current evidence (evidence directories, the four
  research/inventory documents, the safety procedure, the Git history, the handoff).
- Independent read-only review (Reviewer agent) of the research conclusions before Phase 2
  planning, per the `governed` policy.

## Blockers

- (none) — step 5 resolved 2026-09-05 (dev headers confirmed installed, tablet
  key-based SSH verified, password authentication disabled and confirmed rejected; see
  Execution Log).

## Execution Log

- 2026-09-04: plan drafted; awaiting approval. No steps executed.
- 2026-09-05: approved by user; licence GPL-3.0 (`GPL-3.0-or-later`); `openssh-server`
  installed (`ssh.socket` active); dev-header package names verified in the archive.
- 2026-09-05 (Executor session): Step 1 done. Branch renamed `master`→`main`. Initial
  commit `ea68a96` ("Initial commit: documentation, plans, and generated workflow
  scaffolding", 60 files: docs, plans, `.github/`, `.vscode/`, `.workflow_configurator/`).
  Verified: `git log --oneline` = 1 line; `git status --porcelain` = 0 lines.
- 2026-09-05 (Executor session): Step 2 done. Workspace root `Cargo.toml` (resolver "3",
  edition 2024, license `GPL-3.0-or-later`, `rust-version 1.96`, `[workspace.lints.rust]
  unsafe_code="forbid"`); `rust-toolchain.toml` (channel stable + rustfmt/clippy);
  `LICENSE` (verbatim GPL-3.0 text, cross-checked against two independent mirrors);
  `crates/blackroom-core` (empty lib, `#![forbid(unsafe_code)]`, opts into workspace
  lints); `crates/blackroom-experiments` (shared `evidence` module: `ExperimentReport`/
  `ExperimentResult` Doc10 §47 writer, `redact()`, ISO-8601 UTC via `time`, plus `cli`
  module with shared `--no-redact` flag); `deny.toml` (advisories deny, licenses allow
  MIT/Apache-2.0/BSD-2/BSD-3/ISC/Zlib/MPL-2.0/GPL-3.0-or-later/LGPL-2.1-or-later/
  Unicode-3.0, wildcards deny). Dependencies added via `cargo add` against the live
  registry (real resolved versions, not guessed): anyhow 1.0.104, clap 4.6.6 (derive),
  serde 1.0.229 (derive), serde_json 1.0.151, time 0.3.55 (formatting/macros/parsing),
  tracing 0.1.44, tracing-subscriber 0.3.23 (env-filter), zbus 5.19.0 (default features,
  no tokio). Installed `cargo-deny` 0.18.x and `cargo-audit` 0.22.2 via
  `cargo install --locked`.
  Verified (all green): `cargo check --workspace --all-targets`; `cargo fmt --check`;
  `cargo clippy --workspace --all-targets -- -D warnings`; `cargo test --workspace`
  (0 tests, ok — Phase 2 adds real tests); `cargo deny check`
  (advisories ok, bans ok, licenses ok, sources ok); `cargo audit` (125 crate deps
  scanned against 1239 advisories, 0 findings, exit 0). Commits `50e4b4e`, `cb421f0`.
- 2026-09-05 (Executor session): Step 3 done. Added `docs/{gnome,security,experiments,
  protocol,ops}/README.md` and `docs/ops/experiment-safety.md` (out-of-band SSH
  prerequisite incl. current PENDING status, watchdog default N=45s, VT-fallback scope
  limit, snapshot rule, `gnome-remote-desktop` mask/unmask commands, explicit note that
  Phase 0–1 needs none of this because Exp 0–2 are read-only). No markdown links used in
  the new files (nothing to resolve). Commit `c14f3f7`.
- 2026-09-05 (Executor session): Step 4 done. `python3
  .github/skills/project-doctor/scripts/doctor.py` → `OK: 0 error(s), 0 warning(s)`.
  Phase 0 checkpoint: re-ran the full `AGENTS.md` battery
  (`cargo test --workspace`, `cargo fmt --check`,
  `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo check --workspace --all-targets`, `cargo deny check`, `cargo audit`) — all green,
  see Phase 0 report below. `docs/HANDOFF.md` updated (this session).

### Phase 0 report (Document 00 §68 format)

```text
Phase: 0 — Repository Discovery / Bootstrap
Status: Complete

Implemented:
- Git repository initialised (branch `main`), `.gitignore`/`.gitattributes`, initial
  commit of the pre-existing docs/plans/workflow scaffolding.
- Rust workspace: `blackroom-core` (empty lib, forbid-unsafe) and `blackroom-experiments`
  (shared evidence/report/redaction helpers for Phase 1), edition 2024, licence
  GPL-3.0-or-later, `rust-toolchain.toml` pinned to stable.
- `deny.toml` (advisories/licenses/bans/sources policy) and `LICENSE` (verbatim GPL-3.0).
- `docs/{gnome,security,experiments,protocol,ops}/README.md` and
  `docs/ops/experiment-safety.md`.

Verified:
- `git log --oneline` / `git status --porcelain` match acceptance criteria.
- `cargo check/fmt/clippy(-D warnings)/test --workspace` all green (0 tests by design).
- `cargo deny check`: advisories ok, bans ok, licenses ok, sources ok.
- `cargo audit`: 125 crate dependencies scanned against 1239 RustSec advisories, 0
  findings.
- `python3 .github/skills/project-doctor/scripts/doctor.py`: 0 errors, 0 warnings.

Partially Verified: (none)
Unverified: (none)
Failed: (none)
Blocked: (none) — step 5 (Phase 1) is a separate, explicitly-tracked blocker below.

Tests:
- passed: 0
- failed: 0
- skipped: 0
- environment-dependent: 0
(No test code exists yet by design; Phase 2 adds state-machine tests against the mock
GnomeBackend.)

Security Impact: None yet — no I/O or privileged code exists. `forbid(unsafe_code)` is
enforced on `blackroom-core` via workspace lints; `deny.toml` blocks unlicensed/rejected
licences and known-vulnerable/yanked advisories workspace-wide.
Compatibility Impact: None.
Performance Impact: None.

Known Risks:
- `cargo deny check` reports informational "license was not encountered" warnings for
  BSD-2-Clause/BSD-3-Clause/ISC/Zlib/MPL-2.0 (allow-listed per assessment §6.1 for crates
  not yet added, e.g. future GNOME/PipeWire bindings). Not a failure.

Next Recommended Phase: Phase 1 steps 5–11 (environment/GNOME research). Step 5 needs
user action (see Blockers) but does not block steps 6–10, which use only pure-Rust
crates (zbus/serde/clap/tracing/anyhow/time).
```
- 2026-09-05 (Executor session): Step 5 not executed by design (requires user `sudo` +
  a second-device SSH check). Documented in `docs/ops/experiment-safety.md` §1 and
  `docs/HANDOFF.md`; the exact command is recorded in this step's Verify block above.
  Confirmed steps 6–10 have no dependency on it (Phase 1 crate set is pure-Rust: zbus,
  serde, serde_json, clap, tracing, tracing-subscriber, anyhow, time — no libei/pipewire/
  gstreamer/pam bindings needed until later phases). Continuing to step 6.
- 2026-09-05 (Executor session): Step 6 done.
  `crates/blackroom-experiments/src/bin/exp00_environment.rs` collects: `/etc/os-release`;
  `uname -r`; `gnome-shell --version`; dpkg-query versions of mutter (special-cased to the
  installed `libmutter-18-0` ABI package — plain `mutter` does not exist on Ubuntu),
  gnome-shell, gnome-remote-desktop, pipewire, wireplumber, libei1, libeis1,
  xdg-desktop-portal-gnome, systemd; GPU list from `/sys/class/drm` (driver via
  `device/driver` symlink, PCI ID/slot via `device/uevent`) cross-referenced with
  `lspci -nn`; loaded GPU kernel modules from `/proc/modules`; `XDG_SESSION_TYPE`;
  `gnome-remote-desktop.service` state via `systemctl --user is-active` (any-exit-status
  variant, since `is-active` exits non-zero for `inactive`); connected outputs from
  `/sys/class/drm/*/status`; input devices from `/proc/bus/input/devices` (Name + Bus
  only, no serials/phys/sysfs paths). Writes `report.md` (Doc 10 §47 format) +
  `environment.json` to `docs/experiments/evidence/exp00/<UTC date>/`.
  Verified: `cargo check/clippy -D warnings/fmt --check` green (one `collapsible_if`
  fixed). Ran twice consecutively — `diff` of both `report.md` (excluding the `Date:`
  line) and both `environment.json` files were **identical**; `git status --porcelain`
  showed only `docs/experiments/evidence/` as new before/after both runs (no other file
  touched). `grep -ril` for the real username and hostname across the evidence directory
  found neither (redaction had nothing to redact for this experiment; the shared
  `redact()` helper is exercised for real in Experiment 1). Host facts confirmed live:
  Ubuntu 26.04.1 LTS, kernel 7.0.0-30-generic, GNOME Shell 50.1, Mutter 50.1-0ubuntu2.2,
  gnome-remote-desktop 50.2-0ubuntu0.1 (inactive), card0=nvidia/HDMI-A-1 connected,
  card1=i915/eDP-1 connected+DP-1/DP-2 disconnected, 23 input devices,
  `XDG_SESSION_TYPE=wayland`. Result: **PASS**. Commits `03a75e3` (binary),
  `7e3b4ba` (evidence).
- 2026-09-05 (Executor session): Step 7 done.
  `crates/blackroom-experiments/src/bin/exp01_session_discovery.rs` calls
  `org.freedesktop.login1.Manager.ListSessions` (signature confirmed live via `busctl
  introspect`: `a(susso)`) over the **system** bus (zbus 5.19 blocking API), then reads
  each `Session` object's `Type`/`Class`/`Seat` `(so)`/`Active`/`State`/`User` `(uo)`/
  `Display`/`Desktop`/`LockedHint`/`Scope`/`Name` properties (types confirmed live via
  `busctl get-property`). Selection requires a **unique** match on
  `Type=wayland ∧ Class=user ∧ Seat=seat0 ∧ User.uid=<current uid> ∧ Active=true`
  (current uid via `id -u`, no `$DISPLAY`/process-name guessing — Doc 05 §12–14); zero or
  ambiguous (>1) matches both fail closed. Asserts `XDG_SESSION_TYPE=wayland` and that
  `WAYLAND_DISPLAY`/`DBUS_SESSION_BUS_ADDRESS` are non-empty; reports `Desktop` and the
  `gnome-session*`/`graphical-session*` user units via `systemctl --user list-units`.
  `--self-test` spawns a child of itself with `WAYLAND_DISPLAY` removed
  (`Command::env_remove`) and a `BLACKROOM_EXP01_SUPPRESS_EVIDENCE=1` guard so the child
  computes the same classification but never writes evidence; exit codes are classified
  `Ok=0 / SessionNotFound=2 / WaylandUnavailable=3`.
  Verified live: 2 logind sessions found — session "2" (`seat0`, `wayland`, `user`,
  `active=true`) **SELECTED**; session "3" (`Type=unspecified`, `Class=manager`, no seat)
  rejected. Env assertions all true. `--self-test` child exited `3`
  (`WaylandUnavailable`) as expected → negative test **passed**. Ran twice — `report.md`
  (excluding `Date:`) and `session.json` byte-identical across runs; `git status
  --porcelain` showed only the evidence directory as new.
  **Bug caught and fixed during verification**: the first implementation redacted the
  username only in the human-readable `observed` text, not in the `session.json`
  structured sidecar — `grep -ril user docs/experiments/evidence/` found the raw
  username twice in the JSON. Fixed by redacting `SessionProperties.name`/`.display` at
  struct-construction time (before serialisation), not only at report-render time;
  re-verified clean with the same grep (`NO_USERNAME_LEAK`). Recorded as a lesson: redact
  structured/JSON evidence at the data-construction site, never rely on redacting only
  the rendered text.
  `cargo check/clippy -D warnings/fmt --check` green throughout. Result: **PASS**.
  Commits `5ad8f9c` (binary), `0e1b0ba` (fix + evidence).
- 2026-09-05 (Executor session): Step 8 done.
  `crates/blackroom-experiments/src/bin/exp02_mutter_inventory.rs` calls `Introspect()`
  (session bus for the 11 Mutter/Shell/ScreenSaver targets, system bus for
  `login1.Manager` + the Experiment-1-selected `login1.Session`), saves raw XML to
  `docs/gnome/introspection/*.xml`, and parses interfaces/methods/properties/signals with
  a small hand-written line scanner for GDBus's one-tag-per-line output (no XML crate
  added — outside the Phase 1 allow-list). Reads `RemoteDesktop`/`ScreenCast` `Version`;
  calls `DisplayConfig.GetCurrentState` (read-only) and summarises connectors/current
  modes/primary with EDID-derived serials hashed (`DefaultHasher`, non-cryptographic —
  sufficient to avoid printing the raw value). Classifies each Document 20 §8 constant as
  `AVAILABLE`/`NOT_AVAILABLE`/`UNKNOWN`/`N/A` (N/A = another experiment's domain). Every
  call is `Introspect`/`Get`/`GetAll`/`GetCurrentState` only — `RecordVirtual`,
  `ConnectToEIS`, `ApplyMonitorsConfig`, `InputCapture.CreateSession` are never invoked,
  only checked for presence in the introspected schema.
  Verified live: 13/13 targets introspected successfully (all 9 required interfaces +
  `IdleMonitor.Core` sub-object + both `Shell.ScreenShield` candidate paths + `login1`
  Manager/Session). `busctl --user tree` on `org.gnome.Mutter.{ScreenCast,RemoteDesktop,
  InputCapture}` diffed byte-identical before/after the run; `git status --porcelain`
  showed only the intended new evidence/doc files. `grep -ril` for the real username
  across evidence and docs found nothing.
  **Bug caught and fixed during verification**: the first implementation reconstructed
  the selected login1 session's object path by string-formatting the session id
  (`.../session/_2`), which does not match systemd's actual per-byte hex escaping of
  dynamic session ids (`id "2"` → `.../session/_32`); `Introspect` failed with
  `UnknownObject` and the experiment came back `PARTIAL` (12/13). Fixed by reusing the
  real `object_path` from the Experiment-1 `discover()` result instead of ever
  reconstructing a login1 path from a bare id. Re-verified 13/13, **PASS**.
  **Research findings for step 9** (full detail in session notes / feasibility-research.md):
  (1) `org.gnome.Shell.ScreenShield` (bus name, owned by `gnome-shell`) exposes **no**
  distinct interface at `/org/gnome/Shell/ScreenShield` (empty), `/org/gnome/ScreenShield`
  (nonexistent), or `/org/gnome/Shell` (only `org.gnome.Shell`/`.Extensions`) — it
  resolves to the classic `org.gnome.ScreenSaver` interface (`Lock`/`GetActive`/
  `SetActive`/`GetActiveTime`/`ActiveChanged`/`WakeUpScreen`) at `/org/gnome/ScreenSaver`
  on GNOME Shell 50.1. There is one lock interface, not two — significant for Gate A. (2)
  Mutter's `DisplayConfig` connector names differ from kernel DRM names from Experiment 0
  (`HDMI-1` vs `card0-HDMI-A-1`; `eDP-1` matches `card1-eDP-1`) — code must never assume
  these strings are interchangeable. (3) `DisplayConfig.HasExternalMonitor=false` and
  `GetCurrentState` currently reports only **one** logical monitor (`eDP-1`, primary)
  even though `HDMI-1`/`card0-HDMI-A-1` is physically `connected` — the external 4K
  monitor is not currently composed into the desktop; Gate C conclusions need
  re-verification with it actually active. (4) `RecordVirtual` (`ScreenCast.Session`) and
  `ConnectToEIS` (`RemoteDesktop.Session`) are session-scoped and only reachable after
  `CreateSession` (forbidden this phase) — marked `UNKNOWN`, deferred to source-level
  confirmation in `feasibility-research.md`. (5) `RemoteDesktop.Version=1`,
  `ScreenCast.Version=4`, `InputCapture.SupportedCapabilities=15`,
  `RemoteDesktop.SupportedDeviceTypes=7`.
  `cargo check/clippy -D warnings/fmt --check` green throughout. Result: **PASS**.
  Commits `2ec3dc2` (binary), `12a8959` (fix + evidence).
- 2026-09-05 (Executor session): Step 9 done. `docs/gnome/feasibility-research.md`
  covers all 9 Document 00 §50 topics (Mutter private API stability; virtual-monitor
  lifecycle; DisplayConfig incl. hybrid-GPU; GNOME locking vs RemoteDesktop; libei/EIS;
  physical input isolation candidates; PipeWire lifecycle; systemd privilege boundaries;
  GPU-specific behaviour), each with question/sources/findings/confidence
  (`CONFIRMED`/`LIKELY`/`UNVERIFIED`, Doc 01 §51 vocabulary per C12)/escalation
  experiment, plus a "Feasibility blockers" section (4 items, none blocking Phase 2,
  none Doc 00 §49 `UNSUPPORTED`).
  Grounded in: (a) Experiment 0–2 empirical evidence (already `CONFIRMED` live facts);
  (b) genuine source research — confirmed `gitlab.gnome.org/GNOME/mutter` has real
  `50.1`/`50.2`/`50.3`/`50.4`/`51.x` tags (fetched the `/-/tags` page), then read the
  actual `50.1`-tagged `src/backends/meta-input-capture-session.c` in full via its
  `/-/raw/` URL (the `/-/blob/` UI form triggers a Fastly bot-check CAPTCHA on this
  network — raw URLs do not).
  **Notable finding that corrects the existing risk register**: `InputCapture` is a
  barrier-crossing (KVM-edge-style) trigger model — a pointer motion must cross an
  `AddBarrier`-defined line adjacent to a logical-monitor edge to transition
  `ENABLED → ACTIVATED` and start EIS emulation — not an unconditional/permanent grab as
  assessment §7.1's candidate description phrased it. No code path for independent
  physical-keyboard interception was found in that file (routing decision lives in
  compositor/seat code not traced this phase). Labelled `LIKELY`/`UNVERIFIED`
  accordingly and escalated to Experiment 9 (Phase 7) rather than asserting Gate E is
  solved. Other confirmed findings: `org.gnome.Shell.ScreenShield` has no distinct
  interface (aliases to `org.gnome.ScreenSaver`, see step 8); Mutter/kernel connector
  naming mismatch (`HDMI-1` vs `HDMI-A-1`); `HasExternalMonitor=false` with the external
  4K monitor connected but not composited.
  `apt-get source mutter --dry-run` confirmed no `deb-src` configured (assessment §3
  risk realised exactly as predicted) — GitLab tags were the documented fallback.
  No code changes this step (`cargo check/clippy/fmt` unaffected). `grep -ril` for the
  real username found nothing in the new document.
- 2026-09-05 (Executor session): Step 10 done.
  `docs/gnome/capability-report.md` classifies all 16 Document 20 §8 capability
  constants with Document 00 §35 tiers and an evidence pointer: 7 `SUPPORTED`
  (`OS_SUPPORTED`, `GNOME_SUPPORTED`, `WAYLAND_SUPPORTED`, `SYSTEMD_SUPPORTED`,
  `SESSION_FOUND`, `MUTTER_CAPABLE`, `DISPLAY_CONFIG_CAPABLE`), 4 `EXPERIMENTAL`
  (`REMOTE_DESKTOP_CAPABLE`, `SCREENCAST_CAPABLE`, `PIPEWIRE_CAPABLE`,
  `SESSION_LOCK_CAPABLE`), 1 `SUPPORTED_WITH_LIMITATIONS` (`GPU_CAPABLE`), 4 `UNKNOWN`
  (`VIRTUAL_DISPLAY_CAPABLE`, `REMOTE_INPUT_CAPABLE`,
  `PHYSICAL_INPUT_ISOLATION_CAPABLE` — Gate E, and `EMERGENCY_CAPABLE`), 0
  `UNSUPPORTED`. Overall status **`UNKNOWN → activation blocked`**, exactly as the
  plan expects at this stage.
  `docs/security/architecture.md` records assessment §6.1–§6.5 verbatim (languages/
  toolchain/layout, names/paths/accounts, IPC/privilege-crossing, cryptography/secrets,
  numeric defaults) plus a 24-crate Document 00 §51 evaluation table (security/
  maintenance/licence/privilege domain/attack surface). Real crates.io data fetched via
  the lightweight search endpoint (`GET /api/v1/crates?q=<name>&per_page=1`) — the full
  `/api/v1/crates/{name}` endpoint returns the entire version history and proved too
  context-expensive after one trial fetch; switched approaches after that single
  observation (Executor operational-safety guidance: try an alternative after a failed
  approach rather than repeating it).
  **Finding — crate rejected with alternative recorded (Doc 00 §51 "actively
  maintained" criterion)**: both `pam` (last release 2023-11-01, 4 versions ever) and
  `pam-client` (last release 2022-07-30, 8 versions ever) are effectively unmaintained.
  Recorded alternative: `nonstick` (actively developed, last release 2026-03-13, but
  young/pre-1.0, first published 2025-04-15) — Phase 15 must run its own security
  review before adopting it, or reconsider shelling out to `pam_unix`-compatible
  tooling directly; not resolved this phase (Non-Goal). `ed25519-dalek` is
  BSD-3-Clause, not MIT (dalek-cryptography convention) — already covered by
  `deny.toml`'s allow list from step 2, no new exception needed.
  No code changes this step. `grep -ril` for the real username found nothing in either
  document.
- 2026-09-05 (Executor session): Step 11 done — Phase 1 checkpoint. Re-ran the full
  `AGENTS.md` battery: `cargo test --workspace` (0 tests, ok), `cargo fmt --check`,
  `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo check --workspace --all-targets`, `cargo deny check` (advisories/bans/
  licenses/sources ok), `cargo audit` (125 deps, 1239 advisories, 0 findings) — all
  green. `python3 .github/skills/project-doctor/scripts/doctor.py` → 0 errors/warnings.
  `git log --oneline | wc -l` = 19, `git status --porcelain` empty. Phase 1 report
  below; `docs/HANDOFF.md` updated; MemPalace checkpoint written to wing
  `blackroom_console` this session.

### Phase 1 report (Document 00 §68 format)

```text
Phase: 1 — Environment and Research
Status: Complete (step 5 excepted — user action pending, does not block Phase 2)

Implemented:
- Three read-only Document 10 experiment binaries in crates/blackroom-experiments/
  src/bin/ (exp00_environment, exp01_session_discovery, exp02_mutter_inventory)
  sharing a lib.rs evidence/redaction/report-format module and a session-discovery
  module (used by both exp01 and exp02).
- docs/gnome/api-inventory.md, docs/gnome/introspection/*.xml (13 files),
  docs/gnome/feasibility-research.md, docs/gnome/capability-report.md,
  docs/security/architecture.md.

Verified:
- Exp0: full environment report; byte-identical across two runs (excl. timestamp);
  git status showed only the evidence directory changed; no username/hostname in
  evidence (nothing to redact for this experiment).
- Exp1: unique GNOME Wayland session selected (Type=wayland/Class=user/Seat=seat0/
  User=<uid>/Active=true); --self-test negative test (WAYLAND_DISPLAY removed) failed
  closed with the expected classified exit code; deterministic across runs.
- Exp2: 13/13 D-Bus targets introspected successfully, read-only only (Introspect/Get/
  GetCurrentState); `busctl --user tree` on ScreenCast/RemoteDesktop/InputCapture
  byte-identical before/after (no session or object created).
- Capability report: 16/16 Document 20 §8 constants classified (7 SUPPORTED,
  4 EXPERIMENTAL, 1 SUPPORTED_WITH_LIMITATIONS, 4 UNKNOWN, 0 UNSUPPORTED); overall
  UNKNOWN -> activation blocked, as expected.
- Feasibility research: all 9 Document 00 §50 topics answered with sources,
  confidence labels, and escalation experiments; one source-level finding corrects
  the existing risk register (InputCapture is a barrier-crossing model, not an
  unconditional grab).
- cargo check/fmt/clippy(-D warnings)/test, cargo deny check, cargo audit: all green
  at both the Phase 0 and this Phase 1 checkpoint.
- Two real implementation bugs were caught during verification and fixed before being
  accepted: Experiment 1's JSON evidence leaked the username (redaction applied only
  to the text report, not the struct); Experiment 2 reconstructed a login1 session
  path incorrectly (systemd's per-byte id escaping) instead of reusing the real path.

Partially Verified:
- DisplayConfig.GetCurrentState is functionally exercised (real topology returned,
  matching Experiment 0's kernel-level facts) but the all-physical-disabled /
  zero-virtual-monitor question is untested (Exp 5/6, Phase 4/5).
- InputCapture's mechanism is confirmed present and its pointer-barrier activation
  path is understood from the real Mutter 50.1 source; whether it (plus Gate C) can
  achieve complete physical *keyboard* isolation was not established (Exp 9, Phase 7
  — Gate E, the project's highest risk).

Unverified:
- VIRTUAL_DISPLAY_CAPABLE, REMOTE_INPUT_CAPABLE, EMERGENCY_CAPABLE (capability-report.md)
  — pending Phase 3/4/6/10 experiments.
- Whether a RemoteDesktop/ScreenCast session survives GNOME `ScreenSaver.Lock` (Gate A,
  Phase 8, second-highest project risk).
- Whether `pam-auth-helper` needs root for `pam_unix`/`unix_chkpwd` on Ubuntu 26.04
  (assessment §7.7) — no direct evidence gathered this phase; escalated to Phase 15/16
  alongside the `pam`/`pam-client` -> `nonstick` crate finding.

Failed: (none)

Blocked:
- Phase 1 step 5: user must run the dev-header `apt install` (exact command recorded
  in the plan) and verify key-based SSH from a second device before Document 10
  Experiment 6 (Physical Output Isolation) or Experiment 9 (Physical Input Isolation)
  may run. The agent must not run sudo. Confirmed this does **not** block Phase 2
  (state-machine core against the mock GnomeBackend needs none of Phase 1's crates).

Tests:
- passed: 0
- failed: 0
- skipped: 0
- environment-dependent: 0
(No automated test code exists yet by design; Phase 2 introduces the state-machine
test suite against the mock GnomeBackend.)

Security Impact: No system mutation was performed at any point (verified live via
git status and busctl tree diffs, not merely asserted). All Phase 1 D-Bus calls are
read-only (Introspect/Get/GetAll/GetCurrentState/ListSessions). Evidence redaction has
one confirmed-fixed gap (see above); re-verified clean after the fix. cargo-deny
enforces licence/advisory policy on the (currently pure-Rust, unprivileged) dependency
set.

Compatibility Impact: None yet — no code targets specific hardware/GNOME versions
beyond read-only introspection of this exact host. capability-report.md flags
GPU_CAPABLE as SUPPORTED_WITH_LIMITATIONS pending Phase 9/24 GPU-matrix work.

Performance Impact: None (no runtime hot paths implemented yet).

Known Risks:
- Gate E (physical input isolation): mechanism better understood (barrier-crossing,
  pointer-triggered) but not proven for keyboard isolation specifically — Phase 7 must
  resolve this before Gate E can be marked PASS.
- Gate A (GNOME lock + RemoteDesktop interaction): completely unverified, second-highest
  project risk — Phase 8.
- `pam`/`pam-client` crates are unmaintained; `nonstick` is the recorded Phase 15
  evaluation candidate and needs its own security review before adoption.
- Step 5 (SSH + dev-header prerequisite) remains pending user action as of this report.

Next Recommended Phase: `/plan-task` for Phase 2 (State Machine Core: states,
transitions, invariants, transition IDs, idempotency against a mock `GnomeBackend`;
no real GNOME) after independent review of this Phase 1 report and its research
conclusions (`governed` policy: Reviewer agent, read-only).
```

- 2026-09-05 (Executor session, follow-up): Step 5 fully resolved. Dev-header packages
  confirmed installed (`pkg-config --modversion` for libei-1.0/libeis-1.0/libpipewire-0.3/
  gstreamer-1.0/gstreamer-webrtc-1.0/pam all resolved). User's tablet ed25519 public key
  installed in `~/.ssh/authorized_keys` (perms 700/600 verified); first login attempt hit
  a client-side typo (`19.168.1.15` vs `192.168.1.50`, diagnosed via `ssh -v` on the tablet
  showing the wrong target IP — server-side config was already correct: home 750, no
  `sshd_config.d` drop-ins, no `Match`/`AllowUsers`/`AuthenticationMethods` restrictions).
  Retried with the correct IP: user confirmed key-based login succeeded. User then ran
  (own sudo, not the agent): created `/etc/ssh/sshd_config.d/99-blackroom-key-only.conf`
  with `PasswordAuthentication no`, validated with `sshd -t`, reloaded via
  `systemctl try-restart ssh.service`. Verified by forcing password-only auth from the
  tablet (`ssh -o PubkeyAuthentication=no -o PreferredAuthentications=password ...`); user
  confirmed the expected `Permission denied (publickey)` response, i.e. password
  authentication is rejected and only the key works. `docs/ops/experiment-safety.md` §1
  updated accordingly. Experiment 6/9 prerequisite (out-of-band SSH channel) is now
  satisfied.


