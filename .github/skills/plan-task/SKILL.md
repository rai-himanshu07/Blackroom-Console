---
name: plan-task
description: Create a bounded implementation plan and set it as the active plan. Use when the user invokes /plan-task before coding.
argument-hint: '[task and acceptance criteria]'
disable-model-invocation: true
---

# Plan Task

Use the text following `/plan-task` as the task. If no task is supplied, ask for
it. Do not implement the task.

1. When required by the active memory policy, call `mempalace_status`, inspect
   relevant `/memories/repo/` notes, and read
   `docs/MEMORY_PROTOCOL.md` plus `docs/HANDOFF.md`.
2. Call `mempalace_diary_read` for agent `copilot` with wing
   `blackroom_console`, then use the same wing for `mempalace_search` for
   prior decisions and synthesis. If the handoff is missing, stale, or
   contradicted, query Chronicle before broad code exploration. Declare
   `MEMORY DEGRADED` if required memory access fails.
3. Read `AGENTS.md`. Use codebase-memory for structure and impact, checking
   freshness and coverage, then read only nearby live implementation and tests
   needed to plan. Declare `CODE GRAPH DEGRADED` when the graph is unavailable.
4. Create `docs/plans/plan-YYYYMMDD-TASK-SLUG.md` from
   `docs/PLAN.template.md`.
5. Include goal, acceptance criteria, non-goals, evidence, decisions, risks,
   ordered steps, dependencies, touched files, and verification per step.
6. Set the new plan's status to `draft`.
7. Replace the Active plan value in `docs/HANDOFF.md` with the exact new path.
8. Return the path and ask the user to approve or amend it.

Only the new plan and the handoff's Active plan value may be edited.
