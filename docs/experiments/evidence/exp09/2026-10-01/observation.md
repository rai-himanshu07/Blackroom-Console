# exp09_freeze: a frozen daemon under a watchdog supervisor (FEAS-E run B): PASS

Operator-supervised, built-in layout (event2-5), tablet SSH open, temporary ACLs, an external kill timer
(1 s accuracy, 900 s) armed, gnome-remote-desktop masked. The daemon ran as a transient user unit:
`systemd-run --user --unit=blackroom-live-daemon -p Type=notify -p NotifyAccess=main -p WatchdogSec=10
-p WatchdogSignal=SIGKILL .../remote-emergencyd --client-uid 1000 --enable-grabs --socket <sock>`. The shipped
unit file (`systemd/system/remote-emergencyd.service`) was not loaded.

- The probe isolated 4 nodes (lease 30 s), renewed 8 s, verified the daemon (uid, comm `remote-emergenc`),
  SIGSTOPped only that process and waited on the control socket.
- The connection closed after 9883 ms (target 30 s or less); the probe did not need its 60 s fallback kill.
- `watchdog-journal.txt`: at 19:09:07 systemd logged `Watchdog timeout (limit 10s)`, killed process 3597316
  with SIGKILL and ended the unit with result `watchdog`. So the supervisor, not the probe or the kill timer,
  ended the daemon.
- `unit-analysis.txt`: read-only `systemd-analyze verify` (only the missing installed binary) and
  `security --offline` (0.8 SAFE) for the unit template; a static score, not evidence of operation.

Limits: what is measured is the control connection closing, which follows from the process dying (the kernel
drops its evdev grabs); the pointer returning to the operator was not recorded by the probe and the operator could not recall it (a rerun was offered and not needed for the decision). The release bound
comes from the heartbeat interval (5 s) plus `WatchdogSec=10`, so about 5 to 10 s is expected and 9.9 s is near
the top. One run, one layout, same-uid privilege (operator-accepted), nothing installed.
