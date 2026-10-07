# Handoff: Blackroom Console

**Updated:** 2026-10-07 (v0.1.0 public technical preview)
**Current status:** [release and implementation record](plans/plan-20261004-status.md)
**Plan:** [final implementation plan](plans/plan-20261004-final-plan.md), under the MVP fast path
**Branch:** `main` | **Memory:** wing `blackroom_console` | Routine work: mini/compact; release: governed

## Current State
- [v0.1.0 release](https://github.com/rai-himanshu07/Blackroom-Console/releases/tag/v0.1.0): signed tag, installer,
  checksum manifest/signature, public key and verification script; public downloads verified with `RELEASE OK`.
- Shipped app: `blackroom-console`, with Private/Shared sessions, WebRTC video/sound, MJPEG fallback, text clipboard,
  three-factor hostd login, host settings, tray, credential CLI and packaged units. The old simulation chain is not the MVP.
- [README](../README.md) covers install, setup, connection, ports and recovery; host settings are laptop-only on port 8090.
- Code gates, headless/browser/network/package checks and release review passed with recorded limits. Do not rerun them
  merely for documentation. Latest security fixes and evidence: [security report](security/red-team-report.md).
- Support is narrow: [compatibility table](ops/compatibility-matrix.md). Built-in panel only; external outputs unsupported.

## Safety and Decisions
- Never remove the live virtual monitor from a config while its PipeWire consumer streams, or apply display config
  immediately after owner death. Restore only against the original Shell/session identity; never auto-unlock.
- Normal Stop restores display/input and locks only per the session setting. The emergency chord needs an active grab,
  closes remote login until local recovery and does not lock by itself. See [recovery](ops/emergency-recovery.md).
- Lock-screen remote access and automatic login have explicit privacy trade-offs; missing configured hostd fails closed.
- Accepted FEAS-A/C/D/E and integrated results remain scoped and accepted; do not reopen them or infer unobserved PASS.
- [Experiment safety](ops/experiment-safety.md): supported-flow standing approval, recovery kit, one log line per live run;
  a new risk class needs the risk/recovery explained and operator approval. Operator cues must be audible when blanked.

## Next
- Unverified: sleep/lid-close recovery, certificate renewal, long live soak, untested browsers and a new clean-machine
  package acceptance. Owner-reported results are not independent evidence; formal Phase 9/10 gates remain separate.
- HDMI/hotplug, other layouts/GPUs and multi-user work remain deferred. No new live experiments are implied by this handoff.
- Install/update remains the owner's action; publication did not replace the running app. Commit explicit paths and run
  the publication secret scans before pushing; never publish local credentials or machine-specific details.
