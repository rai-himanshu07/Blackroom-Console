#![forbid(unsafe_code)]

use std::ffi::OsStr;
use std::fs::File;
use std::io;
use std::path::Path;

use remote_hostd::store::PersistentHostAuthority;

fn run() -> io::Result<()> {
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    let [mode, flag, directory] = args.as_slice() else {
        return Err(io::Error::from(io::ErrorKind::InvalidInput));
    };
    if mode != OsStr::new("--offline-sim-emergency") || flag != OsStr::new("--state-dir") {
        return Err(io::Error::from(io::ErrorKind::InvalidInput));
    }
    let path = Path::new(directory);
    if !path.is_absolute() {
        return Err(io::Error::from(io::ErrorKind::InvalidInput));
    }
    let fd = rustix::fs::openat2(
        rustix::fs::CWD,
        path,
        rustix::fs::OFlags::RDONLY | rustix::fs::OFlags::DIRECTORY | rustix::fs::OFlags::CLOEXEC,
        rustix::fs::Mode::empty(),
        rustix::fs::ResolveFlags::NO_SYMLINKS | rustix::fs::ResolveFlags::NO_MAGICLINKS,
    )?;
    let directory = File::from(fd);
    PersistentHostAuthority::emergency_stop(&directory)?;
    println!("OFFLINE SIMULATION | emergency stop persisted; no GNOME or device access");
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("offline emergency refused: {error}");
        std::process::exit(1);
    }
}
