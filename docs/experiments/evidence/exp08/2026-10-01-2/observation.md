# exp08 with the real daemon grab, first run: FAIL by the strict rule, not a grab leak

Operator-supervised, built-in layout (event2-5), `remote-emergencyd --enable-grabs` holding all four nodes.
Baseline: 4 physical key downs, 168 pointer moves on the page (the observer works). Daemon: isolated 4
nodes, 741 reads from 2 nodes, still isolated at the end, no pushed release, restored. Every injected
stage was seen exactly (ShiftLeft 2/2, ControlRight 1/1, KeyA 1/1, ArrowLeft 1/1, click, pointer
+40/-40/-10, one wheel event) and the page saw no physical pointer move.

Failure: one extra `KeyF` down (no up) at page time 38.78 s. The stage timings put the release at about
38.33 s (last stage at 4.97 s after the tally reset at page time 30.36 s, plus the fixed 3 s of
operator typing, plus the status call), so the key is about 0.45 s after the release, when the operator
had been told to type until the prompt cleared. The old harness took the verdict tally about 0.5 s after the
release (the fixed 3 s of typing came before it, then two page heartbeats), so it counted the key, and the
record has no release timestamp: the explanation is reconstructed from stage timings. Treated as a harness
defect (verdict window), not as evidence either way. The rerun is `2026-10-01-3`.
