# Experiment safety procedure

§1–4 are mandatory before any experiment that changes **physical** display or input state
(Document 10 Experiment 6 "Physical Output Isolation" onward, Experiment 9 "Physical Input
Isolation" onward — **not** required for the read-only Phase 0–1 experiments 0–2, nor for
Phase 4's Experiments 3–5, which create only a virtual monitor alongside the existing
physical one(s) and never disable/remove a physical output — see Scope note). §5 is
mandatory for any experiment that creates a `RemoteDesktop`/`ScreenCast` session,
starting with Phase 4. Source: assessment §8 ("Safety plan for running experiments on the
development workstation"); this workstation is both the development machine and the
target, so there is no disposable test box.

## 1. Out-of-band access (prerequisite, verified once)

- `openssh-server` is installed and socket-activated (`ssh.socket`), LAN address
  `192.168.1.50`.
- A second device (tablet/phone, e.g. Termux) must reach the host over SSH using
  **key-based** authentication before Experiment 6/9 run for the first time.
- Status as of 2026-09-05: **Verified.** `ssh.socket` active; tablet ed25519 key installed
  in `~/.ssh/authorized_keys`; key-based login confirmed from the second device; password
  authentication disabled via `/etc/ssh/sshd_config.d/99-blackroom-key-only.conf`
  (`PasswordAuthentication no`) and confirmed rejected (forced password-only attempt from
  the tablet returned `Permission denied (publickey)`). Plan step 5 complete; the
  Experiment 6/9 prerequisite is satisfied.

## 2. Experiment watchdog (armed before every mutating experiment)

- Before applying a display- or input-state change, the experiment binary arms a restore
  timer: `systemd-run --user --on-active=<N>s <restore-command>` (or an in-process
  equivalent), default `N = 45` seconds.
- The watchdog is disarmed only after the operator confirms recovery over the out-of-band
  SSH channel (not over the connection/session being tested).
- If confirmation does not arrive in time, the timer fires and restores the pre-experiment
  state unconditionally.

## 3. VT fallback (display-only experiments)

- `Ctrl+Alt+F3` (switch to a text virtual terminal) is the documented manual fallback for
  **display-only** experiments.
- It is **not** relied upon once physical input isolation is active — if input is
  isolated, the physical keyboard cannot be assumed to reach the VT switch either;
  the watchdog (§2) and out-of-band SSH (§1) are the only trusted recovery paths at
  that point.

## 4. Snapshot before every run

- Save `org.gnome.Mutter.DisplayConfig.GetCurrentState` output to
  `docs/experiments/evidence/<expNN>/<date>/` before making any change.
- Back up relevant `gsettings` display state. **Never edit `monitors.xml` directly.**

## 5. `gnome-remote-desktop` masking

- Stop and mask the `gnome-remote-desktop` user service for the duration of any
  `RemoteDesktop`/`ScreenCast` experiment, to avoid two session owners on the same
  Mutter interfaces:
  `systemctl --user mask --now gnome-remote-desktop.service`
- Re-enable afterwards: `systemctl --user unmask gnome-remote-desktop.service`
  (only start it again if it was running before).
- Status as of 2026-09-05 (Phase 4, first real trigger): the unit was already
  `inactive`/`disabled` on this host; masked before Experiment 3 runs. To be unmasked
  (left `disabled`, matching its prior state) once Experiments 3–5 are complete.

## 6. Bounded per-cycle timeout (repeated-cycle experiments)

- Any experiment that repeats a create/destroy cycle in a loop (e.g. Phase 4's 50-cycle
  virtual-monitor reliability test, Doc 19 §16–17) bounds each cycle to 10 s (reusing the
  `PREPARING_REMOTE`/`TEARING_DOWN` per-step numeric convention, assessment §6.5) and
  aborts the run as `FAIL`/`BLOCKED` rather than hanging indefinitely if a cycle exceeds it.
- This is distinct from §2's watchdog: it bounds automated test loops against a hang, not
  operator recovery from a lost physical display/input.

## Scope note

Phase 0–1 performed **no** GNOME mutation: no `RemoteDesktop`/`ScreenCast` sessions, no
`ApplyMonitorsConfig`, no EIS, no lock calls, no systemd unit changes. Experiments 0–2 only
read environment facts and call read-only D-Bus introspection/`GetCurrentState`. Phase 3
added real (but still read-only) session discovery and capability detection — no mutation
either. **Phase 4 is the first phase that mutates real GNOME/Mutter/PipeWire state**
(`RemoteDesktop.CreateSession`, `ScreenCast.CreateSession`, `RecordVirtual`): Experiments
3–5 create real sessions and a real virtual monitor, so §5 now applies for the first time.
§1–4 remain **not required** for Phase 4 specifically, because Experiments 3–5 never
disable, remove, or isolate a physical output or input device — the virtual monitor is
added as an *additional* active display alongside the existing physical one(s) (Phase 4
plan, Evidence #3), so the failure mode §1–4 exist for (the operator stranded without
physical display/input) does not apply yet. §1–4 become mandatory again starting at
Phase 5 (Experiment 6, physical output isolation) and Phase 7 (Experiment 9, physical
input isolation), which is why §1's out-of-band SSH prerequisite was verified ahead of
time in Phase 0–1 rather than deferred to Phase 5.
