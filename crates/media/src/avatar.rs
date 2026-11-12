// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! A chatter's own avatar (ADR-0235): decode an untrusted PNG or JPEG, flatten any transparency
//! onto a light ground, cut the middle square, make it no larger than the chat shows, and write
//! a clean PNG: 8-bit RGB with no other chunks, so nothing the upload carried beside its pixels
//! (EXIF, a location, a colour profile, text) survives. The chat only ever serves the PNG this
//! writes, never the upload.
//!
//! The decoders and the size check before any pixel is allocated are the still's
//! ([`crate::still`]); an avatar's own caps are tighter, as nobody needs a 36-megapixel one.

use serde::{Deserialize, Serialize};

use crate::MediaError;
use crate::still::{Rgb, decode, encode_png, shrink};

/// The longest side of a source picture, in pixels.
pub const MAX_SIDE: usize = 4096;
/// The most pixels in a source picture.
pub const MAX_PIXELS: usize = 16_000_000;
/// The shortest side of a source picture: below this there is nothing to see.
pub const MIN_SIDE: usize = 32;
/// The side an avatar is written at unless the caller says otherwise.
pub const DEFAULT_SIDE: usize = 128;

/// What transparent pixels take: a light neutral, which reads in a light and a dark chat.
const GROUND: [u8; 3] = [0xe9, 0xea, 0xee];

/// What the worker reports for an avatar.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AvatarReport {
    /// `png` or `jpeg`, the format it was read as.
    pub format: String,
    pub source_width: u32,
    pub source_height: u32,
    /// The square written: its side, never larger than asked and never enlarged.
    pub side: u32,
    /// The size of the PNG written.
    pub bytes: usize,
}

/// The middle square of a picture.
fn square(src: &Rgb) -> Rgb {
    let side = src.width.min(src.height);
    if src.width == src.height {
        return Rgb {
            width: side,
            height: side,
            data: src.data.clone(),
        };
    }
    let (x0, y0) = ((src.width - side) / 2, (src.height - side) / 2);
    let mut data = Vec::with_capacity(side * side * 3);
    for y in y0..y0 + side {
        let i = (y * src.width + x0) * 3;
        data.extend_from_slice(&src.data[i..i + side * 3]);
    }
    Rgb {
        width: side,
        height: side,
        data,
    }
}

/// The whole job for an avatar: read a PNG or JPEG, cut its middle square, make it at most
/// `side` pixels (never enlarging it), write the PNG.
pub fn process_avatar(bytes: &[u8], side: usize) -> Result<(Vec<u8>, AvatarReport), MediaError> {
    if bytes.is_empty() {
        return Err(MediaError::Empty);
    }
    // The header's size, checked against the avatar's own caps before the still's decoder
    // (whose caps are wider) allocates anything.
    if let Some((w, h)) = header_size(bytes) {
        check(w, h)?;
    }
    let (format, rgb) = decode(bytes, GROUND)?;
    check(rgb.width, rgb.height)?;
    let cut = square(&rgb);
    let out_side = cut.width.min(side.max(1));
    let small = shrink(&cut, out_side, out_side);
    let png = encode_png(&small)?;
    let report = AvatarReport {
        format: format.to_owned(),
        source_width: rgb.width as u32,
        source_height: rgb.height as u32,
        side: out_side as u32,
        bytes: png.len(),
    };
    Ok((png, report))
}

fn check(width: usize, height: usize) -> Result<(), MediaError> {
    if width > MAX_SIDE || height > MAX_SIDE || width * height > MAX_PIXELS {
        return Err(MediaError::ImageTooBig {
            width,
            height,
            max_side: MAX_SIDE,
            max_pixels: MAX_PIXELS,
        });
    }
    if width < MIN_SIDE || height < MIN_SIDE {
        return Err(MediaError::ImageTooSmall {
            width,
            height,
            min_side: MIN_SIDE,
        });
    }
    Ok(())
}

/// A PNG's size from its first chunk, or a JPEG's from its first frame header, without decoding.
fn header_size(b: &[u8]) -> Option<(usize, usize)> {
    let be16 = |i: usize| {
        Some(usize::from(u16::from_be_bytes([
            *b.get(i)?,
            *b.get(i + 1)?,
        ])))
    };
    if b.len() >= 24 && &b[12..16] == b"IHDR" {
        let be32 = |i: usize| u32::from_be_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]) as usize;
        return Some((be32(16), be32(20)));
    }
    if b.starts_with(&[0xff, 0xd8]) {
        let mut i = 2;
        while i + 4 <= b.len() {
            if b[i] != 0xff {
                return None;
            }
            let marker = b[i + 1];
            // Start-of-frame markers carry the size; the others are skipped by their length.
            if matches!(marker, 0xc0..=0xcf) && !matches!(marker, 0xc4 | 0xc8 | 0xcc) {
                return Some((be16(i + 7)?, be16(i + 5)?));
            }
            i += 2 + be16(i + 2)?;
        }
    }
    None
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

    /// A PNG with a text chunk, as a camera or an editor leaves one.
    fn png_with_text(width: u32, height: u32) -> Vec<u8> {
        let mut out = Vec::new();
        let mut e = png::Encoder::new(&mut out, width, height);
        e.set_color(png::ColorType::Rgb);
        e.set_depth(png::BitDepth::Eight);
        e.add_text_chunk("Location".into(), "56.4907 N, 4.2026 W".into())
            .unwrap();
        let mut w = e.write_header().unwrap();
        w.write_image_data(&vec![90u8; (width * height * 3) as usize])
            .unwrap();
        w.finish().unwrap();
        out
    }

    #[test]
    fn a_wide_picture_is_cut_square_and_made_small() {
        // Left third red, the rest blue: the middle square of 600 by 200 is all blue.
        let mut data = Vec::new();
        for _ in 0..200 {
            for x in 0..600 {
                data.extend_from_slice(if x < 200 { &[255, 0, 0] } else { &[0, 0, 255] });
            }
        }
        let mut src = Vec::new();
        let mut e = png::Encoder::new(&mut src, 600, 200);
        e.set_color(png::ColorType::Rgb);
        e.set_depth(png::BitDepth::Eight);
        let mut w = e.write_header().unwrap();
        w.write_image_data(&data).unwrap();
        w.finish().unwrap();
        let (out, report) = process_avatar(&src, 128).unwrap();
        assert_eq!(report.side, 128);
        assert_eq!((report.source_width, report.source_height), (600, 200));
        let (_, back) = decode(&out, GROUND).unwrap();
        assert_eq!((back.width, back.height), (128, 128));
        assert_eq!(&back.data[..3], &[0, 0, 255]);
    }

    #[test]
    fn nothing_but_pixels_survives() {
        let src = png_with_text(64, 64);
        assert!(src.windows(8).any(|w| w == b"Location"));
        let (out, report) = process_avatar(&src, 128).unwrap();
        assert_eq!(report.side, 64, "never enlarged");
        assert!(!out.windows(8).any(|w| w == b"Location"));
        assert!(!out.windows(4).any(|w| w == b"tEXt" || w == b"eXIf"));
    }

    #[test]
    fn transparency_takes_the_light_ground() {
        let src = png_of(40, 40, png::ColorType::Rgba, &[0, 0, 0, 0]);
        let (out, _) = process_avatar(&src, 128).unwrap();
        let (_, back) = decode(&out, [0, 0, 0]).unwrap();
        assert_eq!(&back.data[..3], &GROUND);
    }

    #[test]
    fn sizes_outside_the_caps_are_refused_before_decoding() {
        let mut huge = Vec::new();
        let mut e = png::Encoder::new(&mut huge, 5000, 5000);
        e.set_color(png::ColorType::Grayscale);
        e.set_depth(png::BitDepth::One);
        e.write_header().unwrap();
        assert!(matches!(
            process_avatar(&huge, 128),
            Err(MediaError::ImageTooBig { .. })
        ));
        let tiny = png_of(8, 8, png::ColorType::Rgb, &[1, 2, 3]);
        assert!(matches!(
            process_avatar(&tiny, 128),
            Err(MediaError::ImageTooSmall { .. })
        ));
    }

    #[test]
    fn a_jpeg_is_read_and_what_is_not_a_picture_is_refused() {
        let jpeg = include_bytes!("../tests/grey-16.jpg");
        assert_eq!(header_size(jpeg), Some((16, 16)));
        // 16 pixels is under the least side: refused as too small, which shows it was read.
        assert!(matches!(
            process_avatar(jpeg, 128),
            Err(MediaError::ImageTooSmall { .. })
        ));
        assert_eq!(
            process_avatar(b"<svg xmlns='http://www.w3.org/2000/svg'/>", 128).unwrap_err(),
            MediaError::NotAnImage
        );
        assert_eq!(process_avatar(b"", 128).unwrap_err(), MediaError::Empty);
    }
}
