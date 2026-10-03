# Offline Emergency Authority Matrix

**Status:** simulation only; no evdev observer, system service or live GNOME
recovery. FEAS-G and the Phase 10 Go/No-Go decision remain open.

| Operation | Hostd alive | Hostd dead | Agent dead |
|---|---|---|---|
| Trigger | Separate `offline-emergency --offline-sim-emergency` process writes an owner-only, fsynced stop marker; hostd/browser not required. | Same. | Same. |
| Authority | Independent process advances the persisted security epoch; hostd checks marker before commands and exits. | Persisted marker and epoch deny restart. | Persisted marker and epoch deny restart. |
| Agent fake recovery | Agent polls marker, revokes input and verifies synthetic teardown, or handles host socket EOF. | Marker poll works with socket still open, including stalled hostd. | No running agent to inspect physical state; gateway shows `FAILED_SAFE`. |
| Gateway/browser | Gateway refuses Start/input and reports `FAILED_SAFE` plus epoch; browser polls status. | Same. | Same. |
| Real lock/display/input | Not implemented by this process. | Not implemented. | Not implemented. |

An offline process test SIGSTOPs the test-owned hostd child before the
independent emergency command. The marker and epoch advance without hostd,
the surviving agent observes the marker with its socket open, and the gateway
refuses input/Start and restart. This does not exercise a deployed daemon or
physical emergency chord.

The marker is intentionally not automatically cleared. Agent-reported
unverified recovery also writes it before hostd exits. This is an offline
stop fence, **not** a certified independent emergency recovery service:
the components run under one UID, the fake's physical state dies with its
process, there is no hardware chord detector, no deployed ACL, and no
operator-approved real restore/lock path. A future independent daemon and
real recovery verifier must resolve those gaps before any live activation.

## Deployed path: `remote-emergencyd` + `blackroom-console` (2026-10-03)

The product path is the console (`docs/ops/console.sh`) with the daemon, not the offline simulation above.
There is no hostd, agent, epoch or stop marker in this chain (the daemon runs without `--state-dir`).
"Observed" means seen on this laptop (built-in eDP-1, event2-5, same uid); everything else is stated as tested
offline or unproven. The system unit template adds `PrivateNetwork=yes` and `RestrictAddressFamilies=AF_UNIX`; the
user unit draft does not.

| Operation | `remote-emergencyd` alone | Needs the console | Needs hostd/agent | Evidence |
|---|---|---|---|---|
| Release the physical input grab on the chord (Left Ctrl+Left Shift+Left Alt+Esc, 2 s) | yes | no | no | Daemon unit tests; observed live (run 1, 2026-10-02, and the 2026-10-01 probes) |
| Release the grab when the console dies or goes silent (socket EOF, 10 s lease) | yes | no | no | Observed (owner kill, Gate F; frozen daemon killed by its watchdog, exp09) |
| Chord with the network down | yes (no network API, only AF_UNIX) | no | no | By construction and tested: the binary links only libc, libgcc_s, libm and the loader; no network or codec crate in the daemon or its client (`crates/remote-emergencyd/tests/process.rs`). Observed once with Wi-Fi turned off (run 22; the stop line was not captured, so this is the operator report). Not run inside a network namespace (unprivileged user namespaces are restricted on this host) |
| Chord with hostd killed | yes | no | not present in this chain | The old chain with hostd SIGSTOPped was observed 2026-10-01; the console chain has no hostd |
| Restore the panel and remove the virtual monitor | no | yes (Stop runs when the daemon reports the early release) | no | Observed in run 1; else the 60 s dead-man `exp07_restore` (observed once, run 13) |
| Lock the screen | no (its optional `loginctl lock-sessions` is off in `console.sh`) | yes (Stop locks, grab released after) | no | Observed; dead-man restore also locks (`--lock-after`, observed once) |
| Persist a stop marker and bump an epoch | yes, only with `--state-dir` | no | hostd's store | Not enabled in this chain; the console has no epoch |
| Refuse a new session after an emergency | yes (latches `emergency_latched` until restarted) | Start then fails at the grab step and rolls back | no | Daemon unit tests; observed (run 22): a stale Start got `Refused("emergency_latched")` at the grab step and rolled back (display restored, locked). The status now reports `latched`, so the console refuses before touching the display (not yet seen live) |
| Repeated emergencies | one per daemon start (latch) | n/a | n/a | Daemon unit tests; not observed live |

Open limits: the daemon runs as the user with ACLs, not as a dedicated uid; the chord is an experiment choice;
suspend, logout and power loss are not observed; FEAS-G stays open until the live emergency runs of Phase 10.