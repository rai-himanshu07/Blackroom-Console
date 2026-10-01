//! PipeWire frame capture (Doc 05 §24; Doc 19 §16: no encoding — proof of
//! capture liveness only, frame count/dimensions/rate). Ported from the
//! proven pattern in Experiments 3–4 (`docs/experiments/evidence/exp0{3,4}/`,
//! the `pipewire_probe` module): a bounded `pw::main_loop` run that quits
//! once `frame_target` frames arrive or `timeout` elapses.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::Duration;

use pipewire as pw;
use pw::properties::properties;
use pw::spa;
use pw::spa::pod::Pod;

use blackroom_core::error::{BlackroomError, ErrorCode};

fn pipewire_unavailable(detail: impl std::fmt::Display) -> BlackroomError {
    BlackroomError::new(ErrorCode::PipewireUnavailable, detail.to_string())
}

/// Outcome of a bounded capture (Doc 19 §16: proof of liveness, not a
/// streaming/encoding pipeline).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CaptureOutcome {
    pub frames_received: u32,
}

/// Connects to PipeWire node `node_id` and blocks until `frame_target`
/// frames arrive or `timeout` elapses (Experiment 3/4's proven pattern).
/// `preferred_width`/`preferred_height` are proposed as the negotiated
/// format's default — the virtual monitor's actual resolution is driven by
/// this negotiated PipeWire video format, not by `RecordVirtual`'s
/// properties dict (Experiment 4 finding, `virtual_monitor.rs`).
pub fn capture_frames(
    node_id: u32,
    preferred_width: i32,
    preferred_height: i32,
    frame_target: u32,
    timeout: Duration,
) -> Result<CaptureOutcome, BlackroomError> {
    capture(
        node_id,
        preferred_width,
        preferred_height,
        frame_target,
        timeout,
        None,
        None,
    )
}

/// Like [`capture_frames`] but keeps one consumer attached until `stop` is set (checked every
/// 100 ms) or `max` elapses, publishing the running frame count in `frames` so a caller can read
/// it at phase boundaries (lock, unlock) without reconnecting.
pub fn capture_until_stopped(
    node_id: u32,
    preferred_width: i32,
    preferred_height: i32,
    stop: Arc<AtomicBool>,
    frames: Arc<AtomicU32>,
    max: Duration,
) -> Result<CaptureOutcome, BlackroomError> {
    capture(
        node_id,
        preferred_width,
        preferred_height,
        u32::MAX,
        max,
        Some(stop),
        Some(frames),
    )
}

fn capture(
    node_id: u32,
    preferred_width: i32,
    preferred_height: i32,
    frame_target: u32,
    timeout: Duration,
    stop: Option<Arc<AtomicBool>>,
    shared_frames: Option<Arc<AtomicU32>>,
) -> Result<CaptureOutcome, BlackroomError> {
    pw::init();
    let mainloop = pw::main_loop::MainLoopRc::new(None).map_err(pipewire_unavailable)?;
    let context = pw::context::ContextRc::new(&mainloop, None).map_err(pipewire_unavailable)?;
    let core = context.connect_rc(None).map_err(pipewire_unavailable)?;

    let frame_count = Rc::new(Cell::new(0_u32));
    struct UserData {
        mainloop: pw::main_loop::MainLoopRc,
        frame_count: Rc<Cell<u32>>,
        shared_frames: Option<Arc<AtomicU32>>,
        frame_target: u32,
    }
    let data = UserData {
        mainloop: mainloop.clone(),
        frame_count: frame_count.clone(),
        shared_frames,
        frame_target,
    };
    let stream = pw::stream::StreamBox::new(
        &core,
        "blackroom-virtual-monitor-capture",
        properties! {
            *pw::keys::MEDIA_TYPE => "Video",
            *pw::keys::MEDIA_CATEGORY => "Capture",
            *pw::keys::MEDIA_ROLE => "Screen",
        },
    )
    .map_err(pipewire_unavailable)?;
    let _listener = stream
        .add_local_listener_with_user_data(data)
        .process(|stream, user_data| {
            if stream.dequeue_buffer().is_some() {
                let count = user_data.frame_count.get() + 1;
                user_data.frame_count.set(count);
                if let Some(shared) = &user_data.shared_frames {
                    shared.store(count, Ordering::Relaxed);
                }
                if count >= user_data.frame_target {
                    user_data.mainloop.quit();
                }
            }
        })
        .register()
        .map_err(pipewire_unavailable)?;

    let obj = spa::pod::object!(
        spa::utils::SpaTypes::ObjectParamFormat,
        spa::param::ParamType::EnumFormat,
        spa::pod::property!(
            spa::param::format::FormatProperties::MediaType,
            Id,
            spa::param::format::MediaType::Video
        ),
        spa::pod::property!(
            spa::param::format::FormatProperties::MediaSubtype,
            Id,
            spa::param::format::MediaSubtype::Raw
        ),
        spa::pod::property!(
            spa::param::format::FormatProperties::VideoFormat,
            Choice,
            Enum,
            Id,
            spa::param::video::VideoFormat::RGBx,
            spa::param::video::VideoFormat::RGBx,
            spa::param::video::VideoFormat::BGRx,
        ),
        spa::pod::property!(
            spa::param::format::FormatProperties::VideoSize,
            Choice,
            Range,
            Rectangle,
            spa::utils::Rectangle {
                width: preferred_width as u32,
                height: preferred_height as u32
            },
            spa::utils::Rectangle {
                width: 1,
                height: 1
            },
            spa::utils::Rectangle {
                width: 7680,
                height: 4320
            }
        ),
        spa::pod::property!(
            spa::param::format::FormatProperties::VideoFramerate,
            Choice,
            Range,
            Fraction,
            spa::utils::Fraction { num: 60, denom: 1 },
            spa::utils::Fraction { num: 0, denom: 1 },
            spa::utils::Fraction {
                num: 1000,
                denom: 1
            }
        ),
    );
    let values: Vec<u8> = spa::pod::serialize::PodSerializer::serialize(
        std::io::Cursor::new(Vec::new()),
        &spa::pod::Value::Object(obj),
    )
    .map_err(pipewire_unavailable)?
    .0
    .into_inner();
    let mut params =
        [Pod::from_bytes(&values).ok_or_else(|| pipewire_unavailable("bad format pod"))?];
    stream
        .connect(
            spa::utils::Direction::Input,
            Some(node_id),
            pw::stream::StreamFlags::AUTOCONNECT | pw::stream::StreamFlags::MAP_BUFFERS,
            &mut params,
        )
        .map_err(pipewire_unavailable)?;

    let quit_on_timeout = mainloop.clone();
    let timer = mainloop.loop_().add_timer(move |_| quit_on_timeout.quit());
    timer
        .update_timer(Some(timeout), None)
        .into_result()
        .map_err(pipewire_unavailable)?;

    let _stop_timer = match stop {
        Some(flag) => {
            let quit_on_stop = mainloop.clone();
            let poll = mainloop.loop_().add_timer(move |_| {
                if flag.load(Ordering::Relaxed) {
                    quit_on_stop.quit();
                }
            });
            let every = Duration::from_millis(100);
            poll.update_timer(Some(every), Some(every))
                .into_result()
                .map_err(pipewire_unavailable)?;
            Some(poll)
        }
        None => None,
    };

    mainloop.run();
    stream.disconnect().map_err(pipewire_unavailable)?;
    Ok(CaptureOutcome {
        frames_received: frame_count.get(),
    })
}
