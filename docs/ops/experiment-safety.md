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
- Status as of 2026-09-05 (Phase 5, first implementation): `exp06_isolate_outputs` arms
  `systemd-run --user --unit=blackroom-exp06-watchdog-<unix-timestamp>
  --on-active=<N>s -- <path-to-exp07_restore> --backup <path-to-backup.json>` before its
  first real `ApplyMonitorsConfig` disable (`N` scales with `--cycles`, floor 45s) and
  disarms it (`systemctl --user stop <unit>.timer`) only once the run's own restore is
  verified.
- **Incident, same day, first live run:** the watchdog's restore command was passed a
  *relative* `--backup` path, which failed once `systemd-run`'s transient unit ran in a
  different working directory (`journalctl` showed `Error: No such file or directory`,
  unit status `FAILURE`) — the watchdog did **not** fire successfully. Separately, the
  binary's own `--pause-after-isolate` "press Enter to restore" path had a code bug and
  never actually restored either. The operator had to restore manually over SSH (§1) —
  exactly what §1 exists for, but neither inner safety net did its job. Both bugs fixed
  same session (absolute path via `std::path::absolute`; the pause branch now actually
  calls and verifies restore). **Verified live same day:** a fresh isolate run was left
  deliberately untouched past the 45s window; the watchdog fired completely unattended,
  invoked `exp07_restore` with the correct absolute path, and restored correctly
  (`journalctl` showed `Result: PASS`; independently confirmed via a fresh
  `GetCurrentState` read and a `gnome-shell` health check). The watchdog mechanism is now
  considered trustworthy for `eDP-1`/`HDMI-1` zero-physical isolation on this host.

## 3. VT fallback (display-only experiments)

- `Ctrl+Alt+F3` (switch to a text virtual terminal) is the documented manual fallback for
  **display-only** experiments.
- It is **not** relied upon once physical input isolation is active — if input is
  isolated, the physical keyboard cannot be assumed to reach the VT switch either;
  the watchdog (§2) and out-of-band SSH (§1) are the only trusted recovery paths at
  that point.
- Status as of 2026-09-05 (Phase 5): **resolved, empirically, for `eDP-1`** via the
  `org.gnome.Mutter.DisplayConfig.PowerSaveMode` probe (`readwrite i`, confirmed present in
  `docs/gnome/api-inventory.md`; DPMS-standard value `3` = OFF, confirmed live — reading
  the property back before the probe showed `0` = ON, matching the screen being visibly on).
  Setting `PowerSaveMode` to `3` visibly blanked **both** `eDP-1` and `HDMI-1` (the property
  is global, not per-output); moving the mouse did **not** wake either panel (normal input
  is not a reliable undo for this state). `Ctrl+Alt+F3` **did** show a visible, readable
  `tty3` login console — the operator confirmed this directly — and `Ctrl+Alt+F2` returned
  cleanly to the graphical session with no crash (`gnome-shell` PID unchanged throughout).
  **Scope caveat:** this tested `PowerSaveMode`/DPMS-off specifically, not yet a real
  `ApplyMonitorsConfig` zero-physical-monitor disable (Experiment 6) — VT-switching acts
  below Mutter's compositor at the kernel/DRM level, so this finding is expected to
  generalize to that case too, but that is reasoned, not independently proven for the
  exact mechanism Experiment 6 uses. Re-confirm during the first real Experiment 6 run
  rather than assuming. Out-of-band SSH (§1) remains the primary trusted path regardless;
  `Ctrl+Alt+F3` is now a reasonably-confirmed secondary path for `eDP-1` specifically, not
  merely an unverified assumption.
- **Update, first real Experiment 6 run, same day:** `Ctrl+Alt+F3` again showed a visible
  `tty3` console under the real `ApplyMonitorsConfig` zero-physical mechanism — that part
  of the generalization held. **But `Ctrl+Alt+F2` did NOT restore the GUI this time**
  (unlike the `PowerSaveMode` case) — switching back to Mutter's VT does not help when
  Mutter's own compositor config genuinely has zero physical monitors active; only an
  actual `ApplyMonitorsConfig` restore fixes that, which is what actually happened, run
  manually by the operator over SSH after two automated restore paths turned out to be
  buggy (see §2). Net effect for `eDP-1`: `Ctrl+Alt+F3` reliably gets a console for
  running recovery commands (e.g. `exp07_restore` directly from `tty3`), but is **not** a
  path back to the GUI on its own — SSH (§1) or an explicit restore command remain the
  only ways to actually get the desktop back.

## 4. Snapshot before every run

- Save `org.gnome.Mutter.DisplayConfig.GetCurrentState` output to
  `docs/experiments/evidence/<expNN>/<date>/` before making any change
  (later same-day runs use `<date>-2`, `<date>-3`, etc.; never overwrite a prior run).
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

## 7. Supervised crash reassessment (Phase 5 stop still applies)

This is a recovery procedure, not authorization to run an experiment. Before any new
GNOME mutation, the operator must be at the workstation, establish a fresh SSH login
from a second device, and approve the exact command. Keep that SSH shell open. Record
the start time and GNOME Shell PID; check `ssh.socket` is active and no earlier exp06
watchdog timer is pending. `gnome-remote-desktop.service` must be masked per §5 for
ScreenCast experiments. Do not run a deliberate SIGKILL or physical-output isolation
as an initial crash diagnostic. A single `exp04_virtual_monitor --skip-cycles` run
leaves physical outputs active but still creates/stops three virtual-monitor sessions
and can crash GNOME; it requires separate approval. A passing run would not clear the
Phase 5 stop or prove the hybrid-GPU crash path safe.

An optional `exp04_virtual_monitor --probe-owner-loss` mode had one separately
approved live run on 2026-09-26; **no repeat or exp06 run is approved**. It retains a Stop-on-error guard until it
has persisted a PARTIAL pre-close report for one confirmed 1280x720 virtual
monitor, then closes that client's D-Bus connection without Stop. This tests
owner disappearance with physical outputs active, not display isolation;
it may still crash GNOME, and there is no exp07 backup for this mode. The
independent Shell PID, journal, and display-state checks below remain required.
That single run removed Meta-0 without a Shell restart (PID 34735 throughout),
and the operator observed a responsive desktop. It does not establish safety
with zero physical displays or on a repeated run.

If a later, separately approved exp06 run leaves the display blank while the *original*
GNOME session is still alive:

1. From the second-device SSH shell, check the named watchdog timer from exp06's output
   with `systemctl --user list-timers --all 'blackroom-exp06-watchdog-*'`. Allow the
   45-second timer to fire; check its service result in `journalctl --user -u
   '<printed-watchdog-unit>.service' -b --no-pager`. `Ctrl+Alt+F3` can show a text
   console, but `Ctrl+Alt+F2` alone cannot restore an isolated desktop.
2. Only if the watchdog did not restore the *still-running original session*, use
   the exact absolute `backup.json` path exp06 printed. From SSH as the same user:

   ```sh
   repo='/media/user/Playground/Playground_Sys/Blackroom Console'
   backup='/absolute/path/printed/by/exp06/backup.json'
   cd "$repo"
   test -f "$backup" && XDG_RUNTIME_DIR="/run/user/$(id -u)" \
     DBUS_SESSION_BUS_ADDRESS="unix:path=/run/user/$(id -u)/bus" \
     "$repo/target/debug/exp07_restore" --backup "$backup"
   ```

   Confirm physical output and desktop visibility independently. If exp06 is still
   paused, send Enter to its original terminal only after restoration so it can stop
   its ScreenCast session; never assume exp07 removed that session for it.

For a routine `exp06 --pause-after-isolate` observation, do not send Enter or
terminate the process while isolated. Let the armed watchdog fire, confirm from
SSH that it restored the original topology and the physical desktop is visible,
then send Enter for the original process's ScreenCast cleanup. The exp06
graceful path currently disarms the timer after its own topology check, before
an independently verified SSH/visual confirmation; waiting for the watchdog
avoids relying on that weaker path for this supervised observation. No new
isolate run is authorized solely by this procedure.

The deliberate **HDMI-only SIGKILL** scenario is a different, higher-risk
test and requires its own explicit approval; earlier GNOME Shell SIGSEGV under
virtual-monitor removal remains unexplained. Before launch, confirm the
operator is physically watching HDMI and can remain through restoration; if
they step away, do not start (or, if already running, let the watchdog
restore without a kill). Save unsaved work first and keep the second-device
SSH connection open. Record exp06's printed PID, exact
absolute backup path, timer name and Shell PID. Confirm HDMI is cleanly blank
using an independent observer/channel if attempting a manual kill; **never
require a VS Code chat reply while the desktop is blank**. Do not switch VT
or press F2/F3 unless recovery is needed. The pause-only `--watchdog-seconds` option
accepts 45-120 seconds; the default stays 45. A longer blank interval
increases recovery risk and does not authorize a new run. Confirm HDMI is still isolated
and verify the *printed timer* is still active and the original GNOME session
still isolated before sending exactly one `kill -9 <printed-PID>` from a
separate shell. If the timer has already fired or the GUI has returned,
**abort the kill**; confirm restore and clean up the still-paused owner with
Enter, with no automatic repeat. If the visual state is uncertain, also abort
the kill. Do not keep extending the watchdog window to force this experiment
on the daily-driver host. Never kill by a broad process-name match
or press Enter while isolated. Immediately make a
read-only `GetCurrentState` attempt to observe Mutter's automatic behavior,
then allow the already-armed watchdog to fire. If the original Shell and
session remain alive but the watchdog fails, follow the explicit-backup
exp07 procedure above. If Shell crashes or a new GDM login appears, do **not**
apply the old backup to that new session; capture journal/coredump diagnostics
from SSH, then use the normal login/local recovery. Any crash, unknown state,
or failed watchdog ends the test without a repeat or Gate FEAS-C promotion.

If the operator can watch but cannot communicate until the GUI returns, the
manual-kill procedure above is not executable as written. Do not extend the
timer or start another run hoping for a chat reply. An opt-in
`exp06 --pause-after-isolate --watchdog-seconds 45
--auto-kill-after-isolate` diagnostic was run once with separate approval;
**no repeat is approved**. It allows one self-SIGKILL
only from an HDMI-only starting layout, with the named timer still active,
the same `org.gnome.Mutter.ScreenCast` D-Bus owner PID, DPMS OFF,
virtual-only logical topology, HDMI still in
raw inventory, and less than 10 seconds since watchdog arming. It writes
PARTIAL pre-kill evidence and rechecks immediately before signaling; if any
check fails, the normal restore guard remains armed. This machine check
cannot prove the panel was visually blank and cannot prevent a compositor
crash. The executable must not be run until a new safety review and exact
operator approval; no chat reply is requested during the blank interval.
The observer must report actual physical-screen observations **after**
recovery; if the compositor crashes, follow the recovery branch below.
On the approved run, HDMI stayed blank until exp07 unblanked it. The
disabled built-in panel flickered too briefly for the operator to determine
whether desktop content was exposed. The operator accepts that flicker as
privacy-acceptable for the run; it does not prove no content was visible.
Mutter restored HDMI logically after
the owner died but left DPMS OFF until the watchdog fired; Shell survived.
This does not prove Gate FEAS-C or justify a repeat without new review.

The 2026-09-26 90-second attempt had no continuous visual observer (the
operator later clarified they were away during the isolation window). Its
watchdog restore PASS is useful, but it is not evidence of physical-screen
privacy or abnormal-termination recovery. A new run requires fresh approval
and a present observer from start through restoration.

If GNOME Shell crashes or logs out, the original D-Bus session may be gone. **Do not
apply an old backup to a new login** or assume the watchdog can resurrect the Shell.
Use the second-device SSH shell or visible `tty3` to collect the exact incident time,
`journalctl --user -b` around it, the exp06 watchdog unit result (if any), and
`coredumpctl info gnome-shell` if a dump exists; keep raw logs local until checked
for private data. Log back in normally if GDM presents a working login screen.
If the GUI does not recover, stop the experiment and use local administrative
recovery; do not retry display mutation against an unhealthy compositor.

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

Phase 5 update (2026-09-26): real isolation runs have since occurred; §3's VT question
was resolved and the watchdog was observed restoring unattended. However, the latest
session reported a GNOME Shell crash and fresh login near virtual-monitor removal;
whether a deliberate exp06 kill preceded it is unknown (the operator no longer recalls).
Doc 00 §49 / Doc 10 §49's Mutter-instability stop condition now applies: do not run
another physical-output mutation or the crash-recovery scenario on this host without
a new safety review and explicit operator approval. Gate FEAS-C is not proven.
