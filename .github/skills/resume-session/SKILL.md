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
6. Run the configured fast test command when it is available:

   ```text
   pytest -x -q
   ```

7. Under `required`, use exposed codebase-memory tools for
   project `Blackroom_Console`, then compare graph evidence with live code.
8. Summarize memory health, current status, check results, blockers, and the next
   approved plan step in at most six bullets.

Do not implement until the user confirms continuation.
