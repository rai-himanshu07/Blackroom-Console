# docs/experiments/

Evidence produced by the Document 10 feasibility experiments (`crates/blackroom-experiments`).

## Layout

```text
docs/experiments/evidence/<expNN>/<YYYY-MM-DD>/
  report.md       # Document 10 §47 result format
  *.json          # structured data (one or more sidecars, e.g. environment.json)
  *.xml           # D-Bus Introspect() output, where applicable (Experiment 2 only)
```

`<expNN>` is the zero-padded experiment id (`exp00`, `exp01`, `exp02`, ...) matching the
binary name in `crates/blackroom-experiments/src/bin/`. Each run creates or reuses today's
date directory; re-running an experiment on the same day overwrites that day's evidence
(the binaries are idempotent and deterministic modulo the timestamp field).

## Rules

- Evidence files redact the Unix username and hostname by default (`--no-redact` to
  disable warnings, Phase 1 decision in the active plan).
- No experiment binary in Phase 0–1 writes outside its own evidence directory.
- Never commit secrets, credentials, or raw EDID serials (hash them) into evidence.
- Before any experiment that changes display or input state (Phase 4+), follow
  `docs/ops/experiment-safety.md`.
