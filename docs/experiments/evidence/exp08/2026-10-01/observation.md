# Exp 8 run 4: remote input with printable and arrow keys and a captured pointer start: PASS

Date: 2026-10-01 local. Operator present, tablet SSH open, work saved, `gnome-remote-desktop` masked then
unmasked (inactive and disabled as before), Firefox observer page focused and fullscreen, nothing touched
during injection. Shell PID 6842 before and after. Binary built from `ea67973` plus the uncommitted run 4
evaluator change (git dirty recorded true). `a` (evdev 30) and Left (105) were checked read-only against
gsettings and xkb first: no bare binding, no screen reader.

## Result: PASS by rule

All 13 stages matched. The page's own tally (cleared at arming, every event trusted, `untrusted` 0):

- Shift tap and Shift + Right Ctrl chord (Right Ctrl arrived with Shift held): ShiftLeft 2/2, ControlRight 1/1.
- `a` (KeyA 1/1) and Left (ArrowLeft 1/1), no modifier, no repeat.
- Pointer path from a start the page captured: 381, 421, 381, 371 (x; y 90 throughout), i.e. steps +40, -40,
  -10 for the injected +10, +40, -40, -10 moves, inside the 1920x1080 viewport. This measures the +40 step
  that run 3 only inferred.
- One left click; one scroll event (a 15.0 request again gave `deltaY` 207, uninvestigated).
- The send after authorization revoke was refused as `LeaseRevoked`; the send after `Stop` was refused as
  `MutterUnavailable`; neither delivered anything. After `Stop` Mutter closed the EIS channel
  (`eis_ready_after_stop` false) and the old session path answered "Object does not exist".

## What this closes and what it does not

- Closes the two evidence gaps the 2026-10-01 review named for FEAS-D: a printable key and a non-modifier key
  reaching the session, and a measured (not inferred) pointer step.
- Does not close the deferred limits: input was not routed to a virtual monitor, authority was a fake
  in-process lease, owner-loss teardown, absolute mapping and cursor shape (Exp 28), other target apps,
  right/middle button, drag and horizontal scroll. The revoke refusal is still the local authorization check.
- Correction to run 3: its "net-zero pointer motion" was inferred from two recorded positions (725, 685);
  the start position was never captured. Run 4's leading +10 move fixes that.
