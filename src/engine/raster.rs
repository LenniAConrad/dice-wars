use super::{vec2, Camera2D, Color, Font, Vec2};
use std::cell::Cell;
use std::collections::HashMap;

pub(super) struct Surface {
    pub width: usize,
    pub height: usize,
    pub pixels: Vec<u8>,
    opaque: bool,
    painted_bounds: Cell<(usize, usize, usize, usize)>,
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
    /// Compare bounded tiles so unchanged board pixels need no filtering.
    /// Keeping one prior raster costs at most the renderer's fixed budget.
    pub fn resolve_changed_into(
        &self,
        output: &mut Self,
        previous: &mut Vec<u8>,
    ) -> Vec<(usize, usize, usize, usize)> {
        if previous.len() != self.pixels.len() {
            self.resolve_into(output);
            previous.clone_from(&self.pixels);
            return vec![(0, 0, output.width, output.height)];
        }
        let mut regions = Vec::new();
        for y in (0..self.height).step_by(32) {
            let y1 = (y + 32).min(self.height);
            for x in (0..self.width).step_by(64) {
                let x1 = (x + 64).min(self.width);
                let changed = (y..y1).any(|row| {
                    let a = (row * self.width + x) * 4;
                    let b = (row * self.width + x1) * 4;
                    self.pixels[a..b] != previous[a..b]
                });
                if changed {
                    regions.push((
                        (x * output.width / self.width).saturating_sub(2),
                        (y * output.height / self.height).saturating_sub(2),
                        ((x1 * output.width).div_ceil(self.width) + 2).min(output.width),
                        ((y1 * output.height).div_ceil(self.height) + 2).min(output.height),
                    ));
                }
            }
        }
        if !regions.is_empty() {
            if regions.len() > (self.width.div_ceil(64) * self.height.div_ceil(32)) / 2 {
                self.resolve_into(output);
                regions.clear();
                regions.push((0, 0, output.width, output.height));
            } else {
                self.resolve_regions(output, &regions);
            }
            previous.copy_from_slice(&self.pixels);
        }
        regions
    }
    /// Resolve subpixels once per changed frame, rather than enlarging jagged
    /// geometry in the native blitter. All on-screen frames are opaque.
    pub fn resolve_into(&self, output: &mut Self) {
        if (self.width, self.height) == (output.width, output.height) {
            output.pixels.copy_from_slice(&self.pixels);
            return;
        }
        self.resolve_regions(output, &[(0, 0, output.width, output.height)]);
    }
    fn resolve_regions(&self, output: &mut Self, regions: &[(usize, usize, usize, usize)]) {
        if self.width == output.width * 2 && self.height == output.height * 2 {
            for &(x0, y0, x1, y1) in regions {
                for y in y0..y1 {
                    let stride = self.width * 4;
                    let a = &self.pixels[y * 2 * stride + x0 * 8..y * 2 * stride + x1 * 8];
                    let b =
                        &self.pixels[(y * 2 + 1) * stride + x0 * 8..(y * 2 + 1) * stride + x1 * 8];
                    let dst = &mut output.pixels
                        [(y * output.width + x0) * 4..(y * output.width + x1) * 4];
                    for ((a, b), dst) in a
                        .chunks_exact(8)
                        .zip(b.chunks_exact(8))
                        .zip(dst.chunks_exact_mut(4))
                    {
                        let p = [
                            u32::from_le_bytes(a[..4].try_into().unwrap()),
                            u32::from_le_bytes(a[4..].try_into().unwrap()),
                            u32::from_le_bytes(b[..4].try_into().unwrap()),
                            u32::from_le_bytes(b[4..].try_into().unwrap()),
                        ];
                        let rb = p.iter().map(|p| p & 0x00ff00ff).sum::<u32>();
                        let ga = p.iter().map(|p| (p >> 8) & 0x00ff00ff).sum::<u32>();
                        dst.copy_from_slice(
                            &((((rb + 0x00020002) >> 2) & 0x00ff00ff)
                                | ((((ga + 0x00020002) >> 2) & 0x00ff00ff) << 8))
                                .to_le_bytes(),
                        );
                    }
                }
            }
            return;
        }
        // Pixel-center bilinear filtering also handles budget-limited sizes
        // and odd window dimensions without nearest-neighbor blocks.
        let axis = |src: usize, dst: usize| -> Vec<(usize, usize, u32)> {
            (0..dst)
                .map(|i| {
                    let p = ((i as f32 + 0.5) * src as f32 / dst as f32 - 0.5)
                        .clamp(0.0, (src - 1) as f32);
                    let lo = p.floor() as usize;
                    (
                        lo,
                        (lo + 1).min(src - 1),
                        ((p - lo as f32) * 256.0 + 0.5) as u32,
                    )
                })
                .collect()
        };
        let xs = axis(self.width, output.width);
        let ys = axis(self.height, output.height);
        let lerp = |a: u32, b: u32, weight: u32| {
            let rb =
                ((a & 0x00ff00ff) * (256 - weight) + (b & 0x00ff00ff) * weight + 0x00800080) >> 8;
            let ga = (((a >> 8) & 0x00ff00ff) * (256 - weight)
                + ((b >> 8) & 0x00ff00ff) * weight
                + 0x00800080)
                >> 8;
            (rb & 0x00ff00ff) | ((ga & 0x00ff00ff) << 8)
        };
        for &(x_start, y_start, x_end, y_end) in regions {
            for (y, &(y0, y1, fy)) in ys.iter().enumerate().take(y_end).skip(y_start) {
                let row0 = &self.pixels[y0 * self.width * 4..(y0 + 1) * self.width * 4];
                let row1 = &self.pixels[y1 * self.width * 4..(y1 + 1) * self.width * 4];
                for (x, &(x0, x1, fx)) in xs.iter().enumerate().take(x_end).skip(x_start) {
                    let read = |row: &[u8], x: usize| {
                        u32::from_le_bytes(row[x * 4..x * 4 + 4].try_into().unwrap())
                    };
                    let top = lerp(read(row0, x0), read(row0, x1), fx);
                    let bottom = lerp(read(row1, x0), read(row1, x1), fx);
                    let dst = (y * output.width + x) * 4;
                    output.pixels[dst..dst + 4]
                        .copy_from_slice(&lerp(top, bottom, fy).to_le_bytes());
                }
            }
        }
    }
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
        if source.opaque {
            self.pixels.copy_from_slice(&source.pixels);
            self.opaque = true;
            self.painted_bounds.set((0, 0, self.width, self.height));
            return;
        }
        let (x0, y0, x1, y1) = source.painted_bounds.get();
        self.include_bounds(x0, y0, x1, y1);
        for y in y0..y1 {
            let start = (y * self.width + x0) * 4;
            let end = (y * self.width + x1) * 4;
            for (pixel, color) in self.pixels[start..end]
                .chunks_exact_mut(4)
                .zip(source.pixels[start..end].chunks_exact(4))
            {
                blend(pixel, color.try_into().unwrap());
            }
        }
    }
    pub fn blit_at(&mut self, source: &Self, position: Vec2) {
        let px = position.x.round() as isize;
        let py = position.y.round() as isize;
        let x0 = (-px).max(0).min(source.width as isize) as usize;
        let y0 = (-py).max(0).min(source.height as isize) as usize;
        let x1 = (self.width as isize - px).clamp(0, source.width as isize) as usize;
        let y1 = (self.height as isize - py).clamp(0, source.height as isize) as usize;
        if x0 >= x1 || y0 >= y1 {
            return;
        }
        self.include_bounds(
            (px + x0 as isize) as usize,
            (py + y0 as isize) as usize,
            (px + x1 as isize) as usize,
            (py + y1 as isize) as usize,
        );
        for y in y0..y1 {
            let dst = ((py + y as isize) as usize * self.width + (px + x0 as isize) as usize) * 4;
            let src = (y * source.width + x0) * 4;
            let len = (x1 - x0) * 4;
            for (pixel, color) in self.pixels[dst..dst + len]
                .chunks_exact_mut(4)
                .zip(source.pixels[src..src + len].chunks_exact(4))
            {
                blend(pixel, color.try_into().unwrap());
            }
        }
    }
    pub fn new(width: usize, height: usize) -> Self {
        assert!(width > 0 && height > 0 && width <= 4096 && height <= 4096);
        Self {
            width,
            height,
            pixels: vec![0; width * height * 4],
            opaque: false,
            painted_bounds: Cell::new((width, height, 0, 0)),
            glyphs: HashMap::new(),
        }
    }
    pub fn clear(&mut self, color: Color) {
        let c = color.bytes();
        self.opaque = c[3] == 255;
        self.painted_bounds.set(if c[3] == 0 {
            (self.width, self.height, 0, 0)
        } else {
            (0, 0, self.width, self.height)
        });
        for pixel in self.pixels.chunks_exact_mut(4) {
            pixel.copy_from_slice(&c);
        }
    }
    fn bounds(&self, a: Vec2, b: Vec2) -> (usize, usize, usize, usize) {
        let bounds = (
            a.x.floor().clamp(0.0, self.width as f32) as usize,
            a.y.floor().clamp(0.0, self.height as f32) as usize,
            b.x.ceil().clamp(0.0, self.width as f32) as usize,
            b.y.ceil().clamp(0.0, self.height as f32) as usize,
        );
        self.include_bounds(bounds.0, bounds.1, bounds.2, bounds.3);
        bounds
    }
    fn include_bounds(&self, x0: usize, y0: usize, x1: usize, y1: usize) {
        if x0 >= x1 || y0 >= y1 {
            return;
        }
        let (a, b, c, d) = self.painted_bounds.get();
        self.painted_bounds
            .set((a.min(x0), b.min(y0), c.max(x1), d.max(y1)));
    }
    pub fn rect(&mut self, a: Vec2, b: Vec2, color: Color) {
        let (x0, y0, x1, y1) = self.bounds(
            vec2(a.x.min(b.x), a.y.min(b.y)),
            vec2(a.x.max(b.x), a.y.max(b.y)),
        );
        let c = color.bytes();
        if c[3] == 255 && (x0, y0, x1, y1) == (0, 0, self.width, self.height) {
            self.opaque = true;
        }
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
                self.bounds(pos, pos + vec2(w as f32 + 1.0, h as f32 + 1.0));
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
    fn partial_resolve_matches_full_filter_at_tile_and_window_edges() {
        for (sw, sh, ow, oh) in [(256, 128, 128, 64), (191, 99, 257, 131), (191, 99, 117, 67)] {
            let mut source = Surface::new(sw, sh);
            source.clear(super::super::WHITE);
            let mut partial = Surface::new(ow, oh);
            let mut prior = Vec::new();
            assert_eq!(
                source.resolve_changed_into(&mut partial, &mut prior),
                vec![(0, 0, ow, oh)]
            );
            assert!(source
                .resolve_changed_into(&mut partial, &mut prior)
                .is_empty());
            for (x, y) in [(63, 31), (64, 32), (sw - 1, sh - 1), (0, 0)] {
                let i = (y * sw + x) * 4;
                source.pixels[i..i + 4].copy_from_slice(&[5, 71, 143, 255]);
                source.resolve_changed_into(&mut partial, &mut prior);
                let mut full = Surface::new(ow, oh);
                source.resolve_into(&mut full);
                assert_eq!(partial.pixels, full.pixels);
            }
        }
    }
    #[test]
    fn sprite_blits_clip_and_transparent_cache_clear_removes_old_bounds() {
        let mut sprite = Surface::new(4, 4);
        sprite.clear(super::super::WHITE);
        let mut dst = Surface::new(4, 4);
        dst.blit_at(&sprite, vec2(-2.0, 2.0));
        assert_eq!(dst.pixels[(2 * 4) * 4 + 3], 255);
        assert_eq!(dst.pixels[(2 * 4 + 2) * 4 + 3], 0);
        sprite.clear(Color::new(0.0, 0.0, 0.0, 0.0));
        dst.clear(super::super::BLACK);
        dst.blit(&sprite);
        assert!(dst.pixels.chunks_exact(4).all(|p| p == [0, 0, 0, 255]));
    }
    #[test]
    fn resolve_preserves_flat_colors_and_averages_subpixel_edges() {
        let mut hi = Surface::new(4, 4);
        hi.clear(super::super::WHITE);
        hi.rect(vec2(0.0, 0.0), vec2(1.0, 4.0), super::super::BLACK);
        let mut lo = Surface::new(2, 2);
        hi.resolve_into(&mut lo);
        assert_eq!(&lo.pixels[..4], &[128, 128, 128, 255]);
        assert_eq!(&lo.pixels[4..8], &[255, 255, 255, 255]);
    }
    #[test]
    fn filtered_scaling_does_not_create_nearest_neighbor_blocks() {
        let mut source = Surface::new(2, 2);
        source.clear(super::super::WHITE);
        source.rect(vec2(0.0, 0.0), vec2(1.0, 2.0), super::super::BLACK);
        let mut output = Surface::new(3, 3);
        source.resolve_into(&mut output);
        assert_eq!(&output.pixels[..4], &[0, 0, 0, 255]);
        assert_eq!(&output.pixels[4..8], &[128, 128, 128, 255]);
        assert_eq!(&output.pixels[8..12], &[255, 255, 255, 255]);
    }
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
