# exp08 with the real daemon grab, rerun: PASS (FEAS-E run A)

Harness change since `2026-10-01-2`: the verdict tally is taken while the grab is still held, and the
daemon's reads must also cover the tally window (`reads_at_arm`, at least 15 inside the window).
Operator-supervised, built-in layout (event2-5), `remote-emergencyd --enable-grabs` started by hand as the
operator uid, ACLs on event2-5, kill timer armed (1 s accuracy), gnome-remote-desktop masked.

- Baseline on the page: 11 physical key downs, 96 pointer moves (observer works).
- Daemon: isolated 4 nodes; 304 reads from 2 nodes in total, 290 of them inside the tally window
  (14 at the reset); still `isolated` at the end; no pushed release; restored.
- Page tally in the window: only the injected input: ShiftLeft 2/2, ControlRight 1/1, KeyA 1/1,
  ArrowLeft 1/1, one click, pointer path (850,660) (890,660) (850,660) (840,660), one wheel event,
  nothing untrusted, no repeats. Nothing from the 290 physical reads reached the page.
- Shell PID 6842 before and after; gnome-remote-desktop inactive; revoked stage refused `LeaseRevoked`,
  post-stop stage refused `MutterUnavailable`.

Limits: the daemon reports counts only, so which nodes the operator used is inferred (2 active nodes of
4); one run on one layout; `git_dirty` is true in `findings.json` because this harness change was
uncommitted when it ran (committed afterwards). The page lost and regained fullscreen once (250 ms)
during the baseline, before the tally window.
