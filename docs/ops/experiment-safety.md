# Experiment safety procedure

## Standing approval (2026-10-02, owner decision; current for the validated flow only)

The integrated live flow (`docs/ops/live-integrated-run.sh`, later the product Start/Stop) is validated on this host:
eDP-only, scale 1.0, built-in input nodes, restore watchdogs with `--keep-live-virtual`. For that flow:

This section supersedes older run-by-run approval and no-repeat restrictions only for the declared flow. It does not
authorize HDMI, hotplug, another layout or an untested recovery change. The dated incidents and spent approvals below
remain historical evidence, not standing permission. Product recovery steps are in [emergency-recovery.md](emergency-recovery.md).

- The operator's instruction to run it, or to run the product, is the approval: no per-run approval question, no
  independent review, no repeat prohibition.
- The kit stays because it is cheap and this is the only machine: work saved, tablet SSH connected, AC on, kill timer
  and restore watchdog armed by the tool, input ACLs set by the operator.
- A NEW risk class gets one chat sentence first (the risk and the recovery; the operator says go): a different layout
  or scale, HDMI or hotplug, self-kill tests, lock or disconnect while remote input is live, a change to the restore
  config path, or the first live use of new product code.
- Evidence is the tool's generated evidence directory plus one line in the active plan's log: what ran, pass or fail,
  what it did not cover. No observation documents and no doc rewrites per run; roadmap and handoff are updated at
  milestones only.
- A Shell crash or a panel that stays blank is still a stop: report, recover, fix, then continue. It does not freeze the
  project.
- Never remove the live virtual monitor from an applied config while its PipeWire consumer streams. Do not apply display
  config immediately after owner death; preserve the validated restore ordering and original Shell/session identity.

The recovery safeguards in §1–4 remain mandatory before an experiment that changes **physical** display or input state;
historical command-specific approvals do not override the current standing-approval scope.
(Document 10 Experiment 6 "Physical Output Isolation" onward, Experiment 9 "Physical Input
Isolation" onward — **not** required for the read-only Phase 0–1 experiments 0–2, nor for
Phase 4's Experiments 3–5, which create only a virtual monitor alongside the existing
physical one(s) and never disable/remove a physical output — see Scope note). §5 is
mandatory for any experiment that creates a `RemoteDesktop`/`ScreenCast` session,
starting with Phase 4. Source: assessment §8 ("Safety plan for running experiments on the
development workstation"); this workstation is both the development machine and the
target, so there is no disposable test box.

## Experiment selection

Implement independent app components with synthetic/local checks first. Run a
live experiment only when it resolves a named unknown essential to the
supported core workflow and cannot be answered offline. One bounded run on
the supported layout should answer one question; expand only after a concrete
failure or a new support claim. Historical 50-cycle targets, every GPU/mode
matrix row, repeated observations, and optional cursor/hotplug experiments
are not automatic prerequisites to writing the app. They remain unproven when
deferred. No live isolation, input injection, or activation is authorized by
this selection rule; the prerequisites below and §7 still apply to each run.

## 1. Out-of-band access (prerequisite, verified once)

- On the tested host, `openssh-server` is installed and socket-activated (`ssh.socket`). Use the host's current address;
  no machine-specific network address is published here. Confirm a fresh second-device SSH connection before a live run.
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
  timer: `systemd-run --user --timer-property=AccuracySec=1s --working-directory=<absolute-run-directory>
  --on-active=<N>s -- <absolute-restore-command>` (or the validated equivalent), with an absolute backup path.
  The historical exp06 default is 45 seconds; the product uses a rolling 60-second restore watchdog.
- The watchdog is disarmed only after the operator confirms recovery over the out-of-band
  SSH channel (not over the connection/session being tested).
- If confirmation does not arrive in time, the timer fires and restores the pre-experiment
  state only against the original Shell/session identity. If that identity changed, restoration must refuse rather than
  applying an old backup to a replacement session; recover locally and record the failure.
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
  proven to have worked for that recorded run. This did not establish every topology or abnormal-termination case;
  the later connected-HDMI failures below limit the accepted product layout to the built-in panel.

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

## 7. Supervised diagnostics (product stop still applies)

**Historical procedure (September 2026).** Its command-specific approvals and original product-stop statements are
preserved below. For today's supported-flow work, use the standing-approval section at the top; unsupported layouts and
new risk classes still need explicit approval and recovery controls. No historical PASS authorizes a new support claim.

This is a recovery procedure, not blanket authorization to run an experiment. Before
any new GNOME mutation, the operator must be at the workstation, establish a fresh SSH
login from a second device, and approve the exact command and run conditions. Keep that
SSH shell open. Record the start time and GNOME Shell PID; check `ssh.socket` is active and no earlier exp06
watchdog timer is pending. `gnome-remote-desktop.service` must be masked per §5 for
ScreenCast experiments. Do not run a deliberate SIGKILL or physical-output isolation
as an initial crash diagnostic. A single `exp04_virtual_monitor --skip-cycles` run
leaves physical outputs active but still creates/stops three virtual-monitor sessions
and can crash GNOME; it requires separate approval. A passing run would not clear the
Phase 5 stop or prove the hybrid-GPU crash path safe.

An optional `exp04_virtual_monitor --probe-owner-loss` mode had one separately
approved live run on 2026-09-26; **that approval does not authorize a repeat or exp06 run**. It retains a Stop-on-error guard until it
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
  repo='/absolute/path/to/Blackroom Console'
   backup='/absolute/path/printed/by/exp06/backup.json'
   cd "$repo"
   test -f "$backup" && XDG_RUNTIME_DIR="/run/user/$(id -u)" \
     DBUS_SESSION_BUS_ADDRESS="unix:path=/run/user/$(id -u)/bus" \
     "$repo/target/debug/exp07_restore" --backup "$backup"
   ```

   Confirm physical output and desktop visibility independently. For an `exp06 --integrated-probe` run with its owner
   still alive, add `--keep-live-virtual` to that command (Mutter crashes otherwise; see docs/gnome/display-isolation.md).
   If exp06 is still
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

### One-run eDP-1 diagnostic exception (operator-authorized 2026-09-27)

The operator accepts the risk of a GNOME Shell crash on the daily-driver laptop
and explicitly overrides the Phase 5 live-test pause for **one supervised,
no-kill eDP-1-only diagnostic** despite the unexplained earlier SIGSEGV.
The prior independent safety review recommended no-go on this host; the
operator acknowledges that warning and authorizes this bounded risk to obtain
new crash/restore evidence. This does not clear the Doc 00/10 §49 product
stop, prove Mutter stable, or establish Gate FEAS-C. The operator also accepts the
prior brief built-in-panel flicker, even though whether content appeared is
unknown; no further capture of that flicker is requested for this diagnostic.
This exception does **not** authorize product activation, other matrix rows, SIGKILL, self-kill,
repeated cycles, or remote-input testing. A force restart
could lose unsaved work and cannot restore the original GNOME session. If the
operator cannot accept that outcome or cannot observe the entire run, do not
start. The authorized diagnostic command, from the repository root after
building both experiment binaries fresh and satisfying every check below, is:

```sh
./target/debug/exp06_isolate_outputs --pause-after-isolate --watchdog-seconds 45
```

The operator's acceptance of the flicker is a scoped risk decision, not an
observation of zero desktop exposure or a Gate FEAS-C PASS. Persist a unique,
non-overwriting preflight record of the original GetCurrentState, Shell PID and
session, `PowerSaveMode`, pending watchdog timers and service state, then persist
the corresponding postflight state, timer result and bounded journal around
the test. Keep raw logs and images local until reviewed for private content.
Without durable evidence, the diagnostic cannot resolve the crash/restore
question and must not be attempted merely to repeat the earlier observation.

Before launch, save all work; enable and verify the built-in panel before
disconnecting HDMI; confirm `eDP-1` is the
sole active physical output by a fresh read-only GetCurrentState; keep a newly
verified key-based SSH login open from a second device; confirm the original
Shell PID, session, `ssh.socket`, and no pending exp06 timer; mask
`gnome-remote-desktop.service` per §5 and preserve its prior state for restore.
The operator must be present and independently watch the built-in screen
through restoration to confirm recovery; no new privacy capture is required
for this diagnostic. Do not
start if any check fails. Confirm the exact command and original eDP-only
layout with the operator immediately before launch; no generic approval
permits changing flags or retrying the run.

During the run, do not press Enter, switch VTs, unplug another output, or kill
the process while isolated. Record the printed absolute backup path, PID and
watchdog unit; allow the 45-second watchdog to restore without intervention.
Check the timer result from the second-device SSH channel and independently
confirm the original topology, `PowerSaveMode=0`, Shell PID/session and visible
desktop before sending Enter for the still-paused owner's ScreenCast cleanup.
If the original session survives but the watchdog fails, use the exact backup
with exp07 as described above; never guess a backup path. If Shell crashes or
a new login appears, do **not** apply the old backup: exp07 now refuses a
backup without the original login session ID and Shell PID, including when
called by the watchdog. A failed watchdog service on identity mismatch is the
expected fail-closed result, not evidence of restored topology. Exp07 exits
before producing its normal report on such a refusal; retain the named unit's
service journal and timer result locally as the durable refusal evidence, then
recover by normal login/restart. Do not retry on
crash, unexpected topology, or restore failure. A clean
run is one diagnostic observation, not Gate FEAS-C PASS; resume the stop and
review its evidence independently before any further mutating test.

**2026-09-27 outcome: stop, no repeat.** The one approved eDP-only run reached
virtual-only topology. The 45-second watchdog exp07 restored eDP-only and the
operator saw a responsive desktop, but after exp06 stopped the ScreenCast
session HDMI-1 became active again. A second identity-guarded exp07 restored
the original eDP-only topology. Shell PID 34735 survived; final restoration
was not stable through owner cleanup. See the saved exp06 observation. Do not
run another isolation row or treat the earlier watchdog PASS as Gate C proof.

**New hardware condition, separately approved diagnostic (completed once):**
the operator physically disconnected HDMI-1, and a fresh read-only
GetCurrentState confirms raw and active logical connectors contain only
eDP-1. This removes the prior run's disabled-but-connected HDMI condition,
not the restoration risk or product stop. Before any one new run with the
same no-kill, 45-second pause command, require a fresh unique baseline, saved
work, live second-device SSH, visible eDP, original Shell/session identity,
masked remote desktop, zero pending timers, fresh checked binaries and exact
operator approval. Exp06 must persist its post-Stop raw/logical outputs,
power, Shell/session, timer/service state and probe errors, and report FAIL
on final mismatch. The first timer will have fired before Enter. Exp06 must
check original Shell/session identity, arm a **second 45-second watchdog**
against the same exact backup before local restore or ScreenCast Stop, print
its unit, and disarm it only after the post-Stop check succeeds. If that check
fails, leave the cleanup timer to fire, then independently verify the original
session and topology before any manual recovery. Never apply the backup to a
replacement Shell/session. No automatic repeat is authorized. Even a clean
run on this different physical layout does not prove the prior connected-HDMI
case or Gate C.

Exp06 may make one identity-checked local reapply after a post-Stop mismatch
while the cleanup timer is active and its restore service is inactive. One
separately approved 2026-09-27 connected-HDMI run observed this local reapply:
exp06 still reported FAIL, the final eDP-only topology verified, and the
second watchdog independently reported PASS. The pre-repair mismatching
state was not saved in that run; see its `2026-09-27-4` observation. Future
exp06 findings retain that initial state, but no repeat is authorized. Continue
to check the timer result and physical desktop independently. A locally
corrected topology is not Gate C PASS or authorization to repeat the run.

**2026-09-27 connected-HDMI connector loss (no repeat):** A separately
approved run recorded both eDP-1 and HDMI-1 logically active immediately
after ScreenCast Stop, followed by one successful local eDP-only reapply.
The second independent exp07 reported FAIL because raw HDMI-1 vanished while
its cable remained plugged in; `/sys/class/drm/card0-HDMI-A-1/status` read
`disconnected` even after one operator cable re-seat. The eDP desktop and
second-device SSH remained usable, but original physical inventory was not
restored. No manual exp07 retry, GPU reset or extra cable cycle is authorized
by that run. The service reported success despite exp07 printing FAIL; a
subsequent synthetic-tested change makes future FAIL reports exit nonzero.
The external monitor normally enters standby without an input signal; standby
alone is neither evidence of a monitor fault nor independent proof of physical
privacy. Preserve the kernel/Mutter findings and the product/Gate C stop.

**2026-09-27 unplugged-HDMI outcome: no repeat.** The one approved run reached
Meta-0-only logical topology with power OFF. The first exp07 watchdog PASS
restored eDP-1 and power ON; after independent software and operator visual
confirmation, exp06 armed a second cleanup watchdog before Enter-triggered
restore and ScreenCast Stop. Its persisted post-Stop findings show only eDP-1
in raw/logical topology, matching the original session/PID, power ON, no
probe errors and Meta-0 gone. The cleanup timer was disarmed; remote desktop
returned to disabled/inactive; the operator confirmed a normal desktop.
This narrows the earlier failure to the connected-HDMI setup without proving
Mutter's cause or a safe connected-HDMI restore. Do not promote Gate C or
repeat either test based on this observation.

### Next supervised diagnostic (prospective rule)

The earlier crash and connected-HDMI restoration failure block product activation
and Gate FEAS-C, not all investigation. The historical one-run approvals above are
spent; they do not authorize a new run. A new bounded diagnostic can proceed on
this host without a new independent review of the *same* mechanism if the operator
explicitly accepts the known crash/privacy/restore risk for its exact command and
layout, and the current code/evidence identifies the failure being investigated.
No approval is needed to continue independent offline implementation or synthetic
tests. A different mechanism, input injection, intentional process kill, or changed
recovery envelope needs its own risk assessment before live execution.

Before each live display/input run, save work, verify a fresh second-device SSH
session, original Shell/login identity and physical layout, capture a unique
baseline, confirm the relevant service state and no stale timers, and verify the
exact-backup identity-guarded restore timer is armed before any mutation. Follow
the two-timer cleanup procedure above when stopping a paused exp06 owner. The
operator watches the physical display through final cleanup; confirm raw and
logical topology, power, Shell/session, timer result and desktop visibility before
declaring recovery. If a prerequisite fails, Shell/session changes, the watchdog
fails, or final topology differs, stop that run, recover via the original-session
path above where possible, and reassess the failure before any retry. Never use
an old backup on a replacement session. Record failures and uncertainty as such;
the next clean diagnostic cannot by itself certify Gate FEAS-C or erase the
connected-HDMI result. Do not repeat matrix runs simply to accumulate PASS reports.

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

2026-10-01 update: for the declared single built-in eDP-1 layout only, FEAS-C is recorded
PASS-WITH-LIMITS (`docs/gnome/display-isolation.md`) after a supervised, operator-approved
run; this supersedes the stop above for that layout and nothing else. Connected HDMI stays
stopped. Since 2026-10-02, the standing approval at the top governs the validated flow; a new layout, intentional kill,
or recovery change still requires its own explicit risk/approval step. The older run-specific rules above are historical.
