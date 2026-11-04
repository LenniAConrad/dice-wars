//! Virtual canvas scaling, text, rounded panels and shared widgets (buttons,
//! segmented rows, toggles, the toolbar).
//!
//! Everything is drawn on a WIN_W x WIN_H virtual canvas which is scaled
//! (letterboxed) to the actual window; the mouse is mapped back into it.

use std::sync::atomic::{AtomicBool, Ordering};

use dicegame::engine::{self, prelude::*};

use crate::game::HUMAN;
use crate::settings::{save_settings, Settings};
use crate::sound::Snd;
use crate::theme::{
    dark_mode, darken, mix, on_color, palette, set_theme, theme_index, with_alpha, ACCENT, BG,
    BORDER, CARD_EDGE, COLORBLIND, INK, INK_ON_FILL, REDUCED_MOTION, SRF, THEMES,
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

#[allow(clippy::too_many_arguments)]
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

// ---------------------------------------------------------------- toolbar
// Two controls in every screen: a quick sound button and a Settings button
// that opens one labelled panel for every preference.

static SETTINGS_OPEN: AtomicBool = AtomicBool::new(false);

pub(crate) fn r_sound() -> Rect {
    Rect::new(WIN_W - 176.0, 16.0, 40.0, 30.0)
}

pub(crate) fn r_settings() -> Rect {
    Rect::new(WIN_W - 128.0, 16.0, 108.0, 30.0)
}

pub(crate) fn settings_open() -> bool {
    SETTINGS_OPEN.load(Ordering::Relaxed)
}

/// Whether `m` is on the toolbar or the open settings panel, so screens can
/// ignore clicks that belong to them.
pub(crate) fn toolbar_contains(m: Vec2) -> bool {
    r_sound().contains(m) || r_settings().contains(m) || (settings_open() && r_panel().contains(m))
}

const PANEL_W: f32 = 312.0;
const ROW_Y: f32 = 150.0; // first option row, below the theme swatches
const ROW_H: f32 = 44.0;
const ROWS: [(&str, &[&str]); 5] = [
    ("SOUND", &["FULL", "HALF", "OFF"]),
    ("GAME SPEED", &["1x", "2x", "4x"]),
    ("WIN CHANCES", &["SHOW", "HIDE"]),
    ("COLOR ASSIST", &["OFF", "ON"]),
    ("MOTION", &["FULL", "LESS"]),
];

fn r_panel() -> Rect {
    Rect::new(
        WIN_W - 20.0 - PANEL_W,
        54.0,
        PANEL_W,
        ROW_Y - 54.0 + ROWS.len() as f32 * ROW_H + 44.0,
    )
}

fn r_theme(i: usize) -> Rect {
    let p = r_panel();
    Rect::new(p.x + 20.0 + i as f32 * 40.0, p.y + 46.0, 32.0, 32.0)
}

fn r_option(row: usize, k: usize) -> Rect {
    let p = r_panel();
    let n = ROWS[row].1.len() as f32;
    let (x0, w) = (p.x + 128.0, PANEL_W - 128.0 - 18.0);
    Rect::new(
        x0 + k as f32 * w / n,
        ROW_Y + row as f32 * ROW_H,
        w / n,
        30.0,
    )
}

fn option_selected(settings: &Settings, row: usize) -> usize {
    match row {
        0 => {
            if settings.muted {
                2
            } else if settings.quiet {
                1
            } else {
                0
            }
        }
        1 => match settings.speed {
            s if s > 3.0 => 2,
            s if s > 1.5 => 1,
            _ => 0,
        },
        2 => (!settings.chances) as usize,
        3 => settings.colorblind as usize,
        _ => settings.reduced_motion as usize,
    }
}

fn set_option(settings: &mut Settings, row: usize, k: usize) {
    match row {
        0 => {
            settings.muted = k == 2;
            settings.quiet = k == 1;
        }
        1 => settings.speed = [1.0, 2.0, 4.0][k.min(2)],
        2 => settings.chances = k == 0,
        3 => {
            settings.colorblind = k == 1;
            COLORBLIND.store(settings.colorblind, Ordering::Relaxed);
        }
        _ => {
            settings.reduced_motion = k == 1;
            REDUCED_MOTION.store(settings.reduced_motion, Ordering::Relaxed);
        }
    }
}

/// Keyboard shortcuts for the toolbar: M sound, F8 motion, F9 color assist,
/// F10 next theme.
pub(crate) fn toolbar_shortcut(settings: &mut Settings, snd: &mut Vec<Snd>, text_editing: bool) {
    if modifier_down() {
        return; // Ctrl/Alt/Cmd combinations belong to the system or other actions
    }
    let changed = if is_key_pressed(KeyCode::F10) {
        settings.theme = (settings.theme + 1) % THEMES.len();
        set_theme(settings.theme);
        true
    } else if is_key_pressed(KeyCode::F9) {
        set_option(settings, 3, (!settings.colorblind) as usize);
        true
    } else if is_key_pressed(KeyCode::F8) {
        set_option(settings, 4, (!settings.reduced_motion) as usize);
        true
    } else if !text_editing && is_key_pressed(KeyCode::M) {
        set_option(settings, 0, (option_selected(settings, 0) + 1) % 3);
        true
    } else {
        false
    };
    if changed {
        save_settings(settings);
        snd.push(Snd::Click);
    }
}

/// Toolbar and settings-panel clicks. Returns true when the click was used
/// here; the caller then hides it from the screen underneath.
pub(crate) fn handle_toolbar_click(settings: &mut Settings, snd: &mut Vec<Snd>, m: Vec2) -> bool {
    if r_sound().contains(m) {
        set_option(settings, 0, (option_selected(settings, 0) + 1) % 3);
    } else if r_settings().contains(m) {
        SETTINGS_OPEN.store(!settings_open(), Ordering::Relaxed);
    } else if settings_open() {
        if !r_panel().contains(m) {
            // A click outside closes the panel and does nothing else.
            SETTINGS_OPEN.store(false, Ordering::Relaxed);
            return true;
        }
        let theme = (0..THEMES.len()).find(|&i| r_theme(i).contains(m));
        let option = (0..ROWS.len())
            .flat_map(|row| (0..ROWS[row].1.len()).map(move |k| (row, k)))
            .find(|&(row, k)| r_option(row, k).contains(m));
        if let Some(i) = theme {
            settings.theme = i;
            set_theme(i);
        } else if let Some((row, k)) = option {
            set_option(settings, row, k);
        } else {
            return true; // inside the panel, between controls
        }
    } else {
        return false;
    }
    save_settings(settings);
    snd.push(Snd::Click);
    true
}

pub(crate) fn close_settings() {
    SETTINGS_OPEN.store(false, Ordering::Relaxed);
}

fn draw_speaker(cx: f32, cy: f32, settings: &Settings) {
    draw_round_rect(cx - 11.0, cy - 3.5, 5.5, 7.0, 2.0, INK());
    draw_triangle(
        vec2(cx - 7.0, cy),
        vec2(cx - 0.5, cy - 7.0),
        vec2(cx - 0.5, cy + 7.0),
        INK(),
    );
    if settings.muted {
        draw_line(cx + 2.0, cy - 5.0, cx + 10.0, cy + 5.0, 2.0, INK());
        draw_line(cx + 2.0, cy + 5.0, cx + 10.0, cy - 5.0, 2.0, INK());
        return;
    }
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
}

// three slider tracks with knobs: the conventional "settings" glyph
fn draw_sliders(cx: f32, cy: f32) {
    for (k, knob) in [(-1.0f32, 0.35f32), (0.0, -0.4), (1.0, 0.15)] {
        let y = cy + k * 5.0;
        draw_line(cx - 7.0, y, cx + 7.0, y, 1.6, with_alpha(INK(), 0.8));
        draw_circle(cx + knob * 7.0, y, 2.4, INK());
    }
}

// a small outlined toolbar button; `active` keeps it highlighted (open panel)
fn tool_button(r: Rect, mp: Vec2, active: bool) {
    let hov = r.contains(mp);
    draw_round_frame(
        r.x,
        r.y,
        r.w,
        r.h,
        10.0,
        1.5,
        control_fill(
            if active {
                mix(SRF(), ACCENT(), 0.14)
            } else {
                SRF()
            },
            hov,
        ),
        if hov || active { ACCENT() } else { CARD_EDGE() },
    );
}

/// The in-game MENU button, top left; it opens the pause dialog.
pub(crate) fn r_game_menu() -> Rect {
    Rect::new(20.0, 16.0, 84.0, 30.0)
}

pub(crate) fn draw_game_menu_button(ui: &Ui, mp: Vec2) {
    let r = r_game_menu();
    tool_button(r, mp, false);
    for k in -1..=1 {
        let y = r.y + r.h * 0.5 + k as f32 * 5.0;
        draw_line(r.x + 13.0, y, r.x + 25.0, y, 1.8, INK());
    }
    ui.text("MENU", r.x + 34.0, r.y + 20.0, LABEL_SIZE, INK());
    if r.contains(mp) && !settings_open() {
        tooltip(ui, "Pause: new map or leave · ESC", r);
    }
}

pub(crate) fn draw_icon_bar(settings: &Settings, ui: &Ui, mp: Vec2) {
    let rs = r_sound();
    tool_button(rs, mp, false);
    draw_speaker(rs.x + rs.w * 0.5, rs.y + rs.h * 0.5, settings);
    let rg = r_settings();
    let open = settings_open();
    tool_button(rg, mp, open);
    draw_sliders(rg.x + 20.0, rg.y + rg.h * 0.5);
    ui.text("SETTINGS", rg.x + 33.0, rg.y + 20.0, LABEL_SIZE, INK());
    if rg.contains(mp) && !open {
        tooltip(ui, "Theme, sound, speed, accessibility", rg);
    } else if rs.contains(mp) && !open {
        let tip = match option_selected(settings, 0) {
            0 => "Sound on · M",
            1 => "Sound at half volume · M",
            _ => "Sound off · M",
        };
        tooltip(ui, tip, rs);
    }
}

fn tooltip(ui: &Ui, tip: &str, anchor: Rect) {
    let width = ui.width(tip, 12.0) + 20.0;
    let x = if anchor.x < WIN_W * 0.5 {
        anchor.x
    } else {
        (anchor.x + anchor.w - width).max(20.0)
    };
    let y = anchor.y + anchor.h + 8.0;
    soft_shadow(x, y, width, 26.0, 8.0, 1.0);
    draw_round_frame(x, y, width, 26.0, 8.0, 1.0, SRF(), CARD_EDGE());
    ui.text(tip, x + 10.0, y + 18.0, 12.0, INK());
}

/// The settings panel, drawn last so it floats above every screen.
pub(crate) fn draw_settings_panel(settings: &Settings, ui: &Ui, m: Vec2) {
    if !settings_open() {
        return;
    }
    let p = r_panel();
    soft_shadow(p.x, p.y, p.w, p.h, 18.0, 2.4);
    draw_round_frame(p.x, p.y, p.w, p.h, 18.0, 1.5, SRF(), CARD_EDGE());
    label(
        ui,
        &format!("THEME · {}", THEMES[theme_index()].name.to_uppercase()),
        p.x + 20.0,
        p.y + 32.0,
    );
    for (i, t) in THEMES.iter().enumerate() {
        let r = r_theme(i);
        let (cx, cy, rad) = (r.x + r.w * 0.5, r.y + r.h * 0.5, r.w * 0.5);
        let chosen = i == theme_index();
        if chosen || r.contains(m) {
            draw_circle(cx, cy, rad + 2.5, if chosen { BORDER() } else { ACCENT() });
            draw_circle(cx, cy, rad, SRF());
        }
        // each swatch shows the theme's surface with its accent dot
        draw_circle(cx, cy, rad - 2.0, t.edge_color());
        draw_circle(cx, cy, rad - 3.5, t.srf_color());
        draw_circle(cx, cy, 5.5, t.accent_color());
    }
    for (row, (name, options)) in ROWS.iter().enumerate() {
        let y = ROW_Y + row as f32 * ROW_H;
        label(ui, name, p.x + 20.0, y + 19.0);
        let sel = option_selected(settings, row);
        let items: Vec<(Rect, &str, bool)> = options
            .iter()
            .enumerate()
            .map(|(k, text)| (r_option(row, k), *text, k == sel))
            .collect();
        seg_group(ui, m, true, &items, 12.0);
    }
    ui.text_centered(
        "M sound · F8 motion · F9 color · F10 theme",
        p.x + p.w * 0.5,
        p.y + p.h - 16.0,
        11.0,
        with_alpha(INK(), 0.55),
    );
}

pub(crate) fn control_fill(fill: Color, hovered: bool) -> Color {
    if !hovered {
        return fill;
    }
    let surface = SRF();
    if fill.r == surface.r && fill.g == surface.g && fill.b == surface.b {
        // White cannot be lightened. Give neutral controls a visible accent
        // tint in either theme, while retaining contrast with their labels.
        mix(fill, ACCENT(), if dark_mode() { 0.16 } else { 0.10 })
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

// ------------------------------------------------------------ shared styles
// One visual language for every screen:
// - labels: small caps text at 72% ink, left of or above their control
// - the selected option of any choice is filled with your color
// - the accent color marks hover and keyboard/text focus only
// - one filled primary button per screen; everything else is outlined

pub(crate) const LABEL_SIZE: f32 = 12.0;

pub(crate) fn label(ui: &Ui, text: &str, x: f32, y: f32) {
    ui.text(text, x, y, LABEL_SIZE, with_alpha(INK(), 0.72));
}

// text baseline that centers caps of `size` inside `r`
fn baseline(r: Rect, size: f32) -> f32 {
    r.y + r.h * 0.5 + size * 0.36
}

/// The one filled call to action of a screen.
pub(crate) fn primary_button(ui: &Ui, r: Rect, text: &str, size: f32, m: Vec2, enabled: bool) {
    let hovered = enabled && r.contains(m);
    let base = if enabled {
        palette(HUMAN).mid
    } else {
        mix(SRF(), BG(), 0.5)
    };
    let edge = if enabled { BORDER() } else { CARD_EDGE() };
    menu_button(r, base, edge, hovered);
    let ink = if enabled {
        on_color(control_fill(base, hovered))
    } else {
        with_alpha(INK(), 0.5)
    };
    ui.text_centered(text, r.x + r.w * 0.5, baseline(r, size), size, ink);
}

/// Every other button: outlined on the surface color.
pub(crate) fn secondary_button(ui: &Ui, r: Rect, text: &str, size: f32, m: Vec2) {
    menu_button(r, SRF(), CARD_EDGE(), r.contains(m));
    ui.text_centered(text, r.x + r.w * 0.5, baseline(r, size), size, INK());
}

/// A titled card: the container for one group of related controls.
pub(crate) fn card(ui: &Ui, r: Rect, title: &str) {
    draw_round_frame(r.x, r.y, r.w, r.h, 16.0, 2.0, SRF(), CARD_EDGE());
    if !title.is_empty() {
        label(ui, title, r.x + 18.0, r.y + 30.0);
    }
}

/// Options in a single pill container; the selected one is filled with your
/// color. `live` false draws the same picture read-only.
pub(crate) fn seg_group(ui: &Ui, m: Vec2, live: bool, items: &[(Rect, &str, bool)], size: f32) {
    let y = items[0].0.y;
    let h = items[0].0.h;
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
        let ink = if *sel { INK_ON_FILL } else { INK() };
        ui.text_centered(
            txt,
            r.x + r.w * 0.5,
            y + h * 0.5 + size * 0.33,
            size,
            if live || *sel {
                ink
            } else {
                with_alpha(ink, 0.6)
            },
        );
    }
}

/// A labelled segmented row of the setup form: label in the left gutter.
pub(crate) fn seg_row(
    ui: &Ui,
    m: Vec2,
    live: bool,
    text: &str,
    items: &[(Rect, &str, bool)],
    size: f32,
) {
    let r = items[0].0;
    label(ui, text, crate::setup::LEFT_X, r.y + r.h * 0.5 + 4.0);
    seg_group(ui, m, live, items, size);
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
