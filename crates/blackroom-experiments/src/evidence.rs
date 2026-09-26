//! Evidence-directory layout and the Document 10 §47 experiment result format.

use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;
use time::OffsetDateTime;
use time::format_description::well_known::Iso8601;

/// Document 10 §47 result classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ExperimentResult {
    Pass,
    Fail,
    Partial,
    Blocked,
    NotTested,
}

impl std::fmt::Display for ExperimentResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let text = match self {
            Self::Pass => "PASS",
            Self::Fail => "FAIL",
            Self::Partial => "PARTIAL",
            Self::Blocked => "BLOCKED",
            Self::NotTested => "NOT_TESTED",
        };
        f.write_str(text)
    }
}

/// One report in the Document 10 §47 format:
/// `Experiment/Date/Environment/Objective/Hypothesis/Procedure/Expected/Observed/
/// Evidence/Result/Failure/Root Cause/Security Impact/Recommended Action/Follow-up`.
#[derive(Debug, Clone)]
pub struct ExperimentReport {
    pub experiment: String,
    pub environment: String,
    pub objective: String,
    pub hypothesis: String,
    pub procedure: String,
    pub expected: String,
    pub observed: String,
    pub evidence: Vec<String>,
    pub result: ExperimentResult,
    pub failure: Option<String>,
    pub root_cause: Option<String>,
    pub security_impact: Option<String>,
    pub recommended_action: Option<String>,
    pub follow_up: Option<String>,
}

impl ExperimentReport {
    /// Render as `report.md`. `generated_at` is stamped in ISO-8601 UTC.
    pub fn render(&self, generated_at: OffsetDateTime) -> String {
        let evidence = if self.evidence.is_empty() {
            "(none)".to_string()
        } else {
            self.evidence
                .iter()
                .map(|line| format!("- {line}"))
                .collect::<Vec<_>>()
                .join("\n")
        };
        let opt = |value: &Option<String>| value.clone().unwrap_or_else(|| "(none)".to_string());
        format!(
            "Experiment: {experiment}\n\
             Date: {date}\n\
             Environment: {environment}\n\
             Objective:\n{objective}\n\n\
             Hypothesis:\n{hypothesis}\n\n\
             Procedure:\n{procedure}\n\n\
             Expected:\n{expected}\n\n\
             Observed:\n{observed}\n\n\
             Evidence:\n{evidence}\n\n\
             Result:\n{result}\n\n\
             Failure:\n{failure}\n\n\
             Root Cause:\n{root_cause}\n\n\
             Security Impact:\n{security_impact}\n\n\
             Recommended Action:\n{recommended_action}\n\n\
             Follow-up:\n{follow_up}\n",
            experiment = self.experiment,
            date = format_iso8601(generated_at),
            environment = self.environment,
            objective = self.objective,
            hypothesis = self.hypothesis,
            procedure = self.procedure,
            expected = self.expected,
            observed = self.observed,
            evidence = evidence,
            result = self.result,
            failure = opt(&self.failure),
            root_cause = opt(&self.root_cause),
            security_impact = opt(&self.security_impact),
            recommended_action = opt(&self.recommended_action),
            follow_up = opt(&self.follow_up),
        )
    }
}

/// Format a timestamp as ISO-8601 UTC (e.g. `2026-09-05T00:00:00.000000000Z`).
pub fn format_iso8601(at: OffsetDateTime) -> String {
    at.to_offset(time::UtcOffset::UTC)
        .format(&Iso8601::DEFAULT)
        .expect("OffsetDateTime always formats as Iso8601")
}

/// Replace the current user's username and hostname with `[USER]`/`[HOST]`
/// unless `enabled` is `false` (Doc 10 §46 "avoid collecting secrets";
/// Doc 13 §31 minimal collection). Case-sensitive, longest-match-first.
pub fn redact(input: &str, enabled: bool) -> String {
    if !enabled {
        return input.to_string();
    }
    let mut output = input.to_string();
    let user = std::env::var("USER").ok().filter(|value| !value.is_empty());
    let hostname = read_hostname().ok().filter(|value| !value.is_empty());
    let mut needles: Vec<(String, &str)> = Vec::new();
    if let Some(user) = &user {
        needles.push((user.clone(), "[USER]"));
    }
    if let Some(hostname) = &hostname {
        needles.push((hostname.clone(), "[HOST]"));
    }
    needles.sort_by_key(|(needle, _)| std::cmp::Reverse(needle.len()));
    for (needle, replacement) in needles {
        output = output.replace(needle.as_str(), replacement);
    }
    output
}

/// Read the kernel hostname without adding a dependency purely for this.
fn read_hostname() -> anyhow::Result<String> {
    let raw = fs::read_to_string("/proc/sys/kernel/hostname")?;
    Ok(raw.trim().to_string())
}

/// Reserve a new directory under `docs/experiments/evidence/<exp_id>/` for each run.
pub fn evidence_dir(exp_id: &str, at: OffsetDateTime) -> anyhow::Result<PathBuf> {
    let date = at
        .to_offset(time::UtcOffset::UTC)
        .format(time::macros::format_description!("[year]-[month]-[day]"))
        .expect("date-only format always succeeds");
    let base = Path::new("docs/experiments/evidence")
        .join(exp_id)
        .join(date);
    reserve_run_dir(&base)
}

fn reserve_run_dir(base: &Path) -> anyhow::Result<PathBuf> {
    let parent = base.parent().expect("evidence directory has a parent");
    let name = base.file_name().expect("evidence directory has a date");
    fs::create_dir_all(parent)?;
    for run in 1.. {
        let dir = if run == 1 {
            base.to_path_buf()
        } else {
            parent.join(format!("{}-{run}", name.to_string_lossy()))
        };
        match fs::create_dir(&dir) {
            Ok(()) => return Ok(dir),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.into()),
        }
    }
    unreachable!("run number cannot be exhausted")
}

/// Write `report.md` plus one JSON sidecar into `dir`. Never touches any path
/// outside `dir`.
pub fn write_evidence<T: Serialize>(
    dir: &Path,
    report_md: &str,
    json_name: &str,
    json_value: &T,
) -> anyhow::Result<()> {
    fs::write(dir.join("report.md"), report_md)?;
    let json = serde_json::to_string_pretty(json_value)?;
    fs::write(dir.join(json_name), json)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeated_run_preserves_existing_evidence() -> anyhow::Result<()> {
        let root = std::env::temp_dir().join(format!(
            "blackroom-evidence-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_nanos()
        ));
        fs::create_dir(&root)?;
        let base = root.join("2026-09-26");
        let first = reserve_run_dir(&base)?;
        write_evidence(&first, "first run", "findings.json", &1)?;
        let second = reserve_run_dir(&base)?;
        write_evidence(&second, "second run", "findings.json", &2)?;

        assert_eq!(first, base);
        assert_eq!(second, root.join("2026-09-26-2"));
        assert_eq!(fs::read_to_string(first.join("report.md"))?, "first run");
        assert_eq!(fs::read_to_string(first.join("findings.json"))?, "1");
        assert_eq!(fs::read_to_string(second.join("report.md"))?, "second run");
        fs::remove_dir_all(root)?;
        Ok(())
    }
}
