# Local Workflow Guard

The local Stop guard is disabled: `.github/hooks/workflow_guard.json` has no
`Stop` or `agentStop` registrations. No automatic project doctor or workflow
guard runs on session stop. `workflow_guard.py` and the read-only project
doctor remain available for explicit troubleshooting only.

VS Code and Copilot CLI/cloud use different event names and hook support is
preview/policy-dependent. If a Stop hook is re-enabled later, review its
scope and host behavior explicitly; a hook prompt is not a security guarantee.
Live experiment recovery remains governed by `docs/ops/experiment-safety.md`,
not by this optional workflow hook.
