//! 128x64 monochrome framebuffer, text rendering and the HID packet encoding
//! for the base-station OLED.
//!
//! Wire format (verified live): the panel is driven by **Feature** reports of
//! 1024 bytes — `[0x06, 0x93, dst_x, 0, strip_w, padded_h, <column-major LSB
//! bitmap>]`. 128 px is sent as two 64 px strips. Brightness and return-to-UI
//! are 64-byte **output** reports instead.

use super::font::{FONT5X7, GLYPH_H, GLYPH_W};
use super::protocol::REPORT_ID;

pub const WIDTH: usize = 128;
pub const HEIGHT: usize = 64;
const STRIDE: usize = WIDTH / 8;

const CMD_SCREEN: u8 = 0x93;
const CMD_BRIGHTNESS: u8 = 0x85;
const CMD_RETURN_UI: u8 = 0x95;
const FRAME_REPORT_SIZE: usize = 1024;
const STRIP_WIDTH: usize = 64;
/// Character advance (5 px glyph + 1 px gap) at scale 1.
pub const CHAR_ADVANCE: usize = GLYPH_W + 1;

/// A 1bpp row-major (MSB-first) 128x64 framebuffer.
#[derive(Clone)]
pub struct Framebuffer {
    buf: [u8; STRIDE * HEIGHT],
}

impl Default for Framebuffer {
    fn default() -> Self {
        Self::new()
    }
}

impl Framebuffer {
    pub fn new() -> Self {
        Self {
            buf: [0u8; STRIDE * HEIGHT],
        }
    }

    #[allow(dead_code)]
    pub fn clear(&mut self) {
        self.buf.fill(0);
    }

    #[inline]
    pub fn set(&mut self, x: isize, y: isize, on: bool) {
        if x < 0 || y < 0 || x as usize >= WIDTH || y as usize >= HEIGHT {
            return;
        }
        let (x, y) = (x as usize, y as usize);
        let idx = y * STRIDE + (x >> 3);
        let mask = 0x80u8 >> (x & 7);
        if on {
            self.buf[idx] |= mask;
        } else {
            self.buf[idx] &= !mask;
        }
    }

    #[inline]
    fn get(&self, x: usize, y: usize) -> bool {
        (self.buf[y * STRIDE + (x >> 3)] >> (7 - (x & 7))) & 1 != 0
    }

    /// Filled rectangle.
    pub fn fill_rect(&mut self, x: isize, y: isize, w: usize, h: usize, on: bool) {
        for dy in 0..h as isize {
            for dx in 0..w as isize {
                self.set(x + dx, y + dy, on);
            }
        }
    }

    /// 1px rectangle outline.
    pub fn stroke_rect(&mut self, x: isize, y: isize, w: usize, h: usize) {
        if w == 0 || h == 0 {
            return;
        }
        let (w, h) = (w as isize, h as isize);
        for dx in 0..w {
            self.set(x + dx, y, true);
            self.set(x + dx, y + h - 1, true);
        }
        for dy in 0..h {
            self.set(x, y + dy, true);
            self.set(x + w - 1, y + dy, true);
        }
    }

    /// Draw one glyph. `scale` enlarges each pixel into a scale×scale block.
    pub fn draw_char(&mut self, x: isize, y: isize, c: char, scale: usize) {
        let code = c as usize;
        if code >= 256 {
            return;
        }
        let scale = scale.max(1) as isize;
        for col in 0..GLYPH_W {
            let bits = FONT5X7[code * GLYPH_W + col];
            for row in 0..GLYPH_H {
                if (bits >> row) & 1 != 0 {
                    self.fill_rect(
                        x + col as isize * scale,
                        y + row as isize * scale,
                        scale as usize,
                        scale as usize,
                        true,
                    );
                }
            }
        }
    }

    /// Draw a string left-to-right. Returns the x just past the last glyph.
    pub fn draw_text(&mut self, x: isize, y: isize, text: &str, scale: usize) -> isize {
        let mut cx = x;
        let advance = CHAR_ADVANCE as isize * scale.max(1) as isize;
        for c in text.chars() {
            self.draw_char(cx, y, c, scale);
            cx += advance;
        }
        cx
    }

    /// Pixel width a string will occupy at a given scale.
    pub fn text_width(text: &str, scale: usize) -> usize {
        text.chars().count() * CHAR_ADVANCE * scale.max(1)
    }

    /// Draw a string centred horizontally at row `y`.
    pub fn draw_text_centered(&mut self, y: isize, text: &str, scale: usize) {
        let w = Self::text_width(text, scale) as isize;
        self.draw_text((WIDTH as isize - w) / 2, y, text, scale);
    }

    /// Set pixels from a 128x64 grayscale buffer using Floyd–Steinberg
    /// dithering (0 = black, 255 = white). Buffer must be WIDTH*HEIGHT long.
    pub fn blit_gray_dithered(&mut self, gray: &[u8]) {
        if gray.len() < WIDTH * HEIGHT {
            return;
        }
        let mut err = vec![0i16; WIDTH * HEIGHT];
        for y in 0..HEIGHT {
            for x in 0..WIDTH {
                let i = y * WIDTH + x;
                let val = gray[i] as i16 + err[i];
                let on = val >= 128;
                self.set(x as isize, y as isize, on);
                let quant_err = val - if on { 255 } else { 0 };
                // Distribute the quantisation error to neighbours (FS weights).
                let mut spread = |xx: usize, yy: usize, num: i16| {
                    if xx < WIDTH && yy < HEIGHT {
                        err[yy * WIDTH + xx] += quant_err * num / 16;
                    }
                };
                if x + 1 < WIDTH {
                    spread(x + 1, y, 7);
                }
                if y + 1 < HEIGHT {
                    if x > 0 {
                        spread(x - 1, y + 1, 3);
                    }
                    spread(x, y + 1, 5);
                    spread(x + 1, y + 1, 1);
                }
            }
        }
    }

    /// Encode the framebuffer into HID feature-report packets (one per 64px
    /// strip). Each is exactly 1024 bytes, ready for `send_feature`.
    pub fn frame_packets(&self) -> Vec<Vec<u8>> {
        let padded_h = HEIGHT; // already a multiple of 8
        let pages = padded_h / 8;
        let mut packets = Vec::new();
        let mut src_x = 0;
        while src_x < WIDTH {
            let strip_w = STRIP_WIDTH.min(WIDTH - src_x);
            let mut pkt = vec![0u8; FRAME_REPORT_SIZE];
            pkt[0] = REPORT_ID;
            pkt[1] = CMD_SCREEN;
            pkt[2] = src_x as u8;
            pkt[3] = 0;
            pkt[4] = strip_w as u8;
            pkt[5] = padded_h as u8;
            for row in 0..HEIGHT {
                let page = row / 8;
                let bit = row % 8;
                for local_col in 0..strip_w {
                    if self.get(src_x + local_col, row) {
                        pkt[6 + local_col * pages + page] |= 1 << bit;
                    }
                }
            }
            packets.push(pkt);
            src_x += strip_w;
        }
        packets
    }
}

/// Brightness output report (level 1..10). 64-byte output report.
pub fn brightness_packet(level: u8) -> Vec<u8> {
    vec![REPORT_ID, CMD_BRIGHTNESS, level.clamp(0, 10)]
}

/// Return-to-SteelSeries-UI output report.
pub fn return_to_ui_packet() -> Vec<u8> {
    vec![REPORT_ID, CMD_RETURN_UI]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn produces_two_strips_of_1024() {
        let fb = Framebuffer::new();
        let packets = fb.frame_packets();
        assert_eq!(packets.len(), 2);
        assert!(packets.iter().all(|p| p.len() == 1024));
        assert_eq!(packets[0][..6], [REPORT_ID, 0x93, 0, 0, 64, 64]);
        assert_eq!(packets[1][..6], [REPORT_ID, 0x93, 64, 0, 64, 64]);
    }

    #[test]
    fn top_left_pixel_lands_in_first_body_byte() {
        let mut fb = Framebuffer::new();
        fb.set(0, 0, true); // row 0, col 0 -> page 0, bit 0 -> body[0] bit 0
        let packets = fb.frame_packets();
        assert_eq!(packets[0][6] & 1, 1);
    }

    #[test]
    fn text_width_matches_advance() {
        assert_eq!(Framebuffer::text_width("AB", 1), 12);
        assert_eq!(Framebuffer::text_width("A", 2), 12);
    }
}
