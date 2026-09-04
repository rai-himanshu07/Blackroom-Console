# Document Extraction: 07 (Remote Session State Machine) & 06 (Systemd Services & Privilege Model)

---

### 07 — Remote Session State Machine

#### 1. Purpose & scope
Defines the authoritative runtime state machine for the whole system as a **behavioral contract**: states, legal transitions, guards, transition actions, failure handling, rollback, timeouts, concurrent-event handling, crash recovery, security invariants, emergency takeover, and reconnect behavior — "implementation details may change, but the externally observable safety properties and state-transition semantics defined here must remain true" ([07 — Remote Session State Machine.md](docs/plans/Detailed_Project_Plan/07%20—%20Remote%20Session%20State%20Machine.md), §1). Scope is explicitly narrow: Ubuntu 26.04 LTS / GNOME 50+ / Wayland / systemd / PipeWire / Mutter / single-user workstation only, with X11, KDE, wlroots, multi-user switching, headless sessions, and other distros explicitly out of scope and to be rejected at capability detection (§3).

#### 2. Named identifiers defined
**States (top-level, 11, §4):** `LOCAL_ACTIVE`, `LOCAL_LOCKED`, `AUTHENTICATING`, `AUTHENTICATED`, `PREPARING_REMOTE`, `REMOTE_ACTIVE`, `REMOTE_DEGRADED`, `TEARING_DOWN`, `RECOVERING`, `EMERGENCY`, `FAILED_SAFE`. No named sub-states are ever given — the doc permits "additional internal substates" (§4) but names none.

**Events/triggers** (from §8 transition table + prose): `lock`, `connection`, `auth success`, `auth failure`, `retry limit`, `authorization success`, `timeout`, `success` (reused across PREPARING_REMOTE/TEARING_DOWN/RECOVERING/FAILED_SAFE rows), `failure` (two guard variants), `lease renewal`, `transient failure`, `disconnect`, `lease expiry`, `client disconnect`, `partial failure`, `emergency`, `completed`, `restoration failure`.

**Guards** (free-text, no formal IDs): "session available", "remote access enabled", "all required factors valid", "retry limit not exceeded"/"limit exceeded", "policy allows access", "timeout exceeded", "all safety checks pass", "rollback possible"/"rollback uncertain", "valid", "lease remains valid", "any", "no renewal", "invalid", "restoration verified", "recovery possible", "safety uncertain", "all checks pass", "emergency trigger", "safety verified", "uncertain".

**Invariants (§7)** — named "Invariant 1"–"Invariant 10", **not** an "INV-###" scheme: 1 No valid lease, no remote input; 2 Authentication does not equal control; 3 TOTP is always mandatory; 4 New clients require Remote Access Key; 5 Emergency always revokes remote authority; 6 Old security epochs are invalid; 7 Disconnect returns to locked local console; 8 No automatic local unlock; 9 Recovery is idempotent; 10 Fail closed.

**Transition IDs:** only `transition_id = UUID` as a concept (§26) — no fixed literal ID scheme.

**Timeouts:** **no numeric values anywhere.** §14 names three qualitative timer classes — "authentication/session credential" (relatively long-lived), "control lease" (short-lived), "heartbeat" (frequent) — and states verbatim: *"Exact production timeout values must be established during testing rather than hard-coded prematurely."* Emergency hold duration is only an example: `Ctrl + Alt + Shift + F12` held "approximately two seconds" (§17), explicitly caveated as configurable.

**Security epoch:** illustrative only — `epoch = 41` → `epoch = 42` (§16).

**Control lease fields (§13):** `host_id`, `user`, `client_id`, `session_id`, `security_epoch`, `issued_at`, `expires_at`, `capabilities`. **Lease capability types (§34):** `keyboard`, `pointer`, `touch`, `tablet`, `clipboard`.

**Persisted state categories (§45):** host identity, authentication configuration, TOTP configuration, Remote Access Key verifier, trusted-device records, security configuration, policy. **Explicitly transient (non-authoritative) state:** active connection, active lease, PipeWire stream, virtual monitor, current transition.

**Log fields (§46):** `timestamp, host_id, transition_id, session_id, client_id, previous_state, event, next_state, result, failure_code, security_epoch, duration`. **Never-log list:** password, TOTP secret, Remote Access Key, session bearer token, trusted-device private credential.

**Diagnostics fields (§47):** current state, GNOME session status, remote session status, lease status, security epoch, virtual monitor status, physical output status, physical input status, PipeWire status, Mutter capability status, emergency daemon status, last transition, last recovery failure.

**Display snapshot fields (§32):** connector/output identity, enabled/disabled state, mode, resolution, refresh rate, position, scale, transform, primary display.

**Acceptance-criteria IDs (§56):** A–J (No remote input without authorization / Disconnect is fail-safe / Network loss is fail-safe / Emergency is independent / Emergency invalidates stale sessions / Physical privacy is restored / Physical control is restored / No automatic unlock / Recovery is idempotent / Startup is safe).

**Categories with zero occurrences in this document** (confirmed by full read + grep): service unit names, service users/groups, socket paths, `/etc|/var/lib|/run|~/.config` paths, `CAP_*` capabilities, systemd hardening directives, polkit actions, D-Bus names, file modes, named IPC message/method identifiers — all of these belong to Document 06.

#### 3. Hard requirements, gates and stop conditions
- System **must fail closed** (§1); no desktop control during `AUTHENTICATING` (§5.3); authentication alone **never** grants control (Invariant 2); TOTP mandatory even for trusted clients (Invariant 3); untrusted/new clients require username+password+TOTP+Remote Access Key (Invariant 4).
- `PREPARING_REMOTE` **must be transactional**; any mandatory-step failure rolls back (§5.5). Remote input authority **must be revoked before** the system is considered safe in `TEARING_DOWN`, regardless of exact ordering (§5.8).
- Recovery **must be idempotent** — repeat calls must not worsen state (§27, Invariant 9). Emergency **must not depend on** browser, WebRTC, network, signalling, gateway, or a healthy `remote-hostd` (§5.10, §19).
- **Never** silently transition to `LOCAL_ACTIVE` while restoration is uncertain; **never** assume recovery succeeded merely because an API call didn't throw (§5.11). **Never** automatically unlock GNOME after teardown (Invariant 8, §37).
- A stale or **trusted** client can **never** regain control merely by reconnecting; a trusted-client credential alone must never establish control (§15, §38, §39). Only **one** active remote controller permitted — reject a second, never silently transfer (§43); control transfer is explicitly **out of scope for v1** (§44).
- Do not wait indefinitely for reconnection after network loss (§12). Emergency **always** overrides trusted status (§40). Do not blindly restart GNOME components — recovery loops must have limits (§21).
- Concurrency priority is fixed: `EMERGENCY > SAFETY/FAILURE > LEASE EXPIRY > DISCONNECT > NORMAL TRANSITION > RECONNECT` (§24) — simultaneous `reconnect + emergency` on `REMOTE_ACTIVE` must resolve to `EMERGENCY`, never `REMOTE_ACTIVE`.
- **Feasibility Gate (§57)** — nine hard gates: same-session reuse, virtual monitor, physical display isolation, remote input, physical input isolation, GNOME lock semantics, safe teardown, emergency takeover, reliable restoration. If any fails: "stop and reassess the architecture… Do not hide an architectural incompatibility behind additional abstraction layers."

#### 4. Technology and library choices
No implementation language is mandated. Pseudocode is Python-flavored (```python fenced blocks at §48–§50) and explicitly introduced as "Conceptual implementation:" / "Conceptual:" — illustrative only, not a language mandate. No mention anywhere of Rust/TypeScript/JavaScript (verified by grep). No concrete GNOME/Mutter/PipeWire/libei APIs, crates, or libraries are named — that detail is deferred elsewhere. The one firm architectural mandate (§58): keep state-machine logic independent of GNOME specifics, and "keep GNOME/Mutter-specific behavior behind a clearly isolated backend" — a direct license for a ports-and-adapters design with a mockable GNOME adapter.

#### 5. State machine detail

**State definitions (§5.1–5.11), condensed:**

| State | Entry conditions | Key notes |
|---|---|---|
| `LOCAL_ACTIVE` | GNOME session exists, physical display+input available, remote input disabled, no lease | normal post-unlock state |
| `LOCAL_LOCKED` | session locked, lease absent, physical display/input restored, remote input disabled | required safe state after teardown; also pre-activation state |
| `AUTHENTICATING` | remote client attempting auth (username+password+TOTP, +Remote Access Key or trusted-client cred) | no control granted |
| `AUTHENTICATED` | short-lived session credential issued | remote input/display/input NOT yet touched |
| `PREPARING_REMOTE` | validating/creating lease, preparing virtual output, disabling physical I/O | transactional; must roll back on failure |
| `REMOTE_ACTIVE` | session+lease+epoch valid, virtual display active, physical I/O isolated, remote input active | only state where remote input is permitted |
| `REMOTE_DEGRADED` | transient network/WebRTC/media/signalling interruption | short lease timeout; must eventually fail closed |
| `TEARING_DOWN` | session terminating | must revoke remote input before declaring safe |
| `RECOVERING` | error during prep/teardown | must be idempotent |
| `EMERGENCY` | independent emergency controller invoked | preempts everything; no dependency on browser/WebRTC/network/gateway/hostd |
| `FAILED_SAFE` | restoration unverifiable | conservative; never silently becomes `LOCAL_ACTIVE`; requires explicit local recovery |

**Complete transition table (§8, reproduced verbatim, 25 rows):**

| Current State | Event | Guard | Action | Next State |
|---|---|---|---|---|
| LOCAL_ACTIVE | lock | session available | lock GNOME | LOCAL_LOCKED |
| LOCAL_LOCKED | connection | remote access enabled | begin authentication | AUTHENTICATING |
| AUTHENTICATING | auth success | all required factors valid | issue session credential | AUTHENTICATED |
| AUTHENTICATING | auth failure | retry limit not exceeded | reject | AUTHENTICATING |
| AUTHENTICATING | retry limit | limit exceeded | temporary block | LOCAL_LOCKED |
| AUTHENTICATED | authorization success | policy allows access | create lease | PREPARING_REMOTE |
| AUTHENTICATED | timeout | timeout exceeded | invalidate credential | LOCAL_LOCKED |
| PREPARING_REMOTE | success | all safety checks pass | activate remote control | REMOTE_ACTIVE |
| PREPARING_REMOTE | failure | rollback possible | rollback | LOCAL_LOCKED |
| PREPARING_REMOTE | failure | rollback uncertain | fail-safe recovery | FAILED_SAFE |
| REMOTE_ACTIVE | lease renewal | valid | continue | REMOTE_ACTIVE |
| REMOTE_ACTIVE | transient failure | lease remains valid | suspend/recover | REMOTE_DEGRADED |
| REMOTE_ACTIVE | disconnect | any | teardown | TEARING_DOWN |
| REMOTE_ACTIVE | lease expiry | no renewal | teardown | TEARING_DOWN |
| REMOTE_DEGRADED | lease renewal | valid | restore connection | REMOTE_ACTIVE |
| REMOTE_DEGRADED | lease expiry | invalid | teardown | TEARING_DOWN |
| REMOTE_DEGRADED | client disconnect | any | teardown | TEARING_DOWN |
| TEARING_DOWN | success | restoration verified | lock | LOCAL_LOCKED |
| TEARING_DOWN | partial failure | recovery possible | retry recovery | RECOVERING |
| RECOVERING | success | safe state verified | lock | LOCAL_LOCKED |
| RECOVERING | failure | safety uncertain | remain conservative | FAILED_SAFE |
| FAILED_SAFE | recovery success | all checks pass | lock | LOCAL_LOCKED |
| ANY | emergency | emergency trigger | revoke + invalidate + restore | EMERGENCY |
| EMERGENCY | completed | safety verified | remain locked | LOCAL_LOCKED |
| EMERGENCY | restoration failure | uncertain | conservative recovery | FAILED_SAFE |

**`PREPARING_REMOTE` sub-steps** — two different granularities exist in the doc: §5.5 gives 14 unordered "typical operations" (validate GNOME session → validate capabilities → validate session ownership → validate security epoch → create remote control lease → prepare virtual output → capture original display topology → activate virtual monitor → disable physical outputs → prepare remote input → disable physical input → verify resulting topology → verify input routing → verify session state). §9 gives a **22-step numbered transaction** `BEGIN_REMOTE_ACTIVATION…COMMIT`: 1 Acquire state-machine lock, 2 Confirm state == LOCAL_LOCKED, 3 Confirm supported GNOME environment, 4 Confirm target GNOME session, 5 Confirm no conflicting remote session, 6 Validate security epoch, 7 Create remote session record, 8 Create control lease, 9 Snapshot physical display configuration, 10 Snapshot input state, 11 Prepare virtual monitor, 12 Verify virtual monitor, 13 Disable physical outputs, 14 Verify physical outputs are isolated, 15 Prepare remote input, 16 Disable physical input, 17 Verify physical input isolation, 18 Verify GNOME session remains usable, 19 Verify PipeWire capture, 20 Verify remote input path, 21 Mark REMOTE_ACTIVE, 22 Release state-machine lock. Rollback on failure follows §10's **10-step** reverse-order list: 1 Stop remote input, 2 Invalidate control lease, 3 Disable remote control, 4 Restore physical input, 5 Restore physical display topology, 6 Destroy virtual monitor, 7 Restore original monitor configuration, 8 Lock GNOME, 9 Clear transient state, 10 Verify safe state → `LOCAL_LOCKED` if verified else `FAILED_SAFE`.

**`TEARING_DOWN` sub-steps** — again two granularities: §5.8's 9-item "required order" (stop accepting remote input → invalidate remote control lease → invalidate remote session authority → restore physical input → restore physical display → destroy virtual display → restore original monitor configuration → lock GNOME session → clear transient remote state), vs §11's 6-item disconnect prose (invalidate lease, terminate session, stop remote input, restore local hardware, lock GNOME, verify restoration).

**Timeout table:** none exists — see §2/§4 above; only the three qualitative buckets in §14.

**Concurrency/serialization model (§24–26):** a single `StateMachineLock` conceptually holds `state`, `active_session`, `active_lease`, `security_epoch`, `transition_id`; fixed priority order as listed in §3 above; worked example given (simultaneous reconnect+emergency ⇒ EMERGENCY).

**Idempotency rules (§27, Invariant 9):** `revoke_remote_input()`, `lock_session()`, `restore_display()`, `destroy_virtual_monitor()` must all be no-op-safe when already applied — required because crash recovery may repeat cleanup.

**Persistence/recovery-on-restart (§28–30, §45):** startup = load persisted config → inspect previous runtime state → invalidate stale sessions/leases → verify security epoch → inspect GNOME/display/input → restore safe local config → lock where appropriate → verify → `LOCAL_LOCKED`. Power loss: all prior sessions invalid on next boot, no auto-restore. Suspend during `REMOTE_ACTIVE` should be inhibited; if it happens anyway → `RECOVERING` → full revalidation of GNOME/display/PipeWire/input/lease/epoch, else `TEARING_DOWN`.

**Emergency preemption:** can interrupt **any** state (§6 diagram); is priority 1 in concurrency ordering (§24); overrides trusted-client status (§40); abandons an in-flight `PREPARING_REMOTE` transaction rather than running normal rollback (§18); pseudocode (§49) nests `try/finally` so one failing cleanup step never blocks the rest.

#### 6. Privilege model detail
Not this document's focus (see Document 06). Doc07 only references privilege-adjacent components in passing: `remote-hostd` crash handling (§20 — lease stops renewing → expires → input revoked → session locked → hardware restored, with `remote-emergencyd` as an "additional local safety mechanism"), and GNOME session/Mutter/PipeWire as black-box dependencies whose failure triggers `RECOVERING` (§21).

#### 7. Tests / verification procedures specified
No numeric test-case IDs exist; categories only (§53–56): **Unit Tests** — every legal/illegal transition, lease expiration, epoch mismatch, auth/authz failures, emergency handling, rollback, idempotent cleanup, concurrent events, startup recovery. **Integration Tests** — GNOME session discovery, Mutter ops, virtual monitor creation, topology changes, physical input isolation, PipeWire lifecycle, GNOME locking, restoration, systemd restart. **Failure Injection** — network disconnect, gateway kill, host daemon kill, GNOME agent kill, PipeWire failure, Mutter op failure, display/input restore failure, power interruption. **Concurrency Tests (§54)** — disconnect+emergency, lease expiry+reconnect, emergency+reconnect, daemon restart+reconnect, hotplug+disconnect, Mutter failure+emergency, PipeWire failure+emergency. **Recovery Tests (§55)** — fail-once/fail-repeatedly/fail-after-partial-completion/retry/restart-daemon/restart-GNOME. **Acceptance Criteria A–J (§56)** as listed in §2 above.

#### 8. Phase / sequencing guidance and definition-of-done
§58 "Implementation Rules for GitHub Copilot Agent" (10 rules) — inspect repo and the `adaptive-workflow-configurator` output first, don't duplicate it, follow existing structure, keep state-machine logic GNOME-independent, keep GNOME specifics behind an isolated backend, keep privileged ops narrowly scoped, keep emergency independent of browser/network, **do not implement the complete product before feasibility gates pass**. §59 "Recommended Implementation Order" (15 steps): 1 State definitions, 2 Event definitions, 3 Transition engine, 4 State-machine locking, 5 Control lease model, 6 Security epoch, 7 Unit tests, 8 GNOME session adapter, 9 Display transaction, 10 Input transaction, 11 Lock/unlock integration, 12 Recovery engine, 13 Emergency integration, 14 Failure injection, 15 End-to-end state-machine tests — only **after** these are reliable should authentication, networking, WebRTC, browser UI, trusted devices, and production packaging begin. Definition-of-done = Acceptance Criteria A–J (§56) plus the Feasibility Gate (§57) plus the closing principle (§60): "Remote control is a temporary lease, not a permanent mode."

#### 9. Cross-references
Implicitly depends on: authentication/TOTP/Remote Access Key/trusted-client concepts (Document 03), GNOME/Mutter/PipeWire/libei details (Document 05), `remote-hostd`/`gnome-session-agent`/emergency architecture (Document 06), browser/WebRTC/gateway (Document 04), test categories overlapping Document 12, packaging/upgrade concerns overlapping Documents 06/15. §58 explicitly instructs reading "previous architecture/security/GNOME documents" before implementing.

#### 10. Ambiguities, gaps, internal contradictions, and conflicts with Document 00
- **Invariant scheme mismatch vs Document 00:** Document 00 (per session context) defines `INV-001`…`INV-014` (14 invariants). Doc07 §7 (lines 452–563) instead defines 10 unlabeled "Invariant 1"–"Invariant 10" with no `INV-` prefix and different granularity — e.g., "lease bound to session/client/host/epoch/expiry/capabilities" from Document 00's set has no corresponding numbered invariant here (the same field list appears only descriptively in §13 Control Lease, not framed as an invariant).
- **Activation transaction step-count mismatch vs Document 00:** Document 00 states a "15-step activation transaction," but doc07 §9 (`BEGIN_REMOTE_ACTIVATION`, line 621) actually enumerates **22** numbered steps (lines 623–655). No 15-step version exists anywhere in this document.
- **Disconnect transaction step-count mismatch vs Document 00:** Document 00 states a "10-step disconnect transaction." Doc07 has no single canonical list of exactly that description — the closest candidates are §10's 10-step **rollback-of-failed-activation** (lines 657–703, not a normal disconnect), §11's 6-item disconnect prose (lines 704–738), and §5.8's 9-item `TEARING_DOWN` order. None is explicitly labeled "disconnect transaction."
- **Internal inconsistency, `PREPARING_REMOTE`:** §5.5's 14-item unordered list and §9's 22-step numbered transaction describe the same phase at different granularity and don't align 1:1 (§9 adds explicit lock-acquisition/state-confirmation pre-steps and verification sub-steps 18–20 that §5.5 never itemizes).
- **Internal inconsistency, teardown/rollback:** three different-length lists (10-step rollback, 6-item disconnect, 9-item teardown order) cover overlapping ground without being reconciled into one canonical procedure.
- **No timeout table exists** despite the topic inviting one — §14 explicitly refuses to hard-code values.
- **Overloaded event vocabulary:** `success`/`failure` are reused across four different current-states in the transition table (§8), disambiguated only by row context and free-text guards, not by distinct event types — an implementer must invent qualified event types (e.g. `PreparationSucceeded` vs `TeardownSucceeded`).
- No component-name variants appear internally (uses `remote-hostd` consistently, e.g. §20/§28); prose sometimes says "GNOME session agent" instead of the hyphenated `gnome-session-agent`, but this reads as informal phrasing, not a conflicting identifier.

#### 11. Risks and feasibility concerns the doc itself raises
- §57 Feasibility Gate: the nine listed capabilities are called **hard** gates — "If any of these cannot be implemented safely, stop and reassess the architecture. Do not hide an architectural incompatibility behind additional abstraction layers."
- §21: Mutter/GNOME Shell may become unavailable mid-session; explicit warning against blind repeated restarts (restart-loop risk).
- §31: physical monitor topology can change mid-session (HDMI/DP/USB-C/dock) — risk of restoring the wrong layout if only a fixed layout is assumed.
- §33: physical input device identity is not stable across reboot/hotplug/USB reconnection/docking — naive `/dev/input/event*` indexing is explicitly flagged as insufficient.
- §30: suspend during `REMOTE_ACTIVE` is a named risk; preferred mitigation is inhibiting suspend outright.
- §16: whether the security epoch is persisted or regenerated across daemon restart is left as an **open implementation decision**, with an explicit warning that getting it wrong lets stale sessions survive restart.
- §29: power loss with zero software-cleanup opportunity is named explicitly as a scenario the design must survive.
- The document frames itself as a contract that could still fail at the feasibility-gate stage before "the complete product" is built (§57) — i.e., the authors acknowledge the whole approach might not survive contact with real GNOME/Mutter/PipeWire behavior.

---

### 06 — Systemd Services & Privilege Model

#### 1. Purpose & scope
Defines the Linux process/systemd/privilege/sandboxing/startup/watchdog/IPC/recovery architecture for `remote-hostd`, `gnome-session-agent`, `remote-gateway`, `remote-emergencyd`: lifecycle, user services, privilege separation, capabilities, filesystem permissions, D-Bus access, IPC, watchdogs, startup ordering, shutdown, crash recovery, sandboxing, resource limits, emergency independence ([06 — Systemd Services & Privilege Model.md](docs/plans/Detailed_Project_Plan/06%20—%20Systemd%20Services%20&%20Privilege%20Model.md), §1). Target: Ubuntu 26.04 LTS/GNOME 50+/Wayland/systemd/single-user workstation (§1). Core principle (§2): never run the whole stack as root — `INTERNET → remote-gateway(UNPRIVILEGED) →[auth IPC]→ remote-hostd(SYSTEM SERVICE) →[auth IPC]→ gnome-session-agent(USER SESSION)`, and independently `Physical Keyboard → remote-emergencyd(MINIMAL PRIVILEGE)`.

#### 2. Named identifiers defined
**Components/process names (§3–4):** `remote-gateway`, `remote-hostd`, `gnome-session-agent`, `remote-emergencyd`; plus helper/adjacent processes introduced later: `pam-auth-helper` (§37, line 801), `remote-input-helper` (§42, line 889), unnamed "media process"/"media/encoder" (§75–78, §107).

**Privilege domains (§4):** DOMAIN A "Network" = remote-gateway/UNPRIVILEGED; DOMAIN B "Security" = remote-hostd/SYSTEM SERVICE; DOMAIN C "Desktop" = gnome-session-agent/USER SESSION; DOMAIN D "Emergency" = remote-emergencyd/MINIMAL PRIVILEGE.

**Service unit paths (§11, lines 316–337):** `/etc/systemd/system/remote-hostd.service`, `/etc/systemd/system/remote-gateway.service`, `/etc/systemd/system/remote-emergencyd.service`; `~/.config/systemd/user/gnome-session-agent.service` (user unit). Caveated: "exact installation paths may differ depending on packaging."

**Illustrative unit directives (§12–14, explicitly "Do not copy this blindly into production," line 361):** `Description=`, `After=network-online.target`, `Wants=network-online.target`, `ExecStart=...`, `Restart=on-failure`, `WantedBy=multi-user.target`, `User=remote-gateway`, `Group=remote-gateway`, `Restart=always` (emergency). Restart-tuning directive **names only, no values** (§23): `RestartSec`, `StartLimitIntervalSec`, `StartLimitBurst`.

**Sandbox/hardening directives:** general set (§46) — `ProtectSystem`, `ProtectHome`, `PrivateTmp`, `PrivateDevices`, `NoNewPrivileges`, `RestrictSUIDSGID`, `LockPersonality`, `RestrictNamespaces`, `ProtectKernelTunables`, `ProtectKernelModules`, `ProtectControlGroups`; `NoNewPrivileges=true` reiterated (§48). Emergency-daemon set **with explicit values** (§97, lines 1651–1672): `NoNewPrivileges=true`, `PrivateTmp=true`, `ProtectSystem=strict`, `ProtectHome=true`, `RestrictSUIDSGID=true`, `LockPersonality=true`, `ProtectKernelModules=true`, `ProtectKernelTunables=true`, `ProtectControlGroups=true`; plus conditional `PrivateNetwork=true` (§99, line 1687).

**Capabilities:** only **one** `CAP_*` is ever named — `CAP_SYS_ADMIN` (§40, lines 861–875, "No `CAP_SYS_ADMIN` by Default"). §39 gives a generic per-capability documentation policy (why/which process/which code path/removable?/effect if removed) without naming any other candidate.

**Directories/paths:** `/tmp` (must not hold auth material, §52), `/run` and `/run/user/<uid>` (§53, generic locations, no concrete socket filenames given), `/dev/input/*`, `/dev/uinput` (explicitly forbidden to `remote-gateway`, §41), `/dev/input/event*` (generic hotplug/allowlisting caution, §44/§88 — never a literal socket path anywhere).

**Service/user accounts:** `remote-gateway` (process + suggested `User=`/`Group=`, §13); example host-daemon account **"remote-host"** (§34, line 762; reused at §112 line 1944 as `root:remote-host`) — note this is **not** the same string as the daemon `remote-hostd`. No concrete UID numbers anywhere, only the `<uid>` placeholder.

**IPC operation names** — Gateway→Host (§7, lines 227–252): `CreateAuthenticationSession`, `Authenticate`, `CreateRemoteSession`, `SignalConnection`, `RequestControl`, `ReleaseControl`, `DisconnectSession`. Host→GNOME-agent (§10, lines 293–315): `GetSessionState`, `GetDisplayState`, `CreateVirtualMonitor`, `DestroyVirtualMonitor`, `StartCapture`, `StopCapture`, `IsolatePhysicalDisplays`, `RestorePhysicalDisplays`, `EnableRemoteInput`, `DisableRemoteInput`, `LockSession`, `GetRecoveryState`. Privileged-helper allowlist (§117–118): `ISOLATE_INPUT`, `RESTORE_INPUT`, `RESTORE_DISPLAY`, `LOCK_SESSION`, `EMERGENCY_REVOKE` (example request shown literally as `operation = RESTORE_DISPLAY`).

**Emergency logging event names (§94):** `EMERGENCY_TRIGGERED`, `EMERGENCY_RECOVERY_STARTED`, `EMERGENCY_RECOVERY_COMPLETED`, `EMERGENCY_RECOVERY_PARTIAL`.

**State-like identifiers used here (loosely, not formally defined in this doc):** `REMOTE_ACTIVE` (§136–144), **`FAIL_SAFE`** (§103, line 1759 — note spelling, see §10 below), persisted flag `recovery_required=true` (§104–105).

**Recovery escalation levels (§102, informal):** "Level 1" remote-hostd recovery, "Level 2" direct GNOME/session recovery, "Level 3" local display/input recovery, "Level 4" remain locked and deny remote control.

**Protocol/version fields (§152–154):** `protocol_version`, `message_type`, `request_id`; illustrative values `hostd protocol = 3`, `agent protocol = 2`, `state_version: 1`.

**Static-analysis tools named (§130, lines 2241–2243):** `cargo clippy`, `cargo audit`, `cargo deny`, plus "appropriate C/C++ tooling if any native helpers are used."

**Checklists:** Hard Security Requirements (§159, 20 unlabeled items) and Completion Criteria (§163, 20 unlabeled items) — two distinct but overlapping checklists (see §10).

**Confirmed absent from this document** (verified via full read + targeted grep for `polkit|PolicyKit|org.freedesktop|org.gnome`, zero hits): polkit action IDs, PolicyKit, and any literal D-Bus bus/service/interface name — despite §56–60 discussing "D-Bus policy" at length. No literal socket filenames anywhere.

#### 3. Hard requirements, gates and stop conditions
- Never run the whole stack as root (§2); "the most exposed process should have the least privilege" (§5). Gateway compromise must **not** directly yield root, `/dev/input` access, GNOME control, PAM access, TOTP secret, Remote Access Key, or arbitrary system commands (§5, §123).
- **No generic RPC** (`call(method, arbitrary_arguments)`) as host IPC — only explicit named operations (§7). **No D-Bus proxy** / no arbitrary `call(service, object, method, arguments)` tunnel hostd→agent (§9).
- Watchdog health **≠** security proof — disabled-input/isolated-output states must be independently verified (§21). Must **not** auto-restore stale remote control after `remote-hostd` restart until security/epoch/GNOME/lease state is reconciled (§22, §25 "Startup Must Fail Closed").
- GNOME agent must **not** gain privileges beyond the normal user account merely because it controls GNOME (§33). Never give the daemon unrestricted root solely because PAM exists — isolate via helper if practical (§36–38); the PAM helper must **never** accept arbitrary PAM service/module/config from remote clients (§38).
- Do not add `CAP_*` preemptively — document why/process/code-path/removability/effect-if-removed for each (§39); no `CAP_SYS_ADMIN` "merely because it makes implementation easier" (§40).
- **Never** give `remote-gateway` access to `/dev/input/*` or `/dev/uinput` (§41). A privileged input helper must **not** interpret arbitrary commands, execute programs, expose raw input to the network, or provide generic `/dev/input` access (§43); must not blindly operate on `/dev/input/event*` without hotplug/seat semantics (§44); must not auto-grant itself access to future/hotplugged devices (§45).
- Do not place auth material in `/tmp` (§52); no world-writable sockets (§53); Unix socket perms must be restrictive (§54). No broad D-Bus `own`/`send` permissions (§56); user-session D-Bus must **not** be exposed to network-facing components (§57); gateway should have **no D-Bus access at all** (§58, diagrammed `remote-gateway | X | D-Bus`).
- Do not bind admin IPC sockets to `0.0.0.0` unless explicitly required (§62); security-sensitive local IPC must stay on Unix domain sockets, never TCP "merely because TCP is convenient" (§63).
- **Never** use env vars for long-lived secrets (`REMOTE_ACCESS_KEY`, `TOTP_SECRET`, `SESSION_TOKEN`, §65); never `Environment=` secrets in unit files (§66); never pass password/TOTP via command-line args (process-inspectable, §67).
- No user-controlled executable paths for security ops in config (no `emergency_command:`/`restore_command:` to a shell, §114); no `system()`/`popen()`/`sh -c`/`bash -c` for network- or config-derived operations (§115); systemd control must not let the browser specify arbitrary `unit_name`/`action` (§116).
- IPC must authenticate peer identity (UID/GID/PID/service identity), never trust a client-supplied identity field (§119–120); must authorize per-operation even post-authentication — gateway MAY request session creation but MAY NOT request emergency takeover (§121–122); emergency functions must never be exposed to the browser (§122).
- Emergency binary should avoid linking WebRTC/browser framework/video codecs/large HTTP stack "unless absolutely necessary" (§129); code must stay "small, boring, deterministic" (§126–127); must never keylog — log only defined `EMERGENCY_*` events, never raw input (§93–96); must still perform whatever local safety action it can even if IPC to `remote-hostd` fails (§101).
- `REMOTE_ACTIVE` must **not** survive an incompatible critical-component upgrade — terminate+restore+lock first (§150); rollback must return to safe local state, never preserve remote control across an uncertain version transition (§151). Security epoch must **never decrease** across migration; if uncertain, increment and invalidate (§155).
- Input/display isolation failure ⇒ `REMOTE_ACTIVE = FORBIDDEN` outright, "the system must not try anyway" (§142–143); restoration failure must never fall back to unrestricted control (§144). Never weaken sandboxing permanently just to pass tests after a regression (§145).
- Installation must not overwrite unrelated system config, replace GNOME config unexpectedly, disable firewall, broadly modify PAM, or silently enable remote access (§147).
- Do not declare the architecture complete merely because units start (§163) — real acceptance = `REMOTE FAILURE → REMOTE AUTHORITY REVOKED → LOCAL CONSOLE RECOVERED → SYSTEM LOCKED`.

#### 4. Technology and library choices
No language is mandated in prose, but `cargo clippy`/`cargo audit`/`cargo deny` (§130) are the only concretely named tools, strongly implying Rust for privileged components — hedged with "appropriate C/C++ tooling if any native helpers are used." Unlike Document 07, **no Python (or any) pseudocode appears** in this document — only prose plus illustrative INI-style systemd unit fragments (§12–14), all explicitly flagged "conceptual"/"Do not copy this blindly into production." `systemd --user` is mentioned as the mechanism for the agent (§28, prose only, not a literal invocation). No specific crate, PAM binding, or D-Bus library name is given anywhere.

#### 5. State machine detail
Not this document's focus (owned by Document 07). It uses `REMOTE_ACTIVE` repeatedly in failure-injection tests (§136–144), introduces its own persisted flag `recovery_required=true` (§104–105) with no counterpart in Document 07's persisted-state list, and names **`FAIL_SAFE`** as "a first-class state" (§103) — conflicting with Document 07's `FAILED_SAFE` spelling used throughout. No transition table, event vocabulary, or lease schema is defined here.

#### 6. Privilege model detail (by component)
**`remote-gateway`** — Domain A, UNPRIVILEGED, Internet-facing (§3–5); suggested `User=remote-gateway`/`Group=remote-gateway` (§13); **zero D-Bus access** (§58); "Minimal" secrets only (§107–108); no input-device access (§41, §107); filesystem limited to app files/static assets/gateway config/TLS material/temp data, no home access (§50); especially strong resource limits — connections/requests/messages/memory (§72); restart policy `Restart=on-failure` (§13); crash behavior: leases eventually expire → input revoked → safe recovery (§138, §158).

**`remote-hostd`** — Domain B, SYSTEM SERVICE (§3–4); dedicated service account suggested, example `remote-host` (§34); should avoid root unless a specific op demonstrably requires it (§34–35); may hold TOTP secret/Access Key verifier/trusted-client metadata/host identity/security epoch (§108); PAM access ideally isolated behind `pam-auth-helper` (minimal lifetime/privilege/IPC, no network, no GNOME access, §37–38); filesystem limited to own config/state dir/required auth interfaces/required IPC sockets, no broad home read (§49); systemd watchdog expected (§20–21); `Restart=on-failure` + `RestartSec`/`StartLimitIntervalSec`/`StartLimitBurst` to prevent restart storms (§23, §79); startup must reconcile persisted state before accepting control (§24–26); D-Bus access "may need carefully scoped system services… avoid unrestricted D-Bus access" (§59); soft dependency chain `network-online → remote-hostd → gnome-session-agent`, explicitly **not** hard-required (§27); called the highest-impact component, requiring "aggressive input validation, dependency minimization, restricted privileges, security-focused testing, fuzzing" (§125).

**`gnome-session-agent`** — Domain C, USER SESSION (§3–4); must run as the target user, never root (§3, §33); started via the user's `systemd --user` manager with GNOME/session dependencies for lifecycle/restart/resource integration (§28, §32); must wait/retry rather than fail permanently if started before GNOME is ready (§29); on logout must shut down and invalidate remote sessions (§30); must distinguish "active" vs "unlocked" sub-states (§31); filesystem limited to own app config, necessary user runtime resources, GNOME/Mutter/PipeWire interfaces (§51); must **not** receive memory limits that break PipeWire/media capture/Mutter comms (§74); D-Bus limited to expected GNOME/Mutter/session services (§60); crash behavior: remote state invalid → input disabled → safe recovery (§137, §158).

**`remote-emergencyd`** — Domain D, MINIMAL PRIVILEGE (§3–4); **no network-facing functionality**, prefer `PrivateNetwork=true` if compatible (§99); minimal privilege only if any is required at all (§107); must not depend on gateway/Internet/WebRTC/PipeWire/browser for its trigger path (§14–15); independence is scoped narrowly: "the emergency trigger must not depend on the normal remote-control/network/browser path," but it **may** coordinate via IPC with `remote-hostd` if alive, falling back to "direct minimal recovery mechanism" if not (§17–19); if `PrivateDevices=true` blocks required input detection, narrow the exception rather than disabling sandboxing globally (§98); full aggressive hardening suite listed with explicit `=true`/`=strict` values (§97); code should be tiny/boring/deterministic, ideally a separately compiled binary excluding WebRTC/browser framework/video codecs/large HTTP stack (§126–129); startup timing must be "early enough to be available when the graphical session becomes usable, but not so early that it depends on GNOME components that do not yet exist" — exact ordering "must be tested" (§87); logs only defined `EMERGENCY_*` events, never raw input (§94–96); must still act locally even if IPC to `remote-hostd` fails (§101).

**media/encoder** (§75–78, §107) — unprivileged; isolated as a separate failure/privilege domain if it exists as its own process; must not have root/input-device/PAM/security-database access (§77); rationale: codecs/encoders are complex attack surfaces (§76).

**Startup ordering/dependencies:** only `After=network-online.target`/`Wants=network-online.target` (§12) and a **soft** chain `remote-hostd → gnome-session-agent` (§27, explicitly not hard-coupled) are given. No `Requires=`, `PartOf=`, or `BindsTo=` directive is used anywhere in the document.

**Restart/watchdog policy:** `Restart=on-failure` (hostd/gateway, §12–13), `Restart=always` (emergencyd, §14); systemd watchdog notify pattern for `remote-hostd` (§20), with an explicit caveat that watchdog health doesn't prove security state (§21); rate-limiting directive names given, no values (§23).

**Resource limits:** qualitative only — gateway "especially strong" (§72), hostd "conservative… but enough" (§73), GNOME agent must not be starved (§74). No concrete numbers anywhere.

**Forbidden actions:** summarized in §3 above (arbitrary shell/D-Bus/exec, secrets in env/argv, broad D-Bus own/send, TCP for local IPC, root-by-default, `/dev/input` for gateway, keylogging, restoring `REMOTE_ACTIVE` after upgrade/isolation failure).

**User-session startup mechanism:** asserted as `systemd --user` unit at `~/.config/systemd/user/gnome-session-agent.service` (§11, §28, §32) — GNOME autostart `.desktop` entries and D-Bus activation are never mentioned or ruled out as alternatives.

**Emergencyd input access:** mechanism undecided — only says "the implementation must determine the safest way to observe the configured physical emergency shortcut," evaluated against "Wayland, GNOME, libinput, seat ownership, hotplug, lock screen, session crashes" (§88).

**PAM helper design:** architecture only (`remote-hostd → pam-auth-helper → PAM`, minimal lifetime/privilege/IPC, no network, no GNOME access, fixed expected PAM service/config) — no concrete PAM module names or code.

**Polkit usage:** **none** — never mentioned despite being the standard Linux privilege-crossing mechanism on exactly this stack (§116 recommends "a narrow D-Bus interface" for systemd control without naming an authorization layer for it).

#### 7. Tests / verification procedures specified
No numeric test-case IDs; identified by section number/title only: §132 Service Security Tests (gateway can't reach secrets/privileged sockets; agent can't reach host secrets; emergencyd has no network); §133 Permission Tests (config/state/socket perms, service users, device perms, sandbox settings); §134 Service Startup Test (post-install reboot, all 4 services start, agent starts with session); §135 Reboot Safety Test (start session → reboot → old session/lease invalid, outputs/input normal, new auth required); §136 Main Daemon Crash Test (`kill -9 remote-hostd` → eventual revoke + safe recovery); §137 GNOME Agent Crash Test (`kill -9 gnome-session-agent` → revoke + recovery); §138 Gateway Crash Test (`kill -9 remote-gateway` → eventual lease loss → revoke + recovery); §139 Emergency Test (all healthy, trigger → revoke, epoch++, lock, display/input restored); §140 Emergency During Main Daemon Failure (kill hostd then trigger emergency → path stays operational — **marked mandatory**); §141 Emergency During GNOME Agent Failure (kill agent then trigger → control already unavailable, emergencyd responsive, physical recovery where possible, document any op requiring a healthy agent); §142 Input Isolation Failure (→ `REMOTE_ACTIVE` forbidden); §143 Display Isolation Failure (→ `REMOTE_ACTIVE` forbidden); §144 Restoration Failure (→ input stays disabled, session stays locked, recovery retries); §145 systemd Sandbox Regression (every sandbox change reruns the integration suite); §130–131 Static Analysis + Fuzzing Privileged IPC (hostd/agent/emergency, malformed requests).

#### 8. Phase / sequencing guidance and definition-of-done
§160 "Final Copilot Agent Instruction" — a 10-step pre-implementation research order: inspect repo → inspect `adaptive-workflow-configurator` config → read prior architecture/security/GNOME docs → inspect actual Ubuntu 26.04 systemd behavior → inspect actual PAM requirements → inspect GNOME user-session lifecycle → inspect PipeWire/libei runtime permissions → determine which ops truly need elevated privilege → minimize privileges before writing unit files → document every privilege exception. Explicit: "Do not start by writing permissive root systemd units and hardening them later… Start from `NO PRIVILEGE` and add only what is demonstrably required." No ordered build-phase list comparable to Document 07's §59 exists here — sequencing guidance concerns the privilege-minimization *process*, not a build order. Definition-of-done = §163 Completion Criteria (20 items) + §159 Hard Security Requirements (20 items, overlapping but distinct) + the closing chain `REMOTE FAILURE → REMOTE AUTHORITY REVOKED → LOCAL CONSOLE RECOVERED → SYSTEM LOCKED`.

#### 9. Cross-references
§160 explicitly directs reading "previous architecture/security/GNOME documents" (auth/PAM/TOTP/Access Key ≈ Document 03; GNOME session/Mutter/PipeWire/libei ≈ Document 05) and inspecting the `adaptive-workflow-configurator`-generated repo configuration — which matches this workspace's actual `AGENTS.md`/`WORKFLOW_CONFIG.md`. Testing sections overlap Document 12; packaging/upgrade/uninstall (§146–155) overlap Document 15. Uses `REMOTE_ACTIVE`/`FAIL_SAFE` concepts formally owned by Document 07.

#### 10. Ambiguities, gaps, internal contradictions, and conflicts with Document 00
- **`FAIL_SAFE` (doc06, line 1759) vs `FAILED_SAFE` (doc07, used throughout, e.g. line 95, §5.11 line 338, transition-table lines 595/606/607/610)** — direct spelling contradiction for the same state between the two documents in this batch.
- **Service-account naming mismatch:** §34 (line 762) suggests example account name `remote-host` for the daemon whose unit/process is `remote-hostd` everywhere else — repeated again at §112 (line 1944, `root:remote-host`) — never reconciled.
- **Unreconciled persistence schema:** `recovery_required=true` (§104–105, lines 1777–1810) is introduced only here; Document 07's own persisted-state list (§45: host identity, authentication configuration, TOTP configuration, Remote Access Key verifier, trusted-device records, security configuration, policy) has no matching boolean flag.
- **No `PartOf=`/`BindsTo=`** directives appear anywhere despite being exactly the kind of directive this topic implies for tightly-coupled units; only `After=`/`Wants=`/`WantedBy=` are shown (§12–14), and §27 explicitly argues against hard-coupling — plausibly deliberate, but never stated as a decision.
- **User-session startup mechanism underspecified:** only `systemd --user` is asserted (§11, §28, §32); GNOME autostart `.desktop` and D-Bus activation (the two natural alternatives) are never discussed or ruled out.
- **Polkit gap:** never mentioned despite being the standard mechanism for exactly the "narrow, authorized, privilege-crossing operation" this document repeatedly calls for (§116, §121–122) — and PolicyKit1 is confirmed present on the actual target system per prior environment probing, so this isn't hypothetical.
- **No concrete socket paths, D-Bus names, or capability list beyond `CAP_SYS_ADMIN`** — nearly all of the "named identifiers" the topic implies are deferred to implementation, consistent with the doc's own "do not copy blindly" caveats, but meaning most of the requested identifier catalogue simply doesn't exist as fixed strings here.
- **Two overlapping but non-identical checklists:** §159 (Hard Security Requirements, 20 items) and §163 (Completion Criteria, 20 items) cover similar ground with some unique items each (e.g. §163 adds "Privileged helper code has been separately reviewed") and are never cross-referenced or reconciled.
- **Loose coupling to Document 07's formal vocabulary:** §158 "Critical Failure Paths" describes flows in arrow-diagram prose ("hostd crash → leases invalidated/expire → remote input disabled → safe recovery") without citing `TEARING_DOWN`/`RECOVERING` by their Document 07 names, even though it clearly means those states.

#### 11. Risks and feasibility concerns the doc itself raises
- §17: explicitly concedes "some recovery operations may inherently require GNOME/session APIs… independence does not mean it must reimplement every GNOME subsystem itself" — true emergency independence is only partial by the doc's own admission.
- §19: calls for an explicit per-operation design-review matrix ("can emergencyd perform directly? / requires GNOME agent? / requires remote-hostd?") — flagged as **not yet done**, only prescribed.
- §47: warns GNOME/PipeWire/PAM "may legitimately require access that aggressive sandboxing blocks" — implies real trial-and-error sandbox loosening during implementation.
- §88–89: the safest way to observe the emergency shortcut under Wayland/libinput/seat-ownership/hotplug/lock-screen/crash conditions is undetermined and "must be evaluated" — an open feasibility question, not a solved design.
- §98: `PrivateDevices=true` may conflict with required emergency input detection, with only a resolution *process* (isolate/narrow/document/test), not a concrete answer.
- §141: explicitly requires documenting "any GNOME operation that cannot be performed without a healthy session agent" — an acknowledged possible hard limit on emergency recovery.
- §145: sandboxing changes are flagged as a recurring regression risk requiring full integration reruns.
- §150–151: an in-place upgrade of "critical components" could silently change GNOME-integration behavior under an active remote session if not explicitly terminated first — named as an upgrade-time risk.

---

### Cross-document notes for this batch

**Contradictions between the two docs (with line refs):**
1. State-name spelling: `FAIL_SAFE` ([06 — Systemd Services & Privilege Model.md:1759](docs/plans/Detailed_Project_Plan/06%20—%20Systemd%20Services%20&%20Privilege%20Model.md)) vs `FAILED_SAFE` ([07 — Remote Session State Machine.md:95](docs/plans/Detailed_Project_Plan/07%20—%20Remote%20Session%20State%20Machine.md), and consistently thereafter, e.g. lines 338, 595, 606, 607, 610).
2. Invariant scheme: Document 00's `INV-001`…`INV-014` (14 items) vs doc07 §7's unlabeled "Invariant 1"–"Invariant 10" (10 items, lines 452–563) — different count, no `INV-` prefix, partially different content grouping.
3. Activation transaction size: Document 00's "15-step activation transaction" vs doc07's actual 22-step `BEGIN_REMOTE_ACTIVATION` (lines 614–655).
4. Disconnect transaction size: Document 00's "10-step disconnect transaction" has no single unambiguous match in doc07 — three candidates of different length/purpose (10-step rollback, lines 657–703; 6-item disconnect prose, lines 704–738; 9-item `TEARING_DOWN` order, §5.8).
5. Service-account naming: example account `remote-host` (doc06 lines 762, 1944) vs daemon/unit name `remote-hostd` (doc06 line 338 and throughout doc07) — a one-letter mismatch in shared vocabulary.
6. Persistence schema divergence: doc06's `recovery_required=true` (lines 1777–1810) has no counterpart in doc07's persisted-state list (§45).
7. Loose/informal cross-linking: doc06 uses `REMOTE_ACTIVE`/`FAIL_SAFE` conversationally (e.g. §158) without importing doc07's formal state list or transition IDs — the two documents share vocabulary but not a single formal schema.

**Five most decision-relevant insights for building the state-machine core and the privilege/service skeleton:**
1. Build the state machine as a pure, GNOME-agnostic core (11 states, typed events — not the overloaded `success`/`failure` strings) behind a swappable adapter interface, per doc07 §58 — this directly enables mock-based testing before any real GNOME work.
2. Reconcile the conflicting activation/rollback/disconnect step lists (22 vs 10 vs 6 vs 9 steps) into **one** canonical, versioned transaction spec before coding — otherwise the implementation will silently pick one and diverge from the "spec."
3. Adopt the four-domain privilege split literally, with its non-negotiables: gateway gets zero D-Bus/secrets/input-device access; emergencyd gets zero network and the smallest possible dependency graph/binary; only `remote-hostd` holds auth secrets, ideally behind a narrow `pam-auth-helper`.
4. The security epoch is the single cheapest cross-cutting invalidation mechanism tying both documents together — implement it first (monotonic, persisted, never-decreasing across migration, incremented on emergency), since nearly every fail-safe path (lease validity, reconnect rejection, stale-session rejection, upgrade safety) reduces to an epoch comparison.
5. Resolve two spec-silent decisions before writing systemd units: (a) the exact GNOME-agent startup mechanism (doc asserts `systemd --user` but never rules out autostart/D-Bus-activation) and (b) whether/how polkit fits the privilege-crossing operations, since it's the platform-standard mechanism but is never mentioned in Document 06.

**Implementable now with mocked adapters (pure logic, no GNOME needed):**
- Full state-machine engine, transition table, typed events, `StateMachineLock`/concurrency-priority resolver (doc07 §4–8, §24–26).
- Control lease model + security epoch logic (doc07 §13–16).
- Idempotent-operation wrappers as no-op-safe stubs (doc07 §27).
- Startup-recovery reconciliation logic against a mocked persisted-state store (doc07 §28–30; doc06 §24–26).
- Emergency control-flow (transition→revoke→invalidate→terminate→lock→restore→verify ordering, nested try/finally) against mocked GNOME/display/input adapters (doc07 §49).
- IPC message schemas/operation enums and their authorization matrix — `CreateAuthenticationSession`…`DisconnectSession`; `GetSessionState`…`GetRecoveryState`; `ISOLATE_INPUT`…`EMERGENCY_REVOKE` — plus "gateway may/may-not" rules (doc06 §7, §10, §118, §121).
- Unit tests for every legal/illegal transition, lease expiry, epoch mismatch, concurrent-event priority ordering (doc07 §53–54).
- Static config/permission/secrets-isolation checks (file modes, ownership, absence of secrets in env/argv) — doc06 §132–133's non-runtime checks.

**Requires real GNOME/Mutter/PipeWire/hardware:**
- Virtual monitor creation/destruction and physical-output isolation/restoration (doc07 §5.5, §31–32; doc06 §10).
- PipeWire capture lifecycle and its degraded/failure semantics (doc07 §5.7, §21, Failure Matrix).
- libei/EIS physical-input isolation/restoration and device-identity stability across hotplug/reboot/dock (doc07 §33, §44–45; doc06 §41–45).
- GNOME session lock/unlock verification and "active vs unlocked" sub-state discrimination (doc07 §5.2; doc06 §31).
- Emergency shortcut detection validation under Wayland/libinput/seat-ownership/lock-screen/session-crash conditions (doc06 §88–89, §98).
- Reboot/power-loss/suspend-resume end-to-end tests (doc07 §29–30; doc06 §135).
- Real systemd sandbox-directive compatibility testing (`ProtectHome`, `PrivateDevices`, etc. against actual PAM/PipeWire/GNOME requirements) (doc06 §47, §97–98, §145).
- Live kill-9/crash-injection tests against real services (doc06 §136–144).