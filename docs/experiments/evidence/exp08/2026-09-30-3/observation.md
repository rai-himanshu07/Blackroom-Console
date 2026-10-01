# Exp 8 run 3: remote input with a position-based pointer check: passed

Date: 2026-10-01 local. Operator present, SSH open, `gnome-remote-desktop` masked then unmasked, observer page focused and fullscreen, nothing touched during injection. Shell PID 6842 unchanged.

## Result: PASS by rule

All nine stages matched the page's own tally (`tally_matches` true, no violations): Shift tap, Shift plus Right Ctrl chord, pointer +40 and -40 (first position inside the viewport, last minus first about -40), one left click, scroll of 15, a send after the authorization was revoked (refused `LeaseRevoked`, by our own authorization check), `session_stop`, and a send after the session stopped (refused `MutterUnavailable`).

## What this closes and what it does not

- The pointer magnitude question left open by run 2 is observed live: a 40 pixel injected move is seen by the page as a 40 pixel move.
- The revoked-send refusal comes from the project's own authorization check before any write, not from Mutter, so Mutter-side revoke attribution is still not shown.
- One run on one host. FEAS-D and the REMOTE_INPUT_CAPABLE tier are not promoted here; that needs the independent review of Phase 6 plan step 4 (not Phase 7 step 6). Correction 2026-10-01: "net-zero" pointer motion was inferred from two positions (725, 685); the start position was never captured. Run 4 (`../2026-10-01/`) measured it.
