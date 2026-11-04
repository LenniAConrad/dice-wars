//! Light/dark theme colors, player palettes and contrast helpers.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use dicegame::engine::prelude::*;

use crate::game::MAX_PLAYERS;

pub(crate) const COLOR_NAMES: [&str; MAX_PLAYERS] = [
    "Buttercup",
    "Lavender",
    "Rose",
    "Mint",
    "Sky",
    "Peach",
    "Teal",
    "Plum",
    "Cocoa",
];

// which palette slot each player uses; a permutation so colors stay unique
static COLOR_SLOTS: [AtomicUsize; MAX_PLAYERS] = [
    AtomicUsize::new(0),
    AtomicUsize::new(1),
    AtomicUsize::new(2),
    AtomicUsize::new(3),
    AtomicUsize::new(4),
    AtomicUsize::new(5),
    AtomicUsize::new(6),
    AtomicUsize::new(7),
    AtomicUsize::new(8),
];

// how many of the players are humans (hotseat); the rest are AI
pub(crate) static HUMANS_N: AtomicUsize = AtomicUsize::new(1);

pub(crate) fn color_map(p: usize) -> usize {
    COLOR_SLOTS[p % MAX_PLAYERS].load(Ordering::Relaxed)
}

// resolve everyone's color: humans keep their picks (first come, first
// served), then every remaining seat gets the lowest free palette slot
pub(crate) fn resolve_colors(players: usize, humans: usize, picks: &[Option<usize>]) -> Vec<usize> {
    let mut used = [false; MAX_PLAYERS];
    let mut out = vec![usize::MAX; players.min(MAX_PLAYERS)];
    for p in 0..humans.min(out.len()) {
        if let Some(c) = picks.get(p).copied().flatten() {
            if c < MAX_PLAYERS && !used[c] {
                out[p] = c;
                used[c] = true;
            }
        }
    }
    // like the solo menu: everyone else keeps their own color, and only the
    // seat whose color was taken swaps onto the freed slot
    for (p, slot) in out.iter_mut().enumerate() {
        if *slot == usize::MAX && !used[p] {
            *slot = p;
            used[p] = true;
        }
    }
    for slot in out.iter_mut() {
        if *slot == usize::MAX {
            let c = (0..MAX_PLAYERS).find(|&c| !used[c]).unwrap_or(0);
            *slot = c;
            used[c] = true;
        }
    }
    out
}

// install a color assignment globally, padding to a full 9-slot permutation
pub(crate) fn apply_colors(v: &[usize]) {
    let mut used = [false; MAX_PLAYERS];
    let mut k = 0;
    for (i, slot) in COLOR_SLOTS.iter().enumerate() {
        let c = match v.get(i) {
            Some(&c) if c < MAX_PLAYERS && !used[c] => c,
            _ => {
                while used[k] {
                    k += 1;
                }
                k
            }
        };
        used[c] = true;
        slot.store(c, Ordering::Relaxed);
    }
}

// solo / menu: a pure swap — you take the picked color's slot and it takes
// yours, every other player keeps its color, so the map never re-shuffles
pub(crate) fn apply_local_colors(my_color: usize) {
    let mut v: Vec<usize> = (0..MAX_PLAYERS).collect();
    v.swap(0, my_color % MAX_PLAYERS);
    apply_colors(&v);
}

// theme: every surface color goes through these so dark mode is one flag
pub(crate) static REDUCED_MOTION: AtomicBool = AtomicBool::new(false);

pub(crate) static DARK: AtomicBool = AtomicBool::new(false);

pub(crate) fn dark_mode() -> bool {
    DARK.load(Ordering::Relaxed)
}

pub(crate) fn th(light: Color, dark: Color) -> Color {
    if dark_mode() {
        dark
    } else {
        light
    }
}

#[allow(non_snake_case)]
pub(crate) fn BG() -> Color {
    th(
        Color::new(0.945, 0.953, 0.972, 1.0),
        Color::new(0.070, 0.078, 0.115, 1.0),
    )
}

#[allow(non_snake_case)]
pub(crate) fn BORDER() -> Color {
    th(
        Color::new(0.38, 0.41, 0.60, 1.0),
        Color::new(0.56, 0.60, 0.80, 1.0),
    )
}

// Focus and hover outlines use one calm indigo accent in both themes.
#[allow(non_snake_case)]
pub(crate) fn ACCENT() -> Color {
    th(
        Color::new(0.33, 0.40, 0.86, 1.0),
        Color::new(0.58, 0.65, 1.0, 1.0),
    )
}

#[allow(non_snake_case)]
pub(crate) fn CARD_EDGE() -> Color {
    th(
        Color::new(0.865, 0.880, 0.910, 1.0),
        Color::new(0.22, 0.24, 0.33, 1.0),
    )
}

#[allow(non_snake_case)]
pub(crate) fn INK() -> Color {
    th(
        Color::new(0.15, 0.16, 0.30, 1.0),
        Color::new(0.90, 0.91, 0.96, 1.0),
    )
}

#[allow(non_snake_case)]
pub(crate) fn SEA() -> Color {
    th(
        Color::new(0.845, 0.835, 0.805, 1.0),
        Color::new(0.22, 0.235, 0.32, 1.0),
    )
}

// the main surface: white in light mode, deep navy in dark mode
#[allow(non_snake_case)]
pub(crate) fn SRF() -> Color {
    th(WHITE, Color::new(0.115, 0.125, 0.18, 1.0))
}

// pastel fills are light in both themes, so text on them is always dark
pub(crate) const INK_ON_FILL: Color = Color::new(0.15, 0.16, 0.30, 1.0);

// Pastel map art keeps the same outline and colors in either interface theme.
pub(crate) const MAP_EDGE: Color = Color::new(0.24, 0.25, 0.42, 1.0);

// one pastel base per player; dice/tile shades are derived from it
pub(crate) const PLAYER_BASE: [Color; MAX_PLAYERS] = [
    Color::new(0.96, 0.82, 0.38, 1.0), // buttercup (you)
    Color::new(0.72, 0.64, 0.93, 1.0), // lavender
    Color::new(0.95, 0.59, 0.62, 1.0), // rose
    Color::new(0.55, 0.85, 0.65, 1.0), // mint
    Color::new(0.56, 0.76, 0.95, 1.0), // sky
    Color::new(0.97, 0.71, 0.47, 1.0), // peach
    Color::new(0.45, 0.82, 0.78, 1.0), // teal
    Color::new(0.88, 0.62, 0.86, 1.0), // plum
    Color::new(0.79, 0.66, 0.50, 1.0), // cocoa
];

// Protan-safe colors optimized for separation after a full-protanopia
// simulation. Blue/yellow position and luminance carry the distinctions that
// would otherwise be lost when reds are perceived weakly.
pub(crate) const PLAYER_BASE_PROTAN: [Color; MAX_PLAYERS] = [
    Color::from_rgba(0x90, 0xF0, 0x10, 0xFF), // bright yellow to protan vision
    Color::from_rgba(0xB0, 0xD0, 0xF0, 0xFF), // pale blue
    Color::from_rgba(0xF0, 0x30, 0x70, 0xFF), // neutral mid-tone to protan vision
    Color::from_rgba(0x50, 0x90, 0x10, 0xFF), // ochre to protan vision
    Color::from_rgba(0x10, 0x90, 0xF0, 0xFF), // sky blue
    Color::from_rgba(0xF0, 0xB0, 0x90, 0xFF), // pale sand
    Color::from_rgba(0x10, 0x10, 0xF0, 0xFF), // cobalt
    Color::from_rgba(0x70, 0x10, 0x70, 0xFF), // deep blue to protan vision
    Color::from_rgba(0x70, 0x30, 0x10, 0xFF), // dark ochre to protan vision
];

pub(crate) static COLORBLIND: AtomicBool = AtomicBool::new(false);

pub(crate) fn colorblind_mode() -> bool {
    COLORBLIND.load(Ordering::Relaxed)
}

pub(crate) struct Palette {
    pub(crate) fill: Color,  // hex tile
    pub(crate) mid: Color,   // die left face
    pub(crate) dark: Color,  // die right face
    pub(crate) light: Color, // die top face
    pub(crate) pip: Color,
}

pub(crate) fn darken(c: Color, f: f32) -> Color {
    Color::new(c.r * f, c.g * f, c.b * f, c.a)
}

pub(crate) fn base_color(i: usize) -> Color {
    if colorblind_mode() {
        PLAYER_BASE_PROTAN[i]
    } else {
        PLAYER_BASE[i]
    }
}

pub(crate) fn palette(player: usize) -> Palette {
    let idx = color_map(player % MAX_PLAYERS);
    if colorblind_mode() {
        // Keep the accessible hues vivid. Pale pips on the darker protan-safe
        // hues preserve face-value contrast on the shaded sides of each die.
        let base = PLAYER_BASE_PROTAN[idx];
        let pale_pips = matches!(idx, 2 | 3 | 4 | 6 | 7 | 8);
        // `fill` is also used behind dark UI labels, so lift darker hues until
        // those labels remain legible without flattening every map color.
        let fill_mix = match idx {
            6 => 0.50,
            7 => 0.48,
            8 => 0.43,
            2 => 0.30,
            _ => 0.16,
        };
        Palette {
            fill: mix(base, WHITE, fill_mix),
            mid: base,
            dark: darken(base, 0.66),
            light: mix(base, WHITE, 0.40),
            pip: if pale_pips {
                Color::new(0.94, 0.95, 1.0, 1.0)
            } else {
                Color::new(0.13, 0.13, 0.22, 1.0)
            },
        }
    } else {
        let base = PLAYER_BASE[idx];
        Palette {
            fill: mix(base, WHITE, 0.46),
            mid: base,
            dark: darken(base, 0.78),
            light: mix(base, WHITE, 0.62),
            pip: Color::new(0.22, 0.23, 0.38, 1.0),
        }
    }
}

fn luminance(c: Color) -> f32 {
    let linear = |v: f32| {
        if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * linear(c.r) + 0.7152 * linear(c.g) + 0.0722 * linear(c.b)
}

pub(crate) fn contrast(a: Color, b: Color) -> f32 {
    let (a, b) = (luminance(a), luminance(b));
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}

pub(crate) fn on_color(background: Color) -> Color {
    if contrast(INK_ON_FILL, background) >= 4.5 {
        return INK_ON_FILL;
    }
    if contrast(WHITE, background) > contrast(BLACK, background) {
        WHITE
    } else {
        BLACK
    }
}

pub(crate) fn pip_color(palette: &Palette, face: Color) -> Color {
    if contrast(palette.pip, face) >= 3.0 {
        palette.pip
    } else {
        on_color(face)
    }
}

pub(crate) fn map_fill(player: usize) -> Color {
    palette(player).fill
}

pub(crate) fn accent_ink(player: usize) -> Color {
    let color = palette(player).dark;
    if contrast(color, SRF()) >= 4.5 {
        return color;
    }
    let target = on_color(SRF());
    for step in 1..=20 {
        let adjusted = mix(color, target, step as f32 / 20.0);
        if contrast(adjusted, SRF()) >= 4.5 {
            return adjusted;
        }
    }
    target
}

pub(crate) fn lighten(c: Color, t: f32) -> Color {
    Color::new(
        c.r + (1.0 - c.r) * t,
        c.g + (1.0 - c.g) * t,
        c.b + (1.0 - c.b) * t,
        c.a,
    )
}

pub(crate) fn with_alpha(c: Color, a: f32) -> Color {
    Color::new(c.r, c.g, c.b, a)
}

pub(crate) fn mix(a: Color, b: Color, t: f32) -> Color {
    Color::new(
        a.r + (b.r - a.r) * t,
        a.g + (b.g - a.g) * t,
        a.b + (b.b - a.b) * t,
        a.a + (b.a - a.a) * t,
    )
}

#[cfg(test)]
mod tests {
    use crate::testutil::GLOBALS;
    use crate::ui::control_fill;

    use super::*;

    #[test]
    fn labels_and_roll_pips_have_contrast_in_both_themes() {
        let _guard = GLOBALS.lock().unwrap();
        let original_dark = dark_mode();
        let original_accessible = colorblind_mode();
        apply_local_colors(0);
        for dark in [false, true] {
            DARK.store(dark, Ordering::Relaxed);
            assert!(contrast(INK(), SRF()) >= 7.0);
            for accessible in [false, true] {
                COLORBLIND.store(accessible, Ordering::Relaxed);
                for player in 0..MAX_PLAYERS {
                    let pal = palette(player);
                    for fill in [pal.mid, pal.dark, pal.fill] {
                        assert!(contrast(on_color(fill), fill) >= 4.5, "label {player}");
                        assert!(
                            contrast(on_color(fill), control_fill(fill, true)) >= 4.5,
                            "hover label {player}"
                        );
                    }
                    apply_local_colors(player);
                    assert!(
                        contrast(INK(), control_fill(SRF(), true)) >= 4.5,
                        "neutral hover label {player}"
                    );
                    apply_local_colors(0);
                    for face in [pal.mid, pal.dark, pal.light] {
                        assert!(
                            contrast(pip_color(&pal, face), face) >= 3.0,
                            "pips {player}"
                        );
                    }
                    assert!(
                        contrast(accent_ink(player), SRF()) >= 4.5,
                        "accent {player}"
                    );
                }
            }
        }
        DARK.store(original_dark, Ordering::Relaxed);
        COLORBLIND.store(original_accessible, Ordering::Relaxed);
    }

    #[test]
    fn colors_first_come_first_served() {
        // host picked slot 2, guest wanted 2 too late (None), other guest 0
        let picks = [Some(2), None, Some(0)];
        assert_eq!(resolve_colors(5, 3, &picks), vec![2, 1, 0, 3, 4]);
    }

    #[test]
    fn colorblind_palette_keeps_labels_and_die_pips_contrasting() {
        let _g = GLOBALS.lock().unwrap();
        let previous = COLORBLIND.swap(true, Ordering::Relaxed);
        apply_colors(&(0..MAX_PLAYERS).collect::<Vec<_>>());
        let luma = |c: Color| 0.2126 * c.r + 0.7152 * c.g + 0.0722 * c.b;
        let srgb_to_linear = |channel: f32| {
            if channel <= 0.04045 {
                channel / 12.92
            } else {
                ((channel + 0.055) / 1.055).powf(2.4)
            }
        };
        let linear_to_srgb = |channel: f32| {
            let encoded = if channel <= 0.003_130_8 {
                12.92 * channel
            } else {
                1.055 * channel.max(0.0).powf(1.0 / 2.4) - 0.055
            };
            encoded.clamp(0.0, 1.0)
        };
        // Full-severity Machado protanopia transform, applied in linear RGB.
        let simulate_protan = |c: Color| {
            let r = srgb_to_linear(c.r);
            let g = srgb_to_linear(c.g);
            let b = srgb_to_linear(c.b);
            Color::new(
                linear_to_srgb(0.152_286 * r + 1.052_583 * g - 0.204_868 * b),
                linear_to_srgb(0.114_503 * r + 0.786_281 * g + 0.099_216 * b),
                linear_to_srgb(-0.003_882 * r - 0.048_116 * g + 1.051_998 * b),
                1.0,
            )
        };
        let distance = |a: Color, b: Color| {
            ((a.r - b.r).powi(2) + (a.g - b.g).powi(2) + (a.b - b.b).powi(2)).sqrt()
        };
        for player in 0..MAX_PLAYERS {
            let pal = palette(player);
            assert!(
                luma(pal.fill) >= 0.55,
                "player {player} has a dark label fill"
            );
            assert!(
                (luma(pal.mid) - luma(pal.pip)).abs() >= 0.35,
                "player {player} has low-contrast die pips"
            );
        }
        for (a, &ca) in PLAYER_BASE_PROTAN.iter().enumerate() {
            for (b, &cb) in PLAYER_BASE_PROTAN.iter().enumerate().skip(a + 1) {
                assert!(
                    distance(ca, cb) >= 0.38,
                    "palette slots {a} and {b} are too similar normally"
                );
                assert!(
                    distance(simulate_protan(ca), simulate_protan(cb)) >= 0.39,
                    "palette slots {a} and {b} collapse under protanopia"
                );
            }
        }
        COLORBLIND.store(previous, Ordering::Relaxed);
        apply_local_colors(0);
    }
}
