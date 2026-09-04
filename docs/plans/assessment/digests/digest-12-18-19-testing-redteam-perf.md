# Research Findings: Documents 12, 18, 19 (Blackroom Console Specification Set)

---

### 12 — TESTING STRATEGY & TEST MATRIX

**1. Purpose & scope**
Defines the overall testing strategy and layered test architecture for the whole project, establishing the governing principle that "a feature is not complete because the happy path works... it is complete only when expected failures, races, restarts, disconnects, and recovery paths preserve the project's security invariants" ([12 — TESTING STRATEGY & TEST MATRIX.md](docs/plans/Detailed_Project_Plan/12%20—%20TESTING%20STRATEGY%20&%20TEST%20MATRIX.md), lines 17-19). It covers unit through hardware/adversarial layers, authentication/session/lease/epoch testing, physical display/input isolation testing, emergency-controller testing, race/cycle/soak testing, CI tiering, severity definitions, and a final release-gate scenario. It explicitly rejects code-coverage-as-a-goal (line 25).

**2. Named identifiers defined**

*Test layers/pyramid* (§3): UNIT TESTS → INTEGRATION TESTS / CONCURRENCY-RACE TESTS → SYSTEM TESTS / FAULT INJECTION → HARDWARE/GPU / SECURITY TESTS → RELEASE/ACCEPTANCE.

*Severity scale* (§49): `P0 — Critical`, `P1 — High`, `P2 — Medium`, `P3 — Low` (lines 1668, 1687, 1702, 1715). "P0 blocks all development progression until resolved." (line 1683).

*CI tiers* (§51): `Tier 1 — Fast CI`, `Tier 2 — Integration CI`, `Tier 3 — Real GNOME System Tests`, `Tier 4 — Hardware Matrix`.

*Cycle counts* (§26, lines 1047-1049): 10 cycles (development), 50 cycles (release candidate), 100+ cycles (representative hardware).

*Soak durations* (§27, lines 1073-1076): 30 minutes, 2 hours, 8 hours, overnight "where practical."

*Resolutions* (§30): 1280×720, 1920×1080, 2560×1440, 3840×2160. *Refresh rates*: 60 Hz, 120 Hz, 144 Hz, "higher refresh rates available on hardware."

*GPU vendors* (§28, §51): Intel (integrated), AMD (integrated/discrete), NVIDIA (proprietary). *Display connectors* (§29): HDMI, DisplayPort, USB-C.

*Components* (§19, §53): `remote-gateway`, `remote-hostd`, `gnome-session-agent`, `remote-emergencyd`.

*States* (§5, matches Doc 00 exactly): `LOCAL_ACTIVE`, `LOCAL_LOCKED`, `AUTHENTICATING`, `AUTHENTICATED`, `PREPARING_REMOTE`, `REMOTE_ACTIVE`, `REMOTE_DEGRADED`, `TEARING_DOWN`, `RECOVERING`, `EMERGENCY`, `FAILED_SAFE`.

*Fault-injection event vocabulary* (§53): `NETWORK_LOST`, `NETWORK_RESTORED`, `LEASE_EXPIRED`, `LEASE_REVOKED`, `SECURITY_EPOCH_CHANGED`, `HOSTD_CRASH`, `GATEWAY_CRASH`, `GNOME_AGENT_CRASH`, `PIPEWIRE_FAILURE`, `MUTTER_FAILURE`, `DISPLAY_RESTORE_FAILURE`, `INPUT_RESTORE_FAILURE`, `EMERGENCY_TRIGGER`, `SESSION_LOGOUT`, `SYSTEM_SUSPEND`.

*Property-based test event vocabulary* (§54): `CONNECT`, `DISCONNECT`, `LEASE_EXPIRE`, `RECONNECT`, `EMERGENCY`, `NETWORK_LOSS`, `NETWORK_RESTORE`, `HOSTD_RESTART`, `GNOME_FAILURE`, `DISPLAY_FAILURE`, `INPUT_FAILURE`.

*Acceptance-matrix gate labels* (§47 table): `BLOCKER`, `RELEASE`, `OPTIONAL`, `FUTURE` — a **fourth, distinct** gating vocabulary from P0-P3 and from Gate A-H.

*Evidence label pair* (§44): `AUTOMATED PASS` vs. `PHYSICAL/HARDWARE VERIFIED` — only a 2-way split (contrast with Doc 18's 6-way vocabulary, §10 below).

*Test Result Format fields* (§46): `Test ID`, `Test Name`, `Environment` (Ubuntu/GNOME/Kernel/GPU/Driver/Monitor), `Initial State`, `Action`, `Expected Result`, `Actual Result`, `Final State`, `Pass/Fail`, `Evidence`, `Logs`, `Known Limitations`, `Follow-up`. No test-ID naming *scheme* (no prefix convention like `RT-`) is ever defined — a placeholder field only.

*No tool names anywhere* — confirmed via grep: zero hits for `cargo`, `pytest`, `ruff`, `pyright`, `valgrind`, `perf`, `AFL`, `libFuzzer`, `criterion`, `proptest`.

**3. Hard requirements, gates and stop conditions**
- §47 Critical Acceptance Matrix: 20 rows marked `BLOCKER` (same GNOME session, virtual monitor, physical display isolation, physical input isolation, remote input, session lock, network-loss recovery, main daemon crash, emergency takeover, security epoch, control lease, authentication, new-device Access Key, trusted-device revocation, privilege separation, secret handling, display restoration, input restoration, repeated cycles, supported GPU matrix, browser security); 3 `RELEASE` (NAT traversal, LAN discovery, UX polish); 2 `OPTIONAL` (clipboard, audio); 1 `FUTURE` (file transfer).
- §48 "A test is PASS only when" 8 numbered MUST conditions, plus an explicit **anti-pattern list**: NOT pass merely because "the remote screen appeared / connection succeeded once / browser displayed video / logs looked normal / process did not crash / a mock returned the expected value."
- §59 Final Release-Gate Test: 26-step end-to-end scenario; "release blocker if any critical safety invariant fails."
- §61 Copilot Agent Stop Rules: 11 stop triggers (e.g., "physical input isolation cannot be verified," "a test passes only because of a workaround that changes the security model") — MUST stop/report, never hide failure or weaken a test to make CI green.
- §52 Test Isolation: destructive system tests (disabling physical displays, suppressing input, locking desktop, modifying privileged services) MUST NOT be part of ordinary dev commands unless explicitly requested.

**4. Technology and library choices**
No framework is mandated by name anywhere in this document (confirmed by grep). §51 Tier 1 speaks only generically of "lint / formatting / type checks where applicable / unit tests." §54 property-based/model testing is only "where practical" (suggested, not mandated). §60 explicitly instructs Copilot to **inspect the repository's existing `adaptive-workflow-configurator` output and respect it rather than prescribing a directory layout** — i.e., this document deliberately defers all tooling/layout decisions to the locally configured workflow (directly consistent with this repo's own [AGENTS.md](AGENTS.md) / [docs/WORKFLOW_CONFIG.md](docs/WORKFLOW_CONFIG.md) governance model). This means the repo's Python `pytest -x -q` / `ruff check .` / `pyright` commands are neither mandated nor contradicted by Doc 12 — it is silent on language/tooling.

**5. Test matrix detail**

*Testing layers × what they validate × environment need (synthesized from §3-§14, §51, since no single literal table exists in the source):*

| Layer | Representative targets | Needs GNOME/hardware? |
|---|---|---|
| Unit | state machine, lease/epoch/credential validation, rate limiting, replay protection, config parsing, protocol/IPC validation (§4) | No — explicitly must NOT require GNOME/monitor/PipeWire/Mutter/`/dev/input`/Internet/browser |
| Integration | IPC, service interaction, mocked GNOME interfaces, WebRTC signalling (§51 Tier 2) | No (mocked) |
| System | session discovery, Mutter, virtual monitor, PipeWire, input, display isolation, lock, teardown, recovery (§11, §51 Tier 3) | Yes — supported Ubuntu/GNOME test machine |
| Hardware/GPU | Intel/AMD/NVIDIA, multi-monitor, high-res/high-refresh (§28-31, §51 Tier 4) | Yes — GPU matrix |
| Security/adversarial | brute force, IPC authorization, privilege boundaries, secret handling (§36-41, §57) | Mixed |
| Race/concurrency | disconnect+emergency, reconnect+emergency, lease-expiry+input, teardown+GNOME-restart (§25) | Mixed (state-machine level = no hardware; real races = hardware) |

*Authentication Flow Matrix (§7, reproduced verbatim):*

| Scenario | Password | TOTP | Remote Access Key | Trusted Credential | Expected |
|---|---:|---:|---:|---:|---|
| New device | Yes | Yes | Yes | No | Allow |
| Trusted device | Yes | Yes | No | Yes | Allow |
| Wrong password | No | Yes | Yes | No | Deny |
| Wrong TOTP | Yes | No | Yes | No | Deny |
| New device without Access Key | Yes | Yes | No | No | Deny |
| Trusted device without TOTP | Yes | No | No | Yes | Deny |
| Revoked trusted device | Yes | Yes | No | Revoked | Deny |
| Revoked Access Key | Yes | Yes | Revoked | No | Deny |
| Lost authenticator recovery | Yes | Recovery | Yes | No | Allow if recovery policy permits |
| Revoked all sessions | Yes | Yes | Yes | Yes | Existing sessions terminated |

*Display Matrix (§29, reproduced verbatim):*

| Configuration | Required |
|---|---:|
| One 1080p monitor | Yes |
| One 1440p monitor | Yes |
| One 4K monitor | Yes |
| Multiple monitors | Yes |
| Mixed resolutions | Yes |
| Mixed refresh rates | Yes |
| HDMI | Yes |
| DisplayPort | Yes |
| USB-C display | Recommended |
| Monitor hotplug | Yes |
| Monitor power cycle | Recommended |

*Critical Acceptance Matrix (§47, reproduced verbatim, 26 rows)* — see §3 above for the full BLOCKER/RELEASE/OPTIONAL/FUTURE enumeration.

*Automated vs Manual split (§44):* Automate = state machine, authentication, authorization, leases, epoch, protocol validation, IPC, configuration, rate limiting, session lifecycle, failure injection, repeated-cycle logic, mocked GNOME, service start/stop, security regression. Manual/Hardware = physical display visibility, monitor power behavior, physical keyboard/mouse isolation, emergency shortcut, visual artifacts, GPU-specific behavior, high-refresh displays, hotplug, suspend/resume, physical privacy.

**6. CI without GNOME hardware vs requires real host**
Explicit: Tier 1 (lint/format/type-check/unit/state-machine/auth/protocol/security-regression) and Tier 2 (IPC, service integration, **mocked GNOME interfaces**, protocol integration, browser/client, WebRTC signalling) run in ordinary CI. Tier 3 (session discovery, Mutter, virtual monitor, PipeWire, input, display isolation, lock, teardown, recovery) runs "on supported Ubuntu/GNOME test machines." Tier 4 (hardware matrix) runs "before release candidates and after major GNOME/Mutter changes." §4 explicitly bars unit tests from touching a running GNOME session, physical monitor, PipeWire, Mutter, real `/dev/input`, Internet, or a browser. **Gap:** the document never specifies *how* mocks/fakes should be structured (no interface/trait/DI guidance) — it only names "mocked GNOME interfaces" as a category.

**7. Evidence and reporting format**
§45 Test Evidence: result, GNOME/Ubuntu/kernel version, GPU+driver, monitor topology, PipeWire version, component versions, before/after state, injected event, expected/actual state, logs, failure reason; screenshots/photos for hardware; never store credentials/secrets in artifacts. §46 Test Result Format template (13 fields, listed in §2 above) — a flat Pass/Fail scheme, no multi-tier verification vocabulary.

**8. Phase / sequencing guidance**
**No mapping to Phase 0-20 exists** (zero hits for "Phase \d" in this document). Instead §60 gives a generic **per-task** loop: `RESEARCH → IDENTIFY TESTABLE BEHAVIOR → WRITE/UPDATE TESTS → IMPLEMENT → RUN FAST TESTS → RUN RELEVANT INTEGRATION TESTS → RUN SYSTEM/HARDWARE TESTS WHEN REQUIRED → ANALYZE FAILURES → DOCUMENT EVIDENCE → REGRESSION TEST`. This is a workflow micro-loop, not a roadmap.

**9. Cross-references**
None. Zero occurrences of "Document N" anywhere in this file — striking given Documents 18 and 19 both cite Document 12 explicitly (asymmetric/one-directional referencing across the doc set).

**10. Ambiguities, gaps, internal contradictions, conflicts with Doc 00**
- **Cycle-count contradiction:** §26 (lines 1047-1049) mandates 10/50/100+ cycles, but Doc 19 §7/§30 (lines 273-275, 890-892) mandates 100/500/1000 cycles for what is functionally the same connect/disconnect/recover loop. The user's own Doc-00-derived context states "cycle tests 100/500/1000," meaning **Doc 12 is the outlier** conflicting with the master prompt.
- **Soak-duration mismatch:** §27 (lines 1073-1076) caps at "overnight" (~≤12h), while Doc 19 §6/§31 (lines 244-248, 904-908) explicitly runs to 48 hours — Doc 12's ceiling is roughly a quarter of Doc 19's/Doc 00's stated ladder (1h/6h/12h/24h/48h).
- **Severity vocabulary collision:** §49 (lines 1668-1721) defines `P0/P1/P2/P3`, while Doc 18 §3 (lines 141-188) defines a *different* 5-level `CRITICAL/HIGH/MEDIUM/LOW/INFORMATIONAL` scale for the same "how bad is this bug" concern — no stated equivalence mapping between the two scales exists anywhere in the doc set.
- **Refresh-rate set mismatch** vs. Doc 19 §11: Doc 12 lists 60/120/144 Hz; Doc 19 lists 60/90/120 Hz (144 Hz absent, 90 Hz absent from the other).
- No test-ID naming scheme (unlike Doc 18's rigorous `RT-CATEGORY-NNN`).
- No Phase 0-20 mapping despite Doc 11 defining exactly that roadmap.
- Silent on implementation language, leaving the Rust-vs-Python tooling question (flagged in session memory) unresolved rather than contradicted.

**11. Risks and feasibility concerns the doc itself raises**
- §16: GNOME lock semantics are explicitly flagged as unknown/risky — "must not assume... Document actual behavior on every supported GNOME release. If locking necessarily destroys the remote session, the design must explicitly account for that behavior rather than hiding the problem."
- §31: hardware-cursor GNOME behavior may require a workaround (disabling HW cursors) with unknown performance impact — flagged as "investigate."
- §34: power-loss testing is acknowledged as fundamentally hard to automate ("Power loss is not controllable in automated testing").
- §43: "Do not broaden compatibility claims until tested" — narrow platform target (Ubuntu 26.04/GNOME 50+/Wayland only) is a self-imposed constraint.
- §28: "Do not claim universal GPU compatibility from a single test machine" — directly relevant given the known dev environment has only NVIDIA+Intel hybrid graphics, no AMD, per prior session notes.

---

### 18_THREAT_DRIVEN_SECURITY_TESTING_AND_RED_TEAM_PLAN

**1. Purpose & scope**
Defines the adversarial/red-team program that deliberately tries to break 10 named highest-priority security invariants (no-auth-no-control, no-lease-no-input, stale-epoch-no-control, emergency-invalidates-authority, disconnect-revokes-input/locks/restores-display/restores-input, remote-control-never-permanent, emergency-independent-of-main-stack). It explicitly complements **Document 9** ("what can go wrong") and **Document 12** ("how the overall system is tested") by defining "how an attacker deliberately tries to make those failures happen" (lines 45-50).

**2. Named identifiers defined**

*Severity scale* (§3, lines 139-192): `CRITICAL` (blocks release), `HIGH` (blocks release unless explicitly reviewed/accepted), `MEDIUM`, `LOW`, `INFORMATIONAL`.

*Red-Team Priority tiers* (§35, distinct axis from severity): `P0` (12 items — must pass before architecture accepted), `P1` (6 items), `P2` (4 items).

*Attack IDs — 82 total, by category:*

| Category | IDs | Count |
|---|---|---:|
| Authentication | RT-AUTH-001…015 | 15 |
| Session Credential | RT-SESSION-001…005 | 5 |
| Control Lease | RT-LEASE-001…007 | 7 |
| Security Epoch | RT-EPOCH-001…004 | 4 |
| Emergency Controller | RT-EMERGENCY-001…007 | 7 |
| Physical Display | RT-DISPLAY-001…005 | 5 |
| Physical Input | RT-INPUT-001…007 | 7 |
| Privilege Escalation | RT-PRIV-001…006 | 6 |
| File-system/Secrets | RT-FILE-001…005 | 5 |
| Browser | RT-BROWSER-001…007 | 7 |
| Network | RT-NET-001…007 | 7 |

Plus unlabeled scenario groups: §18 Gateway Compromise Scenario, §19 Malicious Client Tests, §20 State-Machine Attack Testing, §21 Fault Injection During Critical Transactions, §22 Race-Condition Campaign (`Race A`…`Race F`), §23 Time/Clock Attacks, §24 Resource Exhaustion, §25 Persistence/Restart, §26 Package/Upgrade, §27 Diagnostic/Logging, §28 Physical Attacker Scenarios (`Scenario 1`…`5`, no ID prefix).

*Security Invariant IDs* (§29): `INV-SEC-001`…`INV-SEC-010`.

*Evidence/status vocabulary* (§40): `IMPLEMENTED`, `VERIFIED`, `PARTIALLY VERIFIED`, `UNVERIFIED`, `FAILED`, `BLOCKED` — matches the exact 6-term scheme cited in the user's Doc-00 context.

*Attack Result Format fields* (§33, 17 fields): `Test ID, Attack, Attacker capability, Target, Preconditions, Steps, Expected result, Actual result, Security invariant tested, Evidence, Severity, Reproducibility, Automated, Regression test, Fix, Retest result, Residual risk`.

*Environment tiers* (§34): `AUTOMATED`, `SYSTEM AUTOMATED` (dedicated Ubuntu/GNOME hardware), `MANUAL/PHYSICAL`.

*Attack surface inventory* (§6, ASCII diagram): Internet/LAN → Gateway → Authentication → Host Security Authority → {Session Credential, Control Lease, Security Epoch} → GNOME Session Agent → {Mutter, PipeWire, libei/EIS, DisplayConfig} → Physical Display/Input; Emergency Controller → {Host Security Authority, GNOME/session recovery}. Additional named surfaces: configuration files, secret storage, systemd, D-Bus, Unix sockets, browser storage, TLS, WebSocket, WebRTC signalling, STUN/TURN, rendezvous, mDNS/Avahi, package installation, upgrade process, diagnostics, logs, crash dumps, temp files.

*Test environment stack* (§4): Ubuntu 26.04 LTS, supported GNOME, Wayland, PipeWire, supported GPU, ≥1 physical display, physical keyboard/mouse, separate remote client, browser, LAN, optional Internet/NAT — "use a separate test machine rather than the developer's primary workstation."

*Race Campaign* (§22): `Race A` (Emergency vs Lease Renewal), `Race B` (Disconnect vs Remote Input), `Race C` (Reconnect vs Epoch Increment), `Race D` (Physical Hotplug vs Remote Activation), `Race E` (GNOME Failure vs Emergency), `Race F` (Main Daemon Restart vs Existing Session) — matches the user's Doc-00 context race list exactly.

No specific fuzzer/tool names anywhere (confirmed by grep: 0 hits for AFL/libFuzzer/cargo-fuzz/criterion/proptest); §30 "Fuzzing Program" lists only requirement categories.

**3. Hard requirements, gates and stop conditions**
- §35 P0 (12 items, verbatim): 1. authentication bypass 2. TOTP bypass 3. Remote Access Key bypass 4. control lease bypass 5. security epoch bypass 6. stale-session replay 7. emergency invalidation 8. physical display privacy 9. physical input isolation 10. emergency independence 11. privilege escalation 12. unsafe recovery. "Failure in any P0 test blocks the project."
- §36 Red-Team Acceptance Criteria (14 MUST bullets) including "gateway compromise does not directly imply host compromise" and "repeated adversarial testing produces deterministic outcomes."
- §37 Security Release Blockers (16 bullets, verbatim, includes): remote input possible without valid lease; remote input possible with stale epoch; emergency does not revoke existing remote authority; stale browser can reconnect after emergency; physical keyboard/mouse can control session while remote mode active; physical display exposes remote activity; emergency requires the main remote daemon; authentication/TOTP/Access-Key can be bypassed; privileged IPC allows arbitrary command execution; gateway can directly control privileged host functionality; secrets appear in logs; crash during teardown leaves remote authority active; reboot unexpectedly restores remote authority without authentication.
- §40 Copilot Stop Conditions (10 bullets) + "Do not label an untested security property as verified."
- §38: "Do not assume an implementation is secure because authentication exists / TLS exists / tests pass / a token is signed / a service runs as non-root / the browser hides a button / the UI says 'disconnected.' Security must be verified at the authoritative enforcement point."

**4. Technology and library choices**
None mandated by name. §5 Test Harness lists required *capabilities* (start/stop services, kill processes, inject/replay/delay/duplicate/reorder messages, concurrent sessions, clock manipulation, network-loss/latency/packet-loss simulation, credential revocation, config manipulation, state inspection, machine-readable results) without naming any framework. §30 Fuzzing Program is similarly capability-only. This is a notable absence given the document's entire purpose is adversarial testing — no decision is made here between, e.g., a Rust-native fuzzer (`cargo-fuzz`) and any alternative.

**5. Attack plan detail (full reproduction by category)**

| ID | Attack | Expected result | Severity (as stated) |
|---|---|---|---|
| RT-AUTH-001 | Missing Password | rejected, no session/lease/GNOME change | CRITICAL if bypassed |
| RT-AUTH-002 | Invalid Password | rejected, rate-limited | unstated |
| RT-AUTH-003 | Missing TOTP | rejected | CRITICAL if bypassed |
| RT-AUTH-004 | Invalid TOTP | rejected, no session | unstated |
| RT-AUTH-005 | Reused TOTP | follows documented TOTP replay policy | unstated |
| RT-AUTH-006 | Missing Access Key (new device) | rejected | CRITICAL if bypassed |
| RT-AUTH-007 | Invalid Access Key | rejected, rate-limited | unstated |
| RT-AUTH-008 | Trusted Device w/o TOTP | rejected | unstated (implied CRITICAL class) |
| RT-AUTH-009 | Stolen Trusted Credential | matches documented trust model; password+TOTP still mandatory | unstated |
| RT-AUTH-010 | Revoked Trusted Device | rejected | unstated |
| RT-AUTH-011 | Revoked Access Key | rejected | unstated |
| RT-AUTH-012 | Recovery Code Abuse | new-device auth still rejected w/o Access Key | unstated |
| RT-AUTH-013 | Username Enumeration | no enumeration via body/status/timing | unstated |
| RT-AUTH-014 | Authentication Flooding | rate-limited, bounded resources, no bypass | unstated |
| RT-AUTH-015 | Concurrent Authentication Race | consistent state, no confusion | unstated |
| RT-SESSION-001 | Expired Session Credential | rejected | unstated |
| RT-SESSION-002 | Wrong Host | rejected | unstated |
| RT-SESSION-003 | Wrong Client | rejected | unstated |
| RT-SESSION-004 | Session Replay (post disconnect/expiry/revoke/epoch) | rejected | unstated |
| RT-SESSION-005 | Session Credential Reuse (multi-client) | follows explicit multi-client policy | unstated |
| RT-LEASE-001 | No Lease, Send Input | all input rejected | CRITICAL if bypassed |
| RT-LEASE-002 | Expired Lease | input stops immediately | unstated |
| RT-LEASE-003 | Lease From Previous Session | rejected | unstated |
| RT-LEASE-004 | Wrong Session ID | rejected | unstated |
| RT-LEASE-005 | Wrong Security Epoch | rejected | CRITICAL if bypassed |
| RT-LEASE-006 | Lease Renewal After Revocation | rejected | unstated |
| RT-LEASE-007 | Lease Expiration Race | stale input cannot survive expiry | unstated |
| RT-EPOCH-001 | Stale Session After Epoch Increment | all prior-epoch authority rejected | unstated |
| RT-EPOCH-002 | Reconnect With Stale Credential (post-emergency) | rejected | unstated |
| RT-EPOCH-003 | Concurrent Emergency Race | emergency wins, no input survives | unstated |
| RT-EPOCH-004 | Epoch Persistence (daemon restart) | old authority stays invalid | unstated |
| RT-EMERGENCY-001 | Emergency During Remote Active | 7-step revoke→terminate→epoch++→lock→restore display→restore input→stay locked | unstated |
| RT-EMERGENCY-002 | Emergency During Preparation | safe abort, authority revoked, physical restored, locked | unstated |
| RT-EMERGENCY-003 | Emergency During Teardown | idempotent, safe final state | unstated |
| RT-EMERGENCY-004 | Main Daemon Hung + emergency shortcut | emergency works independently | **"This is a CRITICAL test."** |
| RT-EMERGENCY-005 | Gateway Dead + emergency | still works | unstated |
| RT-EMERGENCY-006 | Browser Maliciously Reconnects post-emergency | rejected | unstated |
| RT-EMERGENCY-007 | Repeated Emergency | no crash, consistent, locked/recoverable | unstated |
| RT-DISPLAY-001…005 | Verification / Re-enable attempt / Hotplug / GNOME failure during isolation / Teardown failure | physical output disabled/stays safe throughout | unstated |
| RT-INPUT-001…007 | Physical keyboard/mouse during active / rapid flood / hotplug / multi-device / teardown race / emergency shortcut must still work | physical input never controls session; emergency trigger still reaches the daemon | unstated |
| RT-PRIV-001…006 | Gateway escape / GNOME-agent escape / command injection / arbitrary D-Bus / IPC credential confusion / privileged-helper abuse | blocked by privilege separation; no shell interpretation; only supported ops possible | unstated |
| RT-FILE-001…005 | Secret file perms / symlink / path traversal / config injection / logs containing secrets | only authorized component can access; no traversal; no secrets in logs | unstated |
| RT-BROWSER-001…007 | XSS / CSRF / clickjacking / malicious WS client / malformed WS / oversized messages / refresh | no injection/bypass/crash; stale state can't regain control | unstated |
| RT-NET-001…007 | TLS downgrade / MITM / replay / reordering / delay / duplicate / connection flood | rejected/idempotent/bounded resources | unstated |

*Race Campaign (§22):*

| Race | Contest | Required outcome |
|---|---|---|
| A | Emergency vs Lease Renewal | Emergency must win |
| B | Disconnect vs Remote Input | No input may survive final revocation |
| C | Reconnect vs Epoch Increment | Stale reconnect must fail |
| D | Physical Hotplug vs Remote Activation | No privacy violation |
| E | GNOME Failure vs Emergency | Emergency must still reach a safe outcome |
| F | Main Daemon Restart vs Existing Session | Old authority must not silently survive |

*Security Invariants (§29):* `INV-SEC-001` no session→no input; `002` no lease→no input; `003` epoch mismatch→no input; `004` post-emergency→authority revoked; `005` post-failure→session locked; `006` post-failure→physical input restored; `007` post-failure→physical display safe; `008` post-emergency→old session can't reconnect; `009` post-restart→control disabled until authenticated; `010` emergency must not depend on Internet/Gateway/Browser/WebRTC/PipeWire/Main Remote Agent.

**6. CI without GNOME hardware vs requires real host**
§34 explicit 3-way split: **AUTOMATED** (protocol/auth/replay/epoch/lease attacks, malformed messages, IPC authorization, config validation, state-machine races, resource limits, regression) — no hardware. **SYSTEM AUTOMATED** (display isolation, input isolation, emergency controller, session locking, virtual monitor, recovery, GNOME/Mutter failures, hardware hotplug) — "dedicated Ubuntu/GNOME hardware." **MANUAL/PHYSICAL** (actual monitor privacy, physical keyboard/mouse isolation, emergency shortcut, monitor power/blank behavior, unusual GPU/display combos) — human-only. No mock/fake structuring guidance given (same gap as Doc 12).

**7. Evidence and reporting format**
§32 Evidence Requirements (state-transition trace, epoch, lease status, session ID, auth result, service/process status, display topology, input routing, GNOME lock state, journal entries, security audit events, network trace, browser console, resource usage, timestamps — never real secrets). §33 Attack Result Format (17 fields, §2 above). §40's 6-term IMPLEMENTED/VERIFIED/PARTIALLY VERIFIED/UNVERIFIED/FAILED/BLOCKED vocabulary is the richest classification scheme across all three documents.

**8. Phase / sequencing guidance**
No Phase 0-20 mapping (0 hits). §39 Copilot Red-Team Implementation Order gives its own 22-step sequence: security invariant assertions → state-machine adversarial tests → authentication attacks → session credential → control lease → security epoch → revocation → emergency → IPC authorization → privilege-boundary → protocol fuzzing → browser security → network attacks → fault-injection → race-condition → resource-exhaustion → physical display → physical input → hardware/GNOME failure → persistence/restart → package/upgrade → full adversarial regression suite. Explicit ordering constraint: "Do not start with large-scale fuzzing before deterministic security invariants and state-machine tests exist."

**9. Cross-references**
Explicit: **Document 9** — Threat Model, Security Boundaries & Abuse Cases ("defines what can go wrong"); **Document 12** — Testing Strategy & Test Matrix ("defines how the overall system is tested"). Document 19 also cites this document back (§46 "Reliability Testing With Document 18"), making 18↔19 the only bidirectional link found among the three assigned docs.

**10. Ambiguities, gaps, internal contradictions, conflicts with Doc 00**
- **The word "Gate" never appears in this document at all** (0 hits), despite its P0 list closely paralleling both Doc 00's *Hard Feasibility Gates* (`Gate A`…`H`: Same Session/Virtual Display/Physical Display Isolation/Remote Input/Physical Input Isolation/Fail-Safe/Emergency/Same-Session Recovery — [00 — COPILOT_AGENT_IMPLEMENTATION_MASTER_PROMPT.md](docs/plans/Detailed_Project_Plan/00%20—%20COPILOT_AGENT_IMPLEMENTATION_MASTER_PROMPT.md), lines 1819-1855) and Doc 09's *Hard Security Gates* (`Gate A`…`J`, [09 — Threat Model, Security Boundaries & Abuse Cases.md](docs/plans/Detailed_Project_Plan/09%20—%20Threat%20Model,%20Security%20Boundaries%20&%20Abuse%20Cases.md), lines 1941-1985). Mapping RT-*/P0 tests to gates requires inference not stated in this doc (see cross-doc section).
- **"P0" term overload:** here `P0` (§35, line 1920) means *execution-priority tier* (which attacks to run first), completely distinct from `CRITICAL`/`HIGH`/etc. (§3), the actual severity axis. Doc 12 instead uses `P0` to mean *severity* — the same token means two different things depending which of the two assigned documents you're reading.
- At least three RT-tests (RT-AUTH-005, RT-AUTH-009, RT-SESSION-005) defer their pass/fail criterion to an externally "documented policy/trust model" that is not defined inside this document — a structural dependency on an unnamed external spec.
- RT-PRIV-* (privilege escalation, 6 tests, explicitly P0 item #11) has no corresponding letter in Doc 00's Gate A-H at all — only Doc 09's `Gate J — No Arbitrary Privileged Execution` covers it.

**11. Risks and feasibility concerns the doc itself raises**
- §4 recommends "a separate test machine rather than the developer's primary workstation" for red-team work and states "never perform destructive red-team tests against a production workstation containing irreplaceable data" — a resourcing requirement not obviously met by a single-workstation dev setup.
- §39's explicit sequencing warning (don't fuzz before invariants/state-machine exist) is a self-identified process risk if ignored.
- §26 Package/Upgrade attacks presuppose a package-signing/upgrade mechanism whose design lives elsewhere (Doc 15), so this section's tests are only as meaningful as that undescribed mechanism.

---

### 19_PERFORMANCE_RELIABILITY_AND_RESOURCE_MANAGEMENT

**1. Purpose & scope**
Defines performance, reliability, and resource-management requirements ensuring responsiveness, predictable CPU/memory/GPU/network/disk usage, no leaks, survivability of network instability/component restarts, and that "performance is subordinate to safety" (line 31: "a slower system that safely terminates remote control is preferable to a faster system that leaves remote authority active after failure"). Explicitly complements Documents 7, 10, 12, 13, 18 (lines 23-27).

**2. Named identifiers defined**

*Numeric targets — sparse by design:* the **only** hard numeric target in the whole document is input latency, "approximately 50–100 ms end-to-end" on typical LAN (§4), immediately hedged: "The exact target should be measured on representative hardware." No numeric fps, CPU%, memory MB, fd-count, or thread-count budget is given anywhere; §41 explicitly instructs to *derive* thresholds from measured baselines rather than adopt fixed numbers ("Avoid arbitrary thresholds before baseline data exists").

*Soak durations* (§6, §31, lines 244-248/904-908): 1 hour, 6 hours, 12 hours, 24 hours, 48 hours.

*Cycle counts* (§7, §30, lines 273-275/890-892): 100 cycles, 500 cycles, 1000 cycles "where hardware/test time permits"/"where practical."

*Benchmark profile IDs* (§42): `PERF-LAN-1080P` (local LAN, 1920×1080, normal desktop activity), `PERF-LAN-4K` (local LAN, 3840×2160, representative workload), `PERF-WAN` (Internet path, realistic latency/jitter, TURN), `PERF-DEGRADED` (high latency/packet loss/limited bandwidth), `PERF-SOAK` (long-running session), `PERF-CYCLE` (repeated connect/disconnect), `PERF-FAILURE` (component failure during active session).

*Reliability Invariant IDs* (§44): `INV-PERF-001`…`INV-PERF-010`.

*Network-degradation figures* (§13): latency 100/200/500 ms; connection interruptions of 1s/5s/30s/"several minutes."

*Resolutions* (§11): 1280×720, 1920×1080, 2560×1440, 3840×2160 (identical set to Doc 12 §30). *Refresh rates*: 60 Hz, 90 Hz, 120 Hz, "higher supported rates" — **differs from Doc 12's 60/120/144 Hz set**.

*Resource-leak tracking categories* (§40): RSS, FD count, thread count, socket count, PipeWire objects, D-Bus subscriptions, WebRTC peers, timers/tasks, temporary files, log volume, GPU memory.

*Metric groups* (§43): Session metrics (successful/failed sessions, prep/teardown/recovery failures, emergency activations); Security metrics (auth failures, lease expirations/revocations, epoch increments, stale-session attempts, emergency invalidations); Resource metrics (CPU/memory/GPU/network/descriptors/active processes/sessions); Reliability metrics (crashes, restarts, watchdog activations, GNOME/PipeWire/WebRTC failures, display/input restoration failures).

*Components* (§5): host daemon (`remote-hostd`), gateway, GNOME agent, encoder, browser client, emergency daemon (`remote-emergencyd`).

No tool names anywhere (confirmed via grep — no valgrind/perf/systemd directive names like `MemoryMax=`/`TasksMax=`/`LimitNOFILE=`; §21 lists only limit *categories*).

**3. Hard requirements, gates and stop conditions**
- §44 10 `INV-PERF` invariants that "must remain true under performance/resource pressure" — e.g. `INV-PERF-001` resource exhaustion cannot create remote authority; `INV-PERF-003` cannot invalidate the emergency path; `INV-PERF-007` cleanup eventually reaches a safe state or explicitly enters `FAILED_SAFE`; `INV-PERF-009`/`010` remote failure cannot leave physical input/display in an unknown state without an explicit documented safe-recovery mechanism.
- §45 Failure Escalation ladder: `NORMAL RECOVERY → RETRY → SECONDARY RECOVERY → FAILED_SAFE`, with `FAILED_SAFE` strictly defined: "must never mean 'We don't know what happened.' It must mean: remote authority is definitely revoked and the system is in a known safe state, even if some convenience functionality is unavailable."
- §47 Performance Optimization Rules: 8-step MUST-do-before-optimizing sequence, plus explicit "Never optimize by:" 8-bullet list (removing auth checks, extending leases unnecessarily, disabling safety verification, bypassing state transitions, increasing privileged permissions, making cleanup async without ownership/timeout semantics, trusting browser state, trusting network state).
- §51 Release Reliability Gates — 13 auto-block conditions (verbatim): memory grows without bound; descriptors grow without bound; repeated sessions leak virtual monitors; PipeWire resources accumulate; WebRTC peers remain after teardown; reconnect loops consume unbounded resources; log flooding can exhaust disk; remote input can survive resource exhaustion; emergency handling becomes unavailable under normal load; recovery can hang indefinitely; GNOME becomes unstable after repeated sessions; resource limits cause unsafe recovery; high CPU/memory/network load can bypass a security invariant.
- §50 Definition of Done — 17-item checklist (resource metrics for all critical components; activation/teardown timing observable; no leaks across repeated sessions; WebRTC/PipeWire/virtual-monitor cleanup; bounded fds/threads/logging/reconnect; backpressure on untrusted input; watchdog tested; emergency available under load; long-running + repeated-cycle + failure-under-load testing performed; baselines documented; security invariants verified under resource pressure).

**4. Technology and library choices**
Entirely process/qualitative — no framework, library, or tool is named. §47 mandates a strict order: measure → identify bottleneck → establish baseline → implement change → benchmark → run safety tests → run regression tests → run long-duration test. §48 point 7: "Establish baselines before introducing performance thresholds" (explicit anti-pattern: don't invent numbers).

**5. Performance plan detail**

*Metric targets:*

| Metric | Target/limit stated | Notes |
|---|---|---|
| Input latency (LAN) | ~50–100 ms end-to-end | Only concrete number in the doc; explicitly provisional |
| Frame latency | none (qualitative) | measure capture/encode/send/receive/decode/display timestamps to find bottleneck |
| Activation stages | none (qualitative) | each of auth→authz→lease→GNOME-prep→virtual-display→physical-isolation→media-ready→REMOTE_ACTIVE individually timed |
| Teardown | "bounded recovery target" (unspecified number) | disconnect/failure→authority revoked→input revoked→locked→display restored→input restored→LOCAL_LOCKED, each stage timed |
| CPU/Memory/FD/thread budgets | none — "measure" only | thresholds to be set only after baseline data exists (§41) |

*Reliability invariants (`INV-PERF-001`…`010`, full list):* 001 resource exhaustion cannot create remote authority; 002 cannot extend an expired lease; 003 cannot invalidate the emergency path; 004 a stalled component cannot retain remote input indefinitely; 005 a network reconnect cannot bypass authentication/authorization; 006 repeated sessions do not produce unbounded resource growth; 007 cleanup eventually reaches a safe state or explicitly enters `FAILED_SAFE`; 008 performance adaptation cannot modify security authority; 009 remote failure cannot leave physical input permanently disabled without an explicit safe-state mechanism; 010 remote failure cannot leave physical display privacy in an unknown state without an explicit documented safe-recovery state.

*Benchmark profiles (§42, full table):*

| Profile | Definition |
|---|---|
| PERF-LAN-1080P | local LAN, 1920×1080, normal desktop activity |
| PERF-LAN-4K | local LAN, 3840×2160, representative workload |
| PERF-WAN | Internet path, realistic latency/jitter, TURN where applicable |
| PERF-DEGRADED | high latency, packet loss, limited bandwidth |
| PERF-SOAK | long-running session |
| PERF-CYCLE | repeated connect/disconnect |
| PERF-FAILURE | component failure during active session |

*Soak/cycle/failure-under-load scenarios:* memory-leak cycles 100/500/1000 (§7); repeated-cycle reliability 100/500/1000 (§30, full lifecycle: authenticate→establish→virtual display→disable physical display→isolate input→remote input→stream→disconnect→revoke lease→lock→restore display→restore input→verify `LOCAL_LOCKED`); soak 1h/6h/12h/24h/48h with application interaction, resolution changes, network degradation, reconnects (§31); reliability-under-load scenarios: CPU pressure, memory pressure, disk pressure (§37 low-disk, §38 low-memory), GPU pressure, network pressure (§36) — expected: quality may degrade but "safety invariants remain intact... emergency remains usable... eventual safe recovery remains possible."

*Degraded-network profiles (§13):* high latency 100/200/500 ms; packet loss (increasing); jitter (variable latency); bandwidth reduction (gradual); connection interruption 1s/5s/30s/several minutes. Expected: user-visible degraded state, controlled reconnect, no unbounded buffering, no runaway CPU/memory, correct lease semantics, safety transition at the configured connection-loss threshold.

*Resource-management rules:* WebRTC (§15) peer/media-track/data-channel/ICE/TURN counts must return to baseline post-disconnect, tested via abrupt termination not just graceful close; PipeWire (§16) streams/nodes/ports/connections/buffers/subscriptions must not leak across create/capture/destroy cycles; Mutter (§17) stale virtual monitors/configs/modes/cursor resources/RemoteDesktop/ScreenCast sessions treated as "first-class reliability concern" — "do not assume an operation is safe simply because it works once."

**6. CI without GNOME hardware vs requires real host**
**No explicit split is defined** (unlike Doc 12's Tier 1-4) — a genuine structural gap. Inferred from content: unit-level resource/timing instrumentation, backpressure/rate-limiting logic, and systemd-config parsing are GNOME-independent; but §§16-18 (PipeWire/Mutter/WebRTC resource cleanup), §28 GPU matrix, and §§30-31 (cycle/soak tests against a live session) inherently require a running GNOME/Mutter/PipeWire stack and real GPU hardware.

**7. Evidence and reporting format**
**No standardized test-result template exists in this document** — unlike Doc 12 §46 and Doc 18 §33, there is no "Test ID:/Environment:/Pass-Fail:" schema here. §43 lists metric categories to "expose or collect," and §51 lists blocking conditions, but neither is a report format. This is a notable inconsistency in the doc set's evidence discipline (three docs, three different levels of reporting rigor).

**8. Phase / sequencing guidance**
No Phase 0-20 mapping (0 hits). §49 Copilot Implementation Order gives its own 20-step sequence: resource instrumentation → state-transition timing → session lifecycle metrics → CPU/memory/FD monitoring → cleanup verification → repeated connect/disconnect testing → network degradation testing → WebRTC resource cleanup → PipeWire cleanup → Mutter resource cleanup → GNOME stability testing → systemd watchdog validation → bounded retry/backoff → backpressure → resource limits → fault-under-load testing → long-running soak tests → performance benchmarks → regression thresholds → release reliability gate.

**9. Cross-references**
Explicit, listed at top (lines 23-27): Document 7 (Remote Session State Machine), Document 10 (Feasibility PoC Implementation & Experiment Plan), Document 12 (Testing Strategy & Test Matrix), Document 13 (Observability & Diagnostics), Document 18 (Threat-Driven Security Testing & Red-Team Plan). §46 "Reliability Testing With Document 18" gives concrete combined-test examples (CPU exhaustion + expired lease; network flooding + emergency shortcut).

**10. Ambiguities, gaps, internal contradictions, conflicts with Doc 00**
- **Cycle-count and soak-duration contradiction with Doc 12** (headline finding, detailed in cross-doc section below): this document's 100/500/1000 cycles and 1h-48h soak ladder match the user's Doc-00-derived context, while Doc 12 states materially smaller numbers.
- **Refresh-rate set** (§11: 60/90/120 Hz) differs from Doc 12 §30 (60/120/144 Hz).
- **No CI/hardware split** defined, unlike Doc 12's explicit 4-tier scheme — makes it unclear where, e.g., "cleanup verification" (Copilot order step 5) is meant to run.
- **No evidence/report template** of its own, unlike Doc 12 §46 and Doc 18 §33.
- **No P0-style severity scale** — §51's Release Reliability Gates are a flat, unlabeled blocking list, a *third* distinct severity/gating vocabulary alongside Doc 12's `P0-P3` and Doc 18's `CRITICAL/HIGH/MEDIUM/LOW/INFORMATIONAL`.
- Deliberately supplies almost no fixed target values (only the provisional 50-100ms figure) — worth stating plainly since the user's request asked for "target values": **this document mostly declines to name them**, treating that as correct discipline rather than an omission (§41).

**11. Risks and feasibility concerns the doc itself raises**
- §17: "Mutter instability must be treated as a first-class reliability concern. Do not assume an operation is safe simply because it works once" — explicit self-flagged fragility risk in the core GNOME integration.
- §21: "A limit that kills a safety-critical transition is worse than no limit" — self-identified risk that aggressive systemd sandboxing/resource limits could break safety-critical code paths.
- §34: clipboard "should remain disabled until its security model is explicitly implemented" — explicit scope-risk flag.
- §37/§38: low-disk/low-memory sections explicitly require fail-closed behavior, implying a real risk that a naive implementation could fail-open under OOM/disk-full conditions.

---

## Cross-document notes for this batch

### Contradictions between the three docs (with line refs)

| # | Topic | Doc 12 | Doc 18 | Doc 19 |
|---|---|---|---|---|
| 1 | Repeated-cycle counts | §26, lines 1047-1049: **10 / 50 / 100+** cycles | — | §7 lines 273-275, §30 lines 890-892: **100 / 500 / 1000** cycles |
| 2 | Soak durations | §27, lines 1073-1076: 30 min / 2 h / 8 h / overnight | — | §6 lines 244-248, §31 lines 904-908: **1 h / 6 h / 12 h / 24 h / 48 h** |
| 3 | Refresh-rate test set | §30: 60 / 120 / **144** Hz | — | §11: 60 / **90** / 120 Hz |
| 4 | Severity vocabulary | §49 lines 1666-1721: `P0/P1/P2/P3` (Critical/High/Medium/Low) | §3 lines 139-192: `CRITICAL/HIGH/MEDIUM/LOW/INFORMATIONAL` (no P-numbers) | §51 lines ~1417-1432: flat unlabeled blocker list (no tiers at all) |
| 5 | "P0" meaning | Bug severity (line 1668) | Attack-execution priority order (§35 line 1920) — a *different axis*, same token | not used |
| 6 | Environment-tier vocabulary | §51: `Tier 1-4` | §34: `AUTOMATED / SYSTEM AUTOMATED / MANUAL-PHYSICAL` | none defined |
| 7 | Evidence/status granularity | §46: binary Pass/Fail | §40: 6-tier `IMPLEMENTED…BLOCKED` | none defined |
| 8 | Gate A-H identity (outside these 3 docs, but directly relevant to the task's own framing) | not referenced | not referenced | not referenced — yet Doc 00 lines 1819-1855 (`Same Session…Same-Session Recovery`) and Doc 09 lines 1941-1985 (`Authentication…No Arbitrary Privileged Execution`, extends to `Gate J`) define **two conflicting Gate A-H/A-J taxonomies** elsewhere in the same spec set |
| 9 | Phase 0-20 mapping | absent (§60 has its own loop) | absent (§39 has its own 22-step order) | absent (§49 has its own 20-step order) — all three docs are phase-agnostic |

### The 5 most decision-relevant insights for planning test infrastructure from Phase 2 onward

1. **Build the fault-injection/event-simulation harness and the state machine together, first.** Doc 12 §53-54's event vocabulary (`NETWORK_LOST`, `LEASE_EXPIRED`, `HOSTD_CRASH`, etc.), Doc 18's `INV-SEC-*` assertions, and Doc 19's `INV-PERF-*` assertions all key off a synthetic, GNOME-independent state machine. Doc 18 §39 explicitly warns: "Do not start with large-scale fuzzing before deterministic security invariants and state-machine tests exist." This is the single highest-leverage, hardware-free artifact.
2. **Make session-credential, control-lease, and security-epoch validation pure, unit-testable modules early.** Roughly 16 of the 82 named RT-* attacks (all `RT-SESSION-*`, `RT-LEASE-*`, `RT-EPOCH-*`) plus `INV-SEC-001/002/003/008/009` and `INV-PERF-001/002/005` depend entirely on this logic being deterministic and hardware-free — it is the largest single bucket of P0-relevant tests answerable without GNOME.
3. **Stand up one real Ubuntu 26.04/GNOME 50 target before Phase 2 exits.** Doc 12 Tier 3/4, Doc 18's `SYSTEM AUTOMATED`/`MANUAL-PHYSICAL` tiers, and nearly all of Doc 19 §§16-18 (PipeWire/Mutter/WebRTC resource cleanup) are entirely blocked without it. Per prior session findings only one hybrid NVIDIA+Intel workstation is currently available — no separate red-team machine (Doc 18 §4) and no AMD GPU (Doc 12 §28/§51 GPU matrix), a resourcing gap worth resolving explicitly rather than discovering late.
4. **Instrument resource/timing metrics from the first working prototype, not after.** Doc 19 §41 explicitly forbids setting thresholds before baseline data exists, and the cycle/soak tests (100-1000 cycles, 1-48h) are retroactive-comparison tests — if RSS/FD/thread/PipeWire-object/WebRTC-peer counters aren't wired in during Phase 2/3, there is no baseline against which Phase-9+ regression thresholds (§41) or the release reliability gate (§51) can ever be evaluated.
5. **Reconcile the three documents' severity/evidence vocabularies into one schema before writing the first test.** Doc 12 (`P0-P3`), Doc 18 (`CRITICAL/HIGH/MEDIUM/LOW/INFORMATIONAL` + `IMPLEMENTED…BLOCKED`), and Doc 19 (flat unlabeled blockers) cannot be mechanically rolled up into a single release/gate decision as written — this directly blocks a clean answer to "has Gate A-H passed," which is exactly what the next planning step needs.

### Consolidated P0 safety tests that must exist before any hard gate (A-H) is declared passed

**Note on ambiguity (must be resolved before use):** none of the three assigned documents define "Gate A-H" themselves. Two incompatible definitions exist elsewhere in the spec set: Doc 00 lines 1819-1855 (*feasibility* gates: A=Same Session, B=Virtual Display, C=Physical Display Isolation, D=Remote Input, E=Physical Input Isolation, F=Fail-Safe, G=Emergency, H=Same-Session Recovery) and Doc 09 lines 1941-1985 (*security* gates: A=Authentication, B=TOTP, C=Session Isolation, D=Lease Expiration, E=Security Epoch, F=Physical Input Isolation, G=Physical Display Isolation, H=Emergency Independence, **plus I=Safe Teardown, J=No Arbitrary Privileged Execution which have no analog in Doc 00's scheme at all**). The mapping below uses **Doc 00's lettering** (the one already established as ground truth in this task's context) and separately calls out Doc 09's extra I/J.

| Gate (Doc 00) | Must-pass P0 tests before declaring PASS |
|---|---|
| **A — Same Session** | Doc 12 §17 Same-Session Continuity Tests; Doc 12 §47 row "Same GNOME session" (BLOCKER); Doc 18 §20 state-machine attack tests confirming `LOCAL_LOCKED→REMOTE_ACTIVE` never skips authentication |
| **B — Virtual Display** | Doc 12 §12 Virtual Monitor Tests; Doc 19 §16/§17 PipeWire & Mutter resource-leak checks across create/destroy cycles |
| **C — Physical Display Isolation** | Doc 12 §13; Doc 18 `RT-DISPLAY-001…005`; `INV-SEC-007`; `INV-PERF-010` |
| **D — Remote Input** | Doc 12 §47 row "Remote input" (BLOCKER); Doc 18 `RT-LEASE-001` (no lease → no input) as the enabling precondition |
| **E — Physical Input Isolation** | Doc 12 §14; Doc 18 `RT-INPUT-001…007`; `INV-SEC-006`; `INV-PERF-009` |
| **F — Fail-Safe** | Doc 12 §19-22 (failure injection, activation-failure matrix, network failure, reconnect) + §58 safety checklist; Doc 18 `RT-LEASE-*`, `RT-EPOCH-*`, `RT-NET-*`; `INV-SEC-001/002/003/009`; Doc 19 `INV-PERF-001/002/004/005/007`; Doc 19 §51 Release Reliability Gates |
| **G — Emergency** | Doc 12 §15 Emergency Controller Tests; Doc 18 `RT-EMERGENCY-001…007` (esp. `RT-EMERGENCY-004`, explicitly marked CRITICAL) and `RT-INPUT-007`; `INV-SEC-004/010`; Doc 19 §19 Emergency Daemon Resource Guarantees, `INV-PERF-003` |
| **H — Same-Session Recovery** | Doc 12 §18 Teardown Tests + §59 Final Release-Gate Test (26-step scenario); Doc 18 `RT-EMERGENCY-006`, `RT-EPOCH-002`; `INV-SEC-005/008`; Doc 19 §45 Failure Escalation ladder / `FAILED_SAFE` semantics |
| **(Doc 09 only) I — Safe Teardown** | Doc 12 §18 idempotent-teardown requirements; Doc 19 §2.4 cleanup-idempotency rules |
| **(Doc 09 only) J — No Arbitrary Privileged Execution** | Doc 18 `RT-PRIV-001…006` — **has no home under Doc 00's 8-letter scheme at all**; if only Doc 00's Gate A-H is honored, privilege-escalation testing (explicitly Doc 18 §35 P0 item #11) would not gate release, which is a real safety gap unless Doc 09's Gate J is also adopted |

Underlying every gate, the following must independently be green first (cut across all gates): Doc 18 §29 `INV-SEC-001…010` (all 10), Doc 19 §44 `INV-PERF-001…010` (all 10), and Doc 12 §57-58's Security/Safety checklists in full — these three lists are the actual atomic P0 test units; the Gate A-H/A-J letters are just groupings over them.