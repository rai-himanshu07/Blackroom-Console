//! Mutter/logind integration. `session`/`capability` (Phase 3) are strictly
//! read-only. `remote_desktop`/`screencast`/`virtual_monitor`/
//! `pipewire_capture` (Phase 4, assessment C2: mock-first ends here for
//! RemoteDesktop/ScreenCast/PipeWire session mechanics) make real
//! `CreateSession`/`Start`/`Stop`/`RecordMonitor`/`RecordVirtual` calls plus
//! real PipeWire frame capture, evidence-cited to Experiments 3–4
//! (`docs/experiments/evidence/exp0{3,4}/`). `display_config` (Phase 5)
//! adds the first `ApplyMonitorsConfig` call that disables a physical
//! output, evidence-cited to Experiments 6–7/26–27/37
//! (`docs/experiments/evidence/exp{06,07,26,27,37}/`). Still no
//! `ConnectToEIS` call anywhere in this module tree — that remains Phase 6.

pub mod capability;
pub mod display_config;
pub mod eis;
pub mod pipewire_capture;
pub mod remote_desktop;
pub mod screencast;
pub mod session;
pub mod virtual_monitor;
