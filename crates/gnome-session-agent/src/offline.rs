use std::ffi::{OsStr, OsString};
use std::fs::File;
use std::io::{self, Write};
use std::os::unix::fs::MetadataExt;
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use blackroom_core::epoch::SecurityEpoch;
use blackroom_core::state::State;
use blackroom_gnome::fake::FaultConfig;
use ed25519_dalek::VerifyingKey;
use gnome_session_agent::{authority::InputAuthority, ipc, offline_lifecycle::OfflineLifecycle};
use remote_hostd::store::PersistentHostAuthority;

fn arguments(
    args: &[OsString],
) -> io::Result<(
    PathBuf,
    VerifyingKey,
    SecurityEpoch,
    u32,
    Option<PathBuf>,
    bool,
)> {
    let (base, state_directory) = match args {
        [base @ .., flag, directory]
            if base.first() == Some(&OsString::from("--offline-sim-agent-service"))
                && flag == OsStr::new("--state-dir") =>
        {
            (base, Some(PathBuf::from(directory)))
        }
        _ => (args, None),
    };
    let [
        mode,
        socket_flag,
        path,
        verifier_flag,
        public_key,
        epoch_flag,
        epoch,
        uid_flag,
        uid,
    ] = base
    else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "offline simulation arguments required",
        ));
    };
    let service_mode = mode == OsStr::new("--offline-sim-agent-service");
    if mode != OsStr::new("--offline-sim-agent") && !service_mode
        || socket_flag != OsStr::new("--socket")
        || verifier_flag != OsStr::new("--verifier-hex")
        || epoch_flag != OsStr::new("--epoch")
        || uid_flag != OsStr::new("--host-uid")
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "offline simulation arguments required",
        ));
    }
    let path = PathBuf::from(path);
    if !path.is_absolute() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "offline socket must be absolute",
        ));
    }
    let mut key = [0_u8; 32];
    hex::decode_to_slice(public_key.to_str().unwrap_or(""), &mut key)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid offline verifier"))?;
    let verifier = VerifyingKey::from_bytes(&key)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid offline verifier"))?;
    let epoch = epoch
        .to_str()
        .unwrap_or("")
        .parse::<u64>()
        .map(SecurityEpoch::from_value)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid offline epoch"))?;
    let uid = uid
        .to_str()
        .unwrap_or("")
        .parse::<u32>()
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid offline UID"))?;
    Ok((path, verifier, epoch, uid, state_directory, service_mode))
}

fn open_private_directory(parent: &Path) -> io::Result<File> {
    if !parent.is_absolute() {
        return Err(io::Error::from(io::ErrorKind::InvalidInput));
    }
    let fd = rustix::fs::openat2(
        rustix::fs::CWD,
        parent,
        rustix::fs::OFlags::RDONLY | rustix::fs::OFlags::DIRECTORY | rustix::fs::OFlags::CLOEXEC,
        rustix::fs::Mode::empty(),
        rustix::fs::ResolveFlags::NO_SYMLINKS | rustix::fs::ResolveFlags::NO_MAGICLINKS,
    )?;
    let directory = File::from(fd);
    let metadata = directory.metadata()?;
    if metadata.uid() != rustix::process::getuid().as_raw() || metadata.mode() & 0o077 != 0 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "offline socket directory must be owner-only",
        ));
    }
    Ok(directory)
}

fn finish_recovery(
    state_directory: Option<&File>,
    authority: &InputAuthority,
    lifecycle: &OfflineLifecycle,
) -> io::Result<()> {
    if lifecycle.state() != State::LocalLocked {
        if let Some(directory) = state_directory {
            PersistentHostAuthority::emergency_stop(directory)?;
        }
        return Err(io::Error::other("offline recovery could not be verified"));
    }
    if let Some(directory) = state_directory {
        if PersistentHostAuthority::emergency_pending(directory)? {
            return Ok(());
        }
        if let Some(pending_epoch) = PersistentHostAuthority::recovery_epoch(directory)? {
            if pending_epoch > authority.epoch() {
                return Err(io::Error::from(io::ErrorKind::PermissionDenied));
            }
            PersistentHostAuthority::verify_recovery(directory, pending_epoch)?;
        }
    }
    Ok(())
}

pub fn run(args: &[OsString]) -> io::Result<()> {
    let (path, verifier, epoch, expected_uid, state_directory, service_mode) = arguments(args)?;
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::from(io::ErrorKind::InvalidInput))?;
    open_private_directory(parent)?;
    let state_directory = state_directory
        .as_deref()
        .map(open_private_directory)
        .transpose()?;
    let listener = UnixListener::bind(&path)?;
    listener.set_nonblocking(true)?;
    println!("OFFLINE SIMULATION | no GNOME session or input");
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut stream = loop {
        match listener.accept() {
            Ok((stream, _)) => break stream,
            Err(error)
                if error.kind() == io::ErrorKind::WouldBlock && Instant::now() < deadline =>
            {
                std::thread::yield_now();
            }
            Err(error) => return Err(error),
        }
    };
    let mut authority = InputAuthority::new(verifier, epoch, "simulated-session-only".into());
    let mut lifecycle = OfflineLifecycle::new(FaultConfig::new());
    loop {
        if state_directory.as_ref().is_some_and(|directory| {
            PersistentHostAuthority::emergency_pending(directory).unwrap_or(true)
        }) {
            lifecycle.recover(&mut authority);
            println!("{}", lifecycle.state());
            return if lifecycle.state() == State::LocalLocked {
                Ok(())
            } else {
                Err(io::Error::other(
                    "offline emergency recovery could not be verified",
                ))
            };
        }
        if service_mode
            && lifecycle.state() == State::RemoteActive
            && lifecycle.confirm_active(&mut authority).is_err()
        {
            println!("{}", lifecycle.state());
            finish_recovery(state_directory.as_ref(), &authority, &lifecycle)?;
            return Ok(());
        }
        if service_mode {
            let mut fds = [rustix::event::PollFd::new(
                &stream,
                rustix::event::PollFlags::IN,
            )];
            let timeout = rustix::event::Timespec {
                tv_sec: 1,
                tv_nsec: 0,
            };
            if rustix::event::poll(&mut fds, Some(&timeout))? == 0 {
                continue;
            }
        }
        match ipc::receive_host_update(&mut stream, expected_uid, &mut authority) {
            Ok(()) => {
                if state_directory.as_ref().is_some_and(|directory| {
                    PersistentHostAuthority::emergency_pending(directory).unwrap_or(true)
                }) {
                    lifecycle.recover(&mut authority);
                    return Err(io::Error::from(io::ErrorKind::PermissionDenied));
                }
                let active = authority.dispatch((), |_, _| Ok(())).is_ok();
                let result = if active {
                    if lifecycle.state() == State::RemoteActive {
                        lifecycle.confirm_active(&mut authority)
                    } else {
                        lifecycle.activate(&mut authority)
                    }
                } else {
                    lifecycle.recover_after_revoke(&mut authority);
                    Ok(())
                };
                let active = result.is_ok() && lifecycle.state() == State::RemoteActive;
                if !active {
                    finish_recovery(state_directory.as_ref(), &authority, &lifecycle)?;
                }
                println!("{}", lifecycle.state());
                if service_mode {
                    stream.set_write_timeout(Some(Duration::from_millis(300)))?;
                    stream.write_all(&[if lifecycle.state() == State::FailedSafe {
                        2
                    } else {
                        u8::from(active)
                    }])?;
                } else {
                    result.map_err(|error| io::Error::other(error.to_string()))?;
                }
            }
            Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => {
                lifecycle.recover(&mut authority);
                println!("{}", lifecycle.state());
                return finish_recovery(state_directory.as_ref(), &authority, &lifecycle);
            }
            Err(error) => {
                lifecycle.recover(&mut authority);
                finish_recovery(state_directory.as_ref(), &authority, &lifecycle)?;
                return Err(error);
            }
        }
    }
}
