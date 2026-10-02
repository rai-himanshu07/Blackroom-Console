//! Experiment 14: MVP milestone 1 check, frames to JPEG, on a throwaway `--headless` Shell only
//! (run through `BR_BIN=target/debug/exp14_mjpeg_probe docs/ops/headless-repro.sh`).

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use blackroom_gnome::mutter::screencast::ScreenCastSession;
use blackroom_gnome::mutter::video::{JpegSlot, VideoOptions, stream_jpeg};
use clap::Parser;
use zbus::blocking::Connection;

#[derive(Parser, Debug)]
#[command(about = "Experiment 14: PipeWire frames to JPEG (headless Shell only)")]
struct Args {
    /// Seconds to stream.
    #[arg(long, default_value_t = 6)]
    seconds: u64,
    /// Where the newest JPEG is written when the run ends.
    #[arg(long, default_value = "/tmp/br-frame.jpg")]
    out: PathBuf,
    #[arg(long, default_value_t = 70)]
    quality: u8,
    #[arg(long, default_value_t = 30)]
    max_fps: u32,
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let conn = Connection::session()?;
    blackroom_experiments::require_headless_shell(&conn)?;

    let mut session = ScreenCastSession::create(&conn)?;
    let stream = session.record_virtual(1920, 1080, 60.0)?;
    let node_id = stream.start_and_wait_for_pipewire_node(&session)?;
    println!("pipewire node {node_id}");

    let slot = JpegSlot::new();
    let stop = Arc::new(AtomicBool::new(false));
    let options = VideoOptions {
        quality: args.quality,
        max_fps: args.max_fps,
        ..VideoOptions::default()
    };
    let consumer = {
        let (slot, stop) = (Arc::clone(&slot), Arc::clone(&stop));
        thread::spawn(move || stream_jpeg(node_id, options, &stop, &slot))
    };

    let started = Instant::now();
    let (mut seq, mut frames, mut bytes, mut newest) = (0, 0_u64, 0_usize, None);
    while started.elapsed() < Duration::from_secs(args.seconds) {
        if let Some((next, jpeg)) = slot.next_after(seq, Duration::from_millis(500)) {
            seq = next;
            frames += 1;
            bytes += jpeg.len();
            newest = Some(jpeg);
        }
    }
    stop.store(true, Ordering::Relaxed);
    let consumer_result = consumer.join();
    let stopped = session.stop();
    let secs = started.elapsed().as_secs_f64();
    println!(
        "frames seen {frames} in {secs:.1} s ({:.1} fps), mean {} bytes, consumer {consumer_result:?}, session stop {stopped:?}",
        frames as f64 / secs,
        bytes.checked_div(usize::try_from(frames)?).unwrap_or(0),
    );
    if let Some(jpeg) = newest {
        std::fs::write(&args.out, jpeg.as_slice())?;
        println!("wrote {}", args.out.display());
    }
    Ok(())
}
