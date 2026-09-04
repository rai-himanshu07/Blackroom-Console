Experiment: Experiment 0 — Environment Discovery
Date: 2026-09-04T19:42:56.054332514Z
Environment: Development workstation (host = target), Ubuntu 26.04 / GNOME 50.1
Objective:
Produce a reproducible, non-mutating environment report (Document 10 §7).

Hypothesis:
OS/GNOME/Mutter/PipeWire/GPU/session facts can be collected entirely read-only via dpkg-query, /sys, /proc, and --version flags.

Procedure:
Run `exp00_environment`; read /etc/os-release, dpkg-query, /sys/class/drm, /proc/modules, /proc/bus/input/devices; query XDG_SESSION_TYPE and the gnome-remote-desktop user unit state.

Expected:
A complete, deterministic report; no system file outside the evidence directory changes.

Observed:
OS: Ubuntu 26.04.1 LTS
Kernel: 7.0.0-30-generic
GNOME Shell: GNOME Shell 50.1
Mutter: 50.1-0ubuntu2.2
GPUs: card0(nvidia), card1(i915)
Connected outputs: card0-HDMI-A-1=connected, card1-DP-1=disconnected, card1-DP-2=disconnected, card1-eDP-1=connected
Input devices found: 23
XDG_SESSION_TYPE: wayland
gnome-remote-desktop unit: inactive

Evidence:
- environment.json (this directory)

Result:
PASS

Failure:
(none)

Root Cause:
(none)

Security Impact:
None: read-only. No secrets, passwords, or device serials collected.

Recommended Action:
(none)

Follow-up:
Experiment 1 — GNOME Session Discovery.
