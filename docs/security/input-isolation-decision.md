# Input isolation decision (Phase 7, Gate FEAS-E)

**Status:** design only, not approved, nothing implemented or run (2026-09-30).
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

## Not verified (plan step 5 must observe)

Release on fd close and on SIGKILL; whether SysRq and the power button still
work under a grab; node layout of the built-in keyboard and touchpad and whether
a grab of them is sufficient; that the observer page sees no physical input while
remote input still works; events queued before the grab starts (leakage);
key-repeat, LED and stuck-modifier state after release; a stalled helper and the
watchdog and lease behavior; a USB combo device plus the internal devices; hotplug
latency under load. The roadmap's 50-cycle requirement is not claimed: one bounded
run answers one question and more cycles need a named failure.

## Out of scope

Touch and tablet semantics beyond the allow-list, multi-seat, X11, non-Linux,
and any installed service, udev rule or UID. The display privacy gate (Gate C)
is separate and still STOP.
