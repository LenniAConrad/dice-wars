use super::{vec2, Camera2D, Color, Font, Vec2};
use std::cell::Cell;
use std::collections::HashMap;

/// (x0, y0, x1, y1) in pixels, end-exclusive.
type PixelRect = (usize, usize, usize, usize);

pub(super) struct Surface {
    pub width: usize,
    pub height: usize,
    pub pixels: Vec<u8>,
    opaque: bool,
    painted_bounds: Cell<(usize, usize, usize, usize)>,
    glyphs: HashMap<(usize, char, usize, usize), Vec<u8>>,
    // A clear is recorded, not written: an opaque full-surface blit right
    // after it (the usual start of a frame) makes the fill unnecessary.
    pending: Option<([u8; 4], PixelRect)>,
    // Drawing is limited to this rectangle (x0, y0, x1, y1); partial layer
    // updates use it to repaint one region of a cached picture.
    clip: (usize, usize, usize, usize),
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
    if old_a == 255 {
        // The on-screen canvas is opaque: keep its alpha and avoid three
        // divisions by a variable output alpha for every translucent pixel.
        for ch in 0..3 {
            pixel[ch] = ((color[ch] as u32 * a + pixel[ch] as u32 * (255 - a) + 127) / 255) as u8;
        }
        return;
    }
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
        &mut self,
        output: &mut Self,
        previous: &mut Vec<u8>,
    ) -> Vec<(usize, usize, usize, usize)> {
        self.settle();
        if (self.width, self.height) == (output.width, output.height) {
            // Native-resolution raster: the presented frame is the previous
            // one, so compare against it and copy only the changed tiles.
            previous.clear();
            previous.shrink_to_fit();
            return self.copy_changed_into(output);
        }
        if previous.len() != self.pixels.len() {
            self.resolve_into(output);
            previous.clone_from(&self.pixels);
            return vec![(0, 0, output.width, output.height)];
        }
        let columns = self.width.div_ceil(64);
        let mut dirty = vec![false; columns * self.height.div_ceil(32)];
        let stride = self.width * 4;
        for (y, (current, prior)) in self
            .pixels
            .chunks_exact(stride)
            .zip(previous.chunks_exact_mut(stride))
            .enumerate()
        {
            // Most animation rows are identical. One wide comparison replaces
            // up to dozens of tiny comparisons per unchanged row.
            if current == prior {
                continue;
            }
            for (x, (current, prior)) in current
                .chunks(64 * 4)
                .zip(prior.chunks_mut(64 * 4))
                .enumerate()
            {
                let tile = y / 32 * columns + x;
                if dirty[tile] || current != prior {
                    dirty[tile] = true;
                    prior.copy_from_slice(current);
                }
            }
        }
        let mut regions = Vec::new();
        for y in (0..self.height).step_by(32) {
            let y1 = (y + 32).min(self.height);
            let mut column = 0;
            while column < columns {
                if dirty[y / 32 * columns + column] {
                    let x = column * 64;
                    while column < columns && dirty[y / 32 * columns + column] {
                        column += 1;
                    }
                    let x1 = (column * 64).min(self.width);
                    regions.push((
                        (x * output.width / self.width).saturating_sub(2),
                        (y * output.height / self.height).saturating_sub(2),
                        ((x1 * output.width).div_ceil(self.width) + 2).min(output.width),
                        ((y1 * output.height).div_ceil(self.height) + 2).min(output.height),
                    ));
                } else {
                    column += 1;
                }
            }
        }
        if !regions.is_empty() {
            if dirty.iter().filter(|&&tile| tile).count() > dirty.len() / 2 {
                self.resolve_into(output);
                regions.clear();
                regions.push((0, 0, output.width, output.height));
            } else {
                self.resolve_regions(output, &regions);
            }
        }
        regions
    }
    fn copy_changed_into(&self, output: &mut Self) -> Vec<(usize, usize, usize, usize)> {
        const TW: usize = 64;
        const TH: usize = 32;
        let columns = self.width.div_ceil(TW);
        let mut dirty = vec![false; columns * self.height.div_ceil(TH)];
        let stride = self.width * 4;
        for (y, (current, shown)) in self
            .pixels
            .chunks_exact(stride)
            .zip(output.pixels.chunks_exact_mut(stride))
            .enumerate()
        {
            if current == shown {
                continue;
            }
            for (x, (current, shown)) in current
                .chunks(TW * 4)
                .zip(shown.chunks_mut(TW * 4))
                .enumerate()
            {
                if current != shown {
                    dirty[y / TH * columns + x] = true;
                    shown.copy_from_slice(current);
                }
            }
        }
        let count = dirty.iter().filter(|&&d| d).count();
        if count > dirty.len() / 2 {
            return vec![(0, 0, self.width, self.height)];
        }
        let mut regions = Vec::new();
        for ty in 0..self.height.div_ceil(TH) {
            let mut column = 0;
            while column < columns {
                if dirty[ty * columns + column] {
                    let x = column * TW;
                    while column < columns && dirty[ty * columns + column] {
                        column += 1;
                    }
                    regions.push((
                        x,
                        ty * TH,
                        (column * TW).min(self.width),
                        ((ty + 1) * TH).min(self.height),
                    ));
                } else {
                    column += 1;
                }
            }
        }
        regions
    }
    /// Resolve subpixels once per changed frame, rather than enlarging jagged
    /// geometry in the native blitter. All on-screen frames are opaque.
    pub fn resolve_into(&mut self, output: &mut Self) {
        self.settle();
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
        self.settle();
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
            // Pixels whose centers lie half a pixel inside the shape (the
            // rounded rectangle inset by 0.5, radius shrunk by 0.5) are pure
            // fill: only the ends of each row need the distance test.
            let py = y as f32 + 0.5;
            let (cx, cy) = ((a.x + b.x) * 0.5, (a.y + b.y) * 0.5);
            let (hw, hh) = ((b.x - a.x) * 0.5 - 0.5, (b.y - a.y) * 0.5 - 0.5);
            let ri = (radius - 0.5).max(0.0);
            let dy = (py - cy).abs();
            let (mut inner0, mut inner1) = (x1, x1);
            if hw > 0.0 && dy <= hh {
                let e = if dy <= hh - ri {
                    hw
                } else {
                    let k = dy - (hh - ri);
                    (hw - ri) + (ri * ri - k * k).max(0.0).sqrt()
                } - 0.001;
                inner0 = ((cx - e - 0.5).ceil() as isize).clamp(x0 as isize, x1 as isize) as usize;
                inner1 = (((cx + e - 0.5).floor() as isize) + 1).clamp(inner0 as isize, x1 as isize)
                    as usize;
                let row =
                    &mut self.pixels[(y * self.width + inner0) * 4..(y * self.width + inner1) * 4];
                if fill[3] == 255 {
                    for px in row.chunks_exact_mut(4) {
                        px.copy_from_slice(&fill);
                    }
                } else if fill[3] > 0 {
                    for px in row.chunks_exact_mut(4) {
                        blend(px, fill);
                    }
                }
            }
            for x in (x0..inner0).chain(inner1..x1) {
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
        debug_assert!(source.pending.is_none(), "blit from an unsettled surface");
        if source.opaque {
            self.pending = None;
            self.pixels.copy_from_slice(&source.pixels);
            self.opaque = true;
            self.painted_bounds.set((0, 0, self.width, self.height));
            return;
        }
        self.settle();
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
        self.settle();
        let px = position.x.round() as isize;
        let py = position.y.round() as isize;
        let (cx0, cy0, cx1, cy1) = self.clip;
        let x0 = (cx0 as isize - px).max(0).min(source.width as isize) as usize;
        let y0 = (cy0 as isize - py).max(0).min(source.height as isize) as usize;
        let x1 = (cx1 as isize - px).clamp(0, source.width as isize) as usize;
        let y1 = (cy1 as isize - py).clamp(0, source.height as isize) as usize;
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
            pending: None,
            clip: (0, 0, width, height),
        }
    }
    /// Limits drawing to a rectangle, or lifts the limit with None.
    pub fn set_clip(&mut self, clip: Option<(usize, usize, usize, usize)>) {
        self.clip = match clip {
            Some((x0, y0, x1, y1)) => (
                x0.min(self.width),
                y0.min(self.height),
                x1.min(self.width),
                y1.min(self.height),
            ),
            None => (0, 0, self.width, self.height),
        };
    }
    fn clipped(&self) -> bool {
        self.clip != (0, 0, self.width, self.height)
    }
    pub fn clear(&mut self, color: Color) {
        if self.clipped() {
            // a clipped clear repaints just the clip rectangle
            self.settle();
            let (x0, y0, x1, y1) = self.clip;
            self.pending = Some((color.bytes(), self.clip));
            self.include_bounds(x0, y0, x1, y1);
            return;
        }
        let c = color.bytes();
        // Only what was painted since the last clear can differ from a
        // transparent clear; an opaque clear covers everything.
        let region = if c[3] == 0 && self.pending.is_none_or(|(p, _)| p[3] == 0) {
            let (x0, y0, x1, y1) = self.painted_bounds.get();
            let previous = self.pending.map(|(_, r)| r);
            match previous {
                Some((a, b, cc, d)) if a < cc && b < d => {
                    (x0.min(a), y0.min(b), x1.max(cc), y1.max(d))
                }
                _ => (x0, y0, x1, y1),
            }
        } else {
            (0, 0, self.width, self.height)
        };
        self.pending = Some((c, region));
        self.opaque = c[3] == 255;
        self.painted_bounds.set(if c[3] == 0 {
            (self.width, self.height, 0, 0)
        } else {
            (0, 0, self.width, self.height)
        });
    }
    /// Writes a recorded clear. Every drawing call starts with this.
    pub fn settle(&mut self) {
        let Some((c, (x0, y0, x1, y1))) = self.pending.take() else {
            return;
        };
        if x0 >= x1 || y0 >= y1 {
            return;
        }
        let word = u32::from_ne_bytes(c);
        for y in y0..y1 {
            let row = &mut self.pixels[(y * self.width + x0) * 4..(y * self.width + x1) * 4];
            // SAFETY: u32 has no invalid bit patterns; align_to_mut only
            // returns the correctly aligned middle part as u32.
            let (head, words, tail) = unsafe { row.align_to_mut::<u32>() };
            for px in head.chunks_exact_mut(4).chain(tail.chunks_exact_mut(4)) {
                px.copy_from_slice(&c);
            }
            if head.len() % 4 == 0 {
                words.fill(word);
            } else {
                // misaligned rows: fall back to byte copies
                for px in row.chunks_exact_mut(4) {
                    px.copy_from_slice(&c);
                }
            }
        }
    }
    fn bounds(&self, a: Vec2, b: Vec2) -> (usize, usize, usize, usize) {
        let (cx0, cy0, cx1, cy1) = self.clip;
        let bounds = (
            a.x.floor().clamp(cx0 as f32, cx1 as f32) as usize,
            a.y.floor().clamp(cy0 as f32, cy1 as f32) as usize,
            b.x.ceil().clamp(cx0 as f32, cx1 as f32) as usize,
            b.y.ceil().clamp(cy0 as f32, cy1 as f32) as usize,
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
        self.settle();
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
        self.settle();
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
    /// A triangle without anti-aliasing: for tessellations whose shared
    /// edges must not let the background bleed through.
    pub fn triangle(&mut self, p: [Vec2; 3], colors: [Color; 3]) {
        self.triangle_aa(p, colors, [false; 3]);
    }

    /// A triangle whose edges listed in `aa` (the edge opposite vertex i)
    /// get analytic coverage anti-aliasing; the others keep the exact
    /// top-left fill rule, so triangles sharing them tile without seams.
    pub fn triangle_aa(&mut self, mut p: [Vec2; 3], mut colors: [Color; 3], mut aa: [bool; 3]) {
        self.settle();
        let cross = |a: Vec2, b: Vec2| a.x * b.y - a.y * b.x;
        let mut area = cross(p[1] - p[0], p[2] - p[0]);
        if !area.is_finite() || area.abs() < 0.0001 {
            return;
        }
        if area < 0.0 {
            p.swap(1, 2);
            colors.swap(1, 2);
            aa.swap(1, 2);
            area = -area;
        }
        let edges = [p[2] - p[1], p[0] - p[2], p[1] - p[0]];
        let starts = [p[1], p[2], p[0]];
        let top_left = edges.map(|v| v.y < 0.0 || (v.y == 0.0 && v.x > 0.0));
        // Per edge: 1/length turns the edge function into a pixel distance.
        let inv_len: [f32; 3] = std::array::from_fn(|i| {
            if aa[i] {
                1.0 / edges[i].length().max(1e-6)
            } else {
                0.0
            }
        });
        let any_aa = aa.iter().any(|&a| a);
        let pad = if any_aa { 1.0 } else { 0.0 };
        let lo = vec2(
            p.iter().map(|v| v.x).fold(f32::INFINITY, f32::min) - pad,
            p.iter().map(|v| v.y).fold(f32::INFINITY, f32::min) - pad,
        );
        let hi = vec2(
            p.iter().map(|v| v.x).fold(f32::NEG_INFINITY, f32::max) + pad,
            p.iter().map(|v| v.y).fold(f32::NEG_INFINITY, f32::max) + pad,
        );
        let (x0, y0, x1, y1) = self.bounds(lo, hi);
        let solid = colors[0] == colors[1] && colors[1] == colors[2];
        let bytes = colors.map(Color::bytes);
        // Coverage of the pixel whose edge functions are `w`: hard edges
        // are in or out, anti-aliased edges ramp over one pixel.
        let coverage = |w: [f32; 3]| -> f32 {
            let mut c = 1.0f32;
            for i in 0..3 {
                if aa[i] {
                    c *= (w[i] * inv_len[i] + 0.5).clamp(0.0, 1.0);
                } else if !(w[i] > 0.0 || (w[i] == 0.0 && top_left[i])) {
                    return 0.0;
                }
            }
            c
        };
        for y in y0..y1 {
            let point = vec2(x0 as f32 + 0.5, y as f32 + 0.5);
            let mut weights = [
                cross(edges[0], point - starts[0]),
                cross(edges[1], point - starts[1]),
                cross(edges[2], point - starts[2]),
            ];
            // Each edge function is linear along the row, so solve for the
            // covered span (anti-aliased edges moved out by half a pixel) and
            // test only that, plus a one-pixel margin.
            let (mut xs, mut xe) = (x0 as f32, x1 as f32);
            for i in 0..3 {
                let ey = edges[i].y;
                let w = if aa[i] {
                    weights[i] + 0.5 / inv_len[i]
                } else {
                    weights[i]
                };
                if ey > 0.0 {
                    xe = xe.min(x0 as f32 + (w / ey).floor() + 2.0);
                } else if ey < 0.0 {
                    xs = xs.max(x0 as f32 + (w / ey).floor() - 1.0);
                } else if !(w > 0.0 || (w == 0.0 && top_left[i])) {
                    xe = xs;
                }
            }
            // Written negated so that NaN spans are skipped too.
            #[allow(clippy::neg_cmp_op_on_partial_ord)]
            if !(xs < xe) {
                continue;
            }
            let (xs, xe) = (
                xs.max(x0 as f32) as usize,
                (xe.min(x1 as f32)).max(0.0) as usize,
            );
            if xs >= xe {
                continue;
            }
            for i in 0..3 {
                weights[i] -= edges[i].y * (xs - x0) as f32;
            }
            let at = |x: usize| -> [f32; 3] {
                std::array::from_fn(|i| weights[i] - edges[i].y * (x - xs) as f32)
            };
            let row = y * self.width;
            if solid {
                // Flat fills: a triangle row is one contiguous run. Blend the
                // anti-aliased fringes, then fill the fully covered middle.
                let mut a = xs;
                while a < xe {
                    let c = coverage(at(a));
                    if c >= 1.0 {
                        break;
                    }
                    if c > 0.0 {
                        let mut px = bytes[0];
                        px[3] = (px[3] as f32 * c + 0.5) as u8;
                        blend(&mut self.pixels[(row + a) * 4..][..4], px);
                    }
                    a += 1;
                }
                let mut b = xe;
                while b > a {
                    let c = coverage(at(b - 1));
                    if c >= 1.0 {
                        break;
                    }
                    if c > 0.0 {
                        let mut px = bytes[0];
                        px[3] = (px[3] as f32 * c + 0.5) as u8;
                        blend(&mut self.pixels[(row + b - 1) * 4..][..4], px);
                    }
                    b -= 1;
                }
                if bytes[0][3] == 255 {
                    for px in self.pixels[(row + a) * 4..(row + b) * 4].chunks_exact_mut(4) {
                        px.copy_from_slice(&bytes[0]);
                    }
                } else {
                    for px in self.pixels[(row + a) * 4..(row + b) * 4].chunks_exact_mut(4) {
                        blend(px, bytes[0]);
                    }
                }
                continue;
            }
            for x in xs..xe {
                let c = if any_aa {
                    coverage(weights)
                } else if (0..3).all(|i| weights[i] > 0.0 || (weights[i] == 0.0 && top_left[i])) {
                    1.0
                } else {
                    0.0
                };
                if c > 0.0 {
                    // barycentric weights from the (clamped) edge functions
                    let wsum = area;
                    let mut px: [u8; 4] = std::array::from_fn(|ch| {
                        ((bytes[0][ch] as f32 * weights[0]
                            + bytes[1][ch] as f32 * weights[1]
                            + bytes[2][ch] as f32 * weights[2])
                            / wsum)
                            .clamp(0.0, 255.0) as u8
                    });
                    if c < 1.0 {
                        px[3] = (px[3] as f32 * c + 0.5) as u8;
                    }
                    blend(&mut self.pixels[(row + x) * 4..][..4], px);
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
        self.settle();
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
                let key = (font.id(), ch, w, h);
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
                    if yy < self.clip.1 as isize || yy >= self.clip.3 as isize {
                        continue;
                    }
                    for x in 0..w {
                        let xx = px + x as isize;
                        if xx < self.clip.0 as isize || xx >= self.clip.2 as isize {
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
    fn opaque_blending_preserves_coverage_for_every_alpha() {
        for alpha in 0..=255u32 {
            let mut pixel = [17, 139, 241, 255];
            blend(&mut pixel, [243, 83, 7, alpha as u8]);
            let expected: [u8; 3] = std::array::from_fn(|ch| {
                (([17, 139, 241][ch] * (255 - alpha) + [243, 83, 7][ch] * alpha + 127) / 255) as u8
            });
            assert_eq!(pixel, [expected[0], expected[1], expected[2], 255]);
        }
    }
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
                assert_eq!(prior, source.pixels);
            }
            source.clear(super::super::WHITE);
            source.resolve_changed_into(&mut partial, &mut prior);
            let mut full = Surface::new(ow, oh);
            source.resolve_into(&mut full);
            assert_eq!(partial.pixels, full.pixels);
            assert_eq!(prior, source.pixels);
        }
    }
    #[test]
    fn sprite_blits_clip_and_transparent_cache_clear_removes_old_bounds() {
        let mut sprite = Surface::new(4, 4);
        sprite.clear(super::super::WHITE);
        sprite.settle();
        let mut dst = Surface::new(4, 4);
        dst.blit_at(&sprite, vec2(-2.0, 2.0));
        assert_eq!(dst.pixels[(2 * 4) * 4 + 3], 255);
        assert_eq!(dst.pixels[(2 * 4 + 2) * 4 + 3], 0);
        sprite.clear(Color::new(0.0, 0.0, 0.0, 0.0));
        sprite.settle();
        dst.clear(super::super::BLACK);
        dst.blit(&sprite);
        dst.settle();
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
    fn anti_aliased_edges_blend_while_shared_edges_stay_seamless() {
        // a diagonal edge gets partial coverage...
        let mut s = Surface::new(8, 8);
        s.clear(super::super::BLACK);
        let white = [super::super::WHITE; 3];
        s.triangle_aa(
            [vec2(0.0, 0.0), vec2(8.0, 0.0), vec2(0.0, 8.0)],
            white,
            [true; 3],
        );
        let on_edge = s.pixels[(3 * 8 + 4) * 4]; // pixel (4, 3) straddles x + y = 8
        assert!(on_edge > 40 && on_edge < 215, "edge pixel {on_edge}");
        assert_eq!(s.pixels[(8 + 1) * 4], 255);
        assert_eq!(s.pixels[(7 * 8 + 7) * 4], 0);
        // ...while two halves of a square split along a hard diagonal leave
        // no trace of the black background between them
        let mut s = Surface::new(8, 8);
        s.clear(super::super::BLACK);
        s.triangle_aa(
            [vec2(1.0, 1.0), vec2(7.0, 1.0), vec2(7.0, 7.0)],
            white,
            [true, false, true], // the diagonal (opposite vertex 1) stays hard
        );
        s.triangle_aa(
            [vec2(1.0, 1.0), vec2(7.0, 7.0), vec2(1.0, 7.0)],
            white,
            [true, true, false],
        );
        for (x, y) in [(2, 2), (3, 3), (5, 5), (2, 5), (5, 2)] {
            assert_eq!(s.pixels[(y * 8 + x) * 4], 255, "seam at {x},{y}");
        }
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
