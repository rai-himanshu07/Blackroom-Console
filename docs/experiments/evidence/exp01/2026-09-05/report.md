Experiment: Experiment 1 — GNOME Session Discovery
Date: 2026-09-05T02:33:11.102654441Z
Environment: Development workstation (host = target), Ubuntu 26.04 / GNOME 50.1
Objective:
Determine exactly how the active GNOME Wayland session is identified, without guessing from $DISPLAY or process names (Document 10 §8, Document 05 §12-14).

Hypothesis:
Exactly one logind session satisfies Type=wayland, Class=user, Seat=seat0, User=<current uid>, Active=true; the binary fails closed with a classified exit code if WAYLAND_DISPLAY is unavailable.

Procedure:
Call login1 Manager.ListSessions, then Get each Session's Type/Class/ Seat/Active/State/User/Display/Desktop/LockedHint/Scope/Name property; select the unique match; assert XDG_SESSION_TYPE/WAYLAND_DISPLAY/ DBUS_SESSION_BUS_ADDRESS; report the gnome-session/graphical-session user units. With --self-test, also spawn a child with WAYLAND_DISPLAY removed and confirm it fails closed.

Expected:
Exactly one session selected; env assertions pass; the negative test (if run) exits non-zero with SESSION_NOT_FOUND or WAYLAND_UNAVAILABLE.

Observed:
current uid: 1000
session 3 (seat=, list_uid=1000): type=unspecified class=manager seat= active=true state=active name=[USER] => rejected
session 2 (seat=seat0, list_uid=1000): type=wayland class=user seat=seat0 active=true state=active name=[USER] => SELECTED
selected session: 2
XDG_SESSION_TYPE=Some("wayland") WAYLAND_DISPLAY_set=true DBUS_SESSION_BUS_ADDRESS_set=true
gnome-session/graphical-session user units:
UNIT                                    LOAD   ACTIVE SUB     DESCRIPTION
  gnome-session-manager@ubuntu.service    loaded active running GNOME Session Manager (session: ubuntu)
  gnome-session-monitor.service           loaded active running Monitor Session leader for GNOME Session
  gnome-session-basic-services.target     loaded active active  GNOME basic session services
  gnome-session-initialized.target        loaded active active  GNOME Session is initialized
  gnome-session-manager.target            loaded active active  GNOME Session Manager is ready
  gnome-session-pre.target                loaded active active  Tasks to be run before GNOME Session starts
  gnome-session-services.target           loaded active active  GNOME session services
  gnome-session-x11-services-ready.target loaded active active  GNOME session X11 services
  gnome-session-x11-services.target       loaded active active  GNOME session X11 services
  gnome-session.target                    loaded active active  GNOME Session
  gnome-session@ubuntu.target             loaded active active  GNOME Session (session: ubuntu)
  graphical-session-pre.target            loaded active active  Session services which should run early before the graphical session is brought up
  graphical-session.target                loaded active active  Current graphical user session

Legend: LOAD   → Reflects whether the unit definition was properly loaded.
        ACTIVE → The high-level unit activation state, i.e. generalization of SUB.
        SUB    → The low-level unit activation state, values depend on unit type.

13 loaded units listed. Pass --all to see loaded but inactive units, too.
To show all installed unit files use 'systemctl list-unit-files'.
Outcome: Ok
Negative test (env -u WAYLAND_DISPLAY): child exit=Some(3), expected one of ["WAYLAND_UNAVAILABLE(3)", "SESSION_NOT_FOUND(2)"], passed=true

Evidence:
- session.json (this directory)

Result:
PASS

Failure:
(none)

Root Cause:
(none)

Security Impact:
None: read-only login1 introspection. Session/desktop names redacted by default.

Recommended Action:
(none)

Follow-up:
Experiment 2 — Mutter Capability Inventory.
