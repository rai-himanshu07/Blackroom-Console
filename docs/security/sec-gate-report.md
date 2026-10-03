# Phase 11 gate report: SEC-A to SEC-E and SEC-J

Date 2026-10-03. Scope: unit and integration level, offline, fake password check (the real PAM
accept path needs the files installed by `docs/ops/install-security.sh` and is the owner's live check).

Checks run: `cargo fmt --check`, `cargo clippy --workspace --all-targets -D warnings`,
`cargo test --workspace --exclude gnome-session-agent` (481 tests pass) and
`cargo test -p gnome-session-agent` (36 pass), `cargo deny check` (workspace and the separate
`crates/pam-auth-helper`), `cargo audit`. The PAM helper has its own workspace: `cargo test --manifest-path
crates/pam-auth-helper/Cargo.toml --target-dir target/pam-helper`.

## Gates (Doc 09 §100)

| Gate | Reading | Evidence |
|---|---|---|
| A Authentication: all required factors enforced | PASS (offline) | `rt_auth_001` to `rt_auth_007`, `rt_auth_011`, `rt_auth_012`; `login.rs` order of checks |
| B TOTP cannot be bypassed for trusted clients | PASS (offline) | `rt_auth_008`, `rt_auth_009`, `rt_auth_005` (replay), `rt_auth_003` |
| C A session without a control lease cannot inject input | PASS (existing tests, not re-run per se) | the authority only authenticates and issues sessions; input needs a lease: `blackroom-core` `input_authorization_checks_auth_signature_and_current_lease`, `validate_rejects_view_only_lease_for_remote_input`, `input_dispatch_never_calls_sink_after_revocation_or_state_change` |
| D Lease expiration revokes input | PASS (existing tests) | `validate_rejects_expired_lease`, `input_dispatch_rechecks_expiry_when_snapshot_time_is_stale`, hostd `expire_grant` path in `offline_hostd.rs`; session expiry `rt_session_001` |
| E Security epoch invalidates stale sessions | PASS (offline) | `rt_epoch_001` to `rt_epoch_004`, `rt_session_004`, `disable_closes_remote_access_at_once...` |
| J No arbitrary privileged execution | PASS by construction | nothing runs privileged; helper and `pkcheck` run with fixed arguments; the two root files are static (`threat-model.md`) |

Not part of this gate and not claimed: F, G, H, I (physical isolation, emergency independence, safe
teardown) belong to Phases 9 and 10.

## RT matrix (Doc 18 §7-10, §15)

Tests are in `crates/remote-hostd/tests/rt_security.rs` unless named otherwise.

| Case | Status | Where |
|---|---|---|
| RT-AUTH-001 missing password | covered | `rt_auth_001` |
| RT-AUTH-002 wrong password, rate limit | covered | `rt_auth_002` |
| RT-AUTH-003 missing TOTP | covered | `rt_auth_003` |
| RT-AUTH-004 invalid TOTP | covered | `rt_auth_004` |
| RT-AUTH-005 reused TOTP | covered | `rt_auth_005` |
| RT-AUTH-006 missing key, new device | covered | `rt_auth_006` |
| RT-AUTH-007 invalid key | covered | `rt_auth_007` (also: no code burnt) |
| RT-AUTH-008 trusted device without TOTP | covered | `rt_auth_008` |
| RT-AUTH-009 stolen trusted credential | covered; trust model is a bearer credential, password and code still required | `rt_auth_009` |
| RT-AUTH-010 revoked device | covered | `rt_auth_010` |
| RT-AUTH-011 revoked key | covered | `rt_auth_011` |
| RT-AUTH-012 recovery code abuse | covered | `rt_auth_012` |
| RT-AUTH-013 enumeration | covered for response and PAM cost; timing not measured | `rt_auth_013` |
| RT-AUTH-014 flooding | covered | `rt_auth_014` (bounded table, no password check once saturated) |
| RT-AUTH-015 concurrent race | covered | `rt_auth_015`; also `a_hung_password_check_does_not_delay_operator_commands` |
| RT-SESSION-001 expired | covered | `rt_session_001` |
| RT-SESSION-002 wrong host | covered (random 256-bit tokens, unknown elsewhere) | `rt_session_002` |
| RT-SESSION-003 wrong client | partial: the session reports its principal and epoch; binding to a connection is the gateway's job (Phase 12) | `rt_session_003` |
| RT-SESSION-004 replay | covered (logout, revoke, expiry, epoch) | `rt_session_004` |
| RT-SESSION-005 multi-client reuse | partial: a live-session cap; one-controller policy is lease level | `rt_session_005` |
| RT-LEASE-001 no lease, input | covered by existing tests | `blackroom-core` lease tests, agent input tests |
| RT-LEASE-002 expired lease | covered by existing tests | `validate_rejects_expired_lease` |
| RT-LEASE-003 lease from previous session | covered by existing tests | hostd `offline_hostd_renews_only_for_the_holder_of_the_active_grant` |
| RT-LEASE-004 wrong session id | covered by existing tests | `validate_rejects_wrong_session_id` |
| RT-LEASE-005 wrong epoch | covered by existing tests | `validate_rejects_epoch_mismatch` |
| RT-LEASE-006 renewal after revocation | covered by existing tests | hostd lib `renewal_resigns_only_an_active_grant...` |
| RT-LEASE-007 expiration race | covered by existing tests | `input_dispatch_rechecks_expiry_when_snapshot_time_is_stale` |
| RT-EPOCH-001 stale session after increment | covered | `rt_epoch_001` |
| RT-EPOCH-002 reconnect with stale credential after emergency | covered | `rt_epoch_002` |
| RT-EPOCH-003 concurrent emergency race | covered at the authority (nothing resolves after `revoke-all` returns); full input path is Phase 10 | `rt_epoch_003` |
| RT-EPOCH-004 epoch persistence | covered | `rt_epoch_004` |
| RT-FILE-001 permissions | covered for modes; no second Unix user is created in tests | `rt_file_001`, `blackroom-store` tests |
| RT-FILE-002 symlink | covered | `rt_file_002`, `blackroom-store` symlink test |
| RT-FILE-003 traversal | covered | `rt_file_003`, `blackroom-store` names test |
| RT-FILE-004 configuration injection | covered | `rt_file_004`, `blackroom-store` corrupt/schema tests |
| RT-FILE-005 secrets in logs | covered | `rt_file_005`; CLI `diagnostics_and_compatibility_are_json_without_secrets` |

Crash consistency (Doc 17 §42): atomic temporary-file-and-rename writes; `crash_leftovers_do_not_change_what_a_restart_trusts`
and the store's concurrent-reader test. A real SIGKILL during a rotation is not simulated.
Data security across users (Doc 17 §74): owner-only modes asserted; separate-user processes not run.

## Needs the owner (live)

1. `docs/ops/install-security.sh --install` (two sudo files), then enrol and `blackroom ... login-check`:
   the first accepted real password through `pam_unix` and `unix_chkpwd` under the user unit.
2. `docs/ops/install-security.sh --check` (a wrong password must be rejected with exit 1, not 2).
3. Optional: `--polkit` and a local `blackroom enable` versus the same command from an SSH session.

Open: wiring the console web app to `auth.sock`, the browser trust-this-device flow, gateway
rate limits per source address, a measured timing check for enumeration, an independent review.
