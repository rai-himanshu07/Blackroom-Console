//! The `--public` interlock: internet-facing mode refuses every weaker setting instead of warning.

/// What the command line asked for.
#[derive(Clone, Copy, Debug)]
pub struct Facts {
    pub hostd_login: bool,
    pub tls_listener: bool,
    /// A certificate and key supplied with `--tls-cert`/`--tls-key` (not the self-signed one).
    pub certificate_files: bool,
    /// The plain-http `--listen` address is loopback only.
    pub http_loopback_only: bool,
}

/// Every reason `--public` cannot start; empty means it may.
pub fn public_problems(facts: Facts) -> Vec<&'static str> {
    let mut problems = Vec::new();
    if !facts.hostd_login {
        problems.push(
            "--public needs --hostd-dir: the URL token and the TOTP-only login are not enough for the internet",
        );
    }
    if !facts.tls_listener {
        problems.push(
            "--public needs --tls-listen: nothing may be served over plain http to the internet",
        );
    }
    if !facts.certificate_files {
        problems.push(
            "--public needs --tls-cert and --tls-key from a real certificate authority (the self-signed one cannot be trusted by a stranger's browser and invites click-through)",
        );
    }
    if !facts.http_loopback_only {
        problems.push(
            "--public needs --listen on 127.0.0.1 (the plain-http port must not face the network)",
        );
    }
    problems
}

#[cfg(test)]
mod tests {
    use super::*;

    fn good() -> Facts {
        Facts {
            hostd_login: true,
            tls_listener: true,
            certificate_files: true,
            http_loopback_only: true,
        }
    }

    #[test]
    fn a_complete_setup_has_no_problems_and_each_gap_is_named() {
        assert!(public_problems(good()).is_empty());
        for (broken, word) in [
            (
                Facts {
                    hostd_login: false,
                    ..good()
                },
                "--hostd-dir",
            ),
            (
                Facts {
                    tls_listener: false,
                    ..good()
                },
                "--tls-listen",
            ),
            (
                Facts {
                    certificate_files: false,
                    ..good()
                },
                "--tls-cert",
            ),
            (
                Facts {
                    http_loopback_only: false,
                    ..good()
                },
                "--listen",
            ),
        ] {
            let problems = public_problems(broken);
            assert_eq!(problems.len(), 1);
            assert!(problems[0].contains(word), "{}", problems[0]);
        }
    }
}
