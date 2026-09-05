Experiment: Experiment 2 — Mutter Capability Inventory
Date: 2026-09-05T02:39:02.541163195Z
Environment: Development workstation (host = target), Ubuntu 26.04 / GNOME 50.1
Objective:
Determine which relevant Mutter/Shell/logind D-Bus interfaces are actually available on this host and produce an API inventory (Document 10 §9).

Hypothesis:
org.gnome.Mutter.{DisplayConfig,RemoteDesktop,ScreenCast,InputCapture,InputMapping,ServiceChannel,IdleMonitor}, org.gnome.Shell.ScreenShield, org.gnome.ScreenSaver, and org.freedesktop.login1 are introspectable read-only without creating any session or object.

Procedure:
Call org.freedesktop.DBus.Introspectable.Introspect on each target, save the raw XML, parse interfaces/methods/properties/signals, read the RemoteDesktop/ScreenCast Version properties, call DisplayConfig.GetCurrentState, and derive Document 20 §8 capability presence flags. Never call RecordVirtual, ConnectToEIS, ApplyMonitorsConfig, or InputCapture.CreateSession.

Expected:
All 9 Mutter/Shell/ScreenSaver targets plus login1 Manager and the selected Session introspect successfully; GetCurrentState returns the live display topology; no session or object is created.

Observed:
Introspected 13/13 targets successfully.
RemoteDesktop.Version=Some(1), ScreenCast.Version=Some(4)
DisplayConfig.GetCurrentState: 2 connector(s), 1 logical monitor(s)
Finding: org.gnome.Shell.ScreenShield exposes no distinct interface; it resolves to org.gnome.ScreenSaver at /org/gnome/ScreenSaver (see api-inventory.md).
Capability presence flags: OS_SUPPORTED=N/A, GNOME_SUPPORTED=N/A, WAYLAND_SUPPORTED=N/A, SYSTEMD_SUPPORTED=N/A, SESSION_FOUND=N/A, MUTTER_CAPABLE=AVAILABLE, REMOTE_DESKTOP_CAPABLE=AVAILABLE, SCREENCAST_CAPABLE=AVAILABLE, PIPEWIRE_CAPABLE=N/A, VIRTUAL_DISPLAY_CAPABLE=UNKNOWN, DISPLAY_CONFIG_CAPABLE=AVAILABLE, REMOTE_INPUT_CAPABLE=UNKNOWN, PHYSICAL_INPUT_ISOLATION_CAPABLE=AVAILABLE, SESSION_LOCK_CAPABLE=AVAILABLE, EMERGENCY_CAPABLE=N/A, GPU_CAPABLE=N/A

Evidence:
- inventory.json (this directory)
- docs/gnome/introspection/*.xml
- docs/gnome/api-inventory.md

Result:
PASS

Failure:
(none)

Root Cause:
(none)

Security Impact:
None: read-only Introspect/Get/GetCurrentState only. No session, virtual monitor, EIS connection, or monitor-config change was created.

Recommended Action:
(none)

Follow-up:
Research findings (Document 00 §50 topics) in docs/gnome/feasibility-research.md; capability report and architecture decisions (step 10).
