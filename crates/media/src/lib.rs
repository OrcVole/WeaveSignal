// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! WeaveSignal's media worker, as a library: decode an untrusted audio file, measure its
//! loudness (EBU R128 integrated loudness and true peak), and write clean FLAC with the gain
//! that brings it to the station's target.
//!
//! The parsing is the dangerous part (ADR-0048 rule 2), so it runs in its own process (the
//! `weavesignal-media` binary), restricted before it reads a byte; and every parser here is
//! memory-safe Rust: Symphonia for WAV, FLAC, MP3, Ogg Vorbis and AAC in MP4, and a pure-Rust
//! Opus decoder for Ogg Opus. Playout only ever reads the FLAC this writes, never the upload.
//!
//! Loudness is set at import, stored as a gain, never measured at play time (report 08): the
//! file keeps the sound as it came (lowered only if it would clip), and playout applies the
//! stored gain when the cart fires.

use std::io::Cursor;

pub mod attachment;
pub mod avatar;
pub mod brand;
mod multistream;
pub mod picture;
pub mod still;
pub mod worker;

use serde::{Deserialize, Serialize};

/// The longest file a cart may be, in seconds, unless the caller says otherwise.
pub const DEFAULT_MAX_SECONDS: f64 = 600.0;
/// The quietest and loudest gain playout accepts (its `carts.fire` checks the same range).
pub const GAIN_MIN_DB: f64 = -40.0;
pub const GAIN_MAX_DB: f64 = 20.0;
/// EBU R128 needs 400 ms blocks; a shorter sound is measured padded with silence to this.
const MIN_MEASURE_S: f64 = 0.4;

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum MediaError {
    #[error("the file is empty")]
    Empty,
    #[error(
        "not an audio file this worker reads (WAV, FLAC, MP3, Ogg Vorbis, Ogg Opus, AAC in MP4): {0}"
    )]
    Unsupported(String),
    #[error("the file is damaged: {0}")]
    Malformed(String),
    #[error("the file has {0} channels in a layout this worker cannot fold to stereo")]
    Channels(usize),
    #[error("the file is longer than {0:.0} s")]
    TooLong(f64),
    #[error("the file is silent")]
    Silent,
    #[error("cannot write the result: {0}")]
    Encode(String),
    #[error("not a picture this worker reads (PNG or JPEG)")]
    NotAnImage,
    /// A picture that is whole but not what its place takes: the sentence says what.
    #[error("{0}")]
    Picture(String),
    #[error(
        "the picture is {width} by {height} pixels; the most is {max_side} on a side and {max_pixels} in all"
    )]
    ImageTooBig {
        width: usize,
        height: usize,
        max_side: usize,
        max_pixels: usize,
    },
    #[error("the picture is {width} by {height} pixels; the least is {min_side} on a side")]
    ImageTooSmall {
        width: usize,
        height: usize,
        min_side: usize,
    },
}

/// Decoded audio: interleaved samples, nominally within -1.0 to 1.0.
#[derive(Clone, Debug, PartialEq)]
pub struct Pcm {
    pub rate: u32,
    pub channels: usize,
    pub samples: Vec<f32>,
}

impl Pcm {
    pub fn frames(&self) -> usize {
        self.samples.len() / self.channels.max(1)
    }

    pub fn seconds(&self) -> f64 {
        self.frames() as f64 / f64::from(self.rate.max(1))
    }
}

/// What the worker settles on: the station's target and the ceiling the gain must respect.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Target {
    /// Integrated loudness to aim for, in LUFS.
    pub lufs: f64,
    /// True peak the gain must not push the sound above, in dBTP.
    pub ceiling_dbtp: f64,
}

impl Default for Target {
    fn default() -> Self {
        Self {
            lufs: -23.0,
            ceiling_dbtp: -1.0,
        }
    }
}

/// What the worker found and decided, as it reports it (one JSON line).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Report {
    /// The container and codec it read, e.g. `ogg/opus`, `flac`, `wav`.
    pub format: String,
    pub sample_rate: u32,
    pub channels: usize,
    pub duration_ms: u64,
    /// The sound as it came: integrated loudness (LUFS) and true peak (dBTP).
    pub integrated_lufs: f64,
    pub true_peak_dbtp: f64,
    /// The gain already in the FLAC (0, or negative if the decoded sound went over full scale).
    pub file_gain_db: f64,
    /// The gain playout applies when the cart fires, so the cart sounds at the target.
    pub play_gain_db: f64,
    /// The loudness the cart then sounds at, which is under the target when the ceiling held
    /// the gain back.
    pub result_lufs: f64,
    pub target: Target,
}

/// Decode any file the worker reads.
pub fn decode(bytes: &[u8], max_seconds: f64) -> Result<(Pcm, String), MediaError> {
    if bytes.is_empty() {
        return Err(MediaError::Empty);
    }
    if is_ogg_opus(bytes) {
        return decode_ogg_opus(bytes, max_seconds).map(|p| (p, "ogg/opus".to_owned()));
    }
    decode_symphonia(bytes, max_seconds)
}

/// An Ogg stream whose first packet is an Opus identification header.
fn is_ogg_opus(bytes: &[u8]) -> bool {
    // The first page's header is 27 bytes plus its segment table; the packet follows.
    if bytes.len() < 28 || &bytes[..4] != b"OggS" {
        return false;
    }
    let segments = usize::from(bytes[26]);
    let start = 27 + segments;
    bytes.get(start..start + 8) == Some(b"OpusHead")
}

fn decode_ogg_opus(bytes: &[u8], max_seconds: f64) -> Result<Pcm, MediaError> {
    let mut reader = ogg::PacketReader::new(Cursor::new(bytes));
    let malformed = |e: &dyn std::fmt::Display| MediaError::Malformed(e.to_string());
    let head = reader
        .read_packet()
        .map_err(|e| malformed(&e))?
        .ok_or_else(|| MediaError::Malformed("no Opus header".into()))?;
    let h = &head.data;
    if h.len() < 19 || &h[..8] != b"OpusHead" {
        return Err(MediaError::Malformed("no Opus header".into()));
    }
    let channels = usize::from(h[9]);
    let pre_skip = usize::from(u16::from_le_bytes([h[10], h[11]]));
    let output_gain_q8 = i16::from_le_bytes([h[16], h[17]]);
    let family = h[18];
    // Family 0 is mono or stereo; family 1 is up to 7.1 in Vorbis order, downmixed below.
    let multi = match family {
        0 if (1..=2).contains(&channels) => None,
        1 if (1..=8).contains(&channels) && h.len() >= 21 + channels => {
            let streams = usize::from(h[19]);
            let coupled = usize::from(h[20]);
            let mapping = h[21..21 + channels].to_vec();
            Some((streams, coupled, mapping))
        }
        _ => return Err(MediaError::Channels(channels)),
    };
    let serial = head.stream_serial();
    // The comment header comes next; it carries nothing the worker needs.
    reader.read_packet().map_err(|e| malformed(&e))?;

    let rate = 48_000u32;
    let max_frames = (max_seconds * f64::from(rate)) as usize;
    enum Dec {
        One(Box<opus_decoder::OpusDecoder>),
        Multi(multistream::Multistream),
    }
    let bad = |e: opus_decoder::OpusError| MediaError::Malformed(format!("Opus: {e:?}"));
    let mut decoder = match &multi {
        None => Dec::One(Box::new(
            opus_decoder::OpusDecoder::new(rate, channels).map_err(bad)?,
        )),
        Some((streams, coupled, mapping)) => {
            Dec::Multi(multistream::Multistream::new(*streams, *coupled, mapping).map_err(bad)?)
        }
    };
    let mut out = vec![0f32; 5760 * channels];
    let mut samples = Vec::new();
    let mut last_granule = None;
    while let Some(p) = reader.read_packet().map_err(|e| malformed(&e))? {
        if p.stream_serial() != serial {
            return Err(MediaError::Unsupported(
                "a chained or multiplexed Ogg stream".into(),
            ));
        }
        let n = match &mut decoder {
            Dec::One(d) => d.decode_float(&p.data, &mut out, false),
            Dec::Multi(d) => d.decode_float(&p.data, &mut out),
        }
        .map_err(|e| MediaError::Malformed(format!("Opus packet: {e:?}")))?;
        samples.extend_from_slice(&out[..n * channels]);
        if samples.len() / channels > max_frames + pre_skip {
            return Err(MediaError::TooLong(max_seconds));
        }
        if p.last_in_page() {
            last_granule = Some(p.absgp_page());
        }
        if p.last_in_stream() {
            break;
        }
    }
    // The pre-skip is the encoder's priming; the last granule position says where the sound
    // ends (RFC 7845 section 4).
    let decoded = samples.len() / channels;
    let end = last_granule
        .map(|g| usize::try_from(g).unwrap_or(usize::MAX).min(decoded))
        .unwrap_or(decoded);
    let start = pre_skip.min(end);
    let mut samples = samples[start * channels..end * channels].to_vec();
    if output_gain_q8 != 0 {
        let g = 10f32.powf(f32::from(output_gain_q8) / (20.0 * 256.0));
        samples.iter_mut().for_each(|s| *s *= g);
    }
    if channels > 2 {
        return Ok(Pcm {
            rate,
            channels: 2,
            samples: downmix_vorbis(&samples, channels),
        });
    }
    Ok(Pcm {
        rate,
        channels,
        samples,
    })
}

/// Down to stereo from Vorbis channel order (RFC 7845 section 5.1.1.2), ITU-style: centre and
/// surrounds at -3 dB into their side, the LFE left out. 3 to 8 channels.
fn downmix_vorbis(samples: &[f32], channels: usize) -> Vec<f32> {
    const H: f32 = std::f32::consts::FRAC_1_SQRT_2;
    // (left weight, right weight) for each channel in order.
    let w: &[(f32, f32)] = match channels {
        3 => &[(1.0, 0.0), (H, H), (0.0, 1.0)],
        4 => &[(1.0, 0.0), (0.0, 1.0), (H, 0.0), (0.0, H)],
        5 => &[(1.0, 0.0), (H, H), (0.0, 1.0), (H, 0.0), (0.0, H)],
        6 => &[
            (1.0, 0.0),
            (H, H),
            (0.0, 1.0),
            (H, 0.0),
            (0.0, H),
            (0.0, 0.0),
        ],
        7 => &[
            (1.0, 0.0),
            (H, H),
            (0.0, 1.0),
            (H, 0.0),
            (0.0, H),
            (0.5, 0.5),
            (0.0, 0.0),
        ],
        _ => &[
            (1.0, 0.0),
            (H, H),
            (0.0, 1.0),
            (H, 0.0),
            (0.0, H),
            (H, 0.0),
            (0.0, H),
            (0.0, 0.0),
        ],
    };
    samples
        .chunks_exact(channels)
        .flat_map(|f| {
            let l: f32 = f.iter().zip(w).map(|(x, (a, _))| x * a).sum();
            let r: f32 = f.iter().zip(w).map(|(x, (_, b))| x * b).sum();
            [l, r]
        })
        .collect()
}

fn decode_symphonia(bytes: &[u8], max_seconds: f64) -> Result<(Pcm, String), MediaError> {
    use symphonia::core::codecs::CodecParameters;
    use symphonia::core::codecs::audio::AudioDecoderOptions;
    use symphonia::core::errors::Error as SymErr;
    use symphonia::core::formats::probe::Hint;
    use symphonia::core::formats::{FormatOptions, TrackType};
    use symphonia::core::io::{MediaSourceStream, MediaSourceStreamOptions};
    use symphonia::core::meta::MetadataOptions;

    let mss = MediaSourceStream::new(
        Box::new(Cursor::new(bytes.to_vec())),
        MediaSourceStreamOptions::default(),
    );
    let mut format = symphonia::default::get_probe()
        .probe(
            &Hint::new(),
            mss,
            FormatOptions::default(),
            MetadataOptions::default(),
        )
        .map_err(|e| MediaError::Unsupported(e.to_string()))?;
    let container = format.format_info().short_name.to_owned();
    let track = format
        .default_track(TrackType::Audio)
        .ok_or_else(|| MediaError::Unsupported("no audio track".into()))?;
    let track_id = track.id;
    let Some(CodecParameters::Audio(params)) = track.codec_params.clone() else {
        return Err(MediaError::Unsupported("no audio codec parameters".into()));
    };
    let mut decoder = symphonia::default::get_codecs()
        .make_audio_decoder(&params, &AudioDecoderOptions::default())
        .map_err(|e| MediaError::Unsupported(e.to_string()))?;
    let codec = decoder.codec_info().short_name.to_owned();
    let format_name = if container == codec {
        container
    } else {
        format!("{container}/{codec}")
    };

    let mut rate = 0u32;
    let mut channels = 0usize;
    let mut samples: Vec<f32> = Vec::new();
    let mut chunk: Vec<f32> = Vec::new();
    loop {
        let packet = match format.next_packet() {
            Ok(Some(p)) => p,
            Ok(None) => break,
            Err(SymErr::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(SymErr::ResetRequired) => {
                return Err(MediaError::Unsupported(
                    "the stream changes its format part-way".into(),
                ));
            }
            Err(e) => return Err(MediaError::Malformed(e.to_string())),
        };
        if packet.track_id != track_id {
            continue;
        }
        let buf = match decoder.decode(&packet) {
            Ok(b) => b,
            // A damaged packet is skipped, as players do; a file of nothing else is caught
            // below as silent or empty.
            Err(SymErr::DecodeError(_)) => continue,
            Err(e) => return Err(MediaError::Malformed(e.to_string())),
        };
        let spec = buf.spec();
        let (r, c) = (spec.rate(), spec.channels().count());
        if rate == 0 {
            if !(1..=2).contains(&c) {
                return Err(MediaError::Channels(c));
            }
            if !(8_000..=192_000).contains(&r) {
                return Err(MediaError::Unsupported(format!("a sample rate of {r} Hz")));
            }
            rate = r;
            channels = c;
        } else if r != rate || c != channels {
            return Err(MediaError::Unsupported(
                "the stream changes its format part-way".into(),
            ));
        }
        buf.copy_to_vec_interleaved(&mut chunk);
        samples.extend_from_slice(&chunk);
        if samples.len() as f64 / channels as f64 > max_seconds * f64::from(rate) {
            return Err(MediaError::TooLong(max_seconds));
        }
    }
    if rate == 0 || samples.is_empty() {
        return Err(MediaError::Malformed("no audio could be decoded".into()));
    }
    Ok((
        Pcm {
            rate,
            channels,
            samples,
        },
        format_name,
    ))
}

/// Integrated loudness (LUFS) and true peak (dBTP) of the sound.
pub fn measure(pcm: &Pcm) -> Result<(f64, f64), MediaError> {
    use ebur128::{EbuR128, Mode};
    let meter_error = |e: ebur128::Error| MediaError::Malformed(format!("loudness meter: {e:?}"));
    let channels = u32::try_from(pcm.channels).map_err(|_| MediaError::Channels(pcm.channels))?;
    let mut meter =
        EbuR128::new(channels, pcm.rate, Mode::I | Mode::TRUE_PEAK).map_err(meter_error)?;
    meter.add_frames_f32(&pcm.samples).map_err(meter_error)?;
    // A sound shorter than one 400 ms block is measured with silence after it.
    let short = MIN_MEASURE_S - pcm.seconds();
    if short > 0.0 {
        let pad = vec![0f32; (short * f64::from(pcm.rate)).ceil() as usize * pcm.channels];
        meter.add_frames_f32(&pad).map_err(meter_error)?;
    }
    let lufs = meter.loudness_global().unwrap_or(f64::NEG_INFINITY);
    let mut peak = 0f64;
    for c in 0..channels {
        peak = peak.max(meter.true_peak(c).unwrap_or(0.0));
    }
    if !lufs.is_finite() || peak <= 0.0 {
        return Err(MediaError::Silent);
    }
    Ok((lufs, 20.0 * peak.log10()))
}

/// Encode to FLAC at 24 bits, so a later gain loses nothing that matters.
pub fn encode_flac(pcm: &Pcm) -> Result<Vec<u8>, MediaError> {
    use flacenc::component::BitRepr;
    use flacenc::error::Verify;
    const FULL: f32 = 8_388_607.0;
    let ints: Vec<i32> = pcm
        .samples
        .iter()
        .map(|s| (s.clamp(-1.0, 1.0) * FULL).round() as i32)
        .collect();
    let config = flacenc::config::Encoder::default()
        .into_verified()
        .map_err(|e| MediaError::Encode(format!("{e:?}")))?;
    let source = flacenc::source::MemSource::from_samples(
        &ints,
        pcm.channels,
        24,
        usize::try_from(pcm.rate).unwrap_or(48_000),
    );
    let stream = flacenc::encode_with_fixed_block_size(&config, source, config.block_size)
        .map_err(|e| MediaError::Encode(format!("{e:?}")))?;
    let mut sink = flacenc::bitsink::ByteSink::new();
    stream
        .write(&mut sink)
        .map_err(|e| MediaError::Encode(format!("{e:?}")))?;
    let mut flac = sink.as_slice().to_vec();
    // flacenc 0.5.1 writes the short last block's size as STREAMINFO's minimum block size;
    // the format excludes the last block, and a reader (Symphonia) then takes the stream for
    // a variable-block one and fails. The stream is fixed-block: minimum = maximum.
    if flac.len() >= 12 && &flac[..4] == b"fLaC" {
        let (min, max) = flac[8..12].split_at_mut(2);
        min.copy_from_slice(max);
    }
    Ok(flac)
}

/// The whole job: decode, measure, settle the gain, write the FLAC.
pub fn process(
    bytes: &[u8],
    target: Target,
    max_seconds: f64,
) -> Result<(Vec<u8>, Report), MediaError> {
    let (mut pcm, format) = decode(bytes, max_seconds)?;
    if pcm.frames() == 0 {
        return Err(MediaError::Empty);
    }
    let (lufs, true_peak) = measure(&pcm)?;
    // A lossy file can decode past full scale; lower it in the file rather than clip it, and
    // give the difference back at play time.
    let sample_peak = pcm.samples.iter().fold(0f32, |m, s| m.max(s.abs()));
    let file_gain_db = if sample_peak > 0.999 {
        -20.0 * f64::from(sample_peak).log10() - 0.1
    } else {
        0.0
    };
    if file_gain_db != 0.0 {
        let g = 10f32.powf(file_gain_db as f32 / 20.0);
        pcm.samples.iter_mut().for_each(|s| *s *= g);
    }
    // The gain to the target, held back so the true peak stays under the ceiling.
    let to_target = target.lufs - lufs;
    let to_ceiling = target.ceiling_dbtp - true_peak;
    let gain = to_target.min(to_ceiling).clamp(GAIN_MIN_DB, GAIN_MAX_DB);
    let play_gain_db = round2(gain - file_gain_db);
    let flac = encode_flac(&pcm)?;
    let report = Report {
        format,
        sample_rate: pcm.rate,
        channels: pcm.channels,
        duration_ms: (pcm.seconds() * 1000.0).round() as u64,
        integrated_lufs: round2(lufs),
        true_peak_dbtp: round2(true_peak),
        file_gain_db: round2(file_gain_db),
        play_gain_db,
        result_lufs: round2(lufs + gain),
        target,
    };
    Ok((flac, report))
}

fn round2(x: f64) -> f64 {
    (x * 100.0).round() / 100.0
}

/// A sine as 16-bit WAV: what the tests here, the control plane's and the stack test feed in.
pub fn wav_sine(rate: u32, channels: u16, secs: f64, freq: f64, amp: f64) -> Vec<u8> {
    let frames = (secs * f64::from(rate)) as u32;
    let data_len = frames * u32::from(channels) * 2;
    let mut w = Vec::with_capacity(44 + data_len as usize);
    w.extend_from_slice(b"RIFF");
    w.extend_from_slice(&(36 + data_len).to_le_bytes());
    w.extend_from_slice(b"WAVEfmt ");
    w.extend_from_slice(&16u32.to_le_bytes());
    w.extend_from_slice(&1u16.to_le_bytes());
    w.extend_from_slice(&channels.to_le_bytes());
    w.extend_from_slice(&rate.to_le_bytes());
    w.extend_from_slice(&(rate * u32::from(channels) * 2).to_le_bytes());
    w.extend_from_slice(&(channels * 2).to_le_bytes());
    w.extend_from_slice(&16u16.to_le_bytes());
    w.extend_from_slice(b"data");
    w.extend_from_slice(&data_len.to_le_bytes());
    for i in 0..frames {
        let t = f64::from(i) / f64::from(rate);
        let v = (amp * (2.0 * std::f64::consts::PI * freq * t).sin() * 32767.0) as i16;
        for _ in 0..channels {
            w.extend_from_slice(&v.to_le_bytes());
        }
    }
    w
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_wav_is_brought_to_the_target_and_written_as_flac() {
        // A 1 kHz sine at half scale is -9 LUFS a channel, -6 LUFS in stereo: a gain of -17 dB.
        let wav = wav_sine(48_000, 2, 3.0, 1000.0, 0.5);
        let (flac, r) = process(&wav, Target::default(), DEFAULT_MAX_SECONDS).unwrap();
        assert_eq!(r.format, "wave/pcm_s16le", "{r:?}");
        assert_eq!(
            (r.sample_rate, r.channels, r.duration_ms),
            (48_000, 2, 3000)
        );
        assert!((r.integrated_lufs - -6.0).abs() < 0.1, "{r:?}");
        assert!((r.result_lufs - -23.0).abs() < 0.02, "{r:?}");
        assert!((r.play_gain_db - (-23.0 - r.integrated_lufs)).abs() < 0.02);
        assert_eq!(&flac[..4], b"fLaC");
        // The FLAC decodes back to the same sound, unchanged in level.
        let (back, fmt) = decode(&flac, DEFAULT_MAX_SECONDS).unwrap();
        assert_eq!(fmt, "flac");
        assert_eq!(back.frames(), 144_000);
        let (lufs, _) = measure(&back).unwrap();
        assert!((lufs - r.integrated_lufs).abs() < 0.05);
    }

    #[test]
    fn the_ceiling_holds_the_gain_back() {
        // A short, peaky click wants a big lift that the ceiling refuses.
        let wav = wav_sine(48_000, 1, 0.05, 1000.0, 0.9);
        let target = Target {
            lufs: -9.0,
            ceiling_dbtp: -1.0,
        };
        let (_, r) = process(&wav, target, DEFAULT_MAX_SECONDS).unwrap();
        assert!(r.true_peak_dbtp + r.play_gain_db <= -0.99, "{r:?}");
        assert!(r.result_lufs < -9.0, "{r:?}");
    }

    #[test]
    fn rubbish_silence_and_length_are_refused_by_name() {
        assert_eq!(
            process(b"", Target::default(), 10.0).unwrap_err(),
            MediaError::Empty
        );
        assert!(matches!(
            process(b"<html>not audio</html>", Target::default(), 10.0).unwrap_err(),
            MediaError::Unsupported(_)
        ));
        let silent = wav_sine(48_000, 2, 1.0, 1000.0, 0.0);
        assert_eq!(
            process(&silent, Target::default(), 10.0).unwrap_err(),
            MediaError::Silent
        );
        let long = wav_sine(8_000, 1, 3.0, 440.0, 0.5);
        assert_eq!(
            process(&long, Target::default(), 2.0).unwrap_err(),
            MediaError::TooLong(2.0)
        );
        // A truncated WAV is refused, not a crash.
        let wav = wav_sine(48_000, 2, 1.0, 1000.0, 0.5);
        assert!(process(&wav[..30], Target::default(), 10.0).is_err());
    }

    #[test]
    fn a_five_one_opus_is_downmixed_to_stereo() {
        // The demo pack's drum roll is 5.1 (mapping family 1).
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/carts-demo/drumroll-1.opus"
        );
        let (pcm, fmt) = decode(&std::fs::read(path).unwrap(), DEFAULT_MAX_SECONDS).unwrap();
        assert_eq!((fmt.as_str(), pcm.channels), ("ogg/opus", 2));
        assert!((pcm.seconds() - 3.82).abs() < 0.05, "{}", pcm.seconds());
        let (l, r) = measure(&pcm).unwrap();
        assert!(l > -30.0 && r < 0.0, "{l} {r}");
        // A centre-only frame lands in both sides at -3 dB.
        let m = downmix_vorbis(&[0.0, 1.0, 0.0, 0.0, 0.0, 0.5], 6);
        assert!((m[0] - std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-6 && m[0] == m[1]);
    }

    #[test]
    fn the_demo_packs_ogg_opus_decodes() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/carts-demo/ding-1.opus"
        );
        let bytes = std::fs::read(path).unwrap();
        assert!(is_ogg_opus(&bytes));
        let (_, r) = process(&bytes, Target::default(), DEFAULT_MAX_SECONDS).unwrap();
        assert_eq!(r.format, "ogg/opus");
        assert_eq!(r.sample_rate, 48_000);
        // CREDITS.md: 1.03 s, set to -16 LUFS with peaks under -1 dBTP.
        assert!((r.duration_ms as i64 - 1030).abs() <= 25, "{r:?}");
        assert!((r.integrated_lufs - -16.0).abs() < 1.0, "{r:?}");
        assert!(r.true_peak_dbtp < -0.5, "{r:?}");
    }
}
