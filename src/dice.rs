//! Dice and stack drawing.

use std::f32::consts::PI;
use std::sync::atomic::Ordering;

use dicegame::engine::{self, prelude::*};

use crate::game::{TerrFx, Territory, MAX_PLAYERS};
use crate::theme::{darken, lighten, palette, pip_color, with_alpha, Palette, REDUCED_MOTION};
use crate::ui::draw_round_frame;

// pip layouts in face coordinates, verified against a projected 3D mock
pub(crate) const PIPS: [&[(f32, f32)]; 6] = [
    &[(0.5, 0.5)],
    &[(0.28, 0.28), (0.72, 0.72)],
    &[(0.25, 0.25), (0.5, 0.5), (0.75, 0.75)],
    &[(0.28, 0.28), (0.72, 0.28), (0.28, 0.72), (0.72, 0.72)],
    &[
        (0.25, 0.25),
        (0.75, 0.25),
        (0.5, 0.5),
        (0.25, 0.75),
        (0.75, 0.75),
    ],
    &[
        (0.27, 0.22),
        (0.73, 0.22),
        (0.27, 0.5),
        (0.73, 0.5),
        (0.27, 0.78),
        (0.73, 0.78),
    ],
];

fn quad(a: Vec2, b: Vec2, c: Vec2, d: Vec2, col: Color) {
    draw_convex(&[a, b, c, d], col, true);
}

// a quad with per-corner colors: real gradients for the die faces
fn grad_quad(p: [Vec2; 4], c: [Color; 4]) {
    let v = |p: Vec2, c: Color| Vertex {
        position: vec3(p.x, p.y, 0.0),
        color: [
            (c.r * 255.0) as u8,
            (c.g * 255.0) as u8,
            (c.b * 255.0) as u8,
            (c.a * 255.0) as u8,
        ],
    };
    draw_mesh(&Mesh {
        vertices: vec![v(p[0], c[0]), v(p[1], c[1]), v(p[2], c[2]), v(p[3], c[3])],
        indices: vec![0, 1, 2, 0, 2, 3],
        texture: None,
    });
}

// a circle lying on a die face, projected: an ellipse spanned by the face basis
fn draw_pip(c: Vec2, bu: Vec2, bv: Vec2, rho: f32, col: Color) {
    const N: usize = 12;
    let rim: [Vec2; N] = std::array::from_fn(|i| {
        let t = i as f32 / N as f32 * std::f32::consts::TAU;
        c + bu * (rho * t.cos()) + bv * (rho * t.sin())
    });
    draw_fan(c, &rim, col);
}

pub(crate) fn face_val(seed: u32, idx: u32, salt: u32) -> usize {
    let mut h = seed
        .wrapping_mul(747_796_405)
        .wrapping_add(idx.wrapping_mul(2_891_336_453))
        .wrapping_add(salt.wrapping_mul(277_803_737));
    h = (h ^ (h >> 13)).wrapping_mul(1_274_126_177);
    ((h >> 16) % 6) as usize
}

// (x, y) is the bottom front corner of an isometric die of width w.
pub(crate) fn draw_die(x: f32, y: f32, w: f32, seed: u32, idx: u32, pal: &Palette) {
    use std::hash::{Hash, Hasher};
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    let faces = face_val(seed, idx, 1) as u32 * 6 + face_val(seed, idx, 2) as u32;
    (
        faces % 24,
        w.to_bits(),
        pal.mid.bytes(),
        pal.dark.bytes(),
        pal.light.bytes(),
        pal.pip.bytes(),
    )
        .hash(&mut hash);
    let pad = (w * 0.04).max(2.0);
    engine::cached_sprite(
        hash.finish(),
        Rect::new(
            x - w * 0.5 - pad,
            y - w * 1.14 - pad,
            w + pad * 2.0,
            w * 1.14 + pad * 2.0,
        ),
        || draw_die_live(x, y, w, seed, idx, pal),
    );
}

pub(crate) fn draw_die_live(x: f32, y: f32, w: f32, seed: u32, idx: u32, pal: &Palette) {
    let hw = w * 0.5;
    let qh = w * 0.26;
    let ch = w * 0.62;

    let a = vec2(x, y);
    let l = vec2(x - hw, y - qh);
    let r = vec2(x + hw, y - qh);
    let at = vec2(x, y - ch);
    let lt = vec2(x - hw, y - qh - ch);
    let rt = vec2(x + hw, y - qh - ch);
    let tt = vec2(x, y - 2.0 * qh - ch);

    // gradient-shaded faces, lit from the upper left (tuned in a python mock)
    grad_quad(
        [a, l, lt, at],
        [
            darken(pal.mid, 0.88),
            pal.mid,
            lighten(pal.mid, 0.10),
            lighten(pal.mid, 0.04),
        ],
    );
    grad_quad(
        [a, r, rt, at],
        [
            darken(pal.dark, 0.86),
            pal.dark,
            lighten(pal.dark, 0.08),
            darken(pal.dark, 0.96),
        ],
    );
    grad_quad(
        [at, lt, tt, rt],
        [
            lighten(pal.light, 0.10),
            pal.light,
            darken(pal.light, 0.96),
            pal.light,
        ],
    );

    // bevel highlights along the meeting edges give the rounded-edge look;
    // each band starts a little away from the shared corner so the
    // translucent quads never stack
    let bw = w * 0.045;
    let s_a = at + (a - at) * 0.10;
    let s_l = at + (lt - at) * 0.10;
    let s_r = at + (rt - at) * 0.10;
    quad(
        s_a,
        a,
        a + vec2(-bw * 0.7, -bw * 0.35),
        s_a + vec2(-bw * 0.7, -bw * 0.35),
        with_alpha(WHITE, 0.30),
    );
    quad(
        s_l,
        lt,
        lt + vec2(bw * 0.5, bw * 0.8),
        s_l + vec2(bw * 0.5, bw * 0.8),
        with_alpha(WHITE, 0.35),
    );
    quad(
        s_r,
        rt,
        rt + vec2(-bw * 0.5, bw * 0.8),
        s_r + vec2(-bw * 0.5, bw * 0.8),
        with_alpha(WHITE, 0.22),
    );

    // silhouette outline: thin, fully opaque, uniform at the joints
    let ol = (w * 0.035).max(0.8);
    let edge = darken(pal.dark, 0.72);
    for (p, q) in [(a, l), (l, lt), (lt, tt), (tt, rt), (rt, r), (r, a)] {
        draw_line(p.x, p.y, q.x, q.y, ol, edge);
    }
    for p in [a, l, lt, tt, rt, r] {
        draw_circle(p.x, p.y, ol * 0.5, edge);
    }

    // the three visible faces of a real die: distinct and mutually adjacent
    // (each row is a corner of a standard die; opposite faces sum to 7)
    const CORNERS: [[usize; 3]; 8] = [
        [1, 2, 3],
        [1, 3, 5],
        [1, 5, 4],
        [1, 4, 2],
        [6, 2, 4],
        [6, 4, 5],
        [6, 5, 3],
        [6, 3, 2],
    ];
    let h = face_val(seed, idx, 1) as u32 * 6 + face_val(seed, idx, 2) as u32;
    let tri = CORNERS[(h % 8) as usize];
    let rot = ((h / 8) % 3) as usize;
    let (tv, lv, rv) = (tri[rot], tri[(rot + 1) % 3], tri[(rot + 2) % 3]);
    const RHO: f32 = 0.105; // pip radius in face units, from the mock
    let pip_at = |o: Vec2, bu: Vec2, bv: Vec2, u: f32, v: f32, color: Color| {
        let c = o + bu * u + bv * v;
        draw_pip(c, bu, bv, RHO, color);
        // specular glint makes the pip read as a glossy dimple
        let s = c + (bu + bv) * (-RHO * 0.3);
        draw_circle(s.x, s.y, w * 0.022, with_alpha(WHITE, 0.5));
    };
    for &(u, v) in PIPS[lv - 1] {
        pip_at(a, l - a, at - a, u, v, pip_color(pal, pal.mid));
    }
    for &(u, v) in PIPS[rv - 1] {
        pip_at(a, r - a, at - a, u, v, pip_color(pal, pal.dark));
    }
    for &(u, v) in PIPS[tv - 1] {
        pip_at(at, lt - at, rt - at, u, v, pip_color(pal, pal.light));
    }
}

pub(crate) fn draw_ellipse_fan(x: f32, y: f32, rx: f32, ry: f32, col: Color) {
    const N: usize = 32;
    let rim: [Vec2; N] = std::array::from_fn(|i| {
        let a = i as f32 / N as f32 * std::f32::consts::TAU;
        vec2(x + rx * a.cos(), y + ry * a.sin())
    });
    draw_fan(vec2(x, y), &rim, col);
}

pub(crate) const DIE_W: f32 = 21.0;

/// Vertical step between stacked dice, as a fraction of the die width. A
/// little more than the die's side height leaves a sliver of each top face
/// showing, so a tower reads as separate dice rather than one block.
pub(crate) const STACK_STEP: f32 = 0.67;

/// The contact shadow under a stack: (x radius, y radius) in die widths.
pub(crate) fn stack_shadow(two_cols: bool) -> (f32, f32) {
    (if two_cols { 1.1 } else { 0.68 }, 0.26 * 1.35)
}

pub(crate) const STACK_SHADOW: Color = Color::new(0.15, 0.16, 0.30, 0.34);

pub(crate) fn draw_stack(t: &Territory, fx: TerrFx, time: f32) {
    let pal = palette(t.owner);
    let w = DIE_W;
    let ch = w * STACK_STEP;

    // shake rattles side to side, bounce pops the stack up once
    let reduced = REDUCED_MOTION.load(Ordering::Relaxed);
    let sx = if reduced {
        0.0
    } else {
        (time * 55.0).sin() * 4.0 * fx.shake
    };
    let by = if reduced {
        0.0
    } else {
        -(fx.bounce * PI).sin() * 9.0
    };
    let base = t.anchor + vec2(sx, 4.0 + by);

    let n = t.dice as usize;
    let two_cols = n > 4;
    // both columns straddle the anchor so the token stays centered on its land
    let (fdx, bdx) = if two_cols {
        (-w * 0.46, w * 0.46)
    } else {
        (0.0, 0.0)
    };

    // contact shadow, centered under the base diamond of the stack
    let qh = w * 0.26;
    let (rx, ry) = stack_shadow(two_cols);
    draw_ellipse_fan(
        t.anchor.x,
        t.anchor.y + 4.0 - qh * 0.8,
        w * rx,
        w * ry,
        STACK_SHADOW,
    );

    // back-right column for dice 5..8, drawn first so the front column overlaps it
    for i in 0..n.saturating_sub(4) {
        draw_die(
            base.x + bdx,
            base.y - w * 0.26 - i as f32 * ch,
            w,
            t.seed,
            (i + 4) as u32,
            &pal,
        );
    }
    for i in 0..n.min(4) {
        draw_die(
            base.x + fdx,
            base.y - i as f32 * ch,
            w,
            t.seed,
            i as u32,
            &pal,
        );
    }
}

// each player also has a distinct symbol so color is never the only cue
pub(crate) fn draw_symbol(x: f32, y: f32, r: f32, player: usize, col: Color) {
    match player % MAX_PLAYERS {
        0 => draw_circle(x, y, r, col),
        1 => draw_rectangle(x - r * 0.85, y - r * 0.85, r * 1.7, r * 1.7, col),
        2 => draw_poly(x, y, 4, r * 1.15, 0.0, col), // diamond
        3 => draw_poly(x, y, 3, r * 1.25, -90.0, col), // triangle up
        4 => draw_poly(x, y, 6, r * 1.05, 0.0, col), // hexagon
        5 => draw_poly(x, y, 3, r * 1.25, 90.0, col), // triangle down
        6 => draw_poly(x, y, 5, r * 1.15, -90.0, col), // pentagon
        7 => {
            // cross
            draw_rectangle(x - r * 0.35, y - r, r * 0.7, r * 2.0, col);
            draw_rectangle(x - r, y - r * 0.35, r * 2.0, r * 0.7, col);
        }
        _ => draw_star(x, y, r * 1.25, col),
    }
}

// a flat die face: rounded square with the given value's pips
pub(crate) fn draw_face_die(x: f32, y: f32, s: f32, val: usize, pal: &Palette) {
    draw_round_frame(
        x,
        y,
        s,
        s,
        s * 0.22,
        s * 0.06,
        pal.light,
        darken(pal.dark, 0.85),
    );
    for &(u, v) in PIPS[val.clamp(1, 6) - 1] {
        draw_circle(x + u * s, y + v * s, s * 0.09, pip_color(pal, pal.light));
    }
}

pub(crate) fn draw_star(cx: f32, cy: f32, r: f32, col: Color) {
    // Ten alternating vertices form five symmetric points. The triangles
    // share edges without overlapping, keeping translucent stars uniform.
    let points: [Vec2; 10] = std::array::from_fn(|i| {
        let angle = -std::f32::consts::FRAC_PI_2 + i as f32 * std::f32::consts::PI / 5.0;
        let radius = if i % 2 == 0 { r } else { r * 0.47 };
        vec2(cx + radius * angle.cos(), cy + radius * angle.sin())
    });
    draw_fan(vec2(cx, cy), &points, col);
}
