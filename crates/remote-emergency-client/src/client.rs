//! Blocking client for the control socket, for `remote-hostd` or the session agent: isolate with a
//! lease, renew it, restore, and collect the `released` events the daemon pushes on its own.

use std::collections::VecDeque;
use std::io::{self, BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::Duration;

use crate::proto::{MAX_LINE_BYTES, Message, Request};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Isolated {
        nodes: usize,
    },
    /// The daemon declined (`keys_held`, `busy`, `nothing_to_grab`, `grab_failed`, `bad_lease`).
    Refused(String),
    /// A request the daemon cannot honour now (`grabs_disabled`, `not_isolated`).
    Error(String),
}

pub struct Client {
    stream: UnixStream,
    reader: BufReader<UnixStream>,
    released: VecDeque<String>,
}

impl Client {
    pub fn connect(path: &Path, reply_timeout: Duration) -> io::Result<Self> {
        let stream = UnixStream::connect(path)?;
        stream.set_read_timeout(Some(reply_timeout))?;
        stream.set_write_timeout(Some(reply_timeout))?;
        let reader = BufReader::new(stream.try_clone()?);
        Ok(Self {
            stream,
            reader,
            released: VecDeque::new(),
        })
    }

    /// The connection, e.g. to check the daemon's peer credentials.
    pub fn socket(&self) -> &UnixStream {
        &self.stream
    }

    /// Replaces the per-reply timeout, e.g. a long one for `isolate` and a short one afterwards.
    pub fn set_reply_timeout(&mut self, timeout: Duration) -> io::Result<()> {
        self.stream.set_read_timeout(Some(timeout))?;
        self.stream.set_write_timeout(Some(timeout))
    }

    fn send(&mut self, request: &Request) -> io::Result<()> {
        let mut bytes = serde_json::to_vec(request).map_err(io::Error::other)?;
        bytes.push(b'\n');
        self.stream.write_all(&bytes)?;
        self.stream.flush()
    }

    fn read_message(&mut self) -> io::Result<Message> {
        let mut line = String::new();
        let read =
            io::Read::take(&mut self.reader, MAX_LINE_BYTES as u64 + 1).read_line(&mut line)?;
        if read == 0 {
            return Err(io::Error::from(io::ErrorKind::UnexpectedEof));
        }
        serde_json::from_str(&line)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "malformed reply"))
    }

    /// Reads until `done` accepts a message; pushed `released` events are kept for later.
    fn until(&mut self, done: impl Fn(&Message) -> bool) -> io::Result<Message> {
        loop {
            let message = self.read_message()?;
            if message.event == "released" && !done(&message) {
                self.released.push_back(message.reason.unwrap_or_default());
                continue;
            }
            if done(&message) {
                return Ok(message);
            }
        }
    }

    /// Starts the gate and grab and waits for the daemon's verdict, which can take the gate's
    /// full timeout while a key is held down.
    pub fn isolate(&mut self, lease_ms: u64) -> io::Result<Outcome> {
        self.send(&Request::Isolate { lease_ms })?;
        let message =
            self.until(|m| matches!(m.event.as_str(), "isolated" | "refused" | "error"))?;
        Ok(match message.event.as_str() {
            "isolated" => Outcome::Isolated {
                nodes: message.nodes.unwrap_or(0),
            },
            "refused" => Outcome::Refused(message.reason.unwrap_or_default()),
            _ => Outcome::Error(message.reason.unwrap_or_default()),
        })
    }

    /// `true` when the lease was extended.
    pub fn renew(&mut self) -> io::Result<bool> {
        self.send(&Request::Renew {})?;
        Ok(self
            .until(|m| matches!(m.event.as_str(), "accepted" | "error"))?
            .event
            == "accepted")
    }

    pub fn restore(&mut self) -> io::Result<()> {
        self.send(&Request::Restore {})?;
        self.until(|m| m.event == "accepted")?;
        Ok(())
    }

    pub fn status(&mut self) -> io::Result<Message> {
        self.send(&Request::Status {})?;
        self.until(|m| m.event == "status")
    }

    /// Reasons of `released` events received so far (`chord`, `lease_expired`, ...).
    pub fn take_released(&mut self) -> Vec<String> {
        self.released.drain(..).collect()
    }

    /// Waits up to the reply timeout for a pushed `released` event.
    pub fn wait_released(&mut self) -> io::Result<String> {
        if let Some(reason) = self.released.pop_front() {
            return Ok(reason);
        }
        Ok(self
            .until(|m| m.event == "released")?
            .reason
            .unwrap_or_default())
    }
}
