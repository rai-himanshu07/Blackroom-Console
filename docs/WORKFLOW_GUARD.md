# Local Workflow Guard

Strong rigor may install `workflow_guard.py` for the supported `Stop` and
`agentStop` lifecycle surfaces. It checks only local handoff/plan presence and
whether workflow-control files are in the Git diff. It cannot verify a remote
service, prove a MemPalace checkpoint, or replace an independent review.

VS Code and Copilot CLI/cloud use different event names and hook support is
preview/policy-dependent. The generated configuration includes both names so
the host can select its supported surface; inspect diagnostics after setup.
Command hooks are bounded by a short timeout and a host may fail open on
timeout. Treat `ask` as a review prompt, not a security guarantee. Keep the
guard readable, review changes to it, and run the project doctor.
