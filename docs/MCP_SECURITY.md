# MCP Threat Model

MCP tools run with real authority. Read-only data mutation controls do not imply
filesystem, confidentiality, availability, or prompt-injection safety. Enable
only the server and tools needed for the current task.

## Shared Controls

- Review publisher, source, package pin, command, arguments, and requested
  credentials before trusting a server.
- Keep default approvals. Review both tool inputs and external tool outputs;
  output can contain prompt injection.
- On macOS and Linux, retain `sandboxEnabled` for local stdio servers. The
  generated sandbox denies workspace writes and common credential paths.
- Keep user-profile MCP configuration folder-independent. VS Code evaluates it
  in empty windows, where `${workspaceFolder}` cannot resolve; put servers that
  require workspace paths or workspace-specific sandbox rules in
  `.vscode/mcp.json` instead.
- On Linux, sandboxed MCP startup requires `bwrap`, `socat`, and `rg` on VS
  Code's inherited `PATH`. Ubuntu's
  `kernel.apparmor_restrict_unprivileged_userns=1` can deny `CAP_SYS_ADMIN` to
  the nested `apply-seccomp` helper even when Bubblewrap itself works. Prefer a
  reviewed AppArmor policy; for trusted local servers, the explicit
  `--without-mcp-sandbox` installer fallback is narrower than disabling that
  kernel restriction globally. Unsandboxed MCP tools must retain normal tool
  confirmations.
- Package pins constrain the top-level package, not all transitive artifacts.
  Prefer persistent isolated `uv tool` installs and direct executable paths in
  local VS Code configuration; runtime startup then needs no PyPI access.
  Generated project-local `uvx` configurations remain portable, but their first
  startup contacts PyPI and writes the uv cache. For stronger supply chain
  control, preinstall from a reviewed lock or internal registry and narrow
  network access afterward.
- Never point an agent at production merely because a server has a restricted
  mode. Use least-privilege identities, replicas, query limits, audit logs, and
  separate production change processes.

## Configurator Read-Only Audit

Analyze inspects proposed and existing VS Code MCP configuration without
starting a server. It reports:

- possible plaintext credentials without returning their values;
- explicit command-shell wrappers;
- floating `uvx`/`npx` download-on-start packages;
- unreviewed server names;
- missing local-stdio sandbox flags;
- non-HTTPS remote endpoints;
- selected servers missing from workspace configuration;
- duplicate workspace/user-profile registration.

Findings are review signals, not proof of exploitation or package trust. Regex
checks cannot validate transitive dependencies, publisher identity, runtime
behavior, server responses, or whether a credential has already leaked. Keep
tool confirmation, source review, restricted identities, and runtime validation.

## MemPalace

**Exposure:** MemPalace contains durable project synthesis, decisions, diaries,
and temporal facts. Write, update, invalidate, and delete tools can corrupt
history or retain sensitive information longer than intended. Mined code drawers
can become stale.

**Minimum controls:** store synthesis rather than secrets or transient logs;
query before historical claims; invalidate changed KG facts; verify current code
against the live tree; review destructive memory operations; and maintain a
tested backup/recovery process for the palace database. Keep health checks
read-only and never create probe drawers or facts.

## codebase-memory-mcp

**Exposure:** the server reads trusted repositories, persists structural
indexes, and its installer can modify user-level agent configuration, skills,
hooks, and instructions. Graph results can be stale or incomplete when files are
excluded, generated, unsupported, or only partially parsed.

**Minimum controls:** audit the installer and signed/checksummed release; index
only trusted roots; review user-configuration changes; check project/index
freshness and path coverage; paginate exhaustive queries; and verify exact code
against live files and language services. Commit shared graph artifacts only
after accepting their repository and merge-policy implications.

## DuckDB / MotherDuck

**Exposure:** SQL can read local files and load extensions; expensive scans can
consume memory, CPU, and disk. DuckDB read-only mode prevents database writes,
not arbitrary filesystem reads available through SQL features.

**Minimum controls:** retain filesystem sandboxing and omit
`--allow-switch-databases`. An in-memory database requires `--read-write`; treat
it as ephemeral scratch state that disappears when the server stops, keep
workspace writes denied by the sandbox, and bound rows/chars/time. For a
persistent local `.duckdb` file, use an absolute path and omit `--read-write` for
the server's read-only default. Expose only approved data paths and do not add
cloud credentials unless remote data access is required.

## Postgres MCP

**Exposure:** schema and rows may be confidential; unrestricted or privileged
roles can mutate data; read queries can still lock, scan, or exhaust resources.
Restricted mode is defense in depth and may not constrain unsafe server-side
procedures or database-specific extensions.

**Minimum controls:** use a database-enforced read-only role against a dev
database or replica, set statement and resource limits, keep credentials in the
secure input store, and allowlist only the database hostname in the MCP sandbox.
Hypothetical-index analysis requires reviewed database extensions.

## MarkItDown MCP

**Exposure:** accepts file and network URIs, parses complex untrusted formats,
and returns document content that may contain prompt injection. The reviewed MCP
package pin is a prerelease.

**Minimum controls:** sandbox filesystem and network access, deny credential
paths, convert only known files, review output before model use, and do not bind
its unauthenticated HTTP transport beyond localhost.

## Context7

**Exposure:** library identifiers, queries, and selected project context leave
the machine; indexed documentation can be stale, incorrect, or adversarial.

**Minimum controls:** send no secrets or proprietary source, prefer versioned
documentation, review returned examples, and verify security-critical APIs
against the upstream project.

## Hugging Face

**Exposure:** the server can expose more than public search, including community
Spaces and infrastructure jobs. Tools can trigger compute spend, execute remote
applications, upload data, or return untrusted model and repository content.

**Minimum controls:** configure the Hugging Face MCP settings page to enable
search/detail tools only unless mutation is explicitly required, use a minimal
token scope, disable dynamic community Spaces by default, and never upload
private data without a separate approval and data-handling review.
