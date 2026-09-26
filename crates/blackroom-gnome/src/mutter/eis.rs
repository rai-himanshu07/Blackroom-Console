//! Owned EI client socket for a RemoteDesktop session.

use std::os::fd::{AsFd, BorrowedFd, OwnedFd};
use std::os::unix::net::UnixStream;
use std::time::{Duration, Instant};

use blackroom_core::error::{BlackroomError, ErrorCode};
use zbus::zvariant;

pub struct EiConnection {
    context: reis::ei::Context,
    connection: Option<reis::event::Connection>,
    converter: Option<reis::event::EiEventConverter>,
}

impl EiConnection {
    pub fn from_fd(fd: zvariant::OwnedFd) -> Result<Self, BlackroomError> {
        let fd: OwnedFd = fd.into();
        let context = reis::ei::Context::new(UnixStream::from(fd)).map_err(|_| {
            BlackroomError::new(
                ErrorCode::MutterUnavailable,
                "EIS socket initialization failed",
            )
        })?;
        Ok(Self {
            context,
            connection: None,
            converter: None,
        })
    }

    pub fn handshake_sender(&mut self, timeout: Duration) -> Result<(), BlackroomError> {
        let deadline = Instant::now() + timeout;
        let mut handshake = reis::handshake::EiHandshaker::new(
            "Blackroom Console",
            reis::ei::handshake::ContextType::Sender,
        );
        loop {
            if Instant::now() >= deadline {
                return Err(BlackroomError::new(
                    ErrorCode::IpcTimeout,
                    "EIS sender handshake timed out",
                ));
            }
            while let Some(message) = self.context.pending_event() {
                if Instant::now() >= deadline {
                    return Err(BlackroomError::new(
                        ErrorCode::IpcTimeout,
                        "EIS sender handshake timed out",
                    ));
                }
                let reis::PendingRequestResult::Request(event) = message else {
                    return Err(BlackroomError::new(
                        ErrorCode::MutterUnavailable,
                        "invalid EIS handshake message",
                    ));
                };
                if let Some(response) = handshake.handle_event(event).map_err(|_| {
                    BlackroomError::new(ErrorCode::MutterUnavailable, "EIS sender handshake failed")
                })? {
                    let converter = reis::event::EiEventConverter::new(&self.context, response);
                    self.connection = Some(converter.connection().clone());
                    self.converter = Some(converter);
                    return Ok(());
                }
            }
            if !Self::readable_until(&self.context, deadline)? {
                return Err(BlackroomError::new(
                    ErrorCode::IpcTimeout,
                    "EIS sender handshake timed out",
                ));
            }
            match self.context.read() {
                Ok(_) => {}
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(_) => {
                    return Err(BlackroomError::new(
                        ErrorCode::MutterUnavailable,
                        "EIS handshake socket closed",
                    ));
                }
            }
        }
    }

    pub fn is_ready(&self) -> bool {
        self.connection.is_some() && self.converter.is_some()
    }

    pub fn next_event_until(
        &mut self,
        timeout: Duration,
    ) -> Result<Option<reis::event::EiEvent>, BlackroomError> {
        let deadline = Instant::now() + timeout;
        let converter = self.converter.as_mut().ok_or_else(|| {
            BlackroomError::new(ErrorCode::MutterUnavailable, "EIS handshake is not ready")
        })?;
        loop {
            if Instant::now() >= deadline {
                return Ok(None);
            }
            if let Some(event) = converter.next_event() {
                return Ok(Some(event));
            }
            while let Some(message) = self.context.pending_event() {
                if Instant::now() >= deadline {
                    return Ok(None);
                }
                let reis::PendingRequestResult::Request(event) = message else {
                    return Err(BlackroomError::new(
                        ErrorCode::MutterUnavailable,
                        "invalid EIS device event",
                    ));
                };
                converter.handle_event(event).map_err(|_| {
                    BlackroomError::new(ErrorCode::MutterUnavailable, "EIS device event failed")
                })?;
            }
            if let Some(event) = converter.next_event() {
                return Ok(Some(event));
            }
            if !Self::readable_until(&self.context, deadline)? {
                return Ok(None);
            }
            match self.context.read() {
                Ok(_) => {}
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(_) => {
                    return Err(BlackroomError::new(
                        ErrorCode::MutterUnavailable,
                        "EIS device socket closed",
                    ));
                }
            }
        }
    }

    fn readable_until(
        context: &reis::ei::Context,
        deadline: Instant,
    ) -> Result<bool, BlackroomError> {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Ok(false);
        }
        let timeout = rustix::event::Timespec {
            tv_sec: i64::try_from(remaining.as_secs()).unwrap_or(i64::MAX),
            tv_nsec: i64::from(remaining.subsec_nanos()),
        };
        let mut fds = [rustix::event::PollFd::new(
            context,
            rustix::event::PollFlags::IN,
        )];
        rustix::event::poll(&mut fds, Some(&timeout))
            .map(|ready| ready > 0)
            .map_err(|_| {
                BlackroomError::new(ErrorCode::MutterUnavailable, "EIS socket poll failed")
            })
    }
}

impl AsFd for EiConnection {
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.context.as_fd()
    }
}

#[cfg(test)]
mod tests {
    use std::io::Read;
    use std::sync::mpsc;
    use std::thread;
    use std::time::{Duration, Instant};

    use super::*;

    #[test]
    fn silent_ei_peer_respects_read_deadline() -> Result<(), Box<dyn std::error::Error>> {
        let (client, mut server) = UnixStream::pair()?;
        let connection = EiConnection::from_fd(zvariant::OwnedFd::from(OwnedFd::from(client)))?;
        assert!(!EiConnection::readable_until(
            &connection.context,
            Instant::now() + Duration::from_millis(30)
        )?);
        use std::io::Write;
        server.write_all(&[1])?;
        assert!(EiConnection::readable_until(
            &connection.context,
            Instant::now() + Duration::from_millis(100)
        )?);
        Ok(())
    }

    #[test]
    fn ei_connection_owns_and_closes_the_returned_socket() -> Result<(), Box<dyn std::error::Error>>
    {
        let (client, mut server) = UnixStream::pair()?;
        server.set_read_timeout(Some(Duration::from_secs(1)))?;
        let fd = zvariant::OwnedFd::from(OwnedFd::from(client));
        let connection = EiConnection::from_fd(fd)?;
        let _ = connection.as_fd();
        drop(connection);

        let mut buffer = [0_u8; 1];
        assert_eq!(server.read(&mut buffer)?, 0);
        Ok(())
    }

    #[test]
    fn sender_handshake_with_synthetic_eis_peer() -> Result<(), Box<dyn std::error::Error>> {
        let (client, server_socket) = UnixStream::pair()?;
        let (done_tx, done_rx) = mpsc::channel();
        let server = thread::spawn(move || -> Result<(), std::io::Error> {
            let context = reis::eis::Context::new(server_socket)?;
            let mut handshake = reis::handshake::EisHandshaker::new(&context, 1);
            let deadline = Instant::now() + Duration::from_secs(2);
            while Instant::now() < deadline {
                match context.read() {
                    Ok(_) => {}
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::yield_now();
                        continue;
                    }
                    Err(error) => return Err(error),
                }
                while let Some(request) = context.pending_request() {
                    let reis::PendingRequestResult::Request(request) = request else {
                        return Err(std::io::Error::other("invalid synthetic EIS request"));
                    };
                    if let Some(response) = handshake
                        .handle_request(request)
                        .map_err(|error| std::io::Error::other(error.to_string()))?
                    {
                        if response.context_type != reis::eis::handshake::ContextType::Sender {
                            return Err(std::io::Error::other(
                                "EI client did not negotiate Sender",
                            ));
                        }
                        context
                            .flush()
                            .map_err(|error| std::io::Error::other(error.to_string()))?;
                        let converter =
                            reis::request::EisRequestConverter::new(&context, response, 1);
                        let _seat = converter.handle().add_seat(
                            Some("synthetic-keyboard"),
                            reis::request::DeviceCapability::Keyboard.into(),
                        );
                        context
                            .flush()
                            .map_err(|error| std::io::Error::other(error.to_string()))?;
                        done_rx
                            .recv_timeout(Duration::from_secs(2))
                            .map_err(std::io::Error::other)?;
                        return Ok(());
                    }
                }
            }
            Err(std::io::Error::other("synthetic EI handshake timed out"))
        });

        let mut sender = EiConnection::from_fd(zvariant::OwnedFd::from(OwnedFd::from(client)))?;
        assert!(!sender.is_ready());
        sender.handshake_sender(Duration::from_secs(2))?;
        assert!(sender.is_ready());
        let event = sender.next_event_until(Duration::from_secs(2))?;
        assert!(matches!(event, Some(reis::event::EiEvent::SeatAdded(_))));
        assert!(
            sender
                .next_event_until(Duration::from_millis(30))?
                .is_none()
        );
        done_tx.send(())?;
        server
            .join()
            .map_err(|_| std::io::Error::other("synthetic EIS server panicked"))??;
        Ok(())
    }

    #[test]
    fn sender_handshake_times_out_with_silent_peer() -> Result<(), Box<dyn std::error::Error>> {
        let (client, _server) = UnixStream::pair()?;
        let mut sender = EiConnection::from_fd(zvariant::OwnedFd::from(OwnedFd::from(client)))?;
        let error = sender
            .handshake_sender(Duration::from_millis(30))
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::IpcTimeout);
        assert!(!sender.is_ready());
        Ok(())
    }
}
