# Code Intelligence Protocol

Use codebase-memory to reduce repeated source exploration while preserving live
verification.

## Evidence Levels

- **Scout:** positive orientation only. Use it for unfamiliar architecture and
  candidate symbols. Do not make absence, exhaustive, dead-code, security, or
  complete-impact claims.
- **Verify:** default for task-directed implementation. Check the canonical
  project/root and freshness when the active interface exposes those operations,
  then verify graph results in live source.
- **Auditor:** negative, exhaustive, security, architecture-boundary, dead-code,
  or complete-impact work. Require a bounded scope, every relevant result page,
  coverage evidence when exposed, and live source checks.

Match the evidence level to the claim. If Auditor evidence cannot be completed,
state the limitation and narrow the claim instead of inventing certainty.

## Structural Discovery

For architecture, symbols, callers, call paths, routes, dependencies, and change
impact:

1. Skip graph work for a supplied known file, exact literal/configuration/error
   lookup, trivial one-file check, or non-code text.
2. Work with codebase-memory project `Blackroom_Console`. Confirm it is
   indexed/current only when the active surface exposes a status operation.
3. Use `get_architecture` for orientation, `search_graph` for symbols, and
   `trace_path` for callers/callees. Use `detect_changes` for diff impact only
   when that optional operation is available.
4. For exhaustive or negative claims, use coverage/freshness operations only
   when they are actually exposed; otherwise mark coverage unverified.
5. Read flagged, skipped, excluded, generated, or partially parsed files live.
6. Verify exact behavior and edits against source, language services, and tests.

Graph evidence can establish positive structural leads. A missing graph result
does not prove absence unless freshness, pagination, and relevant path coverage
have all been checked.

The available surface is capability-detected. Prefer `get_architecture`,
`search_graph`, `trace_path`, and supported complexity/data-flow queries.
`index_status`, `detect_changes`, and coverage operations are optional. If they
are absent, record freshness/coverage as unverified and continue with language
services and targeted text search; never invent a result.

## Degraded Mode

If codebase-memory is unavailable or stale, state `CODE GRAPH DEGRADED`, then use
semantic search, text search, symbol usages, and focused file reads. Do not block
the task, but do not present the fallback as an exhaustive graph-backed result.
