//! Live video for the remote console: PipeWire frames to JPEG, published as the latest frame
//! for an MJPEG stream (MVP milestone 1, `docs/plans/plan-20261002-mvp-fast-path.md`).

use std::borrow::Cow;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, Ordering};
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::time::{Duration, Instant};

use jpeg_encoder::{ColorType, Encoder};
use pipewire as pw;
use pw::properties::properties;
use pw::spa;
use pw::spa::pod::Pod;

use blackroom_core::error::{BlackroomError, ErrorCode};

use super::pipewire_capture::video_format_pod;

fn pipewire_unavailable(detail: impl std::fmt::Display) -> BlackroomError {
    BlackroomError::new(ErrorCode::PipewireUnavailable, detail.to_string())
}

/// The newest encoded frame. A slow reader skips frames instead of queueing them.
#[derive(Default)]
pub struct JpegSlot {
    state: Mutex<SlotState>,
    changed: Condvar,
}

#[derive(Default)]
struct SlotState {
    seq: u64,
    jpeg: Option<Arc<Vec<u8>>>,
    closed: bool,
}

impl JpegSlot {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, SlotState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub fn publish(&self, jpeg: Vec<u8>) {
        let mut state = self.lock();
        state.seq += 1;
        state.jpeg = Some(Arc::new(jpeg));
        self.changed.notify_all();
    }

    /// Ends the stream: readers get `None` once they have seen every published frame.
    pub fn close(&self) {
        self.lock().closed = true;
        self.changed.notify_all();
    }

    /// The newest frame if it is newer than `after`, else waits up to `timeout` for one. `None` on
    /// timeout or when the slot is closed. A `timeout` of zero never waits.
    pub fn next_after(&self, after: u64, timeout: Duration) -> Option<(u64, Arc<Vec<u8>>)> {
        let deadline = Instant::now() + timeout;
        let mut state = self.lock();
        loop {
            if state.seq > after
                && let Some(jpeg) = &state.jpeg
            {
                return Some((state.seq, Arc::clone(jpeg)));
            }
            let left = deadline.checked_duration_since(Instant::now())?;
            if state.closed || left.is_zero() {
                return None;
            }
            state = self
                .changed
                .wait_timeout(state, left)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
    }

    pub fn is_closed(&self) -> bool {
        self.lock().closed
    }
}

#[derive(Debug, Clone, Copy)]
pub struct VideoOptions {
    pub preferred_width: i32,
    pub preferred_height: i32,
    /// JPEG quality, 1 to 100.
    pub quality: u8,
    /// Frames above this rate are dropped before encoding; 0 means no limit.
    pub max_fps: u32,
}

impl Default for VideoOptions {
    fn default() -> Self {
        Self {
            preferred_width: 1920,
            preferred_height: 1080,
            quality: 70,
            max_fps: 30,
        }
    }
}

/// Rows without padding: borrows the buffer when it is already tight, else copies each row.
fn visible_pixels(
    bytes: &[u8],
    offset: usize,
    stride: usize,
    width: usize,
    height: usize,
) -> Option<Cow<'_, [u8]>> {
    let row = width.checked_mul(4)?;
    let stride = if stride == 0 { row } else { stride };
    let needed = stride
        .checked_mul(height.checked_sub(1)?)?
        .checked_add(row)?;
    let bytes = bytes.get(offset..)?;
    if bytes.len() < needed {
        return None;
    }
    if stride == row {
        return Some(Cow::Borrowed(&bytes[..row * height]));
    }
    let mut packed = Vec::with_capacity(row * height);
    for y in 0..height {
        packed.extend_from_slice(&bytes[y * stride..y * stride + row]);
    }
    Some(Cow::Owned(packed))
}

fn encode_jpeg(
    pixels: &[u8],
    width: usize,
    height: usize,
    color: ColorType,
    quality: u8,
) -> Option<Vec<u8>> {
    let (w, h) = (u16::try_from(width).ok()?, u16::try_from(height).ok()?);
    let mut jpeg = Vec::with_capacity(width * height / 8);
    Encoder::new(&mut jpeg, quality)
        .encode(pixels, w, h, color)
        .ok()?;
    Some(jpeg)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RawFormat {
    Bgrx,
    Rgbx,
}

impl RawFormat {
    /// The GStreamer `format` field.
    pub fn gst_name(self) -> &'static str {
        match self {
            Self::Bgrx => "BGRx",
            Self::Rgbx => "RGBx",
        }
    }
}

/// One desktop frame, rows without padding.
pub struct RawFrame {
    pub width: u32,
    pub height: u32,
    pub format: RawFormat,
    pub pixels: Vec<u8>,
}

/// The newest raw frame, for a second consumer (the WebRTC encoder) of the one PipeWire stream.
#[derive(Default)]
pub struct FrameTap {
    state: Mutex<(u64, Option<Arc<RawFrame>>)>,
    changed: Condvar,
}

impl FrameTap {
    fn lock(&self) -> std::sync::MutexGuard<'_, (u64, Option<Arc<RawFrame>>)> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn publish(&self, frame: RawFrame) {
        let mut state = self.lock();
        state.0 += 1;
        state.1 = Some(Arc::new(frame));
        self.changed.notify_all();
    }

    /// The newest frame if it is newer than `after`, else waits up to `timeout` for one.
    pub fn next_after(&self, after: u64, timeout: Duration) -> Option<(u64, Arc<RawFrame>)> {
        let deadline = Instant::now() + timeout;
        let mut state = self.lock();
        loop {
            if state.0 > after
                && let Some(frame) = &state.1
            {
                return Some((state.0, Arc::clone(frame)));
            }
            let left = deadline.checked_duration_since(Instant::now())?;
            state = self
                .changed
                .wait_timeout(state, left)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
    }

    pub fn latest(&self) -> Option<Arc<RawFrame>> {
        self.lock().1.clone()
    }
}

/// JPEG quality and frame-rate cap, adjustable while the stream runs, and the raw-frame tap.
pub struct VideoTuning {
    quality: AtomicU8,
    max_fps: AtomicU32,
    tap: FrameTap,
}

impl VideoTuning {
    pub fn new(options: &VideoOptions) -> Arc<Self> {
        let tuning = Self {
            quality: AtomicU8::new(1),
            max_fps: AtomicU32::new(0),
            tap: FrameTap::default(),
        };
        tuning.set(options.quality, options.max_fps);
        Arc::new(tuning)
    }

    pub fn set(&self, quality: u8, max_fps: u32) {
        self.quality.store(quality.clamp(1, 100), Ordering::Relaxed);
        self.max_fps.store(max_fps, Ordering::Relaxed);
    }

    pub fn tap(&self) -> &FrameTap {
        &self.tap
    }

    fn quality(&self) -> u8 {
        self.quality.load(Ordering::Relaxed)
    }

    fn min_interval(&self) -> Duration {
        match self.max_fps.load(Ordering::Relaxed) {
            0 => Duration::ZERO,
            fps => Duration::from_secs(1) / fps,
        }
    }
}

struct StreamState {
    format: spa::param::video::VideoInfoRaw,
    slot: Arc<JpegSlot>,
    tuning: Arc<VideoTuning>,
    last_encoded: Option<Instant>,
    odd_buffers: u32,
}

/// Consumes PipeWire node `node_id` until `stop` is set, publishing each frame (rate limited) as a
/// JPEG in `slot`. Closes the slot on return.
pub fn stream_jpeg(
    node_id: u32,
    options: VideoOptions,
    stop: &Arc<AtomicBool>,
    slot: &Arc<JpegSlot>,
) -> Result<(), BlackroomError> {
    stream_jpeg_tuned(node_id, options, &VideoTuning::new(&options), stop, slot)
}

/// [`stream_jpeg`] with quality and frame rate taken from `tuning` on every frame.
pub fn stream_jpeg_tuned(
    node_id: u32,
    options: VideoOptions,
    tuning: &Arc<VideoTuning>,
    stop: &Arc<AtomicBool>,
    slot: &Arc<JpegSlot>,
) -> Result<(), BlackroomError> {
    let result = run_stream(node_id, options, tuning, stop, slot);
    slot.close();
    result
}

fn run_stream(
    node_id: u32,
    options: VideoOptions,
    tuning: &Arc<VideoTuning>,
    stop: &Arc<AtomicBool>,
    slot: &Arc<JpegSlot>,
) -> Result<(), BlackroomError> {
    pw::init();
    let mainloop = pw::main_loop::MainLoopRc::new(None).map_err(pipewire_unavailable)?;
    let context = pw::context::ContextRc::new(&mainloop, None).map_err(pipewire_unavailable)?;
    let core = context.connect_rc(None).map_err(pipewire_unavailable)?;
    let stream = pw::stream::StreamBox::new(
        &core,
        "blackroom-console-video",
        properties! {
            *pw::keys::MEDIA_TYPE => "Video",
            *pw::keys::MEDIA_CATEGORY => "Capture",
            *pw::keys::MEDIA_ROLE => "Screen",
        },
    )
    .map_err(pipewire_unavailable)?;
    let state = StreamState {
        format: spa::param::video::VideoInfoRaw::default(),
        slot: Arc::clone(slot),
        tuning: Arc::clone(tuning),
        last_encoded: None,
        odd_buffers: 0,
    };
    let _listener = stream
        .add_local_listener_with_user_data(state)
        .param_changed(|_, state, id, param| {
            let Some(param) = param else { return };
            if id != spa::param::ParamType::Format.as_raw() {
                return;
            }
            let Ok((media_type, media_subtype)) = spa::param::format_utils::parse_format(param)
            else {
                return;
            };
            if media_type == spa::param::format::MediaType::Video
                && media_subtype == spa::param::format::MediaSubtype::Raw
            {
                let _ = state.format.parse(param);
            }
        })
        .process(|stream, state| {
            let Some(mut buffer) = stream.dequeue_buffer() else {
                return;
            };
            if state
                .last_encoded
                .is_some_and(|at| at.elapsed() < state.tuning.min_interval())
            {
                return;
            }
            let size = state.format.size();
            let (width, height) = (size.width as usize, size.height as usize);
            let (color, raw_format) = match state.format.format() {
                spa::param::video::VideoFormat::BGRx => (ColorType::Bgra, RawFormat::Bgrx),
                spa::param::video::VideoFormat::RGBx => (ColorType::Rgba, RawFormat::Rgbx),
                _ => return,
            };
            let Some(data) = buffer.datas_mut().first_mut() else {
                return;
            };
            let chunk = data.chunk();
            let (chunk_size, chunk_flags) = (chunk.size(), chunk.flags().bits());
            // Mutter flags buffers that carry no new picture (empty/corrupted); encoding one shows stale pixels.
            if chunk_size == 0 || chunk_flags != 0 {
                state.odd_buffers += 1;
                if state.odd_buffers <= 30 {
                    tracing::info!(chunk_size, chunk_flags, "unusual PipeWire buffer");
                }
                if chunk_flags != 0 {
                    return;
                }
            }
            let (offset, stride) = (chunk.offset() as usize, chunk.stride().max(0) as usize);
            let Some(bytes) = data.data() else { return };
            let Some(pixels) = visible_pixels(bytes, offset, stride, width, height) else {
                return;
            };
            state.tuning.tap.publish(RawFrame {
                width: size.width,
                height: size.height,
                format: raw_format,
                pixels: pixels.to_vec(),
            });
            if let Some(jpeg) = encode_jpeg(&pixels, width, height, color, state.tuning.quality()) {
                state.slot.publish(jpeg);
                state.last_encoded = Some(Instant::now());
            }
        })
        .register()
        .map_err(pipewire_unavailable)?;

    let values = video_format_pod(options.preferred_width, options.preferred_height)?;
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

    let quit = mainloop.clone();
    let stop = Arc::clone(stop);
    let poll = mainloop.loop_().add_timer(move |_| {
        if stop.load(Ordering::Relaxed) {
            quit.quit();
        }
    });
    let every = Duration::from_millis(100);
    poll.update_timer(Some(every), Some(every))
        .into_result()
        .map_err(pipewire_unavailable)?;

    mainloop.run();
    stream.disconnect().map_err(pipewire_unavailable)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slot_hands_out_only_newer_frames_and_never_waits_for_zero_timeout() {
        let slot = JpegSlot::new();
        assert!(slot.next_after(0, Duration::ZERO).is_none());
        slot.publish(vec![1]);
        slot.publish(vec![2]);
        let (seq, frame) = slot.next_after(0, Duration::ZERO).unwrap();
        assert_eq!(
            (seq, frame.as_slice()),
            (2, [2].as_slice()),
            "skips to the newest"
        );
        assert!(slot.next_after(seq, Duration::ZERO).is_none());
        slot.close();
        assert!(slot.is_closed());
        assert!(slot.next_after(seq, Duration::from_secs(5)).is_none());
    }

    #[test]
    fn slot_wakes_a_waiting_reader() {
        let slot = JpegSlot::new();
        let writer = Arc::clone(&slot);
        let handle = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(50));
            writer.publish(vec![7]);
        });
        let (seq, frame) = slot.next_after(0, Duration::from_secs(5)).unwrap();
        assert_eq!((seq, frame.as_slice()), (1, [7].as_slice()));
        handle.join().unwrap();
    }

    #[test]
    fn rows_are_unpadded_and_short_buffers_are_refused() {
        let tight: Vec<u8> = (0..16).collect();
        assert!(matches!(
            visible_pixels(&tight, 0, 8, 2, 2),
            Some(Cow::Borrowed(rows)) if rows == tight.as_slice()
        ));
        let padded: Vec<u8> = (0..24).collect();
        let rows = visible_pixels(&padded, 0, 12, 2, 2).unwrap();
        assert_eq!(
            rows.as_ref(),
            [0, 1, 2, 3, 4, 5, 6, 7, 12, 13, 14, 15, 16, 17, 18, 19]
        );
        assert!(visible_pixels(&tight, 0, 8, 2, 3).is_none());
        assert!(visible_pixels(&tight, 4, 8, 2, 2).is_none());
        assert!(visible_pixels(&tight, 0, 8, 0, 2).is_some_and(|rows| rows.is_empty()));
    }

    #[test]
    fn jpeg_output_is_a_valid_jfif_stream() {
        let (w, h) = (64, 48);
        let pixels: Vec<u8> = (0..w * h)
            .flat_map(|i| [(i % 256) as u8, (i / 7 % 256) as u8, 128, 255])
            .collect();
        let jpeg = encode_jpeg(&pixels, w, h, ColorType::Bgra, 70).unwrap();
        assert_eq!(&jpeg[..2], [0xFF, 0xD8], "SOI");
        assert_eq!(&jpeg[jpeg.len() - 2..], [0xFF, 0xD9], "EOI");
        assert!(encode_jpeg(&pixels, 70_000, 1, ColorType::Bgra, 70).is_none());
    }
}
