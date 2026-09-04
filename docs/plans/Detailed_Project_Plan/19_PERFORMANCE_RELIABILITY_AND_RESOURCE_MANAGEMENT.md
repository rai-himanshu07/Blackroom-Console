# 19_PERFORMANCE_RELIABILITY_AND_RESOURCE_MANAGEMENT.md

## 1. Purpose

This document defines the performance, reliability, resource-management, and long-running stability requirements for the remote-access system.

The objective is to ensure that the system:

- remains responsive during normal remote use
- maintains predictable CPU, memory, GPU, network, and disk usage
- does not gradually leak resources
- does not degrade the host GNOME session over time
- survives network instability
- survives repeated connect/disconnect cycles
- survives component restarts
- remains recoverable under resource pressure
- does not allow performance optimizations to weaken security
- does not allow resource exhaustion to defeat the emergency path
- returns the workstation to a safe state when performance or reliability deteriorates

This document complements:

- Document 7 — Remote Session State Machine
- Document 10 — Feasibility PoC Implementation & Experiment Plan
- Document 12 — Testing Strategy & Test Matrix
- Document 13 — Observability & Diagnostics
- Document 18 — Threat-Driven Security Testing & Red-Team Plan

Performance is subordinate to safety.

A slower system that safely terminates remote control is preferable to a faster system that leaves remote authority active after failure.

---

# 2. Reliability Principles

## 2.1 Fail closed

When the system cannot determine whether remote control is still authorized, it must assume:

```text
remote_control_allowed = false
```

---

## 2.2 Resource exhaustion must not become a security bypass

CPU, memory, GPU, disk, network, PipeWire, WebRTC, or D-Bus exhaustion must never cause the system to:

- skip authentication
- extend a lease
- ignore epoch changes
- retain remote input
- disable emergency handling
- bypass cleanup
- remain remotely active indefinitely

---

## 2.3 Recovery must be bounded

Every recovery operation must have:

- timeout
- retry policy
- maximum retry count
- failure state
- safe fallback

Never retry indefinitely inside a critical transition.

---

## 2.4 Cleanup must be idempotent

The following operations must be safe to execute more than once:

- revoke session
- revoke lease
- increment/invalidate security epoch
- lock session
- disable physical display
- restore physical display
- disable physical input
- restore physical input
- destroy virtual monitor
- terminate WebRTC
- close IPC
- remove temporary state

Repeated cleanup must not create an unsafe state.

---

# 3. Performance Domains

Measure performance independently across:

```text
Authentication
Session preparation
Virtual display
Display capture
Video encoding
Video transport
Input transport
Input injection
Control lease
State transitions
GNOME/Mutter
PipeWire
WebRTC
Gateway
Host daemon
GNOME agent
Emergency daemon
Disk/logging
```

Do not optimize only end-to-end latency.

A low-latency remote desktop is not useful if the host becomes unstable after several hours.

---

# 4. Performance Objectives

The implementation should establish measurable targets rather than relying on subjective impressions.

Initial engineering targets:

### Input latency

Target:

- typical LAN: approximately 50–100 ms end-to-end
- degraded network: graceful degradation rather than uncontrolled buffering

The exact target should be measured on representative hardware.

---

### Frame latency

Avoid unnecessary buffering.

Measure:

- capture timestamp
- encode timestamp
- network send timestamp
- receive timestamp
- decode timestamp
- display timestamp

Use these measurements to identify the actual bottleneck.

---

### Session activation

Measure:

```text
authentication
→ authorization
→ lease
→ GNOME preparation
→ virtual display
→ physical isolation
→ media ready
→ REMOTE_ACTIVE
```

Each stage must have its own timing.

Do not expose a single "connection time" metric only.

---

### Teardown

Measure:

```text
disconnect/failure
→ authority revoked
→ input revoked
→ session locked
→ display restored
→ input restored
→ LOCAL_LOCKED
```

The system must establish a bounded recovery target.

---

# 5. CPU Resource Management

Measure CPU usage for:

- host daemon
- gateway
- GNOME agent
- encoder
- browser client
- emergency daemon

Record:

- average CPU
- peak CPU
- CPU per active remote session
- CPU during activation
- CPU during teardown
- CPU during network degradation
- CPU during reconnect loops

The emergency daemon should have extremely small resource requirements.

Its functionality must not depend on a high-CPU main process.

---

# 6. Memory Management

Measure:

- resident memory
- virtual memory
- allocation rate
- memory growth over time
- per-session memory
- PipeWire buffer memory
- WebRTC buffer memory
- browser memory
- GNOME agent memory

Run long-duration tests:

```text
1 hour
6 hours
12 hours
24 hours
48 hours
```

where practical.

Memory must reach a stable operating range.

Continuous growth without a known bounded cause is a release blocker for the affected component.

---

# 7. Memory Leak Detection

Run repeated cycles:

```text
CONNECT
REMOTE_ACTIVE
DISCONNECT
RECOVER
```

at least:

```text
100 cycles
500 cycles
1000 cycles
```

where hardware/test time permits.

Record memory after every N cycles.

Expected:

- no unbounded growth
- no accumulating PipeWire objects
- no accumulating virtual monitors
- no stale WebRTC sessions
- no stale D-Bus subscriptions
- no stale timers
- no stale file descriptors

---

# 8. File Descriptor Management

Track:

- sockets
- WebSockets
- WebRTC-related descriptors
- PipeWire descriptors
- D-Bus connections
- event descriptors
- input-device descriptors
- temporary files

Repeated connect/disconnect must not increase descriptor count indefinitely.

Test:

```text
baseline
→ connect
→ active
→ disconnect
→ recovery
→ compare baseline
```

A small bounded difference may be acceptable if documented.

Unbounded growth is not.

---

# 9. Thread and Task Management

Track:

- process thread count
- asynchronous tasks
- timers
- event-loop watchers
- background workers

Repeated sessions must not create permanent worker accumulation.

Every task must have an explicit lifecycle.

Avoid detached threads for critical state-machine operations.

---

# 10. GPU Resource Management

Measure:

- GPU utilization
- GPU memory
- encoder utilization
- decoder utilization
- PipeWire/GNOME graphics resource usage
- virtual monitor resource usage

Test:

- low-resolution remote session
- high-resolution remote session
- high-refresh display
- multiple physical monitors
- different GPU vendors
- integrated graphics
- discrete graphics

The implementation must avoid retaining GPU resources after teardown.

---

# 11. Resolution and Refresh Rate

Test supported remote resolutions such as:

- 1280×720
- 1920×1080
- 2560×1440
- 3840×2160

Test representative refresh rates where supported:

- 60 Hz
- 90 Hz
- 120 Hz
- higher supported rates

The system must clearly define what happens when:

- requested resolution is unsupported
- requested refresh rate is unsupported
- GPU cannot provide requested mode
- physical monitor topology changes

Never allow invalid display parameters to destabilize GNOME/Mutter.

---

# 12. Dynamic Quality Adaptation

Where implemented, the remote session may adapt:

- resolution
- frame rate
- bitrate
- codec
- keyframe interval

based on network conditions.

However:

```text
performance adaptation
must never modify
authorization state
```

For example:

- lowering resolution must not affect the control lease
- reconnecting media must not automatically restore remote input
- media recovery must not imply authorization recovery

---

# 13. Network Degradation

Test:

### High latency

Simulate:

- 100 ms
- 200 ms
- 500 ms

### Packet loss

Simulate increasing loss.

### Jitter

Introduce variable latency.

### Bandwidth reduction

Gradually reduce available bandwidth.

### Connection interruption

Disconnect for:

- 1 second
- 5 seconds
- 30 seconds
- several minutes

Expected:

- user-visible degraded state
- controlled reconnect behavior
- no unbounded buffering
- no runaway CPU/memory
- lease semantics remain correct
- safety transition occurs when the configured connection-loss policy is reached

---

# 14. Network Reconnection

Reconnect logic must be bounded.

Avoid:

```text
infinite rapid reconnect
```

Use:

- exponential backoff
- jitter
- maximum retry rate
- connection timeout
- session timeout

Reconnection must never silently revive a stale control lease.

A reconnecting client must satisfy the current authorization/session/epoch rules.

---

# 15. WebRTC Resource Management

Track:

- peer connections
- media tracks
- data channels
- ICE agents
- TURN allocations
- encoders
- decoders
- buffers

After disconnect:

```text
peer_connection_count
media_track_count
data_channel_count
TURN allocation count
```

must return to expected baseline.

Test abrupt termination rather than only graceful close.

---

# 16. PipeWire Resource Management

Track:

- streams
- nodes
- ports
- connections
- buffers
- subscriptions

Repeated virtual-monitor creation/destruction must not leave stale PipeWire objects.

Test:

```text
create virtual monitor
capture
destroy
repeat
```

for long runs.

---

# 17. Mutter Resource Management

Because the architecture depends on GNOME/Mutter integration, test for:

- stale virtual monitors
- stale monitor configurations
- stale display modes
- stale cursor resources
- stale RemoteDesktop sessions
- stale ScreenCast sessions
- repeated DisplayConfig operations

Mutter instability must be treated as a first-class reliability concern.

Do not assume an operation is safe simply because it works once.

---

# 18. GNOME Session Stability

During long-running remote sessions monitor:

- GNOME Shell stability
- desktop responsiveness
- compositor frame rate
- application responsiveness
- input latency
- GPU behavior
- memory growth
- journal errors

Run representative workloads on the host while remotely connected:

- browser
- terminal
- IDE
- Python workload
- GPU workload
- video playback
- file operations

The remote system must not materially destabilize ordinary workstation usage.

---

# 19. Emergency Daemon Resource Guarantees

`remote-emergencyd` is a safety component.

It must remain functional under conditions such as:

- high CPU load
- high memory pressure
- gateway crash
- host daemon crash
- GNOME agent crash
- network flooding
- WebRTC failure
- PipeWire failure

The emergency path must be deliberately kept small.

Do not add:

- browser functionality
- WebRTC
- media processing
- network discovery
- general-purpose scripting
- plugin execution

to the emergency daemon.

---

# 20. Disk and Log Management

Logging must be bounded.

Measure:

- logs per connection
- logs per failure
- logs during attack/flood conditions
- diagnostic bundle size
- audit-log growth

Use:

- rotation
- retention policy
- rate limiting
- structured logging
- secret redaction

A remote attacker must not be able to fill the root filesystem simply by generating errors.

---

# 21. Systemd Resource Controls

Use systemd controls where appropriate.

Potential controls include:

- memory limits
- CPU quotas
- task limits
- file descriptor limits
- restart limits
- watchdog
- timeout controls
- filesystem restrictions

Do not blindly apply aggressive limits.

Every limit must be tested against:

- normal operation
- peak activation
- high-resolution streaming
- degraded network
- recovery
- emergency operation

A limit that kills a safety-critical transition is worse than no limit.

---

# 22. Startup Performance

Measure startup:

```text
boot
→ remote-hostd ready
→ gateway ready
→ GNOME agent connected
→ emergency daemon ready
```

Startup must not automatically enable remote control.

After boot the expected security state should be:

```text
LOCAL_LOCKED
```

or another explicitly documented safe local state.

Remote authority must require fresh authentication.

---

# 23. Shutdown Performance

Test:

- normal shutdown
- reboot
- service stop
- GNOME logout
- forced power-off where possible

The system must not leave persistent state indicating:

```text
REMOTE_ACTIVE
```

in a way that can accidentally restore remote control after reboot.

---

# 24. Failure Recovery Time

Measure recovery time for:

- browser disconnect
- network loss
- gateway crash
- host daemon crash
- GNOME agent crash
- PipeWire failure
- WebRTC failure
- Mutter failure
- emergency takeover

Record:

```text
failure_detected_at
authority_revoked_at
input_revoked_at
session_locked_at
display_restoration_started_at
input_restoration_started_at
LOCAL_LOCKED_at
```

This allows reliability regressions to be detected.

---

# 25. Watchdog Design

Systemd watchdogs should detect genuinely unhealthy services.

Avoid watchdog implementations that merely prove:

```text
process is alive
```

instead of:

```text
service is making progress
```

For the host daemon, health should consider:

- event-loop responsiveness
- state-machine progress
- IPC responsiveness
- security authority responsiveness

For the emergency daemon, health must be independent of the main remote daemon.

---

# 26. Hung-State Detection

Explicitly test:

- deadlock
- event-loop stall
- blocked IPC
- blocked D-Bus call
- stuck PipeWire operation
- stuck WebRTC operation
- stuck Mutter operation

A critical transition must never wait forever.

Every external operation must have a bounded timeout.

---

# 27. Backpressure

Implement explicit backpressure for:

- input events
- WebSocket messages
- protocol requests
- logs
- diagnostics
- media signalling
- reconnect attempts

Never allow unbounded queues.

Input is particularly important.

An attacker must not be able to send millions of input events and cause:

- memory growth
- delayed emergency handling
- event-loop starvation
- delayed revocation

---

# 28. Input Rate Limiting

Remote input should have sensible rate limits.

Protect against:

- event flooding
- malformed coordinates
- impossible pointer movement
- excessive keyboard events

However, rate limiting must not make normal typing or pointer interaction unusable.

Emergency handling must take priority over normal remote input.

---

# 29. State Transition Performance

Measure every major transition.

Example:

```text
LOCAL_LOCKED
→ AUTHENTICATING
→ AUTHENTICATED
→ PREPARING_REMOTE
→ REMOTE_ACTIVE
```

and:

```text
REMOTE_ACTIVE
→ TEARING_DOWN
→ RECOVERING
→ LOCAL_LOCKED
```

Capture duration of every transition and every sub-operation.

Large unexpected increases should produce diagnostics.

---

# 30. Repeated-Cycle Reliability

Create a soak test:

```text
for N cycles:
    authenticate
    establish session
    create virtual display
    disable physical display
    isolate physical input
    remote input
    stream media
    disconnect
    revoke lease
    lock
    restore display
    restore input
    verify LOCAL_LOCKED
```

Run:

- 100 cycles
- 500 cycles
- 1000 cycles where practical

Every cycle must verify all safety invariants.

Do not merely verify that the next connection succeeds.

---

# 31. Long-Running Soak Test

Run remote sessions for:

- 1 hour
- 6 hours
- 12 hours
- 24 hours
- 48 hours

During the session:

- interact with applications
- change windows
- resize windows
- move the pointer
- type
- change resolution where supported
- trigger network degradation
- reconnect
- monitor resources

Record:

- CPU
- memory
- GPU
- network
- file descriptors
- PipeWire objects
- GNOME errors
- service restarts
- latency

---

# 32. Multi-Client Resource Policy

If multiple clients are supported, explicitly define:

- maximum concurrent authenticated clients
- maximum active remote-control session
- whether only one client can hold the control lease
- whether viewing and controlling can be separated
- how concurrent control requests are resolved

Do not allow accidental concurrent input ownership.

If v1 supports only one active remote controller, enforce that at the host authority.

---

# 33. Multiple Browser Tabs

Test:

- same browser, two tabs
- two browsers
- two devices
- simultaneous reconnects

Expected:

- policy is deterministic
- only authorized control lease holder can inject input
- stale tabs cannot control the host
- closing one tab does not accidentally terminate another valid session unless policy says so

---

# 34. Clipboard and Large Data

If clipboard is implemented later, apply resource controls to:

- clipboard size
- transfer rate
- number of updates
- binary content

Do not allow clipboard transfer to become an unbounded memory or disk channel.

Clipboard should remain disabled until its security model is explicitly implemented.

---

# 35. Diagnostics Performance

Diagnostic collection must not significantly disturb the remote session.

Test:

- health query during normal operation
- diagnostic bundle during high CPU
- diagnostic bundle during network degradation
- diagnostic bundle during recovery
- diagnostic bundle during emergency

Diagnostics must be bounded and non-destructive.

---

# 36. Reliability Under System Load

Test while the host is intentionally busy:

### CPU pressure

Use a controlled CPU workload.

### Memory pressure

Use controlled memory allocation.

### Disk pressure

Reduce available disk space in a test environment.

### GPU pressure

Run a GPU workload.

### Network pressure

Generate controlled network traffic.

Expected:

- system may degrade
- remote quality may reduce
- safety invariants remain intact
- emergency remains usable
- eventual safe recovery remains possible

---

# 37. Low-Disk Behavior

Test with:

- normal disk
- low disk
- nearly full disk

The system must not fail because logging or temporary files cannot be written.

Security-critical operations must not depend on unlimited logging capacity.

If persistent security state cannot be safely updated:

- fail closed
- report a clear error
- do not create new remote authority

---

# 38. Low-Memory Behavior

Under memory pressure:

- authentication must not partially succeed
- new leases must not be issued incorrectly
- stale leases must not be accepted
- emergency must remain available
- system should fail closed

Avoid designs where cleanup requires large allocations.

---

# 39. Dependency Failure

Test failure/unavailability of:

- PipeWire
- Mutter
- GNOME Shell
- D-Bus
- libei/EIS
- WebRTC components
- STUN
- TURN
- rendezvous
- mDNS/Avahi

The host must distinguish:

```text
network failure
media failure
input failure
display failure
authorization failure
GNOME failure
service failure
```

and transition appropriately.

---

# 40. Resource Leak Detection

Every long-running component must have a baseline and post-test comparison.

Track:

```text
RSS
FD count
thread count
socket count
PipeWire objects
D-Bus subscriptions
WebRTC peers
timers/tasks
temporary files
log volume
GPU memory
```

Automate leak detection where possible.

---

# 41. Performance Regression Thresholds

Define thresholds after establishing representative baselines.

Examples:

- memory growth per 100 sessions
- CPU increase after 24 hours
- descriptor growth
- session activation time
- teardown time
- input latency
- frame latency
- network bitrate
- GPU memory growth

Avoid arbitrary thresholds before baseline data exists.

The repository should document the baseline hardware/environment for benchmark results.

---

# 42. Benchmark Profiles

Maintain at least these profiles.

## PERF-LAN-1080P

- local LAN
- 1920×1080
- normal desktop activity

## PERF-LAN-4K

- local LAN
- 3840×2160
- representative workload

## PERF-WAN

- Internet path
- realistic latency/jitter
- TURN where applicable

## PERF-DEGRADED

- high latency
- packet loss
- limited bandwidth

## PERF-SOAK

- long-running session

## PERF-CYCLE

- repeated connect/disconnect

## PERF-FAILURE

- component failure during active session

---

# 43. Reliability Metrics

Expose or collect:

### Session metrics

- successful sessions
- failed sessions
- preparation failures
- teardown failures
- recovery failures
- emergency activations

### Security metrics

- authentication failures
- lease expirations
- lease revocations
- epoch increments
- stale-session attempts
- emergency invalidations

### Resource metrics

- CPU
- memory
- GPU
- network
- descriptors
- active processes
- active sessions

### Reliability metrics

- crashes
- restarts
- watchdog activations
- GNOME failures
- PipeWire failures
- WebRTC failures
- display restoration failures
- input restoration failures

---

# 44. Reliability Invariants

The following must remain true under performance/resource pressure.

## INV-PERF-001

Resource exhaustion cannot create remote authority.

## INV-PERF-002

Resource exhaustion cannot extend an expired lease.

## INV-PERF-003

Resource exhaustion cannot invalidate the emergency path.

## INV-PERF-004

A stalled remote component cannot retain remote input indefinitely.

## INV-PERF-005

A network reconnect cannot bypass authentication/authorization.

## INV-PERF-006

Repeated sessions do not produce unbounded resource growth.

## INV-PERF-007

Cleanup eventually reaches a safe state or explicitly enters FAILED_SAFE.

## INV-PERF-008

Performance adaptation cannot modify security authority.

## INV-PERF-009

Remote failure cannot leave physical input permanently disabled without an explicit safe-state mechanism.

## INV-PERF-010

Remote failure cannot leave physical display privacy in an unknown state without entering an explicitly documented safe recovery state.

---

# 45. Failure Escalation

If recovery cannot complete normally:

```text
NORMAL RECOVERY
      |
      v
RETRY
      |
      v
SECONDARY RECOVERY
      |
      v
FAILED_SAFE
```

`FAILED_SAFE` must have a precisely defined meaning.

It must never mean:

> "We don't know what happened."

It must mean:

> "Remote authority is definitely revoked and the system is in a known safe state, even if some convenience functionality is unavailable."

---

# 46. Reliability Testing With Document 18

Every performance test must be combined with security verification where appropriate.

For example:

```text
CPU exhaustion
+
expired lease
```

must verify both:

- system performance behavior
- lease revocation

Likewise:

```text
network flooding
+
emergency shortcut
```

must verify that emergency handling remains responsive.

Performance and security cannot be tested as completely independent domains.

---

# 47. Performance Optimization Rules

Before optimizing:

1. measure
2. identify bottleneck
3. establish baseline
4. implement change
5. benchmark
6. run safety tests
7. run regression tests
8. run long-duration test

Never optimize by:

- removing authentication checks
- extending leases unnecessarily
- disabling safety verification
- bypassing state transitions
- increasing privileged permissions
- making cleanup asynchronous without ownership/timeout semantics
- trusting browser state
- trusting network state

---

# 48. Copilot Agent Implementation Instructions

GitHub Copilot Agent must:

1. Inspect the repository.
2. Inspect the workflow/configuration produced by `adaptive-workflow-configurator`.
3. Respect that workflow.
4. Read Documents 1–18.
5. Identify existing instrumentation before adding new instrumentation.
6. Avoid duplicating metrics/logging systems unnecessarily.
7. Establish baselines before introducing performance thresholds.
8. Implement resource limits conservatively.
9. Add leak tests for long-running components.
10. Add repeated-session soak tests.
11. Add network degradation tests.
12. Add failure-under-load tests.
13. Verify security invariants under resource pressure.
14. Document any environment-specific benchmark assumptions.

Do not reorganize the repository merely to fit this document.

Do not introduce unnecessary infrastructure solely for benchmarking.

---

# 49. Copilot Implementation Order

Implement reliability work in this order:

```text
1. Resource instrumentation
2. State-transition timing
3. Session lifecycle metrics
4. CPU/memory/FD monitoring
5. Cleanup verification
6. Repeated connect/disconnect testing
7. Network degradation testing
8. WebRTC resource cleanup
9. PipeWire cleanup
10. Mutter resource cleanup
11. GNOME stability testing
12. systemd watchdog validation
13. bounded retry/backoff
14. backpressure
15. resource limits
16. fault-under-load testing
17. long-running soak tests
18. performance benchmarks
19. regression thresholds
20. release reliability gate
```

---

# 50. Definition of Done

This document is implemented when:

- resource metrics exist for all critical components
- activation and teardown timing is observable
- repeated sessions do not leak resources
- WebRTC resources are cleaned up
- PipeWire resources are cleaned up
- virtual monitor resources are cleaned up
- file descriptors remain bounded
- threads/tasks remain bounded
- logging remains bounded
- network reconnect is bounded
- backpressure exists on untrusted inputs
- systemd watchdog behavior is tested
- emergency handling remains available under load
- long-running sessions have been tested
- repeated-cycle testing has been performed
- failure-under-load testing has been performed
- performance baselines are documented
- security invariants have been verified under resource pressure

---

# 51. Release Reliability Gates

Release is blocked if:

- memory grows without bound
- descriptors grow without bound
- repeated sessions leak virtual monitors
- PipeWire resources accumulate
- WebRTC peers remain after teardown
- reconnect loops consume unbounded resources
- log flooding can exhaust disk
- remote input can survive resource exhaustion
- emergency handling becomes unavailable under normal load
- recovery can hang indefinitely
- GNOME becomes unstable after repeated sessions
- resource limits cause unsafe recovery
- high CPU/memory/network load can bypass a security invariant

---

# 52. Final Reliability Principle

The system must be designed around the assumption that:

```text
NETWORKS FAIL
PROCESSES CRASH
PIPEWIRE FAILS
MUTTER FAILS
GNOME RESTARTS
GPUS MISBEHAVE
BROWSERS DISCONNECT
CLIENTS MISBEHAVE
RESOURCES RUN LOW
```

The correct response is not to pretend these conditions cannot happen.

The correct response is to ensure:

```text
FAILURE
   |
   v
DETECT
   |
   v
REVOKE AUTHORITY
   |
   v
RECOVER
   |
   v
VERIFY
   |
   v
KNOWN SAFE STATE
```

Performance is successful only when the system remains responsive **without compromising that safety model**.

The ultimate reliability requirement is:

> A remote session may become degraded, slow, disconnected, crashed, or completely unavailable; it must never become an uncontrolled or permanent remote-control session.