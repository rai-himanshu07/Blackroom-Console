# Capability report — Document 20 §8 constants (Document 00 §35 tiers)

Tiers (Document 00 §35): `SUPPORTED`, `SUPPORTED_WITH_LIMITATIONS`,
`EXPERIMENTAL`, `UNSUPPORTED`, `UNKNOWN`. **`UNKNOWN` never activates**
(Document 00 §35 explicit rule) — presence of a D-Bus method is not the same
as proven support (Document 13 §16), so several rows below are classified more
conservatively than the raw introspection presence alone would suggest.
Evidence pointers cite the Phase 1 experiment or the future phase/experiment
that will produce direct evidence.

| Constant | Tier | Evidence | Notes |
|---|---|---|---|
| `OS_SUPPORTED` | `SUPPORTED` | Exp 0 | Ubuntu 26.04.1 LTS — the exact target release. |
| `GNOME_SUPPORTED` | `SUPPORTED` | Exp 0 | GNOME Shell 50.1 — within the "GNOME 50+" target range. |
| `WAYLAND_SUPPORTED` | `SUPPORTED` | Exp 0, Exp 1 | `XDG_SESSION_TYPE=wayland`; project requires Wayland only (no X11 fallback). |
| `SYSTEMD_SUPPORTED` | `SUPPORTED` | Exp 0 | systemd 259.5-0ubuntu3.4; project requires systemd (`Type=notify`, user units, `login1`). |
| `SESSION_FOUND` | `SUPPORTED` | Exp 1 | Unique session selected by `Type=wayland ∧ Class=user ∧ Seat=seat0 ∧ User=<uid> ∧ Active=true`; negative test (`WAYLAND_DISPLAY` removed) correctly failed closed. |
| `MUTTER_CAPABLE` | `SUPPORTED` | Exp 2 | All required `org.gnome.Mutter.*` interfaces introspected successfully; versioned (`RemoteDesktop.Version=1`, `ScreenCast.Version=4`). |
| `REMOTE_DESKTOP_CAPABLE` | `EXPERIMENTAL` | Exp 2; pending Phase 6 (Exp 8) | `CreateSession` method and `Version`/`SupportedDeviceTypes` properties present and reachable — presence only. `ConnectToEIS` is session-scoped and unexercised. |
| `SCREENCAST_CAPABLE` | `EXPERIMENTAL` | Exp 2; pending Phase 3–4 (Exp 3–4) | `CreateSession`/`Version` present — presence only. `RecordVirtual` is session-scoped and unexercised (hard rule: no session creation in Phase 0–1). |
| `PIPEWIRE_CAPABLE` | `EXPERIMENTAL` | Exp 0; pending Phase 3 (Exp 3) | PipeWire 1.6.2 / WirePlumber 0.5.13 installed and current; no PipeWire node has actually been created by this project yet. |
| `VIRTUAL_DISPLAY_CAPABLE` | `UNKNOWN` | pending Phase 4 (Exp 4–5) | No virtual monitor has been created; `RecordVirtual`'s exact lifecycle was not directly observed (session-scoped, no mutation this phase). |
| `DISPLAY_CONFIG_CAPABLE` | `SUPPORTED` | Exp 2 | `GetCurrentState` is not just present but was **actually called and returned correct live topology** (2 connectors, 1 logical monitor, matching Exp 0's kernel-level facts) — a real functional test, not mere presence. The all-physical-disabled edge case remains open (see `VIRTUAL_DISPLAY_CAPABLE`, `feasibility-research.md` topic 3). |
| `REMOTE_INPUT_CAPABLE` | `UNKNOWN` | pending Phase 6 (Exp 8) | `ConnectToEIS` unexercised; the `reis` crate is not yet a dependency. |
| `PHYSICAL_INPUT_ISOLATION_CAPABLE` | `UNKNOWN` | pending Phase 7 (Exp 9, Exp 38) | This is Gate E, the highest project risk. `InputCapture.CreateSession`/`SupportedCapabilities` are present, and reading the real Mutter 50.1 source (`meta-input-capture-session.c`) confirmed the barrier-crossing mechanism exists — but whether it achieves **complete** physical keyboard+pointer isolation for this product's requirement was not established (`feasibility-research.md` topic 6). Presence must not be read as support here. |
| `SESSION_LOCK_CAPABLE` | `EXPERIMENTAL` | Exp 2; pending Phase 8 (Exp 11) | `org.gnome.ScreenSaver.{Lock,GetActive,SetActive,ActiveChanged}` present and reachable (a real, simple functional primitive) — but interaction with an active `RemoteDesktop` session (survives lock? EIS reaches unlock dialog?) is completely untested. `org.gnome.Shell.ScreenShield` resolves to this same interface, not a separate one (`feasibility-research.md` topic 4). |
| `EMERGENCY_CAPABLE` | `UNKNOWN` | pending Phase 10 | `remote-emergencyd` does not exist yet. The `login1` primitives it will need (`LockSession`, `Session.Leader`, `TerminateSession`) are confirmed present (Exp 1/2), but the system→user unit-control path (assessment §7.5) is unresolved. |
| `GPU_CAPABLE` | `SUPPORTED_WITH_LIMITATIONS` | Exp 0; pending Phase 9/24 (Exp 29) | Hybrid NVIDIA (proprietary, drives the external 4K output) + Intel (`i915`+`xe` both loaded, drives the internal panel) confirmed working for normal desktop use. GPU-specific behaviour for **this project's** use case (virtual-monitor rendering GPU, hardware cursor on the NVIDIA output, cross-GPU buffer scanout cost) is unverified; Mutter's own changelog tracks NVIDIA-specific fixes in this exact release range. |

## Overall status

**`UNKNOWN` → activation blocked**, as expected at this stage (Document 00 §35:
`UNKNOWN` never activates). Three constants are `UNKNOWN`:
`VIRTUAL_DISPLAY_CAPABLE`, `REMOTE_INPUT_CAPABLE`,
`PHYSICAL_INPUT_ISOLATION_CAPABLE` — the last of these is Gate E, the hard
stop if it cannot be resolved safely (assessment §7.1). No constant is
`UNSUPPORTED`; nothing observed this phase contradicts feasibility.

Remote-access activation remains and must remain blocked until Phases 3–10
(Document 10 Experiments 3–10, roadmap Stage II) resolve these three
constants experimentally. This report will be re-issued after each phase that
changes one of these classifications.
