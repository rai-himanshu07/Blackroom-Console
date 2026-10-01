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
| `REMOTE_DESKTOP_CAPABLE` | `SUPPORTED_WITH_LIMITATIONS` | Exp 2, Exp 3, Exp 8 (runs 1-4, 2026-09-30 to 2026-10-01), Exp 9 sessions | Promoted from `EXPERIMENTAL`: Experiment 8 ran input-only `CreateSession`, `Start`, `ConnectToEIS`, the Sender handshake and `Stop` in each of four runs on the existing session (plus the Exp 9 grab runs), saw `DeviceRemoved`/`SeatRemoved`/`Disconnected` and a closed socket after `Stop`, got "Object does not exist" from the old session path, and the Shell PID never changed. **Limits:** input-only session on the single built-in display only; owner-loss (process killed) teardown is unobserved; touch, clipboard and other layouts are untested. |
| `SCREENCAST_CAPABLE` | `SUPPORTED` | Exp 2–5; pending none (this constant) | Promoted from `EXPERIMENTAL` (Phase 4): `CreateSession`/`RecordMonitor`/`RecordVirtual`/`Start`/`Stop` all exercised for real across Experiments 3–5, including 50 clean create/destroy cycles (Doc 19 §16–17) and verified teardown. |
| `PIPEWIRE_CAPABLE` | `SUPPORTED` | Exp 0, Exp 3, Exp 4 | Promoted from `EXPERIMENTAL` (Phase 4): real PipeWire nodes created and real frames received (Experiments 3–4); 50 create/destroy cycles left 0 leaked `Stream/*/Video` nodes (`pw-dump`-equivalent node-count check). |
| `VIRTUAL_DISPLAY_CAPABLE` | `SUPPORTED_WITH_LIMITATIONS` | Exp 4, Exp 5 | Promoted from `UNKNOWN` (Phase 4): `RecordVirtual` creates a real, `GetCurrentState`-confirmed monitor at 1280×720/1920×1080/2560×1440@60Hz, destroyed cleanly across 50 cycles, and usable as an **additional** active display (Experiment 5) with real frames captured from it. **Not** a clean `SUPPORTED`: Mutter accepts **zero** physical monitors enabled on the single built-in eDP layout (Phase 5, `display-isolation.md`, FEAS-C PASS for that layout only; connected HDMI is unsupported), and GPU-specific cross-buffer-scanout / cursor behaviour on this hybrid host stay `UNVERIFIED` (`feasibility-research.md` topics 3, 9; Phase 9/24). |
| `DISPLAY_CONFIG_CAPABLE` | `SUPPORTED_WITH_LIMITATIONS` | Exp 2, Exp 6, Exp 7 (2026-10-01) | `GetCurrentState` is not just present but was **actually called and returned correct live topology** (2 connectors, 1 logical monitor, matching Exp 0's kernel-level facts) — a real functional test, not mere presence. The all-physical-disabled edge case is resolved for the single built-in eDP layout (zero physical monitors accepted, restore verified by hash; `display-isolation.md`); connected HDMI, hotplug and abnormal termination are not claimed. |
| `REMOTE_INPUT_CAPABLE` | `SUPPORTED_WITH_LIMITATIONS` | Exp 8 (runs 1-4; run 4 PASS 2026-10-01); `docs/experiments/evidence/exp08/`; Exp 9 injected taps | Promoted from `EXPERIMENTAL` after run 4: every stage matched the page's own tally (all events trusted): Shift tap, Shift + Right Ctrl chord (modifier held), `a` and Left with no modifier, a pointer path of +40, -40, -10 from a start position the page captured (381, 421, 381, 371), one left click, a scroll (a 15.0 request gave `deltaY` 207, not investigated), a revoked send refused (`LeaseRevoked`) with nothing delivered, and a post-`Stop` send refused with nothing delivered; Shell PID unchanged. Runs 1 (aborted by `F13` opening Settings) and 2 (failed only the pointer check, since fixed) agree with it. **Limits:** input-only session on the built-in display, not routed to the virtual monitor (FEAS-C is stopped); a fake in-process lease, not hostd to agent to EIS; owner-loss teardown unobserved (Gate F); relative pointer only, no absolute mapping or cursor shape (Exp 28); the only target is a browser observer page (browser identity not recorded); no right/middle button, drag, horizontal scroll or special keys beyond Left; the revoke refusal is the local authorization check, not Mutter (Mutter closes the EIS channel on `Stop`, observed). `F13` is **not** inert on this host (`XF86Tools` opens GNOME Settings). |
| `PHYSICAL_INPUT_ISOLATION_CAPABLE` | `SUPPORTED_WITH_LIMITATIONS` | Exp 9 probe runs `exp09/2026-09-30-2..9`; gateway run `exp09/2026-10-01-gateway-grab`; FEAS-E run A `exp08/2026-10-01-3`; run B `exp09/2026-10-01` | Gate E, the highest project risk: **PASS-WITH-LIMITS for the built-in keyboard, PS/2 mouse and touchpad (event2-5) only** (second independent review 2026-10-01; first review had stayed UNPROVEN). `InputCapture` is rejected at source level (it captures injected input too). The real `remote-emergencyd`'s exclusive `EVIOCGRAB` hid physical input from an observer page while injected Shift, `a`, Left, pointer, click and scroll arrived (run A: 304 daemon reads, 290 inside the judged window, page saw only the injected input); a SIGSTOPped daemon under a `WatchdogSec=10` + `WatchdogSignal=SIGKILL` transient unit was killed and its connection closed after 9.883 s (run B, journal attached); the gateway/hostd/daemon path released on revoke, heartbeat loss (about 25 s), a frozen hostd and the Left-key chord (marker written). **Limits:** same uid as hostd plus `input`/ACLs (operator-accepted; a dedicated uid is a product item); nothing installed, the shipped unit file never loaded; the dongle, any USB keyboard and hotplug are **not** covered (an unopenable hotplugged node stays silently uncovered); run A keyboard activity is inferred from counts and the gateway chain and page observer were never one run; no Start with a key held; SysRq and power button under a grab unobserved; the chord exit writes no audit line; one run per question, no cycles; pointer return in run B not recorded by the operator. Fn-row, consumer, power and lid nodes are not grabbed. Details in `docs/security/input-isolation-decision.md`. |
| `SESSION_LOCK_CAPABLE` | `EXPERIMENTAL` | Exp 2; exp11 (2026-09-28 lock-only, 2026-10-01-5); exp12 (2026-10-01) | `org.gnome.ScreenSaver.{Lock,GetActive,SetActive,ActiveChanged}` present and reachable, and now observed live: lock and logind unlock work in under a second, but a lock ends the RemoteDesktop/EIS connection and new sessions are refused while locked (gnome-shell 50.1 inhibits remote access in the locked session mode), so EIS cannot reach the unlock dialog; the lock signals still have no provenance (the ScreenSaver owner is not in the login1 session). Stays `EXPERIMENTAL` until provenance is bound and the operator accepts the replacement design (`docs/gnome/lock-semantics.md`). `org.gnome.Shell.ScreenShield` resolves to this same interface, not a separate one (`feasibility-research.md` topic 4). |
| `EMERGENCY_CAPABLE` | `UNKNOWN` | pending Phase 10 | `remote-emergencyd` does not exist yet. The `login1` primitives it will need (`LockSession`, `Session.Leader`, `TerminateSession`) are confirmed present (Exp 1/2), but the system→user unit-control path (assessment §7.5) is unresolved. |
| `GPU_CAPABLE` | `SUPPORTED_WITH_LIMITATIONS` | Exp 0; pending Phase 9/24 (Exp 29) | Hybrid NVIDIA (proprietary, drives the external 4K output) + Intel (`i915`+`xe` both loaded, drives the internal panel) confirmed working for normal desktop use. GPU-specific behaviour for **this project's** use case (virtual-monitor rendering GPU, hardware cursor on the NVIDIA output, cross-GPU buffer scanout cost) is unverified; Mutter's own changelog tracks NVIDIA-specific fixes in this exact release range. |

## Overall status

**Activation blocked.** `PHYSICAL_INPUT_ISOLATION_CAPABLE` is `SUPPORTED_WITH_LIMITATIONS`
(Gate E PASS-WITH-LIMITS for the built-in layout event2-5 only, after FEAS-E runs A and B and a second
independent review on 2026-10-01; limits in its row); like every constant below `SUPPORTED`
it never activates anything (Document 00 §35), and activation stays disabled until Phase 9 FEAS-F/H
and the Gate F owner-kill observation. `REMOTE_INPUT_CAPABLE` is
`SUPPORTED_WITH_LIMITATIONS` (Experiment 8 run 4, limits in its row) and, like
every constant, does not activate anything by itself. `EMERGENCY_CAPABLE` is
still `UNKNOWN` (Phase 10, orthogonal to remote-access activation itself). No
constant is `UNSUPPORTED`; nothing observed contradicts feasibility.

`VIRTUAL_DISPLAY_CAPABLE`, `SCREENCAST_CAPABLE`, and `PIPEWIRE_CAPABLE` were
promoted in Phase 4 (Experiments 3–5). `REMOTE_DESKTOP_CAPABLE` was not
promoted then (independent review found the evidence insufficient) and was
promoted to `SUPPORTED_WITH_LIMITATIONS` after Experiment 8 supplied a real
`Start`/`Stop` lifecycle. See `docs/gnome/virtual-display.md`
for the full findings and the two decisions (tier-promotion rule and
`GnomeBackend`-assembly deferral) this phase required.

Remote-access activation remains and must remain blocked until Phase 7
(Document 10 Experiment 9, roadmap Stage II) resolves `PHYSICAL_INPUT_ISOLATION_CAPABLE`
and the remote-input gaps above are closed. This report will be re-issued after each phase that
changes one of these classifications.
