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