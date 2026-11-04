//! Vector exports of the board and the logo for documentation.
//!
//! These mirror the raster drawing in `dice.rs` and `draw_game.rs` (die
//! geometry, pips, map colors) with compact, reusable SVG symbols, so the
//! README art is a few kilobytes, sharp at any zoom, and always matches the
//! game.

use std::collections::HashSet;
use std::fmt::Write as _;

use dicegame::engine::prelude::*;

use crate::board::{cell_pos, neighbor_of, COLS, HEX, ROWS};
use crate::dice::{face_val, DIE_W, PIPS};
use crate::game::Game;
use crate::theme::{
    darken, lighten, map_fill, mix, on_color, palette, pip_color, Palette, MAP_EDGE, PLAYER_BASE,
};

fn hex(c: Color) -> String {
    let [r, g, b, _] = c.bytes();
    format!("#{r:02x}{g:02x}{b:02x}")
}

fn pt(p: Vec2) -> String {
    format!("{:.1} {:.1}", p.x, p.y)
}

struct DieShape {
    a: Vec2,
    l: Vec2,
    r: Vec2,
    at: Vec2,
    lt: Vec2,
    rt: Vec2,
    tt: Vec2,
}

// The same isometric corners as `draw_die_live`, for a die whose bottom
// front corner sits at (x, y).
fn die_shape(x: f32, y: f32, w: f32) -> DieShape {
    let (hw, qh, ch) = (w * 0.5, w * 0.26, w * 0.62);
    DieShape {
        a: vec2(x, y),
        l: vec2(x - hw, y - qh),
        r: vec2(x + hw, y - qh),
        at: vec2(x, y - ch),
        lt: vec2(x - hw, y - qh - ch),
        rt: vec2(x + hw, y - qh - ch),
        tt: vec2(x, y - 2.0 * qh - ch),
    }
}

fn poly(out: &mut String, points: &[Vec2], fill: &str, extra: &str) {
    let d: Vec<String> = points.iter().map(|&p| pt(p)).collect();
    let _ = write!(out, r#"<path d="M{}Z" fill="{fill}"{extra}/>"#, d.join("L"));
}

// Projected pips for one face: ellipses spanned by the face basis, written
// as a single path of polygons.
fn pips(out: &mut String, o: Vec2, bu: Vec2, bv: Vec2, value: usize, color: Color) {
    const RHO: f32 = 0.105;
    const N: usize = 12;
    let mut d = String::new();
    for &(u, v) in PIPS[value - 1] {
        let c = o + bu * u + bv * v;
        for k in 0..N {
            let t = k as f32 / N as f32 * std::f32::consts::TAU;
            let p = c + bu * (RHO * t.cos()) + bv * (RHO * t.sin());
            d.push(if k == 0 { 'M' } else { 'L' });
            d.push_str(&pt(p));
        }
        d.push('Z');
    }
    let _ = write!(out, r#"<path d="{d}" fill="{}"/>"#, hex(color));
    // specular glints make the pips read as glossy dimples
    let w = bu.length() / 0.564; // face edge to die width
    for &(u, v) in PIPS[value - 1] {
        let g = o + bu * u + bv * v + (bu + bv) * (-RHO * 0.3);
        let _ = write!(
            out,
            r##"<circle cx="{:.1}" cy="{:.1}" r="{:.2}" fill="#fff" fill-opacity=".5"/>"##,
            g.x,
            g.y,
            w * 0.022
        );
    }
}

// The visible faces (top, left, right) of die `idx` in a stack, exactly as
// the raster renderer picks them.
fn die_faces(seed: u32, idx: u32) -> (usize, usize, usize) {
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
    (tri[rot], tri[(rot + 1) % 3], tri[(rot + 2) % 3])
}

// A die body (faces, bevels, outline) without pips. `shade` adds the face
// gradients used by the large logo; board dice use flat faces.
fn die_body(out: &mut String, x: f32, y: f32, w: f32, pal: &Palette, shade: Option<&str>) {
    let s = die_shape(x, y, w);
    let (left, right, top) = match shade {
        Some(id) => (
            format!("url(#{id}l)"),
            format!("url(#{id}r)"),
            format!("url(#{id}t)"),
        ),
        None => (hex(pal.mid), hex(pal.dark), hex(pal.light)),
    };
    let edge = hex(darken(pal.dark, 0.72));
    let stroke = format!(
        r#" stroke="{edge}" stroke-width="{:.2}" stroke-linejoin="round""#,
        (w * 0.035).max(0.8)
    );
    poly(out, &[s.a, s.l, s.lt, s.at], &left, "");
    poly(out, &[s.a, s.r, s.rt, s.at], &right, "");
    poly(out, &[s.at, s.lt, s.tt, s.rt], &top, "");
    // bevel highlights along the shared edges
    let bw = w * 0.045;
    let sa = s.at + (s.a - s.at) * 0.10;
    let sl = s.at + (s.lt - s.at) * 0.10;
    let sr = s.at + (s.rt - s.at) * 0.10;
    let off_a = vec2(-bw * 0.7, -bw * 0.35);
    let off_l = vec2(bw * 0.5, bw * 0.8);
    let off_r = vec2(-bw * 0.5, bw * 0.8);
    poly(
        out,
        &[sa, s.a, s.a + off_a, sa + off_a],
        "#fff",
        r#" fill-opacity=".3""#,
    );
    poly(
        out,
        &[sl, s.lt, s.lt + off_l, sl + off_l],
        "#fff",
        r#" fill-opacity=".35""#,
    );
    poly(
        out,
        &[sr, s.rt, s.rt + off_r, sr + off_r],
        "#fff",
        r#" fill-opacity=".22""#,
    );
    poly(out, &[s.a, s.l, s.lt, s.tt, s.rt, s.r], "none", &stroke);
}

// One face's pips; side 0 = left, 1 = right, 2 = top.
fn die_pips(out: &mut String, x: f32, y: f32, w: f32, pal: &Palette, side: usize, value: usize) {
    let s = die_shape(x, y, w);
    match side {
        0 => pips(
            out,
            s.a,
            s.l - s.a,
            s.at - s.a,
            value,
            pip_color(pal, pal.mid),
        ),
        1 => pips(
            out,
            s.a,
            s.r - s.a,
            s.at - s.a,
            value,
            pip_color(pal, pal.dark),
        ),
        _ => pips(
            out,
            s.at,
            s.lt - s.at,
            s.rt - s.at,
            value,
            pip_color(pal, pal.light),
        ),
    }
}

fn gradient(out: &mut String, id: &str, from: Vec2, to: Vec2, a: Color, b: Color) {
    let _ = write!(
        out,
        r#"<linearGradient id="{id}" gradientUnits="userSpaceOnUse" x1="{:.1}" y1="{:.1}" x2="{:.1}" y2="{:.1}"><stop offset="0" stop-color="{}"/><stop offset="1" stop-color="{}"/></linearGradient>"#,
        from.x,
        from.y,
        to.x,
        to.y,
        hex(a),
        hex(b)
    );
}

/// The yellow game die, matching `assets/icon.png`.
pub fn logo() -> String {
    let base = PLAYER_BASE[0];
    let pal = Palette {
        fill: mix(base, WHITE, 0.46),
        mid: base,
        dark: darken(base, 0.78),
        light: mix(base, WHITE, 0.62),
        pip: Color::new(0.22, 0.23, 0.38, 1.0),
    };
    let (w, x, y) = (176.0, 128.0, 232.0);
    let s = die_shape(x, y, w);
    let mut defs = String::new();
    gradient(
        &mut defs,
        "gl",
        s.a,
        s.lt,
        darken(pal.mid, 0.88),
        lighten(pal.mid, 0.10),
    );
    gradient(
        &mut defs,
        "gr",
        s.a,
        s.rt,
        darken(pal.dark, 0.86),
        lighten(pal.dark, 0.08),
    );
    gradient(
        &mut defs,
        "gt",
        s.at,
        s.tt,
        lighten(pal.light, 0.10),
        darken(pal.light, 0.96),
    );
    let mut body = String::new();
    die_body(&mut body, x, y, w, &pal, Some("g"));
    for (side, value) in [(0, 2), (1, 3), (2, 1)] {
        die_pips(&mut body, x, y, w, &pal, side, value);
    }
    format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="30 14 196 228" width="196" height="228" role="img" aria-label="Dice Wars: a yellow die"><title>Dice Wars</title><defs>{defs}</defs>{body}</svg>
"#
    )
}

/// The current board: territories, coastlines, dice stacks and count badges,
/// cropped to the map with no interface around it.
pub fn board(game: &Game) -> String {
    let mut body = String::new();
    let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);

    // one filled path per territory: hex subpaths share edges without seams
    for (t, terr) in game.terrs.iter().enumerate() {
        let mut d = String::new();
        for i in 0..COLS * ROWS {
            if game.cell_terr[i] != t as i32 {
                continue;
            }
            let p = cell_pos(i);
            for k in 0..6 {
                let a = (60.0 * k as f32 + 30.0).to_radians();
                let q = p + HEX * vec2(a.cos(), a.sin());
                x0 = x0.min(q.x);
                x1 = x1.max(q.x);
                y0 = y0.min(q.y);
                y1 = y1.max(q.y);
                d.push(if k == 0 { 'M' } else { 'L' });
                d.push_str(&pt(q));
            }
            d.push('Z');
        }
        let _ = write!(
            body,
            r#"<path d="{d}" fill="{}" stroke="{}" stroke-width=".7"/>"#,
            hex(map_fill(terr.owner)),
            hex(map_fill(terr.owner))
        );
    }

    // borders: soft between lands, strong along the coast (as on screen)
    let (mut inner, mut coast) = (String::new(), String::new());
    for i in 0..COLS * ROWS {
        let t = game.cell_terr[i];
        if t < 0 {
            continue;
        }
        let pos = cell_pos(i);
        for dir in 0..6 {
            let n = neighbor_of(i % COLS, i / COLS, dir);
            if n.is_some_and(|n| game.cell_terr[n] == t) {
                continue;
            }
            let is_coast = n.is_none_or(|n| game.cell_terr[n] < 0);
            // shared borders are listed by both sides; keep one copy
            if !is_coast && n.is_some_and(|n| n < i) {
                continue;
            }
            let a0 = (dir as f32 * 60.0 - 30.0).to_radians();
            let a1 = (dir as f32 * 60.0 + 30.0).to_radians();
            let p0 = pos + HEX * vec2(a0.cos(), a0.sin());
            let p1 = pos + HEX * vec2(a1.cos(), a1.sin());
            let target = if is_coast { &mut coast } else { &mut inner };
            let _ = write!(target, "M{}L{}", pt(p0), pt(p1));
        }
    }
    let _ = write!(
        body,
        r#"<path d="{inner}" fill="none" stroke="{}" stroke-width="1.6" stroke-linecap="round"/><path d="{coast}" fill="none" stroke="{}" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round"/>"#,
        hex(mix(MAP_EDGE, WHITE, 0.38)),
        hex(MAP_EDGE)
    );

    // dice stacks back to front, laid out like `draw_stack`
    let mut order: Vec<usize> = (0..game.terrs.len()).collect();
    order.sort_by(|&a, &b| game.terrs[a].anchor.y.total_cmp(&game.terrs[b].anchor.y));
    let w = DIE_W;
    let (ch, qh) = (w * 0.62, w * 0.26);
    // Every die is a <use> of a per-player body plus three pip symbols, all
    // defined once at the origin.
    let mut defs = String::new();
    let mut defined = HashSet::new();
    let mut place =
        |body: &mut String, x: f32, y: f32, owner: usize, faces: (usize, usize, usize)| {
            let pal = palette(owner);
            let (tv, lv, rv) = faces;
            let body_id = format!("b{owner}");
            if defined.insert(body_id.clone()) {
                let _ = write!(defs, r#"<g id="{body_id}">"#);
                die_body(&mut defs, 0.0, 0.0, w, &pal, None);
                defs.push_str("</g>");
            }
            let _ = write!(
                body,
                r##"<use xlink:href="#{body_id}" x="{x:.1}" y="{y:.1}"/>"##
            );
            for (side, value) in [(0, lv), (1, rv), (2, tv)] {
                let id = format!("p{owner}{side}{value}");
                if defined.insert(id.clone()) {
                    let _ = write!(defs, r#"<g id="{id}">"#);
                    die_pips(&mut defs, 0.0, 0.0, w, &pal, side, value);
                    defs.push_str("</g>");
                }
                let _ = write!(body, r##"<use xlink:href="#{id}" x="{x:.1}" y="{y:.1}"/>"##);
            }
        };
    for &t in &order {
        let terr = &game.terrs[t];
        let n = terr.dice as usize;
        let two_cols = n > 4;
        let (fdx, bdx) = if two_cols {
            (-w * 0.46, w * 0.46)
        } else {
            (0.0, 0.0)
        };
        let base = terr.anchor + vec2(0.0, 4.0);
        let _ = write!(
            body,
            r##"<ellipse cx="{:.1}" cy="{:.1}" rx="{:.1}" ry="{:.1}" fill="#262950" fill-opacity=".26"/>"##,
            terr.anchor.x,
            terr.anchor.y + 4.0 - qh * 0.8,
            if two_cols { w * 1.04 } else { w * 0.62 },
            qh * 1.3
        );
        for i in 0..n.saturating_sub(4) {
            let (x, y) = (base.x + bdx, base.y - w * 0.26 - i as f32 * ch);
            place(
                &mut body,
                x,
                y,
                terr.owner,
                die_faces(terr.seed, (i + 4) as u32),
            );
            y0 = y0.min(y - w * 1.2);
        }
        for i in 0..n.min(4) {
            let (x, y) = (base.x + fdx, base.y - i as f32 * ch);
            place(&mut body, x, y, terr.owner, die_faces(terr.seed, i as u32));
            y0 = y0.min(y - w * 1.2);
        }
    }
    // dice-count badges
    for terr in &game.terrs {
        let pal = palette(terr.owner);
        let (bx, by) = (terr.anchor.x + 16.0, terr.anchor.y + 13.0);
        let _ = write!(
            body,
            r##"<circle cx="{bx:.1}" cy="{by:.1}" r="10" fill="#fff"/><circle cx="{bx:.1}" cy="{by:.1}" r="8.8" fill="{}"/><text x="{bx:.1}" y="{:.1}" fill="{}">{}</text>"##,
            hex(pal.dark),
            by + 4.5,
            hex(on_color(pal.dark)),
            terr.dice
        );
    }

    let pad = 6.0;
    let (vx, vy) = (x0 - pad, y0 - pad);
    let (vw, vh) = (x1 - x0 + pad * 2.0, y1 - y0 + pad * 2.0 + 12.0);
    format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" viewBox="{vx:.0} {vy:.0} {vw:.0} {vh:.0}" width="{:.0}" height="{:.0}" role="img" aria-label="A Dice Wars board in the middle of a game"><title>Dice Wars board</title><style>text{{font:bold 13px "DejaVu Sans",Verdana,sans-serif;text-anchor:middle}}</style><defs>{defs}</defs>{body}</svg>
"#,
        vw * 1.0,
        vh * 1.0
    )
}

#[cfg(test)]
mod tests {
    use crate::board::gen_map_local;
    use crate::game::{Difficulty, TeamCfg};
    use crate::svg;
    use crate::testutil::GLOBALS;

    #[test]
    fn svg_exports_are_complete_documents() {
        let _guard = GLOBALS.lock().unwrap_or_else(|e| e.into_inner());
        let logo = svg::logo();
        assert!(logo.starts_with("<svg") && logo.trim_end().ends_with("</svg>"));
        let g = gen_map_local(31337, 6, 1, Difficulty::Normal, TeamCfg::default());
        let board = svg::board(&g);
        assert!(board.starts_with("<svg") && board.trim_end().ends_with("</svg>"));
        let dice: u32 = g.terrs.iter().map(|t| t.dice).sum();
        // one body and three pip uses per die
        assert_eq!(board.matches("<use ").count() as u32, dice * 4);
        assert!(
            board.len() < 400_000,
            "board SVG grew to {} bytes",
            board.len()
        );
    }
}
