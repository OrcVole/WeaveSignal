// SPDX-License-Identifier: AGPL-3.0-only
// SPDX-FileCopyrightText: 2026 The WeaveSignal contributors

//! Pictures as the vision mixer handles them: I420 frames in GStreamer's default layout
//! (each plane's rows padded to four bytes), so a frame goes in from an `appsink` and out to
//! an `appsrc` as one copy. The few operations the desk needs are here in plain Rust: a
//! rectangle laid over another picture (a lower third), a picture made smaller (a guest's
//! thumbnail), and the mean colour of an area (the tests read the picture by it).

/// One I420 picture.
#[derive(Clone, PartialEq, Eq)]
pub struct Frame {
    pub width: usize,
    pub height: usize,
    pub data: Vec<u8>,
}

impl std::fmt::Debug for Frame {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Frame({}x{})", self.width, self.height)
    }
}

fn up4(n: usize) -> usize {
    n.div_ceil(4) * 4
}

fn up2(n: usize) -> usize {
    n.div_ceil(2) * 2
}

/// Where each plane of an I420 picture sits, in GStreamer's default layout.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Layout {
    pub y_stride: usize,
    pub c_stride: usize,
    pub u_offset: usize,
    pub v_offset: usize,
    pub size: usize,
    pub c_width: usize,
    pub c_height: usize,
}

impl Layout {
    pub fn of(width: usize, height: usize) -> Layout {
        let y_stride = up4(width);
        let c_stride = up4(up2(width) / 2);
        let u_offset = y_stride * up2(height);
        let c_height = up2(height) / 2;
        let v_offset = u_offset + c_stride * c_height;
        Layout {
            y_stride,
            c_stride,
            u_offset,
            v_offset,
            size: v_offset + c_stride * c_height,
            c_width: up2(width) / 2,
            c_height,
        }
    }
}

/// A colour as Y, U, V (BT.601, limited range, as GStreamer's test sources make it).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Yuv {
    pub y: f32,
    pub u: f32,
    pub v: f32,
}

impl Yuv {
    /// From 8-bit RGB.
    pub fn from_rgb(r: u8, g: u8, b: u8) -> Yuv {
        let (r, g, b) = (f32::from(r), f32::from(g), f32::from(b));
        Yuv {
            y: 16.0 + (65.481 * r + 128.553 * g + 24.966 * b) / 255.0,
            u: 128.0 + (-37.797 * r - 74.203 * g + 112.0 * b) / 255.0,
            v: 128.0 + (112.0 * r - 93.786 * g - 18.214 * b) / 255.0,
        }
    }

    /// The largest difference of the three components.
    pub fn distance(&self, other: &Yuv) -> f32 {
        (self.y - other.y)
            .abs()
            .max((self.u - other.u).abs())
            .max((self.v - other.v).abs())
    }
}

impl Frame {
    /// A picture of one colour.
    pub fn solid(width: usize, height: usize, c: Yuv) -> Frame {
        let l = Layout::of(width, height);
        let mut data = vec![c.y.round() as u8; l.size];
        data[l.u_offset..l.v_offset].fill(c.u.round() as u8);
        data[l.v_offset..].fill(c.v.round() as u8);
        Frame {
            width,
            height,
            data,
        }
    }

    /// A picture from 8-bit RGB, rows packed (the slate's still, once decoded); `None` if the
    /// size is wrong or empty. Each colour sample is the mean of the (up to) four pixels it
    /// covers.
    pub fn from_rgb(width: usize, height: usize, rgb: &[u8]) -> Option<Frame> {
        if width == 0 || height == 0 || rgb.len() != width * height * 3 {
            return None;
        }
        let l = Layout::of(width, height);
        let mut data = vec![0u8; l.size];
        let px = |x: usize, y: usize| {
            let i = (y.min(height - 1) * width + x.min(width - 1)) * 3;
            Yuv::from_rgb(rgb[i], rgb[i + 1], rgb[i + 2])
        };
        for y in 0..height {
            for x in 0..width {
                data[y * l.y_stride + x] = px(x, y).y.round().clamp(0.0, 255.0) as u8;
            }
        }
        for cy in 0..l.c_height {
            for cx in 0..l.c_width {
                let (mut u, mut v) = (0.0f32, 0.0f32);
                for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                    let c = px(cx * 2 + dx, cy * 2 + dy);
                    u += c.u;
                    v += c.v;
                }
                data[l.u_offset + cy * l.c_stride + cx] = (u / 4.0).round().clamp(0.0, 255.0) as u8;
                data[l.v_offset + cy * l.c_stride + cx] = (v / 4.0).round().clamp(0.0, 255.0) as u8;
            }
        }
        Some(Frame {
            width,
            height,
            data,
        })
    }

    /// A picture from a buffer in the default layout; `None` if the size is wrong.
    pub fn from_bytes(width: usize, height: usize, bytes: &[u8]) -> Option<Frame> {
        (bytes.len() == Layout::of(width, height).size).then(|| Frame {
            width,
            height,
            data: bytes.to_vec(),
        })
    }

    pub fn layout(&self) -> Layout {
        Layout::of(self.width, self.height)
    }

    /// Lay `src` over this picture with its top-left corner at (`x`, `y`), clipped to the
    /// picture; `x` and `y` are rounded down to even numbers (the colour planes are half size).
    pub fn blit(&mut self, src: &Frame, x: usize, y: usize) {
        let (x, y) = (x & !1, y & !1);
        if x >= self.width || y >= self.height {
            return;
        }
        let (d, s) = (self.layout(), src.layout());
        let w = src.width.min(self.width - x);
        let h = src.height.min(self.height - y);
        for row in 0..h {
            let di = (y + row) * d.y_stride + x;
            let si = row * s.y_stride;
            self.data[di..di + w].copy_from_slice(&src.data[si..si + w]);
        }
        let (cx, cy, cw, ch) = (x / 2, y / 2, w.div_ceil(2), h.div_ceil(2));
        for (doff, soff) in [(d.u_offset, s.u_offset), (d.v_offset, s.v_offset)] {
            for row in 0..ch {
                let di = doff + (cy + row) * d.c_stride + cx;
                let si = soff + row * s.c_stride;
                let cw = cw.min(d.c_width - cx);
                self.data[di..di + cw].copy_from_slice(&src.data[si..si + cw]);
            }
        }
    }

    /// This picture made `width` by `height`, each output sample the mean of the input area
    /// it covers (a box filter: soft enough for a thumbnail, and cheap).
    pub fn scaled(&self, width: usize, height: usize) -> Frame {
        let (s, l) = (self.layout(), Layout::of(width, height));
        let mut data = vec![0u8; l.size];
        let plane = |data: &mut [u8],
                     src: &[u8],
                     (sw, sh, ss): (usize, usize, usize),
                     (dw, dh, ds): (usize, usize, usize)| {
            for dy in 0..dh {
                let y0 = dy * sh / dh;
                let y1 = ((dy + 1) * sh / dh).max(y0 + 1);
                for dx in 0..dw {
                    let x0 = dx * sw / dw;
                    let x1 = ((dx + 1) * sw / dw).max(x0 + 1);
                    let mut sum = 0u32;
                    for yy in y0..y1 {
                        for xx in x0..x1 {
                            sum += u32::from(src[yy * ss + xx]);
                        }
                    }
                    data[dy * ds + dx] = (sum / ((y1 - y0) * (x1 - x0)) as u32) as u8;
                }
            }
        };
        plane(
            &mut data[..l.u_offset],
            &self.data[..s.u_offset],
            (self.width, self.height, s.y_stride),
            (width, height, l.y_stride),
        );
        for (doff, soff) in [(l.u_offset, s.u_offset), (l.v_offset, s.v_offset)] {
            plane(
                &mut data[doff..],
                &self.data[soff..],
                (s.c_width, s.c_height, s.c_stride),
                (l.c_width, l.c_height, l.c_stride),
            );
        }
        Frame {
            width,
            height,
            data,
        }
    }

    /// Fill a rectangle with one colour (clipped; corners rounded down to even).
    pub fn fill(&mut self, x: usize, y: usize, w: usize, h: usize, c: Yuv) {
        let block = Frame::solid(up2(w.max(2)), up2(h.max(2)), c);
        self.blit(&block, x, y);
    }

    /// The mean colour of a rectangle (clipped).
    pub fn mean(&self, x: usize, y: usize, w: usize, h: usize) -> Yuv {
        let l = self.layout();
        let (x1, y1) = ((x + w).min(self.width), (y + h).min(self.height));
        let avg = |off: usize, stride: usize, x0: usize, y0: usize, x1: usize, y1: usize| {
            let mut sum = 0u64;
            let mut n = 0u64;
            for yy in y0..y1 {
                for xx in x0..x1 {
                    sum += u64::from(self.data[off + yy * stride + xx]);
                    n += 1;
                }
            }
            if n == 0 { 0.0 } else { sum as f32 / n as f32 }
        };
        Yuv {
            y: avg(0, l.y_stride, x, y, x1, y1),
            u: avg(
                l.u_offset,
                l.c_stride,
                x / 2,
                y / 2,
                x1.div_ceil(2),
                y1.div_ceil(2),
            ),
            v: avg(
                l.v_offset,
                l.c_stride,
                x / 2,
                y / 2,
                x1.div_ceil(2),
                y1.div_ceil(2),
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_layout_is_gstreamers_default_for_odd_widths() {
        // 854x480: GStreamer pads the rows to 856 and 428.
        let l = Layout::of(854, 480);
        assert_eq!((l.y_stride, l.c_stride), (856, 428));
        assert_eq!(l.u_offset, 856 * 480);
        assert_eq!(l.size, 856 * 480 + 2 * 428 * 240);
        let l = Layout::of(426, 240);
        assert_eq!((l.y_stride, l.c_stride), (428, 216));
    }

    #[test]
    fn a_blit_lands_where_it_is_put_and_nowhere_else() {
        let red = Yuv::from_rgb(220, 30, 30);
        let blue = Yuv::from_rgb(30, 30, 220);
        let mut f = Frame::solid(854, 480, blue);
        f.blit(&Frame::solid(200, 100, red), 100, 300);
        assert!(f.mean(110, 310, 180, 80).distance(&red) < 1.0);
        assert!(f.mean(0, 0, 854, 290).distance(&blue) < 1.0);
        assert!(f.mean(320, 300, 500, 100).distance(&blue) < 1.0);
        // Off the edge: clipped, not a panic.
        f.blit(&Frame::solid(200, 100, red), 800, 450);
        assert!(f.mean(810, 460, 40, 20).distance(&red) < 1.0);
    }

    #[test]
    fn a_scaled_picture_keeps_its_colours_where_they_were() {
        let red = Yuv::from_rgb(220, 30, 30);
        let green = Yuv::from_rgb(30, 200, 30);
        let mut f = Frame::solid(854, 480, red);
        f.fill(428, 0, 426, 480, green);
        let t = f.scaled(160, 90);
        assert_eq!(t.data.len(), Layout::of(160, 90).size);
        assert!(t.mean(0, 0, 70, 90).distance(&red) < 2.0);
        assert!(t.mean(90, 0, 70, 90).distance(&green) < 2.0);
    }

    #[test]
    fn a_picture_from_rgb_has_the_colours_it_was_given() {
        let (w, h) = (13, 9); // odd on purpose: the colour planes round up
        let rgb: Vec<u8> = (0..w * h)
            .flat_map(|i| {
                if i % w < 6 {
                    [220, 30, 30]
                } else {
                    [30, 30, 220]
                }
            })
            .collect();
        let f = Frame::from_rgb(w, h, &rgb).unwrap();
        assert_eq!(f.data.len(), Layout::of(w, h).size);
        assert!(f.mean(0, 0, 6, 9).distance(&Yuv::from_rgb(220, 30, 30)) < 2.0);
        assert!(f.mean(7, 0, 6, 9).distance(&Yuv::from_rgb(30, 30, 220)) < 2.0);
        assert!(Frame::from_rgb(0, 9, &[]).is_none());
        assert!(Frame::from_rgb(2, 2, &[0; 5]).is_none());
    }

    #[test]
    fn a_buffer_of_the_wrong_size_is_not_a_frame() {
        assert!(Frame::from_bytes(854, 480, &[0; 100]).is_none());
        let l = Layout::of(854, 480);
        assert!(Frame::from_bytes(854, 480, &vec![0; l.size]).is_some());
    }
}
