# Experiment safety procedure

Mandatory before any experiment that changes display or input state (Document 10
Experiment 6 "Physical Output Isolation" onward, Experiment 9 "Physical Input Isolation"
onward — **not** required for the read-only Phase 1 experiments 0–2). Source: assessment
§8 ("Safety plan for running experiments on the development workstation"); this workstation
is both the development machine and the target, so there is no disposable test box.

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

## Scope note

Phase 0–1 (this plan) performs **no** GNOME mutation: no `RemoteDesktop`/`ScreenCast`
sessions, no `ApplyMonitorsConfig`, no EIS, no lock calls, no systemd unit changes.
Experiments 0–2 only read environment facts and call read-only D-Bus introspection/
`GetCurrentState`. This procedure exists now because Document 00 §51/§68 require it to be
documented before Phase 4+ needs it, and so the out-of-band SSH prerequisite (§1) can be
tracked as a known blocker ahead of time.
