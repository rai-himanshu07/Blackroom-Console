//! Experiment 13: reproduce, on a throwaway `--headless` GNOME Shell only, the Mutter
//! `monitors-changed` crash candidate from `docs/experiments/evidence/exp06/2026-10-01-3`:
//! isolate to the virtual monitor with a streaming PipeWire consumer, then apply the
//! physical-only config that omits it. Run through `docs/ops/headless-repro.sh`.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use blackroom_gnome::mutter::display_config::{
    disable_physical_outputs, restore_physical_outputs, restore_physical_outputs_keeping_virtual,
    snapshot, verify_restored,
};
use blackroom_gnome::mutter::pipewire_capture::capture_until_stopped;
use blackroom_gnome::mutter::screencast::ScreenCastSession;
use clap::Parser;
use zbus::blocking::Connection;

#[derive(Parser, Debug)]
#[command(about = "Experiment 13: virtual-monitor restore crash repro (headless Shell only)")]
struct Args {
    /// Keep no streaming consumer at restore (control: earlier clean exp06 runs).
    #[arg(long)]
    no_consumer: bool,
    /// Milliseconds to hold the isolated state before restoring.
    #[arg(long, default_value_t = 2000)]
    hold_ms: u64,
    /// Candidate fix: restore with the virtual monitor kept as an extra logical monitor.
    #[arg(long)]
    keep_virtual: bool,
    /// Disconnect the consumer before stopping the session (default: Stop while it streams).
    #[arg(long)]
    join_before_stop: bool,
}

fn connectors(backup: &blackroom_gnome::mutter::display_config::DisplayBackup) -> Vec<String> {
    backup.outputs.iter().map(|o| o.connector.clone()).collect()
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let conn = Connection::session()?;
    blackroom_experiments::require_headless_shell(&conn)?;

    let original = snapshot(&conn, "headless-repro")?;
    let before = connectors(&original);
    println!("original connectors: {before:?}");

    let mut session = ScreenCastSession::create(&conn)?;
    let stream = session.record_virtual(1920, 1080, 60.0)?;
    let node_id = stream.start_and_wait_for_pipewire_node(&session)?;
    println!("pipewire node {node_id}");

    // The connector only appears once a consumer streams; it keeps streaming unless --no-consumer.
    let stop = Arc::new(AtomicBool::new(false));
    let frames = Arc::new(AtomicU32::new(0));
    let mut consumer = Some({
        let (stop, frames) = (Arc::clone(&stop), Arc::clone(&frames));
        thread::spawn(move || {
            capture_until_stopped(node_id, 1920, 1080, stop, frames, Duration::from_secs(60))
        })
    });

    let deadline = Instant::now() + Duration::from_secs(15);
    let virtual_connector = loop {
        let current = snapshot(&conn, "headless-repro")?;
        if let Some(found) = current
            .outputs
            .iter()
            .find(|o| !before.contains(&o.connector))
        {
            println!(
                "virtual identity: vendor={} serial={}",
                found.vendor, found.serial
            );
            break found.connector.clone();
        }
        anyhow::ensure!(
            Instant::now() < deadline,
            "virtual connector never appeared"
        );
        thread::sleep(Duration::from_millis(150));
    };
    println!(
        "virtual connector {virtual_connector}, frames so far {}",
        frames.load(Ordering::Relaxed)
    );

    if args.no_consumer {
        stop.store(true, Ordering::Relaxed);
        let _ = consumer
            .take()
            .map(thread::JoinHandle::join)
            .transpose()
            .map_err(|_| anyhow::anyhow!("consumer panicked"))?;
        thread::sleep(Duration::from_millis(500));
        println!("consumer stopped before isolation (control)");
    }

    disable_physical_outputs(&conn, &virtual_connector)?;
    println!("isolated; holding {} ms", args.hold_ms);
    thread::sleep(Duration::from_millis(args.hold_ms));

    println!(
        "consumer frames at restore: {}",
        frames.load(Ordering::Relaxed)
    );
    let restored = if args.keep_virtual {
        println!("restoring (virtual monitor kept as an extra logical monitor)");
        restore_physical_outputs_keeping_virtual(&conn, &original, &virtual_connector)
    } else {
        println!("restoring (physical-only config, virtual monitor omitted)");
        restore_physical_outputs(&conn, &original)
    };
    println!("restore result: {restored:?}");

    if args.join_before_stop {
        stop.store(true, Ordering::Relaxed);
        if let Some(handle) = consumer.take() {
            println!("consumer result: {:?}", handle.join());
        }
    }
    let stopped = session.stop();
    stop.store(true, Ordering::Relaxed);
    if let Some(handle) = consumer.take() {
        println!("consumer result: {:?}", handle.join());
    }
    println!("session stop: {stopped:?}");
    let verified = verify_restored(&conn, &original);
    println!("verified: {verified:?}");
    Ok(())
}
