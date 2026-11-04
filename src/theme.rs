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

pub(crate) static REDUCED_MOTION: AtomicBool = AtomicBool::new(false);

// Interface themes. Every surface color goes through the current theme; the
// pastel map art and dice keep their colors in every theme.
pub(crate) struct Theme {
    pub(crate) name: &'static str,
    pub(crate) dark: bool,
    bg: Color,
    srf: Color,
    ink: Color,
    border: Color,
    accent: Color,
    edge: Color,
    sea: Color,
    // lakes and bays inside the map get a quiet tint of their own
    pub(crate) water: Color,
}

const fn hex(v: u32) -> Color {
    Color::from_rgba((v >> 16) as u8, (v >> 8) as u8, v as u8, 0xFF)
}

pub(crate) const THEMES: [Theme; 7] = [
    Theme {
        name: "Daylight",
        dark: false,
        bg: hex(0xF1F3F8),
        srf: hex(0xFFFFFF),
        ink: hex(0x262951),
        border: hex(0x61699A),
        accent: hex(0x5466DB),
        edge: hex(0xDDE0E8),
        sea: hex(0xC9D3E6),
        water: hex(0xEDF2FA),
    },
    Theme {
        name: "Midnight",
        dark: true,
        bg: hex(0x12141D),
        srf: hex(0x1D202E),
        ink: hex(0xE6E8F5),
        border: hex(0x8F99CC),
        accent: hex(0x94A6FF),
        edge: hex(0x383D54),
        sea: hex(0x404A6C),
        water: hex(0x262D45),
    },
    Theme {
        name: "Sakura",
        dark: false,
        bg: hex(0xFBEFF3),
        srf: hex(0xFFFAFB),
        ink: hex(0x3B2333),
        border: hex(0x9A6A86),
        accent: hex(0xC93F7D),
        edge: hex(0xF0DAE3),
        sea: hex(0xE6C9D5),
        water: hex(0xFBEEF3),
    },
    Theme {
        name: "Sage",
        dark: false,
        bg: hex(0xEBF1E9),
        srf: hex(0xFAFCF8),
        ink: hex(0x1F3326),
        border: hex(0x5E7A66),
        accent: hex(0x2F7F50),
        edge: hex(0xD5E1D2),
        sea: hex(0xC2D3C1),
        water: hex(0xEEF4EE),
    },
    Theme {
        name: "Paper",
        dark: false,
        bg: hex(0xF3EDE1),
        srf: hex(0xFFFBF3),
        ink: hex(0x3A2F24),
        border: hex(0x85735C),
        accent: hex(0xB0602A),
        edge: hex(0xE6DCC9),
        sea: hex(0xDACAB0),
        water: hex(0xF5EEE1),
    },
    Theme {
        name: "Ocean",
        dark: true,
        bg: hex(0x081521),
        srf: hex(0x0F2434),
        ink: hex(0xE2F0F8),
        border: hex(0x7FA6C4),
        accent: hex(0x4FC1F0),
        edge: hex(0x22405A),
        sea: hex(0x305C7E),
        water: hex(0x173B58),
    },
    Theme {
        name: "Ember",
        dark: true,
        bg: hex(0x150E0D),
        srf: hex(0x241817),
        ink: hex(0xF6E8E3),
        border: hex(0xC49A8C),
        accent: hex(0xFF8A5B),
        edge: hex(0x43302C),
        sea: hex(0x5A3F38),
        water: hex(0x342320),
    },
];

impl Theme {
    pub(crate) fn srf_color(&self) -> Color {
        self.srf
    }
    pub(crate) fn edge_color(&self) -> Color {
        self.edge
    }
    pub(crate) fn accent_color(&self) -> Color {
        self.accent
    }
}

static THEME: AtomicUsize = AtomicUsize::new(0);

pub(crate) fn theme() -> &'static Theme {
    &THEMES[THEME.load(Ordering::Relaxed) % THEMES.len()]
}

pub(crate) fn theme_index() -> usize {
    THEME.load(Ordering::Relaxed) % THEMES.len()
}

pub(crate) fn set_theme(i: usize) {
    THEME.store(i % THEMES.len(), Ordering::Relaxed);
}

pub(crate) fn dark_mode() -> bool {
    theme().dark
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
    theme().bg
}

#[allow(non_snake_case)]
pub(crate) fn BORDER() -> Color {
    theme().border
}

// Focus and hover outlines use the theme's single accent.
#[allow(non_snake_case)]
pub(crate) fn ACCENT() -> Color {
    theme().accent
}

#[allow(non_snake_case)]
pub(crate) fn CARD_EDGE() -> Color {
    theme().edge
}

#[allow(non_snake_case)]
pub(crate) fn INK() -> Color {
    theme().ink
}

#[allow(non_snake_case)]
pub(crate) fn SEA() -> Color {
    theme().sea
}

// the main surface of the current theme
#[allow(non_snake_case)]
pub(crate) fn SRF() -> Color {
    theme().srf
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

// Color-assist palette: nine muted hues picked by a search that maximizes the
// smallest pairwise distance under normal vision and full protanopia and
// deuteranopia simulations (lightness and blue/yellow carry the differences).
pub(crate) const PLAYER_BASE_PROTAN: [Color; MAX_PLAYERS] = [
    Color::from_rgba(0xF9, 0xE0, 0x3F, 0xFF), // yellow
    Color::from_rgba(0x80, 0x82, 0xF7, 0xFF), // periwinkle
    Color::from_rgba(0xCE, 0x93, 0x92, 0xFF), // dusty rose
    Color::from_rgba(0xC3, 0xFA, 0xB7, 0xFF), // pale mint
    Color::from_rgba(0x33, 0xD5, 0xFE, 0xFF), // sky
    Color::from_rgba(0xBC, 0x96, 0x11, 0xFF), // ochre
    Color::from_rgba(0x0E, 0x4E, 0xB1, 0xFF), // cobalt
    Color::from_rgba(0x70, 0x52, 0x6D, 0xFF), // mauve
    Color::from_rgba(0x91, 0x31, 0x05, 0xFF), // rust
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
        let pale_pips = matches!(idx, 1 | 6 | 7 | 8);
        // `fill` is also used behind dark UI labels, so lift darker hues until
        // those labels remain legible without flattening every map color.
        let fill_mix = match idx {
            3 => 0.0,
            6 => 0.48,
            7 => 0.42,
            8 => 0.50,
            1 => 0.18,
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
    fn labels_and_roll_pips_have_contrast_in_every_theme() {
        let _guard = GLOBALS.lock().unwrap_or_else(|e| e.into_inner());
        let original_theme = theme_index();
        let original_accessible = colorblind_mode();
        apply_local_colors(0);
        for (i, t) in THEMES.iter().enumerate() {
            set_theme(i);
            assert!(contrast(INK(), SRF()) >= 7.0, "{} ink", t.name);
            assert!(contrast(INK(), BG()) >= 7.0, "{} ink on background", t.name);
            assert!(contrast(ACCENT(), SRF()) >= 3.0, "{} accent", t.name);
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
        set_theme(original_theme);
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
        let _g = GLOBALS.lock().unwrap_or_else(|e| e.into_inner());
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
        let simulate_deutan = |c: Color| {
            let r = srgb_to_linear(c.r);
            let g = srgb_to_linear(c.g);
            let b = srgb_to_linear(c.b);
            Color::new(
                linear_to_srgb(0.367_322 * r + 0.860_646 * g - 0.227_968 * b),
                linear_to_srgb(0.280_085 * r + 0.672_501 * g + 0.047_413 * b),
                linear_to_srgb(-0.011_820 * r + 0.042_940 * g + 0.968_881 * b),
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
                assert!(
                    distance(simulate_deutan(ca), simulate_deutan(cb)) >= 0.39,
                    "palette slots {a} and {b} collapse under deuteranopia"
                );
            }
        }
        COLORBLIND.store(previous, Ordering::Relaxed);
        apply_local_colors(0);
    }
}
