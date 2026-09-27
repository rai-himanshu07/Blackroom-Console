# Offline Console

This is a **simulation**, not a desktop client. By default the gateway links
an offline hostd with a fixed synthetic key to the agent authority in one
process over a Unix socket pair. Separated mode uses a persisted synthetic
key in a private state directory. Neither mode can display the GNOME desktop,
connect to Mutter, or send live input.

From the repository root, run these in separate terminals:

```sh
cargo run -p remote-gateway -- --offline-sim
npm --prefix web ci
npm --prefix web run dev
```

Open `http://127.0.0.1:5173/`. Start simulation, send key/pointer/click/scroll
commands, then revoke and lock. Input is refused while locked; revocation
advances the synthetic epoch. The gateway only binds `127.0.0.1:8787` and
requires the exact `--offline-sim` flag. It does not launch the GNOME agent.
Closing the gateway discards the simulated session and log.

To keep only the synthetic host identity and epoch across gateway restarts,
prepare an **existing, owner-owned `0700` directory outside this repository**
and run the gateway with `--offline-sim --state-dir <absolute-path>`. The path
must contain no symlinks; an unsafe or incomplete directory fails closed.
The console labels this mode `PERSISTED`. Restarting returns to `LOCAL_LOCKED`
with a higher epoch and an empty event log. The private key remains in that
directory; do not use the real hostd secrets directory for this demo.

For a separate-process **offline** simulation, stop the current gateway on
port 8787, build the two offline executables, and use the same private state
directory with explicit absolute binary paths:

```sh
cargo build -p remote-hostd -p gnome-session-agent --bins
cargo run -p remote-gateway -- --offline-sim --separate --state-dir /absolute/private/offline-state --hostd-bin "$PWD/target/debug/remote-hostd" --agent-bin "$PWD/target/debug/gnome-session-agent"
```

The console labels this mode `SEPARATE`. The gateway supervises the two local
processes; the agent acknowledges signed host updates before simulated input
is recorded. The host key and epoch persist, but the fake event log does not.
Hostd gives the gateway a random synthetic Start proof through a private
bootstrap pipe. Each acknowledged Start rotates the proof in the private
control reply; an earlier proof cannot restart after revoke. This is **not**
user authentication, PAM, or an installed service identity. The
agent's fake lock, display, input-isolation and capture
effects must be observed before it acknowledges a signed grant.

The offline authority records a protected `recovery-pending` marker before
issuing a grant. Verified fake restoration clears it on revoke or hostd EOF;
agent death leaves the browser at `FAILED_SAFE` and blocks restart. Never
delete a pending marker by hand: real recovery verification and clearance are
not implemented.

For an offline-only emergency stop of this explicit persisted simulation,
run in another terminal using the **same private state path** (never a live
hostd directory):

```sh
"$PWD/target/debug/offline-emergency" --offline-sim-emergency --state-dir /absolute/private/offline-state
```

The separate process persists an epoch advance and stop marker without
waiting for hostd. The browser reports `FAILED_SAFE` and refuses new control;
restarting the gateway does not clear the marker. This is not a physical
recovery mechanism; no GNOME session or device is changed.

For a disposable separated-process demo instead, use:

```sh
cargo run -p remote-gateway -- --offline-sim --separate-scratch --hostd-bin "$PWD/target/debug/remote-hostd" --agent-bin "$PWD/target/debug/gnome-session-agent"
```

This also displays `SEPARATE`, but creates a private temporary key/epoch.
Stop with Ctrl+C or SIGTERM for graceful child shutdown and scratch cleanup.
A hard kill cannot run cleanup and can leave owner-only directories in `/tmp`;
their contents must be checked before any manual removal.

The agent library also has a synthetic EIS socket test for authorized key
delivery and post-revocation refusal. Offline hostd signs the lease; the agent
checks the socket peer UID (the same UID in this one-process demo) and closes
input on EOF, malformed messages, or stale epochs. This is not evidence of
live GNOME input or physical display/input isolation. A separate offline test
also verifies hostd-to-agent messages across processes. The optional persisted
browser mode without `--separate` still links host and agent authority in one
process and UID. Even with `--separate`, both processes currently run as the
same user; this does not establish product hostd/agent privilege separation.
Offline-only hostd and agent executables now have synthetic process tests
(`cargo test -p remote-hostd --test offline_hostd` and
`cargo test -p gnome-session-agent --test offline_authority`). These flags do
not enable input injection or the installed agent's authority listener.
Deployed hostd identity, peer-UID provisioning, process recovery, live
EIS binding and the physical/privacy gates must be implemented and verified
separately before any live control path is enabled.