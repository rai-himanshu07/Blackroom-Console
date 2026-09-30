# Experiment 8 run 1 (2026-09-30): PARTIAL, aborted by the focus gate

Generated `report.md` and `findings.json` are unchanged. Its Security Impact line
says "only inert keys"; that was wrong for F13 (see Root cause) and is corrected here.

## What happened (from `findings.json`, code at commit `8b06546`)
- Operator approved one run, was present, work saved, second-device SSH open.
  `gnome-remote-desktop` masked for the run, unmasked after (inactive/disabled as before).
- The observer page reported focus and fullscreen; the run then did, on the
  selected existing session (id 2, Shell PID 6842 before and after):
  `CreateSession`, `Start`, `ConnectToEIS`, Sender handshake and seat bind all
  succeeded on an input-only RemoteDesktop session (no ScreenCast).
- Mutter advertised and resumed two devices: "Blackroom Console virtual pointer"
  (pointer, scroll, button) and "Blackroom Console virtual keyboard" (keyboard).
- `key_tap_f13` and `key_chord_shift_f13` were accepted by the transport.
- The page then stopped reporting focus+fullscreen, so the gate aborted before
  `pointer_right_40`. No pointer motion, click or scroll was ever injected, and
  the revoked and post-stop stages were not reached.
- `session_stop` accepted. After Stop the client saw `KeyboardModifiers` x2,
  `DeviceRemoved` x2, `SeatRemoved`, `Disconnected` and a closed socket;
  `eis_ready_after_stop` false. A `Stop` on the old session path returned
  "Object does not exist". Shell PID unchanged.

## Page tally (recorded, but polluted by pre-run activity)
Only an F13 key-up (no key-down) and no Shift reached the page. The tally also
holds the operator's own F11 and about 900 mouse moves from before injection,
so its pointer numbers mean nothing. The run now clears the tally right before
injecting.

## Root cause (read-only evidence, not directly observed on screen)
F13 is not inert on this host. evdev 183 maps to XKB `<FK13>`, whose `inet`
symbol is `XF86Tools`, and `gsettings` shows
`media-keys control-center-static ['XF86Tools']`. The injected F13 therefore
launched GNOME Settings (`gnome-control-center --gapplication-service` was still
running afterwards) and took focus from the page. The key-down was consumed by
the shortcut, the key-up reached the page, which explains "F13 up only" and the
focus loss. The gate did its job: nothing further was injected.

## Not shown
Delivery of pointer, click and scroll; non-delivery after a revoke or after Stop
(the stages were not reached); any FEAS-D conclusion. Operator's on-screen
comment: they started late and the page looked fine; they did not report the
Settings window either way.

## Residual state
`gnome-control-center` is still running from the injected key; closing it is
harmless. No stuck input was reported. No repeat was run; a second run needs its
own approval.

## Fixes made after the run
Keys changed to Shift (tap) and Shift + Right Ctrl (chord): no GNOME, IBus or
Firefox binding found for them (`locate-pointer-key` is Left Ctrl, so Left Ctrl
is avoided). The tally resets before injection; the page records focus
transitions; the abort records the observer state.
