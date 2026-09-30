use std::fs::File;
use std::io::{self, Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::time::{Duration, Instant, SystemTime};

use blackroom_core::error::{BlackroomError, ErrorCode};
use blackroom_core::protocol::AuthorityUpdate;
use blackroom_core::state::State;
use serde::{Deserialize, Serialize};

use crate::{
    audit::{AuditEvent, AuditLog, RevokeCause},
    auth::{HostSessions, mint_input_grant},
    offline_control::{
        DemoCredential, DemoCredentialVerifier, OfflineCommand, OfflineReply, read_frame,
        write_frame,
    },
    store::PersistentHostAuthority,
    write_update,
};

const MAX_INVALID_START_ATTEMPTS: u8 = 5;

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
        next_proof: None,
        input_grant: None,
    }
}

fn new_proof() -> io::Result<String> {
    let mut bytes = [0_u8; 32];
    getrandom::fill(&mut bytes).map_err(|error| io::Error::other(error.to_string()))?;
    Ok(hex::encode(bytes))
}

/// Authenticates through the fake adapter, then issues a grant and its input
/// binding bound to the new session. A failure after authentication leaves
/// no live session.
fn issue_session_grant(
    host: &mut PersistentHostAuthority,
    sessions: &mut HostSessions,
    verifier: &mut DemoCredentialVerifier,
    credential: DemoCredential,
    now: SystemTime,
) -> Result<(AuthorityUpdate, String), BlackroomError> {
    let token = sessions.authenticate(verifier, credential, host.epoch(), now)?;
    let issued = mint_input_grant().and_then(|input_grant| {
        let session = sessions.resolve(&token, host.epoch(), now)?;
        Ok((host.grant_update_for(session)?, input_grant))
    });
    match issued {
        Ok((update, input_grant)) => {
            sessions.bind_grant(&token, input_grant.clone());
            Ok((update, input_grant))
        }
        Err(error) => {
            sessions.revoke(&token);
            Err(error)
        }
    }
}

/// Re-signs the active grant for the session that owns `grant_id`.
fn renew_session_grant(
    host: &mut PersistentHostAuthority,
    sessions: &HostSessions,
    grant_id: &str,
    now: SystemTime,
) -> Result<AuthorityUpdate, BlackroomError> {
    let session = sessions.session_for_grant(grant_id, host.epoch(), now)?;
    host.renew_update_for(session)
}

/// Revokes a grant whose lease expired or whose authentication session ended.
fn expire_grant(
    host: &mut PersistentHostAuthority,
    agent: &mut UnixStream,
    grant: &mut Option<AuthorityUpdate>,
    sessions: &mut HostSessions,
    audit: &mut AuditLog,
    directory: &File,
    now: SystemTime,
) -> io::Result<()> {
    let lease_expired =
        matches!(grant, Some(AuthorityUpdate::Grant { lease, .. }) if now >= lease.expires_at);
    let session_ended = grant.is_some() && sessions.grant_session_lost(host.epoch(), now);
    if !lease_expired && !session_ended {
        return Ok(());
    }
    let Some(AuthorityUpdate::Grant { lease, .. }) = grant.take() else {
        unreachable!("revocable grant must be present");
    };
    sessions.clear();
    let revoke = host.revoke_update()?;
    if acknowledged(agent, &revoke, directory)? {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "agent did not revoke ended grant",
        ));
    }
    host.complete_recovery(lease.security_epoch)?;
    let cause = if lease_expired {
        RevokeCause::LeaseExpired
    } else {
        RevokeCause::SessionEnded
    };
    let _ = audit.record(&AuditEvent::GrantRevoked {
        epoch: lease.security_epoch.value(),
        cause,
    });
    Ok(())
}

/// Deliberately offline and single-peer; no GNOME session is created.
pub fn run(directory: &File, agent_socket: &Path, control_socket: &Path) -> io::Result<()> {
    let mut host = PersistentHostAuthority::open(directory)?;
    if host.emergency_required() {
        return Err(io::Error::from(io::ErrorKind::PermissionDenied));
    }
    let mut audit = AuditLog::open(directory)?;
    audit.record(&AuditEvent::HostStarted {
        epoch: host.epoch().value(),
    })?;
    let listener = UnixListener::bind(control_socket)?;
    let mut verifier = DemoCredentialVerifier::new(new_proof()?);
    let mut sessions = HostSessions::default();
    let bootstrap = OfflineBootstrap {
        verifier_hex: hex::encode(host.verifying_key().to_bytes()),
        epoch: host.epoch().value(),
        simulation_proof: verifier.proof().to_owned(),
    };
    let mut stdout = io::stdout().lock();
    serde_json::to_writer(&mut stdout, &bootstrap).map_err(io::Error::other)?;
    stdout.write_all(b"\n")?;
    stdout.flush()?;
    drop(stdout);

    let mut gateway = accept_gateway(&listener)?;
    let mut agent = Some(connect_agent(agent_socket)?);
    let mut current_grant: Option<AuthorityUpdate> = None;
    let mut input_sequence = 0_u64;
    let mut invalid_start_attempts = 0_u8;
    loop {
        if host.emergency_required() {
            return Err(io::Error::from(io::ErrorKind::PermissionDenied));
        }
        if let Some(agent) = agent.as_mut() {
            expire_grant(
                &mut host,
                agent,
                &mut current_grant,
                &mut sessions,
                &mut audit,
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
                &mut sessions,
                &mut audit,
                directory,
                SystemTime::now(),
            )?;
        }
        let response = match command {
            OfflineCommand::Start { .. }
                if invalid_start_attempts >= MAX_INVALID_START_ATTEMPTS =>
            {
                reply(&host, false, Some("AUTH_RATE_LIMITED"))
            }
            OfflineCommand::Start { proof, demo_code } => {
                let next_proof = new_proof()?;
                match issue_session_grant(
                    &mut host,
                    &mut sessions,
                    &mut verifier,
                    DemoCredential { proof, demo_code },
                    SystemTime::now(),
                ) {
                    Err(error) if error.code == ErrorCode::AuthInvalid => {
                        invalid_start_attempts += 1;
                        let limited = invalid_start_attempts == MAX_INVALID_START_ATTEMPTS;
                        let _ = audit.record(&AuditEvent::AuthRefused {
                            code: if limited {
                                "AUTH_RATE_LIMITED"
                            } else {
                                "AUTH_INVALID"
                            },
                        });
                        if limited {
                            if host.state() == State::RemoteActive {
                                current_grant = None;
                                input_sequence = 0;
                                sessions.clear();
                                let granted_epoch = host.epoch();
                                let update = host.revoke_update()?;
                                let stream = agent
                                    .as_mut()
                                    .ok_or_else(|| io::Error::from(io::ErrorKind::NotConnected))?;
                                if acknowledged(stream, &update, directory)? {
                                    return Err(io::Error::new(
                                        io::ErrorKind::PermissionDenied,
                                        "agent did not revoke after invalid Start attempts",
                                    ));
                                }
                                host.complete_recovery(granted_epoch)?;
                                let _ = audit.record(&AuditEvent::GrantRevoked {
                                    epoch: granted_epoch.value(),
                                    cause: RevokeCause::AbuseLimit,
                                });
                            }
                            reply(&host, false, Some("AUTH_RATE_LIMITED"))
                        } else {
                            reply(&host, false, Some("AUTH_INVALID"))
                        }
                    }
                    Err(error) => reply(&host, false, Some(error.code.as_str())),
                    Ok((update, input_grant)) => {
                        if let AuthorityUpdate::Grant { lease, .. } = &update
                            && audit
                                .record(&AuditEvent::GrantIssued {
                                    epoch: lease.security_epoch.value(),
                                    user_id: &lease.user_id,
                                    client_id: &lease.client_id,
                                })
                                .is_err()
                        {
                            // The agent never saw this grant, so recovery is trivially verified.
                            let granted_epoch = host.epoch();
                            host.revoke_update()?;
                            host.complete_recovery(granted_epoch)?;
                            return Err(io::Error::other("audit log unavailable"));
                        }
                        if agent.is_none() {
                            agent = Some(connect_agent(agent_socket)?);
                        }
                        let stream = agent.as_mut().expect("agent connected");
                        if acknowledged(stream, &update, directory)? {
                            current_grant = Some(update);
                            input_sequence = 0;
                            verifier.rotate(next_proof.clone());
                            invalid_start_attempts = 0;
                            let mut response = reply(&host, true, None);
                            response.next_proof = Some(next_proof);
                            response.input_grant = Some(input_grant);
                            response
                        } else {
                            let _ = audit.record(&AuditEvent::GrantRevoked {
                                epoch: host.epoch().value(),
                                cause: RevokeCause::AgentRefused,
                            });
                            host.revoke_update()?;
                            return Err(io::Error::from(io::ErrorKind::PermissionDenied));
                        }
                    }
                }
            }
            OfflineCommand::Revoke {} if host.state() == State::LocalLocked => {
                reply(&host, true, None)
            }
            OfflineCommand::Revoke {} => {
                current_grant = None;
                input_sequence = 0;
                sessions.clear();
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
                let _ = audit.record(&AuditEvent::GrantRevoked {
                    epoch: granted_epoch.value(),
                    cause: RevokeCause::Revoked,
                });
                reply(&host, true, None)
            }
            OfflineCommand::Input {
                epoch,
                sequence,
                grant_id,
            } => match current_grant.as_ref() {
                Some(AuthorityUpdate::Grant { lease, .. })
                    if lease.security_epoch.value() == epoch
                        && sessions.input_grant_valid(
                            &grant_id,
                            host.epoch(),
                            SystemTime::now(),
                        )
                        && input_sequence.checked_add(1) == Some(sequence) =>
                {
                    let update = current_grant.as_ref().expect("checked current grant");
                    let stream = agent
                        .as_mut()
                        .ok_or_else(|| io::Error::from(io::ErrorKind::NotConnected))?;
                    if acknowledged(stream, update, directory)? {
                        input_sequence = sequence;
                        reply(&host, true, None)
                    } else {
                        current_grant = None;
                        input_sequence = 0;
                        sessions.clear();
                        let _ = audit.record(&AuditEvent::GrantRevoked {
                            epoch: host.epoch().value(),
                            cause: RevokeCause::AgentRefused,
                        });
                        host.revoke_update()?;
                        reply(&host, false, Some("LEASE_INVALID"))
                    }
                }
                Some(_) => reply(&host, false, Some("LEASE_INVALID")),
                None => reply(&host, false, Some("AUTH_INVALID")),
            },
            OfflineCommand::Renew { epoch, grant_id } => match current_grant.as_ref() {
                Some(AuthorityUpdate::Grant { lease, .. })
                    if lease.security_epoch.value() == epoch =>
                {
                    match renew_session_grant(&mut host, &sessions, &grant_id, SystemTime::now()) {
                        Ok(update) => {
                            let stream = agent
                                .as_mut()
                                .ok_or_else(|| io::Error::from(io::ErrorKind::NotConnected))?;
                            if acknowledged(stream, &update, directory)? {
                                current_grant = Some(update);
                                reply(&host, true, None)
                            } else {
                                current_grant = None;
                                input_sequence = 0;
                                sessions.clear();
                                let _ = audit.record(&AuditEvent::GrantRevoked {
                                    epoch: host.epoch().value(),
                                    cause: RevokeCause::AgentRefused,
                                });
                                host.revoke_update()?;
                                reply(&host, false, Some("LEASE_INVALID"))
                            }
                        }
                        Err(_) => {
                            let _ = audit.record(&AuditEvent::RenewRefused {
                                epoch: host.epoch().value(),
                            });
                            reply(&host, false, Some("LEASE_INVALID"))
                        }
                    }
                }
                Some(_) => {
                    let _ = audit.record(&AuditEvent::RenewRefused {
                        epoch: host.epoch().value(),
                    });
                    reply(&host, false, Some("LEASE_INVALID"))
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
        if let Some(stream) = agent.as_mut()
            && let Ok(false) = acknowledged(stream, &update, directory)
        {
            host.complete_recovery(granted_epoch)?;
            let _ = audit.record(&AuditEvent::GrantRevoked {
                epoch: granted_epoch.value(),
                cause: RevokeCause::PeerClosed,
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::offline_control::DEMO_CODE;
    use std::os::unix::fs::PermissionsExt;

    fn private_dir() -> (tempfile::TempDir, File) {
        let directory = tempfile::tempdir().unwrap();
        std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let dirfd = File::open(directory.path()).unwrap();
        (directory, dirfd)
    }

    fn presented(verifier: &DemoCredentialVerifier) -> DemoCredential {
        DemoCredential {
            proof: verifier.proof().to_owned(),
            demo_code: DEMO_CODE.into(),
        }
    }

    #[test]
    fn grant_requires_a_fake_adapter_session_and_takes_its_principal() {
        let (directory, dirfd) = private_dir();
        let mut host = PersistentHostAuthority::open(&dirfd).unwrap();
        let mut sessions = HostSessions::default();
        let mut verifier = DemoCredentialVerifier::new("a".repeat(64));
        let now = SystemTime::now();

        let refused = DemoCredential {
            proof: verifier.proof().to_owned(),
            demo_code: "wrong".into(),
        };
        let error =
            issue_session_grant(&mut host, &mut sessions, &mut verifier, refused, now).unwrap_err();
        assert_eq!(error.code, ErrorCode::AuthInvalid);
        assert_eq!(host.state(), State::LocalLocked);
        assert!(!directory.path().join("recovery-pending").exists());
        assert!(!sessions.grant_session_lost(host.epoch(), now));
        assert_eq!(sessions.live(), 0);

        let credential = presented(&verifier);
        let (update, input_grant) =
            issue_session_grant(&mut host, &mut sessions, &mut verifier, credential, now).unwrap();
        let AuthorityUpdate::Grant { lease, .. } = update else {
            panic!("expected a grant");
        };
        assert_eq!(lease.user_id, "synthetic-user");
        assert_eq!(lease.client_id, "synthetic-client");
        assert!(lease.expires_at <= now + blackroom_core::limits::AUTH_SESSION_TTL);
        assert!(!sessions.grant_session_lost(host.epoch(), now));
        assert_eq!(sessions.live(), 1);

        for _ in 0..8 {
            let credential = presented(&verifier);
            let error =
                issue_session_grant(&mut host, &mut sessions, &mut verifier, credential, now)
                    .unwrap_err();
            assert_eq!(error.code, ErrorCode::LeaseInvalid);
            assert!(!sessions.grant_session_lost(host.epoch(), now));
            assert!(sessions.input_grant_valid(&input_grant, host.epoch(), now));
            assert_eq!(sessions.live(), 1);
        }
        assert_eq!(host.state(), State::RemoteActive);
    }

    #[test]
    fn renewal_needs_the_live_grant_session_and_its_own_input_grant() {
        let (_directory, dirfd) = private_dir();
        let mut host = PersistentHostAuthority::open(&dirfd).unwrap();
        let mut sessions = HostSessions::default();
        let mut verifier = DemoCredentialVerifier::new("c".repeat(64));
        let now = SystemTime::now();
        let credential = presented(&verifier);
        let (update, input_grant) =
            issue_session_grant(&mut host, &mut sessions, &mut verifier, credential, now).unwrap();
        let AuthorityUpdate::Grant { lease: first, .. } = update else {
            panic!("expected a grant");
        };

        for wrong in [String::new(), "0".repeat(32), input_grant[1..].to_owned()] {
            let error = renew_session_grant(&mut host, &sessions, &wrong, now).unwrap_err();
            assert_eq!(error.code, ErrorCode::LeaseInvalid);
        }
        std::thread::sleep(Duration::from_millis(5));
        let AuthorityUpdate::Grant { lease: renewed, .. } =
            renew_session_grant(&mut host, &sessions, &input_grant, SystemTime::now()).unwrap()
        else {
            panic!("expected a renewed grant");
        };
        assert!(renewed.expires_at > first.expires_at);
        assert_eq!(renewed.security_epoch, first.security_epoch);
        assert_eq!(renewed.client_id, first.client_id);
        assert_eq!(host.state(), State::RemoteActive);

        assert_eq!(sessions.revoke_client("synthetic-client"), 1);
        let error = renew_session_grant(&mut host, &sessions, &input_grant, now).unwrap_err();
        assert_eq!(error.code, ErrorCode::SessionNotFound);
        sessions.clear();
        assert!(renew_session_grant(&mut host, &sessions, &input_grant, now).is_err());
    }

    #[test]
    fn ended_session_revokes_its_grant_and_ends_all_sessions() {
        let (_directory, dirfd) = private_dir();
        let mut host = PersistentHostAuthority::open(&dirfd).unwrap();
        let mut audit = AuditLog::open(&dirfd).unwrap();
        let mut sessions = HostSessions::default();
        let mut verifier = DemoCredentialVerifier::new("b".repeat(64));
        let now = SystemTime::now();
        let credential = presented(&verifier);
        let (update, input_grant) =
            issue_session_grant(&mut host, &mut sessions, &mut verifier, credential, now).unwrap();
        let mut grant = Some(update);
        let (mut host_wire, mut agent_wire) = UnixStream::pair().unwrap();
        expire_grant(
            &mut host,
            &mut host_wire,
            &mut grant,
            &mut sessions,
            &mut audit,
            &dirfd,
            now,
        )
        .unwrap();
        assert_eq!(host.state(), State::RemoteActive);
        assert!(grant.is_some());
        assert!(sessions.input_grant_valid(&input_grant, host.epoch(), now));

        assert_eq!(sessions.revoke_client("synthetic-client"), 1);
        let peer = std::thread::spawn(move || {
            let update: AuthorityUpdate =
                crate::offline_control::read_frame(&mut agent_wire).unwrap();
            assert!(matches!(update, AuthorityUpdate::Revoke { .. }));
            agent_wire.write_all(&[0]).unwrap();
        });
        expire_grant(
            &mut host,
            &mut host_wire,
            &mut grant,
            &mut sessions,
            &mut audit,
            &dirfd,
            now,
        )
        .unwrap();
        peer.join().unwrap();
        assert_eq!(host.state(), State::LocalLocked);
        assert!(grant.is_none());
        assert!(!sessions.grant_session_lost(host.epoch(), now));
        assert!(!sessions.input_grant_valid(&input_grant, host.epoch(), now));
        assert_eq!(sessions.live(), 0);
        assert_eq!(host.epoch().value(), 1);
    }

    #[test]
    fn expiry_revokes_without_input_and_persists_epoch() {
        let (_directory, dirfd) = private_dir();
        let mut host = PersistentHostAuthority::open(&dirfd).unwrap();
        let mut audit = AuditLog::open(&dirfd).unwrap();
        let mut sessions = HostSessions::default();
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
            &mut sessions,
            &mut audit,
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
        expire_grant(
            &mut host,
            &mut host_wire,
            &mut grant,
            &mut sessions,
            &mut audit,
            &dirfd,
            expiry,
        )
        .unwrap();
        peer.join().unwrap();
        assert_eq!(host.state(), State::LocalLocked);
        assert!(grant.is_none());
        assert_eq!(host.epoch().value(), 1);
        drop(host);
        let restarted = PersistentHostAuthority::open(&dirfd).unwrap();
        assert_eq!(restarted.epoch().value(), 2);
    }
}
