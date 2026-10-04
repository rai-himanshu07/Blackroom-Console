# Emergency input daemon (`remote-emergencyd`)

**Status:** built and tested offline. The `.deb` installs it as the user unit
`blackroom-console-emergencyd.service` (started with the console, `--enable-grabs`, no `--state-dir`, no
`--lock-on-emergency`), and the owner used the grab through the console. The emergency chord was **not** exercised through the
packaged console. The live behaviours it relies on were observed with the supervised probe
(`exp09_grab_probe`, Phase 7 step 5, `docs/security/input-isolation-decision.md`). FEAS-E and
FEAS-G are not promoted by this code.

## What it does

- Holds the exclusive `EVIOCGRAB` on physical keyboards, mice and touchpads (allow-list by
  capability bits, `seat0` only, software devices skipped) while a remote session is active.
- **Release gate:** before grabbing it waits until no key or button is down and nothing has been
  pressed for 0.5 s (bounded by 20 s). After the grab lands it reads once more and, if anything was
  pressed or is still down, undoes the grab and retries (at most 5 times). A key held at grab start
  would otherwise be seen by the session as down forever and auto-repeat.
- **Lease:** isolation lapses unless the controlling client renews it (1 s to 60 s window). The
  check runs in the same loop as event reading. The loop never blocks on the client for more than
  10 ms per reply, handles at most 4 requests per pass, and runs `loginctl` on the side with a 5 s
  kill, so a slow client or logind cannot starve the lease or the chord. A loop that stalls for
  more than the 10 s watchdog is killed (the unit sets `WatchdogSignal=SIGKILL`, because a SIGSTOPped
  process cannot act on the default SIGABRT) and the kernel then drops the grabs. A frozen process
  keeps them without a supervisor (observed with the probe). Observed once under a transient user unit
  with the same properties: a SIGSTOPped holder was killed by the watchdog and its connection closed 9.883 s
  later (`exp09/2026-10-01`); the shipped unit file was never loaded.
- **Emergency chord:** Left Ctrl + Left Shift + Left Alt + Esc held 2 s on any grabbed keyboard
  (an experiment choice, to be confirmed on each machine). Order of actions (assessment C25):
  release every grab, persist `remote-hostd`'s independent stop marker and epoch bump, optionally
  `loginctl lock-sessions`, then tell the client (`released chord`, then `emergency` with `ok`,
  `failed` or `off` for the marker and the lock; failures also go to the journal). The actions run
  even if the release itself failed, and a failed release is retried on every loop pass until it
  works (`released release_recovered`). After a chord the daemon refuses every further `isolate`
  (`emergency_latched`) until it is restarted on purpose.
- **Hotplug:** the node list is rescanned every 250 ms; a new allow-listed node is grabbed while
  isolated, and a grab failure or loss of every grabbed node releases everything.
- **Fail closed:** a read error, a failed hotplug grab, the client disappearing, a malformed or
  oversized request line, or a lapsed lease restore local input.
- **Privacy:** key codes and coordinates are never logged, stored or sent. Only the four chord keys
  are tracked, in memory, inside the chord detector. Messages carry reasons and counts.

## Control socket

One JSON object per line over a Unix socket (mode 0600) that accepts a single client with the
configured uid; a second or foreign connection is closed at once. Requests: `isolate {lease_ms}`,
`renew`, `restore`, `status`. Replies and pushed events: `accepted`, `isolated {nodes}`,
`refused {reason}` (`busy`, `bad_lease`, `keys_held`, `nothing_to_grab`, `grab_failed`),
`released {reason}` (`restore`, `lease_expired`, `chord`, `hotplug_fail_closed`, `coverage_lost`,
`read_error`, `release_failed`), `status {phase, held, grabs_enabled, reads, active_nodes}` (`reads` counts
key presses/releases and pointer passes the grabbed nodes saw since the last grab landed and `active_nodes`
how many nodes saw any: counts only, kept until the next grab), `error {reason}`
(`grabs_disabled`, `marker_unavailable`, `not_isolated`). `marker_unavailable` means `--state-dir`
was given but hostd's store does not accept it right now (not owned by the daemon's uid, group or
other access, a symlink in the path, or hostd has not initialised it), so the chord could not
write its stop marker; nothing is grabbed. `remote_emergencyd::client::Client` is the blocking client.

## Who holds the lease (decided 2026-10-01)

`remote-hostd` owns the only client (`isolation.rs`, `--offline-sim-service ... --emergency-socket
<absolute path>`, off by default). The daemon dies with its client, so nothing else may connect while
a grant lives.

- **Start:** after the agent acknowledged the grant, hostd connects and asks for the grab with a 10 s
  lease. If the daemon refuses or is unreachable, Start fails closed with `INPUT_ISOLATION_FAILED`,
  the agent is told to revoke and the audit log records `isolation_failed`.
- **Renewal:** every 2 s hostd renews the daemon lease only while the newest browser renew that the
  agent acknowledged is at most 15 s old. A dead link or agent therefore returns local input within
  about 25 s, under the 30 s control lease.
- **Loss:** a pushed `released` event, a lapsed lease or an unreachable daemon ends the grant
  (`isolation_lost`) without any browser action.
- **End of grant:** Revoke, expiry, abuse limit or any other teardown restores the grab and closes the
  connection, so `blackroom emergency-status` works while idle.
- **Trust and failure handling:** hostd refuses a daemon whose socket peer is not its own uid
  (`daemon_untrusted`), retries only a dropped connection, treats a timeout as final, and leaves the
  Start proof usable after a refused grab. The signed grant is issued before the grab, so a grab that
  takes most of the 30 s lease (a key held down) can lapse the first lease; Start again.
- **Not covered:** a daemon crash between ticks is noticed within one tick, and the stop marker after
  a chord still takes the existing `emergency_required` path.

## Not enabled by default

The binary refuses every `isolate` with `grabs_disabled` unless started with `--enable-grabs`.
`systemd/system/remote-emergencyd.service` is a template with hardening (no network, no
capabilities, `DeviceAllow=char-input rw`); it is not installed by anything here.

## Open items before any use

- Marker ownership: hostd's store accepts only a state directory owned by the calling uid with no
  group or other access, so the template runs the daemon as `remote-hostd` plus the `input` group.
  A manual supervised run (`docs/ops/live-grab-runbook.md`) runs it as the operator's own uid with
  temporary ACLs on the event nodes, never under sudo.
  That lets hostd's uid signal the daemon (Yama `ptrace_scope` 1 still blocks attaching). A
  dedicated uid would need the daemon to write its own marker directory and hostd to honour it,
  which is a change to hostd's store and is not built. Also a polkit rule if `--lock-on-emergency`
  is used without root.
- The chord must be confirmed on the machine's built-in keyboard (a first chord using both right
  keys did not release in one supervised run, cause unknown).
- Releasing keys that were pressed in the few milliseconds before a grab is handled by retry, not
  by injecting releases; repeated cycles, the power button and SysRq under a grab are unobserved.
- The agent's activation and rollback steps still use the offline fake; its "physical input
  isolated" check does not ask the daemon. hostd calls the daemon only when started with
  `--emergency-socket` (below). `remote-gateway ... --separate-scratch|--separate ...
  --emergency-socket <abs path>` passes it on, waits up to 30 s for a Start reply, reports a refused
  grab as `INPUT_ISOLATION_FAILED`, tells the page (`physical_input_grab`, a warning bar) and prints
  a banner. The demo code is still public, so anyone local who knows it can Start and grab input.
- An independent review of this mechanism.
