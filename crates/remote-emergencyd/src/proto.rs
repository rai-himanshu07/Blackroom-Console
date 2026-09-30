//! Control protocol over the daemon's Unix socket: one JSON object per line, strict fields, small
//! bounds. The controlling client is `remote-hostd` (or the agent); key codes and coordinates never
//! appear in any message.

use std::io::{self, BufRead, Write};

use serde::{Deserialize, Serialize};

/// A request line longer than this is refused and the connection dropped.
pub const MAX_LINE_BYTES: usize = 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum Request {
    /// Start the gate and grab; the result arrives as `isolated` or `refused`.
    Isolate {
        lease_ms: u64,
    },
    /// Keep the grab lease alive.
    Renew {},
    /// Release every grab now.
    Restore {},
    Status {},
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum Reply {
    /// The request was understood; `isolate` completes later with `isolated` or `refused`.
    Accepted,
    Isolated {
        nodes: usize,
    },
    Refused {
        reason: &'static str,
    },
    Released {
        reason: &'static str,
    },
    Status {
        phase: &'static str,
        held: usize,
        grabs_enabled: bool,
    },
    Error {
        reason: &'static str,
    },
    /// What the daemon did on its own after an emergency chord: `ok`, `failed` or `off`.
    Emergency {
        marker: &'static str,
        lock: &'static str,
    },
}

/// A reply as a client reads it: every field optional, nothing borrowed.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Message {
    pub event: String,
    pub reason: Option<String>,
    pub nodes: Option<usize>,
    pub phase: Option<String>,
    pub held: Option<usize>,
    pub grabs_enabled: Option<bool>,
}

/// Reads one request line, bounded. `Ok(None)` is a clean end of stream.
pub fn read_request(reader: &mut impl BufRead) -> io::Result<Option<Request>> {
    let mut line = Vec::new();
    let read =
        io::Read::take(&mut *reader, MAX_LINE_BYTES as u64 + 1).read_until(b'\n', &mut line)?;
    if read == 0 {
        return Ok(None);
    }
    if line.len() > MAX_LINE_BYTES || line.last() != Some(&b'\n') {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "request line too long or unterminated",
        ));
    }
    parse_request(&line).map(Some)
}

/// Parses one complete request line (with or without its newline).
pub fn parse_request(line: &[u8]) -> io::Result<Request> {
    serde_json::from_slice(line)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "malformed request"))
}

pub fn write_reply(writer: &mut impl Write, reply: &Reply) -> io::Result<()> {
    let mut bytes = serde_json::to_vec(reply).map_err(io::Error::other)?;
    bytes.push(b'\n');
    writer.write_all(&bytes)?;
    writer.flush()
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    fn parse(text: &str) -> io::Result<Option<Request>> {
        read_request(&mut Cursor::new(text.as_bytes().to_vec()))
    }

    #[test]
    fn requests_are_strict_one_line_objects() {
        assert_eq!(
            parse("{\"op\":\"isolate\",\"lease_ms\":5000}\n").unwrap(),
            Some(Request::Isolate { lease_ms: 5000 })
        );
        assert_eq!(
            parse("{\"op\":\"renew\"}\n").unwrap(),
            Some(Request::Renew {})
        );
        assert_eq!(parse("").unwrap(), None);
        assert!(parse("{\"op\":\"restore\",\"extra\":1}\n").is_err());
        assert!(parse("{\"op\":\"isolate\"}\n").is_err());
        assert!(parse("{\"op\":\"unknown\"}\n").is_err());
        assert!(parse("{\"op\":\"status\"}").is_err(), "unterminated line");
        let long = format!(
            "{{\"op\":\"status\",\"pad\":\"{}\"}}\n",
            "x".repeat(MAX_LINE_BYTES)
        );
        assert!(parse(&long).is_err());
    }

    #[test]
    fn replies_carry_only_reasons_and_counts() {
        let mut out = Vec::new();
        write_reply(&mut out, &Reply::Isolated { nodes: 3 }).unwrap();
        write_reply(&mut out, &Reply::Released { reason: "chord" }).unwrap();
        write_reply(
            &mut out,
            &Reply::Status {
                phase: "isolated",
                held: 3,
                grabs_enabled: true,
            },
        )
        .unwrap();
        let text = String::from_utf8(out).unwrap();
        assert_eq!(
            text,
            "{\"event\":\"isolated\",\"nodes\":3}\n{\"event\":\"released\",\"reason\":\"chord\"}\n\
             {\"event\":\"status\",\"phase\":\"isolated\",\"held\":3,\"grabs_enabled\":true}\n"
        );
    }
}
