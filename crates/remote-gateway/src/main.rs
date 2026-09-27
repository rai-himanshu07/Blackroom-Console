use std::ffi::{OsStr, OsString};
use std::fs::File;
use std::io;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::path::{Path, PathBuf};

enum OfflineMode {
    Ephemeral,
    Persisted(PathBuf),
    Separated {
        state: PathBuf,
        hostd: PathBuf,
        agent: PathBuf,
    },
    ScratchSeparated {
        hostd: PathBuf,
        agent: PathBuf,
    },
}

fn offline_mode(args: &[OsString]) -> Result<OfflineMode, &'static str> {
    match args {
        [mode] if mode == OsStr::new("--offline-sim") => Ok(OfflineMode::Ephemeral),
        [mode, flag, path]
            if mode == OsStr::new("--offline-sim") && flag == OsStr::new("--state-dir") =>
        {
            let path = PathBuf::from(path);
            if !path.is_absolute() {
                return Err("--state-dir requires an absolute path");
            }
            Ok(OfflineMode::Persisted(path))
        }
        [
            mode,
            separate,
            state_flag,
            state,
            hostd_flag,
            hostd,
            agent_flag,
            agent,
        ] if mode == OsStr::new("--offline-sim")
            && separate == OsStr::new("--separate")
            && state_flag == OsStr::new("--state-dir")
            && hostd_flag == OsStr::new("--hostd-bin")
            && agent_flag == OsStr::new("--agent-bin") =>
        {
            let state = PathBuf::from(state);
            let hostd = PathBuf::from(hostd);
            let agent = PathBuf::from(agent);
            if !state.is_absolute() || !hostd.is_absolute() || !agent.is_absolute() {
                return Err("separate simulation paths must be absolute");
            }
            Ok(OfflineMode::Separated {
                state,
                hostd,
                agent,
            })
        }
        [mode, scratch, hostd_flag, hostd, agent_flag, agent]
            if mode == OsStr::new("--offline-sim")
                && scratch == OsStr::new("--separate-scratch")
                && hostd_flag == OsStr::new("--hostd-bin")
                && agent_flag == OsStr::new("--agent-bin") =>
        {
            let hostd = PathBuf::from(hostd);
            let agent = PathBuf::from(agent);
            if !hostd.is_absolute() || !agent.is_absolute() {
                return Err("separate simulation binaries must be absolute");
            }
            Ok(OfflineMode::ScratchSeparated { hostd, agent })
        }
        _ => Err(
            "usage: remote-gateway --offline-sim [--state-dir <private absolute path> | --separate --state-dir <private absolute path> --hostd-bin <absolute path> --agent-bin <absolute path> | --separate-scratch --hostd-bin <absolute path> --agent-bin <absolute path>]",
        ),
    }
}

fn open_private_state_dir(path: &Path) -> io::Result<File> {
    let fd = rustix::fs::openat2(
        rustix::fs::CWD,
        path,
        rustix::fs::OFlags::RDONLY | rustix::fs::OFlags::DIRECTORY | rustix::fs::OFlags::CLOEXEC,
        rustix::fs::Mode::empty(),
        rustix::fs::ResolveFlags::NO_SYMLINKS | rustix::fs::ResolveFlags::NO_MAGICLINKS,
    )?;
    Ok(File::from(fd))
}

async fn start_offline(
    address: SocketAddr,
    mode: &OfflineMode,
) -> io::Result<(
    tokio::net::TcpListener,
    axum::Router,
    Option<tempfile::TempDir>,
)> {
    let listener = tokio::net::TcpListener::bind(address).await?;
    let mut scratch_state = None;
    let router = match mode {
        OfflineMode::Ephemeral => remote_gateway::router(),
        OfflineMode::Persisted(path) => {
            remote_gateway::persisted_router(&open_private_state_dir(path)?)?
        }
        OfflineMode::Separated {
            state,
            hostd,
            agent,
        } => {
            let _verified = open_private_state_dir(state)?;
            remote_gateway::separated_router(state, hostd, agent)?
        }
        OfflineMode::ScratchSeparated { hostd, agent } => {
            use std::os::unix::fs::PermissionsExt;

            let directory = tempfile::tempdir()?;
            std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700))?;
            let router = remote_gateway::separated_router(directory.path(), hostd, agent)?;
            scratch_state = Some(directory);
            router
        }
    };
    Ok((listener, router, scratch_state))
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mode = offline_mode(&std::env::args_os().skip(1).collect::<Vec<_>>())?;
    let address = SocketAddrV4::new(Ipv4Addr::LOCALHOST, 8787);
    let (listener, router, scratch_state) = start_offline(address.into(), &mode).await?;
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    let mut interrupt = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())?;
    println!("OFFLINE SIMULATION ONLY | no GNOME session or real input | http://{address}");
    let served = axum::serve(listener, router)
        .with_graceful_shutdown(async move {
            tokio::select! {
                _ = terminate.recv() => {},
                _ = interrupt.recv() => {},
            }
        })
        .await;
    drop(scratch_state);
    served?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    #[test]
    fn persisted_mode_requires_explicit_absolute_non_symlinked_directory() {
        assert!(matches!(
            offline_mode(&["--offline-sim".into()]).unwrap(),
            OfflineMode::Ephemeral
        ));
        assert!(
            offline_mode(&[
                "--offline-sim".into(),
                "--state-dir".into(),
                "relative".into()
            ])
            .is_err()
        );
        assert!(offline_mode(&["--live".into()]).is_err());

        let directory = tempfile::tempdir().unwrap();
        let private_path = directory.path().to_path_buf();
        let args = [
            "--offline-sim".into(),
            "--state-dir".into(),
            private_path.clone().into_os_string(),
        ];
        assert!(
            matches!(offline_mode(&args).unwrap(), OfflineMode::Persisted(path) if path == private_path)
        );
        assert!(open_private_state_dir(&private_path).is_ok());
        let alias = directory.path().join("alias");
        symlink(&private_path, &alias).unwrap();
        assert!(open_private_state_dir(&alias).is_err());
        assert!(open_private_state_dir(&alias.join(".")).is_err());
    }

    #[test]
    fn separated_mode_requires_explicit_absolute_binary_paths() {
        let valid = [
            "--offline-sim",
            "--separate",
            "--state-dir",
            "/tmp/only-for-test",
            "--hostd-bin",
            "/usr/bin/false",
            "--agent-bin",
            "/usr/bin/false",
        ];
        assert!(matches!(
            offline_mode(&valid.map(OsString::from)).unwrap(),
            OfflineMode::Separated { .. }
        ));
        let invalid = [
            "--offline-sim",
            "--separate",
            "--state-dir",
            "/tmp/only-for-test",
            "--hostd-bin",
            "remote-hostd",
            "--agent-bin",
            "/usr/bin/false",
        ];
        assert!(offline_mode(&invalid.map(OsString::from)).is_err());
        let scratch = [
            "--offline-sim",
            "--separate-scratch",
            "--hostd-bin",
            "/usr/bin/false",
            "--agent-bin",
            "/usr/bin/false",
        ];
        assert!(matches!(
            offline_mode(&scratch.map(OsString::from)).unwrap(),
            OfflineMode::ScratchSeparated { .. }
        ));
    }

    #[tokio::test]
    async fn occupied_port_never_bootstraps_persisted_host_state() {
        use std::os::unix::fs::PermissionsExt;

        let directory = tempfile::tempdir().unwrap();
        std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let occupied =
            std::net::TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0)).unwrap();
        let error = start_offline(
            occupied.local_addr().unwrap(),
            &OfflineMode::Persisted(directory.path().into()),
        )
        .await
        .err()
        .unwrap();
        assert_eq!(error.kind(), io::ErrorKind::AddrInUse);
        assert!(!directory.path().join("host-identity.key").exists());
        assert!(!directory.path().join("security-epoch").exists());
    }
}
