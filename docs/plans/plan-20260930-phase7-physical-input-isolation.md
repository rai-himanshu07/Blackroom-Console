# Plan: Phase 7 physical input isolation (Gate FEAS-E)

**Created:** 2026-09-30
**Status:** draft; offline research and code only, no live run approved
**Approved by:** not yet approved (live steps need their own approval)
**Task tier:** governed (live input mechanism, recovery)

## Goal

Decide, with evidence, which mechanism can keep physical keyboard and pointer
input away from the local session while remote input still works, and what
recovery and privilege it needs. Mutter `InputCapture` is rejected at source
level (it captures injected input too); the plan moves to a minimal `EVIOCGRAB`
helper, offline design first. Gate FEAS-E stays UNPROVEN until a supervised run
observes it; an unsafe or failed mechanism is reported as STOP.

## Acceptance Criteria

- While isolated, physical keyboard and pointer produce no effect in the
  session, observed by an independent page, and remote input still works.
- Isolation fails closed: any ending of the grab (release, helper death, device
  removal, emergency chord) is detected and revokes remote control.
- Helper death or a failed release restores local input, and a local and a remote
  recovery path are named and checked before any live grab is approved.
- Privilege is minimal and stated; the helper is allow-listed and reviewed.

## Non-Goals

- No live grab, device open or privilege change in the offline steps.
- No installed service, udev rule or distinct UID yet.
- No FEAS-E, Gate C or Phase 10 claim from source reading.
- Multi-monitor layouts, every device matrix row and 50-cycle loops only if a
  specific failure or support claim requires them.

## Evidence And Decisions

- Evidence, Mutter 50.1 `src/backends/meta-input-capture-session.c` (read
  2026-09-30, raw source):
  - `meta_input_capture_session_process_event` consumes motion, button, scroll
    and key press/release, and drops them when the receiver did not bind that
    device type. So keyboard is isolated by an activated session, which closes
    the earlier "keyboard path unverified" gap at source level only.
  - Activation happens only in `on_barrier_hit`, i.e. a pointer crossing a
    sticky barrier. `AddBarrier` accepts only an axis-aligned line contained in
    exactly one logical-monitor edge; there is no direct "activate" call.
  - `on_monitors_changed` calls `disable`: any monitor change clears barriers and
    returns the session to INIT, so isolation silently ends (fail open) and only
    a `ZonesChanged` signal tells the client.
  - The EIS peer must be a Receiver, one client per session; devices are
    "captured relative pointer" (pointer, button, scroll) and "captured
    keyboard" (XKB keymap).
  - Activation notifies the GNOME remote-access controller, so a shell
    indicator with a Stop action exists (`handle_stop` closes the session).
  - Cancel waits until all captured keys and buttons are released.
- Evidence, this host: `org.gnome.mutter.keybindings cancel-input-capture`
  is `['<Super><Shift>Escape']`, a compositor-level local end of capture (an
  escape hatch and an isolation bypass).
- Evidence, Mutter 50.1 (read-only review of `display.c`, `events.c`,
  `meta-dbus-session-*.c`, `meta-seat-impl.c`, `meta-barrier-native.c`, 2026-09-30):
  - Owner loss closes the session: the watcher's `name_vanished_callback`
    closes every session of the vanished peer, which runs disable and restores
    routing. A dead EIS socket with a live D-Bus peer does not: the session stays
    ACTIVATED and keeps swallowing input.
  - The event router is one flag set in `meta_display_new`; the clutter filter
    `meta_display_handle_event` runs `meta_display_process_captured_input`
    first. There is **no device or virtual-device check**, and RemoteDesktop/EIS
    injected input takes the same `_clutter_event_push` path. So while a session
    is ACTIVATED, injected remote input is captured too and never reaches the
    desktop.
  - Touch, touchpad gestures and tablet events are not consumed.
  - The cancel chord works during capture (keypress, then all keys and buttons
    released). VT switching (`Ctrl+Alt+Fn`) is captured and is **not** a recovery
    path while activated. SysRq and the power button are outside Mutter
    (unverified).
  - Injected pointer motion triggers barrier hits like physical motion.
- Evidence, this host: the user is not in group `input`; `/dev/input/event*`
  are `root:input 0660` and `/dev/uinput` is `root` only, so an `EVIOCGRAB`
  helper needs root or group `input` (keylogger-class privilege).
- Decision: `InputCapture` cannot meet acceptance criterion 1 as designed
  (remote input would be captured with the physical input), so candidate 1 is
  rejected at source level and no live capture is planned for it. The next
  candidate is a minimal `EVIOCGRAB` helper: the kernel releases a grab when the
  fd closes, so helper death restores input, and the helper can see the grabbed
  devices' events to detect the emergency chord itself. It needs a privilege and
  hotplug design and its own review before any device is grabbed.
- Unknown: whether SysRq and the power button still work under a grab; whether
  a grab of the built-in keyboard and touchpad is enough on this laptop; udev
  hotplug latency; how a non-root helper gets device access safely.

## Risks

- An `EVIOCGRAB` helper holds keylogger-class access (every key of the grabbed
  keyboards) and needs root or group `input`; it must be tiny, allow-listed and
  reviewed. A grab of the only keyboard and touchpad locks out the operator if
  release fails; helper death must release (kernel behavior, to be observed).
- Hotplugged devices are not covered unless the helper watches udev.
- The local emergency chord, if implemented in the helper, is a bypass of
  isolation by design and must be stated in the threat model.
- Monitor and connector changes do not affect evdev grabs, but privacy of the
  display (Gate C) is separate and still STOP.

## Steps

- [x] 1. Read Mutter 50.1 `meta-input-capture*.c`.
  - Files: `docs/gnome/input-isolation-research.md`
  - Depends on: none
  - Verify: findings cite functions; unknowns listed.
- [x] 2. Close the offline unknowns (owner loss, event router and virtual-device
      exemption, VT switch and cancel chord, barrier trigger).
  - Files: `docs/gnome/input-isolation-research.md`
  - Depends on: step 1
  - Verify: each answer cites source; InputCapture rejected at source level for
    criterion 1. Gsettings default of the cancel chord was read on this host.
- [x] 3. Write the decision record and helper design: device allow-list and
      classification, `ISOLATE_INPUT`/`RESTORE_INPUT` only, udev hotplug, emergency
      chord in the `remote-emergencyd` binary, grab lease and watchdog, privilege
      model, release-on-death.
  - Files: `docs/security/input-isolation-decision.md`
  - Depends on: step 2
  - Verify: independent review done 2026-09-30; its four findings (emergency
    path independent of hostd, hung helper, mixed-function nodes, unit hardening
    wording) are applied in the record.
- [x] 4. Offline grabber logic behind a fake device source: node classification
      by capability bits, all-or-nothing isolate, release that retries, hotplug
      add and remove with fail-closed, a dead-man grab lease and the emergency
      chord detector. No `/dev/input` access; the caller supplies `Caps` and an
      `EVIOCGRAB` implementation.
  - Files: `crates/remote-input-helper/` (library), workspace `Cargo.toml`
  - Depends on: step 3
  - Verify: `cargo test -p remote-input-helper` (16 tests), clippy `-D warnings`.
    Not done: the privileged binary, evdev/udev I/O, the watchdog wiring and the
    chord choice; those stay behind step 5's approval.
- [ ] 5a. Live, separate approval, lowest risk first: grab only the external
      wireless keyboard and mouse nodes (`event6`, `event7` on this host) so the
      built-in keyboard and touchpad stay usable for local recovery. One bounded
      grab with an external kill timer, a grab lease and second-device SSH; the
      operator types and moves the external devices while the observer page must
      see nothing and remote input must still reach the page.
      Must observe: release on fd close and on SIGKILL, queued-event leakage,
      stuck-modifier/LED/repeat state after release, a stalled helper, hotplug of
      the dongle. The `EVIOCGRAB` layer is the `evdev` crate (chosen by the operator
      2026-10-01; cargo deny and audit green). Still needed at run time: a
      temporary device-access `setfacl` run by the operator on the two nodes (no
      group change), an armed external kill timer `blackroom-exp09-kill` (the
      probe refuses unless it is pending and outlasts the run), masked
      `gnome-remote-desktop`, and the exact `--nodes` and `--expect-phys-prefix`
      from the inventory (a replug or typo is refused).
  - Files: `crates/blackroom-experiments/src/bin/exp09_grab_probe.rs`
  - Depends on: step 4 and the two items above
  - Verify: physical input absent on the page while grabbed, remote input still
    works, release on timer, SIGKILL and explicit restore; Shell PID unchanged.
- [ ] 5b. Only if 5a passes and the built-in keyboard matters: one bounded grab of
      the built-in keyboard and touchpad nodes (`event2`, `event3`, `event4`,
      `event5`) with the same safeguards plus the emergency chord. SysRq is
      expected to stop while the keyboard is grabbed and is not a recovery path.
      The 50-cycle target is not claimed.
  - Files: `crates/blackroom-experiments/src/bin/exp09_grab_probe.rs`
  - Depends on: 5a and a new approval
  - Verify: as 5a, plus chord release and lock-out recovery by SSH.
- [ ] 6. Independent review and FEAS-E decision or STOP-and-report.
  - Files: `docs/gnome/capability-report.md`, `docs/HANDOFF.md`
  - Depends on: step 5
  - Verify: no promotion on source reading or unobserved assertions.

## Final Verification

- Focused tests per offline slice; one independent review before step 4 and
  again before step 5; no broad workspace run unless crates integrate.

## Blockers

- Live steps need operator presence, a second-device SSH session and a named
  recovery path. FEAS-C STOP and unproven FEAS-A/D do not block offline steps.

## Execution Log

- 2026-09-30: drafted from a read-only source review; nothing live was run.
- 2026-09-30 (step 3): decision record written and independently reviewed; the
  grabber is the `remote-emergencyd` binary, a grab lease and watchdog cover a
  hung helper, and hotplug sandboxing is an open review item.
- 2026-09-30 (step 2): a read-only source review found InputCapture captures
  injected remote input as well (no virtual-device exemption), so it cannot
  satisfy criterion 1; candidate 3 (`EVIOCGRAB` helper) is next, offline design
  first. Owner loss, cancel chord, VT-switch and barrier-trigger answers are in
  Evidence. Nothing live was run.
- 2026-09-30 (step 4, direction approved by the operator): added the pure-logic
  crate `remote-input-helper` (classification, isolation state machine, lease,
  chord detector) with 16 tests; no device, ioctl or privilege is touched.
- 2026-09-30 (step 5, read-only part): `exp09_input_inventory` classified this
  host's 23 input nodes without opening any device: 6 would be grabbed, 0 differ
  from udev tags; Fn-row hotkeys are on separate nodes; the grabbed keyboard node
  carries the kernel `sysrq` handler. The live grab part of step 5 is not done and
  needs approval and a privilege path.
- 2026-10-01 (step 5a prepared, offline): built `exp09_grab_probe` (three phases:
  physical input before, during and after the grab, plus one injected EIS Shift
  tap during it) on the `evdev` crate behind the `DeviceGrab` trait, and moved the
  observer server into a shared module. An independent review found that nodes
  were not bound to the inventoried path or seat, the kill timer was only
  substring-matched, and a lapsed lease was not a failure; all fixed with tests.
  The timer check was corrected against real systemd output (monotonic timers
  have no realtime field, so the JSON timer list is used). Nothing was opened or
  grabbed; the probe refuses without `--operator-present`.
