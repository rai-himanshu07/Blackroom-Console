# Offline Emergency Authority Matrix

**Status:** simulation only; no evdev observer, system service or live GNOME
recovery. FEAS-G and the Phase 10 Go/No-Go decision remain open.

| Operation | Hostd alive | Hostd dead | Agent dead |
|---|---|---|---|
| Trigger | Separate `offline-emergency --offline-sim-emergency` process writes an owner-only, fsynced stop marker; hostd/browser not required. | Same. | Same. |
| Authority | Independent process advances the persisted security epoch; hostd checks marker before commands and exits. | Persisted marker and epoch deny restart. | Persisted marker and epoch deny restart. |
| Agent fake recovery | Agent polls marker, revokes input and verifies synthetic teardown, or handles host socket EOF. | Marker poll works with socket still open, including stalled hostd. | No running agent to inspect physical state; gateway shows `FAILED_SAFE`. |
| Gateway/browser | Gateway refuses Start/input and reports `FAILED_SAFE` plus epoch; browser polls status. | Same. | Same. |
| Real lock/display/input | Not implemented by this process. | Not implemented. | Not implemented. |

The marker is intentionally not automatically cleared. Agent-reported
unverified recovery also writes it before hostd exits. This is an offline
stop fence, **not** a certified independent emergency recovery service:
the components run under one UID, the fake's physical state dies with its
process, there is no hardware chord detector, no deployed ACL, and no
operator-approved real restore/lock path. A future independent daemon and
real recovery verifier must resolve those gaps before any live activation.