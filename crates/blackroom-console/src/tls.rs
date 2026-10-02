//! A self-signed certificate that survives restarts, so a browser's "accept the risk" sticks.

use std::path::Path;

use anyhow::Context;

use crate::display::write_private;

/// PEM certificate and key for `names` (DNS names and IP addresses). Reuses the files in `dir`
/// while they cover every name; otherwise creates a new pair.
pub fn load_or_create(dir: &Path, names: &[String]) -> anyhow::Result<(Vec<u8>, Vec<u8>)> {
    let covered = std::fs::read_to_string(dir.join("sans.txt")).unwrap_or_default();
    let have: Vec<&str> = covered.lines().collect();
    if names.iter().all(|name| have.contains(&name.as_str()))
        && let (Ok(cert), Ok(key)) = (
            std::fs::read(dir.join("cert.pem")),
            std::fs::read(dir.join("key.pem")),
        )
    {
        return Ok((cert, key));
    }
    let certified = rcgen::generate_simple_self_signed(names.to_vec())
        .context("generate the self-signed certificate")?;
    let cert = certified.cert.pem().into_bytes();
    let key = certified.signing_key.serialize_pem().into_bytes();
    write_private(dir, "key.pem", &key)?;
    write_private(dir, "cert.pem", &cert)?;
    write_private(dir, "sans.txt", names.join("\n").as_bytes())?;
    Ok((cert, key))
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;

    use super::*;

    #[test]
    fn the_certificate_is_reused_until_a_name_is_missing_and_the_key_is_private() {
        let dir = std::env::temp_dir().join(format!("br-tls-test-{}", std::process::id()));
        let names = vec!["localhost".to_string(), "192.168.1.50".to_string()];
        let (cert, key) = load_or_create(&dir, &names).unwrap();
        assert!(String::from_utf8_lossy(&cert).contains("BEGIN CERTIFICATE"));
        assert!(String::from_utf8_lossy(&key).contains("PRIVATE KEY"));
        let mode = std::fs::metadata(dir.join("key.pem"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600);

        let (again, _) = load_or_create(&dir, &names[..1]).unwrap();
        assert_eq!(again, cert, "a subset of the names reuses the pair");

        let mut more = names.clone();
        more.push("10.0.0.7".into());
        let (renewed, _) = load_or_create(&dir, &more).unwrap();
        assert_ne!(renewed, cert, "a new address needs a new certificate");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
