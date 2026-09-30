# Emergency input daemon (`remote-emergencyd`)

**Status:** built and tested offline; **not installed, enabled or run against real devices by any
automation**. The live behaviours it relies on were observed with the supervised probe
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
  more than the 10 s watchdog is killed and the kernel then drops the grabs; a frozen process
  keeps them (observed), so an external kill remains the last resort.
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
`read_error`, `release_failed`), `status {phase, held, grabs_enabled}`, `error {reason}`
(`grabs_disabled`, `not_isolated`). `remote_emergencyd::client::Client` is the blocking client.

## Not enabled by default

The binary refuses every `isolate` with `grabs_disabled` unless started with `--enable-grabs`.
`systemd/system/remote-emergencyd.service` is a template with hardening (no network, no
capabilities, `DeviceAllow=char-input rw`); it is not installed by anything here.

## Open items before any use

- Marker ownership: hostd's store accepts only a state directory owned by the calling uid with no
  group or other access, so the template runs the daemon as `remote-hostd` plus the `input` group.
  That lets hostd's uid signal the daemon (Yama `ptrace_scope` 1 still blocks attaching). A
  dedicated uid would need the daemon to write its own marker directory and hostd to honour it,
  which is a change to hostd's store and is not built. Also a polkit rule if `--lock-on-emergency`
  is used without root.
- The chord must be confirmed on the machine's built-in keyboard (a first chord using both right
  keys did not release in one supervised run, cause unknown).
- Releasing keys that were pressed in the few milliseconds before a grab is handled by retry, not
  by injecting releases; repeated cycles, the power button and SysRq under a grab are unobserved.
- `remote-hostd` and the agent do not call this daemon yet; the state machine's activation and
  rollback steps still use the offline fake.
- An independent review of this mechanism.
