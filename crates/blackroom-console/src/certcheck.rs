//! Looks at a certificate and its key the way a stranger's browser would care: which names it covers, when it is
//! valid, whether the key belongs to it. It does not check the chain against a trust store: only the browser can.

use std::io::Read;
use std::net::IpAddr;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::Path;
use std::sync::Arc;

use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use serde::Serialize;
use sha2::{Digest, Sha256};

/// A certificate chain or key file larger than this is not one.
const MAX_PEM: u64 = 64 * 1024;
/// A certificate this close to its end gets a renewal reminder (a Tailscale certificate lives 90 days).
pub const RENEW_WARNING_DAYS: i64 = 30;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CertReport {
    /// DNS names and IP addresses the certificate covers (subject alternative names).
    pub names: Vec<String>,
    pub not_before: i64,
    pub not_after: i64,
    /// Issuer and subject are the same: nobody vouches for it.
    pub self_signed: bool,
    /// SHA-256 of the first certificate, `AB:CD:...`, for comparing on the first visit.
    pub fingerprint: String,
    pub chain_len: usize,
}

impl CertReport {
    pub fn days_left(&self, now_unix: i64) -> i64 {
        (self.not_after - now_unix).div_euclid(86_400)
    }

    /// Days left once renewal is due: in the last `RENEW_WARNING_DAYS`, or the last third of a shorter-lived certificate.
    pub fn renew_due(&self, now_unix: i64) -> Option<i64> {
        let life_days = (self.not_after - self.not_before) / 86_400;
        let threshold = (life_days / 3).clamp(1, RENEW_WARNING_DAYS);
        let left = self.days_left(now_unix);
        (now_unix <= self.not_after && left < threshold).then_some(left)
    }
}

/// Reads a file of at most `MAX_PEM` bytes. A secret (`strict`) must be a regular file of yours that nobody else can
/// read, and is opened without following a symbolic link; the checks run on the open descriptor.
pub fn read_bounded(path: &Path, strict: bool) -> Result<Vec<u8>, String> {
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    if strict {
        options.custom_flags(rustix::fs::OFlags::NOFOLLOW.bits() as i32);
    }
    let file = options
        .open(path)
        .map_err(|error| format!("{}: {error}", path.display()))?;
    let meta = file
        .metadata()
        .map_err(|error| format!("{}: {error}", path.display()))?;
    if !meta.is_file() {
        return Err(format!("{} is not a regular file", path.display()));
    }
    if meta.len() > MAX_PEM {
        return Err(format!(
            "{} is too large to be a certificate or key",
            path.display()
        ));
    }
    if strict {
        if meta.uid() != rustix::process::geteuid().as_raw() {
            return Err(format!("{} belongs to another user", path.display()));
        }
        if meta.mode() & 0o077 != 0 {
            return Err(format!(
                "{} can be read by others: run chmod 600 on it",
                path.display()
            ));
        }
    }
    let mut bytes = Vec::new();
    file.take(MAX_PEM + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("{}: {error}", path.display()))?;
    Ok(bytes)
}

fn colon_hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|byte| format!("{byte:02X}"))
        .collect::<Vec<_>>()
        .join(":")
}

/// The first certificate of a PEM chain.
pub fn inspect_pem(cert_pem: &[u8]) -> Result<CertReport, String> {
    use x509_parser::extensions::GeneralName;
    let mut chain_len = 0;
    let mut leaf = None;
    for pem in x509_parser::pem::Pem::iter_from_buffer(cert_pem) {
        let pem = pem.map_err(|error| format!("not a PEM certificate: {error}"))?;
        if pem.label != "CERTIFICATE" {
            continue;
        }
        chain_len += 1;
        if leaf.is_none() {
            leaf = Some(pem);
        }
        if chain_len > 10 {
            return Err("more than 10 certificates in one file".into());
        }
    }
    let pem = leaf.ok_or("no certificate in the file")?;
    let cert = pem
        .parse_x509()
        .map_err(|error| format!("the certificate cannot be read: {error}"))?;
    let mut names = Vec::new();
    if let Ok(Some(san)) = cert.subject_alternative_name() {
        for general in &san.value.general_names {
            match general {
                GeneralName::DNSName(name) => names.push((*name).to_string()),
                GeneralName::IPAddress(bytes) => {
                    let address = match bytes.len() {
                        4 => <[u8; 4]>::try_from(*bytes).ok().map(IpAddr::from),
                        16 => <[u8; 16]>::try_from(*bytes).ok().map(IpAddr::from),
                        _ => None,
                    };
                    if let Some(address) = address {
                        names.push(address.to_string());
                    }
                }
                _ => {}
            }
        }
    }
    Ok(CertReport {
        names,
        not_before: cert.validity().not_before.timestamp(),
        not_after: cert.validity().not_after.timestamp(),
        self_signed: cert.issuer() == cert.subject(),
        fingerprint: colon_hex(&Sha256::digest(&pem.contents)),
        chain_len,
    })
}

/// Fails when the key is not the one the certificate was made for.
pub fn key_matches(cert_pem: &[u8], key_pem: &[u8]) -> Result<(), String> {
    let certs = CertificateDer::pem_slice_iter(cert_pem)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("the certificate file cannot be read: {error}"))?;
    let key = PrivateKeyDer::from_pem_slice(key_pem)
        .map_err(|error| format!("the key file cannot be read: {error}"))?;
    rustls::ServerConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
        .with_safe_default_protocol_versions()
        .map_err(|error| error.to_string())?
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .map(|_| ())
        .map_err(|error| format!("the key does not match the certificate ({error})"))
}

pub fn inspect_files(cert: &Path, key: &Path) -> Result<CertReport, String> {
    let cert_pem = read_bounded(cert, false)?;
    let key_pem = read_bounded(key, true)?;
    let report = inspect_pem(&cert_pem)?;
    key_matches(&cert_pem, &key_pem)?;
    Ok(report)
}

/// Does a certificate name (possibly `*.example.org`) cover what the owner's clients type?
pub fn name_covered(names: &[String], wanted: &str) -> bool {
    if let Ok(address) = wanted.parse::<IpAddr>() {
        return names
            .iter()
            .any(|name| name.parse::<IpAddr>() == Ok(address));
    }
    let wanted = wanted.to_ascii_lowercase();
    names.iter().any(|name| {
        let name = name.to_ascii_lowercase();
        match name.strip_prefix("*.") {
            Some(rest) => wanted
                .split_once('.')
                .is_some_and(|(label, tail)| !label.is_empty() && tail == rest),
            None => name == wanted,
        }
    })
}

pub fn renew_text(days_left: i64) -> String {
    format!(
        "the certificate ends in {days_left} day(s): renew it (docs/ops/internet-access.md), for a Tailscale name run the `sudo tailscale cert` command again"
    )
}

/// What is wrong (`.0`, blocks public mode) and what deserves a warning (`.1`).
pub fn judge(
    report: &CertReport,
    wanted_name: Option<&str>,
    now_unix: i64,
    allow_self_signed: bool,
) -> (Vec<String>, Vec<String>) {
    let (mut problems, mut warnings) = (Vec::new(), Vec::new());
    if now_unix < report.not_before {
        problems.push("the certificate is not valid yet (check the laptop's clock)".to_string());
    }
    if now_unix > report.not_after {
        problems.push("the certificate has expired".to_string());
    } else if let Some(left) = report.renew_due(now_unix) {
        warnings.push(renew_text(left));
    }
    if let Some(name) = wanted_name
        && !name_covered(&report.names, name)
    {
        problems.push(format!(
            "the certificate does not cover {name} (it covers: {})",
            if report.names.is_empty() {
                "no names".to_string()
            } else {
                report.names.join(", ")
            }
        ));
    }
    if report.self_signed && !allow_self_signed {
        problems.push(
            "the certificate is self-signed: a stranger's browser cannot trust it (use one from a real authority)"
                .to_string(),
        );
    }
    (problems, warnings)
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::{PermissionsExt, symlink};

    use super::*;

    fn pair(names: &[&str]) -> (String, String) {
        let certified = rcgen::generate_simple_self_signed(
            names
                .iter()
                .map(|name| (*name).to_string())
                .collect::<Vec<_>>(),
        )
        .unwrap();
        (certified.cert.pem(), certified.signing_key.serialize_pem())
    }

    #[test]
    fn a_certificate_reports_its_names_dates_and_fingerprint() {
        let (cert, _) = pair(&["home.example.org", "203.0.113.7", "*.lab.example.org"]);
        let report = inspect_pem(cert.as_bytes()).unwrap();
        assert!(report.names.contains(&"home.example.org".to_string()));
        assert!(report.names.contains(&"203.0.113.7".to_string()));
        assert!(report.self_signed);
        assert_eq!(report.chain_len, 1);
        assert_eq!(report.fingerprint.split(':').count(), 32);
        assert!(report.not_after > report.not_before);
        assert!(inspect_pem(b"nothing here").is_err());
    }

    #[test]
    fn names_match_exactly_by_wildcard_label_and_by_ip() {
        let names: Vec<String> = [
            "home.example.org",
            "*.lab.example.org",
            "203.0.113.7",
            "2001:db8::1",
        ]
        .map(String::from)
        .to_vec();
        assert!(name_covered(&names, "HOME.example.org"));
        assert!(name_covered(&names, "pc.lab.example.org"));
        assert!(
            !name_covered(&names, "a.b.lab.example.org"),
            "a wildcard covers one label"
        );
        assert!(!name_covered(&names, "lab.example.org"));
        assert!(name_covered(&names, "203.0.113.7"));
        assert!(!name_covered(&names, "203.0.113.8"));
        assert!(name_covered(&names, "2001:db8::1"));
        assert!(!name_covered(&names, "evil.org"));
    }

    #[test]
    fn dates_names_and_self_signing_decide_the_verdict() {
        let (cert, _) = pair(&["home.example.org"]);
        let report = inspect_pem(cert.as_bytes()).unwrap();
        let now = report.not_before + 86_400;
        let (problems, _) = judge(&report, Some("home.example.org"), now, true);
        assert!(problems.is_empty(), "{problems:?}");
        let (problems, _) = judge(&report, Some("home.example.org"), now, false);
        assert!(problems.iter().any(|p| p.contains("self-signed")));
        let (problems, _) = judge(&report, Some("other.example.org"), now, true);
        assert!(problems.iter().any(|p| p.contains("does not cover")));
        let (problems, _) = judge(&report, None, report.not_before - 10, true);
        assert!(problems.iter().any(|p| p.contains("not valid yet")));
        let (problems, _) = judge(&report, None, report.not_after + 10, true);
        assert!(problems.iter().any(|p| p.contains("expired")));
        let (_, warnings) = judge(&report, None, report.not_after - 3 * 86_400, true);
        assert!(warnings.iter().any(|w| w.contains("renew")));
    }

    #[test]
    fn renewal_is_due_in_the_last_month_or_the_last_third_of_a_short_life() {
        let day = 86_400;
        let report = |life_days: i64| CertReport {
            names: Vec::new(),
            not_before: 0,
            not_after: life_days * day,
            self_signed: false,
            fingerprint: String::new(),
            chain_len: 1,
        };
        let tailscale = report(90);
        assert_eq!(tailscale.renew_due(59 * day), None, "31 days left");
        assert_eq!(tailscale.renew_due(61 * day), Some(29));
        assert_eq!(
            tailscale.renew_due(91 * day),
            None,
            "expired is a problem, not a reminder"
        );
        let six_days = report(6);
        assert_eq!(
            six_days.renew_due(3 * day),
            None,
            "a fresh short certificate is not nagged"
        );
        assert_eq!(six_days.renew_due(5 * day), Some(1));
    }

    #[test]
    fn the_key_must_belong_to_the_certificate() {
        let (cert_a, key_a) = pair(&["a.example.org"]);
        let (_, key_b) = pair(&["b.example.org"]);
        assert!(key_matches(cert_a.as_bytes(), key_a.as_bytes()).is_ok());
        assert!(key_matches(cert_a.as_bytes(), key_b.as_bytes()).is_err());
    }

    #[test]
    fn a_key_file_must_be_private_regular_and_no_symlink() {
        let dir = tempfile::tempdir().unwrap();
        let (cert, key) = pair(&["home.example.org"]);
        let cert_path = dir.path().join("cert.pem");
        let key_path = dir.path().join("key.pem");
        std::fs::write(&cert_path, &cert).unwrap();
        std::fs::write(&key_path, &key).unwrap();
        std::fs::set_permissions(&key_path, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(
            inspect_files(&cert_path, &key_path)
                .unwrap_err()
                .contains("chmod 600")
        );
        std::fs::set_permissions(&key_path, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert!(inspect_files(&cert_path, &key_path).is_ok());
        let link = dir.path().join("link.pem");
        symlink(&key_path, &link).unwrap();
        assert!(
            inspect_files(&cert_path, &link).is_err(),
            "a symlinked key is refused"
        );
        std::fs::write(&key_path, vec![b'x'; 70_000]).unwrap();
        std::fs::set_permissions(&key_path, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert!(
            inspect_files(&cert_path, &key_path)
                .unwrap_err()
                .contains("too large")
        );
    }
}
