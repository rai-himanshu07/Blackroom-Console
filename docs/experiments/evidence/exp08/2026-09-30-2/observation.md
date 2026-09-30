# Experiment 8 run 2 (2026-09-30): FAIL by the coded rule, one inconclusive check

Generated `report.md` and `findings.json` are unchanged (code at commit `27e6544`,
tree clean). The only violation is the page-tally pointer check; every stage matched.

## Established (page tally, all events trusted, 0 untrusted)
- Same setup as run 1 succeeded again on the existing session (Shell PID 6842
  before and after): `CreateSession`, `Start`, `ConnectToEIS`, Sender handshake,
  virtual pointer (pointer, scroll, button) and virtual keyboard resumed.
- Keyboard: Shift tap arrived (down with shiftKey, up). Chord arrived in order:
  ShiftLeft down, ControlRight down with shiftKey true, ControlRight up,
  ShiftLeft up. Exactly two Shift downs, no other keys.
- Click: one left click (down, up, click, button 0).
- Scroll: one wheel event, deltaY 207, page scrolled 207 px for a 15.0 request
  (about 14x; scaling not investigated).
- Revoke: `key_tap_after_authorization_revoked` refused as `LeaseRevoked`; the tally
  has no third Shift down, so nothing arrived.
- Stop: accepted. Mutter then sent `DeviceRemoved` x2, `SeatRemoved`,
  `Disconnected` (reason Disconnected) and closed the socket;
  `eis_ready_after_stop` false. The post-stop tap was refused as `MutterUnavailable`
  (the client already knew the connection was gone) and again nothing arrived.
  `Stop` on the old session path: "Object does not exist".
- `gnome-remote-desktop` masked for the run and restored to inactive/disabled;
  no stuck input flagged by the run (`stuck_input_suspect` false).

## Inconclusive: pointer magnitude
The page recorded 2 mousemove events, summed `movementX` -40, `absX` 40. The
rule wanted net 0 and abs 80, so it reported a mismatch. Two readings fit and
this run cannot tell them apart: (a) the browser reported the first move
(+40) as `movementX` 0 because it has no previous position, then -40; (b) the
+40 was clamped at a screen edge and only -40 moved. So relative motion was
delivered (2 events) but its size and return to the origin are not shown. The
check was measuring the wrong thing, not the transport failing.

## Fix made after the run
The page now records each move's `clientX/clientY` and the viewport, and the
evaluator requires exactly two moves, the second 40 px left of the first with
the first inside the viewport (which also rules out an edge clamp). Checked with
trusted mouse moves in a browser rehearsal; nothing else changed. No run 3 has
been done or approved.

## Not shown
Pointer magnitude; behaviour with fractional scaling, other apps, GPU/cursor
variants; any product-path (hostd/gateway) integration. This is not a FEAS-D
decision; that is plan Step 4 after independent review.
