//! The compatibility gate: the console starts only on a desktop combination it has been proven on, unless the
//! owner says `--allow-untested`. Evidence per cell is in `docs/ops/compatibility-matrix.md`.

use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde::Serialize;

/// GNOME Shell major versions with a PASS in the matrix.
pub const TESTED_GNOME: [u32; 1] = [50];
/// PipeWire major version with a PASS in the matrix.
pub const TESTED_PIPEWIRE_MAJOR: u32 = 1;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Facts {
    pub gnome_shell: Option<(u32, u32)>,
    pub pipewire: Option<(u32, u32)>,
    /// `XDG_SESSION_TYPE`; unset in some service environments, which is not held against the host.
    pub session_type: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "verdict", content = "reasons", rename_all = "snake_case")]
pub enum Verdict {
    Supported,
    /// A combination nobody has run: allowed only with `--allow-untested`.
    Untested(Vec<String>),
    /// Cannot work at all (no GNOME Shell, an X11 session, no PipeWire): never allowed.
    Unsupported(Vec<String>),
}

pub fn judge(facts: &Facts, headless: bool) -> Verdict {
    let mut unsupported = Vec::new();
    let mut untested = Vec::new();
    match facts.gnome_shell {
        None => unsupported.push(
            "GNOME Shell was not found (the console drives GNOME through Mutter)".to_string(),
        ),
        Some((major, minor)) if !TESTED_GNOME.contains(&major) => {
            untested.push(format!(
                "GNOME Shell {major}.{minor} (tested: {})",
                list(&TESTED_GNOME)
            ));
        }
        Some(_) => {}
    }
    match facts.pipewire {
        None => untested.push("the PipeWire version could not be read".to_string()),
        Some((major, _)) if major < 1 => {
            unsupported.push(format!("PipeWire {major}.x is too old (1.x needed)"))
        }
        Some((major, minor)) if major != TESTED_PIPEWIRE_MAJOR => {
            untested.push(format!(
                "PipeWire {major}.{minor} (tested: {TESTED_PIPEWIRE_MAJOR}.x)"
            ));
        }
        Some(_) => {}
    }
    if !headless
        && facts
            .session_type
            .as_deref()
            .is_some_and(|kind| kind != "wayland")
    {
        unsupported.push(format!(
            "the session type is {:?}: only a Wayland session is supported",
            facts.session_type.as_deref().unwrap_or_default()
        ));
    }
    if !unsupported.is_empty() {
        Verdict::Unsupported(unsupported)
    } else if !untested.is_empty() {
        Verdict::Untested(untested)
    } else {
        Verdict::Supported
    }
}

fn list(versions: &[u32]) -> String {
    versions
        .iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}

/// `X.Y` from the first dotted number in `text`.
fn parse_version(text: &str) -> Option<(u32, u32)> {
    let start = text.find(|c: char| c.is_ascii_digit())?;
    let mut parts = text[start..].split(|c: char| !c.is_ascii_digit());
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next().and_then(|part| part.parse().ok()).unwrap_or(0);
    Some((major, minor))
}

fn output_lines(program: &str, args: &[&str]) -> Vec<String> {
    let Ok(mut child) = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .stdout(Stdio::piped())
        .spawn()
    else {
        return Vec::new();
    };
    let deadline = Instant::now() + Duration::from_secs(3);
    while child.try_wait().ok().flatten().is_none() {
        if Instant::now() >= deadline {
            let _ = child.kill();
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let Some(stdout) = child.stdout.take() else {
        return Vec::new();
    };
    let lines = BufReader::new(stdout)
        .lines()
        .map_while(Result::ok)
        .take(5)
        .collect();
    let _ = child.wait();
    lines
}

pub fn detect() -> Facts {
    let gnome_shell = output_lines("gnome-shell", &["--version"])
        .first()
        .and_then(|line| parse_version(line));
    let pipewire = ["pipewire", "pw-cli"].iter().find_map(|program| {
        output_lines(program, &["--version"])
            .iter()
            .find(|line| line.contains("Linked with libpipewire"))
            .and_then(|line| parse_version(line.rsplit("libpipewire").next().unwrap_or("")))
    });
    Facts {
        gnome_shell,
        pipewire,
        session_type: std::env::var("XDG_SESSION_TYPE")
            .ok()
            .filter(|kind| !kind.is_empty()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts(
        gnome: Option<(u32, u32)>,
        pipewire: Option<(u32, u32)>,
        session: Option<&str>,
    ) -> Facts {
        Facts {
            gnome_shell: gnome,
            pipewire,
            session_type: session.map(str::to_string),
        }
    }

    #[test]
    fn the_proven_combination_is_supported() {
        assert_eq!(
            judge(&facts(Some((50, 1)), Some((1, 6)), Some("wayland")), false),
            Verdict::Supported
        );
        assert_eq!(
            judge(&facts(Some((50, 0)), Some((1, 2)), None), false),
            Verdict::Supported
        );
    }

    #[test]
    fn other_versions_are_untested_not_unsupported() {
        for gnome in [(49, 2), (51, 0), (46, 0)] {
            assert!(
                matches!(
                    judge(&facts(Some(gnome), Some((1, 6)), Some("wayland")), false),
                    Verdict::Untested(_)
                ),
                "{gnome:?}"
            );
        }
        assert!(matches!(
            judge(&facts(Some((50, 1)), Some((2, 0)), None), false),
            Verdict::Untested(_)
        ));
        assert!(matches!(
            judge(&facts(Some((50, 1)), None, None), false),
            Verdict::Untested(_)
        ));
    }

    #[test]
    fn impossible_setups_are_unsupported_whatever_the_flag() {
        assert!(matches!(
            judge(&facts(None, Some((1, 6)), None), false),
            Verdict::Unsupported(_)
        ));
        assert!(matches!(
            judge(&facts(Some((50, 1)), Some((1, 6)), Some("x11")), false),
            Verdict::Unsupported(_)
        ));
        assert!(matches!(
            judge(&facts(Some((50, 1)), Some((0, 3)), None), false),
            Verdict::Unsupported(_)
        ));
        // The throwaway headless Shell has no session type of its own.
        assert_eq!(
            judge(&facts(Some((50, 1)), Some((1, 6)), Some("x11")), true),
            Verdict::Supported
        );
    }

    #[test]
    fn version_text_is_parsed_loosely() {
        assert_eq!(parse_version("GNOME Shell 50.1"), Some((50, 1)));
        assert_eq!(parse_version("GNOME Shell 50"), Some((50, 0)));
        assert_eq!(parse_version(" 1.6.2"), Some((1, 6)));
        assert_eq!(parse_version("no digits"), None);
    }
}
