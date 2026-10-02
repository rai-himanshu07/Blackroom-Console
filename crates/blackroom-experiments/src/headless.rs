//! Guard shared by the experiments that may only run against a throwaway headless Shell.

use zbus::blocking::{Connection, Proxy};

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
        "refusing to run: the DisplayConfig owner (pid {pid}) is not a --headless compositor"
    );
    Ok(())
}
