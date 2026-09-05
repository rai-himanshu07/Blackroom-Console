Experiment: Experiment 3 — Basic Screen Capture
Date: 2026-09-05T11:34:02.331376019Z
Environment: Development workstation (host = target), Ubuntu 26.04 / GNOME 50.1
Objective:
Prove that the existing GNOME session can be captured over ScreenCast + PipeWire, before touching display topology (Document 10 §10).

Hypothesis:
org.gnome.Mutter.ScreenCast.CreateSession plus one of RecordMonitor/RecordWindow/RecordArea on the returned session object starts a capture of the existing desktop that PipeWire delivers as real video frames.

Procedure:
CreateSession(); introspect the session object; call the first available capture method for the primary connector; introspect the stream object; subscribe to PipeWireStreamAdded; Start(); connect a PipeWire stream to the reported node id and receive frames; Stop(); verify the session object is gone.

Expected:
A stream object is created, PipeWireStreamAdded reports a node id within 10s, at least one real frame arrives within 8s, and Stop() leaves the session object unreachable.

Observed:
connector=eDP-1
capture attempts: [CaptureAttempt { method_tried: "RecordMonitor", connector_argument: Some("eDP-1"), succeeded: true, error: None }]
stream_path=Some(OwnedObjectPath(ObjectPath("/org/gnome/Mutter/ScreenCast/Stream/u8")))
frames_received=1
observed_format=Some(CaptureFormat { format: "VideoFormat::BGRx", width: 1920, height: 1080, framerate_num: 0, framerate_denom: 1 })
cleanup_verified=true
stop_error=None
notes:
RemoteDesktop.Session methods: Get, GetAll, Set, Introspect, Ping, GetMachineId, Start, Stop, NotifyKeyboardKeycode, NotifyKeyboardKeysym, NotifyPointerButton, NotifyPointerAxis, NotifyPointerAxisDiscrete, NotifyPointerMotionRelative, NotifyPointerMotionAbsolute, NotifyTouchDown, NotifyTouchMotion, NotifyTouchUp, EnableClipboard, DisableClipboard, SetSelection, SelectionWrite, SelectionWriteDone, SelectionRead, ConnectToEIS, SetKeymap, SetKeymapLayoutIndex
RemoteDesktop.Session Stop() (without Start()): org.freedesktop.DBus.Error.Failed: Session not started
Introspected ScreenCast.Session methods: Get, GetAll, Set, Introspect, Ping, GetMachineId, Start, Stop, RecordMonitor, RecordWindow, RecordArea, RecordVirtual
ScreenCast.Stream signals: PropertiesChanged, PipeWireStreamAdded
PipeWireStreamAdded node_id=109
Post-Stop() second-Stop()-call reachability probe: same-connection gone=true, fresh-connection gone=true — if both are false, the session object outlives Stop() and this process's own connection (a Phase 9 crash-recovery question, not a Phase 4 defect).

Evidence:
- docs/experiments/evidence/exp03/<date>/report.md
- docs/gnome/introspection/screencast-session-exp03.xml

Result:
PASS

Failure:
(none)

Root Cause:
(none)

Security Impact:
gnome-remote-desktop.service must stay masked for this run (docs/ops/experiment-safety.md §5) to avoid two session owners on the same Mutter interfaces.

Recommended Action:
(none)

Follow-up:
Port the proven mechanics into crates/blackroom-gnome/src/mutter/{remote_desktop.rs,screencast.rs} (Phase 4 plan step 4).
