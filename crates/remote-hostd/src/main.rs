use std::ffi::{OsStr, OsString};
use std::fs::File;
use std::io;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use remote_hostd::isolation::{DaemonIsolation, InputIsolation};
use remote_hostd::{service, store::PersistentHostAuthority, write_update};

fn service_arguments(args: &[OsString]) -> io::Result<(PathBuf, PathBuf, PathBuf)> {
    let (state, runtime, agent, control) = match args {
        [
            mode,
            state_flag,
            state,
            agent_flag,
            agent,
            control_flag,
            control,
        ] if mode == OsStr::new("--offline-sim-service")
            && state_flag == OsStr::new("--state-dir")
            && agent_flag == OsStr::new("--agent-socket")
            && control_flag == OsStr::new("--control-socket") =>
        {
            let state = PathBuf::from(state);
            (
                state.clone(),
                state,
                PathBuf::from(agent),
                PathBuf::from(control),
            )
        }
        [
            mode,
            state_flag,
            state,
            runtime_flag,
            runtime,
            agent_flag,
            agent,
            control_flag,
            control,
        ] if mode == OsStr::new("--offline-sim-service")
            && state_flag == OsStr::new("--state-dir")
            && runtime_flag == OsStr::new("--runtime-dir")
            && agent_flag == OsStr::new("--agent-socket")
            && control_flag == OsStr::new("--control-socket") =>
        {
            (
                PathBuf::from(state),
                PathBuf::from(runtime),
                PathBuf::from(agent),
                PathBuf::from(control),
            )
        }
        _ => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "offline service arguments required",
            ));
        }
    };
    if !state.is_absolute()
        || !runtime.is_absolute()
        || agent.parent() != Some(runtime.as_path())
        || control.parent() != Some(runtime.as_path())
        || agent == control
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "offline sockets must be distinct and inside the absolute runtime directory",
        ));
    }
    let runtime_fd = open_state_dir(&runtime)?;
    let metadata = runtime_fd.metadata()?;
    if std::os::unix::fs::MetadataExt::uid(&metadata) != rustix::process::getuid().as_raw()
        || std::os::unix::fs::MetadataExt::mode(&metadata) & 0o077 != 0
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "offline runtime directory must be owner-only",
        ));
    }
    Ok((state, agent, control))
}

fn arguments(args: &[OsString]) -> io::Result<(PathBuf, PathBuf)> {
    let [mode, state_flag, state, socket_flag, socket] = args else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "offline host arguments required",
        ));
    };
    if mode != OsStr::new("--offline-sim-host")
        || state_flag != OsStr::new("--state-dir")
        || socket_flag != OsStr::new("--agent-socket")
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "offline host arguments required",
        ));
    }
    let state = PathBuf::from(state);
    let socket = PathBuf::from(socket);
    if !state.is_absolute() || socket.parent() != Some(state.as_path()) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "offline agent socket must be inside the absolute state directory",
        ));
    }
    Ok((state, socket))
}

fn open_state_dir(path: &Path) -> io::Result<File> {
    let fd = rustix::fs::openat2(
        rustix::fs::CWD,
        path,
        rustix::fs::OFlags::RDONLY | rustix::fs::OFlags::DIRECTORY | rustix::fs::OFlags::CLOEXEC,
        rustix::fs::Mode::empty(),
        rustix::fs::ResolveFlags::NO_SYMLINKS | rustix::fs::ResolveFlags::NO_MAGICLINKS,
    )?;
    Ok(File::from(fd))
}

fn run(args: &[OsString]) -> io::Result<()> {
    let (state, socket) = arguments(args)?;
    let directory = open_state_dir(&state)?;
    let mut host = PersistentHostAuthority::open(&directory)?;
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut stream = loop {
        match UnixStream::connect(&socket) {
            Ok(stream) => break stream,
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused
                ) && Instant::now() < deadline =>
            {
                std::thread::yield_now()
            }
            Err(error) => return Err(error),
        }
    };
    let grant = host.grant_update().map_err(io::Error::other)?;
    write_update(&mut stream, &grant)?;
    write_update(&mut stream, &host.revoke_update()?)?;
    println!("OFFLINE SIMULATION | signed host updates delivered; no GNOME input");
    Ok(())
}

/// Splits an optional trailing `--emergency-socket <absolute path>` off the service arguments.
fn split_emergency_socket(args: &[OsString]) -> (&[OsString], Option<PathBuf>) {
    match args {
        [rest @ .., flag, socket] if flag == OsStr::new("--emergency-socket") => {
            (rest, Some(PathBuf::from(socket)))
        }
        _ => (args, None),
    }
}

fn main() {
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    let result = if args.first() == Some(&OsString::from("--offline-sim-service")) {
        let (service_args, emergency_socket) = split_emergency_socket(&args);
        service_arguments(service_args).and_then(|(state, agent, control)| {
            let directory = open_state_dir(&state)?;
            let isolation = emergency_socket
                .map(|socket| {
                    DaemonIsolation::new(socket)
                        .map(|isolation| Box::new(isolation) as Box<dyn InputIsolation>)
                })
                .transpose()?;
            service::run(&directory, &agent, &control, isolation)
        })
    } else {
        run(&args)
    };
    if let Err(error) = result {
        eprintln!("offline host refused: {error}");
        std::process::exit(1);
    }
}
