---
name: resume-session
description: Recover task state from the handoff and exact active plan. Use when the user invokes /resume-session in a fresh session.
disable-model-invocation: true
---

# Resume Session

1. Read `AGENTS.md`, `docs/WORKFLOW_CONFIG.md`, and `docs/HANDOFF.md`.
2. Follow the resolved `required` policy. When history matters, call
   `mempalace_status`; if a required call fails, declare `MEMORY DEGRADED` and
   record the pending operation in the handoff.
3. Read the exact active plan or specification path from the handoff. Never
   substitute the newest file. Stop if the value is ambiguous or missing.
4. When historical context matters, call `mempalace_diary_read` for agent
   `copilot` with wing `blackroom_console`. Call `mempalace_search` with that
   same wing for
   the active task's durable decisions and prior synthesis using keyword-only
   queries. Resolve any pending memory operations recorded in the handoff.
   If the handoff is missing, stale, or contradicted, query Chronicle/session
   history for the last relevant session before broad source exploration.
5. Check current workspace state. If this is a Git repository, inspect status and
   recent commits without changing them.
6. Do not run a baseline test just for resuming. After a code edit, run the
   smallest affected check; broad checks only at the risk-based checkpoint in
   `AGENTS.md`. Docs-only resumption needs no code tests.
7. Use exposed codebase-memory tools for project `Blackroom_Console` when
   structural impact is unknown; compare graph evidence with live code.
8. Summarize current status and blockers briefly, then continue already
   approved independent offline steps. Ask only for a genuinely new decision,
   a destructive action, or the live operator participation required by §7 of
   `docs/ops/experiment-safety.md`.
