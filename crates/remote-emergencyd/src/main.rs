//! `remote-emergencyd`: holds the exclusive physical-input grab for a remote session and releases
//! it on request, on lease expiry, when its client disappears, or on the emergency chord. Not
//! installed or enabled by anything in this repository, and it refuses to grab without
//! `--enable-grabs`. See `docs/security/emergency-daemon.md`.
#![forbid(unsafe_code)]

use std::fs;
use std::os::linux::net::SocketAddrExt;
use std::os::unix::fs::{FileTypeExt, PermissionsExt};
use std::os::unix::net::{SocketAddr, UnixDatagram, UnixListener};
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

use clap::Parser;
use remote_emergencyd::core::{Config, Daemon};
use remote_emergencyd::nodes::EvdevNodes;
use remote_emergencyd::server::{DirMarker, Locker, LoginctlLocker, Policy, StopMarker, serve};
use remote_hostd::store::open_state_directory;

type Fallible<T> = Result<T, String>;

#[derive(Parser, Debug)]
#[command(about = "Independent physical-input grab holder and emergency chord observer")]
struct Args {
    /// Absolute path of the control socket (created 0600, replaced if it is a stale socket).
    #[arg(long)]
    socket: PathBuf,
    /// Uid of the only client allowed to connect (the `remote-hostd` service user).
    #[arg(
        long,
        conflicts_with = "client_user",
        required_unless_present = "client_user"
    )]
    client_uid: Option<u32>,
    /// Same, by user name from /etc/passwd.
    #[arg(long)]
    client_user: Option<String>,
    /// Absolute state directory of `remote-hostd`; the chord writes its stop marker there.
    #[arg(long)]
    state_dir: Option<PathBuf>,
    /// Lock every session with `loginctl lock-sessions` after an emergency chord.
    #[arg(long, default_value_t = false)]
    lock_on_emergency: bool,
    /// Directory holding the `eventN` nodes (a test hook; the default is the real `/dev/input`).
    #[arg(long, default_value = "/dev/input")]
    input_dir: PathBuf,
    /// Actually take grabs; without this every isolate request is refused.
    #[arg(long, default_value_t = false)]
    enable_grabs: bool,
}

/// Minimal `sd_notify`: READY once listening and WATCHDOG while the loop makes progress.
struct Notifier {
    socket: Option<(UnixDatagram, SocketAddr)>,
    every: Duration,
    last: Instant,
}

impl Notifier {
    fn from_env() -> Self {
        let address = std::env::var("NOTIFY_SOCKET").ok().and_then(|name| {
            match name.strip_prefix('@') {
                Some(abstract_name) => SocketAddr::from_abstract_name(abstract_name.as_bytes()),
                None => SocketAddr::from_pathname(&name),
            }
            .ok()
        });
        let socket = address.and_then(|address| Some((UnixDatagram::unbound().ok()?, address)));
        let every = std::env::var("WATCHDOG_USEC")
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .map_or(Duration::from_secs(3600), |usec| {
                Duration::from_micros(usec / 2)
            });
        Self {
            socket,
            every,
            last: Instant::now(),
        }
    }

    fn send(&self, message: &str) {
        if let Some((socket, address)) = &self.socket {
            let _ = socket.send_to_addr(message.as_bytes(), address);
        }
    }

    fn heartbeat(&mut self) {
        if self.last.elapsed() >= self.every {
            self.last = Instant::now();
            self.send("WATCHDOG=1");
        }
    }
}

/// Uid of `user` from passwd-format text (`name:x:uid:gid:...`).
fn uid_of(passwd: &str, user: &str) -> Option<u32> {
    passwd.lines().find_map(|line| {
        let mut fields = line.split(':');
        (fields.next()? == user).then_some(())?;
        fields.nth(1)?.parse().ok()
    })
}

fn client_uid(args: &Args) -> Fallible<u32> {
    match (&args.client_uid, &args.client_user) {
        (Some(uid), _) => Ok(*uid),
        (None, Some(user)) => {
            let passwd = fs::read_to_string("/etc/passwd").map_err(|error| error.to_string())?;
            uid_of(&passwd, user).ok_or_else(|| format!("no user named {user}"))
        }
        (None, None) => Err("--client-uid or --client-user is required".to_string()),
    }
}

fn bind_socket(path: &PathBuf) -> Fallible<UnixListener> {
    if !path.is_absolute() {
        return Err("--socket must be an absolute path".to_string());
    }
    if let Ok(meta) = fs::symlink_metadata(path) {
        if !meta.file_type().is_socket() {
            return Err(format!("{} exists and is not a socket", path.display()));
        }
        fs::remove_file(path).map_err(|error| error.to_string())?;
    }
    let listener = UnixListener::bind(path).map_err(|error| error.to_string())?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
        .map_err(|error| error.to_string())?;
    Ok(listener)
}

fn run(args: &Args) -> Fallible<()> {
    let allowed_uid = client_uid(args)?;
    let listener = bind_socket(&args.socket)?;
    let marker: Option<Box<dyn StopMarker + Send>> = match &args.state_dir {
        Some(path) => {
            let directory = open_state_directory(path).map_err(|error| error.to_string())?;
            Some(Box::new(DirMarker(directory)))
        }
        None => None,
    };
    let locker: Option<Box<dyn Locker + Send>> = args
        .lock_on_emergency
        .then(|| Box::new(LoginctlLocker) as Box<dyn Locker + Send>);
    let mut policy = Policy {
        allowed_uid,
        grabs_enabled: args.enable_grabs,
        marker,
        locker,
    };
    let mut daemon = Daemon::new(
        EvdevNodes::with_root(args.input_dir.clone()),
        Config::standard(),
    );
    let mut notifier = Notifier::from_env();
    notifier.send("READY=1");
    // SIGTERM keeps its default action: the process ends and the kernel drops every grab.
    let stop = AtomicBool::new(false);
    let result = serve(&listener, &mut daemon, &mut policy, &stop, || {
        notifier.heartbeat()
    });
    let _ = fs::remove_file(&args.socket);
    result.map_err(|error| error.to_string())
}

fn main() {
    let args = Args::parse();
    if let Err(message) = run(&args) {
        eprintln!("remote-emergencyd: {message}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::uid_of;

    #[test]
    fn a_user_name_is_resolved_from_passwd_text() {
        let passwd = "root:x:0:0:root:/root:/bin/bash\nremote-hostd:x:998:997::/var/lib/x:/usr/sbin/nologin\n";
        assert_eq!(uid_of(passwd, "remote-hostd"), Some(998));
        assert_eq!(uid_of(passwd, "root"), Some(0));
        assert_eq!(uid_of(passwd, "remote"), None);
        assert_eq!(uid_of("broken-line", "broken-line"), None);
    }
}
