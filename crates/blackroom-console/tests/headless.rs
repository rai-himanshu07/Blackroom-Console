//! End-to-end session against a throwaway headless Shell on a private bus:
//! `BR_BIN=<this test binary> docs/ops/headless-repro.sh -- --ignored --nocapture --test-threads=1`
//! (the guard in `RemoteConsole` refuses any compositor that is not `--headless`).

use std::path::PathBuf;
use std::thread::sleep;
use std::time::Duration;

use blackroom_console::{ConsoleConfig, InputEvent, Phase, RemoteConsole};
use blackroom_gnome::mutter::video::VideoOptions;

fn config(heartbeat_timeout: Duration) -> ConsoleConfig {
    ConsoleConfig {
        grab_socket: None,
        state_dir: std::env::temp_dir().join(format!("br-console-headless-{}", std::process::id())),
        headless: true,
        video: VideoOptions::default(),
        heartbeat_timeout,
        restore_bin: PathBuf::new(),
    }
}

#[test]
#[ignore = "needs a headless Shell: docs/ops/headless-repro.sh"]
fn two_sessions_stream_take_input_and_restore() {
    let console = RemoteConsole::spawn(config(Duration::from_secs(60)));
    for round in 1..=2 {
        let status = console
            .start()
            .unwrap_or_else(|e| panic!("round {round} start: {e}"));
        assert_eq!(status.phase, Phase::Running);
        println!(
            "round {round}: running {}x{} notes {:?}",
            status.width, status.height, status.notes
        );

        let slot = console.video().expect("a video slot");
        let (_, frame) = slot
            .next_after(0, Duration::from_secs(10))
            .expect("a frame within 10 s");
        assert_eq!(&frame[..2], [0xFF, 0xD8], "a JPEG");
        println!("round {round}: first frame {} bytes", frame.len());

        let events = vec![
            InputEvent::Move { x: 0.5, y: 0.5 },
            InputEvent::Button {
                code: 0x110,
                down: true,
            },
            InputEvent::Move { x: 0.6, y: 0.6 },
            InputEvent::Button {
                code: 0x110,
                down: false,
            },
            InputEvent::Key {
                code: 42,
                down: true,
            },
            InputEvent::Key {
                code: 30,
                down: true,
            },
            InputEvent::Key {
                code: 30,
                down: false,
            },
            InputEvent::Scroll { dx: 0.0, dy: 15.0 },
        ];
        console.input(events).expect("input accepted");
        console
            .input(vec![InputEvent::Key {
                code: 29,
                down: true,
            }])
            .expect("a held key");
        sleep(Duration::from_millis(600));
        let status = console.status();
        println!(
            "round {round}: input accepted {} refused {}",
            status.input_accepted, status.input_refused
        );
        assert!(status.notes.is_empty(), "{:?}", status.notes);
        assert_eq!(status.input_refused, 0);
        assert_eq!(status.input_accepted, 9);

        let report = console.stop();
        println!("round {round}: {report:#?}");
        assert!(report.errors.is_empty(), "{:?}", report.errors);
        assert_eq!(report.virtual_gone, Some(true));
        assert_eq!(report.topology_restored, Some(true));
        assert_eq!(console.status().phase, Phase::Idle);
        assert!(console.video().is_none());
    }
}

#[test]
#[ignore = "needs a headless Shell: docs/ops/headless-repro.sh"]
fn a_silent_browser_runs_stop() {
    let console = RemoteConsole::spawn(config(Duration::from_secs(2)));
    console.start().expect("start");
    sleep(Duration::from_secs(5));
    let status = console.status();
    assert_eq!(status.phase, Phase::Idle);
    let report = status.last_stop.expect("an automatic stop report");
    assert_eq!(report.reason, "browser heartbeat lost");
    assert_eq!(report.topology_restored, Some(true));
}
