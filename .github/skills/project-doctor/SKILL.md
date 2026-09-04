---
name: project-doctor
description: Diagnose installed agent workflow files, active-plan state, commands, MCP safety, hooks, and context exclusions without modifying the project. Use when setup is new, stale, or not being discovered.
argument-hint: '[--strict] [--json]'
disable-model-invocation: true
---

# Project Doctor

Run the bundled read-only diagnostic from the repository root:

```bash
python .github/skills/project-doctor/scripts/doctor.py --root .
```

Then perform mandatory agent-context checks that a standalone Python process
cannot perform:

  1. When required, call `mempalace_status`,
     `mempalace_diary_read(agent_name="copilot", wing="blackroom_console",
     last_n=3)`, and a small keyword-only search in the same wing.
2. Capability-detect codebase-memory before calling `list_projects`,
  `index_status`, or `check_index_coverage` for the current project. Confirm
  freshness and check one known source file when those operations exist. If a
  requested operation is unavailable, record `CODE GRAPH DEGRADED` and use
  targeted language/text checks instead; never invent a result.
3. Report MemPalace or codebase-memory failure as degraded personal workflow,
  not as a silently skipped optional integration.

Options:

- `--strict` treats warnings as failures for CI.
- `--json` emits machine-readable output.

The Python doctor validates the generated file contract and local task state.
Use the Configurator's read-only Analyze and Memory & Code views for runtime,
environment, and external-service diagnostics.

Do not auto-fix doctor findings. Explain each failed check and let the user
decide whether to edit, merge, or remove project configuration.
