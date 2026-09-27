---
name: handoff-session
description: Persist current task state for a clean next session. Use when the user invokes /handoff-session before ending work.
disable-model-invocation: true
---

# Handoff Session

Rewrite `docs/HANDOFF.md` as current state, not accumulated history.

1. Preserve the exact Active plan path and verify the file exists, or write
   `none` when there is no active plan.
2. Update completed checkboxes and the execution log in that plan using only
   verification actually observed.
3. Record branch or workspace state, changed files, checks run and outcomes, the
   exact stopping point, decisions and rationale, blockers, and ordered next
   actions.
4. Store small durable project conventions in repository memory.
5. When the active memory policy requires a write, prefer one atomic
   `mempalace_checkpoint` as agent `copilot`, with every item and the nested
   diary explicitly using wing `blackroom_console`. If it is unavailable, call
   `mempalace_diary_write` with the same wing and file large
   valuable synthesis with `mempalace_add_drawer` as a documented degraded
   fallback. Prefer `kg_supersede` for changed single-valued KG facts, and
   update facts only when durable facts changed.
6. If any required memory call fails, set the handoff Memory field to
   `DEGRADED`, list every pending read/write under Pending memory operations, and
   report the failure. Never silently discard synthesis.
7. Keep the handoff within 40 lines / 3 KB and remove stale items. Keep current code
   state in the handoff and live working tree, not in MemPalace.

Do not make implementation changes while producing the handoff.
