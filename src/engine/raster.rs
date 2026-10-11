use super::{vec2, Camera2D, Color, Font, Vec2};
use std::collections::HashMap;

pub(super) struct Surface {
    pub width: usize,
    pub height: usize,
    pub pixels: Vec<u8>,
    glyphs: HashMap<(char, usize, usize), Vec<u8>>,
}
#[derive(Clone, Copy)]
pub(super) struct Transform {
    pub sx: f32,
    pub sy: f32,
    ox: f32,
    oy: f32,
}
impl Transform {
    pub fn camera(
        camera: Option<&Camera2D>,
        logical_w: usize,
        logical_h: usize,
        w: usize,
        h: usize,
    ) -> Self {
        match camera {
            Some(c) => {
                let sx = c.zoom.x * w as f32 * 0.5;
                let sy = c.zoom.y * h as f32 * 0.5;
                Self {
                    sx,
                    sy,
                    ox: w as f32 * 0.5 - c.target.x * sx,
                    oy: h as f32 * 0.5 - c.target.y * sy,
                }
            }
            None => Self {
                sx: w as f32 / logical_w as f32,
                sy: h as f32 / logical_h as f32,
                ox: 0.0,
                oy: 0.0,
            },
        }
    }
    pub fn point(self, p: Vec2) -> Vec2 {
        vec2(p.x * self.sx + self.ox, p.y * self.sy + self.oy)
    }
}
fn blend(pixel: &mut [u8], color: [u8; 4]) {
    if color[3] == 255 {
        pixel.copy_from_slice(&color);
        return;
    }
    if color[3] == 0 {
        return;
    }
    let a = color[3] as u32;
    let old_a = pixel[3] as u32;
    let out_a = a + (old_a * (255 - a) + 127) / 255;
    for ch in 0..3 {
        pixel[ch] = ((color[ch] as u32 * a
            + (pixel[ch] as u32 * old_a * (255 - a) + 127) / 255
            + out_a / 2)
            / out_a) as u8;
    }
    pixel[3] = out_a as u8;
}
impl Surface {
    pub fn rounded(
        &mut self,
        a: Vec2,
        b: Vec2,
        radius: f32,
        border_width: f32,
        fill: Color,
        border: Color,
    ) {
        let radius = radius
            .max(0.0)
            .min((b.x - a.x) * 0.5)
            .min((b.y - a.y) * 0.5);
        let bw = border_width.max(0.0);
        let distance = |p: Vec2, a: Vec2, b: Vec2, r: f32| {
            let qx = (p.x - (a.x + b.x) * 0.5).abs() - ((b.x - a.x) * 0.5 - r);
            let qy = (p.y - (a.y + b.y) * 0.5).abs() - ((b.y - a.y) * 0.5 - r);
            let dx = qx.max(0.0);
            let dy = qy.max(0.0);
            let outside = if dx == 0.0 || dy == 0.0 {
                dx.max(dy)
            } else {
                (dx * dx + dy * dy).sqrt()
            };
            outside + qx.max(qy).min(0.0) - r
        };
        let (x0, y0, x1, y1) =
            self.bounds(a - vec2(bw + 1.0, bw + 1.0), b + vec2(bw + 1.0, bw + 1.0));
        let fill = fill.bytes();
        let border = border.bytes();
        for y in y0..y1 {
            for x in x0..x1 {
                let p = vec2(x as f32 + 0.5, y as f32 + 0.5);
                let inner = (0.5 - distance(p, a, b, radius)).clamp(0.0, 1.0);
                let pixel = &mut self.pixels[(y * self.width + x) * 4..][..4];
                if bw > 0.0 {
                    let outer = (0.5
                        - distance(p, a - vec2(bw, bw), b + vec2(bw, bw), radius + bw))
                    .clamp(0.0, 1.0);
                    let mut color = border;
                    color[3] = (color[3] as f32 * (outer - inner).max(0.0)) as u8;
                    blend(pixel, color);
                }
                let mut color = fill;
                color[3] = (color[3] as f32 * inner) as u8;
                blend(pixel, color);
            }
        }
    }
    pub fn blit(&mut self, source: &Self) {
        assert_eq!((self.width, self.height), (source.width, source.height));
        for (pixel, color) in self
            .pixels
            .chunks_exact_mut(4)
            .zip(source.pixels.chunks_exact(4))
        {
            blend(pixel, color.try_into().unwrap());
        }
    }
    pub fn new(width: usize, height: usize) -> Self {
        assert!(width > 0 && height > 0 && width <= 4096 && height <= 4096);
        Self {
            width,
            height,
            pixels: vec![0; width * height * 4],
            glyphs: HashMap::new(),
        }
    }
    pub fn clear(&mut self, color: Color) {
        let c = color.bytes();
        for pixel in self.pixels.chunks_exact_mut(4) {
            pixel.copy_from_slice(&c);
        }
    }
    fn bounds(&self, a: Vec2, b: Vec2) -> (usize, usize, usize, usize) {
        (
            a.x.floor().clamp(0.0, self.width as f32) as usize,
            a.y.floor().clamp(0.0, self.height as f32) as usize,
            b.x.ceil().clamp(0.0, self.width as f32) as usize,
            b.y.ceil().clamp(0.0, self.height as f32) as usize,
        )
    }
    pub fn rect(&mut self, a: Vec2, b: Vec2, color: Color) {
        let (x0, y0, x1, y1) = self.bounds(
            vec2(a.x.min(b.x), a.y.min(b.y)),
            vec2(a.x.max(b.x), a.y.max(b.y)),
        );
        let c = color.bytes();
        for y in y0..y1 {
            for x in x0..x1 {
                blend(&mut self.pixels[(y * self.width + x) * 4..][..4], c);
            }
        }
    }
    pub fn ellipse(&mut self, center: Vec2, rx: f32, ry: f32, color: Color) {
        if rx <= 0.0 || ry <= 0.0 {
            return;
        }
        let (x0, y0, x1, y1) = self.bounds(
            center - vec2(rx + 1.0, ry + 1.0),
            center + vec2(rx + 1.0, ry + 1.0),
        );
        let c = color.bytes();
        for y in y0..y1 {
            for x in x0..x1 {
                let dx = (x as f32 + 0.5 - center.x) / rx;
                let dy = (y as f32 + 0.5 - center.y) / ry;
                let coverage =
                    ((1.0 - (dx * dx + dy * dy).sqrt()) * rx.min(ry) + 0.5).clamp(0.0, 1.0);
                if coverage > 0.0 {
                    let mut c = c;
                    c[3] = (c[3] as f32 * coverage) as u8;
                    blend(&mut self.pixels[(y * self.width + x) * 4..][..4], c);
                }
            }
        }
    }
    pub fn triangle(&mut self, mut p: [Vec2; 3], mut colors: [Color; 3]) {
        let cross = |a: Vec2, b: Vec2| a.x * b.y - a.y * b.x;
        let mut area = cross(p[1] - p[0], p[2] - p[0]);
        if !area.is_finite() || area.abs() < 0.0001 {
            return;
        }
        if area < 0.0 {
            p.swap(1, 2);
            colors.swap(1, 2);
            area = -area;
        }
        let lo = vec2(
            p.iter().map(|v| v.x).fold(f32::INFINITY, f32::min),
            p.iter().map(|v| v.y).fold(f32::INFINITY, f32::min),
        );
        let hi = vec2(
            p.iter().map(|v| v.x).fold(f32::NEG_INFINITY, f32::max),
            p.iter().map(|v| v.y).fold(f32::NEG_INFINITY, f32::max),
        );
        let (x0, y0, x1, y1) = self.bounds(lo, hi);
        let edges = [p[2] - p[1], p[0] - p[2], p[1] - p[0]];
        let starts = [p[1], p[2], p[0]];
        let top_left = edges.map(|v| v.y < 0.0 || (v.y == 0.0 && v.x > 0.0));
        let solid = colors[0] == colors[1] && colors[1] == colors[2];
        let bytes = colors.map(Color::bytes);
        for y in y0..y1 {
            let point = vec2(x0 as f32 + 0.5, y as f32 + 0.5);
            let mut weights = [
                cross(edges[0], point - starts[0]),
                cross(edges[1], point - starts[1]),
                cross(edges[2], point - starts[2]),
            ];
            for x in x0..x1 {
                if (0..3).all(|i| weights[i] > 0.0 || (weights[i] == 0.0 && top_left[i])) {
                    let c = if solid {
                        bytes[0]
                    } else {
                        std::array::from_fn(|ch| {
                            ((bytes[0][ch] as f32 * weights[0]
                                + bytes[1][ch] as f32 * weights[1]
                                + bytes[2][ch] as f32 * weights[2])
                                / area)
                                .clamp(0.0, 255.0) as u8
                        })
                    };
                    blend(&mut self.pixels[(y * self.width + x) * 4..][..4], c);
                }
                for i in 0..3 {
                    weights[i] -= edges[i].y;
                }
            }
        }
    }
    pub fn text(
        &mut self,
        text: &str,
        origin: Vec2,
        font: &Font,
        size: f32,
        color: Color,
        t: Transform,
    ) {
        let scale = size / font.size;
        let mut cursor = origin.x;
        let bytes = color.bytes();
        for ch in text.chars() {
            let glyph = font.glyph(ch);
            let pos = t.point(vec2(cursor + glyph.x * scale, origin.y + glyph.y * scale));
            let w = (glyph.w as f32 * scale * t.sx.abs()).round().max(0.0) as usize;
            let h = (glyph.h as f32 * scale * t.sy.abs()).round().max(0.0) as usize;
            if w > 0 && h > 0 && w <= 1024 && h <= 1024 {
                let key = (ch, w, h);
                if self.glyphs.len() > 4096 {
                    self.glyphs.clear();
                }
                let mask = self.glyphs.entry(key).or_insert_with(|| {
                    let mut mask = vec![0; w * h];
                    for y in 0..h {
                        for x in 0..w {
                            let mut sum = 0u32;
                            // Area samples retain narrow strokes when downscaling the embedded masks.
                            for dy in 0..4 {
                                for dx in 0..4 {
                                    let sx = ((x as f32 + (dx as f32 + 0.5) / 4.0) * glyph.w as f32
                                        / w as f32)
                                        as usize;
                                    let sy = ((y as f32 + (dy as f32 + 0.5) / 4.0) * glyph.h as f32
                                        / h as f32)
                                        as usize;
                                    sum += glyph.pixels
                                        [sy.min(glyph.h - 1) * glyph.w + sx.min(glyph.w - 1)]
                                        as u32;
                                }
                            }
                            mask[y * w + x] = (sum / 16) as u8;
                        }
                    }
                    mask
                });
                let px = pos.x.round() as isize;
                let py = pos.y.round() as isize;
                for y in 0..h {
                    let yy = py + y as isize;
                    if yy < 0 || yy >= self.height as isize {
                        continue;
                    }
                    for x in 0..w {
                        let xx = px + x as isize;
                        if xx < 0 || xx >= self.width as isize {
                            continue;
                        }
                        let mut c = bytes;
                        c[3] = ((c[3] as u16 * mask[y * w + x] as u16 + 127) / 255) as u8;
                        blend(
                            &mut self.pixels[(yy as usize * self.width + xx as usize) * 4..][..4],
                            c,
                        );
                    }
                }
            }
            cursor += glyph.advance * scale;
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rounded_frames_do_not_paint_the_border_under_translucent_fills() {
        let mut s = Surface::new(16, 16);
        s.rounded(
            vec2(3.0, 3.0),
            vec2(13.0, 13.0),
            3.0,
            2.0,
            Color::new(1.0, 0.0, 0.0, 0.5),
            Color::new(0.0, 0.0, 1.0, 1.0),
        );
        let center = &s.pixels[(8 * 16 + 8) * 4..][..4];
        assert_eq!(center, &[255, 0, 0, 128]);
        assert_eq!(&s.pixels[(8 * 16 + 1) * 4..][..4], &[0, 0, 255, 255]);
        assert_eq!(s.pixels[3], 0);
    }
    #[test]
    fn triangles_share_edges_without_double_blending() {
        let mut s = Surface::new(4, 4);
        let c = Color::new(1.0, 0.0, 0.0, 0.5);
        s.triangle([vec2(0.0, 0.0), vec2(4.0, 0.0), vec2(4.0, 4.0)], [c; 3]);
        s.triangle([vec2(0.0, 0.0), vec2(4.0, 4.0), vec2(0.0, 4.0)], [c; 3]);
        assert!(s.pixels.chunks_exact(4).all(|p| p[0] == 255 && p[3] == 128));
    }
    #[test]
    fn primitives_clip_at_surface_bounds() {
        let mut s = Surface::new(8, 8);
        s.rect(
            vec2(-100.0, -100.0),
            vec2(100.0, 100.0),
            super::super::WHITE,
        );
        s.ellipse(vec2(-3.0, 4.0), 9.0, 4.0, super::super::BLACK);
        s.triangle(
            [vec2(-8.0, -8.0), vec2(80.0, 0.0), vec2(0.0, 80.0)],
            [super::super::WHITE; 3],
        );
        assert_eq!(s.pixels.len(), 256);
    }
}
