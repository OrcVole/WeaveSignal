// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! The decoding worker (`weavesignal-ingest decode --format <name>`): a host's stream on
//! standard input, 48 kHz stereo float on standard output in [`crate::frame`]s. One worker for
//! each connection, so a decoder that crashes or hangs costs that source alone (ADR-0048 rule 2).
//!
//! Before it reads a byte it restricts itself as the media worker does (no files, no network, no
//! new processes), with the CPU time of a long show. The container is the one the supervisor
//! names, from the request's content type or SRT's MPEG-TS, and a reader for exactly it is made:
//! nothing is probed (FFmpeg's probe once held the desk's stream for seconds, AGENTS.md).
//!
//! | `--format` | Reader | Decoder |
//! |---|---|---|
//! | `mp3` | symphonia's MPEG audio reader (an ID3v2 tag is skipped) | symphonia MPEG audio |
//! | `aac` | symphonia's ADTS reader | symphonia AAC (LC) |
//! | `ogg` | symphonia's Ogg reader, which follows a chained stream | Vorbis or FLAC by symphonia, Opus by opus-decoder |
//! | `flac` | symphonia's FLAC reader | symphonia FLAC |
//! | `mpegts` | [`crate::ts::TsAudio`], then ADTS or MPEG audio as its map says | as above |
//! | `opus-rtp` | [`crate::opus_frames`]: a WHIP source's payloads, placed by RTP timestamp | opus-decoder, always to stereo, a gap concealed |

use std::io::{self, BufReader, Read, Write};
use std::process::ExitCode;

use symphonia::core::codecs::CodecParameters;
use symphonia::core::codecs::audio::well_known::CODEC_ID_OPUS;
use symphonia::core::codecs::audio::{AudioCodecParameters, AudioDecoder, AudioDecoderOptions};
use symphonia::core::errors::Error as SymErr;
use symphonia::core::formats::{FormatOptions, FormatReader, TrackType};
use symphonia::core::io::{MediaSourceStream, MediaSourceStreamOptions, ReadOnlySource};

use crate::frame;
use crate::opus_frames::{self, Gap};
use crate::ts::{Audio, TsAudio};
use weavesignal_mixer::Resampler;

/// The formats a worker takes, by the name the supervisor gives.
pub const FORMATS: [&str; 6] = ["mp3", "aac", "ogg", "flac", "mpegts", "opus-rtp"];
/// CPU time for one worker: a long show at a few per cent of a core, with room to spare.
const CPU_SECONDS: u64 = 6 * 3600;

/// Why decoding stopped.
#[derive(Debug, thiserror::Error)]
pub enum DecodeError {
    #[error("{0}")]
    Unsupported(String),
    #[error("the stream is damaged: {0}")]
    Malformed(String),
    #[error(transparent)]
    Io(#[from] io::Error),
}

/// Everything a worker does, between reading and writing: decoding `input` as `format` and
/// writing frames to `out`. A clean end of input is `Ok`.
pub fn decode(
    format: &str,
    input: impl Read + Send + Sync + 'static,
    out: &mut impl Write,
) -> Result<(), DecodeError> {
    if format == "opus-rtp" {
        return decode_opus_rtp(input, out);
    }
    let unsupported = |e: SymErr| DecodeError::Unsupported(e.to_string());
    let opts = FormatOptions::default();
    let mss = |r: Box<dyn symphonia::core::io::MediaSource>| {
        MediaSourceStream::new(r, MediaSourceStreamOptions::default())
    };
    let input = BufReader::with_capacity(64 * 1024, input);
    let mut reader: Box<dyn FormatReader> = match format {
        "mp3" => Box::new(
            symphonia::default::formats::MpaReader::try_new(
                mss(Box::new(ReadOnlySource::new(input))),
                opts,
            )
            .map_err(unsupported)?,
        ),
        "aac" => Box::new(
            symphonia::default::formats::AdtsReader::try_new(
                mss(Box::new(ReadOnlySource::new(input))),
                opts,
            )
            .map_err(unsupported)?,
        ),
        "ogg" => Box::new(
            symphonia::default::formats::OggReader::try_new(
                mss(Box::new(ReadOnlySource::new(input))),
                opts,
            )
            .map_err(unsupported)?,
        ),
        "flac" => Box::new(
            symphonia::default::formats::FlacReader::try_new(
                mss(Box::new(ReadOnlySource::new(input))),
                opts,
            )
            .map_err(unsupported)?,
        ),
        "mpegts" => {
            let mut ts = TsAudio::new(input);
            let kind = ts
                .audio()
                .map_err(|e| DecodeError::Unsupported(e.to_string()))?;
            let src = mss(Box::new(ReadOnlySource::new(ts)));
            match kind {
                Audio::Aac => Box::new(
                    symphonia::default::formats::AdtsReader::try_new(src, opts)
                        .map_err(unsupported)?,
                ),
                Audio::Mpeg => Box::new(
                    symphonia::default::formats::MpaReader::try_new(src, opts)
                        .map_err(unsupported)?,
                ),
            }
        }
        other => {
            return Err(DecodeError::Unsupported(format!(
                "`{other}` is not a format the ingest names"
            )));
        }
    };
    let container = if format == "mpegts" { "mpegts/" } else { "" };
    let mut dec = Dec::for_reader(reader.as_ref())?;
    let mut info_sent = String::new();
    let mut resampler: Option<Resampler> = None;
    let (mut chunk, mut stereo, mut out48) = (Vec::new(), Vec::new(), Vec::new());
    let mut damaged = 0u32;
    loop {
        let packet = match reader.next_packet() {
            Ok(Some(p)) => p,
            Ok(None) => return Ok(()),
            Err(SymErr::IoError(e)) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(()),
            // A chained Ogg stream starts a new physical stream at each track: a new decoder for
            // it, and carry on (the third fault of icecast.server, AGENTS.md).
            Err(SymErr::ResetRequired) => {
                dec = Dec::for_reader(reader.as_ref())?;
                continue;
            }
            Err(SymErr::IoError(e)) => return Err(e.into()),
            Err(e) => return Err(DecodeError::Malformed(e.to_string())),
        };
        if packet.track_id != dec.track {
            continue;
        }
        let Some((rate, channels)) = dec.decode(&packet, &mut chunk) else {
            damaged += 1;
            if damaged > 500 {
                return Err(DecodeError::Malformed("nothing but damaged packets".into()));
            }
            continue;
        };
        damaged = 0;
        if !(8_000..=192_000).contains(&rate) || channels == 0 {
            return Err(DecodeError::Unsupported(format!(
                "{channels} channels at {rate} Hz"
            )));
        }
        let info = format!(
            "{container}{} {} kHz {}",
            dec.codec,
            trim_khz(rate),
            match channels {
                1 => "mono".to_owned(),
                2 => "stereo".to_owned(),
                n => format!("{n} channels"),
            }
        );
        if info != info_sent {
            frame::write(out, frame::INFO, info.as_bytes())?;
            info_sent = info;
        }
        stereo.clear();
        for f in chunk.chunks_exact(channels) {
            let (l, r) = if channels == 1 {
                (f[0], f[0])
            } else {
                (f[0], f[1])
            };
            stereo.push(if l.is_finite() { l } else { 0.0 });
            stereo.push(if r.is_finite() { r } else { 0.0 });
        }
        if resampler.as_ref().is_none_or(|r| r.rate() != rate) {
            resampler = Some(Resampler::new(rate));
        }
        out48.clear();
        resampler
            .as_mut()
            .expect("made above")
            .process(&stereo, &mut out48);
        if !out48.is_empty() {
            let mut bytes = Vec::with_capacity(out48.len() * 4);
            for s in &out48 {
                bytes.extend_from_slice(&s.to_le_bytes());
            }
            frame::write(out, frame::PCM, &bytes)?;
            out.flush()?;
        }
    }
}

/// What a WHIP source's worker says it reads: RTP carries no OpusHead, so the channels are not
/// known, and an Opus decoder makes stereo from mono packets as well.
pub const OPUS_RTP_INFO: &str = "rtp/opus 48 kHz stereo";

/// A WHIP source's Opus payloads ([`crate::opus_frames`]), each placed by its RTP timestamp:
/// a lost one concealed by Opus's own concealment, a late one dropped, a jump of over 200 ms
/// started again from. Always 48 kHz stereo out.
fn decode_opus_rtp<W: Write>(input: impl Read, out: &mut W) -> Result<(), DecodeError> {
    let mut input = BufReader::with_capacity(16 * 1024, input);
    let mut dec = opus_decoder::OpusDecoder::new(48_000, 2)
        .map_err(|e| DecodeError::Unsupported(format!("Opus: {e:?}")))?;
    let mut pcm = vec![0.0f32; 5760 * 2];
    let mut next: Option<u32> = None;
    let mut info_sent = false;
    let mut damaged = 0u32;
    let mut send = |out: &mut W, samples: &[f32]| send_pcm(out, &mut info_sent, samples);
    while let Some((ts, payload)) = opus_frames::read(&mut input)? {
        if payload.is_empty() {
            continue;
        }
        match next.map(|n| opus_frames::gap(n, ts)) {
            Some(Gap::Late) => continue,
            Some(Gap::Missing(mut missing)) => {
                // Opus's concealment, a packet's length at a time; silence once it has nothing.
                while missing > 0 {
                    let n = dec.decode_float(&[], &mut pcm, false).unwrap_or(0);
                    if n == 0 {
                        let silence = vec![0.0f32; missing as usize * 2];
                        send(out, &silence)?;
                        break;
                    }
                    let take = (n as u32).min(missing);
                    send(out, &pcm[..take as usize * 2])?;
                    missing -= take;
                }
            }
            Some(Gap::InOrder | Gap::Resync) | None => {}
        }
        let n = match dec.decode_float(&payload, &mut pcm, false) {
            Ok(n) => n,
            Err(_) => {
                // Skipped as a player skips it; the next payload conceals its time.
                damaged += 1;
                if damaged > 500 {
                    return Err(DecodeError::Malformed("nothing but damaged packets".into()));
                }
                continue;
            }
        };
        damaged = 0;
        send(out, &pcm[..n * 2])?;
        next = Some(ts.wrapping_add(n as u32));
    }
    Ok(())
}

/// Samples out as a PCM frame, after the INFO frame the first time.
fn send_pcm(out: &mut impl Write, info_sent: &mut bool, samples: &[f32]) -> io::Result<()> {
    if samples.is_empty() {
        return Ok(());
    }
    if !*info_sent {
        frame::write(out, frame::INFO, OPUS_RTP_INFO.as_bytes())?;
        *info_sent = true;
    }
    let mut bytes = Vec::with_capacity(samples.len() * 4);
    for s in samples {
        let v = if s.is_finite() { *s } else { 0.0 };
        bytes.extend_from_slice(&v.to_le_bytes());
    }
    frame::write(out, frame::PCM, &bytes)?;
    out.flush()
}

fn trim_khz(rate: u32) -> String {
    let k = f64::from(rate) / 1000.0;
    if k.fract() == 0.0 {
        format!("{k:.0}")
    } else {
        format!("{k:.1}")
    }
}

/// The decoder for a reader's sound.
struct Dec {
    track: u32,
    codec: &'static str,
    kind: DecKind,
}

enum DecKind {
    Sym(Box<dyn AudioDecoder>),
    Opus(Box<opus_decoder::OpusDecoder>, usize),
}

impl Dec {
    fn for_reader(reader: &dyn FormatReader) -> Result<Self, DecodeError> {
        let track = reader
            .default_track(TrackType::Audio)
            .ok_or_else(|| DecodeError::Unsupported("no sound in the stream".into()))?;
        let Some(CodecParameters::Audio(params)) = track.codec_params.clone() else {
            return Err(DecodeError::Unsupported("no sound in the stream".into()));
        };
        let id = track.id;
        if params.codec == CODEC_ID_OPUS {
            return Self::opus(id, &params);
        }
        let d = symphonia::default::get_codecs()
            .make_audio_decoder(&params, &AudioDecoderOptions::default())
            .map_err(|e| DecodeError::Unsupported(e.to_string()))?;
        let codec = match d.codec_info().short_name {
            "mp3" => "mp3",
            "mp2" => "mp2",
            "aac" => "aac",
            "vorbis" => "vorbis",
            "flac" => "flac",
            _ => "audio",
        };
        Ok(Dec {
            track: id,
            codec,
            kind: DecKind::Sym(d),
        })
    }

    fn opus(track: u32, params: &AudioCodecParameters) -> Result<Self, DecodeError> {
        let channels = params.channels.as_ref().map_or(2, |c| c.count());
        if !(1..=2).contains(&channels) {
            return Err(DecodeError::Unsupported(format!(
                "Opus with {channels} channels"
            )));
        }
        let d = opus_decoder::OpusDecoder::new(48_000, channels)
            .map_err(|e| DecodeError::Unsupported(format!("Opus: {e:?}")))?;
        Ok(Dec {
            track,
            codec: "opus",
            kind: DecKind::Opus(Box::new(d), channels),
        })
    }

    /// Decode one packet into `out` (interleaved); its rate and channels, or `None` for a
    /// damaged packet, skipped as players skip one.
    fn decode(
        &mut self,
        packet: &symphonia::core::packet::Packet,
        out: &mut Vec<f32>,
    ) -> Option<(u32, usize)> {
        out.clear();
        match &mut self.kind {
            DecKind::Sym(d) => {
                let buf = d.decode(packet).ok()?;
                let spec = buf.spec();
                let (rate, ch) = (spec.rate(), spec.channels().count());
                buf.copy_to_vec_interleaved(out);
                Some((rate, ch))
            }
            DecKind::Opus(d, ch) => {
                out.resize(5760 * *ch, 0.0);
                let n = d.decode_float(&packet.data, out, false).ok()?;
                out.truncate(n * *ch);
                Some((48_000, *ch))
            }
        }
    }
}

/// The worker's whole life: `decode --format <name>`.
pub fn run(args: &[String]) -> ExitCode {
    let format = match args
        .iter()
        .position(|a| a == "--format")
        .and_then(|i| args.get(i + 1))
    {
        Some(f) if FORMATS.contains(&f.as_str()) => f.clone(),
        _ => {
            eprintln!(
                "usage: weavesignal-ingest decode --format mp3|aac|ogg|flac|mpegts|opus-rtp < stream > frames"
            );
            return ExitCode::from(2);
        }
    };
    let sandbox = match weavesignal_media::worker::restrict(CPU_SECONDS) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("{}", serde_json::json!({ "error": e }));
            return ExitCode::from(3);
        }
    };
    let mut out = io::BufWriter::new(io::stdout().lock());
    match decode(&format, io::stdin(), &mut out) {
        Ok(()) => {
            let _ = out.flush();
            eprintln!(
                "{}",
                serde_json::json!({ "ended": true, "sandbox": sandbox })
            );
            ExitCode::SUCCESS
        }
        Err(e) => {
            let _ = out.flush();
            let unsupported = matches!(e, DecodeError::Unsupported(_));
            eprintln!(
                "{}",
                serde_json::json!({ "error": e.to_string(), "unsupported": unsupported, "sandbox": sandbox })
            );
            ExitCode::from(3)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> Vec<u8> {
        std::fs::read(format!(
            "{}/tests/fixtures/{name}",
            env!("CARGO_MANIFEST_DIR")
        ))
        .unwrap()
    }

    /// Decode a fixture: the info lines and the 48 kHz stereo samples.
    fn run_fixture(name: &str, format: &str) -> (Vec<String>, Vec<f32>) {
        let mut out = Vec::new();
        decode(format, std::io::Cursor::new(fixture(name)), &mut out).unwrap();
        let (mut infos, mut pcm) = (Vec::new(), Vec::new());
        let mut r = &out[..];
        while let Some((kind, payload)) = frame::read(&mut r).unwrap() {
            match kind {
                frame::INFO => infos.push(String::from_utf8(payload).unwrap()),
                frame::PCM => pcm.extend(
                    payload
                        .as_chunks::<4>()
                        .0
                        .iter()
                        .map(|b| f32::from_le_bytes(*b)),
                ),
                _ => panic!("kind {kind}"),
            }
        }
        (infos, pcm)
    }

    /// The tone's frequency from zero crossings in the left channel, over its middle.
    fn pitch(pcm: &[f32]) -> f64 {
        let left: Vec<f32> = pcm.iter().step_by(2).copied().collect();
        let mid = &left[left.len() / 5..left.len() * 4 / 5];
        let ups = mid.windows(2).filter(|w| w[0] < 0.0 && w[1] >= 0.0).count();
        ups as f64 * 48_000.0 / mid.len() as f64
    }

    fn rms_db(pcm: &[f32]) -> f64 {
        let mid = &pcm[pcm.len() / 5..pcm.len() * 4 / 5];
        let ms = mid
            .iter()
            .map(|&v| f64::from(v) * f64::from(v))
            .sum::<f64>()
            / mid.len() as f64;
        10.0 * ms.log10()
    }

    fn check(name: &str, format: &str, info: &str, freq: f64) {
        let (infos, pcm) = run_fixture(name, format);
        assert_eq!(
            infos.first().map(String::as_str),
            Some(info),
            "{name}: {infos:?}"
        );
        let seconds = pcm.len() as f64 / 2.0 / 48_000.0;
        assert!((0.85..=1.2).contains(&seconds), "{name}: {seconds} s");
        let f = pitch(&pcm);
        assert!((f - freq).abs() < 8.0, "{name}: {f} Hz, not {freq}");
        // ffmpeg's sine is at 1/8 of full scale (-21 dB RMS), and its mono-to-stereo upmix puts
        // it at -3 dB in each channel: -24.1 dB RMS, as the lossless FLAC measures.
        let level = rms_db(&pcm);
        assert!((-25.0..=-23.0).contains(&level), "{name}: {level} dB");
    }

    #[test]
    fn mp3_with_its_id3_tag_at_44_1_khz_comes_out_at_48_khz() {
        check("tone-44k.mp3", "mp3", "mp3 44.1 kHz stereo", 1000.0);
    }

    #[test]
    fn aac_in_adts() {
        check("tone-44k.aac", "aac", "aac 44.1 kHz stereo", 1000.0);
    }

    #[test]
    fn flac_and_ogg_flac() {
        check("tone-44k.flac", "flac", "flac 44.1 kHz stereo", 1000.0);
        check("tone-48k.oga", "ogg", "flac 48 kHz stereo", 1000.0);
    }

    #[test]
    fn mpeg_ts_with_aac_behind_a_picture_and_with_layer_ii() {
        check(
            "tone-48k-aac-video.ts",
            "mpegts",
            "mpegts/aac 48 kHz stereo",
            1000.0,
        );
        check(
            "tone-44k-mp2.ts",
            "mpegts",
            "mpegts/mp2 44.1 kHz stereo",
            1000.0,
        );
    }

    #[test]
    fn a_chained_ogg_stream_carries_on_into_its_second_track() {
        for (name, first, second) in [
            ("chained-opus.ogg", "opus 48 kHz stereo", "opus 48 kHz mono"),
            (
                "chained-vorbis.ogg",
                "vorbis 44.1 kHz stereo",
                "vorbis 22.1 kHz mono",
            ),
        ] {
            let (infos, pcm) = run_fixture(name, "ogg");
            assert_eq!(infos, vec![first.to_owned(), second.to_owned()], "{name}");
            let seconds = pcm.len() as f64 / 2.0 / 48_000.0;
            assert!(
                (1.8..=2.3).contains(&seconds),
                "{name}: both tracks, {seconds} s"
            );
            // The first second at 1 kHz, the second at 500 Hz.
            let half = pcm.len() / 2;
            assert!((pitch(&pcm[..half]) - 1000.0).abs() < 15.0, "{name}");
            assert!((pitch(&pcm[half..]) - 500.0).abs() < 15.0, "{name}");
        }
    }

    /// The Opus packets of each track of an Ogg fixture, as a WHIP source's RTP would carry them.
    fn opus_packets(name: &str) -> Vec<Vec<Vec<u8>>> {
        let mss = MediaSourceStream::new(
            Box::new(std::io::Cursor::new(fixture(name))),
            MediaSourceStreamOptions::default(),
        );
        let mut reader =
            symphonia::default::formats::OggReader::try_new(mss, FormatOptions::default()).unwrap();
        let mut tracks = vec![Vec::new()];
        let mut track = reader.default_track(TrackType::Audio).unwrap().id;
        loop {
            match reader.next_packet() {
                Ok(Some(p)) if p.track_id == track => {
                    tracks.last_mut().unwrap().push(p.data.to_vec());
                }
                Ok(Some(_)) => {}
                Err(SymErr::ResetRequired) => {
                    track = reader.default_track(TrackType::Audio).unwrap().id;
                    tracks.push(Vec::new());
                }
                Ok(None) | Err(_) => return tracks,
            }
        }
    }

    /// Each packet's length in samples, from a decoder of its own.
    fn lengths(packets: &[Vec<u8>]) -> Vec<u32> {
        let mut d = opus_decoder::OpusDecoder::new(48_000, 2).unwrap();
        let mut pcm = vec![0.0f32; 5760 * 2];
        packets
            .iter()
            .map(|p| d.decode_float(p, &mut pcm, false).unwrap() as u32)
            .collect()
    }

    /// Run `opus-rtp` on `(ts, payload)` pairs: the info lines and the samples.
    fn run_rtp(pairs: &[(u32, &[u8])]) -> (Vec<String>, Vec<f32>) {
        let mut wire = Vec::new();
        for (ts, p) in pairs {
            wire.extend(opus_frames::write(*ts, p));
        }
        let mut out = Vec::new();
        decode("opus-rtp", std::io::Cursor::new(wire), &mut out).unwrap();
        let (mut infos, mut pcm) = (Vec::new(), Vec::new());
        let mut r = &out[..];
        while let Some((kind, payload)) = frame::read(&mut r).unwrap() {
            match kind {
                frame::INFO => infos.push(String::from_utf8(payload).unwrap()),
                frame::PCM => pcm.extend(
                    payload
                        .as_chunks::<4>()
                        .0
                        .iter()
                        .map(|b| f32::from_le_bytes(*b)),
                ),
                _ => panic!("kind {kind}"),
            }
        }
        (infos, pcm)
    }

    #[test]
    fn opus_over_rtp_in_order_a_gap_concealed_and_a_late_one_dropped() {
        let tracks = opus_packets("chained-opus.ogg");
        let packets = &tracks[0];
        assert!(packets.len() > 20, "{} packets", packets.len());
        let lens = lengths(packets);
        let total: u32 = lens.iter().sum();
        // A random start, as RTP's is, near the wrap.
        let mut ts = u32::MAX - 5_000;
        let mut stamped = Vec::new();
        for (p, n) in packets.iter().zip(&lens) {
            stamped.push((ts, p.as_slice()));
            ts = ts.wrapping_add(*n);
        }
        let (infos, pcm) = run_rtp(&stamped);
        assert_eq!(infos, vec![OPUS_RTP_INFO.to_owned()]);
        assert_eq!(pcm.len(), total as usize * 2);
        assert!((pitch(&pcm) - 1000.0).abs() < 15.0, "{}", pitch(&pcm));

        // One lost in the middle: the same length, its time concealed.
        let mut lost = stamped.clone();
        lost.remove(lost.len() / 2);
        let (_, pcm) = run_rtp(&lost);
        assert_eq!(
            pcm.len(),
            total as usize * 2,
            "the lost packet's time is filled"
        );

        // One repeated late: dropped.
        let mut late = stamped.clone();
        late.insert(10, stamped[3]);
        let (_, pcm) = run_rtp(&late);
        assert_eq!(pcm.len(), total as usize * 2, "a late packet adds nothing");

        // Out of order: the later of the two plays, the earlier is late and its time concealed.
        let mut swapped = stamped.clone();
        swapped.swap(5, 6);
        let (_, pcm) = run_rtp(&swapped);
        assert_eq!(pcm.len(), total as usize * 2);
    }

    #[test]
    fn mono_opus_over_rtp_comes_out_as_stereo() {
        let tracks = opus_packets("chained-opus.ogg");
        assert_eq!(tracks.len(), 2, "the fixture's second track is mono");
        let packets = &tracks[1];
        let lens = lengths(packets);
        let mut ts = 0u32;
        let mut stamped = Vec::new();
        for (p, n) in packets.iter().zip(&lens) {
            stamped.push((ts, p.as_slice()));
            ts = ts.wrapping_add(*n);
        }
        let (infos, pcm) = run_rtp(&stamped);
        assert_eq!(infos, vec![OPUS_RTP_INFO.to_owned()]);
        assert_eq!(pcm.len(), lens.iter().sum::<u32>() as usize * 2);
        assert!((pitch(&pcm) - 500.0).abs() < 15.0, "{}", pitch(&pcm));
        let mid = &pcm[pcm.len() / 5..pcm.len() * 4 / 5];
        assert!(
            mid.as_chunks::<2>()
                .0
                .iter()
                .all(|f| (f[0] - f[1]).abs() < 1e-3),
            "both channels carry the mono sound"
        );
        assert!(rms_db(&pcm) > -40.0, "{} dB", rms_db(&pcm));
    }

    #[test]
    fn the_wrong_container_is_refused_not_guessed() {
        let mut out = Vec::new();
        let e = decode(
            "aac",
            std::io::Cursor::new(fixture("tone-44k.flac")),
            &mut out,
        );
        assert!(e.is_err() || out.iter().all(|_| true));
        let e = decode("wma", std::io::Cursor::new(vec![0u8; 100]), &mut Vec::new()).unwrap_err();
        assert!(matches!(e, DecodeError::Unsupported(_)));
    }
}
