//! Loopback observer server shared by the live input experiments: serves a page
//! (random port, one-time token, Host check) and receives its focus heartbeats and
//! event tally. The page decides what is tallied; this module only transports it.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::{JoinHandle, sleep};
use std::time::{Duration, Instant};

use anyhow::Context;
use serde::Deserialize;
use serde_json::Value;

/// A heartbeat older than this no longer proves the page is focused.
pub const FRESH: Duration = Duration::from_millis(1200);
const MAX_HEADER: usize = 16 * 1024;
const MAX_BODY: usize = 256 * 1024;

#[derive(Default)]
pub struct ObserverState {
    pub beat_at: Option<Instant>,
    pub focus: bool,
    pub fullscreen: bool,
    pub tally: Option<Value>,
    pub beats: u64,
    /// Tally generation the page is asked to reset to before injection.
    pub epoch: u64,
}

#[derive(Deserialize)]
struct Beat {
    focus: bool,
    fullscreen: bool,
    tally: Value,
}

struct Request {
    method: String,
    path: String,
    host: Option<String>,
    body: Vec<u8>,
}

/// Loopback-only observer server: serves the page and receives its heartbeats.
pub struct Observer {
    state: Arc<Mutex<ObserverState>>,
    stop: Arc<AtomicBool>,
    port: u16,
    token: String,
    thread: Option<JoinHandle<()>>,
}

impl Observer {
    pub fn start(page: &'static str) -> anyhow::Result<Self> {
        let listener = TcpListener::bind(("127.0.0.1", 0)).context("observer listener")?;
        listener.set_nonblocking(true)?;
        let port = listener.local_addr()?.port();
        let mut raw = [0_u8; 16];
        getrandom::fill(&mut raw).context("observer token")?;
        let token: String = raw.iter().map(|byte| format!("{byte:02x}")).collect();
        let state = Arc::new(Mutex::new(ObserverState::default()));
        let stop = Arc::new(AtomicBool::new(false));
        let thread = {
            let (state, stop, token) = (Arc::clone(&state), Arc::clone(&stop), token.clone());
            std::thread::spawn(move || {
                while !stop.load(Ordering::Relaxed) {
                    match listener.accept() {
                        Ok((stream, _)) => handle(stream, &state, &token, port, page),
                        Err(_) => sleep(Duration::from_millis(20)),
                    }
                }
            })
        };
        Ok(Self {
            state,
            stop,
            port,
            token,
            thread: Some(thread),
        })
    }

    pub fn url(&self) -> String {
        format!("http://127.0.0.1:{}/{}/", self.port, self.token)
    }

    pub fn snapshot<T>(&self, read: impl FnOnce(&ObserverState) -> T) -> T {
        read(&self.state.lock().unwrap_or_else(PoisonError::into_inner))
    }

    pub fn ready_now(&self, max_age: Duration) -> bool {
        self.snapshot(|state| {
            state.focus
                && state.fullscreen
                && state.beat_at.is_some_and(|at| at.elapsed() <= max_age)
        })
    }

    /// True once the page has been focused and fullscreen continuously for `settle`.
    pub fn wait_ready(&self, timeout: Duration, settle: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        let mut since: Option<Instant> = None;
        let mut last_hint = Instant::now();
        while Instant::now() < deadline {
            if self.ready_now(FRESH) {
                let started = *since.get_or_insert_with(Instant::now);
                if started.elapsed() >= settle {
                    return true;
                }
            } else {
                since = None;
                if last_hint.elapsed() >= Duration::from_secs(15) {
                    println!("  still waiting for the observer page: focused and fullscreen (F11)");
                    last_hint = Instant::now();
                }
            }
            sleep(Duration::from_millis(100));
        }
        false
    }

    /// Asks the page to clear its tally (dropping pre-run F11, mouse and focus
    /// noise) and waits until a fresh focused beat carries the new epoch.
    pub fn arm(&self, timeout: Duration) -> bool {
        let epoch = {
            let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
            state.epoch += 1;
            state.epoch
        };
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            let acknowledged = self.snapshot(|state| {
                state
                    .tally
                    .as_ref()
                    .is_some_and(|tally| tally["epoch"] == epoch)
                    && state.focus
                    && state.fullscreen
                    && state.beat_at.is_some_and(|at| at.elapsed() <= FRESH)
            });
            if acknowledged {
                return true;
            }
            sleep(Duration::from_millis(50));
        }
        false
    }

    pub fn describe(&self) -> String {
        self.snapshot(|state| {
            format!(
                "focus={} fullscreen={} last_beat_ms_ago={:?} beats={}",
                state.focus,
                state.fullscreen,
                state.beat_at.map(|at| at.elapsed().as_millis()),
                state.beats
            )
        })
    }

    pub fn wait_beats(&self, extra: u64, timeout: Duration) {
        let target = self.snapshot(|state| state.beats) + extra;
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline && self.snapshot(|state| state.beats) < target {
            sleep(Duration::from_millis(50));
        }
    }
}

impl Drop for Observer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn read_request(stream: &mut TcpStream) -> Option<Request> {
    let mut buffer = Vec::new();
    let mut chunk = [0_u8; 4096];
    let header_end = loop {
        if let Some(position) = buffer.windows(4).position(|window| window == b"\r\n\r\n") {
            break position;
        }
        if buffer.len() > MAX_HEADER {
            return None;
        }
        let read = stream.read(&mut chunk).ok()?;
        if read == 0 {
            return None;
        }
        buffer.extend_from_slice(&chunk[..read]);
    };
    let head = std::str::from_utf8(&buffer[..header_end]).ok()?;
    let mut lines = head.split("\r\n");
    let mut request_line = lines.next()?.split(' ');
    let method = request_line.next()?.to_string();
    let path = request_line.next()?.to_string();
    let (mut host, mut length) = (None, 0_usize);
    for line in lines {
        let (name, value) = line.split_once(':')?;
        match name.trim().to_ascii_lowercase().as_str() {
            "host" => host = Some(value.trim().to_string()),
            "content-length" => length = value.trim().parse().ok()?,
            _ => {}
        }
    }
    if length > MAX_BODY {
        return None;
    }
    let mut body = buffer[header_end + 4..].to_vec();
    while body.len() < length {
        let read = stream.read(&mut chunk).ok()?;
        if read == 0 {
            return None;
        }
        body.extend_from_slice(&chunk[..read]);
    }
    body.truncate(length);
    Some(Request {
        method,
        path,
        host,
        body,
    })
}

fn response(status: &str, content_type: &str, body: &[u8]) -> Vec<u8> {
    let mut out = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\n\
         Cache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nConnection: close\r\n\r\n",
        body.len()
    )
    .into_bytes();
    out.extend_from_slice(body);
    out
}

fn route(
    request: &Request,
    state: &Mutex<ObserverState>,
    token: &str,
    port: u16,
    page: &'static str,
) -> Vec<u8> {
    // The Host check blocks DNS rebinding; the token blocks other local pages.
    if request.host.as_deref() != Some(&format!("127.0.0.1:{port}")) {
        return response("403 Forbidden", "text/plain", b"");
    }
    let base = format!("/{token}/");
    match request.method.as_str() {
        "GET" if request.path == base => {
            response("200 OK", "text/html; charset=utf-8", page.as_bytes())
        }
        "POST" if request.path == format!("{base}beat") => {
            match serde_json::from_slice::<Beat>(&request.body) {
                Ok(beat) => {
                    let mut state = state.lock().unwrap_or_else(PoisonError::into_inner);
                    state.beat_at = Some(Instant::now());
                    state.focus = beat.focus;
                    state.fullscreen = beat.fullscreen;
                    state.tally = Some(beat.tally);
                    state.beats += 1;
                    let reply = format!("{{\"epoch\":{}}}", state.epoch);
                    response("200 OK", "application/json", reply.as_bytes())
                }
                Err(_) => response("400 Bad Request", "text/plain", b""),
            }
        }
        _ => response("404 Not Found", "text/plain", b""),
    }
}

fn handle(
    mut stream: TcpStream,
    state: &Mutex<ObserverState>,
    token: &str,
    port: u16,
    page: &'static str,
) {
    let _ = stream.set_nonblocking(false);
    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(2)));
    if let Some(request) = read_request(&mut stream) {
        let _ = stream.write_all(&route(&request, state, token, port, page));
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    const TEST_PAGE: &str = "<html>observer-test-page</html>";

    fn http(port: u16, request: &str) -> String {
        let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("connect");
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .expect("timeout");
        stream.write_all(request.as_bytes()).expect("write");
        let mut out = String::new();
        let _ = stream.read_to_string(&mut out);
        out
    }

    #[test]
    fn observer_serves_only_its_token_and_host_and_tracks_focus() {
        let observer = Observer::start(TEST_PAGE).expect("observer");
        let (port, token) = (observer.port, observer.token.clone());
        let host = format!("Host: 127.0.0.1:{port}\r\n");
        let get = |path: &str, host: &str| format!("GET {path} HTTP/1.1\r\n{host}\r\n");
        assert!(http(port, &get(&format!("/{token}/"), &host)).contains("observer-test-page"));
        assert!(http(port, &get("/wrong/", &host)).starts_with("HTTP/1.1 404"));
        assert!(
            http(port, &get(&format!("/{token}/"), "Host: evil.example\r\n"))
                .starts_with("HTTP/1.1 403")
        );

        assert!(!observer.ready_now(FRESH));
        let body = json!({"focus": true, "fullscreen": true, "tally": {}}).to_string();
        let post = |body: &str| {
            format!(
                "POST /{token}/beat HTTP/1.1\r\n{host}Content-Length: {}\r\n\r\n{body}",
                body.len()
            )
        };
        let reply = http(port, &post(&body));
        assert!(reply.starts_with("HTTP/1.1 200") && reply.ends_with("{\"epoch\":0}"));
        assert!(observer.ready_now(FRESH));
        assert!(http(port, &post("not json")).starts_with("HTTP/1.1 400"));
        let blurred = json!({"focus": false, "fullscreen": true, "tally": {}}).to_string();
        assert!(http(port, &post(&blurred)).starts_with("HTTP/1.1 200"));
        assert!(!observer.ready_now(FRESH));
        let oversized = format!(
            "POST /{token}/beat HTTP/1.1\r\n{host}Content-Length: {}\r\n\r\n",
            MAX_BODY + 1
        );
        assert_eq!(http(port, &oversized), "");
    }

    #[test]
    fn arming_waits_for_a_focused_beat_that_carries_the_new_epoch() {
        let observer = Observer::start(TEST_PAGE).expect("observer");
        let (port, token) = (observer.port, observer.token.clone());
        let host = format!("Host: 127.0.0.1:{port}\r\n");
        let beat = |epoch: u64| {
            let body =
                json!({"focus": true, "fullscreen": true, "tally": {"epoch": epoch}}).to_string();
            http(
                port,
                &format!(
                    "POST /{token}/beat HTTP/1.1\r\n{host}Content-Length: {}\r\n\r\n{body}",
                    body.len()
                ),
            )
        };
        assert!(
            !observer.arm(Duration::from_millis(300)),
            "no beat carries the epoch"
        );
        let page = std::thread::scope(|scope| {
            let arming = scope.spawn(|| observer.arm(Duration::from_secs(3)));
            // A page that follows the reply epoch, like the real one.
            let mut epoch = 0;
            while !arming.is_finished() {
                let reply = beat(epoch);
                epoch = reply
                    .rsplit("\"epoch\":")
                    .next()
                    .and_then(|rest| rest.trim_end_matches('}').parse().ok())
                    .unwrap_or(0);
                sleep(Duration::from_millis(50));
            }
            arming.join().expect("arm thread")
        });
        assert!(page);
    }
}
