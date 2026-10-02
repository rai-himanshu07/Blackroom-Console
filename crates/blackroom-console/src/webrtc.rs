//! H.264 over WebRTC: PipeWire -> hardware (NVENC) or software (OpenH264) encoder -> RTP -> `webrtcbin`.
//! One browser at a time; signalling is a single non-trickle offer/answer exchange over HTTP.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use blackroom_gnome::mutter::video::{RawFrame, VideoTuning};
use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_sdp as gst_sdp;
use gstreamer_webrtc as gst_webrtc;

const NEGOTIATE_TIMEOUT: Duration = Duration::from_secs(5);
const GATHER_TIMEOUT: Duration = Duration::from_secs(4);
const PROBE_TIMEOUT: gst::ClockTime = gst::ClockTime::from_seconds(5);
const MAX_OFFER_BYTES: usize = 64 * 1024;
const FIRST_FRAME_TIMEOUT: Duration = Duration::from_secs(3);
/// A still desktop sends no frames: the newest one is pushed again this often.
const KEEPALIVE: Duration = Duration::from_millis(250);
/// Constrained baseline level 4.0: what both encoders write for 1920x1080.
const DEFAULT_PROFILE_LEVEL_ID: &str = "42c028";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Encoder {
    Nvenc,
    OpenH264,
}

impl Encoder {
    pub fn label(self) -> &'static str {
        match self {
            Self::Nvenc => "nvh264enc",
            Self::OpenH264 => "openh264enc",
        }
    }

    /// `nvh264enc` counts kbit/s, `openh264enc` bit/s.
    fn bitrate_value(self, kbps: u32) -> String {
        match self {
            Self::Nvenc => kbps.to_string(),
            Self::OpenH264 => (u64::from(kbps) * 1000).to_string(),
        }
    }

    fn pipeline_fragment(self, kbps: u32) -> String {
        match self {
            Self::Nvenc => format!(
                "video/x-raw,format=NV12 ! nvh264enc name=enc zerolatency=true bframes=0 gop-size=120 \
                 rc-mode=cbr bitrate={} preset=p4 tune=ultra-low-latency",
                self.bitrate_value(kbps)
            ),
            Self::OpenH264 => format!(
                "video/x-raw,format=I420 ! openh264enc name=enc usage-type=screen rate-control=bitrate \
                 complexity=low gop-size=120 bitrate={}",
                self.bitrate_value(kbps)
            ),
        }
    }
}

fn probe(description: &str) -> bool {
    let Ok(element) = gst::parse::launch(description) else {
        return false;
    };
    let Ok(pipeline) = element.downcast::<gst::Pipeline>() else {
        return false;
    };
    let Some(bus) = pipeline.bus() else {
        return false;
    };
    let worked = pipeline.set_state(gst::State::Playing).is_ok()
        && bus
            .timed_pop_filtered(
                PROBE_TIMEOUT,
                &[gst::MessageType::Eos, gst::MessageType::Error],
            )
            .is_some_and(|message| matches!(message.view(), gst::MessageView::Eos(_)));
    let _ = pipeline.set_state(gst::State::Null);
    worked
}

/// NVENC when a real frame encodes on this machine, else OpenH264; probed once.
pub fn pick_encoder() -> Result<Encoder, String> {
    static CHOICE: OnceLock<Option<Encoder>> = OnceLock::new();
    gst::init().map_err(|e| format!("GStreamer init: {e}"))?;
    let choice = CHOICE.get_or_init(|| {
        if probe("videotestsrc num-buffers=3 ! video/x-raw,format=NV12,width=640,height=360 ! nvh264enc ! fakesink") {
            Some(Encoder::Nvenc)
        } else if probe("videotestsrc num-buffers=3 ! video/x-raw,format=I420,width=640,height=360 ! openh264enc ! fakesink") {
            Some(Encoder::OpenH264)
        } else {
            None
        }
    });
    choice
        .ok_or_else(|| "no H.264 encoder works (nvh264enc and openh264enc both failed)".to_string())
}

/// The H.264 entry of a browser offer the stream must match exactly for `webrtcbin` to accept it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct H264Offer {
    pub payload: u8,
    pub profile_level_id: String,
}

/// Packetization mode 1 with a baseline profile: what the encoder produces. Prefers constrained baseline.
pub fn pick_h264(offer_sdp: &str) -> Option<H264Offer> {
    let mut h264 = Vec::new();
    for line in offer_sdp.lines() {
        if let Some(rest) = line.strip_prefix("a=rtpmap:")
            && let Some((pt, codec)) = rest.split_once(' ')
            && codec.to_ascii_uppercase().starts_with("H264/90000")
            && let Ok(pt) = pt.parse::<u8>()
        {
            h264.push(pt);
        }
    }
    let mut candidates = Vec::new();
    for line in offer_sdp.lines() {
        let Some(rest) = line.strip_prefix("a=fmtp:") else {
            continue;
        };
        let Some((pt, params)) = rest.split_once(' ') else {
            continue;
        };
        let Ok(pt) = pt.parse::<u8>() else { continue };
        if !h264.contains(&pt) {
            continue;
        }
        let param = |key: &str| {
            params
                .split(';')
                .find_map(|kv| kv.trim().strip_prefix(key)?.strip_prefix('='))
        };
        if param("packetization-mode") == Some("1")
            && let Some(id) = param("profile-level-id")
            && id.len() == 6
            && id.starts_with("42")
        {
            candidates.push(H264Offer {
                payload: pt,
                profile_level_id: id.to_ascii_lowercase(),
            });
        }
    }
    candidates
        .iter()
        .find(|c| c.profile_level_id.starts_with("42e0"))
        .or(candidates.first())
        .cloned()
}

struct Pixels(Arc<RawFrame>);

impl AsRef<[u8]> for Pixels {
    fn as_ref(&self) -> &[u8] {
        &self.0.pixels
    }
}

/// Pushes the shared desktop frames into the pipeline until `stop`, repeating the newest on silence.
fn feed(
    source: &gst::Element,
    tuning: &VideoTuning,
    stop: &AtomicBool,
    shape: (u32, u32, blackroom_gnome::mutter::video::RawFormat),
) {
    let mut seen = 0;
    let mut newest: Option<Arc<RawFrame>> = None;
    while !stop.load(Ordering::Relaxed) {
        if let Some((seq, frame)) = tuning.tap().next_after(seen, KEEPALIVE) {
            seen = seq;
            newest = Some(frame);
        }
        let Some(frame) = &newest else { continue };
        if (frame.width, frame.height, frame.format) != shape {
            tracing::warn!("the desktop frame size changed; the WebRTC stream stops");
            return;
        }
        let buffer = gst::Buffer::from_slice(Pixels(Arc::clone(frame)));
        let flow = source.emit_by_name::<gst::FlowReturn>("push-buffer", &[&buffer]);
        if !matches!(flow, gst::FlowReturn::Ok) {
            return;
        }
    }
}

/// Replaces `profile-level-id` in the `a=fmtp:<payload>` line of `offer_sdp`.
pub fn rewrite_profile_level_id(offer_sdp: &str, payload: u8, id: &str) -> String {
    let prefix = format!("a=fmtp:{payload} ");
    let mut out = String::with_capacity(offer_sdp.len());
    for line in offer_sdp.split_inclusive('\n') {
        if line.starts_with(&prefix) {
            let (text, ending) = line.split_at(line.trim_end_matches(['\r', '\n']).len());
            let params: Vec<String> = text[prefix.len()..]
                .split(';')
                .map(|kv| {
                    if kv.trim().starts_with("profile-level-id=") {
                        format!("profile-level-id={id}")
                    } else {
                        kv.to_string()
                    }
                })
                .collect();
            out.push_str(&prefix);
            out.push_str(&params.join(";"));
            out.push_str(ending);
        } else {
            out.push_str(line);
        }
    }
    out
}

pub struct WebRtcSession {
    pipeline: gst::Pipeline,
    rtc: gst::Element,
    encoder_element: gst::Element,
    payloader: gst::Element,
    payload: u8,
    encoder: Encoder,
    failure: Arc<Mutex<Option<String>>>,
    frames: Arc<AtomicU64>,
    feeder_stop: Arc<AtomicBool>,
    feeder: Mutex<Option<JoinHandle<()>>>,
    closed: AtomicBool,
}

impl WebRtcSession {
    /// Builds the pipeline fed by the session's shared PipeWire frames and starts it; negotiation comes next.
    pub fn start(
        tuning: &Arc<VideoTuning>,
        bitrate_kbps: u32,
        h264: &H264Offer,
    ) -> Result<Self, String> {
        let payload = h264.payload;
        let encoder = pick_encoder()?;
        // The first frame fixes size and pixel format; a still desktop sends nothing new, so the
        // tap keeps the newest one.
        let (_, first) = tuning
            .tap()
            .next_after(0, FIRST_FRAME_TIMEOUT)
            .ok_or("the desktop has delivered no frame yet")?;
        let description = format!(
            "webrtcbin name=rtc bundle-policy=max-bundle \
             appsrc name=src is-live=true format=time do-timestamp=true block=false max-buffers=3 \
             leaky-type=downstream caps=\"video/x-raw,format={},width={},height={},framerate=30/1\" \
             ! queue max-size-buffers=2 leaky=downstream ! videoconvert ! {} \
             ! video/x-h264,profile=constrained-baseline ! h264parse config-interval=-1 \
             ! rtph264pay name=pay config-interval=-1 pt={pt} aggregate-mode=zero-latency \
             ! application/x-rtp,media=video,encoding-name=H264,payload={pt} ! rtc.",
            first.format.gst_name(),
            first.width,
            first.height,
            encoder.pipeline_fragment(bitrate_kbps),
            pt = h264.payload,
        );
        let pipeline = gst::parse::launch(&description)
            .map_err(|e| format!("pipeline: {e}"))?
            .downcast::<gst::Pipeline>()
            .map_err(|_| "pipeline is not a pipeline".to_string())?;
        let rtc = pipeline.by_name("rtc").ok_or("webrtcbin missing")?;
        let encoder_element = pipeline.by_name("enc").ok_or("encoder missing")?;
        let payloader = pipeline.by_name("pay").ok_or("payloader missing")?;
        let source = pipeline.by_name("src").ok_or("source missing")?;

        let failure = Arc::new(Mutex::new(None));
        if let Some(bus) = pipeline.bus() {
            let sink = Arc::clone(&failure);
            bus.set_sync_handler(move |_, message| {
                if let gst::MessageView::Error(error) = message.view() {
                    let text = format!("{} ({:?})", error.error(), error.debug());
                    tracing::warn!(%text, "webrtc pipeline error");
                    *sink.lock().unwrap_or_else(PoisonError::into_inner) = Some(text);
                }
                gst::BusSyncReply::Pass
            });
        }
        let frames = Arc::new(AtomicU64::new(0));
        if let Some(pad) = encoder_element.static_pad("src") {
            let counter = Arc::clone(&frames);
            pad.add_probe(gst::PadProbeType::BUFFER, move |_, _| {
                counter.fetch_add(1, Ordering::Relaxed);
                gst::PadProbeReturn::Ok
            });
        }
        pipeline
            .set_state(gst::State::Playing)
            .map_err(|e| format!("start pipeline: {e}"))?;
        let feeder_stop = Arc::new(AtomicBool::new(false));
        let feeder = {
            let (tuning, stop) = (Arc::clone(tuning), Arc::clone(&feeder_stop));
            let shape = (first.width, first.height, first.format);
            std::thread::Builder::new()
                .name("webrtc-feeder".into())
                .spawn(move || feed(&source, &tuning, &stop, shape))
                .map_err(|e| format!("feeder thread: {e}"))?
        };
        Ok(Self {
            pipeline,
            rtc,
            encoder_element,
            payloader,
            payload,
            encoder,
            failure,
            frames,
            feeder_stop,
            feeder: Mutex::new(Some(feeder)),
            closed: AtomicBool::new(false),
        })
    }

    /// Encoded frames produced so far; zero for a while means the source delivers nothing.
    pub fn frames(&self) -> u64 {
        self.frames.load(Ordering::Relaxed)
    }

    pub fn encoder(&self) -> Encoder {
        self.encoder
    }

    pub fn failure(&self) -> Option<String> {
        self.failure
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    pub fn set_bitrate(&self, kbps: u32) {
        self.encoder_element
            .set_property_from_str("bitrate", &self.encoder.bitrate_value(kbps));
    }

    /// Answers the browser's offer; the answer already carries this host's ICE candidates.
    pub fn answer(&self, offer_sdp: &str) -> Result<String, String> {
        if offer_sdp.len() > MAX_OFFER_BYTES {
            return Err("offer too large".into());
        }
        // webrtcbin matches the stream's exact profile-level-id (level 4.0 for 1080p), which no browser
        // offers; the browser accepts the same profile at another level.
        let offer_sdp =
            &rewrite_profile_level_id(offer_sdp, self.payload, &self.stream_profile_level_id());
        let sdp = gst_sdp::SDPMessage::parse_buffer(offer_sdp.as_bytes())
            .map_err(|_| "offer is not valid SDP".to_string())?;
        let offer =
            gst_webrtc::WebRTCSessionDescription::new(gst_webrtc::WebRTCSDPType::Offer, sdp);

        let (tx, rx) = mpsc::channel();
        let promise = gst::Promise::with_change_func(move |reply| {
            let _ = tx.send(reply.map(|_| ()).map_err(|e| format!("{e:?}")));
        });
        self.rtc
            .emit_by_name::<()>("set-remote-description", &[&offer, &promise]);
        rx.recv_timeout(NEGOTIATE_TIMEOUT)
            .map_err(|_| "set-remote-description timed out".to_string())??;

        let (tx, rx) = mpsc::channel();
        let promise = gst::Promise::with_change_func(move |reply| {
            let answer = reply.map_err(|e| format!("{e:?}")).and_then(|reply| {
                reply
                    .ok_or_else(|| "empty answer".to_string())?
                    .value("answer")
                    .map_err(|_| "no answer in reply".to_string())?
                    .get::<gst_webrtc::WebRTCSessionDescription>()
                    .map_err(|_| "answer has the wrong type".to_string())
            });
            let _ = tx.send(answer);
        });
        self.rtc
            .emit_by_name::<()>("create-answer", &[&None::<gst::Structure>, &promise]);
        let answer = rx
            .recv_timeout(NEGOTIATE_TIMEOUT)
            .map_err(|_| "create-answer timed out".to_string())??;
        self.rtc
            .emit_by_name::<()>("set-local-description", &[&answer, &None::<gst::Promise>]);

        let deadline = Instant::now() + GATHER_TIMEOUT;
        while self
            .rtc
            .property::<gst_webrtc::WebRTCICEGatheringState>("ice-gathering-state")
            != gst_webrtc::WebRTCICEGatheringState::Complete
        {
            if Instant::now() >= deadline {
                return Err("ICE gathering timed out".into());
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        let local = self
            .rtc
            .property::<Option<gst_webrtc::WebRTCSessionDescription>>("local-description")
            .ok_or("no local description")?;
        local
            .sdp()
            .as_text()
            .map_err(|_| "answer SDP could not be written".to_string())
    }

    /// The profile-level-id the payloader writes, known once the first frame has been encoded.
    fn stream_profile_level_id(&self) -> String {
        let deadline = Instant::now() + Duration::from_secs(4);
        loop {
            let id = self
                .payloader
                .static_pad("src")
                .and_then(|pad| pad.current_caps())
                .and_then(|caps| caps.structure(0)?.get::<String>("profile-level-id").ok());
            if let Some(id) = id {
                return id;
            }
            if Instant::now() >= deadline {
                return DEFAULT_PROFILE_LEVEL_ID.to_string();
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    pub fn close(&self) {
        if !self.closed.swap(true, Ordering::SeqCst) {
            self.feeder_stop.store(true, Ordering::Relaxed);
            let _ = self.pipeline.set_state(gst::State::Null);
            if let Some(feeder) = self
                .feeder
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .take()
            {
                let _ = feeder.join();
            }
        }
    }
}

impl Drop for WebRtcSession {
    fn drop(&mut self) {
        self.close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bitrate_units_follow_the_encoder() {
        assert_eq!(Encoder::Nvenc.bitrate_value(6000), "6000");
        assert_eq!(Encoder::OpenH264.bitrate_value(6000), "6000000");
    }

    const CHROME_LIKE_OFFER: &str = "v=0\r\nm=video 9 UDP/TLS/RTP/SAVPF 96 102 104 106\r\n\
a=rtpmap:96 VP8/90000\r\na=rtpmap:102 H264/90000\r\n\
a=fmtp:102 level-asymmetry-allowed=1;packetization-mode=1;profile-level-id=42001f\r\n\
a=rtpmap:104 H264/90000\r\n\
a=fmtp:104 level-asymmetry-allowed=1;packetization-mode=1;profile-level-id=42e01f\r\n\
a=rtpmap:106 H264/90000\r\n\
a=fmtp:106 level-asymmetry-allowed=1;packetization-mode=0;profile-level-id=42e01f\r\n";

    #[test]
    fn the_offered_h264_entry_is_chosen_by_mode_and_profile() {
        assert_eq!(
            pick_h264(CHROME_LIKE_OFFER),
            Some(H264Offer {
                payload: 104,
                profile_level_id: "42e01f".into()
            })
        );
        let high_only =
            "a=rtpmap:98 H264/90000\r\na=fmtp:98 packetization-mode=1;profile-level-id=640c1f\r\n";
        assert_eq!(pick_h264(high_only), None);
        assert_eq!(pick_h264("a=rtpmap:96 VP8/90000\r\n"), None);
    }

    #[test]
    fn only_the_chosen_payloads_profile_level_id_is_rewritten() {
        let out = rewrite_profile_level_id(CHROME_LIKE_OFFER, 104, "42c028");
        assert!(out.contains(
            "a=fmtp:104 level-asymmetry-allowed=1;packetization-mode=1;profile-level-id=42c028\r\n"
        ));
        assert!(out.contains(
            "a=fmtp:102 level-asymmetry-allowed=1;packetization-mode=1;profile-level-id=42001f\r\n"
        ));
        assert!(out.contains(
            "a=fmtp:106 level-asymmetry-allowed=1;packetization-mode=0;profile-level-id=42e01f\r\n"
        ));
        assert_eq!(out.lines().count(), CHROME_LIKE_OFFER.lines().count());
    }

    #[test]
    fn an_encoder_is_available_on_this_host() {
        let encoder = pick_encoder().expect("an H.264 encoder");
        println!("encoder: {}", encoder.label());
    }

    #[test]
    fn garbage_offers_are_refused_before_any_pipeline_work() {
        gst::init().unwrap();
        let rtc = gst::ElementFactory::make("webrtcbin").build().unwrap();
        let session = WebRtcSession {
            pipeline: gst::Pipeline::new(),
            encoder_element: gst::ElementFactory::make("identity").build().unwrap(),
            payloader: gst::ElementFactory::make("identity").build().unwrap(),
            payload: 96,
            rtc,
            encoder: Encoder::OpenH264,
            failure: Arc::new(Mutex::new(None)),
            frames: Arc::new(AtomicU64::new(0)),
            feeder_stop: Arc::new(AtomicBool::new(false)),
            feeder: Mutex::new(None),
            closed: AtomicBool::new(false),
        };
        assert!(session.answer("not sdp").is_err());
        assert!(session.answer(&"v=0\r\n".repeat(20_000)).is_err());
    }
}
