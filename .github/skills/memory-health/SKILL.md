---
name: memory-health
description: Verify the mandatory MemPalace connection and recovery protocol without changing project files. Use at setup, after an outage, or when historical memory appears incomplete.
disable-model-invocation: true
---

# Memory Health

Run these read-only checks in order:

1. Call `mempalace_status` and report backend, wing/room counts, and whether the
   AAAK protocol loaded.
2. Call `mempalace_diary_read` with `agent_name="copilot"`,
   `wing="blackroom_console"`, and `last_n=3`.
3. Call `mempalace_search` with the keywords `memory health protocol`, wing
   `blackroom_console`, no room filter, and a small result limit.
4. Call `mempalace_memories_filed_away` when available to inspect checkpoint
   recency.
5. Verify `docs/HANDOFF.md` records any memory operations still pending.

Do not add test drawers, KG facts, or diary entries during a health check. Report
`HEALTHY`, `DEGRADED`, or `UNAVAILABLE`, the failed capability, and the exact
next action. Historical work must not proceed silently in degraded mode.