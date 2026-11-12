// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! Stills for the video slate (ADR-0010): decode an untrusted PNG or JPEG, flatten any
//! transparency onto the slate's colour, make it fit the room the slate gives a picture, and
//! write a clean PNG: 8-bit RGB with no other chunks. The desk only ever reads the PNG this
//! writes, never the upload, as playout only ever reads the FLAC of `process`.
//!
//! Both decoders are memory-safe Rust (`png`, `zune-jpeg`), and the sizes are checked from the
//! header before a pixel is allocated, so a small file cannot ask for gigabytes.

use std::io::Cursor;

use serde::{Deserialize, Serialize};

use crate::MediaError;

/// The longest side of a source image, in pixels.
pub const MAX_SIDE: usize = 8192;
/// The most pixels in a source image (the 8-bit RGB copy of it is three times this).
pub const MAX_PIXELS: usize = 36_000_000;

/// The slate's background, which transparent pixels take (the desk's `SLATE_BG`).
const BACKGROUND: [u8; 3] = [0x15, 0x17, 0x1c];

const PNG_SIGNATURE: &[u8] = b"\x89PNG\r\n\x1a\n";

/// What the worker reports for a still.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StillReport {
    /// `png` or `jpeg`, the format it was read as.
    pub format: String,
    pub source_width: u32,
    pub source_height: u32,
    /// The picture written, never larger than asked and never enlarged.
    pub width: u32,
    pub height: u32,
    /// The size of the PNG written.
    pub bytes: usize,
}

/// 8-bit RGB, rows packed.
pub(crate) struct Rgb {
    pub(crate) width: usize,
    pub(crate) height: usize,
    pub(crate) data: Vec<u8>,
}

pub(crate) fn check_size(width: usize, height: usize) -> Result<(), MediaError> {
    if width == 0 || height == 0 {
        return Err(MediaError::Malformed("the picture is empty".into()));
    }
    if width > MAX_SIDE || height > MAX_SIDE || width * height > MAX_PIXELS {
        return Err(MediaError::ImageTooBig {
            width,
            height,
            max_side: MAX_SIDE,
            max_pixels: MAX_PIXELS,
        });
    }
    Ok(())
}

pub(crate) fn over_background(c: u8, a: u8, bg: u8) -> u8 {
    let (c, a, bg) = (u32::from(c), u32::from(a), u32::from(bg));
    ((c * a + bg * (255 - a) + 127) / 255) as u8
}

pub(crate) fn decode_png(bytes: &[u8], background: [u8; 3]) -> Result<Rgb, MediaError> {
    let bad = |e: png::DecodingError| MediaError::Malformed(format!("the PNG is damaged: {e}"));
    // The size is in the first chunk: read it ourselves, so nothing is allocated for a size
    // that is refused.
    if bytes.len() >= 24 && &bytes[12..16] == b"IHDR" {
        let be =
            |i: usize| u32::from_be_bytes([bytes[i], bytes[i + 1], bytes[i + 2], bytes[i + 3]]);
        check_size(be(16) as usize, be(20) as usize)?;
    }
    let mut decoder = png::Decoder::new(Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    decoder.set_limits(png::Limits {
        bytes: MAX_PIXELS * 4 + 1024 * 1024,
    });
    let mut reader = decoder.read_info().map_err(bad)?;
    let (w, h) = {
        let i = reader.info();
        (i.width as usize, i.height as usize)
    };
    check_size(w, h)?;
    let mut buf = vec![
        0u8;
        reader.output_buffer_size().ok_or_else(|| {
            MediaError::Malformed("the PNG's size does not fit this machine".into())
        })?
    ];
    let frame = reader.next_frame(&mut buf).map_err(bad)?;
    let (color, depth) = reader.output_color_type();
    if depth != png::BitDepth::Eight {
        return Err(MediaError::Malformed(
            "the PNG is not 8-bit after expanding".into(),
        ));
    }
    let src = &buf[..frame.buffer_size()];
    let samples = match color {
        png::ColorType::Rgb => 3,
        png::ColorType::Rgba => 4,
        png::ColorType::GrayscaleAlpha => 2,
        _ => 1,
    };
    if src.len() < w * h * samples {
        return Err(MediaError::Malformed("the PNG has too few pixels".into()));
    }
    let mut data = Vec::with_capacity(w * h * 3);
    match color {
        png::ColorType::Rgb => data.extend_from_slice(&src[..w * h * 3]),
        png::ColorType::Rgba => {
            for p in src[..w * h * 4].as_chunks::<4>().0 {
                data.extend(
                    background
                        .iter()
                        .zip(p)
                        .map(|(bg, c)| over_background(*c, p[3], *bg)),
                );
            }
        }
        png::ColorType::Grayscale => {
            for &g in &src[..w * h] {
                data.extend_from_slice(&[g, g, g]);
            }
        }
        png::ColorType::GrayscaleAlpha => {
            for p in src[..w * h * 2].as_chunks::<2>().0 {
                data.extend(background.iter().map(|bg| over_background(p[0], p[1], *bg)));
            }
        }
        png::ColorType::Indexed => {
            return Err(MediaError::Malformed(
                "the PNG's palette was not expanded".into(),
            ));
        }
    }
    Ok(Rgb {
        width: w,
        height: h,
        data,
    })
}

pub(crate) fn decode_jpeg(bytes: &[u8]) -> Result<Rgb, MediaError> {
    use zune_core::bytestream::ZCursor;
    use zune_core::colorspace::ColorSpace;
    use zune_core::options::DecoderOptions;

    let bad = |e: zune_jpeg::errors::DecodeErrors| {
        MediaError::Malformed(format!("the JPEG is damaged: {e:?}"))
    };
    let options = DecoderOptions::default()
        .jpeg_set_out_colorspace(ColorSpace::RGB)
        .set_max_width(MAX_SIDE)
        .set_max_height(MAX_SIDE);
    let mut decoder = zune_jpeg::JpegDecoder::new_with_options(ZCursor::new(bytes), options);
    decoder.decode_headers().map_err(bad)?;
    let info = decoder
        .info()
        .ok_or_else(|| MediaError::Malformed("the JPEG has no header".into()))?;
    let (w, h) = (usize::from(info.width), usize::from(info.height));
    check_size(w, h)?;
    let data = decoder.decode().map_err(bad)?;
    if data.len() != w * h * 3 {
        return Err(MediaError::Malformed(
            "the JPEG did not decode to three colours".into(),
        ));
    }
    Ok(Rgb {
        width: w,
        height: h,
        data,
    })
}

/// The size that fits `width` by `height` inside `max_w` by `max_h`, keeping its shape and
/// never enlarging it.
fn fit(width: usize, height: usize, max_w: usize, max_h: usize) -> (usize, usize) {
    if width <= max_w && height <= max_h {
        return (width, height);
    }
    // Compare width / max_w with height / max_h by cross-multiplying.
    if width * max_h >= height * max_w {
        (max_w, (height * max_w / width).max(1))
    } else {
        ((width * max_h / height).max(1), max_h)
    }
}

/// `src` made `width` by `height`: each output pixel the mean of the area it covers.
pub(crate) fn shrink(src: &Rgb, width: usize, height: usize) -> Rgb {
    if (width, height) == (src.width, src.height) {
        return Rgb {
            width,
            height,
            data: src.data.clone(),
        };
    }
    let mut data = Vec::with_capacity(width * height * 3);
    for y in 0..height {
        let (y0, y1) = (
            y * src.height / height,
            ((y + 1) * src.height / height).max(y * src.height / height + 1),
        );
        for x in 0..width {
            let (x0, x1) = (
                x * src.width / width,
                ((x + 1) * src.width / width).max(x * src.width / width + 1),
            );
            let mut sum = [0u32; 3];
            for sy in y0..y1.min(src.height) {
                for sx in x0..x1.min(src.width) {
                    let i = (sy * src.width + sx) * 3;
                    for (acc, c) in sum.iter_mut().zip(&src.data[i..i + 3]) {
                        *acc += u32::from(*c);
                    }
                }
            }
            let n = ((y1.min(src.height) - y0) * (x1.min(src.width) - x0)) as u32;
            for s in sum {
                data.push(((s + n / 2) / n) as u8);
            }
        }
    }
    Rgb {
        width,
        height,
        data,
    }
}

pub(crate) fn encode_png(rgb: &Rgb) -> Result<Vec<u8>, MediaError> {
    let fail = |e: png::EncodingError| MediaError::Encode(e.to_string());
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, rgb.width as u32, rgb.height as u32);
    encoder.set_color(png::ColorType::Rgb);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().map_err(fail)?;
    writer.write_image_data(&rgb.data).map_err(fail)?;
    writer.finish().map_err(fail)?;
    Ok(out)
}

/// Read an untrusted PNG or JPEG as 8-bit RGB, transparency flattened onto `background`: the
/// format it was read as, and the pixels.
pub(crate) fn decode(bytes: &[u8], background: [u8; 3]) -> Result<(&'static str, Rgb), MediaError> {
    if bytes.starts_with(PNG_SIGNATURE) {
        Ok(("png", decode_png(bytes, background)?))
    } else if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        Ok(("jpeg", decode_jpeg(bytes)?))
    } else {
        Err(MediaError::NotAnImage)
    }
}

/// The whole job for a still: read a PNG or JPEG, fit it inside `max_width` by `max_height`
/// (never enlarging it), write the PNG.
pub fn process_still(
    bytes: &[u8],
    max_width: usize,
    max_height: usize,
) -> Result<(Vec<u8>, StillReport), MediaError> {
    if bytes.is_empty() {
        return Err(MediaError::Empty);
    }
    let (format, rgb) = decode(bytes, BACKGROUND)?;
    let (w, h) = fit(rgb.width, rgb.height, max_width.max(1), max_height.max(1));
    let small = shrink(&rgb, w, h);
    let png = encode_png(&small)?;
    let report = StillReport {
        format: format.to_owned(),
        source_width: rgb.width as u32,
        source_height: rgb.height as u32,
        width: w as u32,
        height: h as u32,
        bytes: png.len(),
    };
    Ok((png, report))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png_of(width: u32, height: u32, color: png::ColorType, pixel: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut e = png::Encoder::new(&mut out, width, height);
        e.set_color(color);
        e.set_depth(png::BitDepth::Eight);
        let mut w = e.write_header().unwrap();
        let data: Vec<u8> = pixel
            .iter()
            .copied()
            .cycle()
            .take(pixel.len() * (width * height) as usize)
            .collect();
        w.write_image_data(&data).unwrap();
        w.finish().unwrap();
        out
    }

    fn decoded(png: &[u8]) -> Rgb {
        decode_png(png, BACKGROUND).unwrap()
    }

    #[test]
    fn a_png_is_fitted_and_rewritten_clean() {
        let src = png_of(1600, 800, png::ColorType::Rgb, &[200, 30, 30]);
        let (out, report) = process_still(&src, 800, 600).unwrap();
        assert_eq!((report.width, report.height), (800, 400));
        assert_eq!((report.source_width, report.source_height), (1600, 800));
        assert_eq!(report.format, "png");
        assert_eq!(report.bytes, out.len());
        let back = decoded(&out);
        assert_eq!((back.width, back.height), (800, 400));
        assert_eq!(&back.data[..3], &[200, 30, 30]);
    }

    #[test]
    fn a_small_picture_is_not_enlarged() {
        let src = png_of(100, 60, png::ColorType::Rgb, &[1, 2, 3]);
        let (_, report) = process_still(&src, 854, 480).unwrap();
        assert_eq!((report.width, report.height), (100, 60));
    }

    #[test]
    fn transparency_takes_the_slates_colour() {
        let src = png_of(4, 4, png::ColorType::Rgba, &[255, 255, 255, 0]);
        let (out, _) = process_still(&src, 100, 100).unwrap();
        assert_eq!(&decoded(&out).data[..3], &BACKGROUND);
        let half = png_of(4, 4, png::ColorType::GrayscaleAlpha, &[255, 255]);
        let (out, _) = process_still(&half, 100, 100).unwrap();
        assert_eq!(&decoded(&out).data[..3], &[255, 255, 255]);
    }

    #[test]
    fn a_jpeg_is_read() {
        // A 16 x 16 grey baseline JPEG, made by hand-free means: encode through zune? Not
        // available, so use a tiny stored JFIF.
        let jpeg = include_bytes!("../tests/grey-16.jpg");
        let (out, report) = process_still(jpeg, 8, 8).unwrap();
        assert_eq!(report.format, "jpeg");
        assert_eq!((report.width, report.height), (8, 8));
        let back = decoded(&out);
        let mean = back.data.iter().map(|b| u32::from(*b)).sum::<u32>() / back.data.len() as u32;
        assert!(
            (100..160).contains(&mean),
            "a mid grey stays mid grey, got {mean}"
        );
    }

    #[test]
    fn what_is_not_an_image_is_refused() {
        assert_eq!(process_still(b"", 10, 10).unwrap_err(), MediaError::Empty);
        assert_eq!(
            process_still(b"RIFF....WAVEfmt ", 10, 10).unwrap_err(),
            MediaError::NotAnImage
        );
        assert!(matches!(
            process_still(
                &PNG_SIGNATURE
                    .iter()
                    .chain(b"junk junk junk")
                    .copied()
                    .collect::<Vec<u8>>(),
                10,
                10
            ),
            Err(MediaError::Malformed(_))
        ));
        assert!(matches!(
            process_still(&[0xff, 0xd8, 0xff, 0xe0, 0, 4, 1, 2], 10, 10),
            Err(MediaError::Malformed(_))
        ));
    }

    #[test]
    fn a_huge_picture_is_refused_before_it_is_decoded() {
        // A header that claims 60,000 by 60,000 pixels and carries no pixels at all.
        let mut out = Vec::new();
        let mut e = png::Encoder::new(&mut out, 60_000, 60_000);
        e.set_color(png::ColorType::Grayscale);
        e.set_depth(png::BitDepth::One);
        e.write_header().unwrap();
        let err = process_still(&out, 100, 100).unwrap_err();
        assert!(matches!(err, MediaError::ImageTooBig { .. }), "{err:?}");
    }

    #[test]
    fn fitting_keeps_the_shape() {
        assert_eq!(fit(1000, 1000, 854, 480), (480, 480));
        assert_eq!(fit(4000, 1000, 854, 480), (854, 213));
        assert_eq!(fit(10, 10, 854, 480), (10, 10));
        assert_eq!(fit(10_000, 1, 100, 100), (100, 1));
    }
}
