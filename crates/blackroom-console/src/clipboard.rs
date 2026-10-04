//! Text clipboard between the browser and the laptop, over Mutter's RemoteDesktop
//! clipboard. Explicit (one user gesture per direction), text only, size-capped,
//! rate-limited; the content is never logged.

use std::fs::File;
use std::io::{Read, Write};
use std::os::fd::OwnedFd;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

/// Largest text either way (the page and `POST /clipboard` enforce the same).
pub const MAX_BYTES: usize = 256 * 1024;
/// Minimum gap between two clipboard operations of one console.
pub const MIN_INTERVAL: Duration = Duration::from_millis(500);
/// A paste or read that takes longer is abandoned.
pub const TRANSFER_TIMEOUT: Duration = Duration::from_secs(4);
/// Offered when the browser sets the laptop clipboard, and tried in this order when reading it.
pub const TEXT_MIMES: [&str; 2] = ["text/plain;charset=utf-8", "text/plain"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClipboardError {
    /// The console was started without `--clipboard`.
    Disabled,
    NotRunning,
    TooLarge,
    /// The text itself is not acceptable (a client mistake, not a failure of the laptop).
    Invalid(&'static str),
    TooFast,
    /// The laptop clipboard is empty or holds something other than text.
    NoText,
    Failed(String),
}

impl std::fmt::Display for ClipboardError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Disabled => f.write_str("the clipboard is not enabled on this console"),
            Self::NotRunning => f.write_str("not running"),
            Self::TooLarge => write!(f, "text is larger than {} KiB", MAX_BYTES / 1024),
            Self::Invalid(reason) => f.write_str(reason),
            Self::TooFast => f.write_str("too many clipboard requests, wait a moment"),
            Self::NoText => f.write_str("the laptop clipboard holds no text"),
            Self::Failed(reason) => write!(f, "clipboard transfer failed: {reason}"),
        }
    }
}

/// Text from the browser: within the cap and free of NUL, which no text field holds.
pub fn validate(text: &str) -> Result<(), ClipboardError> {
    if text.len() > MAX_BYTES {
        return Err(ClipboardError::TooLarge);
    }
    if text.contains('\0') {
        return Err(ClipboardError::Invalid("text contains a NUL byte"));
    }
    Ok(())
}

/// Transfers whose helper thread is still alive. A thread stuck on a silent peer keeps its pipe until the peer lets go,
/// so only this many may be stuck at once; more are refused instead of leaking threads and descriptors.
pub const MAX_UNFINISHED: usize = 4;

pub struct Transfers {
    open: AtomicUsize,
    limit: usize,
}

static TRANSFERS: Transfers = Transfers::new(MAX_UNFINISHED);

struct Place(&'static Transfers);

impl Transfers {
    pub const fn new(limit: usize) -> Self {
        Self {
            open: AtomicUsize::new(0),
            limit,
        }
    }

    fn enter(&'static self) -> Option<Place> {
        self.open
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| {
                (n < self.limit).then_some(n + 1)
            })
            .ok()?;
        Some(Place(self))
    }
}

impl Drop for Place {
    fn drop(&mut self) {
        self.0.open.fetch_sub(1, Ordering::AcqRel);
    }
}

/// Writes `data` to the pipe Mutter handed out; false when the reader went away or
/// the transfer outlived `timeout`. A stuck writer thread ends with the pipe.
pub fn write_with_timeout(fd: OwnedFd, data: Vec<u8>, timeout: Duration) -> bool {
    write_within(&TRANSFERS, fd, data, timeout)
}

fn write_within(
    transfers: &'static Transfers,
    fd: OwnedFd,
    data: Vec<u8>,
    timeout: Duration,
) -> bool {
    let Some(place) = transfers.enter() else {
        return false;
    };
    let (done, result) = mpsc::channel();
    let spawned = thread::Builder::new()
        .name("clipboard-write".into())
        .spawn(move || {
            let _place = place;
            let mut file = File::from(fd);
            let _ = done.send(file.write_all(&data).and_then(|()| file.flush()).is_ok());
        });
    spawned.is_ok() && result.recv_timeout(timeout).unwrap_or(false)
}

/// Reads at most [`MAX_BYTES`] of UTF-8 text from the pipe Mutter handed out.
pub fn read_with_timeout(fd: OwnedFd, timeout: Duration) -> Result<String, ClipboardError> {
    read_within(&TRANSFERS, fd, timeout)
}

fn read_within(
    transfers: &'static Transfers,
    fd: OwnedFd,
    timeout: Duration,
) -> Result<String, ClipboardError> {
    let place = transfers.enter().ok_or_else(|| {
        ClipboardError::Failed("earlier clipboard transfers are still unfinished".into())
    })?;
    let (done, result) = mpsc::channel();
    thread::Builder::new()
        .name("clipboard-read".into())
        .spawn(move || {
            let _place = place;
            let mut data = Vec::new();
            let read = File::from(fd)
                .take(MAX_BYTES as u64 + 1)
                .read_to_end(&mut data);
            let _ = done.send(read.map(|_| data));
        })
        .map_err(|error| ClipboardError::Failed(error.to_string()))?;
    let data = result
        .recv_timeout(timeout)
        .map_err(|_| ClipboardError::Failed("the clipboard owner did not answer".into()))?
        .map_err(|error| ClipboardError::Failed(error.to_string()))?;
    if data.len() > MAX_BYTES {
        return Err(ClipboardError::TooLarge);
    }
    String::from_utf8(data).map_err(|_| ClipboardError::NoText)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::net::UnixStream;

    #[test]
    fn validation_enforces_the_cap_and_rejects_nul() {
        assert_eq!(validate("hello"), Ok(()));
        assert_eq!(validate(&"x".repeat(MAX_BYTES)), Ok(()));
        assert_eq!(
            validate(&"x".repeat(MAX_BYTES + 1)),
            Err(ClipboardError::TooLarge)
        );
        assert_eq!(
            validate("a\0b"),
            Err(ClipboardError::Invalid("text contains a NUL byte"))
        );
    }

    #[test]
    fn a_pipe_round_trip_carries_text() {
        let (writer, reader) = UnixStream::pair().unwrap();
        assert!(write_with_timeout(
            OwnedFd::from(writer),
            "grüße".as_bytes().to_vec(),
            Duration::from_secs(2)
        ));
        let text = read_with_timeout(OwnedFd::from(reader), Duration::from_secs(2)).unwrap();
        assert_eq!(text, "grüße");
    }

    #[test]
    fn reading_stops_at_the_cap_and_refuses_binary() {
        let (mut writer, reader) = UnixStream::pair().unwrap();
        thread::spawn(move || {
            let _ = writer.write_all(&vec![b'a'; MAX_BYTES + 10]);
        });
        assert_eq!(
            read_with_timeout(OwnedFd::from(reader), Duration::from_secs(2)),
            Err(ClipboardError::TooLarge)
        );
        let (mut writer, reader) = UnixStream::pair().unwrap();
        writer.write_all(&[0xff, 0xfe]).unwrap();
        drop(writer);
        assert_eq!(
            read_with_timeout(OwnedFd::from(reader), Duration::from_secs(2)),
            Err(ClipboardError::NoText)
        );
    }

    #[test]
    fn stuck_transfers_are_capped_and_free_their_place_when_the_peer_lets_go() {
        let transfers: &'static Transfers = Box::leak(Box::new(Transfers::new(2)));
        let peers: Vec<_> = (0..2)
            .map(|_| {
                let (peer, reader) = UnixStream::pair().unwrap();
                assert!(matches!(
                    read_within(transfers, OwnedFd::from(reader), Duration::from_millis(50)),
                    Err(ClipboardError::Failed(_))
                ));
                peer
            })
            .collect();
        let (_silent, third) = UnixStream::pair().unwrap();
        let started = std::time::Instant::now();
        assert!(matches!(
            read_within(transfers, OwnedFd::from(third), Duration::from_secs(2)),
            Err(ClipboardError::Failed(text)) if text.contains("unfinished")
        ));
        assert!(started.elapsed() < Duration::from_millis(500));
        let (_writer_peer, writer) = UnixStream::pair().unwrap();
        assert!(!write_within(
            transfers,
            OwnedFd::from(writer),
            vec![1],
            Duration::from_secs(2)
        ));
        drop(peers);
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while transfers.open.load(Ordering::Acquire) != 0 {
            assert!(std::time::Instant::now() < deadline, "threads did not end");
            thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn a_silent_owner_times_out() {
        let (_keep_open, reader) = UnixStream::pair().unwrap();
        assert!(matches!(
            read_with_timeout(OwnedFd::from(reader), Duration::from_millis(100)),
            Err(ClipboardError::Failed(_))
        ));
    }
}
