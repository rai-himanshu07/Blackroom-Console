# Agent Surface Compatibility

Verified against GitHub and VS Code documentation on 2026-08-01. These products
change independently; run the live checks below after upgrades.

| Customization | VS Code Local / Agent Host | Copilot CLI | Copilot cloud agent | Code review |
|---|---|---|---|---|
| `AGENTS.md` | Loaded when enabled | Loaded | Loaded | Support varies by review surface |
| `.github/copilot-instructions.md` | Loaded | Loaded | Loaded | Primary portable review guidance |
| `.github/instructions/` | Path/task scoped | Loaded | Loaded | Supported on GitHub; local review support varies |
| `.github/agents/` | Supported; policy/model availability applies | Supported | Supported where custom agents are enabled | Not a review-policy substitute |
| `.github/skills/` | Supported | Supported | Supported | GitHub review support is preview |
| `.github/hooks/` | Preview | Supported | Supported event subset in ephemeral Linux | Do not assume support |
| VS Code user-profile `mcp.json` | Loaded for every workspace using that profile | **Not loaded** | **Not loaded** | **Not loaded** |
| `.vscode/mcp.json` | VS Code only | **Not loaded** | **Not loaded** | **Not loaded** |
| `.mcp.json` / `.github/mcp.json` | Not the VS Code workspace schema | Loaded by CLI when trusted | Configure cloud MCP separately | Configure review MCP separately |
| `.github/prompts/*.prompt.md` | Local harness only in VS Code 1.129 | Not portable | Not portable | Not portable |

## Agent Host Boundaries

- Agent Host sessions can continue without an editor client. Extension tools and
  integrated-browser tools generally require a connected VS Code window whose
  extension host contributed them.
- Files, commands, and MCP processes run where the Agent Host is located. For a
  remote host, install runtimes and credentials on that host, not only locally.
- Skills are the preferred portable slash workflow. Prompt files remain useful
  only when Local Agent is an explicit requirement.

## MCP Portability

The personal default is to register folder-independent MCP servers once in the
active VS Code user profile via **MCP: Open User Configuration**. Profiles have
independent MCP configuration, so repeat that setup only for another profile
that actually needs the servers. A user-profile MCP file must not reference
`${workspaceFolder}` because VS Code also evaluates it in windows with no folder
open. Use **MCP: List Servers** to enable or disable servers by workspace instead
of duplicating their definitions.

The installer generates `.vscode/mcp.json` only when `--mcp` is supplied. Use
that project-local scope for DuckDB, MarkItDown, Postgres, shareable
project-specific database paths, credential prompts, remote hosts, or sandbox
policies. This preserves workspace-relative paths and write restrictions without
breaking empty VS Code windows. Do not register the same server in both user and
workspace scope. The bundled project doctor validates only repository-local
`.vscode/mcp.json`; inspect user-profile servers through VS Code.

Copilot CLI does not read either VS Code MCP file. Its repository configuration
uses `mcpServers`, requires explicit `tools` filters, and expands environment
variables instead of VS Code input prompts.

Do not mechanically copy the VS Code file. For CLI, add each server deliberately
with `copilot mcp add`, use an environment variable for credentials, select the
smallest tool list, then inspect it with:

```bash
copilot mcp list --json
copilot plugins list --kind mcp --kind skill --json
```

## Discovery Checks

1. Run `/project-doctor --live` or the bundled doctor script with `--live`.
2. In VS Code, open **Chat: Open Customizations** and inspect each surface.
3. Open **Developer: Open Agent Debug Panel** to see loaded files and failures.
4. In Copilot CLI, run `/env`; use `/agent`, `/skills`, and `/hooks` for the
   surfaces omitted from `copilot plugins list`.
5. In a monorepo opened below its Git root, enable
   `chat.useCustomizationsInParentRepositories` only after trusting the parent.

For behavioral quality rather than discovery alone, the optional **Chat
Customizations Evaluations** extension can analyze instruction, agent, and skill
contradictions and run Waza evaluations for skills. Treat its model-based output
as review evidence, not a deterministic gate.
