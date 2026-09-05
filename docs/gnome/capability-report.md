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
| `REMOTE_DESKTOP_CAPABLE` | `EXPERIMENTAL` | Exp 2, Exp 3; pending Phase 6 (Exp 8) | `CreateSession` method and `Version`/`SupportedDeviceTypes` properties present and reachable; Experiment 3 additionally created and introspected a real session — but its only recorded `Session.Stop()` call was made **without** a prior `Start()` and correctly errored ("Session not started"). No successful `Start()`/verified cleanup has been exercised for `RemoteDesktop` specifically (independent-review finding, corrected from an earlier draft of this report that had promoted it to `SUPPORTED`). `ConnectToEIS` remains unexercised (Phase 6). |
| `SCREENCAST_CAPABLE` | `SUPPORTED` | Exp 2–5; pending none (this constant) | Promoted from `EXPERIMENTAL` (Phase 4): `CreateSession`/`RecordMonitor`/`RecordVirtual`/`Start`/`Stop` all exercised for real across Experiments 3–5, including 50 clean create/destroy cycles (Doc 19 §16–17) and verified teardown. |
| `PIPEWIRE_CAPABLE` | `SUPPORTED` | Exp 0, Exp 3, Exp 4 | Promoted from `EXPERIMENTAL` (Phase 4): real PipeWire nodes created and real frames received (Experiments 3–4); 50 create/destroy cycles left 0 leaked `Stream/*/Video` nodes (`pw-dump`-equivalent node-count check). |
| `VIRTUAL_DISPLAY_CAPABLE` | `SUPPORTED_WITH_LIMITATIONS` | Exp 4, Exp 5 | Promoted from `UNKNOWN` (Phase 4): `RecordVirtual` creates a real, `GetCurrentState`-confirmed monitor at 1280×720/1920×1080/2560×1440@60Hz, destroyed cleanly across 50 cycles, and usable as an **additional** active display (Experiment 5) with real frames captured from it. **Not** a clean `SUPPORTED`: whether Mutter permits **zero** physical monitors enabled remains open (Phase 5 Experiment 6, assessment §7.3), and GPU-specific cross-buffer-scanout / cursor behaviour on this hybrid host stay `UNVERIFIED` (`feasibility-research.md` topics 3, 9; Phase 9/24). |
| `DISPLAY_CONFIG_CAPABLE` | `SUPPORTED` | Exp 2 | `GetCurrentState` is not just present but was **actually called and returned correct live topology** (2 connectors, 1 logical monitor, matching Exp 0's kernel-level facts) — a real functional test, not mere presence. The all-physical-disabled edge case remains open (see `VIRTUAL_DISPLAY_CAPABLE` above and Phase 5 Experiment 6). |
| `REMOTE_INPUT_CAPABLE` | `UNKNOWN` | pending Phase 6 (Exp 8) | `ConnectToEIS` unexercised; the `reis` crate is not yet a dependency. |
| `PHYSICAL_INPUT_ISOLATION_CAPABLE` | `UNKNOWN` | pending Phase 7 (Exp 9, Exp 38) | This is Gate E, the highest project risk. `InputCapture.CreateSession`/`SupportedCapabilities` are present, and reading the real Mutter 50.1 source (`meta-input-capture-session.c`) confirmed the barrier-crossing mechanism exists — but whether it achieves **complete** physical keyboard+pointer isolation for this product's requirement was not established (`feasibility-research.md` topic 6). Presence must not be read as support here. |
| `SESSION_LOCK_CAPABLE` | `EXPERIMENTAL` | Exp 2; pending Phase 8 (Exp 11) | `org.gnome.ScreenSaver.{Lock,GetActive,SetActive,ActiveChanged}` present and reachable (a real, simple functional primitive) — but interaction with an active `RemoteDesktop` session (survives lock? EIS reaches unlock dialog?) is completely untested. `org.gnome.Shell.ScreenShield` resolves to this same interface, not a separate one (`feasibility-research.md` topic 4). |
| `EMERGENCY_CAPABLE` | `UNKNOWN` | pending Phase 10 | `remote-emergencyd` does not exist yet. The `login1` primitives it will need (`LockSession`, `Session.Leader`, `TerminateSession`) are confirmed present (Exp 1/2), but the system→user unit-control path (assessment §7.5) is unresolved. |
| `GPU_CAPABLE` | `SUPPORTED_WITH_LIMITATIONS` | Exp 0; pending Phase 9/24 (Exp 29) | Hybrid NVIDIA (proprietary, drives the external 4K output) + Intel (`i915`+`xe` both loaded, drives the internal panel) confirmed working for normal desktop use. GPU-specific behaviour for **this project's** use case (virtual-monitor rendering GPU, hardware cursor on the NVIDIA output, cross-GPU buffer scanout cost) is unverified; Mutter's own changelog tracks NVIDIA-specific fixes in this exact release range. |

## Overall status

**`UNKNOWN` → activation blocked**, as expected at this stage (Document 00 §35:
`UNKNOWN` never activates). Two constants relevant to remote-access activation
are still `UNKNOWN`: `REMOTE_INPUT_CAPABLE`, `PHYSICAL_INPUT_ISOLATION_CAPABLE`
— the latter is Gate E, the hard stop if it cannot be resolved safely
(assessment §7.1). `EMERGENCY_CAPABLE` is also still `UNKNOWN` (Phase 10,
orthogonal to remote-access activation itself). No constant is `UNSUPPORTED`;
nothing observed through Phase 4 contradicts feasibility.

`VIRTUAL_DISPLAY_CAPABLE`, `SCREENCAST_CAPABLE`, and `PIPEWIRE_CAPABLE` were
promoted this phase (Phase 4, Experiments 3–5); `REMOTE_DESKTOP_CAPABLE` was
**not** promoted (independent review found the evidence did not support it —
see the row above) and stays `EXPERIMENTAL`. See `docs/gnome/virtual-display.md`
for the full findings and the two decisions (tier-promotion rule and
`GnomeBackend`-assembly deferral) this phase required.

Remote-access activation remains and must remain blocked until Phases 6–7
(Document 10 Experiments 8–9, roadmap Stage II) resolve the two remaining
constants experimentally. This report will be re-issued after each phase that
changes one of these classifications.
