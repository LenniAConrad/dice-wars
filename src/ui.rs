//! Virtual canvas scaling, text, rounded panels and shared widgets (buttons,
//! segmented rows, toggles, the toolbar).
//!
//! Everything is drawn on a WIN_W x WIN_H virtual canvas which is scaled
//! (letterboxed) to the actual window; the mouse is mapped back into it.

use std::sync::atomic::Ordering;

use dicegame::engine;
use dicegame::engine::prelude::*;

use crate::dice::draw_ellipse_fan;
use crate::game::HUMAN;
use crate::menu::COL_L;
use crate::settings::{save_settings, Settings};
use crate::sound::Snd;
use crate::theme::{
    dark_mode, darken, mix, on_color, palette, with_alpha, ACCENT, BORDER, CARD_EDGE, COLORBLIND,
    DARK, INK, INK_ON_FILL, REDUCED_MOTION, SRF,
};

// virtual canvas; scaled to the real window with letterboxing
pub(crate) const WIN_W: f32 = 930.0;

pub(crate) const WIN_H: f32 = 820.0;

fn view_scale() -> f32 {
    (screen_width() / WIN_W)
        .min(screen_height() / WIN_H)
        .max(0.01)
}

pub(crate) fn view_camera() -> Camera2D {
    let sw = screen_width().max(1.0);
    let sh = screen_height().max(1.0);
    let k = view_scale();
    Camera2D {
        target: vec2(WIN_W * 0.5, WIN_H * 0.5),
        zoom: vec2(2.0 * k / sw, 2.0 * k / sh),
        ..Default::default()
    }
}

pub(crate) fn mouse_virtual() -> Vec2 {
    let (mx, my) = mouse_position();
    let sw = screen_width().max(1.0);
    let sh = screen_height().max(1.0);
    let k = view_scale();
    vec2(
        (mx - sw * 0.5) / k + WIN_W * 0.5,
        (my - sh * 0.5) / k + WIN_H * 0.5,
    )
}

pub(crate) struct Ui {
    pub(crate) font: Font,
}

impl Ui {
    // rasterize at the real on-screen pixel size so text stays crisp at any scale
    pub(crate) fn text(&self, s: &str, x: f32, y: f32, size: f32, col: Color) {
        let k = view_scale();
        draw_text_ex(
            s,
            x,
            y,
            TextParams {
                font: Some(&self.font),
                font_size: (size * k).round().max(8.0) as u16,
                font_scale: 1.0 / k,
                color: col,
                ..Default::default()
            },
        );
    }

    pub(crate) fn width(&self, s: &str, size: f32) -> f32 {
        let k = view_scale();
        measure_text(
            s,
            Some(&self.font),
            (size * k).round().max(8.0) as u16,
            1.0 / k,
        )
        .width
    }

    pub(crate) fn text_centered(&self, s: &str, cx: f32, y: f32, size: f32, col: Color) {
        self.text(s, cx - self.width(s, size) * 0.5, y, size, col);
    }

    // Fit status copy into its lane without crossing adjacent controls.
    pub(crate) fn text_fitted(&self, s: &str, x: f32, y: f32, size: f32, width: f32, col: Color) {
        let mut text = s.to_string();
        if self.width(&text, size) > width {
            while !text.is_empty() && self.width(&(text.clone() + "…"), size) > width {
                text.pop();
            }
            text.push('…');
        }
        self.text(&text, x, y, size, col);
    }

    pub(crate) fn text_outlined(
        &self,
        s: &str,
        x: f32,
        y: f32,
        size: f32,
        fill: Color,
        outline: Color,
    ) {
        for (dx, dy) in [
            (-1.6, 0.0),
            (1.6, 0.0),
            (0.0, -1.6),
            (0.0, 1.6),
            (-1.2, -1.2),
            (1.2, -1.2),
            (-1.2, 1.2),
            (1.2, 1.2),
        ] {
            self.text(s, x + dx, y + dy, size, outline);
        }
        self.text(s, x, y, size, fill);
    }
}

pub(crate) fn ease_out(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t).powi(3)
}

// Native coverage masks give controls smooth corners without triangle-fan seams.
pub(crate) fn draw_round_rect(x: f32, y: f32, w: f32, h: f32, radius: f32, color: Color) {
    engine::rounded_frame(x, y, w, h, radius, 0.0, color, color);
}

// Outline weight is a theme token: call sites state relative emphasis and the
// modern style renders it as a light hairline rather than a heavy cartoon stroke.
const STROKE_SCALE: f32 = 0.6;

// A diffuse drop shadow built from a few translucent layers instead of a hard
// offset slab, so cards appear to float slightly above the surface.
pub(crate) fn soft_shadow(x: f32, y: f32, w: f32, h: f32, radius: f32, strength: f32) {
    use std::hash::{Hash, Hasher};
    // Quantized so animated strengths reuse a handful of cached sprites.
    let strength = (strength.clamp(0.0, 4.0) * 10.0).round() / 10.0;
    if strength <= 0.0 {
        return;
    }
    let (c, k) = if dark_mode() {
        (Color::new(0.0, 0.0, 0.02, 1.0), 2.6)
    } else {
        (Color::new(0.10, 0.14, 0.32, 1.0), 1.0)
    };
    let layers = move || {
        for (grow, dy, a) in [(7.0, 6.0, 0.030), (3.5, 3.5, 0.045), (1.0, 1.5, 0.060)] {
            draw_round_rect(
                x - grow,
                y - grow + dy,
                w + grow * 2.0,
                h + grow * 2.0,
                radius + grow,
                with_alpha(c, (a * k * strength).min(1.0)),
            );
        }
    };
    // Shadows depend only on size and theme, so each shape is rasterized once
    // and blitted afterwards; translucent layers are the costly part.
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    (
        "shadow",
        w.to_bits(),
        h.to_bits(),
        radius.to_bits(),
        (strength * 10.0) as u32,
        dark_mode(),
    )
        .hash(&mut hash);
    engine::cached_sprite(
        hash.finish(),
        Rect::new(x - 8.0, y - 2.0, w + 16.0, h + 16.0),
        layers,
    );
}

pub(crate) fn draw_round_frame(
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    radius: f32,
    border_width: f32,
    fill: Color,
    border: Color,
) {
    let stroke = if border_width > 0.0 {
        (border_width * STROKE_SCALE).max(1.0)
    } else {
        0.0
    };
    engine::rounded_frame(x, y, w, h, radius, stroke, fill, border);
}

pub(crate) fn r_icon(i: usize) -> Rect {
    Rect::new(WIN_W - 284.0 + i as f32 * 44.0, 18.0, 40.0, 28.0)
}

pub(crate) fn handle_icon_bar_click(settings: &mut Settings, snd: &mut Vec<Snd>, m: Vec2) -> bool {
    for i in 0..6 {
        if !r_icon(i).contains(m) {
            continue;
        }
        match i {
            // Sound cycles full → half volume → off.
            0 => {
                if settings.muted {
                    settings.muted = false;
                    settings.quiet = false;
                } else if settings.quiet {
                    settings.muted = true;
                } else {
                    settings.quiet = true;
                }
            }
            1 => settings.chances = !settings.chances,
            2 => {
                settings.colorblind = !settings.colorblind;
                COLORBLIND.store(settings.colorblind, Ordering::Relaxed);
            }
            3 => settings.speed = if settings.speed > 1.5 { 1.0 } else { 2.0 },
            4 => {
                settings.dark = !settings.dark;
                DARK.store(settings.dark, Ordering::Relaxed);
            }
            _ => {
                settings.reduced_motion = !settings.reduced_motion;
                REDUCED_MOTION.store(settings.reduced_motion, Ordering::Relaxed);
            }
        }
        save_settings(settings);
        snd.push(Snd::Click);
        return true;
    }
    false
}

pub(crate) fn draw_icon_bar(settings: &Settings, ui: &Ui, mp: Vec2) {
    let slash = |r: Rect| {
        draw_line(
            r.x + 8.0,
            r.y + r.h - 5.0,
            r.x + r.w - 8.0,
            r.y + 5.0,
            2.0,
            palette(2).dark,
        );
    };
    for i in 0..6 {
        let r = r_icon(i);
        let hov = r.contains(mp);
        draw_round_frame(
            r.x,
            r.y,
            r.w,
            r.h,
            8.0,
            1.5,
            control_fill(SRF(), hov),
            if hov { ACCENT() } else { CARD_EDGE() },
        );
        let (cx, cy) = (r.x + r.w * 0.5, r.y + r.h * 0.5);
        match i {
            0 => {
                // speaker: rounded body, cone, and sound-wave arcs
                draw_round_rect(cx - 11.0, cy - 3.5, 5.5, 7.0, 2.0, INK());
                draw_triangle(
                    vec2(cx - 7.0, cy),
                    vec2(cx - 0.5, cy - 7.0),
                    vec2(cx - 0.5, cy + 7.0),
                    INK(),
                );
                if !settings.muted {
                    let arcs: &[(f32, f32)] = if settings.quiet {
                        &[(4.5, 0.9)]
                    } else {
                        &[(4.5, 0.9), (8.0, 0.55)]
                    };
                    for &(rr, aa) in arcs {
                        let n = 6;
                        for k in 0..n {
                            let t0 = -0.85 + 1.7 * k as f32 / n as f32;
                            let t1 = -0.85 + 1.7 * (k + 1) as f32 / n as f32;
                            draw_line(
                                cx + 1.0 + rr * t0.cos(),
                                cy + rr * t0.sin(),
                                cx + 1.0 + rr * t1.cos(),
                                cy + rr * t1.sin(),
                                1.5,
                                with_alpha(INK(), aa),
                            );
                        }
                    }
                } else {
                    slash(r);
                }
            }
            1 => {
                ui.text_centered("%", cx, cy + 6.0, 18.0, INK());
                if !settings.chances {
                    slash(r);
                }
            }
            2 => {
                // eye; crossed out = colorblind assist on
                draw_ellipse_fan(cx, cy, 10.0, 5.5, INK());
                draw_circle(cx, cy, 3.4, SRF());
                draw_circle(cx, cy, 1.6, INK());
                if settings.colorblind {
                    slash(r);
                }
            }
            3 => {
                ui.text_centered(
                    if settings.speed > 1.5 { "2x" } else { "1x" },
                    cx,
                    cy + 6.0,
                    15.0,
                    INK(),
                );
            }
            4 => {
                // moon: theme toggle
                let f = if hov {
                    mix(SRF(), palette(HUMAN).fill, 0.35)
                } else {
                    SRF()
                };
                draw_circle(cx + 1.0, cy, 7.0, INK());
                draw_circle(cx + 4.5, cy - 2.5, 5.6, f);
            }
            _ => {
                ui.text_centered("FX", cx, cy + 5.0, 13.0, INK());
                if settings.reduced_motion {
                    slash(r);
                }
            }
        }
        if hov {
            let tip = match i {
                0 => {
                    if settings.muted {
                        "Sound off · M"
                    } else if settings.quiet {
                        "Sound 50% · M"
                    } else {
                        "Sound 100% · M"
                    }
                }
                1 => {
                    if settings.chances {
                        "Win chances on"
                    } else {
                        "Win chances off"
                    }
                }
                2 => {
                    if settings.colorblind {
                        "Color assist on · F9"
                    } else {
                        "Color assist off · F9"
                    }
                }
                3 => {
                    if settings.speed > 1.5 {
                        "Game speed 2x"
                    } else {
                        "Game speed 1x"
                    }
                }
                4 => {
                    if settings.dark {
                        "Dark theme · F10"
                    } else {
                        "Light theme · F10"
                    }
                }
                _ => {
                    if settings.reduced_motion {
                        "Reduced motion · F8"
                    } else {
                        "Motion on · F8"
                    }
                }
            };
            let width = ui.width(tip, 12.0) + 20.0;
            // Right-aligned under the toolbar, so tips never cover the title.
            let _ = cx;
            let x = r_icon(5).x + r_icon(5).w - width;
            soft_shadow(x, 54.0, width, 26.0, 8.0, 1.0);
            draw_round_frame(x, 54.0, width, 26.0, 8.0, 1.0, SRF(), CARD_EDGE());
            ui.text(tip, x + 10.0, 72.0, 12.0, INK());
        }
    }
}

pub(crate) fn control_fill(fill: Color, hovered: bool) -> Color {
    if !hovered {
        return fill;
    }
    let surface = SRF();
    if fill.r == surface.r && fill.g == surface.g && fill.b == surface.b {
        // White cannot be lightened. Give neutral controls a visible accent
        // tint in either theme, while retaining contrast with their labels.
        mix(
            fill,
            palette(HUMAN).fill,
            if dark_mode() { 0.16 } else { 0.30 },
        )
    } else if on_color(fill).r > 0.5 {
        darken(fill, 0.85)
    } else {
        mix(fill, WHITE, 0.25)
    }
}

pub(crate) fn menu_button(r: Rect, fill: Color, edge: Color, hovered: bool) {
    let f = control_fill(fill, hovered);
    soft_shadow(r.x, r.y, r.w, r.h, 14.0, 1.0);
    draw_round_frame(
        r.x,
        r.y,
        r.w,
        r.h,
        14.0,
        2.5,
        f,
        if hovered { ACCENT() } else { edge },
    );
}

// one segmented control: label left, options in a single pill container
pub(crate) fn seg_row(
    ui: &Ui,
    m: Vec2,
    live: bool,
    label: &str,
    items: &[(Rect, &str, bool)],
    size: f32,
) {
    let y = items[0].0.y;
    let h = items[0].0.h;
    ui.text(
        label,
        COL_L - 230.0,
        y + h * 0.5 + 4.0,
        12.0,
        with_alpha(INK(), 0.72),
    );
    let last = &items[items.len() - 1].0;
    let x0 = items[0].0.x;
    draw_round_frame(
        x0,
        y,
        last.x + last.w - x0,
        h,
        h * 0.5,
        2.0,
        SRF(),
        CARD_EDGE(),
    );
    for (i, (r, txt, sel)) in items.iter().enumerate() {
        if i > 0 && !sel && !items[i - 1].2 {
            draw_line(r.x, y + 8.0, r.x, y + h - 8.0, 1.5, CARD_EDGE());
        }
        let hov = live && r.contains(m);
        if *sel {
            draw_round_frame(
                r.x + 2.5,
                y + 2.5,
                r.w - 5.0,
                h - 5.0,
                (h - 5.0) * 0.5,
                2.0,
                control_fill(palette(HUMAN).fill, hov),
                BORDER(),
            );
        } else if hov {
            draw_round_rect(
                r.x + 2.5,
                y + 2.5,
                r.w - 5.0,
                h - 5.0,
                (h - 5.0) * 0.5,
                control_fill(SRF(), hov),
            );
        }
        ui.text_centered(
            txt,
            r.x + r.w * 0.5,
            y + h * 0.5 + size * 0.33,
            size,
            if *sel { INK_ON_FILL } else { INK() },
        );
    }
}

// checkbox row shared by the menu card and both lobbies
pub(crate) fn draw_toggle(r: Rect, label: &str, on: bool, ui: &Ui, m: Vec2) {
    let hov = r.contains(m);
    let (bx, by) = (r.x, r.y + 2.0);
    if hov {
        draw_round_rect(
            r.x - 4.0,
            r.y - 2.0,
            r.w + 8.0,
            r.h + 4.0,
            6.0,
            control_fill(SRF(), true),
        );
    }
    draw_round_frame(
        bx,
        by,
        20.0,
        20.0,
        6.0,
        2.0,
        control_fill(if on { palette(HUMAN).mid } else { SRF() }, hov),
        if hov { ACCENT() } else { CARD_EDGE() },
    );
    if on {
        // round caps at the elbow and tips keep the check seamless
        draw_line(
            bx + 4.5,
            by + 10.0,
            bx + 8.5,
            by + 14.5,
            2.6,
            on_color(palette(HUMAN).mid),
        );
        draw_line(
            bx + 8.5,
            by + 14.5,
            bx + 15.5,
            by + 5.5,
            2.6,
            on_color(palette(HUMAN).mid),
        );
        for (px, py) in [(4.5, 10.0), (8.5, 14.5), (15.5, 5.5)] {
            draw_circle(bx + px, by + py, 1.3, on_color(palette(HUMAN).mid));
        }
    }
    ui.text(
        label,
        r.x + 28.0,
        r.y + 17.0,
        13.0,
        with_alpha(INK(), if hov { 0.95 } else { 0.72 }),
    );
}
