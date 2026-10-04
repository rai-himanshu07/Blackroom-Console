//! "Can this laptop be reached from outside, and is that safe?" One check, used at start-up, by `blackroom internet`
//! and by the host page, so the answer is the same everywhere. It looks at local settings and files only: it never
//! contacts the network, so whether a phone on mobile data really gets in is for the owner to try.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::certcheck;
use crate::exposure;
use crate::host::{HostConfig, PublicCert};

/// What the console will run with once `host.json` and the command line are merged.
#[derive(Debug, Clone)]
pub struct Effective {
    pub public: bool,
    pub public_name: Option<String>,
    pub public_cert: Option<PublicCert>,
    pub listen: SocketAddr,
    pub tls_listen: Option<SocketAddr>,
    pub tls_cert: Option<PathBuf>,
    pub tls_key: Option<PathBuf>,
    /// Where the console's own self-signed certificate lives.
    pub cert_dir: PathBuf,
    /// Login goes through remote-hostd, and whether its sockets exist now.
    pub hostd_login: bool,
    pub hostd_running: bool,
    pub stun: Vec<String>,
    pub turn: Vec<String>,
    pub turn_secret_file: Option<PathBuf>,
    pub ice_ports: Option<String>,
    /// `host.json` exists but is unusable: the listeners were forced onto this laptop.
    pub damaged: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct Report {
    /// `direct` (public mode on), `private-network` (a certificate is set but public mode is off) or `home`.
    pub access: &'static str,
    pub public: bool,
    /// Things that stop public mode from starting, or that already are wrong.
    pub problems: Vec<String>,
    pub warnings: Vec<String>,
    pub notes: Vec<String>,
    pub certificate: Option<certcheck::CertReport>,
    pub days_left: Option<i64>,
    /// The address clients type, once public mode is set up.
    pub url: Option<String>,
    /// The port the https listener uses (what to forward, and what a VPN address needs).
    pub https_port: Option<u16>,
    pub stun_set: bool,
    pub turn_set: bool,
    pub ice_ports: Option<String>,
    /// What to forward on the router (public mode only).
    pub forwards: Vec<String>,
    pub self_signed: bool,
}

fn url_for(name: &str, port: u16) -> String {
    if name.parse::<std::net::Ipv6Addr>().is_ok() {
        format!("https://[{name}]:{port}/")
    } else {
        format!("https://{name}:{port}/")
    }
}

pub fn preflight(e: &Effective, now_unix: i64) -> Report {
    let mut report = Report {
        access: if e.public {
            "direct"
        } else if e.tls_cert.is_some() {
            "private-network"
        } else {
            "home"
        },
        public: e.public,
        problems: Vec::new(),
        warnings: Vec::new(),
        notes: Vec::new(),
        certificate: None,
        days_left: None,
        url: None,
        https_port: e.tls_listen.map(|addr| addr.port()),
        stun_set: !e.stun.is_empty(),
        turn_set: !e.turn.is_empty(),
        ice_ports: e.ice_ports.clone(),
        forwards: Vec::new(),
        self_signed: false,
    };
    // In public mode a flaw blocks the start; elsewhere it is only worth saying.
    let flaw = |report: &mut Report, text: String| {
        if e.public {
            report.problems.push(text);
        } else {
            report.warnings.push(text);
        }
    };
    if e.damaged {
        report.problems.push(
            "host.json is damaged: the network listeners stay on this laptop until it is repaired (open the host settings page here and save, or fix the file)"
                .into(),
        );
    }
    let self_signed_choice =
        e.public_cert == Some(PublicCert::SelfSigned) && e.public_name.is_some();
    if e.public {
        report.problems.extend(
            exposure::public_problems(exposure::Facts {
                hostd_login: e.hostd_login,
                tls_listener: e.tls_listen.is_some(),
                certificate_files: e.tls_cert.is_some(),
                self_signed_for_name: self_signed_choice,
                http_loopback_only: e.listen.ip().is_loopback(),
            })
            .into_iter()
            .map(String::from),
        );
        if e.hostd_login && !e.hostd_running {
            report.problems.push(
                "the login authority (remote-hostd) is not running: systemctl --user start remote-hostd.service"
                    .into(),
            );
        }
        if e.public_name.is_none() {
            report.problems.push(
                "internet access needs the name or address your devices will use (public_name)"
                    .into(),
            );
        }
        if e.tls_cert.is_some() && e.public_cert == Some(PublicCert::SelfSigned) {
            report.problems.push(
                "choose one certificate: files from a real authority or the console's own self-signed one"
                    .into(),
            );
        }
    }

    // The certificate that will be served.
    let (cert_pem, key_pem_ok) = match (&e.tls_cert, &e.tls_key) {
        (Some(cert), Some(key)) => match certcheck::inspect_files(cert, key) {
            Ok(found) => (Some(found), true),
            Err(error) => {
                flaw(&mut report, format!("certificate: {error}"));
                (None, false)
            }
        },
        _ if e.tls_listen.is_some() => match std::fs::read(e.cert_dir.join("cert.pem")) {
            Ok(bytes) => (certcheck::inspect_pem(&bytes).ok(), true),
            Err(_) => (None, true),
        },
        _ => (None, true),
    };
    let own_certificate = e.tls_cert.is_none();
    if let Some(found) = cert_pem {
        if own_certificate && !e.public {
            // The home-network certificate is expected to be self-signed: say nothing about it.
        } else if own_certificate {
            // The console remakes its own certificate at start when a name is missing, so that is not a flaw yet.
            let (problems, warnings) = certcheck::judge(&found, None, now_unix, true);
            for text in problems {
                flaw(&mut report, format!("certificate: {text}"));
            }
            report
                .warnings
                .extend(warnings.into_iter().map(|w| format!("certificate: {w}")));
            if let Some(name) = e.public_name.as_deref()
                && !certcheck::name_covered(&found.names, name)
            {
                report.notes.push(format!(
                    "the self-signed certificate does not cover {name} yet: it is remade when the console starts; run this check again afterwards to read its fingerprint"
                ));
            }
        } else {
            let wanted = e.public_name.as_deref();
            let (problems, warnings) =
                certcheck::judge(&found, wanted, now_unix, self_signed_choice);
            for text in problems {
                flaw(&mut report, format!("certificate: {text}"));
            }
            report
                .warnings
                .extend(warnings.into_iter().map(|w| format!("certificate: {w}")));
        }
        report.days_left = Some(found.days_left(now_unix));
        report.self_signed = found.self_signed;
        report.certificate = Some(found);
    } else if self_signed_choice && e.public && key_pem_ok && own_certificate {
        report.notes.push(
            "the self-signed certificate is made when the console starts: run this check again afterwards to read its fingerprint"
                .into(),
        );
    }

    // TURN, when asked for.
    if !e.turn.is_empty() {
        match &e.turn_secret_file {
            None => report
                .problems
                .push("turn is set but turn_secret_file is not".into()),
            Some(path) => match certcheck::read_bounded(path, true) {
                Err(error) => report.problems.push(format!("TURN secret: {error}")),
                Ok(bytes) if String::from_utf8_lossy(&bytes).trim().len() < 16 => {
                    report
                        .problems
                        .push("the TURN secret file holds fewer than 16 characters".into());
                }
                Ok(_) => {}
            },
        }
    }

    if e.public {
        if let (Some(name), Some(addr)) = (&e.public_name, e.tls_listen) {
            report.url = Some(url_for(name, addr.port()));
        }
        if let Some(addr) = e.tls_listen {
            report.forwards.push(format!(
                "TCP {} to this laptop: https, the login, and video and input over the MJPEG fallback",
                addr.port()
            ));
        }
        match &e.ice_ports {
            Some(range) => report.forwards.push(format!(
                "UDP {range} to this laptop: WebRTC media (without it video falls back to MJPEG)"
            )),
            None => report.notes.push(
                "media ports are chosen at random: set a fixed range (ice_ports) if you want WebRTC through a router; otherwise the https fallback is used"
                    .into(),
            ),
        }
        if !e.turn.is_empty() {
            report.forwards.push(
                "TCP and UDP 3478 and the relay range of your coturn server, on the machine that runs it".into(),
            );
        } else {
            report.notes.push(
                "no TURN relay: a mobile network that blocks direct media falls back to the slower https video (no laptop sound)".into(),
            );
        }
        if e.tls_listen.is_some_and(|addr| addr.ip().is_unspecified()) {
            report.notes.push(
                "if a firewall (ufw, nftables) is active on this laptop, allow the forwarded ports there too".into(),
            );
        }
        if self_signed_choice {
            report.warnings.push(
                "the certificate is self-signed: compare its fingerprint on the first visit and never click through a changed warning"
                    .into(),
            );
        }
    }
    report
        .notes
        .push("local checks only: whether a phone on mobile data gets in is not tested".into());
    report
}

/// Applies a JSON object of `host.json` keys to the saved file: a key with `null` is cleared. The result must pass the
/// same validation as a load. Returns the new configuration; a damaged file is never overwritten. With `save` false
/// nothing is written (a dry run).
pub fn apply_patch(
    dir: &Path,
    patch: &serde_json::Value,
    save: bool,
) -> Result<HostConfig, String> {
    let loaded = HostConfig::load_checked(dir);
    if loaded.damaged {
        return Err(format!(
            "host.json is damaged and was not changed: {}",
            loaded.note.unwrap_or_default()
        ));
    }
    let serde_json::Value::Object(patch) = patch else {
        return Err("the change must be a JSON object".into());
    };
    let mut merged = serde_json::to_value(&loaded.config).map_err(|error| error.to_string())?;
    let object = merged
        .as_object_mut()
        .ok_or("host settings are not an object")?;
    for (key, value) in patch {
        object.insert(key.clone(), value.clone());
    }
    let config: HostConfig =
        serde_json::from_value(merged).map_err(|error| format!("not accepted: {error}"))?;
    let config = config.validated()?;
    if save {
        config
            .save(dir)
            .map_err(|error| format!("not saved: {error}"))?;
    }
    Ok(config)
}

pub fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| {
            i64::try_from(elapsed.as_secs()).unwrap_or(i64::MAX)
        })
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;

    use super::*;

    fn effective(dir: &Path) -> Effective {
        Effective {
            public: false,
            public_name: None,
            public_cert: None,
            listen: "127.0.0.1:8080".parse().unwrap(),
            tls_listen: Some("0.0.0.0:8443".parse().unwrap()),
            tls_cert: None,
            tls_key: None,
            cert_dir: dir.to_path_buf(),
            hostd_login: true,
            hostd_running: true,
            stun: vec![],
            turn: vec![],
            turn_secret_file: None,
            ice_ports: None,
            damaged: false,
        }
    }

    fn write_pair(dir: &Path, names: &[&str]) -> (PathBuf, PathBuf) {
        let certified = rcgen::generate_simple_self_signed(
            names
                .iter()
                .map(|name| (*name).to_string())
                .collect::<Vec<_>>(),
        )
        .unwrap();
        let (cert, key) = (dir.join("cert.pem"), dir.join("key.pem"));
        std::fs::write(&cert, certified.cert.pem()).unwrap();
        std::fs::write(&key, certified.signing_key.serialize_pem()).unwrap();
        std::fs::set_permissions(&key, std::fs::Permissions::from_mode(0o600)).unwrap();
        (cert, key)
    }

    #[test]
    fn the_home_default_has_no_problems_and_says_what_it_did_not_test() {
        let dir = tempfile::tempdir().unwrap();
        let report = preflight(&effective(dir.path()), certcheck_now());
        assert_eq!(report.access, "home");
        assert!(report.problems.is_empty());
        assert!(report.notes.iter().any(|note| note.contains("not tested")));
    }

    fn certcheck_now() -> i64 {
        now_unix()
    }

    #[test]
    fn public_mode_lists_every_gap_and_a_good_setup_names_the_forwards_and_the_address() {
        let dir = tempfile::tempdir().unwrap();
        let mut e = effective(dir.path());
        e.public = true;
        e.hostd_login = false;
        e.listen = "0.0.0.0:8080".parse().unwrap();
        let bad = preflight(&e, now_unix());
        for word in ["--hostd-dir", "--tls-cert", "--listen", "public_name"] {
            assert!(
                bad.problems.iter().any(|p| p.contains(word)),
                "{word}: {:?}",
                bad.problems
            );
        }
        let (cert, key) = write_pair(dir.path(), &["home.example.org"]);
        e.hostd_login = true;
        e.listen = "127.0.0.1:8080".parse().unwrap();
        e.public_name = Some("home.example.org".into());
        e.tls_cert = Some(cert);
        e.tls_key = Some(key);
        e.ice_ports = Some("50000-50100".into());
        let report = preflight(&e, now_unix());
        // The fixture is self-signed, which a real authority's certificate is not.
        assert!(
            report.problems.iter().any(|p| p.contains("self-signed")),
            "{:?}",
            report.problems
        );
        assert_eq!(
            report.url.as_deref(),
            Some("https://home.example.org:8443/")
        );
        assert!(report.forwards.iter().any(|f| f.starts_with("TCP 8443")));
        assert!(report.forwards.iter().any(|f| f.contains("50000-50100")));
        assert!(report.notes.iter().any(|n| n.contains("no TURN")));
    }

    #[test]
    fn a_bare_ip_may_use_the_consoles_own_self_signed_certificate_by_explicit_choice() {
        let dir = tempfile::tempdir().unwrap();
        let mut e = effective(dir.path());
        e.public = true;
        e.public_name = Some("203.0.113.7".into());
        e.public_cert = Some(PublicCert::SelfSigned);
        let before = preflight(&e, now_unix());
        assert!(before.problems.is_empty(), "{:?}", before.problems);
        assert!(
            before
                .notes
                .iter()
                .any(|n| n.contains("made when the console starts"))
        );
        assert!(before.warnings.iter().any(|w| w.contains("fingerprint")));
        write_pair(dir.path(), &["203.0.113.7", "localhost"]);
        let after = preflight(&e, now_unix());
        assert!(after.problems.is_empty(), "{:?}", after.problems);
        assert_eq!(
            after.certificate.unwrap().fingerprint.split(':').count(),
            32
        );
        e.public_name = Some("198.51.100.9".into());
        let renamed = preflight(&e, now_unix());
        assert!(renamed.problems.is_empty(), "{:?}", renamed.problems);
        assert!(
            renamed
                .notes
                .iter()
                .any(|n| n.contains("remade when the console starts"))
        );
    }

    #[test]
    fn a_damaged_file_and_a_bad_turn_secret_are_problems() {
        let dir = tempfile::tempdir().unwrap();
        let mut e = effective(dir.path());
        e.damaged = true;
        assert!(
            preflight(&e, now_unix())
                .problems
                .iter()
                .any(|p| p.contains("damaged"))
        );
        e.damaged = false;
        e.turn = vec!["turn:t.example.org:3478".into()];
        let secret = dir.path().join("secret");
        std::fs::write(&secret, "short").unwrap();
        std::fs::set_permissions(&secret, std::fs::Permissions::from_mode(0o600)).unwrap();
        e.turn_secret_file = Some(secret.clone());
        assert!(
            preflight(&e, now_unix())
                .problems
                .iter()
                .any(|p| p.contains("fewer than 16"))
        );
        std::fs::write(&secret, "a-long-enough-turn-secret").unwrap();
        assert!(preflight(&e, now_unix()).problems.is_empty());
        std::fs::set_permissions(&secret, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(
            preflight(&e, now_unix())
                .problems
                .iter()
                .any(|p| p.contains("chmod 600"))
        );
    }

    #[test]
    fn a_patch_is_validated_merged_saved_and_never_overwrites_a_damaged_file() {
        let dir = tempfile::tempdir().unwrap();
        let saved = apply_patch(
            dir.path(),
            &serde_json::json!({"public_name": "home.example.org", "ice_ports": "50000-50100", "approval": "ask"}),
            true,
        )
        .unwrap();
        assert_eq!(saved.public_name.as_deref(), Some("home.example.org"));
        let dry = apply_patch(
            dir.path(),
            &serde_json::json!({"ice_ports": "50200-50300"}),
            false,
        )
        .unwrap();
        assert_eq!(dry.ice_ports.as_deref(), Some("50200-50300"));
        assert_eq!(
            HostConfig::load(dir.path()).0.ice_ports.as_deref(),
            Some("50000-50100"),
            "a dry run saves nothing"
        );
        let again = apply_patch(dir.path(), &serde_json::json!({"ice_ports": null}), true).unwrap();
        assert_eq!(again.ice_ports, None);
        assert_eq!(
            again.public_name.as_deref(),
            Some("home.example.org"),
            "other keys stay"
        );
        for bad in [
            serde_json::json!({"public_name": "https://x/"}),
            serde_json::json!({"nonsense": 1}),
            serde_json::json!([1]),
        ] {
            assert!(apply_patch(dir.path(), &bad, true).is_err(), "{bad}");
        }
        assert_eq!(
            HostConfig::load(dir.path()).0.public_name.as_deref(),
            Some("home.example.org")
        );
        std::fs::write(dir.path().join(crate::host::HOST_FILE), "{ nope").unwrap();
        assert!(
            apply_patch(
                dir.path(),
                &serde_json::json!({"ice_ports": "50000-50100"}),
                true
            )
            .unwrap_err()
            .contains("damaged")
        );
        assert_eq!(
            std::fs::read_to_string(dir.path().join(crate::host::HOST_FILE)).unwrap(),
            "{ nope"
        );
    }
}
