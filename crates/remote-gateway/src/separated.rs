use std::fs::File;
use std::io::{self, BufRead, BufReader};
use std::net::Shutdown;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use blackroom_core::error::{BlackroomError, ErrorCode};
use blackroom_core::state::State;
use remote_hostd::{
    offline_control::{OfflineCommand, OfflineReply, read_frame, write_frame},
    service::OfflineBootstrap,
    store::PersistentHostAuthority,
};

use crate::{InputEvent, PointerPosition, Snapshot};

struct OwnedChild(Child);

impl Drop for OwnedChild {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

pub struct SeparatedHost {
    hostd: OwnedChild,
    agent: OwnedChild,
    control: UnixStream,
    _runtime: tempfile::TempDir,
    state_directory: PathBuf,
    hostd_binary: PathBuf,
    agent_binary: PathBuf,
    simulation_proof: String,
    state: State,
    epoch: u64,
    events: Vec<InputEvent>,
    pointer: PointerPosition,
}

fn checked_reply(
    command: &OfflineCommand,
    response: &OfflineReply,
    current_epoch: u64,
) -> Result<State, BlackroomError> {
    if !response.accepted {
        return Err(BlackroomError::new(
            ErrorCode::LeaseInvalid,
            "offline authority refused command",
        ));
    }
    let expected = match command {
        OfflineCommand::Start { .. } | OfflineCommand::Input {} => State::RemoteActive,
        OfflineCommand::Revoke {} => State::LocalLocked,
        OfflineCommand::Status {} => match response.state.as_str() {
            "LOCAL_LOCKED" => State::LocalLocked,
            "REMOTE_ACTIVE" => State::RemoteActive,
            _ => {
                return Err(BlackroomError::new(
                    ErrorCode::HostUnavailable,
                    "invalid offline authority status",
                ));
            }
        },
    };
    if response.epoch < current_epoch || response.state != expected.as_str() {
        return Err(BlackroomError::new(
            ErrorCode::HostUnavailable,
            "invalid offline authority response",
        ));
    }
    match (command, response.next_proof.as_deref()) {
        (OfflineCommand::Start { .. }, Some(proof))
            if proof.len() == 64 && proof.bytes().all(|digit| digit.is_ascii_hexdigit()) => {}
        (OfflineCommand::Start { .. }, _) | (_, Some(_)) => {
            return Err(BlackroomError::new(
                ErrorCode::HostUnavailable,
                "invalid offline authority proof rotation",
            ));
        }
        _ => {}
    }
    Ok(expected)
}

impl SeparatedHost {
    pub fn launch(
        state_directory: &Path,
        hostd_binary: &Path,
        agent_binary: &Path,
    ) -> io::Result<Self> {
        let runtime = tempfile::tempdir()?;
        std::fs::set_permissions(runtime.path(), std::fs::Permissions::from_mode(0o700))?;
        let agent_socket = runtime.path().join("agent.sock");
        let control_socket = runtime.path().join("control.sock");
        let mut hostd = OwnedChild(
            Command::new(hostd_binary)
                .args(["--offline-sim-service", "--state-dir"])
                .arg(state_directory)
                .arg("--runtime-dir")
                .arg(runtime.path())
                .arg("--agent-socket")
                .arg(&agent_socket)
                .arg("--control-socket")
                .arg(&control_socket)
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .spawn()?,
        );
        let stdout = hostd
            .0
            .stdout
            .take()
            .ok_or_else(|| io::Error::other("missing host bootstrap"))?;
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            let mut line = String::new();
            let result = BufReader::new(stdout).read_line(&mut line).map(|_| line);
            let _ = sender.send(result);
        });
        let bootstrap_line = receiver
            .recv_timeout(Duration::from_secs(3))
            .map_err(|_| io::Error::from(io::ErrorKind::TimedOut))??;
        let bootstrap: OfflineBootstrap = serde_json::from_str(&bootstrap_line).map_err(|_| {
            io::Error::new(io::ErrorKind::InvalidData, "invalid offline host bootstrap")
        })?;
        let agent = OwnedChild(
            Command::new(agent_binary)
                .arg("--offline-sim-agent-service")
                .arg("--socket")
                .arg(&agent_socket)
                .arg("--verifier-hex")
                .arg(&bootstrap.verifier_hex)
                .arg("--epoch")
                .arg(bootstrap.epoch.to_string())
                .arg("--host-uid")
                .arg(rustix::process::getuid().as_raw().to_string())
                .arg("--state-dir")
                .arg(state_directory)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .spawn()?,
        );
        let deadline = Instant::now() + Duration::from_secs(3);
        let control = loop {
            match UnixStream::connect(&control_socket) {
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
        control.set_read_timeout(Some(Duration::from_secs(1)))?;
        control.set_write_timeout(Some(Duration::from_secs(1)))?;
        Ok(Self {
            hostd,
            agent,
            control,
            _runtime: runtime,
            state_directory: state_directory.to_owned(),
            hostd_binary: hostd_binary.to_owned(),
            agent_binary: agent_binary.to_owned(),
            simulation_proof: bootstrap.simulation_proof,
            state: State::LocalLocked,
            epoch: bootstrap.epoch,
            events: Vec::new(),
            pointer: PointerPosition { x: 50.0, y: 55.0 },
        })
    }

    pub fn snapshot(&mut self) -> Snapshot {
        match self.emergency_epoch() {
            Ok(Some(epoch)) => {
                self.epoch = self.epoch.max(epoch);
                self.state = State::FailedSafe;
            }
            Err(()) => self.state = State::FailedSafe,
            Ok(None)
                if !matches!(self.hostd.0.try_wait(), Ok(None))
                    || !matches!(self.agent.0.try_wait(), Ok(None)) =>
            {
                match self.recovery_epoch() {
                    Ok(Some(epoch)) => {
                        self.epoch = self.epoch.max(epoch);
                        self.state = State::FailedSafe;
                    }
                    Err(()) => self.state = State::FailedSafe,
                    Ok(None) => self.state = State::LocalLocked,
                }
            }
            Ok(None) => {
                let _ = self.request(OfflineCommand::Status {});
            }
        }
        Snapshot {
            mode: "OFFLINE_SIMULATION",
            live_control: false,
            authority_store: "SEPARATE",
            state: self.state.as_str(),
            epoch: self.epoch,
            auth_blocked: false,
            events: self.events.clone(),
            pointer: self.pointer,
        }
    }

    fn emergency_pending(&self) -> bool {
        !matches!(self.emergency_epoch(), Ok(None))
    }

    fn emergency_epoch(&self) -> Result<Option<u64>, ()> {
        File::open(&self.state_directory)
            .and_then(|directory| PersistentHostAuthority::emergency_epoch(&directory))
            .map(|epoch| epoch.map(|value| value.value()))
            .map_err(|_| ())
    }

    fn recovery_epoch(&self) -> Result<Option<u64>, ()> {
        File::open(&self.state_directory)
            .and_then(|directory| PersistentHostAuthority::recovery_epoch(&directory))
            .map(|epoch| epoch.map(|value| value.value()))
            .map_err(|_| ())
    }

    fn request(&mut self, command: OfflineCommand) -> Result<(), BlackroomError> {
        if self.emergency_pending() {
            self.state = State::FailedSafe;
            return Err(BlackroomError::new(
                ErrorCode::EmergencyTriggered,
                "offline emergency stop requires local recovery",
            ));
        }
        let response = (|| -> io::Result<OfflineReply> {
            if self.hostd.0.try_wait()?.is_some() || self.agent.0.try_wait()?.is_some() {
                return Err(io::Error::from(io::ErrorKind::BrokenPipe));
            }
            write_frame(&mut self.control, &command)?;
            read_frame(&mut self.control)
        })();
        let response = match response {
            Ok(response) => response,
            Err(_) => {
                self.state =
                    if self.emergency_pending() || !matches!(self.recovery_epoch(), Ok(None)) {
                        State::FailedSafe
                    } else {
                        State::LocalLocked
                    };
                return Err(BlackroomError::new(
                    ErrorCode::HostUnavailable,
                    "offline authority process unavailable",
                ));
            }
        };
        if matches!(command, OfflineCommand::Start { .. })
            && self.state == State::RemoteActive
            && !response.accepted
            && response.state == State::RemoteActive.as_str()
            && response.epoch == self.epoch
        {
            return Err(BlackroomError::new(
                ErrorCode::LeaseInvalid,
                "offline simulation already active",
            ));
        }
        let next = checked_reply(&command, &response, self.epoch).inspect_err(|_| {
            self.state = State::LocalLocked;
            let _ = self.control.shutdown(Shutdown::Both);
        })?;
        if matches!(command, OfflineCommand::Start { .. }) {
            self.simulation_proof = response.next_proof.expect("checked Start proof rotation");
        }
        self.epoch = response.epoch;
        self.state = next;
        Ok(())
    }

    fn relaunch(&mut self) -> Result<(), BlackroomError> {
        self.state = State::LocalLocked;
        let _ = self.control.shutdown(Shutdown::Both);
        for child in [&mut self.hostd, &mut self.agent] {
            let _ = child.0.kill();
            let _ = child.0.wait();
        }
        let replacement = Self::launch(
            &self.state_directory,
            &self.hostd_binary,
            &self.agent_binary,
        )
        .map_err(|_| {
            BlackroomError::new(
                ErrorCode::HostUnavailable,
                "offline authority processes could not restart",
            )
        })?;
        drop(std::mem::replace(self, replacement));
        Ok(())
    }

    pub fn start(&mut self, demo_code: &str) -> Result<(), BlackroomError> {
        if self.emergency_pending() {
            self.state = State::FailedSafe;
            return Err(BlackroomError::new(
                ErrorCode::EmergencyTriggered,
                "offline emergency stop requires local recovery",
            ));
        }
        let child_lost = !matches!(self.hostd.0.try_wait(), Ok(None))
            || !matches!(self.agent.0.try_wait(), Ok(None));
        if child_lost && !matches!(self.recovery_epoch(), Ok(None)) {
            self.state = State::FailedSafe;
            return Err(BlackroomError::new(
                ErrorCode::RecoveryFailed,
                "offline recovery has not been verified",
            ));
        }
        if child_lost {
            self.relaunch()?;
        }
        self.request(OfflineCommand::Start {
            proof: self.simulation_proof.clone(),
            demo_code: demo_code.to_owned(),
        })
    }

    pub fn revoke(&mut self) -> Result<(), BlackroomError> {
        self.state = if self.emergency_pending() {
            State::FailedSafe
        } else {
            State::LocalLocked
        };
        self.request(OfflineCommand::Revoke {})
    }

    pub fn input(&mut self, event: InputEvent) -> Result<(), BlackroomError> {
        if !event.valid() {
            return Err(BlackroomError::new(
                ErrorCode::IpcInvalidMessage,
                "invalid simulated input",
            ));
        }
        if self.state != State::RemoteActive {
            return Err(BlackroomError::new(
                ErrorCode::LeaseInvalid,
                "offline input requires an active session",
            ));
        }
        self.request(OfflineCommand::Input {})?;
        if let InputEvent::Move { dx, dy } = event {
            self.pointer.x = (self.pointer.x + dx / 10.0).clamp(5.0, 95.0);
            self.pointer.y = (self.pointer.y + dy / 10.0).clamp(8.0, 88.0);
        }
        if self.events.len() == 12 {
            self.events.remove(0);
        }
        self.events.push(event);
        Ok(())
    }
}

impl Drop for SeparatedHost {
    fn drop(&mut self) {
        if self.state == State::RemoteActive {
            let _ = self.revoke();
        }
        let _ = self.control.shutdown(Shutdown::Both);
        for child in [&mut self.hostd, &mut self.agent] {
            let deadline = Instant::now() + Duration::from_secs(2);
            while Instant::now() < deadline {
                match child.0.try_wait() {
                    Ok(Some(_)) | Err(_) => break,
                    Ok(None) => std::thread::yield_now(),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use remote_hostd::offline_control::DEMO_CODE;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn separated_reply_never_accepts_locked_input_or_epoch_rollback() {
        let mut reply = OfflineReply {
            accepted: true,
            state: "LOCAL_LOCKED".into(),
            epoch: 4,
            code: None,
            next_proof: None,
        };
        assert!(checked_reply(&OfflineCommand::Input {}, &reply, 4).is_err());
        reply.state = "REMOTE_ACTIVE".into();
        reply.epoch = 3;
        reply.next_proof = Some("ab".repeat(32));
        assert!(
            checked_reply(
                &OfflineCommand::Start {
                    proof: "test".into(),
                    demo_code: DEMO_CODE.into(),
                },
                &reply,
                4
            )
            .is_err()
        );
        reply.epoch = 4;
        assert_eq!(
            checked_reply(
                &OfflineCommand::Start {
                    proof: "test".into(),
                    demo_code: DEMO_CODE.into(),
                },
                &reply,
                4
            )
            .unwrap(),
            State::RemoteActive
        );
        assert!(checked_reply(&OfflineCommand::Input {}, &reply, 4).is_err());
        reply.next_proof = Some("invalid".into());
        assert!(
            checked_reply(
                &OfflineCommand::Start {
                    proof: "test".into(),
                    demo_code: DEMO_CODE.into(),
                },
                &reply,
                4
            )
            .is_err()
        );
        reply.next_proof = None;
        assert!(
            checked_reply(
                &OfflineCommand::Start {
                    proof: "test".into(),
                    demo_code: DEMO_CODE.into(),
                },
                &reply,
                4
            )
            .is_err()
        );
        assert_eq!(
            checked_reply(&OfflineCommand::Input {}, &reply, 4).unwrap(),
            State::RemoteActive
        );
        assert_eq!(
            checked_reply(&OfflineCommand::Status {}, &reply, 4).unwrap(),
            State::RemoteActive
        );
        reply.state = "LOCAL_LOCKED".into();
        reply.epoch = 5;
        assert_eq!(
            checked_reply(&OfflineCommand::Status {}, &reply, 4).unwrap(),
            State::LocalLocked
        );
        reply.epoch = 3;
        assert!(checked_reply(&OfflineCommand::Status {}, &reply, 4).is_err());
        reply.epoch = 5;
        reply.state = "FAILED_SAFE".into();
        assert!(checked_reply(&OfflineCommand::Status {}, &reply, 4).is_err());
    }

    #[test]
    #[ignore = "run with built offline agent/hostd binaries in BLACKROOM_TEST_AGENT_BIN and BLACKROOM_TEST_HOSTD_BIN"]
    fn host_status_refreshes_gateway_after_out_of_band_revoke() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let agent = std::env::var_os("BLACKROOM_TEST_AGENT_BIN").unwrap();
        let hostd = std::env::var_os("BLACKROOM_TEST_HOSTD_BIN").unwrap();
        let mut demo =
            SeparatedHost::launch(directory.path(), Path::new(&hostd), Path::new(&agent)).unwrap();
        demo.start(DEMO_CODE).unwrap();
        let active_epoch = demo.snapshot().epoch;
        write_frame(&mut demo.control, &OfflineCommand::Revoke {}).unwrap();
        let response: OfflineReply = read_frame(&mut demo.control).unwrap();
        assert!(response.accepted);
        assert_eq!(demo.state, State::RemoteActive);
        let snapshot = demo.snapshot();
        assert_eq!(snapshot.state, State::LocalLocked.as_str());
        assert!(snapshot.epoch > active_epoch);
        demo.start(DEMO_CODE).unwrap();
        assert_eq!(demo.snapshot().state, State::RemoteActive.as_str());
    }

    #[test]
    #[ignore = "run with built offline agent/hostd binaries in BLACKROOM_TEST_AGENT_BIN and BLACKROOM_TEST_HOSTD_BIN"]
    fn hostd_death_refuses_input_and_restart_stays_locked() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let agent = std::env::var_os("BLACKROOM_TEST_AGENT_BIN").unwrap();
        let hostd = std::env::var_os("BLACKROOM_TEST_HOSTD_BIN").unwrap();
        let mut demo =
            SeparatedHost::launch(directory.path(), Path::new(&hostd), Path::new(&agent)).unwrap();
        demo.start(DEMO_CODE).unwrap();
        let prior_epoch = demo.snapshot().epoch;
        demo.hostd.0.kill().unwrap();
        demo.hostd.0.wait().unwrap();
        assert_ne!(demo.snapshot().state, State::RemoteActive.as_str());
        assert!(demo.input(InputEvent::Key { code: 30 }).is_err());
        let deadline = Instant::now() + Duration::from_secs(3);
        while demo.agent.0.try_wait().unwrap().is_none() && Instant::now() < deadline {
            std::thread::yield_now();
        }
        assert!(demo.agent.0.try_wait().unwrap().is_some());
        assert_eq!(demo.snapshot().state, State::LocalLocked.as_str());
        assert!(demo.snapshot().events.is_empty());
        demo.start(DEMO_CODE).unwrap();
        assert!(demo.snapshot().epoch > prior_epoch);
        demo.input(InputEvent::Key { code: 30 }).unwrap();
        assert_eq!(demo.snapshot().events.len(), 1);
        drop(demo);
        let mut restarted =
            SeparatedHost::launch(directory.path(), Path::new(&hostd), Path::new(&agent)).unwrap();
        assert_eq!(restarted.snapshot().state, State::LocalLocked.as_str());
        assert!(restarted.snapshot().epoch > prior_epoch);
    }

    #[test]
    #[ignore = "run with built offline agent/hostd binaries in BLACKROOM_TEST_AGENT_BIN and BLACKROOM_TEST_HOSTD_BIN"]
    fn agent_death_blocks_restart_until_recovery_is_verified() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let agent = std::env::var_os("BLACKROOM_TEST_AGENT_BIN").unwrap();
        let hostd = std::env::var_os("BLACKROOM_TEST_HOSTD_BIN").unwrap();
        let mut demo =
            SeparatedHost::launch(directory.path(), Path::new(&hostd), Path::new(&agent)).unwrap();
        demo.start(DEMO_CODE).unwrap();
        let prior_epoch = demo.snapshot().epoch;
        demo.agent.0.kill().unwrap();
        demo.agent.0.wait().unwrap();
        assert_eq!(demo.snapshot().state, State::FailedSafe.as_str());
        assert!(demo.input(InputEvent::Key { code: 30 }).is_err());
        assert_eq!(demo.snapshot().state, State::FailedSafe.as_str());
        assert!(demo.snapshot().events.is_empty());
        assert_eq!(
            demo.start(DEMO_CODE).unwrap_err().code,
            ErrorCode::RecoveryFailed
        );
        let dirfd = File::open(directory.path()).unwrap();
        assert_eq!(
            PersistentHostAuthority::recovery_epoch(&dirfd)
                .unwrap()
                .unwrap()
                .value(),
            prior_epoch
        );
        drop(demo);
        assert!(
            SeparatedHost::launch(directory.path(), Path::new(&hostd), Path::new(&agent)).is_err()
        );
    }

    #[test]
    #[ignore = "run with built offline agent/hostd/emergency binaries in BLACKROOM_TEST_AGENT_BIN, BLACKROOM_TEST_HOSTD_BIN and BLACKROOM_TEST_EMERGENCY_BIN"]
    fn emergency_process_blocks_input_even_after_agent_or_hostd_loss() {
        let agent = std::env::var_os("BLACKROOM_TEST_AGENT_BIN").unwrap();
        let hostd = std::env::var_os("BLACKROOM_TEST_HOSTD_BIN").unwrap();
        let emergency = std::env::var_os("BLACKROOM_TEST_EMERGENCY_BIN").unwrap();
        for loss in ["none", "agent", "hostd", "paused-hostd"] {
            let directory = tempfile::tempdir().unwrap();
            std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700))
                .unwrap();
            let mut demo =
                SeparatedHost::launch(directory.path(), Path::new(&hostd), Path::new(&agent))
                    .unwrap();
            demo.start(DEMO_CODE).unwrap();
            let before = demo.snapshot().epoch;
            match loss {
                "agent" => {
                    demo.agent.0.kill().unwrap();
                    demo.agent.0.wait().unwrap();
                }
                "hostd" => {
                    demo.hostd.0.kill().unwrap();
                    demo.hostd.0.wait().unwrap();
                }
                "paused-hostd" => {
                    assert!(
                        Command::new("/usr/bin/kill")
                            .args(["-STOP", &demo.hostd.0.id().to_string()])
                            .status()
                            .unwrap()
                            .success()
                    );
                }
                _ => {}
            }
            let result = Command::new(&emergency)
                .args(["--offline-sim-emergency", "--state-dir"])
                .arg(directory.path())
                .output()
                .unwrap();
            assert!(result.status.success(), "{loss}");
            let stopped = demo.snapshot();
            assert_eq!(stopped.state, State::FailedSafe.as_str(), "{loss}");
            assert!(stopped.epoch > before, "{loss}");
            assert!(demo.input(InputEvent::Key { code: 30 }).is_err(), "{loss}");
            assert!(demo.snapshot().events.is_empty(), "{loss}");
            assert_eq!(
                demo.start(DEMO_CODE).unwrap_err().code,
                ErrorCode::EmergencyTriggered
            );
            let persisted_epoch = u64::from_be_bytes(
                std::fs::read(directory.path().join("security-epoch"))
                    .unwrap()
                    .try_into()
                    .unwrap(),
            );
            assert!(persisted_epoch > before, "{loss}");
            drop(demo);
            assert!(
                SeparatedHost::launch(directory.path(), Path::new(&hostd), Path::new(&agent))
                    .is_err(),
                "{loss}"
            );
        }
    }

    #[test]
    #[ignore = "run with built offline agent/hostd binaries in BLACKROOM_TEST_AGENT_BIN and BLACKROOM_TEST_HOSTD_BIN"]
    fn failed_explicit_relaunch_never_reopens_input() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let agent = std::env::var_os("BLACKROOM_TEST_AGENT_BIN").unwrap();
        let hostd = std::env::var_os("BLACKROOM_TEST_HOSTD_BIN").unwrap();
        let mut demo =
            SeparatedHost::launch(directory.path(), Path::new(&hostd), Path::new(&agent)).unwrap();
        demo.start(DEMO_CODE).unwrap();
        let previous_epoch = demo.snapshot().epoch;
        demo.hostd.0.kill().unwrap();
        demo.hostd.0.wait().unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        while demo.agent.0.try_wait().unwrap().is_none() && Instant::now() < deadline {
            std::thread::yield_now();
        }
        assert!(demo.agent.0.try_wait().unwrap().is_some());
        let dirfd = File::open(directory.path()).unwrap();
        assert!(
            PersistentHostAuthority::recovery_epoch(&dirfd)
                .unwrap()
                .is_none()
        );
        demo.hostd_binary = directory.path().join("missing-offline-hostd");
        assert_eq!(
            demo.start(DEMO_CODE).unwrap_err().code,
            ErrorCode::HostUnavailable
        );
        assert_eq!(demo.snapshot().state, State::LocalLocked.as_str());
        assert!(demo.input(InputEvent::Key { code: 30 }).is_err());
        assert!(demo.snapshot().events.is_empty());
        drop(demo);
        let mut restarted =
            SeparatedHost::launch(directory.path(), Path::new(&hostd), Path::new(&agent)).unwrap();
        assert!(restarted.snapshot().epoch > previous_epoch);
        assert_eq!(restarted.snapshot().state, State::LocalLocked.as_str());
    }
}
