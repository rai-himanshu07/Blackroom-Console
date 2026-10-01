# Input isolation decision (Phase 7, Gate FEAS-E)

**Status:** the daemon (`remote-emergencyd`) is built and was observed in supervised runs (probe runs
2026-09-30/10-01 and one gateway/hostd/daemon run 2026-10-01); Gate FEAS-E is **UNPROVEN** after the
independent review of 2026-10-01 (not STOP). Not installed; privilege unaccepted. Originally written as
design only (2026-09-30).
Plan: `docs/plans/plan-20260930-phase7-physical-input-isolation.md`.
Research: `docs/gnome/input-isolation-research.md`.

## Decision

1. Mutter `InputCapture` is **rejected**: an activated session captures injected
   RemoteDesktop/EIS input as well as physical input (no virtual-device
   exemption in `meta_display_process_captured_input`), so remote control would
   not reach the desktop while physical input is blocked.
2. Candidate 3 is **selected for design**: a minimal helper that takes an
   exclusive `EVIOCGRAB` on allow-listed physical evdev devices. Mutter's
   virtual devices used by RemoteDesktop/EIS are not evdev nodes and are
   unaffected. Nothing here is evidence that it works on this laptop; that needs
   the supervised run in plan step 5.

## Helper contract (the grabber is the emergency executable)

The grabber is the same small binary as `remote-emergencyd` (assessment §7.1 and
roadmap Phase 7 verify text), not a second helper. It already persists a stop
marker and bumps the epoch without hostd cooperation, so the emergency chord can
end remote control even when `remote-hostd` is wedged, and the chord stays
observable by the emergency observer path. FEAS-G (emergency path) remains its
own open gate.

- Commands over a Unix socket, peer-credential checked, only `ISOLATE_INPUT`
  and `RESTORE_INPUT` plus a status read. No event data leaves the helper.
- Device allow-list, decided by capability bits, not by name, per evdev node:
  - grab whole nodes: keyboards (`EV_KEY` with letter keys), mice and touchpads
    (`EV_REL` or `EV_ABS` with buttons), touchscreens (`EV_ABS` multitouch);
  - never grab: nodes with only power or sleep buttons, lid and tablet-mode
    switches, video-bus and WMI hotkey nodes, and any device not on `seat0`.
  - A node that mixes typing keys and vendor hotkeys is grabbed whole, so its
    hotkeys stop working while isolated; only hotkeys on separate nodes keep
    working. The built-in keyboard and touchpad node layout on this laptop is
    unverified until the read-only enumeration in plan step 5.
- Isolation is all-or-nothing: if any allow-listed grab fails, release every
  grab taken, report failure, and the caller stays `LOCAL_LOCKED`.
- Hotplug: a udev `input` monitor classifies new nodes and grabs allow-listed ones
  within one second while isolated; a classification or grab error fails closed
  (release all, tell hostd).
- Release: explicit `RESTORE_INPUT`, helper exit or crash (the kernel drops a
  grab when the fd closes; to be observed, not assumed), the emergency chord, and
  a **grab lease**: isolation lapses unless hostd or the agent renews it within a
  short window, checked by a thread that does not depend on the event loop.
- Hung helper: a systemd watchdog (`WatchdogSec=`, notified only after the event
  and lease loops make progress) kills a stalled process, and the kernel then
  drops the grabs. Doc 06 notes a watchdog proves process health, not safety, so
  the live run also arms an external timer that kills the helper (plan step 5).
- Emergency chord: the binary already reads the grabbed devices, so it detects
  the chord itself (chord choice is open; it must not be typable by accident and
  must be documented). On detection it releases all grabs, persists the stop
  marker and epoch directly, and then tells hostd. This is a deliberate bypass:
  a local person with the physical keyboard can always end remote control.
- Privacy: never log or forward key codes or coordinates; keep only the chord
  state machine window and device counts.

## Privilege model

- Needs root or group `input` on this host (`/dev/input/event*` are
  `root:input 0660`; the user is not in `input`; `/dev/uinput` is root-only).
- Proposed: a dedicated system user with only the `input` group and systemd
  hardening (`NoNewPrivileges`, `ProtectSystem=strict`, `ProtectHome`,
  `DeviceAllow` for `char-input`, a tight `SystemCallFilter`), socket mode 0660
  for the hostd group only. Not installed or enabled by this plan.
- Hotplug monitoring needs a kernel uevent netlink socket, which a private
  network namespace cannot receive, so `PrivateNetwork=yes` is **not** assumed.
  Use `RestrictAddressFamilies=AF_UNIX AF_NETLINK` or an inotify/rescan fallback;
  the exact unit semantics are an open review item, not a reviewed claim.
- Software-created (uinput, `BUS_VIRTUAL`) devices are not grabbed, so a root-run
  tool that injects through uinput (RustDesk is one example) keeps reaching the
  local session while isolated. Root is already outside this threat model, but
  the threat model must say so, and the inventory should flag such nodes.
- A compromised helper is a keylogger for every grabbed keyboard. The helper
  must stay tiny, dependency-light and reviewed; this risk is accepted only if
  the gate needs it.

## Failure behavior

| Event | Local input | Remote control |
|---|---|---|
| Helper crash or kill | restored by the kernel (to verify) | hostd sees EOF, revokes |
| One grab fails at isolate | none taken | not granted |
| Hotplug grab fails | restored by release-all | revoked |
| udev monitor error | restored by release-all | revoked |
| Emergency chord | restored | revoked, marker set |
| Hostd or agent death | helper releases on socket EOF | already revoked |

## Host inventory (read-only, 2026-09-30)

`docs/experiments/evidence/exp09/2026-09-30/`: 23 event nodes, 6 would be grabbed
(built-in keyboard, PS/2 mouse, the touchpad's Mouse and Touchpad nodes, an
external wireless keyboard and its mouse node), 0 differences from udev tags.
Fn-row hotkeys are on separate nodes and stay usable. The built-in keyboard node
carries the kernel `sysrq` handler; because the input core delivers events only to
the grabbing handle, SysRq and kernel VT key handling are expected to stop under a
grab (unobserved), so they must not be counted as recovery paths.

## Observed live on this host (2026-10-01, plan step 5; evidence `docs/experiments/evidence/exp09/`)

All with the operator present, an external kill timer and second-device SSH; counts only, no key codes.

- While a grab is held, physical keyboard, mouse and touchpad input stops reaching the session (the observer page saw nothing while the holder read hundreds of events) and an injected EIS Shift tap still arrives: dongle in-process, dongle in a separate helper process, and the built-in keyboard and touchpad with the dongle grabbed in the same run (no window had keyboard, touchpad and dongle all active).
- Releasing: SIGKILL of the holder releases the grab and input returns within seconds; a frozen holder (SIGSTOP) keeps the grab, so a lease thread cannot help and only an external kill does (24 s with a 1 s accurate timer); killing the holder releases the built-in devices too; an emergency chord detected inside the helper releases cleanly (experiment chord Left Ctrl + Left Shift + Left Alt + Esc, held 2 s, from the built-in keyboard).
- A key or button held when the grab starts is never released to the session, which then auto-repeats it for the whole grab (347 repeats in 10 s). The helper therefore must wait until every key and button is up before grabbing (the probe now does, bounded, with a prompt) and should release stragglers afterwards through the authorized input path (not built).
- systemd timers default to 1 min accuracy, so `systemd-run --on-active=N` can fire up to a minute late; every kill and watchdog timer must set `AccuracySec=1s` (done in the probe and in the exp06 watchdog).
- A chord made of keys that a laptop may not have or that the operator cannot press is a lock-out risk; the chord must be confirmed on each machine's built-in keyboard (a first chord using both Right keys did not release in one run, cause unknown).
- Through the real gateway, hostd and `remote-emergencyd` (2026-10-01, `docs/experiments/evidence/exp09/2026-10-01-gateway-grab/observation.md`): the grab held the built-in keyboard, mouse and touchpad; Revoke released it; with no heartbeat it lapsed about 25 s after the last renew (hostd logged `isolation_lost`); with hostd SIGSTOPped the daemon's 10 s lease released it by itself; the Left-key chord released it and the daemon wrote hostd's stop marker (`emergency-stop`, epoch bumped, gateway FAILED_SAFE) with no key stuck. The chord exit leaves `recovery-pending` and no audit line. Timings include the operator's 1 to 5 s reaction.

## Review result (2026-10-01) and what is built

Independent read-only review: FEAS-E stays **UNPROVEN**. As built: the daemon runs as the same uid as hostd (the
stop marker store and hostd's peer check require it) plus group `input` or ACLs, so hostd's uid can SIGSTOP or
SIGKILL the holder; the socket is 0600; hotplug is a 250 ms directory rescan and a node that cannot be opened or
set non-blocking stays uncovered; nodes with fewer than 20 letter keys, Fn-row (Dell WMI, Intel HID), consumer,
power-button and lid nodes are **not** grabbed. The dedicated uid, 0660 group socket and udev monitor of the
contract above are design only. A frozen holder keeps the grab (observed); the unit's `WatchdogSec` is unobserved.
The daemon links `remote-hostd` (about 88 crates), not the "tiny" helper the contract asks for.

## Still not verified

Whether SysRq and the power button work under a grab (SysRq expected off, see above); hotplug of the dongle; LED, repeat and stuck-modifier state after release when chord keys are still held; synthetic release of keys held at grab start; lock screen and VT interplay; repeated cycles (the roadmap's 50-cycle requirement is not claimed: one bounded run answers one question and more cycles need a named failure); a real privileged helper instead of the experiment binary. FEAS-E needs the independent review of plan step 6 before any promotion.

## Out of scope

Touch and tablet semantics beyond the allow-list, multi-seat, X11, non-Linux,
and any installed service, udev rule or UID. The display privacy gate (Gate C)
is separate and still STOP.
