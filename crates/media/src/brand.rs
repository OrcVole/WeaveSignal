// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! A station's brand pictures for its public pages (ADR-0237): the brand icon (also the
//! favicon) and the banner across the top of the player.
//!
//! As with stills and carts, the upload is untrusted and never served: this decodes a PNG or a
//! JPEG (recognised by its first bytes; nothing else, and never SVG, which is a document that
//! can carry script), and writes a clean 8-bit PNG with no other chunks, so no metadata
//! survives. Sizes are checked from the header before a pixel is allocated.
//!
//! - **The icon** keeps its transparency. It is fitted inside [`ICON_SIDE`] pixels square,
//!   centred on a transparent square, and never enlarged.
//! - **The banner** is cropped about its middle to three to one, the shape the player shows it
//!   in, then fitted inside [`BANNER_WIDTH`] by [`BANNER_HEIGHT`], never enlarged.

use std::io::Cursor;

use serde::{Deserialize, Serialize};

use crate::MediaError;
use crate::still::{MAX_PIXELS, MAX_SIDE};

/// The icon as stored: at most this many pixels on a side.
pub const ICON_SIDE: usize = 256;
/// An icon's source: each side at least this, so it is still a picture at favicon size...
pub const ICON_MIN_SIDE: usize = 64;
/// ...and at most this.
pub const ICON_MAX_SIDE: usize = 4096;
/// The largest icon upload read, in bytes.
pub const ICON_MAX_BYTES: usize = 1024 * 1024;

/// The banner as stored: at most this wide and this tall (three to one).
pub const BANNER_WIDTH: usize = 1200;
pub const BANNER_HEIGHT: usize = 400;
/// A banner's source is at least this wide and this tall, so it is not a smear on a desktop.
pub const BANNER_MIN_WIDTH: usize = 600;
pub const BANNER_MIN_HEIGHT: usize = 200;
/// The largest banner upload read, in bytes.
pub const BANNER_MAX_BYTES: usize = 4 * 1024 * 1024;

const PNG_SIGNATURE: &[u8] = b"\x89PNG\r\n\x1a\n";

/// Which picture.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Icon,
    Banner,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Icon => "icon",
            Kind::Banner => "banner",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "icon" => Some(Kind::Icon),
            "banner" => Some(Kind::Banner),
            _ => None,
        }
    }

    /// The largest upload read for this picture, in bytes.
    pub fn max_bytes(self) -> usize {
        match self {
            Kind::Icon => ICON_MAX_BYTES,
            Kind::Banner => BANNER_MAX_BYTES,
        }
    }
}

/// What the worker reports for a brand picture.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrandReport {
    /// `png` or `jpeg`, the format it was read as.
    pub format: String,
    pub source_width: u32,
    pub source_height: u32,
    /// The picture written.
    pub width: u32,
    pub height: u32,
    /// The size of the PNG written.
    pub bytes: usize,
    /// Whether the PNG written has transparency.
    pub alpha: bool,
}

/// 8-bit RGBA, rows packed, alpha not premultiplied.
struct Rgba {
    width: usize,
    height: usize,
    data: Vec<u8>,
}

fn too_big(width: usize, height: usize, max_side: usize) -> MediaError {
    MediaError::ImageTooBig {
        width,
        height,
        max_side,
        max_pixels: MAX_PIXELS,
    }
}

/// The source's size against what this picture takes: `Err` before any pixel is allocated.
fn check_size(kind: Kind, width: usize, height: usize) -> Result<(), MediaError> {
    if width == 0 || height == 0 {
        return Err(MediaError::Malformed("the picture is empty".into()));
    }
    match kind {
        Kind::Icon => {
            if width > ICON_MAX_SIDE || height > ICON_MAX_SIDE {
                return Err(too_big(width, height, ICON_MAX_SIDE));
            }
            if width < ICON_MIN_SIDE || height < ICON_MIN_SIDE {
                return Err(MediaError::Picture(format!(
                    "the icon is {width} by {height} pixels; each side is at least {ICON_MIN_SIDE}"
                )));
            }
        }
        Kind::Banner => {
            if width > MAX_SIDE || height > MAX_SIDE || width * height > MAX_PIXELS {
                return Err(too_big(width, height, MAX_SIDE));
            }
            if width < BANNER_MIN_WIDTH || height < BANNER_MIN_HEIGHT {
                return Err(MediaError::Picture(format!(
                    "the banner is {width} by {height} pixels; it is at least {BANNER_MIN_WIDTH} wide and {BANNER_MIN_HEIGHT} tall"
                )));
            }
        }
    }
    Ok(())
}

fn decode_png(kind: Kind, bytes: &[u8]) -> Result<Rgba, MediaError> {
    let bad = |e: png::DecodingError| MediaError::Malformed(format!("the PNG is damaged: {e}"));
    // The size is in the first chunk: read it ourselves, so nothing is allocated for a size
    // that is refused.
    if bytes.len() >= 24 && &bytes[12..16] == b"IHDR" {
        let be =
            |i: usize| u32::from_be_bytes([bytes[i], bytes[i + 1], bytes[i + 2], bytes[i + 3]]);
        check_size(kind, be(16) as usize, be(20) as usize)?;
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
    check_size(kind, w, h)?;
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
        png::ColorType::Grayscale => 1,
        png::ColorType::Indexed => {
            return Err(MediaError::Malformed(
                "the PNG's palette was not expanded".into(),
            ));
        }
    };
    if src.len() < w * h * samples {
        return Err(MediaError::Malformed("the PNG has too few pixels".into()));
    }
    let mut data = Vec::with_capacity(w * h * 4);
    for p in src[..w * h * samples].chunks_exact(samples) {
        match samples {
            3 => data.extend_from_slice(&[p[0], p[1], p[2], 255]),
            4 => data.extend_from_slice(p),
            2 => data.extend_from_slice(&[p[0], p[0], p[0], p[1]]),
            _ => data.extend_from_slice(&[p[0], p[0], p[0], 255]),
        }
    }
    Ok(Rgba {
        width: w,
        height: h,
        data,
    })
}

fn decode_jpeg(kind: Kind, bytes: &[u8]) -> Result<Rgba, MediaError> {
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
    check_size(kind, w, h)?;
    let rgb = decoder.decode().map_err(bad)?;
    if rgb.len() != w * h * 3 {
        return Err(MediaError::Malformed(
            "the JPEG did not decode to three colours".into(),
        ));
    }
    let mut data = Vec::with_capacity(w * h * 4);
    for p in rgb.as_chunks::<3>().0 {
        data.extend_from_slice(&[p[0], p[1], p[2], 255]);
    }
    Ok(Rgba {
        width: w,
        height: h,
        data,
    })
}

/// The middle `width` by `height` of `src`.
fn crop_middle(src: &Rgba, width: usize, height: usize) -> Rgba {
    let (x0, y0) = ((src.width - width) / 2, (src.height - height) / 2);
    let mut data = Vec::with_capacity(width * height * 4);
    for y in y0..y0 + height {
        let i = (y * src.width + x0) * 4;
        data.extend_from_slice(&src.data[i..i + width * 4]);
    }
    Rgba {
        width,
        height,
        data,
    }
}

/// `src` made `width` by `height` (never larger than it is): each output pixel the mean of the
/// area it covers, with colour weighted by its alpha so a transparent edge brings no fringe.
fn shrink(src: &Rgba, width: usize, height: usize) -> Rgba {
    if (width, height) == (src.width, src.height) {
        return Rgba {
            width,
            height,
            data: src.data.clone(),
        };
    }
    let mut data = Vec::with_capacity(width * height * 4);
    for y in 0..height {
        let y0 = y * src.height / height;
        let y1 = ((y + 1) * src.height / height).max(y0 + 1).min(src.height);
        for x in 0..width {
            let x0 = x * src.width / width;
            let x1 = ((x + 1) * src.width / width).max(x0 + 1).min(src.width);
            let (mut colour, mut alpha) = ([0u64; 3], 0u64);
            for sy in y0..y1 {
                for sx in x0..x1 {
                    let i = (sy * src.width + sx) * 4;
                    let a = u64::from(src.data[i + 3]);
                    for (acc, c) in colour.iter_mut().zip(&src.data[i..i + 3]) {
                        *acc += u64::from(*c) * a;
                    }
                    alpha += a;
                }
            }
            let n = ((y1 - y0) * (x1 - x0)) as u64;
            for c in colour {
                // No colour where nothing is opaque.
                data.push((c + alpha / 2).checked_div(alpha).unwrap_or(0) as u8);
            }
            data.push(((alpha + n / 2) / n) as u8);
        }
    }
    Rgba {
        width,
        height,
        data,
    }
}

/// `src` centred on a transparent square as wide as its longer side.
fn on_square(src: Rgba) -> Rgba {
    let side = src.width.max(src.height);
    if src.width == src.height {
        return src;
    }
    let mut data = vec![0u8; side * side * 4];
    let (x0, y0) = ((side - src.width) / 2, (side - src.height) / 2);
    for y in 0..src.height {
        let from = y * src.width * 4;
        let to = ((y0 + y) * side + x0) * 4;
        data[to..to + src.width * 4].copy_from_slice(&src.data[from..from + src.width * 4]);
    }
    Rgba {
        width: side,
        height: side,
        data,
    }
}

/// The PNG of `image`: RGB when every pixel is opaque, else RGBA. Whether it has transparency.
fn encode_png(image: &Rgba) -> Result<(Vec<u8>, bool), MediaError> {
    let fail = |e: png::EncodingError| MediaError::Encode(e.to_string());
    let alpha = image.data.as_chunks::<4>().0.iter().any(|p| p[3] != 255);
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, image.width as u32, image.height as u32);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.set_compression(png::Compression::High);
    if alpha {
        encoder.set_color(png::ColorType::Rgba);
        let mut writer = encoder.write_header().map_err(fail)?;
        writer.write_image_data(&image.data).map_err(fail)?;
        writer.finish().map_err(fail)?;
    } else {
        encoder.set_color(png::ColorType::Rgb);
        let rgb: Vec<u8> = image
            .data
            .as_chunks::<4>()
            .0
            .iter()
            .flat_map(|p| [p[0], p[1], p[2]])
            .collect();
        let mut writer = encoder.write_header().map_err(fail)?;
        writer.write_image_data(&rgb).map_err(fail)?;
        writer.finish().map_err(fail)?;
    }
    Ok((out, alpha))
}

/// The size that fits `width` by `height` inside `max_w` by `max_h`, keeping its shape and
/// never enlarging it.
fn fit(width: usize, height: usize, max_w: usize, max_h: usize) -> (usize, usize) {
    if width <= max_w && height <= max_h {
        return (width, height);
    }
    if width * max_h >= height * max_w {
        (max_w, (height * max_w / width).max(1))
    } else {
        ((width * max_h / height).max(1), max_h)
    }
}

/// The whole job for a brand picture: read a PNG or JPEG, shape it for its place, write the PNG.
pub fn process_brand(bytes: &[u8], kind: Kind) -> Result<(Vec<u8>, BrandReport), MediaError> {
    if bytes.is_empty() {
        return Err(MediaError::Empty);
    }
    if bytes.len() > kind.max_bytes() {
        return Err(MediaError::Picture(format!(
            "the {} is larger than {} MB",
            kind.as_str(),
            kind.max_bytes() / 1024 / 1024
        )));
    }
    let (format, source) = if bytes.starts_with(PNG_SIGNATURE) {
        ("png", decode_png(kind, bytes)?)
    } else if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        ("jpeg", decode_jpeg(kind, bytes)?)
    } else {
        return Err(MediaError::NotAnImage);
    };
    let shaped = match kind {
        Kind::Icon => {
            let (w, h) = fit(source.width, source.height, ICON_SIDE, ICON_SIDE);
            on_square(shrink(&source, w, h))
        }
        Kind::Banner => {
            // Three to one, about the middle: the wider of the two crops that fit.
            let (cw, ch) = if source.width >= source.height * 3 {
                (source.height * 3, source.height)
            } else {
                (source.width, (source.width / 3).max(1))
            };
            let cropped = crop_middle(&source, cw, ch);
            let (w, h) = fit(cw, ch, BANNER_WIDTH, BANNER_HEIGHT);
            shrink(&cropped, w, h)
        }
    };
    let (png, alpha) = encode_png(&shaped)?;
    let report = BrandReport {
        format: format.to_owned(),
        source_width: source.width as u32,
        source_height: source.height as u32,
        width: shaped.width as u32,
        height: shaped.height as u32,
        bytes: png.len(),
        alpha,
    };
    Ok((png, report))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png_of(width: u32, height: u32, color: png::ColorType, pixels: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut e = png::Encoder::new(&mut out, width, height);
        e.set_color(color);
        e.set_depth(png::BitDepth::Eight);
        let mut w = e.write_header().unwrap();
        w.write_image_data(pixels).unwrap();
        w.finish().unwrap();
        out
    }

    fn flat(width: u32, height: u32, color: png::ColorType, pixel: &[u8]) -> Vec<u8> {
        let data: Vec<u8> = pixel
            .iter()
            .copied()
            .cycle()
            .take(pixel.len() * (width * height) as usize)
            .collect();
        png_of(width, height, color, &data)
    }

    /// The PNG as RGBA, its size, and the names of its chunks.
    fn read(png: &[u8]) -> (Vec<u8>, u32, u32, Vec<String>) {
        let mut chunks = Vec::new();
        let mut at = 8;
        while at + 8 <= png.len() {
            let len = u32::from_be_bytes(png[at..at + 4].try_into().unwrap()) as usize;
            chunks.push(String::from_utf8_lossy(&png[at + 4..at + 8]).into_owned());
            at += 12 + len;
        }
        let mut d = png::Decoder::new(Cursor::new(png));
        d.set_transformations(png::Transformations::EXPAND);
        let mut r = d.read_info().unwrap();
        let mut buf = vec![0u8; r.output_buffer_size().unwrap()];
        let f = r.next_frame(&mut buf).unwrap();
        let (color, _) = r.output_color_type();
        let rgba = match color {
            png::ColorType::Rgba => buf[..f.buffer_size()].to_vec(),
            png::ColorType::Rgb => buf[..f.buffer_size()]
                .as_chunks::<3>()
                .0
                .iter()
                .flat_map(|p| [p[0], p[1], p[2], 255])
                .collect(),
            other => panic!("written as {other:?}"),
        };
        (rgba, f.width, f.height, chunks)
    }

    #[test]
    fn an_icon_keeps_its_transparency_and_becomes_a_square() {
        // 512 by 256: the left half opaque red, the right half fully transparent.
        let mut px = Vec::new();
        for _ in 0..256 {
            for x in 0..512 {
                px.extend_from_slice(if x < 256 {
                    &[200, 30, 30, 255]
                } else {
                    &[0, 0, 0, 0]
                });
            }
        }
        let (png, report) =
            process_brand(&png_of(512, 256, png::ColorType::Rgba, &px), Kind::Icon).unwrap();
        assert_eq!((report.width, report.height), (256, 256));
        assert!(report.alpha);
        let (rgba, w, h, _) = read(&png);
        assert_eq!((w, h), (256, 256));
        let at = |x: usize, y: usize| &rgba[(y * 256 + x) * 4..(y * 256 + x) * 4 + 4];
        // The picture is 256 by 128, centred: rows 64 to 191. Above and below is padding.
        assert_eq!(at(10, 10), [0, 0, 0, 0]);
        assert_eq!(at(10, 250), [0, 0, 0, 0]);
        assert_eq!(at(60, 128), [200, 30, 30, 255]);
        assert_eq!(at(200, 128), [0, 0, 0, 0]);
    }

    #[test]
    fn a_transparent_edge_brings_no_dark_fringe() {
        // A checkerboard of opaque white and transparent black, halved: half-transparent white,
        // not grey.
        let mut px = Vec::new();
        for y in 0..128 {
            for x in 0..128 {
                px.extend_from_slice(if (x + y) % 2 == 0 {
                    &[255, 255, 255, 255]
                } else {
                    &[0, 0, 0, 0]
                });
            }
        }
        let src = Rgba {
            width: 128,
            height: 128,
            data: px,
        };
        let half = shrink(&src, 64, 64);
        assert_eq!(&half.data[..4], [255, 255, 255, 128]);
    }

    #[test]
    fn a_small_icon_is_not_enlarged_and_an_opaque_one_is_written_without_alpha() {
        let (png, report) = process_brand(
            &flat(96, 96, png::ColorType::Rgb, &[10, 120, 200]),
            Kind::Icon,
        )
        .unwrap();
        assert_eq!((report.width, report.height, report.alpha), (96, 96, false));
        let (rgba, ..) = read(&png);
        assert_eq!(&rgba[..4], [10, 120, 200, 255]);
    }

    #[test]
    fn a_banner_is_cropped_about_its_middle_to_three_to_one() {
        // 1,800 by 1,200: a green band across the middle third, blue above and below.
        let mut px = Vec::with_capacity(1800 * 1200 * 3);
        for y in 0..1200 {
            for _ in 0..1800 {
                px.extend_from_slice(if (300..900).contains(&y) {
                    &[0, 160, 0]
                } else {
                    &[0, 0, 160]
                });
            }
        }
        let (png, report) =
            process_brand(&png_of(1800, 1200, png::ColorType::Rgb, &px), Kind::Banner).unwrap();
        assert_eq!((report.width, report.height), (1200, 400));
        assert!(!report.alpha);
        let (rgba, w, h, _) = read(&png);
        assert_eq!((w, h), (1200, 400));
        // The crop is rows 300 to 899 of the source: all green.
        assert_eq!(&rgba[..4], [0, 160, 0, 255]);
        assert_eq!(&rgba[rgba.len() - 4..], [0, 160, 0, 255]);
    }

    #[test]
    fn a_wide_banner_loses_its_sides_and_a_small_one_is_not_enlarged() {
        let (_, wide) = process_brand(
            &flat(4000, 400, png::ColorType::Rgb, &[9, 9, 9]),
            Kind::Banner,
        )
        .unwrap();
        assert_eq!((wide.width, wide.height), (1200, 400));
        let (_, small) = process_brand(
            &flat(600, 200, png::ColorType::Rgb, &[9, 9, 9]),
            Kind::Banner,
        )
        .unwrap();
        assert_eq!((small.width, small.height), (600, 200));
        let (_, odd) = process_brand(
            &flat(700, 500, png::ColorType::Rgb, &[9, 9, 9]),
            Kind::Banner,
        )
        .unwrap();
        assert_eq!((odd.width, odd.height), (700, 233));
    }

    #[test]
    fn what_is_written_has_no_chunk_but_the_picture() {
        // A source with a text chunk (a comment, where a camera or an editor writes metadata).
        let mut src = Vec::new();
        {
            let mut e = png::Encoder::new(&mut src, 64, 64);
            e.set_color(png::ColorType::Rgb);
            e.set_depth(png::BitDepth::Eight);
            e.add_text_chunk("Comment".into(), "taken at home, 55.95 N 3.19 W".into())
                .unwrap();
            let mut w = e.write_header().unwrap();
            w.write_image_data(&[7u8; 64 * 64 * 3]).unwrap();
            w.finish().unwrap();
        }
        assert!(src.windows(7).any(|w| w == b"taken a"));
        let (png, _) = process_brand(&src, Kind::Icon).unwrap();
        let (_, _, _, chunks) = read(&png);
        assert!(
            chunks
                .iter()
                .all(|c| ["IHDR", "IDAT", "IEND"].contains(&c.as_str())),
            "{chunks:?}"
        );
        assert!(!png.windows(7).any(|w| w == b"taken a"));
    }

    #[test]
    fn svg_and_everything_else_that_is_no_png_or_jpeg_is_refused() {
        for bytes in [
            &b"<svg xmlns=\"http://www.w3.org/2000/svg\"><script>alert(1)</script></svg>"[..],
            b"<?xml version=\"1.0\"?><svg/>",
            b"GIF89a\x01\x00\x01\x00",
            b"RIFF\x00\x00\x00\x00WEBPVP8 ",
            b"<!doctype html><html>",
            b"\x00\x00\x01\x00\x01\x00",
        ] {
            for kind in [Kind::Icon, Kind::Banner] {
                assert_eq!(process_brand(bytes, kind), Err(MediaError::NotAnImage));
            }
        }
        assert_eq!(process_brand(b"", Kind::Icon), Err(MediaError::Empty));
    }

    #[test]
    fn a_damaged_picture_is_refused_not_guessed_at() {
        let whole = flat(128, 128, png::ColorType::Rgb, &[1, 2, 3]);
        let cut = &whole[..whole.len() / 2];
        assert!(matches!(
            process_brand(cut, Kind::Icon),
            Err(MediaError::Malformed(_))
        ));
        assert!(matches!(
            process_brand(b"\xff\xd8\xff\xe0not a jpeg at all", Kind::Banner),
            Err(MediaError::Malformed(_))
        ));
    }

    #[test]
    fn sizes_are_refused_from_the_header_before_any_pixel() {
        // A header that claims 60,000 by 60,000 and has no pixels behind it.
        let mut huge = flat(64, 64, png::ColorType::Rgb, &[0, 0, 0]);
        huge[16..20].copy_from_slice(&60_000u32.to_be_bytes());
        huge[20..24].copy_from_slice(&60_000u32.to_be_bytes());
        for kind in [Kind::Icon, Kind::Banner] {
            assert!(matches!(
                process_brand(&huge, kind),
                Err(MediaError::ImageTooBig { .. })
            ));
        }
        let e = process_brand(
            &flat(5000, 100, png::ColorType::Grayscale, &[0]),
            Kind::Icon,
        );
        assert!(
            matches!(e, Err(MediaError::ImageTooBig { max_side, .. }) if max_side == ICON_MAX_SIDE)
        );
        let small = process_brand(&flat(32, 32, png::ColorType::Rgb, &[0, 0, 0]), Kind::Icon);
        assert_eq!(
            small.unwrap_err().to_string(),
            "the icon is 32 by 32 pixels; each side is at least 64"
        );
        let thin = process_brand(
            &flat(599, 300, png::ColorType::Rgb, &[0, 0, 0]),
            Kind::Banner,
        );
        assert_eq!(
            thin.unwrap_err().to_string(),
            "the banner is 599 by 300 pixels; it is at least 600 wide and 200 tall"
        );
    }

    #[test]
    fn an_upload_over_the_cap_is_refused_unread() {
        let big = vec![0x89u8; ICON_MAX_BYTES + 1];
        assert_eq!(
            process_brand(&big, Kind::Icon).unwrap_err().to_string(),
            "the icon is larger than 1 MB"
        );
        let big = vec![0xffu8; BANNER_MAX_BYTES + 1];
        assert_eq!(
            process_brand(&big, Kind::Banner).unwrap_err().to_string(),
            "the banner is larger than 4 MB"
        );
    }

    #[test]
    fn grey_and_grey_with_alpha_are_read() {
        let (png, r) = process_brand(
            &flat(64, 64, png::ColorType::GrayscaleAlpha, &[200, 100]),
            Kind::Icon,
        )
        .unwrap();
        assert!(r.alpha);
        let (rgba, ..) = read(&png);
        assert_eq!(&rgba[..4], [200, 200, 200, 100]);
    }
}
