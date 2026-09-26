//! Owned EI client socket for a RemoteDesktop session.

use std::os::fd::{AsFd, BorrowedFd, OwnedFd};
use std::os::unix::net::UnixStream;

use blackroom_core::error::{BlackroomError, ErrorCode};
use zbus::zvariant;

pub struct EiConnection {
    context: reis::ei::Context,
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
        Ok(Self { context })
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
    use std::time::Duration;

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
}
