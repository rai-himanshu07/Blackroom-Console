# Physical Input Isolation: Offline Decision

**Status:** synthetic contract only; FEAS-E unproven. No device was opened or grabbed.

The Phase 0–1 source read in `feasibility-research.md` section 6 and the saved
GNOME 50.1 API inventory establish this candidate order:

| Candidate | Current evidence | Privilege, hotplug and emergency question |
|---|---|---|
| Mutter InputCapture | `CreateSession` present; Mutter 50.1 activates on a pointer barrier crossing, not an unconditional grab. Independent keyboard blocking is unverified. | Needs a real-session keyboard/pointer proof, new-device coverage and independent chord observation. |
| RemoteDesktop / InputMapping | `GetDeviceMapping` present; no evidence that mapping inhibits local physical input. | No isolation or emergency contract established. |
| Narrow EVIOCGRAB helper | Possible fallback, not chosen or implemented. | Would need a privileged allow-list, hotplug handling, exclusion of the emergency observer, and a recovery path that works if the helper dies. Never grab `/dev/input/event*` wholesale. |

The offline `FakeGnomeBackend` models a blanket physical-input block: a device
introduced after isolation is blocked immediately, remote input is a separate
channel, and the emergency chord remains observable. Rollback and teardown
restore the physical channel; failure to observe restoration produces
`FAILED_SAFE`. This is a test contract, **not** evidence that any candidate
actually provides those properties on GNOME. The real mechanism remains
undecided until an approved, bounded, independently observed Gate E test.

## Source review update (2026-09-30, read-only)

Mutter 50.1 source (`meta-input-capture-session.c`, `display.c`, `events.c`,
`meta-dbus-session-*.c`, `meta-seat-impl.c`, `meta-barrier-native.c`) changes the
InputCapture row above:

- An activated session consumes key, motion, button and scroll events; with no
  receiver device bound they are dropped. Keyboard isolation therefore exists
  once activated (touch, touchpad gestures and tablet events are not captured).
- The router has no device filter: RemoteDesktop/EIS injected input takes the
  same path and is captured as well. InputCapture cannot block physical input
  while letting remote input through, so it is rejected for this project at
  source level. A live run is not needed to decide this, but no run has
  contradicted or confirmed it.
- Other behaviors: activation needs a sticky-barrier hit (physical or injected
  motion); owner loss closes the session but a dead EIS socket alone does not;
  a monitor change silently disables it; `Super+Shift+Escape` cancels it; VT
  switching is captured while activated.
- Candidate 3 (minimal `EVIOCGRAB` helper) is next. On this host `/dev/input/event*`
  are `root:input 0660`, the user is not in `input`, and `/dev/uinput` is
  root-only, so the helper needs root or group `input`. Release-on-fd-close,
  hotplug and SysRq/power-button behavior are unverified.
FEAS-E stays UNPROVEN. Plan: `docs/plans/plan-20260930-phase7-physical-input-isolation.md`.
