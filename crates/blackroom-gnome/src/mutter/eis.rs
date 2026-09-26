//! Owned EI client socket for a RemoteDesktop session.

use std::os::fd::{AsFd, BorrowedFd, OwnedFd};
use std::os::unix::net::UnixStream;

use blackroom_core::error::{BlackroomError, ErrorCode};
use zbus::zvariant;

pub struct EiConnection {
    context: reis::ei::Context,
    connection: Option<reis::event::Connection>,
    events: Option<reis::event::EiConvertEventIterator>,
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
            events: None,
        })
    }

    pub fn handshake_sender(&mut self) -> Result<(), BlackroomError> {
        let (connection, events) = self
            .context
            .handshake_blocking(
                "Blackroom Console",
                reis::ei::handshake::ContextType::Sender,
            )
            .map_err(|_| {
                BlackroomError::new(ErrorCode::MutterUnavailable, "EIS sender handshake failed")
            })?;
        self.connection = Some(connection);
        self.events = Some(events);
        Ok(())
    }

    pub fn is_ready(&self) -> bool {
        self.connection.is_some() && self.events.is_some()
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
        sender.handshake_sender()?;
        assert!(sender.is_ready());
        done_tx.send(())?;
        server
            .join()
            .map_err(|_| std::io::Error::other("synthetic EIS server panicked"))??;
        Ok(())
    }
}
