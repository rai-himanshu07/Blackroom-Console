//! Display-side helpers for `RemoteConsole`: the backup file the restore watchdog reads, the
//! rolling restore watchdog, identity checks and the session lock.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use anyhow::Context;
use blackroom_gnome::mutter::display_config::{CONFIGURATION_HASH_VERSION, DisplayBackup};
use serde::{Deserialize, Serialize};
use zbus::blocking::{Connection, Proxy};
use zbus::zvariant::{OwnedValue, Type};

/// Refuses unless the compositor owning DisplayConfig on this bus was started `--headless`.
pub fn require_headless_shell(conn: &Connection) -> anyhow::Result<()> {
    let dbus = Proxy::new(
        conn,
        "org.freedesktop.DBus",
        "/org/freedesktop/DBus",
        "org.freedesktop.DBus",
    )?;
    let pid: u32 = dbus.call(
        "GetConnectionUnixProcessID",
        &("org.gnome.Mutter.DisplayConfig",),
    )?;
    let cmdline = std::fs::read(format!("/proc/{pid}/cmdline"))?;
    anyhow::ensure!(
        cmdline.split(|b| *b == 0).any(|arg| arg == b"--headless"),
        "refusing to run headless: the DisplayConfig owner (pid {pid}) is not a --headless compositor"
    );
    Ok(())
}

pub fn gnome_shell_pid(conn: &Connection) -> anyhow::Result<u32> {
    let proxy = zbus::blocking::fdo::DBusProxy::new(conn)?;
    Ok(
        proxy.get_connection_unix_process_id(zbus::names::BusName::try_from(
            "org.gnome.Mutter.ScreenCast",
        )?)?,
    )
}

// Same wire shape as the other DisplayConfig readers; zvariant matches fields by position.
#[derive(Debug, Type, Deserialize)]
struct ConnectorInfo {
    connector: String,
    #[allow(dead_code)]
    vendor: String,
    #[allow(dead_code)]
    product: String,
    #[allow(dead_code)]
    serial: String,
}

#[allow(dead_code)]
#[derive(Debug, Type, Deserialize)]
struct ModeInfo {
    id: String,
    width: i32,
    height: i32,
    refresh_rate: f64,
    preferred_scale: f64,
    supported_scales: Vec<f64>,
    properties: HashMap<String, OwnedValue>,
}

#[allow(dead_code)]
#[derive(Debug, Type, Deserialize)]
struct MonitorEntry {
    connector_info: ConnectorInfo,
    modes: Vec<ModeInfo>,
    properties: HashMap<String, OwnedValue>,
}

#[allow(dead_code)]
#[derive(Debug, Type, Deserialize)]
struct LogicalMonitorEntry {
    x: i32,
    y: i32,
    scale: f64,
    transform: u32,
    primary: bool,
    monitors: Vec<ConnectorInfo>,
    properties: HashMap<String, OwnedValue>,
}

type GetCurrentStateResult = (
    u32,
    Vec<MonitorEntry>,
    Vec<LogicalMonitorEntry>,
    HashMap<String, OwnedValue>,
);

/// Every connector Mutter knows, active or not.
pub fn connectors(conn: &Connection) -> anyhow::Result<Vec<String>> {
    let proxy = Proxy::new(
        conn,
        "org.gnome.Mutter.DisplayConfig",
        "/org/gnome/Mutter/DisplayConfig",
        "org.gnome.Mutter.DisplayConfig",
    )?;
    let (_serial, monitors, _logical, _props): GetCurrentStateResult =
        proxy.call("GetCurrentState", &())?;
    Ok(monitors
        .into_iter()
        .map(|monitor| monitor.connector_info.connector)
        .collect())
}

/// The shape `exp07_restore --backup` reads.
#[derive(Serialize)]
struct BackupFile<'a> {
    session_id: &'a str,
    shell_pid: u32,
    outputs: Vec<OutputJson<'a>>,
    topology: Vec<TopologyJson<'a>>,
    primary_output: Option<&'a (String, String)>,
    hash_version: u32,
    configuration_hash: u64,
}

#[derive(Serialize)]
struct OutputJson<'a> {
    connector: &'a str,
    vendor: &'a str,
    product: &'a str,
    serial: &'a str,
    mode_id: &'a str,
    width: i32,
    height: i32,
    refresh_rate: f64,
    enabled: bool,
}

#[derive(Serialize)]
struct TopologyJson<'a> {
    x: i32,
    y: i32,
    scale: f64,
    transform: u32,
    primary: bool,
    monitors: &'a [(String, String)],
}

pub fn backup_json(backup: &DisplayBackup, shell_pid: u32) -> anyhow::Result<String> {
    let file = BackupFile {
        session_id: &backup.session_id,
        shell_pid,
        outputs: backup
            .outputs
            .iter()
            .map(|o| OutputJson {
                connector: &o.connector,
                vendor: &o.vendor,
                product: &o.product,
                serial: &o.serial,
                mode_id: &o.mode_id,
                width: o.width,
                height: o.height,
                refresh_rate: o.refresh_rate,
                enabled: o.enabled,
            })
            .collect(),
        topology: backup
            .topology
            .iter()
            .map(|t| TopologyJson {
                x: t.x,
                y: t.y,
                scale: t.scale,
                transform: t.transform,
                primary: t.primary,
                monitors: &t.monitors,
            })
            .collect(),
        primary_output: backup.primary_output.as_ref(),
        hash_version: CONFIGURATION_HASH_VERSION,
        configuration_hash: backup.configuration_hash,
    };
    Ok(serde_json::to_string_pretty(&file)?)
}

/// Writes `backup.json` into `dir` (created private) and returns its absolute path.
pub fn write_backup(dir: &Path, backup: &DisplayBackup, shell_pid: u32) -> anyhow::Result<PathBuf> {
    write_private(
        dir,
        "backup.json",
        backup_json(backup, shell_pid)?.as_bytes(),
    )
}

/// Writes `name` (mode 0600) into `dir` (created 0700) and returns its absolute path.
pub fn write_private(dir: &Path, name: &str, contents: &[u8]) -> anyhow::Result<PathBuf> {
    use std::io::Write;
    use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)
        .with_context(|| format!("create {}", dir.display()))?;
    let path = std::path::absolute(dir.join(name))?;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&path)?;
    file.write_all(contents)?;
    Ok(path)
}

pub const WATCHDOG_PREFIX: &str = "blackroom-console-wd";

/// The `systemd-run` arguments of one restore timer. Absolute paths and `--working-directory`
/// because the transient unit does not share this process's cwd; `AccuracySec=1s` because the
/// default accuracy lets a timer fire up to a minute late.
pub fn systemd_run_args(
    unit: &str,
    seconds: u64,
    cwd: &Path,
    restore_bin: &Path,
    backup: &Path,
) -> Vec<String> {
    vec![
        "--user".into(),
        "--timer-property=AccuracySec=1s".into(),
        format!("--unit={unit}"),
        format!("--on-active={seconds}s"),
        format!("--working-directory={}", cwd.display()),
        "--".into(),
        restore_bin.display().to_string(),
        "--backup".into(),
        backup.display().to_string(),
        "--keep-live-virtual".into(),
        "--lock-after".into(),
    ]
}

fn quiet(command: &mut Command) -> std::io::Result<std::process::ExitStatus> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
}

/// A dead-man restore: each refresh arms a new timer and then stops the previous one, so a live
/// process always has one pending and a dead one lets it fire (restore keeping the virtual monitor).
pub struct Watchdog {
    restore_bin: PathBuf,
    backup: PathBuf,
    cwd: PathBuf,
    sequence: u32,
    current: Option<String>,
}

impl Watchdog {
    pub fn new(restore_bin: PathBuf, backup: PathBuf) -> anyhow::Result<Self> {
        anyhow::ensure!(
            restore_bin.is_file(),
            "restore binary {} is missing (cargo build -p blackroom-experiments --bin exp07_restore)",
            restore_bin.display()
        );
        Ok(Self {
            restore_bin,
            backup,
            cwd: std::env::current_dir()?,
            sequence: 0,
            current: None,
        })
    }

    pub fn refresh(&mut self, seconds: u64) -> anyhow::Result<()> {
        self.sequence += 1;
        let unit = format!("{WATCHDOG_PREFIX}-{}-{}", std::process::id(), self.sequence);
        let args = systemd_run_args(&unit, seconds, &self.cwd, &self.restore_bin, &self.backup);
        let status = quiet(Command::new("systemd-run").args(&args))?;
        anyhow::ensure!(
            status.success(),
            "systemd-run failed to arm the restore watchdog"
        );
        if let Some(previous) = self.current.replace(unit) {
            stop_timer(&previous);
        }
        Ok(())
    }

    pub fn disarm(&mut self) {
        if let Some(unit) = self.current.take() {
            stop_timer(&unit);
        }
    }
}

fn stop_timer(unit: &str) {
    let _ = quiet(Command::new("systemctl").args(["--user", "stop", &format!("{unit}.timer")]));
}

/// True while a console restore timer from any process is pending.
pub fn watchdog_pending() -> bool {
    Command::new("systemctl")
        .args([
            "--user",
            "list-timers",
            "--all",
            "--no-pager",
            "--no-legend",
            &format!("{WATCHDOG_PREFIX}-*"),
        ])
        .stdin(Stdio::null())
        .output()
        .is_ok_and(|output| !output.stdout.iter().all(u8::is_ascii_whitespace))
}

/// Locks the session through logind; the console never unlocks it again.
pub fn lock_session(session_id: &str) -> bool {
    let mut child = match Command::new("loginctl")
        .args(["lock-session", session_id])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(child) => child,
        Err(_) => return false,
    };
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return status.success(),
            Ok(None) if std::time::Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(50));
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return false;
            }
        }
    }
}

pub const RECOVERY_MARKER: &str = "recovery.json";

/// Holds suspend and idle sleep off while a session runs; released on drop.
pub enum SleepLock {
    Logind(zbus::zvariant::OwnedFd),
    Gnome { conn: Connection, cookie: u32 },
}

impl Drop for SleepLock {
    fn drop(&mut self) {
        if let Self::Gnome { conn, cookie } = self
            && let Ok(session) = gnome_session(conn)
        {
            let _ = session.call::<_, _, ()>("Uninhibit", &(*cookie,));
        }
    }
}

fn gnome_session(conn: &Connection) -> zbus::Result<Proxy<'_>> {
    Proxy::new(
        conn,
        "org.gnome.SessionManager",
        "/org/gnome/SessionManager",
        "org.gnome.SessionManager",
    )
}

const SLEEP_REASON: &str = "a remote console session is running";

/// logind refuses a `block` lock to callers outside a local session (an SSH or systemd start asks for
/// admin authentication), so the GNOME session manager's suspend+idle inhibitor is the fallback.
pub fn inhibit_sleep(session: &Connection) -> anyhow::Result<SleepLock> {
    match logind_sleep_lock() {
        Ok(fd) => Ok(SleepLock::Logind(fd)),
        Err(logind) => gnome_sleep_lock(session)
            .map_err(|gnome| anyhow::anyhow!("logind: {logind}; GNOME session: {gnome}")),
    }
}

fn gnome_sleep_lock(session: &Connection) -> zbus::Result<SleepLock> {
    // Flags: 4 = suspend, 8 = idle.
    let cookie: u32 = gnome_session(session)?
        .call("Inhibit", &("Blackroom Console", 0u32, SLEEP_REASON, 12u32))?;
    Ok(SleepLock::Gnome {
        conn: session.clone(),
        cookie,
    })
}

/// A logind `block` inhibitor for suspend and idle sleep; it ends when the returned fd is dropped.
fn logind_sleep_lock() -> anyhow::Result<zbus::zvariant::OwnedFd> {
    let system = Connection::system()?;
    let manager = Proxy::new(
        &system,
        "org.freedesktop.login1",
        "/org/freedesktop/login1",
        "org.freedesktop.login1.Manager",
    )?;
    let fd: zbus::zvariant::OwnedFd = manager.call(
        "Inhibit",
        &("sleep:idle", "Blackroom Console", SLEEP_REASON, "block"),
    )?;
    Ok(fd)
}

#[derive(Serialize, Deserialize)]
struct RecoveryMarker {
    pid: u32,
    backup: PathBuf,
}

/// Written before the display is touched and cleared once it is verified restored, so a console
/// that dies mid-session leaves a trace the next start acts on.
pub fn write_recovery_marker(dir: &Path, backup: &Path) -> anyhow::Result<()> {
    let marker = RecoveryMarker {
        pid: std::process::id(),
        backup: backup.to_path_buf(),
    };
    write_private(dir, RECOVERY_MARKER, &serde_json::to_vec(&marker)?)?;
    Ok(())
}

pub fn clear_recovery_marker(dir: &Path) {
    let _ = std::fs::remove_file(dir.join(RECOVERY_MARKER));
}

#[derive(Debug, PartialEq, Eq)]
pub enum Recovery {
    /// No marker, or the writer is still running.
    Nothing,
    /// A marker that no longer applies (no backup, or another login session) was removed.
    Cleared(String),
    /// The unclean session's display was restored and the screen locked.
    Restored,
    /// The restore failed: Start stays refused until the marker is dealt with.
    Pending(String),
}

#[derive(Deserialize)]
struct BackupIdentity {
    session_id: String,
    shell_pid: u32,
}

/// Acts on a marker left by a console that died mid-session. `alive` tells whether the writer pid
/// is still a console, `same_login` compares the backup's session and Shell pid with the live ones,
/// `restore` runs the restore binary against the backup (and locks).
pub fn recover(
    dir: &Path,
    alive: impl Fn(u32) -> bool,
    same_login: impl Fn(&str, u32) -> bool,
    restore: impl Fn(&Path) -> bool,
) -> Recovery {
    let marker_path = dir.join(RECOVERY_MARKER);
    let Ok(text) = std::fs::read_to_string(&marker_path) else {
        return Recovery::Nothing;
    };
    let Ok(marker) = serde_json::from_str::<RecoveryMarker>(&text) else {
        let _ = std::fs::remove_file(&marker_path);
        return Recovery::Cleared("unreadable marker".into());
    };
    if marker.pid != std::process::id() && alive(marker.pid) {
        return Recovery::Nothing;
    }
    let identity = std::fs::read_to_string(&marker.backup)
        .ok()
        .and_then(|text| serde_json::from_str::<BackupIdentity>(&text).ok());
    let Some(identity) = identity else {
        let _ = std::fs::remove_file(&marker_path);
        return Recovery::Cleared("no readable backup left".into());
    };
    if !same_login(&identity.session_id, identity.shell_pid) {
        let _ = std::fs::remove_file(&marker_path);
        return Recovery::Cleared("the backup belongs to another login session".into());
    }
    if restore(&marker.backup) {
        let _ = std::fs::remove_file(&marker_path);
        Recovery::Restored
    } else {
        Recovery::Pending(format!(
            "the display restore of an unclean session failed; check the panel, then remove {}",
            marker_path.display()
        ))
    }
}

#[cfg(test)]
mod tests {
    use blackroom_gnome::mutter::display_config::{LogicalMonitorBackup, OutputBackup};

    use super::*;

    fn sample() -> DisplayBackup {
        DisplayBackup {
            timestamp_unix: 0,
            session_id: "182".into(),
            outputs: vec![OutputBackup {
                connector: "eDP-1".into(),
                vendor: "V".into(),
                product: "P".into(),
                serial: "S".into(),
                mode_id: "1920x1080@60".into(),
                width: 1920,
                height: 1080,
                refresh_rate: 60.0,
                enabled: true,
            }],
            topology: vec![LogicalMonitorBackup {
                x: 0,
                y: 0,
                scale: 1.0,
                transform: 0,
                primary: true,
                monitors: vec![("eDP-1".into(), "S".into())],
            }],
            primary_output: Some(("eDP-1".into(), "S".into())),
            configuration_hash: 7,
        }
    }

    #[test]
    fn backup_json_has_the_fields_exp07_restore_reads() {
        let json: serde_json::Value =
            serde_json::from_str(&backup_json(&sample(), 4242).unwrap()).unwrap();
        assert_eq!(json["session_id"], "182");
        assert_eq!(json["shell_pid"], 4242);
        assert_eq!(json["hash_version"], CONFIGURATION_HASH_VERSION);
        assert_eq!(json["configuration_hash"], 7);
        assert_eq!(json["outputs"][0]["enabled"], true);
        assert_eq!(json["outputs"][0]["mode_id"], "1920x1080@60");
        assert_eq!(json["topology"][0]["monitors"][0][0], "eDP-1");
        assert_eq!(json["primary_output"][1], "S");
    }

    #[test]
    fn watchdog_timer_is_absolute_accurate_and_keeps_the_virtual_monitor() {
        let args = systemd_run_args(
            "u",
            60,
            Path::new("/repo"),
            Path::new("/repo/target/debug/exp07_restore"),
            Path::new("/run/user/1000/blackroom-console/backup.json"),
        );
        assert!(args.contains(&"--timer-property=AccuracySec=1s".to_string()));
        assert!(args.contains(&"--on-active=60s".to_string()));
        assert!(args.contains(&"--working-directory=/repo".to_string()));
        assert_eq!(args.last().map(String::as_str), Some("--lock-after"));
        assert!(args.contains(&"--keep-live-virtual".to_string()));
        let dash = args.iter().position(|a| a == "--").unwrap();
        assert!(args[dash + 1].starts_with('/'));
    }

    fn recovery_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("br-recover-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn marked(dir: &Path, session: &str, shell: u32, pid: u32) {
        let backup = write_private(
            dir,
            "backup.json",
            format!(r#"{{"session_id":"{session}","shell_pid":{shell}}}"#).as_bytes(),
        )
        .unwrap();
        let marker = RecoveryMarker { pid, backup };
        write_private(dir, RECOVERY_MARKER, &serde_json::to_vec(&marker).unwrap()).unwrap();
    }

    #[test]
    fn recovery_does_nothing_without_a_marker_or_with_a_live_writer() {
        let dir = recovery_dir("none");
        let calls = std::cell::Cell::new(0);
        let go = |alive: bool| {
            recover(
                &dir,
                |_| alive,
                |_, _| true,
                |_| {
                    calls.set(calls.get() + 1);
                    true
                },
            )
        };
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(go(true), Recovery::Nothing);
        marked(&dir, "3", 7, 999_999);
        assert_eq!(go(true), Recovery::Nothing);
        assert!(dir.join(RECOVERY_MARKER).exists());
        assert_eq!(calls.get(), 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn recovery_clears_markers_that_no_longer_apply_without_restoring() {
        let dir = recovery_dir("stale");
        let restored = std::cell::Cell::new(false);
        let restore = |_: &Path| {
            restored.set(true);
            true
        };
        marked(&dir, "3", 7, 999_999);
        std::fs::remove_file(dir.join("backup.json")).unwrap();
        assert!(matches!(
            recover(&dir, |_| false, |_, _| true, restore),
            Recovery::Cleared(_)
        ));
        assert!(!dir.join(RECOVERY_MARKER).exists());
        marked(&dir, "3", 7, 999_999);
        assert!(matches!(
            recover(
                &dir,
                |_| false,
                |session, shell| (session, shell) == ("4", 7),
                restore
            ),
            Recovery::Cleared(_)
        ));
        assert!(!dir.join(RECOVERY_MARKER).exists());
        assert!(!restored.get());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn recovery_restores_an_unclean_session_and_keeps_the_marker_when_that_fails() {
        let dir = recovery_dir("restore");
        marked(&dir, "3", 7, 999_999);
        let same = |session: &str, shell: u32| (session, shell) == ("3", 7);
        assert!(matches!(
            recover(&dir, |_| false, same, |_| false),
            Recovery::Pending(_)
        ));
        assert!(dir.join(RECOVERY_MARKER).exists());
        assert_eq!(recover(&dir, |_| false, same, |_| true), Recovery::Restored);
        assert!(!dir.join(RECOVERY_MARKER).exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    #[ignore = "needs the system bus: cargo test -p blackroom-console -- --ignored sleep_inhibitor"]
    fn sleep_inhibitor_is_listed_while_held_and_gone_after_drop() {
        let listed = || {
            let out = Command::new("systemd-inhibit")
                .arg("--list")
                .output()
                .unwrap();
            String::from_utf8_lossy(&out.stdout).contains("a remote console session is running")
        };
        assert!(!listed());
        let fd = logind_sleep_lock().unwrap();
        assert!(listed());
        drop(fd);
        std::thread::sleep(Duration::from_millis(300));
        assert!(!listed());
    }

    #[test]
    #[ignore = "needs the session bus: cargo test -p blackroom-console -- --ignored gnome_sleep_lock"]
    fn gnome_sleep_lock_is_counted_while_held_and_gone_after_drop() {
        let session = Connection::session().unwrap();
        let count = || -> usize {
            let inhibitors: Vec<zbus::zvariant::OwnedObjectPath> = gnome_session(&session)
                .unwrap()
                .call("GetInhibitors", &())
                .unwrap();
            inhibitors.len()
        };
        let before = count();
        let lock = gnome_sleep_lock(&session).unwrap();
        assert_eq!(count(), before + 1);
        drop(lock);
        assert_eq!(count(), before);
    }

    #[test]
    fn backup_is_written_private() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("br-console-test-{}", std::process::id()));
        let path = write_backup(&dir, &sample(), 1).unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
