# PoC security findings (draft)

**Status:** DRAFT 2026-10-03, from the code and the live runs up to that date. Findings, not fixes; the owner
decides which become work items. "Observed" means seen on this laptop.

| # | Finding | Evidence | Severity for a home-LAN, single-owner use |
|---|---|---|---|
| 1 | Same-uid boundary: any process of the user can unlock the session (`loginctl unlock-session`, observed) and, while the extension is enabled, open a remote session on the locked screen. | exp12; extension | Accepted by the owner; matters only if untrusted code runs as the user. |
| 2 | With the extension enabled, locking is not a kill switch. The kill switches are the chord, Stop, heartbeat loss and an SSH kill. | README, amendment addendum | Documented; enable only while using the console. |
| 3 | The web authority is one random token (URL then HttpOnly SameSite=Strict cookie, constant-time compare, same-origin check). No TOTP, no per-device identity, no rate limit on the token. | `server.rs` | Acceptable on a trusted LAN; Phase 11 replaces it. |
| 4 | The console listens on `0.0.0.0` by default (http and self-signed https); the token travels in the URL and, over plain http, in the clear. | `main.rs` | Use https; bind to the LAN address if needed. |
| 5 | Input is validated (key range, a blocked-key list, pointer and scroll limits) but a logged-in tablet can type anything else, including shell commands, once the desktop is unlocked. | `console.rs` | By design: it is a remote console. |
| 6 | Physical input isolation uses `EVIOCGRAB` through a daemon running as the user with ACLs on four nodes granted by `sudo setfacl`; the user is not in `input`. A same-uid process can open those nodes while the ACLs exist. | exp09, `docs/ops/console.sh` | Accepted; ACLs are removed after each run. |
| 7 | The panel is blank only through DPMS and a virtual-only topology; a failed restore leaves it black until the watchdog (60 s) or SSH. | exp06/07, run 13 | Mitigated by the dead-man restore (observed once). |
| 8 | A console killed mid-Stop (SIGHUP, live) left the restore watchdog armed; the restore then ran 61 s later and locked. Fixed by registering the signal handlers first; one live re-run stopped cleanly in 0.8 s. | runs 13 and 14, `last_stop.json` | Fail-safe worked; the root cause is not proven. |
| 11 | After a console crash Mutter returns the panel to the desktop at once; until the 60 s dead-man restore it used to stay unlocked (run 18). A crash guard (a transient service that waits for the console pid and only locks) now locks within milliseconds (run 20). Its first version also restored the display and crashed the Shell (run 19). | runs 18, 19, 20 | Exposure reduced from about 60 s to a flash; one run. |
| 9 | The state directory (`$XDG_RUNTIME_DIR/blackroom-console`) holds the display backup, `last_stop.json`, `recovery.json` and the token URL, all mode 0600 in a 0700 directory. | `display.rs` | Low; tmpfs, same uid. |
| 10 | The emergency daemon and its client use unix sockets only and link only libc-level libraries (tested). | `tests/process.rs` | Low. |

Not assessed: dependency CVEs beyond `cargo deny`/`cargo audit` at each gate, TLS certificate pinning (self-signed,
exception clicked per browser), a hostile client on the LAN with the token, denial of service by a LAN client
holding the single session, and anything involving PAM or polkit (Phase 11, after Go).
