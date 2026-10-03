//! STUN and TURN settings for reaching the console from outside the LAN.
//!
//! The browser gets its `iceServers` from `/ice` (so TURN credentials are never baked into the page
//! and need a login to read). TURN uses the coturn "REST" scheme: a shared secret mints a username
//! `<expiry>:blackroom` and a password `base64(HMAC-SHA1(secret, username))`, so a leaked credential
//! stops working at its expiry. The same helpers configure the server's own `webrtcbin`.

use std::sync::OnceLock;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use remote_hostd::totp::hmac_sha1;
use serde_json::{Value, json};
use zeroize::Zeroizing;

#[derive(Default)]
pub struct IceConfig {
    pub stun: Vec<String>,
    pub turn: Vec<String>,
    pub turn_secret: Option<Zeroizing<Vec<u8>>>,
    pub turn_ttl: Duration,
    /// Inclusive UDP port range for the server's media, so it can be forwarded on a router.
    pub port_range: Option<(u32, u32)>,
}

static CONFIG: OnceLock<IceConfig> = OnceLock::new();

/// Set once at start-up; later calls are ignored.
pub fn install(config: IceConfig) {
    let _ = CONFIG.set(config);
}

pub fn config() -> &'static IceConfig {
    CONFIG.get_or_init(IceConfig::default)
}

const STANDARD: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn base64_padded(bytes: &[u8]) -> String {
    let mut text = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let value = chunk
            .iter()
            .enumerate()
            .fold(0_u32, |value, (index, byte)| {
                value | (u32::from(*byte) << (16 - 8 * index))
            });
        for index in 0..4 {
            if index <= chunk.len() {
                text.push(char::from(
                    STANDARD[((value >> (18 - 6 * index)) & 63) as usize],
                ));
            } else {
                text.push('=');
            }
        }
    }
    text
}

fn unix(now: SystemTime) -> u64 {
    now.duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

/// `(username, password)` valid until `now + ttl`.
pub fn turn_credentials(secret: &[u8], now: SystemTime, ttl: Duration) -> (String, String) {
    let username = format!("{}:blackroom", unix(now) + ttl.as_secs());
    let password = base64_padded(&hmac_sha1(secret, username.as_bytes()));
    (username, password)
}

/// Percent-encodes everything but unreserved characters, for the userinfo of a `turn://` URL.
fn encode(text: &str) -> String {
    text.bytes()
        .map(|byte| {
            if byte.is_ascii_alphanumeric() || b"-._~".contains(&byte) {
                char::from(byte).to_string()
            } else {
                format!("%{byte:02X}")
            }
        })
        .collect()
}

impl IceConfig {
    pub fn is_empty(&self) -> bool {
        self.stun.is_empty() && self.turn.is_empty()
    }

    /// The `RTCConfiguration.iceServers` list for the logged-in browser.
    pub fn browser_servers(&self, now: SystemTime) -> Value {
        let mut servers = Vec::new();
        if !self.stun.is_empty() {
            servers.push(json!({ "urls": self.stun }));
        }
        if !self.turn.is_empty()
            && let Some(secret) = &self.turn_secret
        {
            let (username, credential) = turn_credentials(secret, now, self.turn_ttl);
            servers
                .push(json!({ "urls": self.turn, "username": username, "credential": credential }));
        }
        json!({ "iceServers": servers, "waitMs": if self.is_empty() { 2500 } else { 6000 } })
    }

    /// `stun://host:port` for the server's own ICE agent (the first server only).
    pub fn server_stun(&self) -> Option<String> {
        let url = self.stun.first()?;
        let rest = url.strip_prefix("stun:")?.trim_start_matches("//");
        Some(format!("stun://{rest}"))
    }

    /// `turn://user:password@host:port` entries for the server's own ICE agent.
    pub fn server_turn(&self, now: SystemTime) -> Vec<String> {
        let Some(secret) = &self.turn_secret else {
            return Vec::new();
        };
        let (username, credential) = turn_credentials(secret, now, self.turn_ttl);
        self.turn
            .iter()
            .filter_map(|url| {
                let (scheme, rest) = if let Some(rest) = url.strip_prefix("turns:") {
                    ("turns", rest)
                } else {
                    ("turn", url.strip_prefix("turn:")?)
                };
                let rest = rest.trim_start_matches("//");
                Some(format!(
                    "{scheme}://{}:{}@{rest}",
                    encode(&username),
                    encode(&credential)
                ))
            })
            .collect()
    }
}

/// Accepts `stun:host[:port]` only.
pub fn valid_stun(url: &str) -> bool {
    url.strip_prefix("stun:").is_some_and(host_port_ok)
}

/// Accepts `turn:` / `turns:` with an optional `?transport=udp|tcp`.
pub fn valid_turn(url: &str) -> bool {
    let Some(rest) = url
        .strip_prefix("turns:")
        .or_else(|| url.strip_prefix("turn:"))
    else {
        return false;
    };
    let (host, query) = rest
        .split_once('?')
        .map_or((rest, None), |(host, query)| (host, Some(query)));
    host_port_ok(host)
        && query.is_none_or(|query| matches!(query, "transport=udp" | "transport=tcp"))
}

fn host_port_ok(text: &str) -> bool {
    let text = text.trim_start_matches("//");
    let (host, port) = text
        .rsplit_once(':')
        .map_or((text, None), |(host, port)| (host, Some(port)));
    !host.is_empty()
        && host.len() <= 253
        && host
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-.".contains(&byte))
        && port.is_none_or(|port| port.parse::<u16>().is_ok_and(|port| port > 0))
}

/// `MIN-MAX`, both above 1024, `MIN <= MAX`.
pub fn parse_port_range(text: &str) -> Option<(u32, u32)> {
    let (low, high) = text.split_once('-')?;
    let (low, high): (u32, u32) = (low.parse().ok()?, high.parse().ok()?);
    (low > 1024 && high <= 65535 && low <= high).then_some((low, high))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_the_rfc_vectors_with_padding() {
        assert_eq!(base64_padded(b""), "");
        assert_eq!(base64_padded(b"f"), "Zg==");
        assert_eq!(base64_padded(b"fo"), "Zm8=");
        assert_eq!(base64_padded(b"foo"), "Zm9v");
        assert_eq!(base64_padded(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn turn_credentials_follow_the_coturn_rest_scheme_and_expire() {
        let now = UNIX_EPOCH + Duration::from_secs(1_800_000_000);
        let (username, password) = turn_credentials(b"secret", now, Duration::from_secs(3600));
        assert_eq!(username, "1800003600:blackroom");
        // HMAC-SHA1("secret", "1800003600:blackroom"), base64: 20 bytes give 28 characters ending in '='.
        assert_eq!(password.len(), 28);
        assert!(password.ends_with('='));
        let again = turn_credentials(b"secret", now, Duration::from_secs(3600));
        assert_eq!((username.clone(), password.clone()), again);
        assert_ne!(
            turn_credentials(b"other", now, Duration::from_secs(3600)).1,
            password
        );
        assert_ne!(
            turn_credentials(
                b"secret",
                now + Duration::from_secs(1),
                Duration::from_secs(3600)
            )
            .0,
            username
        );
    }

    #[test]
    fn the_browser_list_carries_credentials_only_for_turn() {
        let config = IceConfig {
            stun: vec!["stun:stun.example.org:3478".into()],
            turn: vec!["turn:turn.example.org:3478?transport=udp".into()],
            turn_secret: Some(Zeroizing::new(b"secret".to_vec())),
            turn_ttl: Duration::from_secs(600),
            port_range: None,
        };
        let value = config.browser_servers(UNIX_EPOCH + Duration::from_secs(1_000));
        let servers = value["iceServers"].as_array().unwrap();
        assert_eq!(servers.len(), 2);
        assert!(servers[0].get("credential").is_none());
        assert_eq!(servers[1]["username"], "1600:blackroom");
        assert_eq!(value["waitMs"], 6000);
        assert_eq!(
            IceConfig::default().browser_servers(UNIX_EPOCH)["iceServers"],
            json!([])
        );
    }

    #[test]
    fn the_servers_own_urls_are_encoded_for_gstreamer() {
        let config = IceConfig {
            stun: vec!["stun:stun.example.org:3478".into()],
            turn: vec!["turns:turn.example.org:5349".into()],
            turn_secret: Some(Zeroizing::new(b"secret".to_vec())),
            turn_ttl: Duration::from_secs(600),
            port_range: None,
        };
        assert_eq!(
            config.server_stun().unwrap(),
            "stun://stun.example.org:3478"
        );
        let turn = config.server_turn(UNIX_EPOCH + Duration::from_secs(1_000));
        assert_eq!(turn.len(), 1);
        assert!(turn[0].starts_with("turns://1600%3Ablackroom:"));
        assert!(turn[0].ends_with("@turn.example.org:5349"));
        assert!(
            !turn[0][8..].contains('/'),
            "'/' in the password is percent-encoded"
        );
    }

    #[test]
    fn urls_and_ranges_are_validated() {
        assert!(valid_stun("stun:stun.l.google.com:19302"));
        assert!(!valid_stun("http://x") && !valid_stun("stun:") && !valid_stun("stun:a b:1"));
        assert!(
            valid_turn("turn:t.example.org:3478?transport=tcp")
                && valid_turn("turns:t.example.org")
        );
        assert!(
            !valid_turn("turn:t.example.org:3478?foo=bar") && !valid_turn("turn:t.example.org:0")
        );
        assert_eq!(parse_port_range("50000-50100"), Some((50000, 50100)));
        for bad in ["80-90", "50100-50000", "50000", "a-b", "50000-70000"] {
            assert_eq!(parse_port_range(bad), None, "{bad}");
        }
    }
}
