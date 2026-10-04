# Contributing

Blackroom Console is a personal project made public. Issues and ideas are welcome; pull requests may be declined if they do not
fit its small scope (one owner, one laptop, GNOME on Wayland). Security problems: see [SECURITY.md](SECURITY.md), not an issue.

## Before you change code

- Read [AGENTS.md](AGENTS.md) for the working rules and [docs/WORKFLOW_CONFIG.md](docs/WORKFLOW_CONFIG.md) for the checks.
- Reuse what exists before adding a helper, dependency or setting. Keep the change single-purpose and say what it does not do.
- Safety first: restore, lock, revoke and recovery paths must keep failing visibly, never silently. Test the changed behaviour.

## Checks (there is no CI: run them yourself)

```
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --exclude gnome-session-agent && cargo test -p gnome-session-agent
cargo deny check && cargo audit
```

Page, tray and recovery changes also need the headless suites in `docs/ops/` (they start a throwaway GNOME Shell on a private
D-Bus and never touch your real screen): `headless-browser-test.sh`, `headless-hostpage-test.sh`, `headless-indicator-test.sh`,
`headless-faults-test.sh`, run through `docs/ops/headless-repro.sh` where the script says so. Run one cargo command at a time and
use `CARGO_INCREMENTAL=0`.

Do not run anything that changes your real display or input without a second device logged in over SSH: see
[docs/ops/experiment-safety.md](docs/ops/experiment-safety.md).

## Commits and licence

Commit with a noreply address. Contributions are accepted under the project licence, GPL-3.0-or-later. Do not commit secrets,
real hostnames or personal data; evidence files should be readable by a stranger.

## Versions

SemVer. `0.x` is a technical preview. The version lives once, in the workspace `Cargo.toml`; a release is tagged `vX.Y.Z` and the
Debian package is `X.Y.Z-1`.
