// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! The video path (M5, ADR-0009): cameras in, the vision mixer, the slate, AV1 to CMAF HLS,
//! and the programme picture back into the room.
//!
//! **Audio first.** Nothing here runs in the desk's audio pipeline or its mixing pass, and
//! nothing here can stop either. Each camera is decoded in a small pipeline of its own; the
//! encoders and the HLS writer are one pipeline; the picture for the room is another. A
//! failure in any of them takes down that pipeline alone, which is made again a moment later
//! (a camera on its next keyframe, NVENC replaced by SVT-AV1 if it was NVENC that failed). The
//! desk's own pipeline only hands each camera's encoded frames to an `appsink`, as it hands
//! voices to theirs, so a camera that fails cannot take a voice with it.
//!
//! **One picture pass.** Like the mixing pass for sound, one thread (`desk-vision`) makes the
//! programme picture: every frame time (50 ms at 20 fps) it asks the [`Director`] for the
//! shot, takes that camera's frame for the moment (or the slate), lays the lower third over
//! it, and keeps it in a short history. The history is the profanity delay for pictures: the
//! frame sent to the encoders is the one from the delay's length ago, read afresh every frame
//! from the delay the sound is going through, so picture and sound are dumped together and
//! build back up together (ADR-0008). The live frame, before the delay, goes to the room.
//!
//! **One timeline.** Every frame and every block of the HLS audio is stamped with the desk's
//! running time: audio by its block's place, pictures by the moment they show. A camera's
//! frames carry the running time their voice's samples carry, so the picture stays with the
//! voice it belongs to.

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, Weak};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};
use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_app as gst_app;
use serde::Serialize;

use crate::config::{Config, EncoderChoice, Rung, Video as Settings};
use crate::picture::{Frame, Yuv};
use crate::vision::{Candidate, Director, PictureMode, Rule, Shot};

/// NVENC sessions this process holds (GeForce cards allow 12 per machine).
static NVENC_IN_USE: AtomicU32 = AtomicU32::new(0);
/// Set once NVENC has failed in this process: SVT-AV1 from then on.
static NVENC_BROKEN: AtomicBool = AtomicBool::new(false);

/// What encodes a rung.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Encoder {
    Nvenc,
    Svt,
}

impl Encoder {
    /// Whether this machine can encode AV1 on an NVIDIA GPU: the element is registered only
    /// where the driver and a GPU are visible (a container needs `--device nvidia.com/gpu=all`).
    pub fn nvenc_present() -> bool {
        !NVENC_BROKEN.load(Ordering::SeqCst)
            && gst::ElementFactory::find("nvav1enc").is_some()
            && gst::ElementFactory::find("cudaupload").is_some()
    }
}

/// Pick each rung's encoder, holding NVENC sessions for those that get it.
fn choose_encoders(settings: &Settings, rungs: &[Rung]) -> Vec<Encoder> {
    rungs
        .iter()
        .map(|r| {
            let want_nvenc = settings.encoder != EncoderChoice::Svt;
            if want_nvenc && Encoder::nvenc_present() {
                let held = NVENC_IN_USE.fetch_add(1, Ordering::SeqCst);
                if held < settings.nvenc_sessions {
                    return Encoder::Nvenc;
                }
                NVENC_IN_USE.fetch_sub(1, Ordering::SeqCst);
                tracing::warn!(rung = r.name, held, limit = settings.nvenc_sessions, "no NVENC session left for this rung; SVT-AV1 on the CPU encodes it");
            } else if settings.encoder == EncoderChoice::Nvenc {
                tracing::warn!(rung = r.name, "NVENC was asked for but this machine has none (or it failed); SVT-AV1 on the CPU encodes it");
            }
            Encoder::Svt
        })
        .collect()
}

/// Hold one NVENC session if one is free under `limit` (RTMP out's encoders, `rtmp-out`).
pub(crate) fn take_nvenc(limit: u32) -> bool {
    let held = NVENC_IN_USE.fetch_add(1, Ordering::SeqCst);
    if held < limit {
        return true;
    }
    NVENC_IN_USE.fetch_sub(1, Ordering::SeqCst);
    false
}

pub(crate) fn release_nvenc() {
    NVENC_IN_USE.fetch_sub(1, Ordering::SeqCst);
}

fn release_encoders(encoders: &[Encoder]) {
    for e in encoders {
        if *e == Encoder::Nvenc {
            NVENC_IN_USE.fetch_sub(1, Ordering::SeqCst);
        }
    }
}

/// A person, as the vision mixer asks the desk about them.
#[derive(Clone, Debug)]
pub struct VisionPerson {
    pub identity: String,
    pub name: String,
    /// Their voice is in the programme now.
    pub audible: bool,
    pub loudness: Option<f32>,
    /// Callers are heard, not seen, in release one.
    pub may_be_seen: bool,
}

/// What the desk tells the vision mixer, and what the vision mixer needs from it.
pub trait DeskSide: Send + Sync {
    /// The desk's running time, in nanoseconds; `None` before it plays.
    fn running_ns(&self) -> Option<u64>;
    fn people(&self) -> Vec<VisionPerson>;
    /// The profanity delay's length now.
    fn delay_ms(&self) -> u32;
    /// Hand the HLS audio encoder's sink pad to the mixing pass (`None`: take it away).
    fn hls_audio(&self, pad: Option<gst::Pad>);
    /// Whether anyone is at the desk (the room picture runs only then).
    fn anyone(&self) -> bool;
    /// Hand an RTMP profile's sound encoder (its slot, [`crate::rtmp::Profile::slot`]) to the
    /// mixing pass; `None` takes it away (`rtmp-out`).
    fn rtmp_audio(&self, slot: usize, pad: Option<gst::Pad>);
}

/// The now-playing text on the slate (ADR-0010).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, serde::Deserialize)]
pub struct NowPlaying {
    #[serde(default)]
    pub station: Option<String>,
    #[serde(default)]
    pub show: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub artist: Option<String>,
}

/// The video path as the status reports it.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct VideoStatus {
    pub mode: PictureMode,
    #[serde(flatten)]
    pub shot: Shot,
    /// People whose camera has a picture now.
    pub cameras: Vec<String>,
    pub rungs: Vec<RungStatus>,
    /// The encoders and the HLS writer are running.
    pub hls: bool,
    /// The programme picture is published into the room.
    pub room: bool,
    /// Frames composed, and frames composed late (the pass fell behind and skipped).
    pub frames: u64,
    pub skipped: u64,
    pub now_playing: NowPlaying,
    /// The slate has a picture (ADR-0010).
    pub still: bool,
    /// RTMP out (ADR-0230): each destination and the encoders running.
    pub rtmp: crate::rtmp::RtmpStatus,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct RungStatus {
    pub name: &'static str,
    pub encoder: Encoder,
}

/// A frame's bytes for a `gst::Buffer` without a copy.
struct FrameBytes(Arc<Frame>);

impl AsRef<[u8]> for FrameBytes {
    fn as_ref(&self) -> &[u8] {
        &self.0.data
    }
}

/// One camera's frames: decoded and scaled to the canvas, stamped with the desk's running
/// time, the newest last.
type Frames = Arc<Mutex<VecDeque<(u64, Arc<Frame>)>>>;

struct Camera {
    /// The subscription it came from.
    serial: u64,
    /// The desk-side sink pad, to ask the sender for a keyframe through.
    pad: Option<gst::Pad>,
    decode: Option<Decode>,
    frames: Frames,
    last_frame: Arc<Mutex<Option<Instant>>>,
    /// Waiting for a keyframe to start decoding from.
    waiting_key: bool,
    asked_key: Option<Instant>,
    failed_at: Option<Instant>,
}

struct Decode {
    pipeline: gst::Pipeline,
    src: gst_app::AppSrc,
    caps_name: String,
    /// When it started: a decoder that has put out no picture since is stalled too.
    started: Instant,
}

impl Decode {
    fn stop(&self) {
        let _ = self.pipeline.set_state(gst::State::Null);
    }
}

struct HlsOut {
    pipeline: gst::Pipeline,
    src: gst_app::AppSrc,
    encoders: Vec<Encoder>,
}

struct RoomOut {
    pipeline: gst::Pipeline,
    src: gst_app::AppSrc,
}

/// The longest side of a camera thumbnail for the studio, and how often each is made again.
const THUMB_WIDTH: usize = 192;
const THUMB_EVERY: Duration = Duration::from_millis(900);
/// At most this many cameras get a thumbnail.
const THUMB_MAX: usize = 12;

/// `appsrc ! jpegenc ! appsink` at one size: one pipeline for every camera, fed one picture at
/// a time.
struct JpegPipe {
    pipeline: gst::Pipeline,
    src: gst_app::AppSrc,
    sink: gst_app::AppSink,
}

impl Drop for JpegPipe {
    fn drop(&mut self) {
        let _ = self.pipeline.set_state(gst::State::Null);
    }
}

impl JpegPipe {
    fn new(width: usize, height: usize) -> Result<JpegPipe> {
        let pipeline = gst::Pipeline::new();
        let src = gst_app::AppSrc::builder()
            .caps(
                &gst::Caps::builder("video/x-raw")
                    .field("format", "I420")
                    .field("width", width as i32)
                    .field("height", height as i32)
                    .field("framerate", gst::Fraction::new(1, 1))
                    .build(),
            )
            .format(gst::Format::Time)
            .build();
        let enc = gst::ElementFactory::make("jpegenc")
            .property("quality", 70i32)
            .build()
            .context("GStreamer element `jpegenc` is missing (gst-plugins-good)")?;
        let sink = gst_app::AppSink::builder()
            .caps(&gst::Caps::builder("image/jpeg").build())
            .sync(false)
            .max_buffers(2)
            .build();
        pipeline.add_many([src.upcast_ref(), &enc, sink.upcast_ref()])?;
        gst::Element::link_many([src.upcast_ref(), &enc, sink.upcast_ref()])?;
        pipeline.set_state(gst::State::Playing)?;
        Ok(JpegPipe {
            pipeline,
            src,
            sink,
        })
    }

    fn encode(&self, frame: &Frame) -> Result<Vec<u8>> {
        let mut buf = gst::Buffer::from_slice(frame.data.clone());
        {
            let b = buf.get_mut().expect("a new buffer is writable");
            b.set_pts(gst::ClockTime::ZERO);
            b.set_duration(gst::ClockTime::from_seconds(1));
        }
        self.src.push_buffer(buf)?;
        let sample = self
            .sink
            .try_pull_sample(gst::ClockTime::from_seconds(2))
            .ok_or_else(|| anyhow!("jpegenc gave no picture in 2 s"))?;
        let out = sample.buffer().ok_or_else(|| anyhow!("no buffer"))?;
        Ok(out.map_readable()?.as_slice().to_vec())
    }
}

#[derive(Default)]
struct Thumbs {
    pipe: Option<(JpegPipe, usize, usize)>,
    /// Per camera: when it was made, the camera frame it came from, and the JPEG in base64.
    made: HashMap<String, (Instant, Arc<Frame>, String)>,
}

struct Vision {
    mode: PictureMode,
    director: Director,
    now_playing: NowPlaying,
    slate: Arc<Frame>,
    slate_dirty: bool,
    /// Slates started and slates stored, so an older one never replaces a newer (they are
    /// rendered on threads of their own).
    slate_started: u64,
    slate_stored: u64,
    /// The slate's picture (ADR-0010) as decoded, and the file it came from.
    still: Option<((std::time::SystemTime, u64), Arc<Frame>)>,
    still_key: Option<(std::time::SystemTime, u64)>,
    still_checked: Option<Instant>,
    /// Rendered name straps, by name; `None` while one is being rendered.
    straps: HashMap<String, Option<Arc<Frame>>>,
    strap: Option<(String, Instant)>,
    /// A line the studio put up (a chat message featured on air, say), over any shot, until
    /// when.
    caption: Option<(String, Instant)>,
    history: VecDeque<(u64, Arc<Frame>)>,
    thumbs: HashMap<String, (Arc<Frame>, Arc<Frame>)>,
    frames: u64,
    skipped: u64,
    hls_retry: Option<Instant>,
    room_retry: Option<Instant>,
}

struct Inner {
    settings: Settings,
    livekit: crate::config::LiveKit,
    desk_identity: String,
    /// Publish the picture into the room (tests without a room do not).
    to_room: bool,
    canvas: Rung,
    cams: Mutex<HashMap<String, Camera>>,
    vision: Mutex<Vision>,
    hls: Mutex<Option<HlsOut>>,
    room: Mutex<Option<RoomOut>>,
    /// The JPEG encoder for the studio's camera thumbnails, made when first asked, and the
    /// last picture sent for each camera.
    thumbs: Mutex<Thumbs>,
    stop: AtomicBool,
    pushed: AtomicU64,
    /// RTMP out (`rtmp-out`).
    rtmp: crate::rtmp::Rtmp,
}

/// The video path. Cheap to clone.
#[derive(Clone)]
pub struct Video {
    inner: Arc<Inner>,
}

const SLATE_BG: (u8, u8, u8) = (0x15, 0x17, 0x1c);
const STRAP_BG: (u8, u8, u8) = (0x10, 0x12, 0x16);
const ACCENT: (u8, u8, u8) = (0xd9, 0x3a, 0x2b);

pub(crate) fn caps_raw(r: &Rung) -> gst::Caps {
    gst::Caps::builder("video/x-raw")
        .field("format", "I420")
        .field("width", r.width as i32)
        .field("height", r.height as i32)
        .field("framerate", gst::Fraction::new(r.fps as i32, 1))
        .field("pixel-aspect-ratio", gst::Fraction::new(1, 1))
        .build()
}

pub(crate) fn el(factory: &str) -> Result<gst::Element> {
    gst::ElementFactory::make(factory)
        .build()
        .with_context(|| format!("GStreamer element `{factory}` is missing"))
}

fn named(factory: &str, name: &str) -> Result<gst::Element> {
    gst::ElementFactory::make(factory)
        .name(name)
        .build()
        .with_context(|| format!("GStreamer element `{factory}` is missing"))
}

pub(crate) fn capsfilter(caps: gst::Caps) -> Result<gst::Element> {
    Ok(gst::ElementFactory::make("capsfilter")
        .property("caps", caps)
        .build()?)
}

pub(crate) fn leaky(ms: u64) -> Result<gst::Element> {
    Ok(gst::ElementFactory::make("queue")
        .property("max-size-buffers", 0u32)
        .property("max-size-bytes", 0u32)
        .property("max-size-time", ms * 1_000_000)
        .property_from_str("leaky", "downstream")
        .build()?)
}

/// The first error waiting on a pipeline's bus, as a line.
pub(crate) fn bus_error(p: &gst::Pipeline) -> Option<String> {
    let bus = p.bus()?;
    while let Some(msg) = bus.pop_filtered(&[gst::MessageType::Error]) {
        if let gst::MessageView::Error(e) = msg.view() {
            let src = msg.src().map(|s| s.path_string()).unwrap_or_default();
            return Some(format!(
                "{src}: {} ({})",
                e.error(),
                e.debug().map(|d| d.to_string()).unwrap_or_default()
            ));
        }
    }
    None
}

/// Escape text for Pango markup.
fn markup(s: &str) -> String {
    gst::glib::markup_escape_text(s).to_string()
}

/// Render Pango markup over a solid background, `width` by `height`, with GStreamer's
/// `textoverlay` (a one-off pipeline; a few milliseconds).
pub fn render_text(
    width: usize,
    height: usize,
    bg: (u8, u8, u8),
    text: &str,
    halign: &str,
    font: &str,
    xpad: i32,
) -> Result<Frame> {
    let argb = 0xff00_0000u32 | (u32::from(bg.0) << 16) | (u32::from(bg.1) << 8) | u32::from(bg.2);
    let p = gst::Pipeline::new();
    let src = gst::ElementFactory::make("videotestsrc")
        .property_from_str("pattern", "solid-color")
        .property("foreground-color", argb)
        .property("num-buffers", 1i32)
        .build()?;
    let caps = capsfilter(
        gst::Caps::builder("video/x-raw")
            .field("format", "I420")
            .field("width", width as i32)
            .field("height", height as i32)
            .field("framerate", gst::Fraction::new(1, 1))
            .build(),
    )?;
    let overlay = gst::ElementFactory::make("textoverlay")
        .property("text", text)
        .property("font-desc", font)
        .property_from_str("valignment", "center")
        .property_from_str("halignment", halign)
        .property_from_str("line-alignment", halign)
        .property("xpad", xpad)
        .property("shaded-background", false)
        .property("draw-shadow", false)
        .property("draw-outline", false)
        .build()
        .context("GStreamer element `textoverlay` is missing (gst-plugins-base's pango)")?;
    let conv = el("videoconvert")?;
    let sink = gst_app::AppSink::builder()
        .caps(
            &gst::Caps::builder("video/x-raw")
                .field("format", "I420")
                .build(),
        )
        .sync(false)
        .build();
    p.add_many([&src, &caps, &overlay, &conv, sink.upcast_ref()])?;
    gst::Element::link_many([&src, &caps, &overlay, &conv, sink.upcast_ref()])?;
    p.set_state(gst::State::Playing)?;
    let sample = sink.try_pull_sample(gst::ClockTime::from_seconds(5));
    let _ = p.set_state(gst::State::Null);
    let sample = sample.ok_or_else(|| anyhow!("textoverlay gave no picture"))?;
    let buf = sample.buffer().ok_or_else(|| anyhow!("no buffer"))?;
    let map = buf.map_readable()?;
    Frame::from_bytes(width, height, map.as_slice())
        .ok_or_else(|| anyhow!("a picture of the wrong size"))
}

fn yuv(c: (u8, u8, u8)) -> Yuv {
    Yuv::from_rgb(c.0, c.1, c.2)
}

/// How often the slate's picture file is looked at.
const STILL_LOOK: Duration = Duration::from_secs(1);

/// The slate's picture, from the PNG the media worker wrote (8-bit RGB): decoded here, since
/// the worker has already read the upload in its sandbox. The most pixels on a side is the
/// worker's own.
fn load_still(path: &Path) -> Result<Frame> {
    let bytes = std::fs::read(path).with_context(|| format!("cannot read {}", path.display()))?;
    let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    decoder.set_limits(png::Limits {
        bytes: 128 * 1024 * 1024,
    });
    let mut reader = decoder.read_info()?;
    let (w, h) = (reader.info().width as usize, reader.info().height as usize);
    if w == 0 || h == 0 || w > 8192 || h > 8192 {
        bail!("the picture is {w} by {h}; at most 8192 on a side");
    }
    let mut buf = vec![
        0u8;
        reader
            .output_buffer_size()
            .ok_or_else(|| anyhow!("the picture is too large for this machine"))?
    ];
    let info = reader.next_frame(&mut buf)?;
    let buf = &buf[..info.buffer_size()];
    let rgb: Vec<u8> = match reader.output_color_type().0 {
        png::ColorType::Rgb => buf.to_vec(),
        png::ColorType::Rgba => buf
            .as_chunks::<4>()
            .0
            .iter()
            .flat_map(|p| [p[0], p[1], p[2]])
            .collect(),
        png::ColorType::Grayscale => buf.iter().flat_map(|g| [*g, *g, *g]).collect(),
        png::ColorType::GrayscaleAlpha => buf
            .as_chunks::<2>()
            .0
            .iter()
            .flat_map(|p| [p[0], p[0], p[0]])
            .collect(),
        png::ColorType::Indexed => bail!("a palette picture was not expanded"),
    };
    Frame::from_rgb(w, h, &rgb).ok_or_else(|| anyhow!("the picture has the wrong number of pixels"))
}

/// The slate: the station, the show, and what is playing (ADR-0010). With a picture (`still`)
/// it takes the upper part, fitted and centred, and the words go in a band under it; without
/// one the words fill the screen.
fn render_slate(
    canvas: &Rung,
    station: &str,
    np: &NowPlaying,
    still: Option<&Frame>,
) -> Result<Frame> {
    let (w, h) = (canvas.width as usize, canvas.height as usize);
    let station = np.station.as_deref().unwrap_or(station);
    let mut text = format!(
        "<span size='xx-large' weight='bold'>{}</span>",
        markup(station)
    );
    if let Some(show) = np.show.as_deref().filter(|s| !s.is_empty()) {
        text.push_str(&format!("\n<span size='large'>{}</span>", markup(show)));
    }
    let playing = match (np.artist.as_deref(), np.title.as_deref()) {
        (Some(a), Some(t)) if !a.is_empty() && !t.is_empty() => Some(format!("{a} – {t}")),
        (_, Some(t)) if !t.is_empty() => Some(t.to_owned()),
        _ => None,
    };
    let mut f = match still {
        None => {
            if let Some(p) = playing {
                text.push_str(&format!(
                    "\n\n<span size='small' foreground='#9aa0aa'>NOW PLAYING</span>\n{}",
                    markup(&p)
                ));
            }
            let size = (h / 22).max(10);
            render_text(
                w,
                h,
                SLATE_BG,
                &text,
                "center",
                &format!("Noto Sans {size}"),
                0,
            )?
        }
        Some(still) => {
            // The words in a band along the bottom, the picture above it.
            let band = (h * 2 / 9) & !1;
            let area = (w, (h - band) & !1);
            if let Some(p) = playing {
                text.push_str(&format!(
                    "\n<span size='small' foreground='#9aa0aa'>NOW PLAYING</span>  {}",
                    markup(&p)
                ));
            }
            let size = (h / 36).max(8);
            let words = render_text(
                w,
                band,
                SLATE_BG,
                &text,
                "center",
                &format!("Noto Sans {size}"),
                0,
            )?;
            let mut f = Frame::solid(w, h, yuv(SLATE_BG));
            let shown = fit_into(still, area.0, area.1);
            let (x, y) = ((area.0 - shown.width) / 2, (area.1 - shown.height) / 2);
            f.blit(&shown, x, y);
            f.blit(&words, 0, area.1);
            f
        }
    };
    // The station's accent: a bar along the bottom.
    f.fill(0, h - h / 60 - 2, w, h / 60 + 2, yuv(ACCENT));
    Ok(f)
}

/// `picture` made to fit inside `w` by `h`, keeping its shape: smaller than the room, as it
/// is; larger, shrunk.
fn fit_into(picture: &Frame, w: usize, h: usize) -> Arc<Frame> {
    if picture.width <= w && picture.height <= h {
        return Arc::new(picture.clone());
    }
    let (nw, nh) = if picture.width * h >= picture.height * w {
        (w, (picture.height * w / picture.width).max(2))
    } else {
        ((picture.width * h / picture.height).max(2), h)
    };
    Arc::new(picture.scaled(nw & !1, nh & !1))
}

/// What marks a cached strap as a caption rather than a name.
const CAPTION: &str = "\u{1}caption:";

/// The longest caption shown (a featured chat message is cut there, with an ellipsis).
pub const CAPTION_MAX: usize = 160;

/// A caption: up to two lines on a wider strap.
fn render_caption(canvas: &Rung, text: &str) -> Result<Frame> {
    let (w, h) = (
        (canvas.width as usize * 7 / 8) & !1,
        (canvas.height as usize / 6) & !1,
    );
    let size = (h / 5).max(8);
    let mut f = render_text(
        w,
        h,
        STRAP_BG,
        &markup(text),
        "left",
        &format!("Noto Sans {size}"),
        (h / 4) as i32,
    )?;
    f.fill(0, 0, (h / 12).max(4), h, yuv(ACCENT));
    Ok(f)
}

/// A lower third: a name on a dark strap with the accent at its left.
fn render_strap(canvas: &Rung, name: &str) -> Result<Frame> {
    let (w, h) = (
        (canvas.width as usize * 5 / 8) & !1,
        (canvas.height as usize / 9) & !1,
    );
    let size = (h * 2 / 5).max(8);
    let mut f = render_text(
        w,
        h,
        STRAP_BG,
        &format!("<b>{}</b>", markup(name)),
        "left",
        &format!("Noto Sans {size}"),
        (h / 2) as i32,
    )?;
    f.fill(0, 0, (h / 8).max(4), h, yuv(ACCENT));
    Ok(f)
}

impl Video {
    /// The video path for a desk; nothing runs until [`Video::start`].
    pub fn new(config: &Config, to_room: bool) -> Video {
        let mut settings = config.video.clone();
        settings.camera_codecs = usable_camera_codecs(&settings.camera_codecs);
        let canvas = settings.canvas();
        let s = &settings.switch;
        let rule = Rule {
            speaking_lufs: s.speaking_lufs,
            margin_db: s.margin_db,
            switch_after: Duration::from_millis(u64::from(s.switch_after_ms)),
            min_shot: Duration::from_millis(u64::from(s.min_shot_ms)),
        };
        let slate = Arc::new(Frame::solid(
            canvas.width as usize,
            canvas.height as usize,
            yuv(SLATE_BG),
        ));
        Video {
            inner: Arc::new(Inner {
                livekit: config.livekit.clone(),
                desk_identity: config.desk.identity.clone(),
                to_room: to_room && settings.room.publish,
                canvas,
                cams: Mutex::new(HashMap::new()),
                vision: Mutex::new(Vision {
                    mode: PictureMode::Auto,
                    director: Director::new(rule, Instant::now()),
                    now_playing: NowPlaying::default(),
                    slate,
                    slate_dirty: true,
                    slate_started: 0,
                    slate_stored: 0,
                    still: None,
                    still_key: None,
                    still_checked: None,
                    straps: HashMap::new(),
                    strap: None,
                    caption: None,
                    history: VecDeque::new(),
                    thumbs: HashMap::new(),
                    frames: 0,
                    skipped: 0,
                    hls_retry: None,
                    room_retry: None,
                }),
                hls: Mutex::new(None),
                room: Mutex::new(None),
                thumbs: Mutex::new(Thumbs::default()),
                stop: AtomicBool::new(false),
                pushed: AtomicU64::new(0),
                rtmp: crate::rtmp::Rtmp::new(&settings, canvas),
                settings,
            }),
        }
    }

    pub fn settings(&self) -> &Settings {
        &self.inner.settings
    }

    /// RTMP out (`rtmp-out`).
    pub fn rtmp(&self) -> &crate::rtmp::Rtmp {
        &self.inner.rtmp
    }

    /// The picture's size and rate.
    pub fn canvas(&self) -> Rung {
        self.inner.canvas
    }

    fn vision(&self) -> MutexGuard<'_, Vision> {
        self.inner.vision.lock().expect("vision lock")
    }

    fn cams(&self) -> MutexGuard<'_, HashMap<String, Camera>> {
        self.inner.cams.lock().expect("cameras lock")
    }

    /// Start the picture pass on a thread of its own.
    pub fn start(&self, desk: Weak<dyn DeskSide>) -> Result<()> {
        let me = self.clone();
        std::thread::Builder::new()
            .name("desk-vision".into())
            .spawn(move || me.run(desk))
            .context("cannot start the vision thread")?;
        Ok(())
    }

    /// Stop the pass and every video pipeline.
    pub fn stop(&self) {
        self.inner.stop.store(true, Ordering::SeqCst);
        self.inner.thumbs.lock().expect("thumbs lock").pipe = None;
        self.inner.rtmp.stop(None);
        self.stop_hls(None);
        if let Some(r) = self.inner.room.lock().expect("room lock").take() {
            let _ = r.pipeline.set_state(gst::State::Null);
        }
        for (_, c) in self.cams().drain() {
            if let Some(d) = c.decode {
                d.stop();
            }
        }
    }

    /// A small JPEG of each camera that has a picture now, by identity, in base64: what the
    /// studio's camera strip shows so the presenter sees what they can cut to. Made when
    /// asked, and not more than about once a second for a camera, however often it is asked.
    pub fn thumbs(&self) -> Result<std::collections::BTreeMap<String, String>> {
        let canvas = &self.inner.canvas;
        let tw = THUMB_WIDTH.min(canvas.width as usize) & !1;
        let th = ((tw * canvas.height as usize / canvas.width as usize) & !1).max(2);
        let mut t = self.inner.thumbs.lock().expect("thumbs lock");
        let now = Instant::now();
        let mut out = std::collections::BTreeMap::new();
        let ids: Vec<String> = self.fresh_cameras().into_iter().take(THUMB_MAX).collect();
        for id in &ids {
            let Some(frame) = self.camera_frame(id, u64::MAX) else {
                continue;
            };
            if let Some((at, from, text)) = t.made.get(id)
                && (Arc::ptr_eq(from, &frame) || now.duration_since(*at) < THUMB_EVERY)
            {
                out.insert(id.clone(), text.clone());
                continue;
            }
            if t.pipe.as_ref().is_none_or(|(_, w, h)| (*w, *h) != (tw, th)) {
                t.pipe = Some((JpegPipe::new(tw, th)?, tw, th));
            }
            let small = if (frame.width, frame.height) == (tw, th) {
                frame.clone()
            } else {
                Arc::new(frame.scaled(tw, th))
            };
            let jpeg = match t.pipe.as_ref().expect("made above").0.encode(&small) {
                Ok(j) => j,
                Err(e) => {
                    // The encoder is made again on the next ask.
                    t.pipe = None;
                    return Err(e);
                }
            };
            let text = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, jpeg);
            t.made.insert(id.clone(), (now, frame, text.clone()));
            out.insert(id.clone(), text);
        }
        // Forget cameras that are gone.
        t.made.retain(|id, _| ids.contains(id));
        Ok(out)
    }

    pub fn set_mode(&self, mode: PictureMode) {
        tracing::info!(?mode, "picture");
        self.vision().mode = mode;
    }

    /// Put a line up over the picture for `ms` (`None` text takes it down): the chat's
    /// featured message, or anything else the studio wants viewers to read. It goes through
    /// the delay with the picture.
    pub fn set_caption(&self, text: Option<&str>, ms: u32) {
        let line = text.map(|t| {
            let t = t.trim();
            if t.chars().count() > CAPTION_MAX {
                format!("{}…", t.chars().take(CAPTION_MAX - 1).collect::<String>())
            } else {
                t.to_owned()
            }
        });
        self.vision().caption = line
            .filter(|t| !t.is_empty())
            .map(|t| (t, Instant::now() + Duration::from_millis(u64::from(ms))));
    }

    pub fn set_now_playing(&self, np: NowPlaying) {
        let mut v = self.vision();
        if v.now_playing != np {
            v.now_playing = np;
            v.slate_dirty = true;
        }
    }

    pub fn status(&self) -> VideoStatus {
        let cameras = self.fresh_cameras();
        let (hls, rungs) = match self.inner.hls.lock().expect("hls lock").as_ref() {
            Some(h) => (
                true,
                self.inner
                    .settings
                    .rung_list()
                    .iter()
                    .zip(&h.encoders)
                    .map(|(r, e)| RungStatus {
                        name: r.name,
                        encoder: *e,
                    })
                    .collect(),
            ),
            None => (false, Vec::new()),
        };
        let room = self.inner.room.lock().expect("room lock").is_some();
        let v = self.vision();
        VideoStatus {
            mode: v.mode.clone(),
            shot: v.director.shot().clone(),
            cameras,
            rungs,
            hls,
            room,
            frames: v.frames,
            skipped: v.skipped,
            now_playing: v.now_playing.clone(),
            still: v.still.is_some(),
            rtmp: self.inner.rtmp.status(),
        }
    }

    /// Make the encoders' pipeline fail, as a dying encoder would (tests: a video failure
    /// must leave the audio on air). `false` when it is not running.
    pub fn fail_encoders(&self) -> bool {
        let hls = self.inner.hls.lock().expect("hls lock");
        let Some(h) = hls.as_ref() else { return false };
        let Some(e) = h.pipeline.by_name("hls-v0") else {
            return false;
        };
        let msg =
            gst::message::Error::builder(gst::CoreError::Failed, "failure injected by a test")
                .src(&e)
                .build();
        e.post_message(msg).is_ok()
    }

    /// Frames sent to the encoders so far.
    pub fn frames_pushed(&self) -> u64 {
        self.inner.pushed.load(Ordering::Relaxed)
    }

    fn fresh_cameras(&self) -> Vec<String> {
        let stale = Duration::from_millis(u64::from(self.inner.settings.stale_ms));
        let mut v: Vec<String> = self
            .cams()
            .iter()
            .filter(|(_, c)| {
                c.last_frame
                    .lock()
                    .expect("camera clock")
                    .is_some_and(|t| t.elapsed() < stale)
            })
            .map(|(id, _)| id.clone())
            .collect();
        v.sort();
        v
    }

    // -----------------------------------------------------------------------------------
    // Cameras.

    /// The caps the desk takes a subscribed camera in: encoded, so the room's element does
    /// not decode it in the desk's pipeline.
    pub fn camera_caps(&self) -> gst::Caps {
        let mut caps = gst::Caps::new_empty();
        {
            let c = caps.get_mut().expect("new caps");
            for codec in &self.inner.settings.camera_codecs {
                let name = match codec.as_str() {
                    "VP8" => "video/x-vp8",
                    "VP9" => "video/x-vp9",
                    _ => "video/x-av1",
                };
                c.append_structure(gst::Structure::new_empty(name));
            }
        }
        caps
    }

    /// A person's camera arrived (`pad`: the desk-side sink, to ask for keyframes through).
    pub fn camera_attach(&self, identity: &str, serial: u64, pad: Option<gst::Pad>) {
        let old = self.cams().insert(
            identity.to_owned(),
            Camera {
                serial,
                pad,
                decode: None,
                frames: Arc::new(Mutex::new(VecDeque::new())),
                last_frame: Arc::new(Mutex::new(None)),
                waiting_key: true,
                asked_key: None,
                failed_at: None,
            },
        );
        if let Some(d) = old.and_then(|c| c.decode) {
            d.stop();
        }
        tracing::info!(participant = %identity, "camera");
    }

    /// A person's camera went (their subscription dropped; `serial`: only that one).
    pub fn camera_detach(&self, identity: &str, serial: Option<u64>) {
        let gone = {
            let mut cams = self.cams();
            if cams
                .get(identity)
                .is_some_and(|c| serial.is_none_or(|n| c.serial == n))
            {
                cams.remove(identity)
            } else {
                None
            }
        };
        if let Some(c) = gone {
            if let Some(d) = c.decode {
                d.stop();
            }
            tracing::info!(participant = %identity, "camera gone");
        }
    }

    /// One encoded (or, from a test, raw) frame from a camera, stamped with the desk's
    /// running time. Called on the room element's streaming thread: it only queues.
    pub fn camera_sample(&self, identity: &str, serial: u64, sample: &gst::Sample, pts: u64) {
        let (Some(buf), Some(caps)) = (sample.buffer(), sample.caps()) else {
            return;
        };
        let caps_name = caps
            .structure(0)
            .map(|s| s.name().to_string())
            .unwrap_or_default();
        // AV1 from the room carries no delta flags (rtpav1depay marks none), so every buffer
        // would look like a keyframe and a decoder started again mid-stream never finds its
        // sequence header. A keyframe is where the sender's sequence header is.
        let key = if caps_name == "video/x-av1" {
            buf.map_readable()
                .is_ok_and(|m| av1_has_sequence_header(m.as_slice()))
        } else {
            !buf.flags().contains(gst::BufferFlags::DELTA_UNIT)
        };
        let src = {
            let mut cams = self.cams();
            let Some(cam) = cams.get_mut(identity).filter(|c| c.serial == serial) else {
                return;
            };
            if cam
                .decode
                .as_ref()
                .is_some_and(|d| d.caps_name != caps_name)
            {
                tracing::info!(participant = %identity, codec = %caps_name, "camera codec changed");
                if let Some(d) = cam.decode.take() {
                    d.stop();
                }
                cam.waiting_key = true;
            }
            // A decoder that stopped putting out pictures while the camera still sends: after a
            // lost packet an AV1 decoder discards what it cannot parse, says nothing, and never
            // starts again by itself (the talk station, 2026-11-11: the picture went back to the
            // slate for good). Ask for a keyframe; if that is not enough, start the decoder again
            // on the next one.
            let stale = Duration::from_millis(u64::from(self.inner.settings.stale_ms));
            if let Some(d) = &cam.decode {
                let since = cam
                    .last_frame
                    .lock()
                    .expect("camera clock")
                    .map_or(Duration::MAX, |t| t.elapsed())
                    .min(d.started.elapsed());
                if since > stale * 3 {
                    let jitter = cam.pad.as_ref().map(video_jitter_stats).unwrap_or_default();
                    tracing::warn!(participant = %identity, stalled_ms = since.as_millis() as u64, %jitter, "a camera's decoder stalled; it starts again on the next keyframe");
                    if let Some(d) = cam.decode.take() {
                        d.stop();
                    }
                    cam.waiting_key = true;
                } else if since > stale
                    && cam
                        .asked_key
                        .is_none_or(|t| t.elapsed() > Duration::from_secs(1))
                {
                    cam.asked_key = Some(Instant::now());
                    if let Some(pad) = &cam.pad {
                        ask_keyframe(pad);
                    }
                }
            }
            if cam.decode.is_none() {
                if cam
                    .failed_at
                    .is_some_and(|t| t.elapsed() < Duration::from_secs(2))
                {
                    return;
                }
                if !key && caps_name != "video/x-raw" {
                    // Nothing to decode from until a keyframe: ask the sender for one.
                    if cam
                        .asked_key
                        .is_none_or(|t| t.elapsed() > Duration::from_secs(1))
                    {
                        cam.asked_key = Some(Instant::now());
                        if let Some(pad) = &cam.pad {
                            ask_keyframe(pad);
                        }
                    }
                    return;
                }
                match self.make_decode(identity, caps, &caps_name, cam) {
                    Ok(d) => {
                        cam.decode = Some(d);
                        cam.waiting_key = false;
                    }
                    Err(e) => {
                        tracing::warn!(participant = %identity, error = %format!("{e:#}"), "cannot decode a camera");
                        cam.failed_at = Some(Instant::now());
                        return;
                    }
                }
            }
            cam.decode.as_ref().map(|d| d.src.clone())
        };
        if let Some(src) = src {
            let mut b = buf.copy();
            {
                let b = b.get_mut().expect("a copied buffer is writable");
                b.set_pts(gst::ClockTime::from_nseconds(pts));
                b.set_dts(gst::ClockTime::NONE);
            }
            let _ = src.push_buffer(b);
        }
    }

    /// A camera's decoder: its frames, scaled to the canvas with borders as needed, into the
    /// camera's frame list.
    fn make_decode(
        &self,
        identity: &str,
        caps: &gst::CapsRef,
        caps_name: &str,
        cam: &Camera,
    ) -> Result<Decode> {
        let canvas = self.inner.canvas;
        let p = gst::Pipeline::with_name(&format!("cam-{identity}"));
        let src = gst_app::AppSrc::builder()
            .caps(&caps.to_owned())
            .format(gst::Format::Time)
            .is_live(false)
            .max_bytes(4_000_000)
            .build();
        src.set_property_from_str("leaky-type", "downstream");
        let mut chain: Vec<gst::Element> = vec![src.clone().upcast()];
        match caps_name {
            "video/x-raw" => {}
            "video/x-vp8" => chain.push(el("vp8dec")?),
            "video/x-vp9" => chain.push(el("vp9dec")?),
            "video/x-av1" => {
                chain.push(el("av1parse")?);
                let dec = gst::ElementFactory::make("dav1ddec")
                    .property("n-threads", 2u32)
                    .build()
                    .or_else(|_| el("av1dec"))?;
                chain.push(dec);
            }
            other => bail!("cannot decode {other}"),
        }
        chain.push(el("videoconvert")?);
        chain.push(
            gst::ElementFactory::make("videoscale")
                .property("add-borders", true)
                .build()?,
        );
        chain.push(capsfilter(
            gst::Caps::builder("video/x-raw")
                .field("format", "I420")
                .field("width", canvas.width as i32)
                .field("height", canvas.height as i32)
                .field("pixel-aspect-ratio", gst::Fraction::new(1, 1))
                .build(),
        )?);
        let sink = gst_app::AppSink::builder()
            .sync(false)
            .max_buffers(2)
            .build();
        sink.set_property_from_str("leaky-type", "downstream");
        let frames = cam.frames.clone();
        let last = cam.last_frame.clone();
        let keep =
            Duration::from_millis(u64::from(self.inner.settings.lag_ms) + 400).as_nanos() as u64;
        let (w, h) = (canvas.width as usize, canvas.height as usize);
        // Said once per decoder: "decoding a camera" alone does not mean a picture came out.
        let first = AtomicBool::new(false);
        let who = identity.to_owned();
        sink.set_callbacks(
            gst_app::AppSinkCallbacks::builder()
                .new_sample(move |s| {
                    let sample = s.pull_sample().map_err(|_| gst::FlowError::Eos)?;
                    if !first.swap(true, Ordering::Relaxed) {
                        tracing::info!(participant = %who, "a camera's first picture");
                    }
                    let Some(buf) = sample.buffer() else {
                        return Ok(gst::FlowSuccess::Ok);
                    };
                    let (Some(pts), Ok(map)) = (buf.pts(), buf.map_readable()) else {
                        return Ok(gst::FlowSuccess::Ok);
                    };
                    if let Some(f) = Frame::from_bytes(w, h, map.as_slice()) {
                        let t = pts.nseconds();
                        let mut q = frames.lock().expect("camera frames");
                        q.push_back((t, Arc::new(f)));
                        while q.len() > 1
                            && q.front()
                                .is_some_and(|(ft, _)| t.saturating_sub(*ft) > keep)
                        {
                            q.pop_front();
                        }
                        *last.lock().expect("camera clock") = Some(Instant::now());
                    }
                    Ok(gst::FlowSuccess::Ok)
                })
                .build(),
        );
        chain.push(sink.upcast());
        p.add_many(&chain)?;
        gst::Element::link_many(&chain)?;
        p.set_state(gst::State::Playing)
            .context("a camera's decoder would not start")?;
        tracing::info!(participant = %identity, codec = %caps_name, caps = %caps, "decoding a camera");
        Ok(Decode {
            pipeline: p,
            src,
            caps_name: caps_name.to_owned(),
            started: Instant::now(),
        })
    }

    /// A camera's frame for the moment `t` (the newest at or before it, else the oldest).
    fn camera_frame(&self, identity: &str, t: u64) -> Option<Arc<Frame>> {
        let stale = Duration::from_millis(u64::from(self.inner.settings.stale_ms));
        let cams = self.cams();
        let cam = cams.get(identity)?;
        if !cam
            .last_frame
            .lock()
            .expect("camera clock")
            .is_some_and(|l| l.elapsed() < stale)
        {
            return None;
        }
        let q = cam.frames.lock().expect("camera frames");
        q.iter()
            .rev()
            .find(|(ft, _)| *ft <= t)
            .or_else(|| q.front())
            .map(|(_, f)| f.clone())
    }

    /// Take down cameras whose decoder failed; they start again on a keyframe.
    fn check_cameras(&self) {
        let mut cams = self.cams();
        for (id, cam) in cams.iter_mut() {
            let err = cam.decode.as_ref().and_then(|d| bus_error(&d.pipeline));
            if let Some(e) = err {
                tracing::warn!(participant = %id, error = %e, "a camera's decoder failed; it starts again on the next keyframe");
                if let Some(d) = cam.decode.take() {
                    d.stop();
                }
                cam.failed_at = Some(Instant::now());
                cam.waiting_key = true;
            }
        }
    }

    // -----------------------------------------------------------------------------------
    // The HLS writer.

    /// Build and start the encoders and the HLS writer; hand the audio encoder to the desk.
    fn start_hls(&self, desk: &dyn DeskSide) -> Result<()> {
        let s = &self.inner.settings;
        let rungs = s.rung_list();
        let encoders = choose_encoders(s, &rungs);
        let built = build_hls(s, &self.inner.canvas, &rungs, &encoders);
        let (pipeline, src, audio_pad) = match built {
            Ok(b) => b,
            Err(e) => {
                release_encoders(&encoders);
                return Err(e);
            }
        };
        if let Err(e) = pipeline.set_state(gst::State::Playing) {
            let _ = pipeline.set_state(gst::State::Null);
            release_encoders(&encoders);
            return Err(anyhow!("the HLS writer would not start: {e}"));
        }
        desk.hls_audio(Some(audio_pad));
        tracing::info!(
            dir = %s.hls.dir.display(),
            encoders = ?encoders,
            rungs = ?rungs.iter().map(|r| r.name).collect::<Vec<_>>(),
            "video: AV1 to CMAF HLS"
        );
        *self.inner.hls.lock().expect("hls lock") = Some(HlsOut {
            pipeline,
            src,
            encoders,
        });
        Ok(())
    }

    fn stop_hls(&self, desk: Option<&dyn DeskSide>) {
        if let Some(d) = desk {
            d.hls_audio(None);
        }
        if let Some(h) = self.inner.hls.lock().expect("hls lock").take() {
            let _ = h.pipeline.set_state(gst::State::Null);
            release_encoders(&h.encoders);
        }
    }

    fn check_hls(&self, desk: &dyn DeskSide) {
        let err = self
            .inner
            .hls
            .lock()
            .expect("hls lock")
            .as_ref()
            .and_then(|h| bus_error(&h.pipeline).map(|e| (e, h.encoders.clone())));
        if let Some((e, encoders)) = err {
            tracing::error!(error = %e, "the video encoders or HLS writer failed; audio goes on, video starts again shortly");
            if encoders.contains(&Encoder::Nvenc) && (e.contains("nvav1enc") || e.contains("cuda"))
            {
                NVENC_BROKEN.store(true, Ordering::SeqCst);
                tracing::warn!("NVENC failed: SVT-AV1 on the CPU from now on");
            }
            self.stop_hls(Some(desk));
            self.vision().hls_retry = Some(Instant::now() + Duration::from_secs(2));
        }
    }

    // -----------------------------------------------------------------------------------
    // The picture in the room (ADR-0028).

    fn start_room(&self) -> Result<()> {
        let canvas = self.inner.canvas;
        let p = gst::Pipeline::with_name("video-room");
        let src = gst_app::AppSrc::builder()
            .caps(&caps_raw(&canvas))
            .format(gst::Format::Time)
            .is_live(true)
            .do_timestamp(true)
            .max_buffers(4)
            .build();
        src.set_property_from_str("leaky-type", "downstream");
        let q = leaky(500)?;
        let sink = named("livekitwebrtcsink", "room-picture")?;
        sink.set_property("video-caps", gst::Caps::builder("video/x-vp8").build());
        sink.set_property("max-bitrate", 1_200_000u32);
        let identity = format!("{}-pgm", self.inner.desk_identity);
        // Anyone in the room may watch the picture: it is the programme, and who watches it
        // changes as people join (ADR-0118).
        crate::pipeline::signal(
            &sink,
            &self.inner.livekit,
            &self.inner.livekit.room,
            &identity,
            crate::pipeline::Join::Output { only: None },
        );
        p.add_many([src.upcast_ref(), &q, &sink])?;
        gst::Element::link_many([src.upcast_ref(), &q, &sink])?;
        p.set_state(gst::State::Playing)
            .context("the room picture would not start")?;
        tracing::info!(%identity, "video: the programme picture published to the room (VP8)");
        *self.inner.room.lock().expect("room lock") = Some(RoomOut { pipeline: p, src });
        Ok(())
    }

    fn check_room(&self) {
        let err = self
            .inner
            .room
            .lock()
            .expect("room lock")
            .as_ref()
            .and_then(|r| bus_error(&r.pipeline));
        if let Some(e) = err {
            tracing::warn!(error = %e, "the room picture failed; publishing it again in 5 s");
            if let Some(r) = self.inner.room.lock().expect("room lock").take() {
                let _ = r.pipeline.set_state(gst::State::Null);
            }
            self.vision().room_retry = Some(Instant::now() + Duration::from_secs(5));
        }
    }

    // -----------------------------------------------------------------------------------
    // The picture pass.

    fn run(&self, desk: Weak<dyn DeskSide>) {
        let canvas = self.inner.canvas;
        let frame_ns = 1_000_000_000 / u64::from(canvas.fps);
        let lag_ns = u64::from(self.inner.settings.lag_ms) * 1_000_000;
        let mut next: Option<u64> = None;
        while !self.inner.stop.load(Ordering::SeqCst) {
            std::thread::sleep(Duration::from_millis(5));
            let Some(desk) = desk.upgrade() else { break };
            let desk: &dyn DeskSide = &*desk;
            let Some(now) = desk.running_ns() else {
                next = None;
                continue;
            };
            self.housekeeping(desk);
            let Some(due_until) = now.checked_sub(lag_ns) else {
                continue;
            };
            let t = next.get_or_insert(due_until / frame_ns * frame_ns);
            if due_until.saturating_sub(*t) > 10 * frame_ns {
                let skip = (due_until - *t) / frame_ns;
                self.vision().skipped += skip;
                *t += skip * frame_ns;
            }
            while *t <= due_until {
                self.tick(desk, *t, frame_ns);
                *t += frame_ns;
            }
        }
        self.stop();
    }

    /// Pipelines checked, started and made again; the slate and straps rendered.
    fn housekeeping(&self, desk: &dyn DeskSide) {
        self.check_cameras();
        self.check_hls(desk);
        self.check_room();
        self.inner.rtmp.housekeeping(desk);
        let now = Instant::now();
        let hls_due = {
            let v = self.vision();
            v.hls_retry.is_none_or(|t| now >= t)
        };
        if hls_due && self.inner.hls.lock().expect("hls lock").is_none() {
            if let Err(e) = self.start_hls(desk) {
                tracing::error!(error = %format!("{e:#}"), "video: cannot start the encoders; trying again in 5 s");
                self.vision().hls_retry = Some(now + Duration::from_secs(5));
            } else {
                self.vision().hls_retry = None;
            }
        }
        if self.inner.to_room {
            let running = self.inner.room.lock().expect("room lock").is_some();
            let due = self.vision().room_retry.is_none_or(|t| now >= t);
            if !running
                && due
                && desk.anyone()
                && let Err(e) = self.start_room()
            {
                tracing::warn!(error = %format!("{e:#}"), "video: cannot publish the picture to the room; trying again in 5 s");
                self.vision().room_retry = Some(now + Duration::from_secs(5));
            }
        }
        // The slate's picture: look at the file about once a second; a change draws the slate
        // again.
        let look = {
            let mut v = self.vision();
            if v.still_checked
                .is_none_or(|t| now.duration_since(t) >= STILL_LOOK)
            {
                v.still_checked = Some(now);
                true
            } else {
                false
            }
        };
        if look {
            let key = std::fs::metadata(&self.inner.settings.still_file)
                .ok()
                .filter(|m| m.is_file())
                .map(|m| (m.modified().unwrap_or(std::time::UNIX_EPOCH), m.len()));
            let mut v = self.vision();
            if key != v.still_key {
                v.still_key = key;
                v.slate_dirty = true;
            }
        }
        // The slate, when now-playing or its picture changed; off this thread, the old slate
        // meanwhile.
        let render = {
            let mut v = self.vision();
            if v.slate_dirty {
                v.slate_dirty = false;
                v.slate_started += 1;
                Some((v.now_playing.clone(), v.slate_started, v.still_key))
            } else {
                None
            }
        };
        if let Some((np, seq, key)) = render {
            let me = self.clone();
            let _ = std::thread::Builder::new()
                .name("desk-slate".into())
                .spawn(move || {
                    let settings = &me.inner.settings;
                    // The file's picture; the one already decoded while it has not changed.
                    let have = me
                        .vision()
                        .still
                        .clone()
                        .filter(|(k, _)| Some(*k) == key);
                    let still = match (key, have) {
                        (None, _) => None,
                        (Some(_), Some(had)) => Some(had),
                        (Some(k), None) => match load_still(&settings.still_file) {
                            Ok(f) => Some((k, Arc::new(f))),
                            Err(e) => {
                                tracing::warn!(error = %format!("{e:#}"), file = %settings.still_file.display(), "cannot read the slate's picture; the slate is text");
                                None
                            }
                        },
                    };
                    let picture = still.as_ref().map(|(_, f)| f.clone());
                    match render_slate(&me.inner.canvas, &settings.station, &np, picture.as_deref())
                    {
                        Ok(f) => {
                            let mut v = me.vision();
                            v.still = still;
                            if seq > v.slate_stored {
                                v.slate_stored = seq;
                                v.slate = Arc::new(f);
                            }
                        }
                        Err(e) => {
                            tracing::warn!(error = %format!("{e:#}"), "cannot render the slate")
                        }
                    }
                });
        }
    }

    fn strap_for(&self, name: &str) -> Option<Arc<Frame>> {
        let mut v = self.vision();
        match v.straps.get(name) {
            Some(s) => s.clone(),
            None => {
                v.straps.insert(name.to_owned(), None);
                drop(v);
                let me = self.clone();
                let name = name.to_owned();
                let _ = std::thread::Builder::new()
                    .name("desk-strap".into())
                    .spawn(move || {
                        let f = match name.strip_prefix(CAPTION) {
                            Some(text) => render_caption(&me.inner.canvas, text),
                            None => render_strap(&me.inner.canvas, &name),
                        }
                            .map_err(|e| tracing::warn!(error = %format!("{e:#}"), "cannot render a lower third"))
                            .ok()
                            .map(Arc::new);
                        let mut v = me.vision();
                        if v.straps.len() > 64 {
                            v.straps.clear();
                        }
                        v.straps.insert(name, f);
                    });
                None
            }
        }
    }

    /// One frame: the shot for the moment `t`, its lower third, into the history; the frame
    /// from the delay's length ago to the encoders; the live one to the room.
    fn tick(&self, desk: &dyn DeskSide, t: u64, frame_ns: u64) {
        let people = desk.people();
        let cameras = self.fresh_cameras();
        let candidates: Vec<Candidate> = people
            .iter()
            .filter(|p| p.may_be_seen)
            .map(|p| Candidate {
                identity: p.identity.clone(),
                audible: p.audible,
                camera: cameras.contains(&p.identity),
                loudness: p.loudness,
            })
            .collect();
        let now = Instant::now();
        let (shot, cut, mode_slate) = {
            let mut v = self.vision();
            let mode = v.mode.clone();
            let cut = v.director.decide(&mode, &candidates, now);
            (v.director.shot().clone(), cut, mode == PictureMode::Slate)
        };
        let _ = mode_slate;
        let base = match &shot {
            Shot::Person { identity } => self.camera_frame(identity, t),
            Shot::Slate => None,
        };
        let lt_ms = u64::from(self.inner.settings.lower_third_ms);
        if let (true, Shot::Person { identity }) = (cut, &shot)
            && lt_ms > 0
        {
            let name = people
                .iter()
                .find(|p| &p.identity == identity)
                .map(|p| {
                    if p.name.is_empty() {
                        p.identity.clone()
                    } else {
                        p.name.clone()
                    }
                })
                .unwrap_or_else(|| identity.clone());
            self.vision().strap = Some((name, now + Duration::from_millis(lt_ms)));
        }
        // A caption (over any shot) before a name (over a camera only).
        let over = {
            let mut v = self.vision();
            if v.caption.as_ref().is_some_and(|(_, until)| now >= *until) {
                v.caption = None;
            }
            if v.strap.as_ref().is_some_and(|(_, until)| now >= *until) {
                v.strap = None;
            }
            match (&v.caption, &v.strap, base.is_some()) {
                (Some((text, _)), ..) => Some(format!("{CAPTION}{text}")),
                (None, Some((name, _)), true) => Some(name.clone()),
                _ => None,
            }
        };
        let frame = base.unwrap_or_else(|| self.vision().slate.clone());
        let composed: Arc<Frame> = match over.and_then(|n| self.strap_for(&n)) {
            Some(s) => {
                let mut f = (*frame).clone();
                let (w, h) = (f.width, f.height);
                f.blit(&s, w / 16, h - h / 8 - s.height);
                Arc::new(f)
            }
            None => frame,
        };
        // The history: the delay for pictures.
        let delay_ns = u64::from(desk.delay_ms()) * 1_000_000;
        let out = {
            let mut v = self.vision();
            v.frames += 1;
            v.history.push_back((t, composed.clone()));
            let want = t.saturating_sub(delay_ns);
            while v.history.len() > 1 && v.history.get(1).is_some_and(|(ht, _)| *ht <= want) {
                v.history.pop_front();
            }
            v.history.front().map(|(_, f)| f.clone())
        };
        if let Some(out) = out {
            let src = self
                .inner
                .hls
                .lock()
                .expect("hls lock")
                .as_ref()
                .map(|h| h.src.clone());
            let mut b = gst::Buffer::from_slice(FrameBytes(out));
            {
                let b = b.get_mut().expect("a new buffer is writable");
                b.set_pts(gst::ClockTime::from_nseconds(t));
                b.set_duration(gst::ClockTime::from_nseconds(frame_ns));
            }
            // RTMP out takes the same delayed frame (`rtmp-out`).
            self.inner.rtmp.picture(&b);
            if let Some(src) = src
                && src.push_buffer(b).is_ok()
            {
                self.inner.pushed.fetch_add(1, Ordering::Relaxed);
            }
        }
        let room = self
            .inner
            .room
            .lock()
            .expect("room lock")
            .as_ref()
            .map(|r| r.src.clone());
        if let Some(src) = room {
            let frame = if self.inner.settings.room.thumbnails {
                self.with_thumbnails(&composed, &cameras, &shot)
            } else {
                composed
            };
            let _ = src.push_buffer(gst::Buffer::from_slice(FrameBytes(frame)));
        }
    }

    /// The room's picture: the programme with small pictures of the other cameras along the
    /// bottom, so the people on the show see each other through the desk (ADR-0028).
    fn with_thumbnails(&self, frame: &Arc<Frame>, cameras: &[String], shot: &Shot) -> Arc<Frame> {
        let others: Vec<&String> = cameras
            .iter()
            .filter(|c| !matches!(shot, Shot::Person { identity } if identity == *c))
            .take(5)
            .collect();
        if others.is_empty() {
            return frame.clone();
        }
        let (w, h) = (frame.width, frame.height);
        let (tw, th) = ((w / 6) & !1, (h / 6) & !1);
        let mut f = (**frame).clone();
        for (i, id) in others.iter().enumerate() {
            let Some(src) = self.camera_frame(id, u64::MAX) else {
                continue;
            };
            let thumb = {
                let mut v = self.vision();
                match v.thumbs.get(*id) {
                    Some((from, t)) if Arc::ptr_eq(from, &src) => t.clone(),
                    _ => {
                        let t = Arc::new(src.scaled(tw, th));
                        v.thumbs.insert((*id).clone(), (src, t.clone()));
                        t
                    }
                }
            };
            let x = w - 8 - (i + 1) * (tw + 8);
            f.blit(&thumb, x, h - th - 8);
        }
        Arc::new(f)
    }
}

/// Ask the sender of a camera for a keyframe: a force-key-unit event upstream, which the
/// room's element turns into a picture-loss indication.
/// The GStreamer elements a camera in `codec` needs at the desk: its RTP depayloader, and a
/// decoder (any one of the last list).
fn camera_elements(codec: &str) -> (&'static [&'static str], &'static [&'static str]) {
    match codec {
        "VP8" => (&["rtpvp8depay"], &["vp8dec"]),
        "VP9" => (&["rtpvp9depay"], &["vp9dec"]),
        _ => (&["rtpav1depay", "av1parse"], &["dav1ddec", "av1dec"]),
    }
}

/// Of the configured camera codecs, those this machine's GStreamer can take from the room and
/// decode. Offering the room a codec without its depayloader breaks that person's whole
/// subscription, voice included ("not-linked", made again about three times a second: the
/// talk station's AV1 cameras from Chromium, 2026-11-11); left out, a browser's page sends its
/// backup codec (VP8) instead. Never empties the list: with nothing usable, it is kept as set.
pub fn usable_camera_codecs(codecs: &[String]) -> Vec<String> {
    let have = |e: &&str| gst::ElementFactory::find(e).is_some();
    let usable: Vec<String> = codecs
        .iter()
        .filter(|c| {
            let (need, decoders) = camera_elements(c);
            let mut missing: Vec<String> = need
                .iter()
                .filter(|e| !have(e))
                .map(|e| (*e).to_owned())
                .collect();
            if !decoders.iter().any(have) {
                missing.push(decoders.join(" or "));
            }
            if missing.is_empty() {
                true
            } else {
                tracing::error!(codec = %c, missing = ?missing, "video: this machine cannot take cameras in this codec, so the desk does not offer it to the room");
                false
            }
        })
        .cloned()
        .collect();
    if usable.is_empty() {
        tracing::error!("video: no camera codec can be taken here; cameras will fail");
        return codecs.to_vec();
    }
    usable
}

/// Whether an AV1 buffer (OBUs in low-overhead format, each with its size) holds a sequence
/// header (OBU type 1): what a decoder needs to start, and what a sender puts before a keyframe.
fn av1_has_sequence_header(mut b: &[u8]) -> bool {
    while let Some(&h) = b.first() {
        let obu_type = (h >> 3) & 0x0f;
        if obu_type == 1 {
            return true;
        }
        let ext = h & 0x04 != 0;
        let has_size = h & 0x02 != 0;
        let mut at = 1 + usize::from(ext);
        if !has_size {
            return false; // the rest of the buffer is this OBU
        }
        let mut size = 0usize;
        for i in 0..8 {
            let Some(&byte) = b.get(at) else { return false };
            at += 1;
            size |= usize::from(byte & 0x7f) << (7 * i);
            if byte & 0x80 == 0 {
                break;
            }
        }
        let Some(rest) = b.get(at.saturating_add(size)..) else {
            return false;
        };
        b = rest;
    }
    false
}

/// The room's element a camera comes from (`livekitwebrtcsrc`), from the desk's sink for it,
/// behind its queue.
fn room_bin(pad: &gst::Pad) -> Option<gst::Bin> {
    pad.peer()
        .and_then(|p| p.parent_element())
        .and_then(|q| q.static_pad("sink"))
        .and_then(|s| s.peer())
        .and_then(|p| p.parent_element())
        .and_then(|e| e.downcast::<gst::Bin>().ok())
}

/// The video jitter buffers' own counts inside a camera's room element (lost, late, resent),
/// for the log when a camera stalls: what the network did to it at the desk.
fn video_jitter_stats(pad: &gst::Pad) -> String {
    let Some(bin) = room_bin(pad) else {
        return String::new();
    };
    let mut out = Vec::new();
    for e in bin.iterate_recurse().into_iter().flatten() {
        if e.factory().is_none_or(|f| f.name() != "rtpjitterbuffer") {
            continue;
        }
        if e.find_property("stats").is_none() {
            continue;
        }
        let caps = e
            .static_pad("sink")
            .and_then(|p| p.current_caps())
            .and_then(|c| {
                c.structure(0).map(|s| {
                    format!(
                        "{}/{}",
                        s.get::<String>("media").unwrap_or_default(),
                        s.get::<String>("encoding-name").unwrap_or_default()
                    )
                })
            })
            .unwrap_or_default();
        let st = e.property::<gst::Structure>("stats");
        let n = |k: &str| st.get::<u64>(k).unwrap_or_default();
        out.push(format!(
            "{} {caps}: pushed {} lost {} late {} dup {} rtx {} rtx-ok {} rtx-late {} latency {} ms",
            e.name(),
            n("num-pushed"),
            n("num-lost"),
            n("num-late"),
            n("num-duplicates"),
            n("rtx-count"),
            n("rtx-success-count"),
            n("rtx-late-count"),
            e.property::<u32>("latency")
        ));
    }
    out.join("; ")
}

/// Ask a camera's sender for a keyframe. `pad` is the desk's sink for the camera, behind a queue
/// fed by the room's element. Sent up from there the request is lost inside `livekitwebrtcsrc`
/// (its decodebin3, parsebin and depayloader stand between; 2026-11-12: no PLI ever left), so it
/// also goes straight into the element's `webrtcbin`, whose video pads lead to the RTP session
/// that turns it into a PLI for LiveKit to pass on.
fn ask_keyframe(pad: &gst::Pad) {
    let event = || {
        gst::event::CustomUpstream::new(
            gst::Structure::builder("GstForceKeyUnit")
                .field("all-headers", false)
                .build(),
        )
    };
    pad.send_event(event());
    let Some(bin) = room_bin(pad) else {
        return;
    };
    for e in bin.iterate_recurse().into_iter().flatten() {
        if e.factory().is_none_or(|f| f.name() != "webrtcbin") {
            continue;
        }
        for p in e.src_pads() {
            let video = p
                .current_caps()
                .and_then(|c| c.structure(0).and_then(|s| s.get::<String>("media").ok()))
                .is_some_and(|m| m == "video");
            if video {
                p.send_event(event());
            }
        }
    }
}

/// Every file the writer made before, so a start does not mix old segments with new.
fn clear_dir(dir: &Path) -> Result<()> {
    if !dir.exists() {
        return Ok(());
    }
    for entry in std::fs::read_dir(dir)? {
        let p = entry?.path();
        let ours = p
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| matches!(e, "m4s" | "mp4" | "m3u8" | "tmp"));
        if p.is_file() && ours {
            std::fs::remove_file(&p)?;
        }
    }
    Ok(())
}

fn write_atomic(path: &Path, text: &str) -> Result<()> {
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, text).with_context(|| format!("cannot write {}", tmp.display()))?;
    std::fs::rename(&tmp, path).with_context(|| format!("cannot write {}", path.display()))?;
    Ok(())
}

/// The AV1 codec string for HLS: main profile, level 3.0 (480p) or 3.1, main tier, 8-bit.
fn av1_codec(r: &Rung) -> &'static str {
    if r.width * r.height <= 665_856 {
        "av01.0.04M.08"
    } else {
        "av01.0.05M.08"
    }
}

/// The two multivariant playlists: `live.m3u8` (a video rendition per rung, with the audio
/// rendition) and `audio.m3u8` (the same audio rendition alone: the audio-only toggle,
/// ADR-0026, and older Apple devices, ADR-0024).
pub fn multivariant(rungs: &[Rung], opus_bitrate: u32) -> (String, String) {
    let audio_bw = opus_bitrate * 11 / 10;
    let mut live = String::from(
        "#EXTM3U\n#EXT-X-VERSION:7\n#EXT-X-INDEPENDENT-SEGMENTS\n\
         #EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"aud\",NAME=\"Programme\",LANGUAGE=\"und\",DEFAULT=YES,AUTOSELECT=YES,CHANNELS=\"2\",URI=\"audio/index.m3u8\"\n",
    );
    for (i, r) in rungs.iter().enumerate() {
        live.push_str(&format!(
            "#EXT-X-STREAM-INF:BANDWIDTH={},AVERAGE-BANDWIDTH={},CODECS=\"{},opus\",RESOLUTION={}x{},FRAME-RATE={}.000,AUDIO=\"aud\"\nv{i}/index.m3u8\n",
            r.kbps * 1000 * 12 / 10 + audio_bw,
            r.kbps * 1000 + opus_bitrate,
            av1_codec(r),
            r.width,
            r.height,
            r.fps,
        ));
    }
    let audio = format!(
        "#EXTM3U\n#EXT-X-VERSION:7\n#EXT-X-INDEPENDENT-SEGMENTS\n\
         #EXT-X-STREAM-INF:BANDWIDTH={audio_bw},CODECS=\"opus\"\naudio/index.m3u8\n"
    );
    (live, audio)
}

fn hls_sink(name: &str, dir: &Path, settings: &Settings) -> Result<gst::Element> {
    let h = &settings.hls;
    std::fs::create_dir_all(dir).with_context(|| format!("cannot make {}", dir.display()))?;
    clear_dir(dir)?;
    let at = |f: &str| dir.join(f).to_string_lossy().into_owned();
    gst::ElementFactory::make("hlscmafsink")
        .name(name)
        .property("sync", false)
        .property("target-duration", h.segment_ms / 1000)
        .property("playlist-length", h.playlist_segments)
        .property("max-files", h.keep_segments)
        .property("enable-program-date-time", true)
        .property("location", at("seg%05d.m4s"))
        .property("init-location", at("init%05d.mp4"))
        .property("playlist-location", at("index.m3u8"))
        .build()
        .context("GStreamer element `hlscmafsink` is missing (gst-plugins-rs hlssink3)")
}

/// The encode for one rung, from the canvas's frames to AV1 (public for the VMAF measurement).
pub fn encoder_chain(
    r: &Rung,
    canvas: &Rung,
    enc: Encoder,
    segment_s: u32,
) -> Result<Vec<gst::Element>> {
    let mut v = vec![leaky(2_000)?];
    if (r.width, r.height) != (canvas.width, canvas.height) {
        v.push(el("videoscale")?);
        v.push(capsfilter(
            gst::Caps::builder("video/x-raw")
                .field("width", r.width as i32)
                .field("height", r.height as i32)
                .build(),
        )?);
    }
    if r.fps != canvas.fps {
        v.push(
            gst::ElementFactory::make("videorate")
                .property("drop-only", true)
                .build()?,
        );
        v.push(capsfilter(
            gst::Caps::builder("video/x-raw")
                .field("framerate", gst::Fraction::new(r.fps as i32, 1))
                .build(),
        )?);
    }
    let gop = r.fps * segment_s;
    match enc {
        Encoder::Nvenc => {
            v.push(el("videoconvert")?);
            v.push(capsfilter(
                gst::Caps::builder("video/x-raw")
                    .field("format", "NV12")
                    .build(),
            )?);
            v.push(el("cudaupload")?);
            v.push(
                gst::ElementFactory::make("nvav1enc")
                    .property("bitrate", r.kbps)
                    .property("gop-size", gop as i32)
                    .property_from_str("rc-mode", "cbr")
                    .property_from_str("preset", "p4")
                    .property_from_str("tune", "ultra-low-latency")
                    .property("bframes", 0u32)
                    .property("zerolatency", true)
                    .property("vbv-buffer-size", r.kbps / 2)
                    .build()
                    .context("GStreamer element `nvav1enc` is missing")?,
            );
        }
        Encoder::Svt => {
            // Low-delay SVT-AV1 stalls in GStreamer's element (2026-10-26), so random access
            // with a short mini-GOP: four frames of reordering, 200 ms at 20 fps.
            v.push(
                gst::ElementFactory::make("svtav1enc")
                    .property("target-bitrate", r.kbps)
                    .property("intra-period-length", gop as i32 - 1)
                    .property("preset", 10u32)
                    .property("parameters-string", "hierarchical-levels=2")
                    .build()
                    .context("GStreamer element `svtav1enc` is missing")?,
            );
        }
    }
    v.push(el("av1parse")?);
    Ok(v)
}

/// The HLS pipeline: the canvas's frames into a tee, one encode and writer per rung; the
/// programme's sound (pushed by the mixing pass, after the delay) into Opus and its writer.
#[allow(clippy::type_complexity)]
fn build_hls(
    settings: &Settings,
    canvas: &Rung,
    rungs: &[Rung],
    encoders: &[Encoder],
) -> Result<(gst::Pipeline, gst_app::AppSrc, gst::Pad)> {
    let dir: PathBuf = settings.hls.dir.clone();
    std::fs::create_dir_all(&dir).with_context(|| format!("cannot make {}", dir.display()))?;
    clear_dir(&dir)?;
    let p = gst::Pipeline::with_name("video-hls");
    let src = gst_app::AppSrc::builder()
        .name("hls-picture")
        .caps(&caps_raw(canvas))
        .format(gst::Format::Time)
        .is_live(false)
        .max_buffers(40)
        .build();
    src.set_property_from_str("leaky-type", "downstream");
    let tee = el("tee")?;
    p.add_many([src.upcast_ref(), &tee])?;
    src.link(&tee)?;
    let segment_s = settings.hls.segment_ms / 1000;
    for (i, (r, e)) in rungs.iter().zip(encoders).enumerate() {
        let mut chain = encoder_chain(r, canvas, *e, segment_s)?;
        chain.push(hls_sink(
            &format!("hls-v{i}"),
            &dir.join(format!("v{i}")),
            settings,
        )?);
        p.add_many(&chain)?;
        gst::Element::link_many(&chain)?;
        tee.link(&chain[0])?;
    }
    let opus = gst::ElementFactory::make("opusenc")
        .name("hls-opus")
        .property("bitrate", settings.hls.opus_bitrate as i32)
        .property_from_str("frame-size", "20")
        .build()
        .context("GStreamer element `opusenc` is missing")?;
    let q = leaky(1_000)?;
    let audio = hls_sink("hls-audio", &dir.join("audio"), settings)?;
    p.add_many([&opus, &q, &audio])?;
    gst::Element::link_many([&opus, &q, &audio])?;
    let (live, audio_only) = multivariant(rungs, settings.hls.opus_bitrate);
    write_atomic(&dir.join("live.m3u8"), &live)?;
    write_atomic(&dir.join("audio.m3u8"), &audio_only)?;
    let pad = opus.static_pad("sink").expect("opusenc sink");
    Ok((p, src, pad))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_multivariant_playlists_name_av1_and_opus_and_the_audio_only_one_has_no_video() {
        let rungs = [Rung::PRESETS[0], Rung::PRESETS[2]];
        let (live, audio) = multivariant(&rungs, 96_000);
        assert!(
            live.contains("CODECS=\"av01.0.04M.08,opus\",RESOLUTION=854x480,FRAME-RATE=20.000")
        );
        assert!(live.contains("v0/index.m3u8") && live.contains("v1/index.m3u8"));
        assert!(live.contains("URI=\"audio/index.m3u8\""));
        assert!(audio.contains("CODECS=\"opus\"\naudio/index.m3u8"));
        assert!(!audio.contains("av01"));
        assert_eq!(live.matches("#EXT-X-STREAM-INF").count(), 2);
    }

    #[test]
    fn an_av1_keyframe_is_where_the_sequence_header_is() {
        // OBU header: type << 3 | has_size (0x02); then the size as leb128.
        let td = [2 << 3 | 0x02, 0];
        let seq = [1 << 3 | 0x02, 3, 0xaa, 0xbb, 0xcc];
        let frame = [6 << 3 | 0x02, 2, 0x10, 0x20];
        let ext_frame = [6 << 3 | 0x02 | 0x04, 0x08, 2, 0x10, 0x20];
        assert!(av1_has_sequence_header(
            &[td.as_slice(), &seq, &frame].concat()
        ));
        assert!(av1_has_sequence_header(&seq));
        assert!(!av1_has_sequence_header(&[td.as_slice(), &frame].concat()));
        assert!(!av1_has_sequence_header(
            &[ext_frame.as_slice(), &frame].concat()
        ));
        // After an OBU with an extension byte, the next is still found.
        assert!(av1_has_sequence_header(
            &[ext_frame.as_slice(), &seq].concat()
        ));
        // A size past the end, or an OBU without a size, ends the walk.
        assert!(!av1_has_sequence_header(&[6 << 3 | 0x02, 9, 1]));
        assert!(!av1_has_sequence_header(&[6 << 3, 1, 2, 3]));
        assert!(!av1_has_sequence_header(&[]));
    }

    #[test]
    fn a_camera_codec_is_offered_only_with_its_depayloader_and_a_decoder() {
        gst::init().unwrap();
        let all: Vec<String> = ["VP8", "AV1", "VP9"].map(String::from).to_vec();
        let usable = usable_camera_codecs(&all);
        let have = |e: &str| gst::ElementFactory::find(e).is_some();
        assert_eq!(
            usable.contains(&"AV1".to_owned()),
            have("rtpav1depay") && have("av1parse") && (have("dav1ddec") || have("av1dec")),
            "{usable:?}"
        );
        assert!(usable.contains(&"VP8".to_owned()), "{usable:?}");
        // Nothing usable: the list stays as set rather than empty.
        let none = vec!["AV1".to_owned()];
        if !usable.contains(&"AV1".to_owned()) {
            assert_eq!(usable_camera_codecs(&none), none);
        }
    }

    /// A PNG of one colour, as the media worker writes them.
    fn png_file(dir: &Path, name: &str, w: u32, h: u32, rgb: [u8; 3]) -> PathBuf {
        let path = dir.join(name);
        let mut out = Vec::new();
        let mut e = png::Encoder::new(&mut out, w, h);
        e.set_color(png::ColorType::Rgb);
        e.set_depth(png::BitDepth::Eight);
        let mut wr = e.write_header().unwrap();
        let data: Vec<u8> = rgb
            .iter()
            .copied()
            .cycle()
            .take(3 * (w * h) as usize)
            .collect();
        wr.write_image_data(&data).unwrap();
        wr.finish().unwrap();
        std::fs::write(&path, out).unwrap();
        path
    }

    fn scratch(name: &str) -> PathBuf {
        let d =
            std::env::temp_dir().join(format!("weavesignal-slate-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn a_still_is_read_from_the_workers_png() {
        let dir = scratch("load");
        let path = png_file(&dir, "slate.png", 201, 99, [220, 30, 30]);
        let f = load_still(&path).unwrap();
        assert_eq!((f.width, f.height), (201, 99));
        assert!(
            f.mean(10, 10, 100, 50)
                .distance(&Yuv::from_rgb(220, 30, 30))
                < 3.0
        );
        assert!(load_still(&dir.join("none.png")).is_err());
        std::fs::write(dir.join("junk.png"), b"not a picture").unwrap();
        assert!(load_still(&dir.join("junk.png")).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_slate_puts_a_still_above_its_words() {
        gst::init().unwrap();
        let canvas = Rung::PRESETS[0];
        let (w, h) = (canvas.width as usize, canvas.height as usize);
        let np = NowPlaying {
            show: Some("The Morning Kettle".into()),
            title: Some("Skye Boat Song".into()),
            artist: Some("The Corries".into()),
            ..NowPlaying::default()
        };
        let red = Yuv::from_rgb(220, 30, 30);
        let bg = yuv(SLATE_BG);
        // A wide picture, smaller than the room: centred, not enlarged, with the background
        // showing at its sides.
        let still = Frame::solid(400, 120, red);
        let f = render_slate(&canvas, "Radio Kettle", &np, Some(&still)).unwrap();
        assert_eq!((f.width, f.height), (w, h));
        let area_h = h - ((h * 2 / 9) & !1);
        assert!(
            f.mean(w / 2 - 100, area_h / 2 - 30, 200, 60).distance(&red) < 3.0,
            "no picture"
        );
        assert!(
            f.mean(8, 8, 100, 60).distance(&bg) < 3.0,
            "the picture was enlarged"
        );
        // The words in the band under it.
        let band = f.mean(w / 4, h * 7 / 9 + 4, w / 2, h / 9 - 8);
        assert!(band.y > bg.y + 3.0, "no words in the band: {band:?}");
        assert!(
            f.mean(0, h - 6, w, 4).distance(&yuv(ACCENT)) < 16.0,
            "no accent bar"
        );

        // A picture bigger than the room is shrunk to fit it.
        let big = Frame::solid(1280, 720, red);
        let f = render_slate(&canvas, "Radio Kettle", &np, Some(&big)).unwrap();
        assert!(f.mean(w / 2, area_h / 2, 40, 40).distance(&red) < 3.0);
        assert!(
            f.mean(w / 2 - 20, area_h + 2, 40, 2).distance(&red) > 20.0,
            "it ran into the words"
        );

        // No picture: the words fill the screen, as before.
        let f = render_slate(&canvas, "Radio Kettle", &np, None).unwrap();
        assert!(f.mean(8, 8, 100, 60).distance(&bg) < 3.0);
        assert!(f.mean(w / 4, h / 3, w / 2, h / 3).y > bg.y + 4.0);
    }

    #[test]
    fn markup_is_escaped() {
        assert_eq!(markup("Rock & <Roll>"), "Rock &amp; &lt;Roll&gt;");
    }
}
