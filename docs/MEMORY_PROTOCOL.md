# MemPalace Protocol

MemPalace preserves durable synthesis across sessions; it does not replace the
live working tree. It is mandatory only when the resolved memory policy is
`required`, used selectively when the policy is `on-demand`, and skipped when
the policy is `off`.

The canonical project wing is `blackroom_console`. Supply it explicitly to every
project diary read, search, checkpoint item, checkpoint diary, mine, and sync.
Reserve `wing_copilot` for genuinely cross-project lessons.

## Session Start

1. Call `mempalace_status` before project work.
2. Inspect relevant built-in repo memory under `/memories/repo/`.
3. Read `docs/HANDOFF.md` and its exact active plan.
4. When resuming prior work or recovering after compaction, call
   `mempalace_diary_read` for agent `copilot` with wing `blackroom_console`, then
   search that wing for relevant durable
   synthesis before exploring source.
5. If the handoff is missing, stale, or contradicted by the working tree, query
   Chronicle/session history for the last relevant session before reconstructing
   work from source.
6. Before answering about a person, project, decision, or past event, call
   `mempalace_kg_query` or `mempalace_search`. Never guess from chat history.

Search queries contain keywords only and stay below 250 characters. Put
background in the tool's context field. If wing or room filters fail, omit them
and filter returned metadata client-side.

## Targeted retrieval and atomic writes

Use project-scoped retrieval for the current task rather than loading a whole
palace wing. When the active interface supports it, prefer the compact
`mempalace_checkpoint` operation: it semantic-deduplicates durable items and
writes the diary together, reducing partial lifecycle state. For a
single-valued fact that changes, prefer atomic `kg_supersede` at the shared
boundary instead of separate invalidate/add calls. These operations are
capability-detected; do not invent a missing MCP tool or claim that a write
succeeded when the tool is unavailable.

## Storage Boundaries

- **MemPalace:** decisions, rationale, architecture synthesis, roadmaps, gap
  analyses, and lessons that are expensive to reconstruct.
- **Knowledge graph:** durable facts and temporal relationships. Prefer atomic
  `kg_supersede` for a changed single-valued fact; use invalidate/add only as a
  documented degraded fallback when supersede is unavailable.
- **Repo memory:** small project conventions and verified commands.
- **Chronicle/session history:** recovery evidence for crashed or incomplete
   sessions; distill durable conclusions into MemPalace rather than treating raw
   chat history as permanent memory.
- **Handoff and active plan:** current task state and exact stopping point.
- **Live files, language services, commands, and tests:** current code truth.

Never use a mined drawer or old diary entry as proof that a current endpoint,
signature, implementation, or test still exists. Verify those in the working
tree.

## Session End

After every substantial session:

1. Update the active plan and `docs/HANDOFF.md`.
2. Prefer one atomic `mempalace_checkpoint` as agent `copilot`, with every item
   and the nested diary set to wing `blackroom_console`, using concise AAAK; use
   explicitly scoped `mempalace_diary_write` and `mempalace_add_drawer` as a documented
   fallback only when the atomic operation is unavailable.
3. Add or update knowledge-graph facts with `kg_supersede` when a single-valued
   fact changed, after verifying the live tree.
4. Keep ephemeral command output and current code state out of durable memory.
5. Update repo memory when a small convention or verified command changed.

## Degraded Mode

If MemPalace is unavailable:

1. State `MEMORY DEGRADED` and the failed operation.
2. Continue only from the handoff, plan, repo memory, Git, and live code.
3. Do not claim historical completeness.
4. Record pending memory reads or writes in the handoff.
5. Retry `/memory-health` before ending the session; do not silently discard
   durable synthesis.

The workflow configurator's strong protocol guard checks only local handoff,
plan, and change-isolation facts. It never reports external memory success.
Copilot, VS Code, and cloud hook event names and timeout behavior differ, so
the guard is an optional reviewed prompt rather than a fail-safe substitute for
the live memory lifecycle or human review.
