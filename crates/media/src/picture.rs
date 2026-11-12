// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! A picture posted in the chat (ADR-0258, ADR-0262): decode an untrusted PNG, JPEG or WebP,
//! flatten any transparency onto white, make it a size that keeps text legible, and write it
//! again with nothing but its pixels, with a small copy for the message's thumbnail.
//!
//! **The size.** A long side of [`DEFAULT_LONG_SIDE`] (1,600) pixels, never enlarged. A long
//! strip (more than twice as long as it is wide: a phone's screenshot of a whole page) would be
//! illegible at that, so it keeps [`STRIP_SHORT_SIDE`] (800) across instead, up to
//! [`STRIP_LONG_SIDE`] (4,096) long.
//!
//! **The format.** Text must stay sharp, so a picture is written as a lossless PNG first. A
//! screenshot of a document is a few hundred kilobytes that way; a photograph is megabytes, so
//! when the PNG is larger than [`PNG_MAX`] a JPEG at quality 90 with full colour resolution (no
//! chroma subsampling, which blurs coloured text) is written too, and the smaller kept. The
//! thumbnail follows the same rule at its own size.
//!
//! The decoders are the still's for PNG and JPEG ([`crate::still`]) and `image-webp` for WebP,
//! all memory-safe Rust, with the size read from the header before a pixel is allocated. An
//! animated WebP is refused: a chat picture is one still.

use std::io::Cursor;

use serde::{Deserialize, Serialize};

use crate::MediaError;
use crate::still::{Rgb, check_size, decode_jpeg, decode_png, encode_png, over_background, shrink};

/// A picture's long side unless the caller says otherwise.
pub const DEFAULT_LONG_SIDE: usize = 1600;
/// A long strip keeps this many pixels across ...
pub const STRIP_SHORT_SIDE: usize = 800;
/// ... up to this long.
pub const STRIP_LONG_SIDE: usize = 4096;
/// The thumbnail fits in a square this size (twice what a message shows, for sharp screens).
pub const DEFAULT_THUMB_SIDE: usize = 480;
/// The shortest side of a source picture.
pub const MIN_SIDE: usize = 16;
/// A PNG larger than this is tried as a JPEG too.
pub const PNG_MAX: usize = 1024 * 1024;
/// The thumbnail's PNG larger than this is tried as a JPEG too.
pub const THUMB_PNG_MAX: usize = 96 * 1024;

/// What transparent pixels take: white, as a document or a diagram is drawn on.
const WHITE: [u8; 3] = [255, 255, 255];

/// One written picture.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Written {
    /// `png` or `jpeg`.
    pub format: String,
    pub width: u32,
    pub height: u32,
    pub bytes: usize,
}

/// What the worker reports for a picture. Its standard output is the picture's bytes followed by
/// the thumbnail's; `full.bytes` says where one ends.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PictureReport {
    /// `png`, `jpeg` or `webp`, the format it was read as.
    pub source_format: String,
    pub source_width: u32,
    pub source_height: u32,
    pub full: Written,
    pub thumb: Written,
}

fn decode_webp(bytes: &[u8]) -> Result<Rgb, MediaError> {
    let bad =
        |e: image_webp::DecodingError| MediaError::Malformed(format!("the WebP is damaged: {e}"));
    let mut d = image_webp::WebPDecoder::new(Cursor::new(bytes)).map_err(bad)?;
    let (w, h) = d.dimensions();
    let (w, h) = (w as usize, h as usize);
    check_size(w, h)?;
    if d.is_animated() {
        return Err(MediaError::Picture(
            "the WebP is animated; a picture in the chat is one still".into(),
        ));
    }
    d.set_memory_limit(crate::still::MAX_PIXELS * 4 + 1024 * 1024);
    let alpha = d.has_alpha();
    let size = d
        .output_buffer_size()
        .ok_or_else(|| MediaError::Malformed("the WebP's size does not fit this machine".into()))?;
    if size != w * h * if alpha { 4 } else { 3 } {
        return Err(MediaError::Malformed(
            "the WebP's size does not add up".into(),
        ));
    }
    let mut buf = vec![0u8; size];
    d.read_image(&mut buf).map_err(bad)?;
    let data = if alpha {
        let mut out = Vec::with_capacity(w * h * 3);
        for p in buf.as_chunks::<4>().0 {
            out.extend(
                WHITE
                    .iter()
                    .zip(p)
                    .map(|(bg, c)| over_background(*c, p[3], *bg)),
            );
        }
        out
    } else {
        buf
    };
    Ok(Rgb {
        width: w,
        height: h,
        data,
    })
}

/// Read a PNG, JPEG or WebP: the format it was read as, and the pixels.
fn decode_picture(bytes: &[u8]) -> Result<(&'static str, Rgb), MediaError> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Ok(("png", decode_png(bytes, WHITE)?))
    } else if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        Ok(("jpeg", decode_jpeg(bytes)?))
    } else if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Ok(("webp", decode_webp(bytes)?))
    } else {
        Err(MediaError::NotAnImage)
    }
}

/// The size a picture is written at: see the module's words. Never enlarged.
pub fn target_size(width: usize, height: usize, long_side: usize) -> (usize, usize) {
    let (long, short) = (width.max(height), width.min(height).max(1));
    let (new_long, new_short) = if long > 2 * short {
        // A strip: keep it readable across, within the longest side allowed.
        let scale_short = STRIP_SHORT_SIDE.min(short);
        let mut l = long * scale_short / short;
        let mut s = scale_short;
        if l > STRIP_LONG_SIDE {
            s = (s * STRIP_LONG_SIDE / l).max(1);
            l = STRIP_LONG_SIDE;
        }
        (l.min(long), s)
    } else if long <= long_side {
        (long, short)
    } else {
        (long_side, (short * long_side / long).max(1))
    };
    if width >= height {
        (new_long, new_short)
    } else {
        (new_short, new_long)
    }
}

/// Fit inside a square of `side`, never enlarging.
fn fit_square(width: usize, height: usize, side: usize) -> (usize, usize) {
    let long = width.max(height);
    if long <= side {
        return (width, height);
    }
    ((width * side / long).max(1), (height * side / long).max(1))
}

fn encode_jpeg(rgb: &Rgb, quality: u8) -> Result<Vec<u8>, MediaError> {
    let (w, h) = (
        u16::try_from(rgb.width).map_err(|_| MediaError::Encode("too wide for a JPEG".into()))?,
        u16::try_from(rgb.height).map_err(|_| MediaError::Encode("too tall for a JPEG".into()))?,
    );
    let mut out = Vec::new();
    let mut e = jpeg_encoder::Encoder::new(&mut out, quality);
    // Full colour resolution: chroma subsampling smears coloured text.
    e.set_sampling_factor(jpeg_encoder::SamplingFactor::R_4_4_4);
    e.set_optimized_huffman_tables(true);
    e.encode(&rgb.data, w, h, jpeg_encoder::ColorType::Rgb)
        .map_err(|e| MediaError::Encode(e.to_string()))?;
    Ok(out)
}

/// PNG, unless that is over `png_max` and a JPEG is smaller.
fn write(rgb: &Rgb, png_max: usize, quality: u8) -> Result<(Vec<u8>, Written), MediaError> {
    let png = encode_png(rgb)?;
    let (format, bytes) = if png.len() > png_max {
        let jpeg = encode_jpeg(rgb, quality)?;
        if jpeg.len() < png.len() {
            ("jpeg", jpeg)
        } else {
            ("png", png)
        }
    } else {
        ("png", png)
    };
    let w = Written {
        format: format.into(),
        width: rgb.width as u32,
        height: rgb.height as u32,
        bytes: bytes.len(),
    };
    Ok((bytes, w))
}

/// The whole job for a chat picture: the picture and its thumbnail, and the report.
pub fn process_picture(
    bytes: &[u8],
    long_side: usize,
    thumb_side: usize,
) -> Result<(Vec<u8>, Vec<u8>, PictureReport), MediaError> {
    if bytes.is_empty() {
        return Err(MediaError::Empty);
    }
    let (format, rgb) = decode_picture(bytes)?;
    check_size(rgb.width, rgb.height)?;
    if rgb.width < MIN_SIDE || rgb.height < MIN_SIDE {
        return Err(MediaError::ImageTooSmall {
            width: rgb.width,
            height: rgb.height,
            min_side: MIN_SIDE,
        });
    }
    let (w, h) = target_size(rgb.width, rgb.height, long_side.max(64));
    let full_rgb = shrink(&rgb, w, h);
    let (full, full_w) = write(&full_rgb, PNG_MAX, 90)?;
    let (tw, th) = fit_square(w, h, thumb_side.max(32));
    let thumb_rgb = shrink(&full_rgb, tw, th);
    let (thumb, thumb_w) = write(&thumb_rgb, THUMB_PNG_MAX, 85)?;
    let report = PictureReport {
        source_format: format.into(),
        source_width: rgb.width as u32,
        source_height: rgb.height as u32,
        full: full_w,
        thumb: thumb_w,
    };
    Ok((full, thumb, report))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::still::decode;

    fn png_rgb(width: u32, height: u32, data: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut e = png::Encoder::new(&mut out, width, height);
        e.set_color(png::ColorType::Rgb);
        e.set_depth(png::BitDepth::Eight);
        e.add_text_chunk("Location".into(), "55.9533 N, 3.1883 W".into())
            .unwrap();
        let mut w = e.write_header().unwrap();
        w.write_image_data(data).unwrap();
        w.finish().unwrap();
        out
    }

    /// Black text-like strokes on white: what a screenshot of a document is.
    fn page(width: usize, height: usize) -> Vec<u8> {
        let mut d = vec![255u8; width * height * 3];
        for y in 0..height {
            for x in 0..width {
                if (y / 6) % 3 == 0 && (x / 3) % 4 != 0 {
                    let i = (y * width + x) * 3;
                    d[i..i + 3].copy_from_slice(&[20, 20, 30]);
                }
            }
        }
        d
    }

    /// Noise, as a photograph is to a compressor.
    fn photo(width: usize, height: usize) -> Vec<u8> {
        let mut x: u32 = 0x9e37_79b9;
        (0..width * height * 3)
            .map(|i| {
                x ^= x << 13;
                x ^= x >> 17;
                x ^= x << 5;
                // Smooth enough to look like something, noisy enough to defeat PNG.
                ((i / 3 % width) as u32 / 8 + (x & 63)) as u8
            })
            .collect()
    }

    #[test]
    fn sizes_keep_text_legible() {
        assert_eq!(target_size(2560, 1440, 1600), (1600, 900));
        assert_eq!(target_size(1440, 2560, 1600), (900, 1600));
        assert_eq!(target_size(800, 600, 1600), (800, 600), "never enlarged");
        // A phone's screenshot of a whole page: 800 across, not 1600 long and 288 across.
        assert_eq!(target_size(1080, 6000, 1600), (737, 4096));
        assert_eq!(target_size(1080, 3000, 1600), (800, 2222));
        // A narrow strip already under 800 across stays as it is.
        assert_eq!(target_size(600, 2000, 1600), (600, 2000));
    }

    #[test]
    fn a_screenshot_stays_a_lossless_png_without_its_metadata() {
        let (w, h) = (2400, 1350);
        let src = png_rgb(w as u32, h as u32, &page(w, h));
        assert!(src.windows(8).any(|x| x == b"Location"));
        let (full, thumb, r) = process_picture(&src, 1600, 480).unwrap();
        assert_eq!(r.full.format, "png");
        assert_eq!((r.full.width, r.full.height), (1600, 900));
        assert_eq!((r.thumb.width, r.thumb.height), (480, 270));
        assert_eq!(r.full.bytes, full.len());
        assert_eq!(r.thumb.bytes, thumb.len());
        assert!(!full.windows(8).any(|x| x == b"Location"));
        assert!(
            !full
                .windows(4)
                .any(|x| x == b"tEXt" || x == b"eXIf" || x == b"iCCP")
        );
        let (_, back) = decode(&full, WHITE).unwrap();
        assert_eq!((back.width, back.height), (1600, 900));
    }

    #[test]
    fn a_photograph_becomes_a_jpeg_at_full_colour_resolution() {
        let (w, h) = (1800, 1200);
        let src = png_rgb(w as u32, h as u32, &photo(w, h));
        let (full, _, r) = process_picture(&src, 1600, 480).unwrap();
        assert_eq!(r.full.format, "jpeg", "{r:?}");
        assert!(full.starts_with(&[0xff, 0xd8, 0xff]));
        // No EXIF (APP1) segment: the only application segment is the JFIF header.
        assert!(!full.windows(6).any(|x| x == b"Exif\0\0"));
        let (_, back) = decode(&full, WHITE).unwrap();
        assert_eq!((back.width, back.height), (1600, 1066));
    }

    #[test]
    fn a_webp_is_read_and_its_transparency_goes_white() {
        // A lossless WebP, 40 by 30, all transparent.
        let mut webp = Vec::new();
        image_webp::WebPEncoder::new(&mut webp)
            .encode(
                &vec![0u8; 40 * 30 * 4],
                40,
                30,
                image_webp::ColorType::Rgba8,
            )
            .unwrap();
        let (full, _, r) = process_picture(&webp, 1600, 480).unwrap();
        assert_eq!(r.source_format, "webp");
        let (_, back) = decode(&full, [0, 0, 0]).unwrap();
        assert_eq!(&back.data[..3], &WHITE);
    }

    #[test]
    fn what_is_not_a_picture_or_too_big_is_refused() {
        assert_eq!(
            process_picture(b"%PDF-1.7", 1600, 480).unwrap_err(),
            MediaError::NotAnImage
        );
        assert_eq!(
            process_picture(b"<svg xmlns='http://www.w3.org/2000/svg'/>", 1600, 480).unwrap_err(),
            MediaError::NotAnImage
        );
        let mut huge = Vec::new();
        let mut e = png::Encoder::new(&mut huge, 9000, 9000);
        e.set_color(png::ColorType::Grayscale);
        e.set_depth(png::BitDepth::One);
        e.write_header().unwrap();
        assert!(matches!(
            process_picture(&huge, 1600, 480),
            Err(MediaError::ImageTooBig { .. })
        ));
    }

    #[test]
    fn a_polyglot_keeps_only_its_pixels() {
        // A PNG with a page of script after its end: the rewritten picture has none of it.
        let mut src = png_rgb(64, 64, &page(64, 64));
        src.extend_from_slice(b"<html><script>alert(document.cookie)</script></html>");
        let (full, thumb, _) = process_picture(&src, 1600, 480).unwrap();
        for out in [&full, &thumb] {
            assert!(!out.windows(7).any(|x| x == b"<script"));
        }
    }
}
