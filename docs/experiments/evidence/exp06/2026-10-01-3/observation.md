# Integrated probe, 2026-10-01 23:01-23:02: GNOME Shell SIGSEGV during the restore (session lost)

Run by the operator with `docs/ops/live-integrated-run.sh` (harness `2a243d7`): remote session, virtual monitor,
isolate eDP-1, real input grab, remote Shift/a/Left, hold, then the orderly restore. Operator opened the observer page
fullscreen and kept hands off. The GNOME session ended and the operator had to log in again (not a reboot: uptime
4 days; new login session 182, new Shell PID). Whether unsaved work was lost is not recorded.

Journal timeline (wall clock +05:30):
- 23:01:27 kill timer armed; 23:01:45 `Added virtual monitor Meta-0`, restore watchdog (120 s) armed, then at the
  isolation moment Mutter logged repeated `meta_monitor_manager_get_logical_monitor_from_number` and
  `meta_workspace_get_work_area_for_monitor` assertion failures, `Direct scanout page flip failed: Timer disarmed`
  and ubuntu-dock extension exceptions (`desktopData is null`).
- 23:02:11 `Client error: socket disconnected` (the remote session's EIS socket closing), the cleanup watchdog timer
  was armed (the program had reached the restore stage), and GNOME Shell crashed with signal 11 (apport crash report
  dated 23:02:11, 55 MB, kept at `/var/crash/_usr_bin_gnome-shell.1000.crash`; systemd saw the exit at 23:02:20 after
  the dump). 23:02:22 logind removed session 2.
- 23:02:19 the script's cleanup trap stopped the kill timer; the daemon, live dir and mask were cleaned. At 23:03:06
  the cleanup watchdog service failed in the new session (exp07 refuses a mismatched session, as designed).
- No `integrated.json` exists: the probe died with the session.

Native stack of the crashing thread (function names only; the core was unpacked to a private temp dir, read for the
backtrace and deleted): SIGSEGV inside two libmutter functions run from a `g_signal_emit` inside
`meta_monitor_manager_rebuild`, inside `meta_monitor_manager_apply_monitors_config`, called from a D-Bus method
handler: that is the probe's own restore `ApplyMonitorsConfig` re-enabling eDP-1. libmutter is stripped and the symbol
service gave no names. An earlier note here tied the handler to `meta_remote_desktop_session_*`; that was wrong: the
0x22000-byte exported-symbol gap after it also holds the screen-cast sources (see Source analysis).

Reading: this is a controlled reproduction of the unexplained historical Shell SIGSEGV class (apply/rebuild around
virtual-monitor and topology changes). What was new compared with about 13 clean exp06 runs (including the eDP-only
owner-kill at 22:34 today): a RemoteDesktop/EIS session stopped immediately before the restore, a PipeWire consumer on
the virtual monitor, and a fullscreen browser window moved onto the virtual monitor and back. The cause is NOT
established. The earlier RemoteDesktop-teardown guess is dropped in favour of the source-backed candidate below.

## Source analysis (offline, Mutter tag 50.1; the host runs 50.1-0ubuntu2.4, upstream main checked 2026-10-01)

`meta-screen-cast-virtual-stream-src.c` `on_monitors_changed` (connected to `monitors-changed-internal` for the life of
the stream) does, when the stream is enabled (a PipeWire consumer is streaming): `view = view_from_src (src);
setup_view (virtual_src, view);` with no NULL check. `view_from_src` returns NULL when the virtual monitor's CRTC has
no renderer view, i.e. when the monitors config in force has no logical monitor for the virtual monitor.
`setup_view` then calls `meta_stage_view_inhibit_cursor_overlay (META_STAGE_VIEW (view))` (default cursor mode =
hidden) which writes through the NULL instance-private pointer; in other cursor modes `make_frame_clock_passive` also
dereferences the view. Upstream main has the same unguarded handler (renamed `meta-stream-source-virtual.c`).

Fit with the evidence: (1) the probe's restore applies the original physical-only config (`restore_original`), which
omits Meta-0, so the virtual monitor loses its logical monitor/view; (2) the PipeWire capture consumer exists only in
`--integrated-probe` (`capture_until_stopped` thread) and was merely signalled, not joined, before the restore, so the
stream was still enabled; all ~13 clean exp06 runs had no streaming consumer at restore; (3) the crash is in a signal
handler run from `meta_monitor_manager_rebuild`; (4) the two libmutter frames 0x1fd bytes apart match a helper and its
caller (`setup_view` inside `on_monitors_changed`). Isolation itself is safe because Meta-0 is then the only logical
monitor and has a view. This candidate was then reproduced (next section). The same class may explain the unexplained
2026-09-26 SIGSEGV near virtual-monitor removal.

Implication: any topology change that leaves a virtual monitor without a logical monitor while its stream is enabled is
unsafe on this Mutter. Mitigation chosen by the operator and verified on the throwaway Shell below: keep the virtual
monitor as an extra logical monitor in the restore config, then Stop the ScreenCast session (Stop is safe: the stream
source clears its handler before releasing the virtual monitor). Waiting for the consumer to disconnect first was
rejected as racy (the disable is asynchronous).

## Headless reproduction (2026-10-02 00:03-00:10, throwaway Shell only)

`docs/ops/headless-repro.sh` starts `gnome-shell --headless --wayland --no-x11 --virtual-monitor 1920x1080` under
`dbus-run-session` (private bus, no real display or input, GSETTINGS_BACKEND=memory) and runs `exp13_virtual_restore`,
which refuses unless the DisplayConfig owner was started `--headless`. Flow: snapshot, `RecordVirtual`, streaming
PipeWire consumer, `disable_physical_outputs` (only the virtual monitor), hold 2 s, restore, Stop. The producer and
consumer nodes lived in the real PipeWire daemon and vanished with the process.

| Variant | Runs | Result |
|---|---|---|
| consumer streaming, restore omits the virtual monitor (the probe's restore) | 5 (one under gdb) | headless Shell SIGSEGV (status 139) 5/5 |
| no consumer at restore (earlier clean exp06 runs) | 2 | safe, topology verified |
| consumer streaming, restore keeps the virtual monitor, then Stop while it streams | 6 | safe, topology verified 6/6 |
| same, consumer disconnected before Stop | 2 | safe, topology verified 2/2 |

The last five runs of the matrix (after the review fixes) also printed the consumer's frame count at restore: 30-31
frames, up from 2-4 when the connector appeared, so the consumer was streaming; the no-consumer control had stopped at 4.
The shell logs live in /tmp and are not committed; only these summaries are.

Under gdb the crash matches the real one: the headless backtrace frames at libmutter offsets 0x15dd47, 0x15df44, 0xc3be7,
0x1b84f6, 0xc6c30 and 0x776ab equal the real core's offsets for the same frames (computed from its ProcMaps; the headless
trace stopped at 14 frames, so the real frame at 0x8178c was not compared), with `meta_monitor_manager_rebuild` and
`meta_monitor_manager_apply_monitors_config` in the stack. The faulting instruction exists only from the headless run:
`addl $0x1,0x18(%r14,%rax,1)` with `rax=-0x100` (the negative GObject private offset), consistent with
`inhibit_cursor_overlay_count++` of `meta_stage_view_inhibit_cursor_overlay` on a NULL view (r14 and the fault address were
not recorded; libmutter is stripped, so the NULL view is inferred from the source and this instruction). Root cause:
reproduced, high confidence, for this Mutter build (50.1-0ubuntu2.4). The RemoteDesktop/EIS session, the fullscreen page
and the input grab are not needed for the crash.

Changes: `display_config::restore_physical_outputs_keeping_virtual` and its pure helper `kept_virtual_origin`; exp06
`--integrated-probe` restores with `with_kept_virtual` (also inside the drop-time `RestoreGuard`, which falls back to the
physical-only config once the virtual connector is gone after Stop) and checks the topology ignoring the kept virtual
monitor; `exp13_virtual_restore` and the runner. Review-driven limit: the kept virtual monitor is placed only when every
restored monitor has scale 1.0 and no transform (Mutter's layout width uses `round(width / scale)` in logical layout mode
and the plain mode width in physical mode, and `layout-mode` is not read); otherwise the integrated probe refuses before it
changes anything. The recorded eDP-only backup qualifies; HiDPI scaling would need `layout-mode` handling. Open hazards:
the `exp07_restore` watchdog still applies the physical-only config, so a watchdog that fires while the owner is alive
with a streaming consumer would crash the Shell the same way (needs the same keep-virtual rule before any live integrated
run); the fix is not yet exercised on the real session; an upstream report (missing NULL check in the virtual stream
`monitors-changed-internal` handler, also on upstream main) is not filed. Side effect: coreutils `timeout` re-raises the
child's SIGSEGV, so apport wrote a crash report for `timeout` in /var/crash.

Consequences: the Mutter-instability stop (Doc 00 section 49) stays in force for the real session: no live display,
input or session experiment until the exp07 hazard is closed, a reviewed plan exists and the operator approves one run.
The integrated script is guarded. Not tested: whether the injection, grab and capture parts work (no data). State after
the crash: nothing left running; the operator removed the temporary ACLs on event2-5 afterwards.
