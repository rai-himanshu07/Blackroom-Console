use std::ffi::{OsStr, OsString};
use std::fs::File;
use std::io;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use remote_hostd::authd::{
    ADMIN_SOCKET, AUTH_SOCKET, AuthDaemon, EnableAuthorizer, OwnerOnly, PamFactory, Polkit,
    bind_private, serve,
};
use remote_hostd::isolation::{DaemonIsolation, InputIsolation};
use remote_hostd::login::MultiFactorVerifier;
use remote_hostd::password::{PamHelper, PasswordCheck};
use remote_hostd::ratelimit::FailureLimiter;
use remote_hostd::totp::{self, Limits};
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

const POLKIT_CHECK: &str = "/usr/bin/pkcheck";

fn flag_value(args: &[OsString], flag: &str) -> Option<PathBuf> {
    let at = args.iter().position(|arg| arg == OsStr::new(flag))?;
    args.get(at + 1).map(PathBuf::from)
}

/// `--auth-service --state-dir D --runtime-dir R --pam-helper P [--polkit]`: the real login
/// authority (Phase 11). Fails closed: nothing starts unless a TOTP account is enrolled.
fn auth_service(args: &[OsString]) -> io::Result<()> {
    let invalid = |message: &'static str| io::Error::new(io::ErrorKind::InvalidInput, message);
    let known = [
        "--auth-service",
        "--state-dir",
        "--runtime-dir",
        "--pam-helper",
        "--polkit",
    ];
    if args.iter().any(|arg| {
        arg.to_str()
            .is_some_and(|text| text.starts_with("--") && !known.contains(&text))
    }) {
        return Err(invalid("unknown option"));
    }
    let state = flag_value(args, "--state-dir").ok_or_else(|| invalid("--state-dir required"))?;
    let runtime =
        flag_value(args, "--runtime-dir").ok_or_else(|| invalid("--runtime-dir required"))?;
    let helper =
        flag_value(args, "--pam-helper").ok_or_else(|| invalid("--pam-helper required"))?;
    if !state.is_absolute() || !runtime.is_absolute() || !helper.is_absolute() {
        return Err(invalid("all paths must be absolute"));
    }
    if !helper.is_file() {
        return Err(invalid("--pam-helper is not a file"));
    }
    let directory = open_state_dir(&state)?;
    if totp::enrolled_accounts(&directory)?.is_empty() {
        return Err(invalid(
            "no TOTP account enrolled: run `blackroom --state-dir <dir> enroll --account <name>`",
        ));
    }
    let store = blackroom_store::SecretStore::open(&directory).map_err(io::Error::other)?;
    let verifier = MultiFactorVerifier::new(
        store,
        totp::load_verifier(&directory, Limits::default())?,
        None,
        FailureLimiter::new(Limits::default()),
    );
    let gate: Box<dyn EnableAuthorizer> = if args.iter().any(|arg| arg == OsStr::new("--polkit")) {
        Box::new(Polkit {
            pkcheck: PathBuf::from(POLKIT_CHECK),
        })
    } else {
        Box::new(OwnerOnly)
    };
    let daemon = Arc::new(AuthDaemon::new(&directory, verifier, gate)?);
    let auth = bind_private(&runtime, AUTH_SOCKET)?;
    let admin = bind_private(&runtime, ADMIN_SOCKET)?;
    println!(
        "AUTH SERVICE READY | sockets {} and {}",
        runtime.join(AUTH_SOCKET).display(),
        runtime.join(ADMIN_SOCKET).display()
    );
    // SIGTERM ends the process; stale sockets are replaced at the next start.
    serve(
        &daemon,
        auth,
        admin,
        &(Arc::new(move || Box::new(PamHelper::new(helper.clone())) as Box<dyn PasswordCheck>)
            as PamFactory),
        &Arc::new(AtomicBool::new(false)),
    )
}

fn main() {
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    let result = if args.first() == Some(&OsString::from("--auth-service")) {
        auth_service(&args)
    } else if args.first() == Some(&OsString::from("--offline-sim-service")) {
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
