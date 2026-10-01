//! Experiment 9, frozen daemon (Phase 7, Gate FEAS-E): does a frozen `remote-emergencyd` that holds
//! the physical-input grab get killed, and so release it, by its supervisor?
//!
//! MUTATING and supervised. It is the daemon's client: it takes the grab (the operator's keyboard,
//! mouse and touchpad go dead), keeps the lease alive for `--hold-secs`, then SIGSTOPs the daemon
//! (the process on the other end of the socket, verified by uid and name) and measures how long
//! the connection takes to close. The daemon must run under `systemd-run --user -p Type=notify
//! -p NotifyAccess=main -p WatchdogSec=10 -p WatchdogSignal=SIGKILL`, so the watchdog kills it;
//! without a supervisor nothing releases the grab, this probe SIGKILLs the daemon itself at
//! `--limit-secs` and the result is FAIL. A 1 s-accuracy external kill timer must also be armed.

use std::io::Read;
use std::path::PathBuf;
use std::thread::sleep;
use std::time::{Duration, Instant};

use anyhow::{Context, bail};
use blackroom_experiments::{
    CommonArgs, ExperimentReport, ExperimentResult, evidence_dir, write_evidence,
};
use clap::Parser;
use remote_emergency_client::client::{Client, Outcome};
use rustix::process::{Pid, Signal, kill_process, test_kill_process};
use serde::Serialize;
use time::OffsetDateTime;

const EXP_ID: &str = "exp09";
/// `comm` of `remote-emergencyd` (the kernel truncates names to 15 characters).
const DAEMON_COMM: &str = "remote-emergenc";
const ACCEPT_MS: u128 = 30_000;

#[derive(Parser, Debug)]
#[command(
    about = "Experiment 9: does a supervisor release the grab of a frozen daemon? (MUTATING)"
)]
struct Args {
    #[command(flatten)]
    common: CommonArgs,
    /// Required: the operator is present, work is saved, second-device SSH is open and a kill
    /// timer with 1 s accuracy is armed (docs/ops/experiment-safety.md section 7).
    #[arg(long, default_value_t = false)]
    operator_present: bool,
    /// Control socket of a running `remote-emergencyd --enable-grabs` started under systemd.
    #[arg(long)]
    socket: PathBuf,
    /// Seconds to hold the grab, renewing the lease, before the daemon is frozen.
    #[arg(long, default_value_t = 8, value_parser = clap::value_parser!(u64).range(3..=30))]
    hold_secs: u64,
    /// Give up on the supervisor after this long and SIGKILL the daemon.
    #[arg(long, default_value_t = 60, value_parser = clap::value_parser!(u64).range(20..=120))]
    limit_secs: u64,
}

#[derive(Debug, Default, Serialize)]
struct Findings {
    isolated_nodes: Option<usize>,
    daemon_pid: Option<i32>,
    daemon_comm: Option<String>,
    closed_after_ms: Option<u128>,
    ended_by: Option<&'static str>,
    daemon_gone_after_close: Option<bool>,
    notes: Vec<String>,
}

/// Kills the daemon if it is still alive when the probe ends, so a frozen holder never outlives it.
struct Frozen(Pid);

impl Drop for Frozen {
    fn drop(&mut self) {
        if test_kill_process(self.0).is_ok() {
            let _ = kill_process(self.0, Signal::KILL);
        }
    }
}

fn is_done(findings: &Findings) -> bool {
    findings.ended_by == Some("daemon_closed")
        && findings.closed_after_ms.is_some_and(|ms| ms <= ACCEPT_MS)
}

fn run(args: &Args, findings: &mut Findings) -> anyhow::Result<()> {
    let mut client = Client::connect(&args.socket, Duration::from_secs(30)).context("connect")?;
    match client.isolate(30_000).context("isolate")? {
        Outcome::Isolated { nodes } => findings.isolated_nodes = Some(nodes),
        Outcome::Refused(reason) | Outcome::Error(reason) => {
            bail!("the daemon did not grab: {reason}")
        }
    }
    client.set_reply_timeout(Duration::from_secs(5))?;

    let peer =
        rustix::net::sockopt::socket_peercred(client.socket()).context("peer credentials")?;
    let pid = peer.pid;
    let comm = std::fs::read_to_string(format!("/proc/{}/comm", pid.as_raw_nonzero()))
        .unwrap_or_default()
        .trim()
        .to_string();
    findings.daemon_pid = Some(pid.as_raw_nonzero().get());
    findings.daemon_comm = Some(comm.clone());
    if peer.uid.as_raw() != rustix::process::getuid().as_raw() || comm != DAEMON_COMM {
        bail!(
            "the socket peer is not this user's remote-emergencyd (comm {comm:?}); not freezing it"
        );
    }

    println!(
        "grab ON ({} nodes). Touch the touchpad: it must be frozen. Freezing the daemon in {} s.",
        findings.isolated_nodes.unwrap_or(0),
        args.hold_secs
    );
    let hold = Instant::now();
    while hold.elapsed() < Duration::from_secs(args.hold_secs) {
        client.renew().context("renew")?;
        sleep(Duration::from_secs(2));
    }

    let guard = Frozen(pid);
    kill_process(pid, Signal::STOP).context("SIGSTOP")?;
    let frozen = Instant::now();
    println!("daemon FROZEN. Keep touching the touchpad and note when the pointer moves.");

    let socket = client.socket();
    socket.set_read_timeout(Some(Duration::from_millis(500)))?;
    let mut byte = [0_u8; 1];
    loop {
        match (&*socket).read(&mut byte) {
            Ok(0) => {
                findings.ended_by = Some("daemon_closed");
                break;
            }
            Ok(_) => {}
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) => {}
            Err(error) => {
                findings.ended_by = Some("daemon_closed");
                findings.notes.push(format!("socket error: {error}"));
                break;
            }
        }
        if frozen.elapsed() >= Duration::from_secs(args.limit_secs) {
            findings.ended_by = Some("limit_kill");
            findings
                .notes
                .push("no supervisor killed the frozen daemon; the probe killed it".to_string());
            break;
        }
    }
    findings.closed_after_ms = Some(frozen.elapsed().as_millis());
    sleep(Duration::from_secs(2));
    findings.daemon_gone_after_close = Some(test_kill_process(pid).is_err());
    drop(guard);
    println!(
        "ended by {:?} after {} ms",
        findings.ended_by,
        findings.closed_after_ms.unwrap_or(0)
    );
    Ok(())
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::try_init().ok();
    let args = Args::parse();
    anyhow::ensure!(
        args.operator_present,
        "refusing to grab input: pass --operator-present only after the safety preflight in \
         docs/ops/experiment-safety.md section 7 (work saved, SSH open, kill timer armed)"
    );
    let now = OffsetDateTime::now_utc();
    let mut findings = Findings::default();
    let outcome = run(&args, &mut findings);
    if let Err(error) = &outcome {
        findings.notes.push(format!("{error:#}"));
    }
    let result = if outcome.is_err() {
        ExperimentResult::Blocked
    } else if is_done(&findings) {
        ExperimentResult::Pass
    } else {
        ExperimentResult::Fail
    };
    let observed = format!("result={result}; {findings:?}");
    println!("{observed}");
    let report = ExperimentReport {
        experiment: "Experiment 9 (frozen daemon) \u{2014} supervisor release (FEAS-E)".to_string(),
        environment: "Development workstation (host = target), Ubuntu 26.04 / GNOME 50.1, built-in keyboard, mouse and touchpad".to_string(),
        objective: "Show that a frozen remote-emergencyd holding the physical-input grab is killed by its supervisor, which releases the grab, within 30 s.".to_string(),
        hypothesis: "systemd WatchdogSec=10 with WatchdogSignal=SIGKILL kills a SIGSTOPped daemon and the kernel drops every grab.".to_string(),
        procedure: format!(
            "Operator starts remote-emergencyd under systemd-run --user with a watchdog and arms a 1 s-accuracy kill timer; \
             this probe isolates (lease 30 s), renews for {} s, verifies the peer is the daemon, SIGSTOPs it and waits up to {} s \
             for the connection to close.",
            args.hold_secs, args.limit_secs
        ),
        expected: "Connection closed by the supervisor kill within 30 s; probe did not have to kill the daemon.".to_string(),
        observed,
        evidence: vec!["findings.json (this directory)".to_string()],
        result,
        failure: outcome.err().map(|error| format!("{error:#}")),
        root_cause: None,
        security_impact: Some(
            "The physical keyboard, mouse and touchpad are grabbed for about ten seconds or more; the lock-out recovery is the external kill timer and SSH.".to_string(),
        ),
        recommended_action: None,
        follow_up: Some("FEAS-E decision is a separate review.".to_string()),
    };
    let dir = evidence_dir(EXP_ID, now)?;
    write_evidence(&dir, &report.render(now), "findings.json", &findings)?;
    println!("Wrote evidence to {}", dir.display());
    std::process::exit(match result {
        ExperimentResult::Pass => 0,
        ExperimentResult::Fail => 1,
        _ => 2,
    });
}
