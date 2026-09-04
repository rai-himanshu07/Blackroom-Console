# Python Environment Policy

Conda and uv are both supported, but they are alternatives for project
dependency ownership. Use exactly one manager per project and never mix their
add, sync, or update operations.

## Decision Order

1. **Existing project declarations win.**
   - `environment.yml`, `environment.yaml`, or `conda-lock.yml` selects Conda.
   - `uv.lock` or a `[tool.uv]` section in `pyproject.toml` selects uv.
   - If both sets of markers exist, stop. Inspect project history and ask which
     declaration is authoritative before changing dependencies.
2. **New ML, LLM, analytics, or data-engineering project:** use
   Miniforge/Conda with Python 3.12. Conda is the personal default because CUDA,
   native libraries, and vendor packages commonly participate in these stacks.
3. **New pure-Python utility, service with pure-Python dependencies, or CLI:**
   uv is acceptable when its speed and lockfile workflow are useful.

Record the selected manager, environment name, and exact create, update, and run
commands in `AGENTS.md` once initialized.

## Conda Contract

- Prefer Miniforge and the `conda-forge` channel.
- Commit `environment.yml`; commit `conda-lock.yml` when the project uses
  conda-lock for platform-specific reproducibility.
- Put PyPI-only packages under the `pip:` subsection of `environment.yml` so
  one file still owns the complete environment. Do not run an unrecorded
  `pip install` inside the environment.
- Typical commands:

  ```bash
  conda env create -f environment.yml
  conda env update -f environment.yml --prune
  conda activate ENV_NAME
  conda run -n ENV_NAME pytest -x -q
  ```

- Read `name:` from `environment.yml` rather than guessing the environment name.

## uv Contract

- Commit `pyproject.toml` and `uv.lock`.
- Add and remove dependencies through uv so project metadata and the lockfile
  remain synchronized.
- Typical commands:

  ```bash
  uv sync
  uv add PACKAGE
  uv remove PACKAGE
  uv run pytest -x -q
  ```

## `uv tool` And `uvx` Are Separate Plumbing

The personal default is to install frequently used local MCP executables once
with `uv tool install`. Each tool gets a persistent isolated environment,
normally under `~/.local/share/uv/tools`, while its command is exposed through
`~/.local/bin`. This keeps MCP dependency trees out of Conda base and project
environments. Installation scope and VS Code registration scope are separate:
folder-independent services can be user-profile MCP entries, while data/file
servers that need `${workspaceFolder}` remain workspace entries.

```bash
uv tool install 'mcp-server-motherduck==1.0.7'
uv tool install 'markitdown-mcp==0.0.1a4'
uv tool dir --bin
```

The installer's optional project-local MCP configuration instead uses `uvx` to
launch pinned DuckDB, Postgres, and MarkItDown servers in isolated cached
environments. This is the portable fallback when direct persistent executables
are unavailable. Keep these servers workspace-scoped when they use a
project-specific database, credential prompt, remote agent host, or sandbox
policy. Do not register the same server in both the VS Code user profile and
`.vscode/mcp.json`.

Neither `uv tool` nor `uvx` selects uv as the project's environment manager or
modifies a Conda project.

For a Conda-first machine, install the uv binary alongside Miniforge only as MCP
tool plumbing:

```bash
conda install -n base -c conda-forge uv
```

If a machine must be uv-free, skip local MCP servers or preinstall their pinned
packages in a dedicated Conda environment and manually point MCP commands to
those binaries. That manual configuration owns package updates. Persistent
direct executables do not need the uv-cache write and PyPI bootstrap allowances
used by generated `uvx` configurations.
