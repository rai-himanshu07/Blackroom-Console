use std::fs::File;
use std::io::{self, Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::time::{Duration, Instant, SystemTime};

use blackroom_core::protocol::AuthorityUpdate;
use blackroom_core::state::State;
use serde::{Deserialize, Serialize};

use crate::{
    offline_control::{OfflineCommand, OfflineReply, read_frame, write_frame},
    store::PersistentHostAuthority,
    write_update,
};

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OfflineBootstrap {
    pub verifier_hex: String,
    pub epoch: u64,
    pub simulation_proof: String,
}

fn accept_gateway(listener: &UnixListener) -> io::Result<UnixStream> {
    listener.set_nonblocking(true)?;
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        match listener.accept() {
            Ok((stream, _)) => {
                let credentials = rustix::net::sockopt::socket_peercred(&stream)?;
                if credentials.uid.as_raw() != rustix::process::getuid().as_raw() {
                    return Err(io::Error::from(io::ErrorKind::PermissionDenied));
                }
                return Ok(stream);
            }
            Err(error)
                if error.kind() == io::ErrorKind::WouldBlock && Instant::now() < deadline =>
            {
                std::thread::yield_now()
            }
            Err(error) => return Err(error),
        }
    }
}

fn connect_agent(socket: &Path) -> io::Result<UnixStream> {
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        match UnixStream::connect(socket) {
            Ok(stream) => {
                let credentials = rustix::net::sockopt::socket_peercred(&stream)?;
                if credentials.uid.as_raw() != rustix::process::getuid().as_raw() {
                    return Err(io::Error::from(io::ErrorKind::PermissionDenied));
                }
                stream.set_read_timeout(Some(Duration::from_millis(300)))?;
                stream.set_write_timeout(Some(Duration::from_millis(300)))?;
                return Ok(stream);
            }
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
    }
}

fn acknowledged(
    stream: &mut UnixStream,
    update: &AuthorityUpdate,
    directory: &File,
) -> io::Result<bool> {
    write_update(stream, update)?;
    let mut response = [0_u8; 1];
    stream.read_exact(&mut response)?;
    match response[0] {
        0 => Ok(false),
        1 => Ok(true),
        2 => {
            PersistentHostAuthority::emergency_stop(directory)?;
            Err(io::Error::other(
                "agent reported unverified offline recovery",
            ))
        }
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid agent acknowledgement",
        )),
    }
}

fn reply(host: &PersistentHostAuthority, accepted: bool, code: Option<&str>) -> OfflineReply {
    OfflineReply {
        accepted,
        state: host.state().as_str().into(),
        epoch: host.epoch().value(),
        code: code.map(str::to_owned),
    }
}

fn expire_grant(
    host: &mut PersistentHostAuthority,
    agent: &mut UnixStream,
    grant: &mut Option<AuthorityUpdate>,
    directory: &File,
    now: SystemTime,
) -> io::Result<()> {
    if !matches!(grant, Some(AuthorityUpdate::Grant { lease, .. }) if now >= lease.expires_at) {
        return Ok(());
    }
    let Some(AuthorityUpdate::Grant { lease, .. }) = grant.take() else {
        unreachable!("expired grant must be present");
    };
    let revoke = host.revoke_update()?;
    if acknowledged(agent, &revoke, directory)? {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "agent did not revoke expired grant",
        ));
    }
    host.complete_recovery(lease.security_epoch)?;
    Ok(())
}

/// Deliberately offline and single-peer; no GNOME session is created.
pub fn run(directory: &File, agent_socket: &Path, control_socket: &Path) -> io::Result<()> {
    let mut host = PersistentHostAuthority::open(directory)?;
    if host.emergency_required() {
        return Err(io::Error::from(io::ErrorKind::PermissionDenied));
    }
    let listener = UnixListener::bind(control_socket)?;
    let mut proof_bytes = [0_u8; 32];
    getrandom::fill(&mut proof_bytes).map_err(|error| io::Error::other(error.to_string()))?;
    let simulation_proof = hex::encode(proof_bytes);
    let bootstrap = OfflineBootstrap {
        verifier_hex: hex::encode(host.verifying_key().to_bytes()),
        epoch: host.epoch().value(),
        simulation_proof: simulation_proof.clone(),
    };
    let mut stdout = io::stdout().lock();
    serde_json::to_writer(&mut stdout, &bootstrap).map_err(io::Error::other)?;
    stdout.write_all(b"\n")?;
    stdout.flush()?;
    drop(stdout);

    let mut gateway = accept_gateway(&listener)?;
    let mut agent = Some(connect_agent(agent_socket)?);
    let mut current_grant: Option<AuthorityUpdate> = None;
    loop {
        if host.emergency_required() {
            return Err(io::Error::from(io::ErrorKind::PermissionDenied));
        }
        if let Some(agent) = agent.as_mut() {
            expire_grant(
                &mut host,
                agent,
                &mut current_grant,
                directory,
                SystemTime::now(),
            )?;
        }
        let mut fds = [rustix::event::PollFd::new(
            &gateway,
            rustix::event::PollFlags::IN,
        )];
        let timeout = rustix::event::Timespec {
            tv_sec: 1,
            tv_nsec: 0,
        };
        if rustix::event::poll(&mut fds, Some(&timeout))? == 0 {
            continue;
        }
        let command = match read_frame::<OfflineCommand>(&mut gateway) {
            Ok(command) => command,
            Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => break,
            Err(error) => return Err(error),
        };
        if host.emergency_required() {
            return Err(io::Error::from(io::ErrorKind::PermissionDenied));
        }
        if let Some(agent) = agent.as_mut() {
            expire_grant(
                &mut host,
                agent,
                &mut current_grant,
                directory,
                SystemTime::now(),
            )?;
        }
        let response = match command {
            OfflineCommand::Start { ref proof } if proof != &simulation_proof => {
                reply(&host, false, Some("AUTH_INVALID"))
            }
            OfflineCommand::Start { .. } if host.state() == State::RemoteActive => {
                reply(&host, false, Some("LEASE_INVALID"))
            }
            OfflineCommand::Start { .. } => {
                let update = host.grant_update().map_err(io::Error::other)?;
                if agent.is_none() {
                    agent = Some(connect_agent(agent_socket)?);
                }
                let stream = agent.as_mut().expect("agent connected");
                if acknowledged(stream, &update, directory)? {
                    current_grant = Some(update);
                    reply(&host, true, None)
                } else {
                    host.revoke_update()?;
                    return Err(io::Error::from(io::ErrorKind::PermissionDenied));
                }
            }
            OfflineCommand::Revoke {} if host.state() == State::LocalLocked => {
                reply(&host, true, None)
            }
            OfflineCommand::Revoke {} => {
                current_grant = None;
                let granted_epoch = host.epoch();
                let update = host.revoke_update()?;
                let stream = agent
                    .as_mut()
                    .ok_or_else(|| io::Error::from(io::ErrorKind::NotConnected))?;
                if acknowledged(stream, &update, directory)? {
                    return Err(io::Error::new(
                        io::ErrorKind::PermissionDenied,
                        "agent did not revoke",
                    ));
                }
                host.complete_recovery(granted_epoch)?;
                reply(&host, true, None)
            }
            OfflineCommand::Input {} => match current_grant.as_ref() {
                Some(update) => {
                    let stream = agent
                        .as_mut()
                        .ok_or_else(|| io::Error::from(io::ErrorKind::NotConnected))?;
                    if acknowledged(stream, update, directory)? {
                        reply(&host, true, None)
                    } else {
                        current_grant = None;
                        host.revoke_update()?;
                        reply(&host, false, Some("LEASE_INVALID"))
                    }
                }
                None => reply(&host, false, Some("AUTH_INVALID")),
            },
            OfflineCommand::Status {} => reply(&host, true, None),
        };
        if host.emergency_required() {
            return Err(io::Error::from(io::ErrorKind::PermissionDenied));
        }
        write_frame(&mut gateway, &response)?;
    }
    if host.state() == State::RemoteActive {
        let granted_epoch = host.epoch();
        let update = host.revoke_update()?;
        if let Some(stream) = agent.as_mut() {
            if let Ok(false) = acknowledged(stream, &update, directory) {
                host.complete_recovery(granted_epoch)?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn expiry_revokes_without_input_and_persists_epoch() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let dirfd = File::open(directory.path()).unwrap();
        let mut host = PersistentHostAuthority::open(&dirfd).unwrap();
        let mut grant = Some(host.grant_update().unwrap());
        let Some(AuthorityUpdate::Grant { lease, .. }) = grant.as_mut() else {
            panic!("expected grant");
        };
        let expiry = lease.expires_at;
        let (mut host_wire, mut agent_wire) = UnixStream::pair().unwrap();
        expire_grant(
            &mut host,
            &mut host_wire,
            &mut grant,
            &dirfd,
            expiry - Duration::from_secs(1),
        )
        .unwrap();
        assert_eq!(host.state(), State::RemoteActive);
        assert!(grant.is_some());

        let peer = std::thread::spawn(move || {
            let update: AuthorityUpdate =
                crate::offline_control::read_frame(&mut agent_wire).unwrap();
            assert!(matches!(update, AuthorityUpdate::Revoke { .. }));
            agent_wire.write_all(&[0]).unwrap();
        });
        expire_grant(&mut host, &mut host_wire, &mut grant, &dirfd, expiry).unwrap();
        peer.join().unwrap();
        assert_eq!(host.state(), State::LocalLocked);
        assert!(grant.is_none());
        assert_eq!(host.epoch().value(), 1);
        drop(host);
        let restarted = PersistentHostAuthority::open(&dirfd).unwrap();
        assert_eq!(restarted.epoch().value(), 2);
    }
}
