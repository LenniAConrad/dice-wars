use macroquad::audio::{load_sound_from_bytes, play_sound, PlaySoundParams, Sound};
use macroquad::prelude::*;
use macroquad::rand::gen_range;
use std::collections::{HashMap, HashSet};
use std::f32::consts::PI;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

// ---------------------------------------------------------------- constants

const COLS: usize = 30;
const ROWS: usize = 22;
const HEX: f32 = 16.0;
const OFF_X: f32 = 48.0;
const OFF_Y: f32 = 96.0;
const SQRT3: f32 = 1.732_050_8;

// virtual canvas; scaled to the real window with letterboxing
const WIN_W: f32 = 930.0;
const WIN_H: f32 = 820.0;

const MAX_PLAYERS: usize = 8;
const N_SEEDS: usize = 60; // regions grown on the grid
const N_HOLES: usize = 9; // regions removed for lakes and ragged coasts
const MAX_DICE: u32 = 8;
const HUMAN: usize = 0;
const AI_STEP: f32 = 0.35; // pause between AI actions (on top of battle time)

// battle animation timing
const T_SHAKE: f32 = 0.45; // dice rattle before the rolls appear
const T_BATTLE: f32 = 1.65; // total battle length

const SEED_MOD: u64 = 100_000_000; // seeds are 8 decimal digits
const BOOKMARK_FILE: &str = "bookmarks.txt";

// settings/bookmarks/replays live in the platform's per-user data directory,
// so the installed game works no matter where it is launched from
fn replay_file(name: &str) -> String {
    let dir = std::path::PathBuf::from(data_file("replays"));
    let _ = std::fs::create_dir_all(&dir);
    dir.join(name).to_string_lossy().into_owned()
}

fn data_file(name: &str) -> String {
    let base = if cfg!(windows) {
        std::env::var("APPDATA").ok().map(std::path::PathBuf::from)
    } else {
        std::env::var("XDG_DATA_HOME")
            .ok()
            .map(std::path::PathBuf::from)
            .or_else(|| {
                std::env::var("HOME")
                    .ok()
                    .map(|h| std::path::PathBuf::from(h).join(".local/share"))
            })
    };
    match base {
        Some(b) => {
            let d = b.join("dice-wars");
            let _ = std::fs::create_dir_all(&d);
            d.join(name).to_string_lossy().into_owned()
        }
        None => name.to_string(),
    }
}

const COLOR_NAMES: [&str; MAX_PLAYERS] = [
    "Buttercup", "Lavender", "Rose", "Mint", "Sky", "Peach", "Teal", "Plum",
];

// which palette slot the human picked; other players swap around it
static HUMAN_COLOR: AtomicUsize = AtomicUsize::new(0);
// how many of the players are humans (hotseat); the rest are AI
static HUMANS_N: AtomicUsize = AtomicUsize::new(1);

const HUMAN_LABELS: [&str; MAX_PLAYERS] = ["P1", "P2", "P3", "P4", "P5", "P6", "P7", "P8"];

fn color_map(p: usize) -> usize {
    let c = HUMAN_COLOR.load(Ordering::Relaxed);
    if p == HUMAN {
        c
    } else if p == c {
        0
    } else {
        p
    }
}

fn player_name(p: usize) -> &'static str {
    let humans = HUMANS_N.load(Ordering::Relaxed);
    if p < humans {
        if humans == 1 {
            "You"
        } else {
            HUMAN_LABELS[p]
        }
    } else {
        COLOR_NAMES[color_map(p)]
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Difficulty {
    Easy,
    Normal,
    Hard,
}

#[derive(Clone, Copy, PartialEq)]
enum Persona {
    Aggressive, // attacks on any non-losing odds
    Defensive,  // only attacks with a clear advantage
    Greedy,     // loves conquests that consolidate its own region
    Balancer,   // schemes against whoever is winning
    Chaotic,    // unpredictable
}

// theme: every surface color goes through these so dark mode is one flag
static DARK: AtomicBool = AtomicBool::new(false);

fn dark_mode() -> bool {
    DARK.load(Ordering::Relaxed)
}

fn th(light: Color, dark: Color) -> Color {
    if dark_mode() {
        dark
    } else {
        light
    }
}

#[allow(non_snake_case)]
fn BG() -> Color {
    th(Color::new(0.905, 0.930, 0.960, 1.0), Color::new(0.055, 0.065, 0.10, 1.0))
}
#[allow(non_snake_case)]
fn BORDER() -> Color {
    th(Color::new(0.24, 0.25, 0.42, 1.0), Color::new(0.56, 0.60, 0.80, 1.0))
}
#[allow(non_snake_case)]
fn PANEL_SHADOW() -> Color {
    th(Color::new(0.815, 0.850, 0.895, 1.0), Color::new(0.04, 0.045, 0.075, 1.0))
}
#[allow(non_snake_case)]
fn CARD_EDGE() -> Color {
    th(Color::new(0.835, 0.860, 0.895, 1.0), Color::new(0.24, 0.26, 0.36, 1.0))
}
#[allow(non_snake_case)]
fn INK() -> Color {
    th(Color::new(0.15, 0.16, 0.30, 1.0), Color::new(0.90, 0.91, 0.96, 1.0))
}
#[allow(non_snake_case)]
fn SEA() -> Color {
    th(Color::new(0.845, 0.835, 0.805, 1.0), Color::new(0.22, 0.235, 0.32, 1.0))
}
// the main surface: white in light mode, deep navy in dark mode
#[allow(non_snake_case)]
fn SRF() -> Color {
    th(WHITE, Color::new(0.115, 0.125, 0.18, 1.0))
}
// pastel fills are light in both themes, so text on them is always dark
const INK_ON_FILL: Color = Color::new(0.15, 0.16, 0.30, 1.0);

// one pastel base per player; dice/tile shades are derived from it
const PLAYER_BASE: [Color; MAX_PLAYERS] = [
    Color::new(0.96, 0.82, 0.38, 1.0), // buttercup (you)
    Color::new(0.72, 0.64, 0.93, 1.0), // lavender
    Color::new(0.95, 0.59, 0.62, 1.0), // rose
    Color::new(0.55, 0.85, 0.65, 1.0), // mint
    Color::new(0.56, 0.76, 0.95, 1.0), // sky
    Color::new(0.97, 0.71, 0.47, 1.0), // peach
    Color::new(0.45, 0.82, 0.78, 1.0), // teal
    Color::new(0.88, 0.62, 0.86, 1.0), // plum
];

// Okabe-Ito inspired alternative with much larger hue separation
const PLAYER_BASE_CB: [Color; MAX_PLAYERS] = [
    Color::new(0.94, 0.89, 0.26, 1.0), // you: yellow
    Color::new(0.80, 0.47, 0.65, 1.0), // lavender -> reddish purple
    Color::new(0.84, 0.37, 0.00, 1.0), // rose -> vermillion
    Color::new(0.00, 0.62, 0.45, 1.0), // mint -> bluish green
    Color::new(0.34, 0.71, 0.91, 1.0), // sky -> sky blue
    Color::new(0.90, 0.62, 0.00, 1.0), // peach -> orange
    Color::new(0.00, 0.45, 0.70, 1.0), // teal -> blue
    Color::new(0.45, 0.45, 0.48, 1.0), // plum -> gray
];

static COLORBLIND: AtomicBool = AtomicBool::new(false);

fn colorblind_mode() -> bool {
    COLORBLIND.load(Ordering::Relaxed)
}

// pip layouts in face coordinates, verified against a projected 3D mock
const PIPS: [&[(f32, f32)]; 6] = [
    &[(0.5, 0.5)],
    &[(0.28, 0.28), (0.72, 0.72)],
    &[(0.25, 0.25), (0.5, 0.5), (0.75, 0.75)],
    &[(0.28, 0.28), (0.72, 0.28), (0.28, 0.72), (0.72, 0.72)],
    &[(0.25, 0.25), (0.75, 0.25), (0.5, 0.5), (0.25, 0.75), (0.75, 0.75)],
    &[(0.27, 0.22), (0.73, 0.22), (0.27, 0.5), (0.73, 0.5), (0.27, 0.78), (0.73, 0.78)],
];

struct Palette {
    fill: Color,  // hex tile
    mid: Color,   // die left face
    dark: Color,  // die right face
    light: Color, // die top face
    pip: Color,
}

fn darken(c: Color, f: f32) -> Color {
    Color::new(c.r * f, c.g * f, c.b * f, c.a)
}

fn base_color(i: usize) -> Color {
    if colorblind_mode() {
        PLAYER_BASE_CB[i]
    } else {
        PLAYER_BASE[i]
    }
}

fn palette(player: usize) -> Palette {
    let idx = color_map(player % MAX_PLAYERS);
    if colorblind_mode() {
        // vivid, original-Dice-Wars-style saturation with Okabe-Ito hues
        let base = PLAYER_BASE_CB[idx];
        Palette {
            fill: mix(base, WHITE, 0.20),
            mid: base,
            dark: darken(base, 0.66),
            light: mix(base, WHITE, 0.40),
            pip: Color::new(0.13, 0.13, 0.22, 1.0),
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

// ---------------------------------------------------------------- view scaling
// Everything is drawn on a WIN_W x WIN_H virtual canvas which is scaled
// (letterboxed) to the actual window; the mouse is mapped back into it.

fn view_scale() -> f32 {
    (screen_width() / WIN_W)
        .min(screen_height() / WIN_H)
        .max(0.2)
}

fn view_camera() -> Camera2D {
    let sw = screen_width().max(1.0);
    let sh = screen_height().max(1.0);
    let k = view_scale();
    Camera2D {
        target: vec2(WIN_W * 0.5, WIN_H * 0.5),
        zoom: vec2(2.0 * k / sw, 2.0 * k / sh),
        ..Default::default()
    }
}

fn mouse_virtual() -> Vec2 {
    let (mx, my) = mouse_position();
    let sw = screen_width().max(1.0);
    let sh = screen_height().max(1.0);
    let k = view_scale();
    vec2(
        (mx - sw * 0.5) / k + WIN_W * 0.5,
        (my - sh * 0.5) / k + WIN_H * 0.5,
    )
}

// ---------------------------------------------------------------- text (embedded font)

struct Ui {
    font: Font,
}

impl Ui {
    // rasterize at the real on-screen pixel size so text stays crisp at any scale
    fn text(&self, s: &str, x: f32, y: f32, size: f32, col: Color) {
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

    fn width(&self, s: &str, size: f32) -> f32 {
        let k = view_scale();
        measure_text(s, Some(&self.font), (size * k).round().max(8.0) as u16, 1.0 / k).width
    }

    fn text_centered(&self, s: &str, cx: f32, y: f32, size: f32, col: Color) {
        self.text(s, cx - self.width(s, size) * 0.5, y, size, col);
    }

    fn text_outlined(&self, s: &str, x: f32, y: f32, size: f32, fill: Color, outline: Color) {
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

// ---------------------------------------------------------------- small helpers

fn lighten(c: Color, t: f32) -> Color {
    Color::new(
        c.r + (1.0 - c.r) * t,
        c.g + (1.0 - c.g) * t,
        c.b + (1.0 - c.b) * t,
        c.a,
    )
}

fn with_alpha(c: Color, a: f32) -> Color {
    Color::new(c.r, c.g, c.b, a)
}

fn mix(a: Color, b: Color, t: f32) -> Color {
    Color::new(
        a.r + (b.r - a.r) * t,
        a.g + (b.g - a.g) * t,
        a.b + (b.b - a.b) * t,
        a.a + (b.a - a.a) * t,
    )
}

fn ease_out(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t).powi(3)
}

// corner centers with the start angle of each quarter arc
fn round_rect_corners(x: f32, y: f32, w: f32, h: f32, r: f32) -> [(f32, f32, f32); 4] {
    [
        (x + r, y + r, PI),                // top-left
        (x + w - r, y + r, 1.5 * PI),      // top-right
        (x + w - r, y + h - r, 0.0),       // bottom-right
        (x + r, y + h - r, 0.5 * PI),      // bottom-left
    ]
}

// built from non-overlapping pieces so translucent fills blend evenly
fn draw_round_rect(x: f32, y: f32, w: f32, h: f32, r: f32, col: Color) {
    let r = r.min(w * 0.5).min(h * 0.5);
    const N: usize = 8;
    draw_rectangle(x + r, y, w - 2.0 * r, h, col);
    draw_rectangle(x, y + r, r, h - 2.0 * r, col);
    draw_rectangle(x + w - r, y + r, r, h - 2.0 * r, col);
    for (cx, cy, a0) in round_rect_corners(x, y, w, h, r) {
        for i in 0..N {
            let t0 = a0 + i as f32 / N as f32 * 0.5 * PI;
            let t1 = a0 + (i + 1) as f32 / N as f32 * 0.5 * PI;
            draw_triangle(
                vec2(cx, cy),
                vec2(cx + r * t0.cos(), cy + r * t0.sin()),
                vec2(cx + r * t1.cos(), cy + r * t1.sin()),
                col,
            );
        }
    }
}

// fill plus a true border ring around it — no stacked layers, so a faded
// frame has one uniform transparency everywhere
fn draw_round_frame(x: f32, y: f32, w: f32, h: f32, r: f32, bw: f32, fill: Color, border: Color) {
    let r = r.min(w * 0.5).min(h * 0.5);
    const N: usize = 8;
    draw_round_rect(x, y, w, h, r, fill);
    // a hair of inward overlap hides AA seams between fill and ring
    let ri = r - 0.4;
    draw_rectangle(x + r, y - bw, w - 2.0 * r, bw + 0.4, border);
    draw_rectangle(x + r, y + h - 0.4, w - 2.0 * r, bw + 0.4, border);
    draw_rectangle(x - bw, y + r, bw + 0.4, h - 2.0 * r, border);
    draw_rectangle(x + w - 0.4, y + r, bw + 0.4, h - 2.0 * r, border);
    for (cx, cy, a0) in round_rect_corners(x, y, w, h, r) {
        for i in 0..N {
            let t0 = a0 + i as f32 / N as f32 * 0.5 * PI;
            let t1 = a0 + (i + 1) as f32 / N as f32 * 0.5 * PI;
            let (c0, s0) = (t0.cos(), t0.sin());
            let (c1, s1) = (t1.cos(), t1.sin());
            let p0i = vec2(cx + ri * c0, cy + ri * s0);
            let p0o = vec2(cx + (r + bw) * c0, cy + (r + bw) * s0);
            let p1i = vec2(cx + ri * c1, cy + ri * s1);
            let p1o = vec2(cx + (r + bw) * c1, cy + (r + bw) * s1);
            draw_triangle(p0i, p0o, p1i, border);
            draw_triangle(p1i, p0o, p1o, border);
        }
    }
}

// ---------------------------------------------------------------- sound
// All effects are synthesized at startup into in-memory WAVs — no asset files.

const SND_RATE: u32 = 44_100;

fn wav_bytes(samples: &[f32]) -> Vec<u8> {
    let n = samples.len() as u32 * 2;
    let mut d = Vec::with_capacity(44 + samples.len() * 2);
    d.extend_from_slice(b"RIFF");
    d.extend_from_slice(&(36 + n).to_le_bytes());
    d.extend_from_slice(b"WAVEfmt ");
    d.extend_from_slice(&16u32.to_le_bytes());
    d.extend_from_slice(&1u16.to_le_bytes()); // PCM
    d.extend_from_slice(&1u16.to_le_bytes()); // mono
    d.extend_from_slice(&SND_RATE.to_le_bytes());
    d.extend_from_slice(&(SND_RATE * 2).to_le_bytes());
    d.extend_from_slice(&2u16.to_le_bytes());
    d.extend_from_slice(&16u16.to_le_bytes());
    d.extend_from_slice(b"data");
    d.extend_from_slice(&n.to_le_bytes());
    for &s in samples {
        d.extend_from_slice(&((s.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes());
    }
    d
}

// a tone sliding from f0 to f1 with exponential decay
fn glide(f0: f32, f1: f32, dur: f32, amp: f32, decay: f32) -> Vec<f32> {
    let n = (dur * SND_RATE as f32) as usize;
    let mut phase = 0.0f32;
    (0..n)
        .map(|i| {
            let t = i as f32 / SND_RATE as f32;
            let f = f0 + (f1 - f0) * (t / dur);
            phase += std::f32::consts::TAU * f / SND_RATE as f32;
            phase.sin() * amp * (-t * decay).exp()
        })
        .collect()
}

fn add_at(base: &mut Vec<f32>, add: Vec<f32>, offset: f32) {
    let off = (offset * SND_RATE as f32) as usize;
    if base.len() < off + add.len() {
        base.resize(off + add.len(), 0.0);
    }
    for (i, s) in add.into_iter().enumerate() {
        base[off + i] += s;
    }
}

// soft dice tumble, like dice dropped on felt: heavily low-passed noise with
// gentle attacks and a low warm thump under each bounce
fn rattle() -> Vec<f32> {
    let dur = 0.38f32;
    let n = (dur * SND_RATE as f32) as usize;
    let mut seed = 0x1234_5678u32;
    let bursts = [0.0f32, 0.1, 0.19];
    let mut lp = 0.0f32;
    (0..n)
        .map(|i| {
            let t = i as f32 / SND_RATE as f32;
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let noise = (seed >> 16) as f32 / 32768.0 - 1.0;
            lp += 0.05 * (noise - lp); // very mellow: only the low rumble remains
            let mut s = 0.0f32;
            for &b in &bursts {
                if t >= b {
                    let a = t - b;
                    let env = (a * 120.0).min(1.0) * (-a * 30.0).exp();
                    s += lp * 1.6 * env
                        + (a * 150.0 * std::f32::consts::TAU).sin() * 0.22 * env;
                }
            }
            s * 0.8
        })
        .collect()
}

#[derive(Clone, Copy)]
enum Snd {
    Click,
    Select,
    Invalid,
    Roll,
    Capture,
    Repel,
    Reinforce,
    Turn,
    Eliminated,
    Win,
    Lose,
}

struct Sounds {
    click: Sound,
    select: Sound,
    invalid: Sound,
    roll: Sound,
    capture: Sound,
    repel: Sound,
    reinforce: Sound,
    turn: Sound,
    eliminated: Sound,
    win: Sound,
    lose: Sound,
}

impl Sounds {
    // None if the audio device is unavailable — the game then runs silent
    async fn load() -> Option<Sounds> {
        async fn snd(samples: Vec<f32>) -> Option<Sound> {
            load_sound_from_bytes(&wav_bytes(&samples)).await.ok()
        }
        let capture = {
            let mut v = glide(523.0, 523.0, 0.30, 0.32, 9.0);
            add_at(&mut v, glide(659.0, 659.0, 0.24, 0.28, 9.0), 0.07);
            add_at(&mut v, glide(784.0, 784.0, 0.22, 0.28, 8.0), 0.14);
            v
        };
        let reinforce = {
            let mut v = glide(720.0, 900.0, 0.08, 0.3, 22.0);
            add_at(&mut v, glide(900.0, 1120.0, 0.08, 0.3, 22.0), 0.09);
            v
        };
        let win = {
            let mut v = glide(523.0, 523.0, 0.5, 0.3, 5.0);
            add_at(&mut v, glide(659.0, 659.0, 0.45, 0.28, 5.0), 0.12);
            add_at(&mut v, glide(784.0, 784.0, 0.4, 0.28, 4.5), 0.24);
            add_at(&mut v, glide(1046.0, 1046.0, 0.6, 0.3, 3.5), 0.36);
            v
        };
        Some(Sounds {
            click: snd(glide(900.0, 840.0, 0.05, 0.5, 60.0)).await?,
            select: snd(glide(1250.0, 1180.0, 0.06, 0.4, 55.0)).await?,
            invalid: snd(glide(220.0, 140.0, 0.18, 0.45, 15.0)).await?,
            roll: snd(rattle()).await?,
            capture: snd(capture).await?,
            repel: snd(glide(260.0, 150.0, 0.3, 0.3, 8.0)).await?,
            reinforce: snd(reinforce).await?,
            turn: snd(glide(600.0, 660.0, 0.09, 0.28, 26.0)).await?,
            eliminated: {
                let mut v = glide(440.0, 440.0, 0.16, 0.35, 12.0);
                add_at(&mut v, glide(294.0, 294.0, 0.3, 0.35, 8.0), 0.14);
                snd(v).await?
            },
            win: snd(win).await?,
            lose: snd(glide(300.0, 80.0, 0.8, 0.4, 3.5)).await?,
        })
    }

    fn play(&self, s: Snd) {
        let (sound, volume) = match s {
            Snd::Click => (&self.click, 0.5),
            Snd::Select => (&self.select, 0.5),
            Snd::Invalid => (&self.invalid, 0.5),
            Snd::Roll => (&self.roll, 0.32),
            Snd::Capture => (&self.capture, 0.48),
            Snd::Repel => (&self.repel, 0.42),
            Snd::Reinforce => (&self.reinforce, 0.5),
            Snd::Turn => (&self.turn, 0.5),
            Snd::Eliminated => (&self.eliminated, 0.6),
            Snd::Win => (&self.win, 0.7),
            Snd::Lose => (&self.lose, 0.7),
        };
        play_sound(
            sound,
            PlaySoundParams {
                looped: false,
                volume,
            },
        );
    }
}

// ---------------------------------------------------------------- hex grid
// Pointy-top hexes, odd-r offset layout. Directions: 0=E 1=SE 2=SW 3=W 4=NW 5=NE.

fn cell_pos(i: usize) -> Vec2 {
    let c = (i % COLS) as f32;
    let r = i / COLS;
    let shift = if r % 2 == 1 { 0.5 } else { 0.0 };
    vec2(
        OFF_X + SQRT3 * HEX * (c + shift),
        OFF_Y + 1.5 * HEX * r as f32,
    )
}

fn neighbor_of(c: usize, r: usize, d: usize) -> Option<usize> {
    let odd = (r % 2) as i32;
    let (dc, dr): (i32, i32) = match d {
        0 => (1, 0),
        1 => (odd, 1),
        2 => (odd - 1, 1),
        3 => (-1, 0),
        4 => (odd - 1, -1),
        _ => (odd, -1),
    };
    let nc = c as i32 + dc;
    let nr = r as i32 + dr;
    if nc < 0 || nr < 0 || nc >= COLS as i32 || nr >= ROWS as i32 {
        None
    } else {
        Some(nr as usize * COLS + nc as usize)
    }
}

fn cell_neighbors(i: usize) -> Vec<usize> {
    let (c, r) = (i % COLS, i / COLS);
    (0..6).filter_map(|d| neighbor_of(c, r, d)).collect()
}

fn fill_hex(p: Vec2, s: f32, col: Color) {
    let mut pts = [Vec2::ZERO; 6];
    for (k, pt) in pts.iter_mut().enumerate() {
        let a = (60.0 * k as f32 + 30.0).to_radians();
        *pt = p + s * vec2(a.cos(), a.sin());
    }
    for k in 1..5 {
        draw_triangle(pts[0], pts[k], pts[k + 1], col);
    }
}

// ---------------------------------------------------------------- game state

// everything needed to replay a finished game (the map itself comes from the seed)
#[derive(Clone)]
enum RepEvent {
    Attack {
        a: usize,
        d: usize,
        ra: u32,
        rd: u32,
        captured: bool,
    },
    Reinforce {
        player: usize,
        lands: Vec<usize>,
    },
}

struct Territory {
    owner: usize,
    dice: u32,
    neighbors: Vec<usize>,
    anchor: Vec2,
    seed: u32, // stable pseudo-random die faces
}

// per-territory animation state, all timers run 1 -> 0
#[derive(Clone, Copy, Default)]
struct TerrFx {
    shake: f32,
    bounce: f32,
    flash: f32,
}

#[derive(Clone)]
struct Battle {
    a: usize,
    d: usize,
    ra: u32,
    rd: u32,
    ad: Vec<u8>, // attacker's individual dice
    dd: Vec<u8>, // defender's individual dice
    captured: bool,
    t: f32,
}

struct FloatText {
    pos: Vec2,
    text: String,
    col: Color,
    size: f32,
    t: f32,
    life: f32,
}

struct Game {
    players: usize,
    humans: usize, // players 0..humans are hotseat humans, the rest are AI
    difficulty: Difficulty,
    personas: Vec<Persona>, // one per player; index 0 (human) is unused
    seed: u64,
    reserve: Vec<u32>, // per player: reinforcements stored when all lands are full (max 64)
    recording: bool,   // live game: record events; replays don't
    net_guest: bool,   // this instance mirrors a remote host: no AI, no dice rolls
    my_seat: usize,    // which seat this screen controls (0 = host/local)
    events: Vec<RepEvent>,
    cell_terr: Vec<i32>, // -1 = not part of the map
    terrs: Vec<Territory>,
    fx: Vec<TerrFx>,
    current: usize,
    selected: Option<usize>,
    log: Vec<String>, // recent events, newest last
    hint: String,     // why the last click was invalid
    hint_t: f32,
    ai_timer: f32,
    over: Option<String>,
    over_t: f32,
    battle: Option<Battle>,
    floats: Vec<FloatText>,
    banner: String,
    banner_t: f32,
    time: f32,
    snd: Vec<Snd>, // sound events, drained and played by the main loop
}

fn shuffle<T>(v: &mut [T]) {
    for i in (1..v.len()).rev() {
        let j = gen_range(0, i + 1);
        v.swap(i, j);
    }
}

// partition a known dice total back into n plausible faces (display only)
fn split_sum(sum: u32, n: u32) -> Vec<u8> {
    let n = n.max(1);
    let mut v = vec![1u8; n as usize];
    let mut rem = sum.saturating_sub(n).min(n * 5);
    let mut i = 0usize;
    while rem > 0 {
        let idx = i % n as usize;
        let room = 6 - v[idx] as u32;
        let add = rem.min(room).min(gen_range(1u32, 6u32));
        v[idx] += add as u8;
        rem -= add;
        i += 1;
    }
    shuffle(&mut v);
    v
}

// exact probability that `a` dice roll a strictly higher sum than `d` dice
fn win_chance(a: u32, d: u32) -> f32 {
    fn dist(n: u32) -> Vec<f64> {
        let mut v = vec![1.0f64];
        for _ in 0..n {
            let mut nv = vec![0.0; v.len() + 6];
            for (s, p) in v.iter().enumerate() {
                for f in 1..=6usize {
                    nv[s + f] += p / 6.0;
                }
            }
            v = nv;
        }
        v
    }
    let da = dist(a);
    let dd = dist(d);
    let mut p = 0.0;
    for (sa, pa) in da.iter().enumerate() {
        for (sd, pd) in dd.iter().enumerate() {
            if sa > sd {
                p += pa * pd;
            }
        }
    }
    p as f32
}

fn random_seed() -> u64 {
    let t = (macroquad::miniquad::date::now() * 1000.0) as u64;
    (t ^ ((macroquad::rand::rand() as u64) << 16)) % SEED_MOD
}

// seed the global RNG, then generate: the same seed always yields the same map
fn gen_map(seed: u64, players: usize, humans: usize, difficulty: Difficulty) -> Game {
    macroquad::rand::srand(seed.wrapping_add(0x9E37_79B9));
    HUMANS_N.store(humans.clamp(1, players), Ordering::Relaxed);
    let mut g = Game::new(players, humans.clamp(1, players), difficulty);
    g.seed = seed;
    g
}

impl Game {
    fn new(players: usize, humans: usize, difficulty: Difficulty) -> Self {
        'gen: loop {
            let total = COLS * ROWS;

            // Grow N_SEEDS organic regions with a randomized multi-source flood fill.
            let mut cell_terr = vec![-1i32; total];
            let mut picked = HashSet::new();
            let mut frontier: Vec<usize> = Vec::new();
            let mut next_id = 0i32;
            while picked.len() < N_SEEDS {
                let c = gen_range(0, total);
                if picked.insert(c) {
                    cell_terr[c] = next_id;
                    next_id += 1;
                    frontier.push(c);
                }
            }
            while !frontier.is_empty() {
                let k = gen_range(0, frontier.len());
                let cell = frontier[k];
                let free: Vec<usize> = cell_neighbors(cell)
                    .into_iter()
                    .filter(|&n| cell_terr[n] == -1)
                    .collect();
                if free.is_empty() {
                    frontier.swap_remove(k);
                    continue;
                }
                let n = free[gen_range(0, free.len())];
                cell_terr[n] = cell_terr[cell];
                frontier.push(n);
            }

            // Delete a few regions so the map gets an irregular outline and
            // holes — removing extras until every player gets the same count
            let mut holes_n = N_HOLES;
            while (N_SEEDS - holes_n) % players != 0 {
                holes_n += 1;
            }
            let mut ids: Vec<i32> = (0..N_SEEDS as i32).collect();
            shuffle(&mut ids);
            let holes: HashSet<i32> = ids[..holes_n].iter().copied().collect();
            let mut remap = vec![-1i32; N_SEEDS];
            for (new_id, &old) in ids[holes_n..].iter().enumerate() {
                remap[old as usize] = new_id as i32;
            }
            for t in cell_terr.iter_mut() {
                *t = if *t >= 0 && !holes.contains(t) {
                    remap[*t as usize]
                } else {
                    -1
                };
            }
            let n_terr = N_SEEDS - holes_n;

            let mut cells: Vec<Vec<usize>> = vec![Vec::new(); n_terr];
            for (i, &t) in cell_terr.iter().enumerate() {
                if t >= 0 {
                    cells[t as usize].push(i);
                }
            }
            if cells.iter().any(|c| c.len() < 3) {
                continue 'gen;
            }

            // Territory adjacency from cell adjacency.
            let mut neigh: Vec<HashSet<usize>> = vec![HashSet::new(); n_terr];
            for (i, &t) in cell_terr.iter().enumerate() {
                if t < 0 {
                    continue;
                }
                for n in cell_neighbors(i) {
                    let u = cell_terr[n];
                    if u >= 0 && u != t {
                        neigh[t as usize].insert(u as usize);
                    }
                }
            }

            // The whole map must be connected, otherwise it can be unwinnable.
            let mut seen = vec![false; n_terr];
            let mut stack = vec![0usize];
            seen[0] = true;
            let mut reached = 1;
            while let Some(t) = stack.pop() {
                for &n in &neigh[t] {
                    if !seen[n] {
                        seen[n] = true;
                        reached += 1;
                        stack.push(n);
                    }
                }
            }
            if reached != n_terr {
                continue 'gen;
            }

            let mut terrs: Vec<Territory> = (0..n_terr)
                .map(|t| {
                    let centroid = cells[t]
                        .iter()
                        .fold(Vec2::ZERO, |acc, &c| acc + cell_pos(c))
                        / cells[t].len() as f32;
                    let anchor_cell = *cells[t]
                        .iter()
                        .min_by(|&&a, &&b| {
                            cell_pos(a)
                                .distance(centroid)
                                .total_cmp(&cell_pos(b).distance(centroid))
                        })
                        .unwrap();
                    Territory {
                        owner: 0,
                        dice: 1,
                        neighbors: neigh[t].iter().copied().collect(),
                        anchor: cell_pos(anchor_cell),
                        seed: gen_range(0u32, 1_000_000u32),
                    }
                })
                .collect();

            // Deal territories so every player's biggest starting cluster is
            // comparable — the deal lottery used to decide games at generation
            let mut order: Vec<usize> = (0..n_terr).collect();
            let mut best_assign: Vec<usize> = Vec::new();
            let mut best_spread = usize::MAX;
            for _ in 0..300 {
                shuffle(&mut order);
                let mut owner_of = vec![0usize; n_terr];
                for (i, &t) in order.iter().enumerate() {
                    owner_of[t] = i % players;
                }
                let (mut mn, mut mx) = (usize::MAX, 0usize);
                for p in 0..players {
                    let mut seen = vec![false; n_terr];
                    let mut best = 0usize;
                    for s in 0..n_terr {
                        if owner_of[s] != p || seen[s] {
                            continue;
                        }
                        let mut stack = vec![s];
                        seen[s] = true;
                        let mut size = 0usize;
                        while let Some(t) = stack.pop() {
                            size += 1;
                            for &nb in &neigh[t] {
                                if !seen[nb] && owner_of[nb] == p {
                                    seen[nb] = true;
                                    stack.push(nb);
                                }
                            }
                        }
                        best = best.max(size);
                    }
                    mn = mn.min(best);
                    mx = mx.max(best);
                }
                let spread = mx - mn;
                if spread < best_spread {
                    best_spread = spread;
                    best_assign = owner_of.clone();
                    if spread <= 1 {
                        break;
                    }
                }
            }
            for (t, owner) in best_assign.iter().enumerate() {
                terrs[t].owner = *owner;
            }
            for p in 0..players {
                let own: Vec<usize> = (0..n_terr).filter(|&t| terrs[t].owner == p).collect();
                for _ in 0..own.len() as u32 * 2 {
                    let cand: Vec<usize> = own
                        .iter()
                        .copied()
                        .filter(|&t| terrs[t].dice < MAX_DICE)
                        .collect();
                    if cand.is_empty() {
                        break;
                    }
                    terrs[cand[gen_range(0, cand.len())]].dice += 1;
                }
            }

            // seeded like the map, so a bookmarked seed replays the same cast
            let personas: Vec<Persona> = (0..players)
                .map(|_| match gen_range(0, 5) {
                    0 => Persona::Aggressive,
                    1 => Persona::Defensive,
                    2 => Persona::Greedy,
                    3 => Persona::Balancer,
                    _ => Persona::Chaotic,
                })
                .collect();

            // rotate who moves first: it's decided by the seed, not the host
            let first = gen_range(0, players);

            return Game {
                players,
                humans,
                difficulty,
                personas,
                seed: 0,
                reserve: vec![0; players],
                recording: true,
                net_guest: false,
                my_seat: 0,
                events: Vec::new(),
                cell_terr,
                fx: vec![TerrFx::default(); terrs.len()],
                terrs,
                current: first,
                selected: None,
                log: vec!["A new war begins!".to_string()],
                hint: String::new(),
                hint_t: 0.0,
                ai_timer: 0.0,
                over: None,
                over_t: 0.0,
                battle: None,
                floats: Vec::new(),
                banner: if first == HUMAN && humans == 1 {
                    "Your turn".to_string()
                } else {
                    format!("{}'s turn", player_name(first))
                },
                banner_t: 1.6,
                time: 0.0,
                snd: Vec::new(),
            };
        }
    }

    fn count(&self, p: usize) -> usize {
        self.terrs.iter().filter(|t| t.owner == p).count()
    }

    fn largest_region(&self, p: usize) -> u32 {
        let n = self.terrs.len();
        let mut seen = vec![false; n];
        let mut best = 0u32;
        for start in 0..n {
            if seen[start] || self.terrs[start].owner != p {
                continue;
            }
            let mut size = 0u32;
            let mut stack = vec![start];
            seen[start] = true;
            while let Some(t) = stack.pop() {
                size += 1;
                for &nb in &self.terrs[t].neighbors {
                    if !seen[nb] && self.terrs[nb].owner == p {
                        seen[nb] = true;
                        stack.push(nb);
                    }
                }
            }
            best = best.max(size);
        }
        best
    }

    // -------------------------------------------------- turn / battle logic

    fn begin_attack(&mut self, a: usize, d: usize) {
        let ad: Vec<u8> = (0..self.terrs[a].dice)
            .map(|_| gen_range(1u32, 7u32) as u8)
            .collect();
        let dd: Vec<u8> = (0..self.terrs[d].dice)
            .map(|_| gen_range(1u32, 7u32) as u8)
            .collect();
        let ra: u32 = ad.iter().map(|&x| x as u32).sum();
        let rd: u32 = dd.iter().map(|&x| x as u32).sum();
        self.battle = Some(Battle {
            a,
            d,
            ra,
            rd,
            ad,
            dd,
            captured: ra > rd,
            t: 0.0,
        });
        // recorded (and broadcast) the moment the battle starts, so remote
        // players watch the same animation at the same time
        if self.recording {
            self.events.push(RepEvent::Attack {
                a,
                d,
                ra,
                rd,
                captured: ra > rd,
            });
        }
        self.fx[a].shake = 1.0;
        self.fx[d].shake = 1.0;
        self.selected = None;
        self.snd.push(Snd::Roll);
    }

    fn push_log(&mut self, s: String) {
        self.log.push(s);
        if self.log.len() > 6 {
            self.log.remove(0);
        }
    }

    fn set_hint(&mut self, s: &str) {
        self.hint = s.to_string();
        self.hint_t = 3.0;
        self.snd.push(Snd::Invalid);
    }

    fn apply_battle(&mut self, b: Battle) {
        let atk = self.terrs[b.a].owner;
        let def = self.terrs[b.d].owner;
        if b.captured {
            self.terrs[b.d].owner = atk;
            self.terrs[b.d].dice = self.terrs[b.a].dice - 1;
            self.fx[b.d].flash = 1.0;
            self.fx[b.d].bounce = 1.0;
            self.push_log(format!(
                "{} rolled {} vs {} {} — captured!",
                player_name(atk), b.ra, player_name(def), b.rd
            ));
        } else {
            self.fx[b.a].bounce = 1.0;
            self.push_log(format!(
                "{} rolled {} vs {} {} — repelled!",
                player_name(atk), b.ra, player_name(def), b.rd
            ));
        }
        self.terrs[b.a].dice = 1;
        self.snd.push(if b.captured { Snd::Capture } else { Snd::Repel });
        if b.captured && self.count(def) == 0 {
            self.push_log(format!("{} eliminated!", player_name(def)));
            self.snd.push(Snd::Eliminated);
        }

        if !self.recording && !self.net_guest {
            return; // pure replays never end the game themselves
        }

        let humans_alive = (0..self.humans).filter(|&p| self.count(p) > 0).count();
        if humans_alive == 0 {
            self.over = Some(if self.humans == 1 {
                "You have been wiped out!".to_string()
            } else {
                "The AI has conquered you all!".to_string()
            });
            self.banner_t = 0.0;
            self.snd.push(Snd::Lose);
        } else if let Some(w) = (0..self.players).find(|&p| self.count(p) == self.terrs.len()) {
            self.over = Some(if w == HUMAN && self.humans == 1 {
                "You conquered the whole map!".to_string()
            } else {
                format!("{} conquered the whole map!", player_name(w))
            });
            self.banner_t = 0.0;
            self.snd.push(if w < self.humans { Snd::Win } else { Snd::Lose });
        }
    }

    fn end_turn(&mut self) {
        let p = self.current;
        // stored dice from earlier turns join this turn's reinforcements
        let n = self.largest_region(p) + self.reserve[p];
        let mut reinforced: Vec<usize> = Vec::new();
        for _ in 0..n {
            let mut cand: Vec<usize> = (0..self.terrs.len())
                .filter(|&t| self.terrs[t].owner == p && self.terrs[t].dice < MAX_DICE)
                .collect();
            if cand.is_empty() {
                break;
            }
            // bots stack their borders instead of scattering dice inland
            if self.difficulty != Difficulty::Easy && p >= self.humans {
                let front: Vec<usize> = cand
                    .iter()
                    .copied()
                    .filter(|&t| {
                        self.terrs[t]
                            .neighbors
                            .iter()
                            .any(|&nb| self.terrs[nb].owner != p)
                    })
                    .collect();
                if !front.is_empty() {
                    cand = front;
                }
            }
            let t = cand[gen_range(0, cand.len())];
            self.terrs[t].dice += 1;
            self.fx[t].bounce = 1.0;
            reinforced.push(t);
        }
        // whatever could not be placed is stored off-board, capped like the original
        self.reserve[p] = (n - reinforced.len() as u32).min(64);
        if let Some(&t) = reinforced.first() {
            self.floats.push(FloatText {
                pos: self.terrs[t].anchor + vec2(0.0, -92.0),
                text: format!("+{}", reinforced.len()),
                col: palette(p).dark,
                size: 36.0,
                t: 0.0,
                life: 1.4,
            });
        }
        if self.reserve[p] > 0 {
            self.push_log(format!(
                "{} reinforced with {} dice ({} stored).",
                player_name(p),
                reinforced.len(),
                self.reserve[p]
            ));
        } else {
            self.push_log(format!(
                "{} reinforced with {} dice.",
                player_name(p),
                reinforced.len()
            ));
        }
        if self.recording {
            self.events.push(RepEvent::Reinforce {
                player: p,
                lands: reinforced.clone(),
            });
        }
        if p < self.humans && !reinforced.is_empty() {
            self.snd.push(Snd::Reinforce);
        }

        self.advance_turn();
    }

    fn advance_turn(&mut self) {
        for step in 1..=self.players {
            let q = (self.current + step) % self.players;
            if self.count(q) > 0 {
                self.current = q;
                break;
            }
        }
        self.selected = None;
        self.ai_timer = -0.3;
        if self.current < self.humans {
            self.snd.push(Snd::Turn);
        }
        self.banner = if self.current == HUMAN && self.humans == 1 {
            "Your turn".to_string()
        } else {
            format!("{}'s turn", player_name(self.current))
        };
        self.banner_t = 1.6;
    }

    // -------- guest-side application of host events

    fn net_apply_attack(&mut self, a: usize, d: usize, ra: u32, rd: u32, captured: bool) {
        if a >= self.terrs.len() || d >= self.terrs.len() {
            return;
        }
        let ad = split_sum(ra, self.terrs[a].dice);
        let dd = split_sum(rd, self.terrs[d].dice);
        if self.recording {
            self.events.push(RepEvent::Attack { a, d, ra, rd, captured });
        }
        self.battle = Some(Battle {
            a,
            d,
            ra,
            rd,
            ad,
            dd,
            captured,
            t: 0.0,
        });
        self.fx[a].shake = 1.0;
        self.fx[d].shake = 1.0;
        self.selected = None;
        self.snd.push(Snd::Roll);
    }

    // instant attack used while replaying a reconnect backlog
    fn net_apply_attack_fast(&mut self, a: usize, d: usize, ra: u32, rd: u32, captured: bool) {
        if a >= self.terrs.len() || d >= self.terrs.len() {
            return;
        }
        if self.recording {
            self.events.push(RepEvent::Attack { a, d, ra, rd, captured });
        }
        if captured {
            self.terrs[d].owner = self.terrs[a].owner;
            self.terrs[d].dice = self.terrs[a].dice.saturating_sub(1).max(1);
        }
        self.terrs[a].dice = 1;
    }

    fn net_apply_reinforce(&mut self, player: usize, lands: Vec<usize>) {
        // mirror the host's stock arithmetic so displays stay in sync
        let n = self.largest_region(player) + self.reserve[player];
        let mut placed = 0u32;
        let mut placed_list: Vec<usize> = Vec::new();
        for t in lands {
            if t < self.terrs.len() && self.terrs[t].dice < MAX_DICE {
                self.terrs[t].dice += 1;
                self.fx[t].bounce = 1.0;
                placed += 1;
                placed_list.push(t);
            }
        }
        if self.recording {
            self.events.push(RepEvent::Reinforce {
                player,
                lands: placed_list,
            });
        }
        self.reserve[player] = n.saturating_sub(placed).min(64);
        self.push_log(format!("{} reinforced with {} dice.", player_name(player), placed));
        if player < self.humans && placed > 0 {
            self.snd.push(Snd::Reinforce);
        }
        self.advance_turn();
    }

    // Pick the AI's next attack based on its personality and the difficulty.
    fn best_ai_attack(&self) -> Option<(usize, usize)> {
        let cur = self.current;
        let persona = self.personas[cur];

        // easy bots lose their nerve and end their turn early often
        if self.difficulty == Difficulty::Easy && gen_range(0.0f32, 1.0) < 0.45 {
            return None;
        }

        let mut min_adv: i32 = match persona {
            Persona::Aggressive => 0,
            Persona::Defensive => 2,
            _ => 1,
        };
        if self.difficulty == Difficulty::Easy {
            min_adv += 2;
        }

        // who is currently winning (for the schemer and the shared instinct)
        let leader = (0..self.players)
            .filter(|&p| p != cur && self.count(p) > 0)
            .max_by_key(|&p| self.count(p));
        // how dominant the leader is, 0..1 of the whole map
        let threat = leader
            .map(|p| self.count(p) as f32 / self.terrs.len() as f32)
            .unwrap_or(0.0);
        // how strongly this bot coordinates against a runaway leader
        let coord = match self.difficulty {
            Difficulty::Easy => 0.25,
            Difficulty::Normal => 1.0,
            Difficulty::Hard => 1.6,
        };

        let mut best: Option<(f32, usize, usize)> = None;
        for a in 0..self.terrs.len() {
            let ta = &self.terrs[a];
            if ta.owner != cur || ta.dice < 2 {
                continue;
            }
            for &d in &ta.neighbors {
                let td = &self.terrs[d];
                if td.owner == cur {
                    continue;
                }
                let adv = ta.dice as i32 - td.dice as i32;
                // big stacks may gamble on even odds when bold enough
                let allow_even = adv == 0
                    && self.difficulty != Difficulty::Easy
                    && (ta.dice == MAX_DICE
                        || (self.difficulty == Difficulty::Hard && ta.dice >= 6))
                    && (persona == Persona::Aggressive || self.difficulty == Difficulty::Hard);
                if adv < min_adv && !allow_even {
                    continue;
                }
                // when one player is running away with the game, stop wearing
                // each other down and turn on the leader instead
                if threat > 0.45
                    && Some(td.owner) != leader
                    && adv < 2
                    && self.difficulty != Difficulty::Easy
                {
                    continue;
                }
                let mut score = adv as f32 * 10.0 + ta.dice as f32;
                if threat > 0.34 {
                    if Some(td.owner) == leader {
                        score += (threat - 0.3) * 90.0 * coord;
                    } else {
                        score -= (threat - 0.3) * 60.0 * coord;
                    }
                }
                if self.difficulty != Difficulty::Easy {
                    // prefer attacking from positions with friendly backup
                    let backup = ta
                        .neighbors
                        .iter()
                        .filter(|&&n| self.terrs[n].owner == cur)
                        .count();
                    score += backup as f32 * 1.5;
                }
                if self.difficulty == Difficulty::Easy {
                    score += gen_range(0.0f32, 40.0); // sloppy target choice
                }
                if self.difficulty == Difficulty::Hard {
                    // values conquests that consolidate territory
                    let friends = td
                        .neighbors
                        .iter()
                        .filter(|&&n| self.terrs[n].owner == cur)
                        .count();
                    score += friends as f32 * 4.0;
                }
                match persona {
                    Persona::Aggressive => score += 6.0,
                    Persona::Defensive => score += adv as f32 * 10.0,
                    Persona::Greedy => {
                        let friends = td
                            .neighbors
                            .iter()
                            .filter(|&&n| self.terrs[n].owner == cur)
                            .count();
                        score += friends as f32 * 8.0;
                    }
                    Persona::Balancer => {
                        if Some(td.owner) == leader {
                            score += 25.0;
                        }
                    }
                    Persona::Chaotic => score += gen_range(0.0f32, 30.0),
                }
                if self.difficulty == Difficulty::Hard && td.owner < self.humans {
                    score += 6.0; // hard bots gang up on you
                }
                if best.map_or(true, |b| score > b.0) {
                    best = Some((score, a, d));
                }
            }
        }
        best.map(|(_, a, d)| (a, d))
    }

    // -------------------------------------------------- per-frame update

    fn update(&mut self, dt: f32) {
        self.time += dt;
        for fx in &mut self.fx {
            fx.shake = (fx.shake - dt / T_SHAKE).max(0.0);
            fx.bounce = (fx.bounce - dt / 0.55).max(0.0);
            fx.flash = (fx.flash - dt / 0.6).max(0.0);
        }
        for f in &mut self.floats {
            f.t += dt;
        }
        self.floats.retain(|f| f.t < f.life);
        self.banner_t = (self.banner_t - dt).max(0.0);
        self.hint_t = (self.hint_t - dt).max(0.0);
        if self.over.is_some() {
            self.over_t = (self.over_t + dt * 2.0).min(1.0);
        }

        // battle animation
        if let Some(b) = &mut self.battle {
            b.t += dt;
        }
        if self.battle.as_ref().map_or(false, |b| b.t >= T_BATTLE) {
            let b = self.battle.take().unwrap();
            self.apply_battle(b);
        }

        // AI acts between battles
        if self.over.is_none() && self.battle.is_none() && self.current >= self.humans && !self.net_guest {
            self.ai_timer += dt;
            if self.ai_timer >= AI_STEP {
                self.ai_timer = 0.0;
                if let Some((a, d)) = self.best_ai_attack() {
                    self.begin_attack(a, d);
                } else {
                    self.end_turn();
                }
            }
        }
    }
}

// ---------------------------------------------------------------- dice drawing

fn quad(a: Vec2, b: Vec2, c: Vec2, d: Vec2, col: Color) {
    draw_triangle(a, b, c, col);
    draw_triangle(a, c, d, col);
}

// a quad with per-corner colors: real gradients for the die faces
fn grad_quad(p: [Vec2; 4], c: [Color; 4]) {
    let v = |p: Vec2, c: Color| Vertex {
        position: vec3(p.x, p.y, 0.0),
        uv: vec2(0.0, 0.0),
        color: [
            (c.r * 255.0) as u8,
            (c.g * 255.0) as u8,
            (c.b * 255.0) as u8,
            (c.a * 255.0) as u8,
        ],
        normal: vec4(0.0, 0.0, 0.0, 0.0),
    };
    draw_mesh(&Mesh {
        vertices: vec![v(p[0], c[0]), v(p[1], c[1]), v(p[2], c[2]), v(p[3], c[3])],
        indices: vec![0, 1, 2, 0, 2, 3],
        texture: None,
    });
}

// a circle lying on a die face, projected: an ellipse spanned by the face basis
fn draw_pip(c: Vec2, bu: Vec2, bv: Vec2, rho: f32, col: Color) {
    const N: usize = 10;
    for i in 0..N {
        let t0 = i as f32 / N as f32 * std::f32::consts::TAU;
        let t1 = (i + 1) as f32 / N as f32 * std::f32::consts::TAU;
        draw_triangle(
            c,
            c + bu * (rho * t0.cos()) + bv * (rho * t0.sin()),
            c + bu * (rho * t1.cos()) + bv * (rho * t1.sin()),
            col,
        );
    }
}

fn face_val(seed: u32, idx: u32, salt: u32) -> usize {
    let mut h = seed
        .wrapping_mul(747_796_405)
        .wrapping_add(idx.wrapping_mul(2_891_336_453))
        .wrapping_add(salt.wrapping_mul(277_803_737));
    h = (h ^ (h >> 13)).wrapping_mul(1_274_126_177);
    ((h >> 16) % 6) as usize
}

// (x, y) is the bottom front corner of an isometric die of width w.
fn draw_die(x: f32, y: f32, w: f32, seed: u32, idx: u32, pal: &Palette) {
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
    let pip_at = |o: Vec2, bu: Vec2, bv: Vec2, u: f32, v: f32| {
        let c = o + bu * u + bv * v;
        draw_pip(c, bu, bv, RHO, pal.pip);
        // specular glint makes the pip read as a glossy dimple
        let s = c + (bu + bv) * (-RHO * 0.3);
        draw_circle(s.x, s.y, w * 0.022, with_alpha(WHITE, 0.5));
    };
    for &(u, v) in PIPS[lv - 1] {
        pip_at(a, l - a, at - a, u, v);
    }
    for &(u, v) in PIPS[rv - 1] {
        pip_at(a, r - a, at - a, u, v);
    }
    for &(u, v) in PIPS[tv - 1] {
        pip_at(at, lt - at, rt - at, u, v);
    }
}

fn draw_ellipse_fan(x: f32, y: f32, rx: f32, ry: f32, col: Color) {
    const N: usize = 24;
    for i in 0..N {
        let a0 = i as f32 / N as f32 * std::f32::consts::TAU;
        let a1 = (i + 1) as f32 / N as f32 * std::f32::consts::TAU;
        draw_triangle(
            vec2(x, y),
            vec2(x + rx * a0.cos(), y + ry * a0.sin()),
            vec2(x + rx * a1.cos(), y + ry * a1.sin()),
            col,
        );
    }
}

const DIE_W: f32 = 21.0;

fn draw_stack(t: &Territory, fx: TerrFx, time: f32) {
    let pal = palette(t.owner);
    let w = DIE_W;
    let ch = w * 0.62;

    // shake rattles side to side, bounce pops the stack up once
    let sx = (time * 55.0).sin() * 4.0 * fx.shake;
    let by = -(fx.bounce * PI).sin() * 9.0;
    let base = t.anchor + vec2(sx, 4.0 + by);

    let n = t.dice as usize;
    let two_cols = n > 4;
    // both columns straddle the anchor so the token stays centered on its land
    let (fdx, bdx) = if two_cols { (-w * 0.46, w * 0.46) } else { (0.0, 0.0) };

    // contact shadow, centered under the base diamond of the stack
    let qh = w * 0.26;
    draw_ellipse_fan(
        t.anchor.x,
        t.anchor.y + 4.0 - qh * 0.8,
        if two_cols { w * 1.04 } else { w * 0.62 },
        qh * 1.3,
        Color::new(0.15, 0.16, 0.30, 0.26),
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
        draw_die(base.x + fdx, base.y - i as f32 * ch, w, t.seed, i as u32, &pal);
    }
}

// each player also has a distinct symbol so color is never the only cue
fn draw_symbol(x: f32, y: f32, r: f32, player: usize, col: Color) {
    match player % MAX_PLAYERS {
        0 => draw_circle(x, y, r, col),
        1 => draw_rectangle(x - r * 0.85, y - r * 0.85, r * 1.7, r * 1.7, col),
        2 => draw_poly(x, y, 4, r * 1.15, 0.0, col),   // diamond
        3 => draw_poly(x, y, 3, r * 1.25, -90.0, col), // triangle up
        4 => draw_poly(x, y, 6, r * 1.05, 0.0, col),   // hexagon
        5 => draw_poly(x, y, 3, r * 1.25, 90.0, col),  // triangle down
        6 => draw_poly(x, y, 5, r * 1.15, -90.0, col), // pentagon
        _ => {
            // cross
            draw_rectangle(x - r * 0.35, y - r, r * 0.7, r * 2.0, col);
            draw_rectangle(x - r, y - r * 0.35, r * 2.0, r * 0.7, col);
        }
    }
}

// ---------------------------------------------------------------- game input

#[derive(Clone, Copy, PartialEq)]
enum Confirm {
    NewMap,
    Menu,
}

fn r_endturn() -> Rect {
    Rect::new(WIN_W - 216.0, 640.0, 172.0, 44.0)
}
fn r_btn_menu() -> Rect {
    Rect::new(WIN_W - 216.0, 694.0, 78.0, 30.0)
}
fn r_btn_new() -> Rect {
    Rect::new(WIN_W - 122.0, 694.0, 78.0, 30.0)
}
fn r_over_replay() -> Rect {
    Rect::new(WIN_W * 0.5 - 195.0, 392.0, 185.0, 46.0)
}
fn r_over_gif() -> Rect {
    Rect::new(WIN_W * 0.5 + 10.0, 392.0, 185.0, 46.0)
}
fn r_confirm_yes() -> Rect {
    Rect::new(WIN_W * 0.5 - 128.0, 398.0, 120.0, 44.0)
}
fn r_confirm_no() -> Rect {
    Rect::new(WIN_W * 0.5 + 8.0, 398.0, 120.0, 44.0)
}

fn pick_territory(game: &Game, p: Vec2) -> Option<usize> {
    let mut best: Option<(f32, usize)> = None;
    for i in 0..COLS * ROWS {
        let t = game.cell_terr[i];
        if t < 0 {
            continue;
        }
        let dist = cell_pos(i).distance(p);
        if dist < HEX && best.map_or(true, |b| dist < b.0) {
            best = Some((dist, t as usize));
        }
    }
    best.map(|b| b.1)
}

fn handle_input(game: &mut Game, mut net_out: Option<&mut Vec<String>>) {
    if is_key_pressed(KeyCode::Space) {
        if let Some(out) = net_out.as_deref_mut() {
            out.push("END".to_string());
        } else {
            game.end_turn();
        }
        return;
    }
    if !is_mouse_button_pressed(MouseButton::Left) {
        return;
    }
    let mp = mouse_virtual();
    if r_endturn().contains(mp) {
        game.snd.push(Snd::Click);
        if let Some(out) = net_out.as_deref_mut() {
            out.push("END".to_string());
        } else {
            game.end_turn();
        }
        return;
    }
    if r_btn_menu().contains(mp) || r_btn_new().contains(mp) {
        return; // handled in the main loop (also works during AI turns)
    }
    if (0..5).any(|i| r_icon(i).contains(mp)) {
        return; // toolbar clicks are handled in the main loop
    }
    let Some(t) = pick_territory(game, mp) else {
        game.selected = None;
        return;
    };
    let owner = game.terrs[t].owner;
    let dice = game.terrs[t].dice;
    match game.selected {
        Some(sel) if sel == t => {
            game.selected = None;
            game.snd.push(Snd::Click);
        }
        Some(sel) => {
            if owner != game.current {
                if game.terrs[sel].neighbors.contains(&t) {
                    if let Some(out) = net_out.as_deref_mut() {
                        out.push(format!("ATTACK {} {}", sel, t));
                        game.selected = None;
                    } else {
                        game.begin_attack(sel, t);
                    }
                } else {
                    game.set_hint("Too far away — you can only attack direct neighbors.");
                }
            } else if dice > 1 {
                game.selected = Some(t);
                game.snd.push(Snd::Select);
            } else {
                game.set_hint("That land has only 1 die — it cannot attack.");
            }
        }
        None => {
            if owner != game.current {
                game.set_hint("That is an enemy land — select one of your own first.");
            } else if dice > 1 {
                game.selected = Some(t);
                game.snd.push(Snd::Select);
            } else {
                game.set_hint("That land has only 1 die — it cannot attack.");
            }
        }
    }
}

// ---------------------------------------------------------------- game drawing

fn draw_board(game: &Game, ui: &Ui, chances: bool) {
    // the whole canvas is the play surface
    draw_rectangle(0.0, 0.0, WIN_W, WIN_H, SRF());

    let hover = if game.current == game.my_seat && game.over.is_none() && game.battle.is_none() {
        pick_territory(game, mouse_virtual())
    } else {
        None
    };

    // empty water cells become a quiet dot grid, so holes read as sea
    for i in 0..COLS * ROWS {
        if game.cell_terr[i] < 0 {
            let p = cell_pos(i);
            if dark_mode() {
                fill_hex(p, HEX + 0.6, Color::new(0.155, 0.17, 0.235, 1.0));
            }
            draw_circle(p.x, p.y, 2.2, SEA());
        }
    }

    // tile fills
    for i in 0..COLS * ROWS {
        let t = game.cell_terr[i];
        if t < 0 {
            continue;
        }
        let t = t as usize;
        let terr = &game.terrs[t];
        let fx = game.fx[t];
        let mut col = palette(terr.owner).fill;
        let selectable = terr.owner == game.current && terr.dice > 1;
        let attackable = game.selected.map_or(false, |s| {
            terr.owner != game.current && game.terrs[s].neighbors.contains(&t)
        });
        if game.selected == Some(t) {
            col = lighten(col, 0.38 + 0.1 * (game.time * 6.0).sin());
        } else if game.selected.is_some() && !attackable {
            // invalid tiles go fully white, only valid targets keep color
            col = SRF();
        } else if hover == Some(t) && (attackable || selectable) {
            col = lighten(col, 0.2);
        }
        if fx.flash > 0.0 {
            col = mix(col, WHITE, fx.flash * 0.85);
        }
        fill_hex(cell_pos(i), HEX + 0.6, col);
    }

    // territory borders: every hex edge whose neighbor belongs to a different territory
    for i in 0..COLS * ROWS {
        let t = game.cell_terr[i];
        if t < 0 {
            continue;
        }
        let (c, r) = (i % COLS, i / COLS);
        let pos = cell_pos(i);
        for d in 0..6 {
            let same = neighbor_of(c, r, d).map_or(false, |n| game.cell_terr[n] == t);
            if !same {
                let a0 = (d as f32 * 60.0 - 30.0).to_radians();
                let a1 = (d as f32 * 60.0 + 30.0).to_radians();
                let p0 = pos + HEX * vec2(a0.cos(), a0.sin());
                let p1 = pos + HEX * vec2(a1.cos(), a1.sin());
                // round caps at the corners keep the border joints clean
                draw_line(p0.x, p0.y, p1.x, p1.y, 2.4, BORDER());
                draw_circle(p0.x, p0.y, 1.2, BORDER());
                draw_circle(p1.x, p1.y, 1.2, BORDER());
            }
        }
    }

    // attack arrow during a battle
    if let Some(b) = &game.battle {
        let from = game.terrs[b.a].anchor + vec2(0.0, -18.0);
        let to = game.terrs[b.d].anchor + vec2(0.0, -18.0);
        let dir = (to - from).normalize_or_zero();
        let start = from + dir * 26.0;
        let end = to - dir * 30.0;
        let col = palette(game.terrs[b.a].owner).dark;
        draw_line(start.x, start.y, end.x, end.y, 10.0, col);
        let perp = vec2(-dir.y, dir.x);
        draw_triangle(end + dir * 26.0, end + perp * 15.0, end - perp * 15.0, col);
    }

    // with a land selected, point out every attack option
    if game.battle.is_none() && game.over.is_none() {
        if let Some(sel) = game.selected {
            let alpha = 0.5 + 0.2 * (game.time * 5.0).sin();
            let col = with_alpha(palette(game.current).dark, alpha);
            for &d in &game.terrs[sel].neighbors {
                if game.terrs[d].owner == game.current {
                    continue;
                }
                let from = game.terrs[sel].anchor;
                let to = game.terrs[d].anchor;
                let dir = (to - from).normalize_or_zero();
                let start = from + dir * 32.0;
                let end = to - dir * 36.0;
                draw_line(start.x, start.y, end.x, end.y, 6.9, col);
                let perp = vec2(-dir.y, dir.x);
                draw_triangle(end + dir * 18.0, end + perp * 10.4, end - perp * 10.4, col);
            }
        }
    }

    // dice stacks, back to front
    let mut order: Vec<usize> = (0..game.terrs.len()).collect();
    order.sort_by(|&a, &b| game.terrs[a].anchor.y.total_cmp(&game.terrs[b].anchor.y));
    for t in order {
        draw_stack(&game.terrs[t], game.fx[t], game.time);
    }

    // readable dice-count badge at each stack's base; in colorblind mode the
    // badge takes the owner's symbol shape so color is never the only cue
    for t in &game.terrs {
        let pal = palette(t.owner);
        let bx = t.anchor.x + 16.0;
        let by = t.anchor.y + 13.0;
        if colorblind_mode() {
            draw_symbol(bx, by, 13.0, t.owner, SRF());
            draw_symbol(bx, by, 11.0, t.owner, pal.dark);
            ui.text_centered(&t.dice.to_string(), bx, by + 4.0, 11.0, WHITE);
        } else {
            draw_circle(bx, by, 10.0, SRF());
            draw_circle(bx, by, 8.8, pal.dark);
            ui.text_centered(&t.dice.to_string(), bx, by + 4.5, 13.0, WHITE);
        }
    }

    // subtle win-chance hint on every attack option of the selected land
    if chances && game.battle.is_none() && game.over.is_none() && game.current == game.my_seat {
        if let Some(sel) = game.selected {
            for &d in &game.terrs[sel].neighbors {
                if game.terrs[d].owner == game.current {
                    continue;
                }
                let c = win_chance(game.terrs[sel].dice, game.terrs[d].dice);
                let txt = format!("{:.0}%", c * 100.0);
                let p = game.terrs[d].anchor + vec2(0.0, 32.0);
                let tw = ui.width(&txt, 12.0);
                let col = if c >= 0.6 {
                    palette(4).dark // blue: good odds (colorblind-safe pair)
                } else if c >= 0.4 {
                    INK()
                } else {
                    palette(5).dark // orange: risky
                };
                draw_round_rect(
                    p.x - tw * 0.5 - 6.0,
                    p.y - 12.0,
                    tw + 12.0,
                    16.0,
                    8.0,
                    with_alpha(SRF(), 0.75),
                );
                ui.text_centered(&txt, p.x, p.y + 0.5, 12.0, with_alpha(col, 0.85));
            }
        }
    }

    // floating roll numbers / reinforcement popups
    for f in &game.floats {
        let p = (f.t / f.life).clamp(0.0, 1.0);
        let alpha = (1.0 - p * p).clamp(0.0, 1.0);
        let y = f.pos.y - 46.0 * ease_out(p);
        let w = ui.width(&f.text, f.size);
        ui.text_outlined(
            &f.text,
            f.pos.x - w * 0.5,
            y,
            f.size,
            with_alpha(f.col, alpha),
            with_alpha(SRF(), alpha),
        );
    }
}

fn draw_panel(game: &Game, ui: &Ui, settings: &Settings) {
    let chances = settings.chances;
    let px = 16.0;
    let py = 632.0;
    let pw = WIN_W - 32.0;
    let ph = WIN_H - py - 14.0;

    let _ = (px, pw, ph);
    draw_round_rect(20.0, py - 9.0, WIN_W - 40.0, 3.0, 1.5, with_alpha(INK(), 0.12));

    let cur = game.current;
    let mp = mouse_virtual();

    // ---- who is playing, and which phase
    let turn_text = if game.over.is_some() {
        "Game over".to_string()
    } else if cur < game.humans {
        if cur == game.my_seat {
            "Your turn — attack phase".to_string()
        } else {
            format!("{}'s turn — attack phase", HUMAN_LABELS[cur])
        }
    } else {
        format!("{}'s turn (AI)", player_name(cur))
    };
    let tw = ui.width(&turn_text, 21.0);
    draw_round_frame(40.0, py + 12.0, tw + 60.0, 36.0, 12.0, 2.5, palette(cur).fill, BORDER());
    draw_symbol(62.0, py + 30.0, 8.5, cur, palette(cur).dark);
    ui.text(&turn_text, 80.0, py + 37.0, 21.0, INK_ON_FILL);

    // ---- current step instruction, invalid-action hint, or attack preview
    let hover = pick_territory(game, mp);
    let instruction = if game.battle.is_some() {
        "Battle! Watch the dice roll...".to_string()
    } else if game.over.is_some() {
        "Click anywhere to return to the menu.".to_string()
    } else if cur >= game.humans {
        "The AI players are taking their turns...".to_string()
    } else if cur != game.my_seat {
        format!("Waiting for {} to make their move...", HUMAN_LABELS[cur])
    } else if let Some(sel) = game.selected {
        let target = hover.filter(|&h| {
            game.terrs[h].owner != cur && game.terrs[sel].neighbors.contains(&h)
        });
        if let Some(tgt) = target {
            if chances {
                let c = win_chance(game.terrs[sel].dice, game.terrs[tgt].dice);
                format!(
                    "You: {} dice  vs  {}: {} dice — {:.0}% win chance. Click to attack!",
                    game.terrs[sel].dice,
                    player_name(game.terrs[tgt].owner),
                    game.terrs[tgt].dice,
                    c * 100.0
                )
            } else {
                format!(
                    "You: {} dice  vs  {}: {} dice — click to attack!",
                    game.terrs[sel].dice,
                    player_name(game.terrs[tgt].owner),
                    game.terrs[tgt].dice
                )
            }
        } else {
            "Step 2: click a glowing enemy neighbor to attack. Click again or ESC to cancel."
                .to_string()
        }
    } else {
        "Step 1: click one of your lands with 2 or more dice.".to_string()
    };
    if game.hint_t > 0.0 && game.battle.is_none() && game.over.is_none() {
        ui.text(&game.hint, 40.0, py + 76.0, 17.0, palette(2).dark); // rose = warning
    } else {
        ui.text(&instruction, 40.0, py + 76.0, 17.0, INK());
    }

    // ---- last event, small
    if let Some(last) = game.log.last() {
        ui.text(
            &format!("Last: {}", last),
            40.0,
            py + 98.0,
            13.5,
            with_alpha(INK(), 0.65),
        );
    }

    // ---- buttons (right side)
    let active = cur == game.my_seat && game.over.is_none() && game.battle.is_none();
    let b = r_endturn();
    let hovered = active && b.contains(mp);
    let pressed = hovered && is_mouse_button_down(MouseButton::Left);
    let off = if pressed { 2.0 } else { 0.0 };
    if !pressed {
        draw_round_rect(b.x + 2.0, b.y + 4.0, b.w, b.h, 22.0, PANEL_SHADOW());
    }
    let fill = if hovered {
        mix(palette(HUMAN).fill, SRF(), 0.4)
    } else {
        SRF()
    };
    let edge = if active { BORDER() } else { with_alpha(BORDER(), 0.3) };
    draw_round_frame(b.x + off, b.y + off, b.w, b.h, 22.0, 2.5, fill, edge);
    let label = if active {
        format!("END TURN +{}", game.largest_region(game.my_seat) + game.reserve[game.my_seat])
    } else {
        "END TURN".to_string()
    };
    ui.text_centered(
        &label,
        b.x + off + b.w * 0.5,
        b.y + off + 29.0,
        18.0,
        if active { INK() } else { with_alpha(INK(), 0.35) },
    );
    for (r, label) in [(r_btn_menu(), "MENU"), (r_btn_new(), "NEW MAP")] {
        let h = r.contains(mp);
        let f = if h {
            mix(palette(HUMAN).fill, SRF(), 0.5)
        } else {
            SRF()
        };
        draw_round_frame(r.x, r.y, r.w, r.h, 10.0, 2.0, f, CARD_EDGE());
        ui.text_centered(label, r.x + r.w * 0.5, r.y + 21.0, 13.0, INK());
    }

    draw_player_cards(game, ui, py + 110.0, 0.0);
}

// player cards: symbol, land count, and a stored-dice badge
fn draw_player_cards(game: &Game, ui: &Ui, cy: f32, right_margin: f32) {
    let chip_w = 88.0;
    let chip_h = 48.0;
    let gap = 8.0;
    let total = game.players as f32 * chip_w + (game.players as f32 - 1.0) * gap;
    let x0 = ((WIN_W - right_margin - total) * 0.5 + 12.0).max(20.0);
    for p in 0..game.players {
        let cx = x0 + p as f32 * (chip_w + gap);
        let alive = game.count(p) > 0;
        let is_cur = p == game.current && game.over.is_none();
        if is_cur {
            draw_round_frame(cx, cy, chip_w, chip_h, 14.0, 3.0, palette(p).fill, BORDER());
        } else {
            draw_round_frame(cx, cy, chip_w, chip_h, 14.0, 2.0, SRF(), CARD_EDGE());
        }
        let a = if alive { 1.0 } else { 0.25 };
        draw_symbol(cx + 26.0, cy + 24.0, 9.0, p, with_alpha(palette(p).dark, a));
        if game.humans > 1 && p == game.my_seat {
            let bw_ = ui.width("YOU", 10.0) + 10.0;
            draw_round_rect(cx - 4.0, cy - 8.0, bw_, 16.0, 8.0, BORDER());
            ui.text_centered("YOU", cx - 4.0 + bw_ * 0.5, cy + 4.0, 10.0, SRF());
        }
        if alive {
            ui.text(
                &format!("{}", game.count(p)),
                cx + 46.0,
                cy + 32.0,
                24.0,
                if is_cur { INK_ON_FILL } else { INK() },
            );
            if game.reserve[p] > 0 {
                // stored dice as a small badge on the card corner
                let bt = format!("+{}", game.reserve[p]);
                let bw_ = ui.width(&bt, 11.0) + 10.0;
                let bx = cx + chip_w - bw_ + 6.0;
                let by = cy - 8.0;
                draw_round_rect(bx, by, bw_, 17.0, 8.5, palette(p).dark);
                ui.text_centered(&bt, bx + bw_ * 0.5, by + 13.0, 11.0, WHITE);
            }
        }
    }
}

fn r_icon(i: usize) -> Rect {
    Rect::new(WIN_W - 240.0 + i as f32 * 44.0, 18.0, 40.0, 28.0)
}

fn draw_icon_bar(settings: &Settings, ui: &Ui, mp: Vec2) {
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
    for i in 0..5 {
        let r = r_icon(i);
        let hov = r.contains(mp);
        draw_round_frame(r.x, r.y, r.w, r.h, 8.0, 1.5, if hov { mix(SRF(), palette(HUMAN).fill, 0.35) } else { SRF() }, CARD_EDGE());
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
                    for (rr, aa) in [(4.5f32, 0.9f32), (8.0, 0.55)] {
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
            _ => {
                // moon: theme toggle
                let f = if hov {
                    mix(SRF(), palette(HUMAN).fill, 0.35)
                } else {
                    SRF()
                };
                draw_circle(cx + 1.0, cy, 7.0, INK());
                draw_circle(cx + 4.5, cy - 2.5, 5.6, f);
            }
        }
    }
}

// replays show only the stats: no turn pill, instructions, or buttons
fn draw_panel_replay(game: &Game, ui: &Ui) {
    let py = 632.0;
    draw_round_rect(20.0, py - 9.0, WIN_W - 40.0, 3.0, 1.5, with_alpha(INK(), 0.12));
    if let Some(last) = game.log.last() {
        ui.text_centered(last, WIN_W * 0.5, py + 40.0, 16.0, with_alpha(INK(), 0.7));
    }
    draw_player_cards(game, ui, py + 70.0, 0.0);
}

fn draw_banner(game: &Game, ui: &Ui) {
    if game.banner_t <= 0.0 || game.over.is_some() {
        return;
    }
    let alpha = (game.banner_t / 0.35).min(1.0) * ((1.6 - game.banner_t) / 0.2).min(1.0);
    let alpha = alpha.clamp(0.0, 1.0);
    let w = ui.width(&game.banner, 28.0) + 48.0;
    let h = 48.0;
    let x = (WIN_W - w) * 0.5;
    let y = 16.0;
    let fill = palette(game.current).fill;
    draw_round_frame(x, y, w, h, 14.0, 3.0, with_alpha(fill, alpha), with_alpha(BORDER(), alpha));
    ui.text(&game.banner, x + 24.0, y + 33.0, 28.0, with_alpha(INK_ON_FILL, alpha));
}

fn draw_over(game: &Game, ui: &Ui) {
    let Some(msg) = &game.over else { return };
    let a = ease_out(game.over_t);
    draw_rectangle(0.0, 0.0, WIN_W, WIN_H, with_alpha(SRF(), 0.75 * a));

    // size the card to fit the message
    let w = (ui.width(msg, 36.0) + 100.0).max(480.0).min(WIN_W - 40.0);
    let h = 216.0;
    let x = (WIN_W - w) * 0.5;
    let y = (WIN_H - h) * 0.5 - 40.0 + 24.0 * (1.0 - a);
    draw_round_frame(x, y, w, h, 22.0, 4.0, with_alpha(SRF(), a), with_alpha(BORDER(), a));
    ui.text_centered(msg, WIN_W * 0.5, y + 70.0, 36.0, with_alpha(INK(), a));
    ui.text_centered(
        "Click anywhere else to return to the menu",
        WIN_W * 0.5,
        y + 104.0,
        15.0,
        with_alpha(INK(), 0.55 * a),
    );
    for (rct, label) in [(r_over_replay(), "WATCH REPLAY"), (r_over_gif(), "SAVE GIF")] {
        let hovered = rct.contains(mouse_virtual());
        let fill = if hovered {
            mix(palette(HUMAN).fill, WHITE, 0.35)
        } else {
            palette(HUMAN).fill
        };
        let by = y + 130.0;
        draw_round_frame(rct.x, by, rct.w, rct.h, 16.0, 2.5, with_alpha(fill, a), with_alpha(BORDER(), a));
        ui.text_centered(label, rct.x + rct.w * 0.5, by + 30.0, 18.0, with_alpha(INK_ON_FILL, a));
    }
}

fn draw_confirm(kind: Confirm, ui: &Ui) {
    draw_rectangle(0.0, 0.0, WIN_W, WIN_H, with_alpha(SRF(), 0.6));
    let w = 480.0;
    let h = 180.0;
    let x = (WIN_W - w) * 0.5;
    let y = 280.0;
    draw_round_frame(x, y, w, h, 20.0, 3.5, SRF(), BORDER());
    let title = match kind {
        Confirm::NewMap => "Start a new map?",
        Confirm::Menu => "Return to the menu?",
    };
    ui.text_centered(title, WIN_W * 0.5, y + 56.0, 28.0, INK());
    ui.text_centered(
        "The current game will be lost.",
        WIN_W * 0.5,
        y + 88.0,
        17.0,
        with_alpha(INK(), 0.7),
    );
    let m = mouse_virtual();
    let yes = r_confirm_yes();
    let no = r_confirm_no();
    let yf = if yes.contains(m) {
        mix(palette(HUMAN).fill, WHITE, 0.35)
    } else {
        palette(HUMAN).fill
    };
    draw_round_frame(yes.x, yes.y, yes.w, yes.h, 14.0, 2.5, yf, BORDER());
    ui.text_centered("YES", yes.x + yes.w * 0.5, yes.y + 29.0, 20.0, INK_ON_FILL);
    let nf = if no.contains(m) {
        mix(SRF(), INK(), 0.06)
    } else {
        SRF()
    };
    draw_round_frame(no.x, no.y, no.w, no.h, 14.0, 2.0, nf, CARD_EDGE());
    ui.text_centered("CANCEL", no.x + no.w * 0.5, no.y + 29.0, 20.0, INK());
}

fn draw_game(game: &Game, ui: &Ui, settings: &Settings) {
    draw_board(game, ui, settings.chances);
    draw_battle_overlay(game, ui);
    draw_icon_bar(settings, ui, mouse_virtual());
    draw_panel(game, ui, settings);
    draw_banner(game, ui);
    draw_over(game, ui);
}

// ---------------------------------------------------------------- network play
// Host-authoritative: guests send intents, the host validates, rolls, and
// broadcasts the resulting events (the same RepEvents replays use).

const NET_PORT_DEFAULT: u16 = 7777;
const NET_PROTO: &str = "2"; // bumped whenever map generation or messages change

// DW_PORT overrides the port (used by the automated tests)
fn net_port() -> u16 {
    std::env::var("DW_PORT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(NET_PORT_DEFAULT)
}
const NET_MAX_LINE: usize = 200; // guest -> host intents are tiny
const NET_MAX_HOST_LINE: usize = 4096; // host -> guest events can be long (REINF lists)

fn diff_idx(d: Difficulty) -> usize {
    match d {
        Difficulty::Easy => 0,
        Difficulty::Normal => 1,
        Difficulty::Hard => 2,
    }
}

fn idx_diff(i: usize) -> Difficulty {
    match i {
        0 => Difficulty::Easy,
        2 => Difficulty::Hard,
        _ => Difficulty::Normal,
    }
}

fn send_net_line(stream: &mut std::net::TcpStream, line: &str) {
    use std::io::Write;
    let _ = stream.write_all(line.as_bytes());
    let _ = stream.write_all(b"\n");
}

// read one newline-terminated line with a hard length cap; transient
// errors (interrupted syscalls, spurious timeouts on Windows) are retried
// rather than treated as a dead connection
fn read_net_line(r: &mut std::io::BufReader<std::net::TcpStream>, cap: usize) -> Option<String> {
    use std::io::Read;
    let mut buf = Vec::new();
    loop {
        let mut byte = [0u8; 1];
        match r.read(&mut byte) {
            Ok(0) => return None,
            Err(e) => match e.kind() {
                std::io::ErrorKind::Interrupted
                | std::io::ErrorKind::WouldBlock
                | std::io::ErrorKind::TimedOut => continue,
                _ => return None,
            },
            Ok(_) => {
                if byte[0] == b'\n' {
                    return String::from_utf8(buf).ok();
                }
                if buf.len() >= cap {
                    return None;
                }
                buf.push(byte[0]);
            }
        }
    }
}

enum HostMsg {
    Joined(#[allow(dead_code)] usize),
    Rejoined(usize), // lobby seat that reattached mid-game
    Left(usize),
    Intent(usize, String),
}

struct HostShared {
    seats: Vec<Option<std::net::TcpStream>>, // index = lobby seat - 1
    gens: Vec<u64>, // bumped on every (re)attach; guards stale cleanup
    tokens: HashMap<String, usize>, // reconnect token -> lobby seat
    banned: HashSet<std::net::IpAddr>,
    attempts: HashMap<std::net::IpAddr, u32>,
    last_try: HashMap<std::net::IpAddr, std::time::Instant>,
    conns: usize,
    started: bool,
}

// the active lobby's routing info; the accept thread reads this so the
// port only ever needs to be bound once per process
struct HostSession {
    code: String,
    shared: std::sync::Arc<std::sync::Mutex<HostShared>>,
    tx: std::sync::mpsc::Sender<HostMsg>,
}

static HOST_SLOT: std::sync::OnceLock<
    std::sync::Arc<std::sync::Mutex<Option<HostSession>>>,
> = std::sync::OnceLock::new();
static LISTENING: AtomicBool = AtomicBool::new(false);

fn host_slot() -> std::sync::Arc<std::sync::Mutex<Option<HostSession>>> {
    HOST_SLOT
        .get_or_init(|| std::sync::Arc::new(std::sync::Mutex::new(None)))
        .clone()
}

struct HostNet {
    rx: std::sync::mpsc::Receiver<HostMsg>,
    shared: std::sync::Arc<std::sync::Mutex<HostShared>>,
    code: String,
    remap: HashMap<usize, usize>, // lobby seat -> game seat, set at start
    sent: usize, // events broadcast so far
}

impl Drop for HostNet {
    fn drop(&mut self) {
        *host_slot().lock().unwrap() = None; // lobby gone; listener stays
    }
}

impl HostNet {
    fn start(guests: usize) -> std::io::Result<HostNet> {
        // bind the port exactly once; afterwards every lobby reuses it,
        // avoiding TIME_WAIT rebind failures when hosting again
        if !LISTENING.load(Ordering::Relaxed) {
            let listener = std::net::TcpListener::bind(("0.0.0.0", net_port()))?;
            listener.set_nonblocking(true)?;
            LISTENING.store(true, Ordering::Relaxed);
            let slot = host_slot();
            std::thread::spawn(move || loop {
                match listener.accept() {
                    Ok((stream, peer)) => {
                        let ip = peer.ip();
                        let session = {
                            let guard = slot.lock().unwrap();
                            match guard.as_ref() {
                                Some(s) => {
                                    (s.code.clone(), s.shared.clone(), s.tx.clone())
                                }
                                None => continue, // no lobby: drop the connection
                            }
                        };
                        let (code, shared, tx) = session;
                        {
                            let mut sh = shared.lock().unwrap();
                            // bans, connection cap, 1/sec per-IP rate limit
                            let too_fast = sh.last_try.get(&ip).map_or(false, |t| {
                                t.elapsed() < std::time::Duration::from_secs(1)
                            });
                            if sh.banned.contains(&ip) || sh.conns >= 8 || too_fast {
                                continue;
                            }
                            sh.last_try.insert(ip, std::time::Instant::now());
                            sh.conns += 1;
                        }
                        let _ = stream.set_nonblocking(false);
                        std::thread::spawn(move || {
                            host_handle_client(stream, ip, code, shared, tx)
                        });
                    }
                    Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(std::time::Duration::from_millis(100));
                    }
                    Err(_) => {
                        std::thread::sleep(std::time::Duration::from_millis(500));
                    }
                }
            });
        }
        let code = format!("{:04}", gen_range(0, 10000));
        let (tx, rx) = std::sync::mpsc::channel();
        let shared = std::sync::Arc::new(std::sync::Mutex::new(HostShared {
            seats: (0..guests).map(|_| None).collect(),
            gens: vec![0; guests],
            tokens: HashMap::new(),
            banned: HashSet::new(),
            attempts: HashMap::new(),
            last_try: HashMap::new(),
            conns: 0,
            started: false,
        }));
        *host_slot().lock().unwrap() = Some(HostSession {
            code: code.clone(),
            shared: shared.clone(),
            tx,
        });
        Ok(HostNet {
            rx,
            shared,
            code,
            remap: HashMap::new(),
            sent: 0,
        })
    }

    fn slots(&self) -> Vec<bool> {
        self.shared
            .lock()
            .unwrap()
            .seats
            .iter()
            .map(|s| s.is_some())
            .collect()
    }

    fn joined(&self) -> usize {
        self.shared
            .lock()
            .unwrap()
            .seats
            .iter()
            .filter(|s| s.is_some())
            .count()
    }

    fn broadcast(&mut self, line: &str) {
        use std::io::Write;
        let mut sh = self.shared.lock().unwrap();
        for slot in sh.seats.iter_mut().flatten() {
            let _ = slot
                .write_all(line.as_bytes())
                .and_then(|_| slot.write_all(b"\n"));
        }
    }

    // lock in whoever is connected, compact their seats, tell each guest
    // their game seat, and return the human count (host included)
    fn start_game(&mut self, seed: u64, players: usize, diff: Difficulty) -> usize {
        use std::io::Write;
        let mut sh = self.shared.lock().unwrap();
        sh.started = true;
        let occupied: Vec<usize> = sh
            .seats
            .iter()
            .enumerate()
            .filter_map(|(i, s)| s.as_ref().map(|_| i))
            .collect();
        let humans = occupied.len() + 1;
        self.remap.clear();
        for (k, &i) in occupied.iter().enumerate() {
            self.remap.insert(i + 1, k + 1);
            if let Some(s) = sh.seats[i].as_mut() {
                let _ = writeln!(
                    s,
                    "START {} {} {} {} {} {}",
                    seed,
                    players,
                    humans,
                    diff_idx(diff),
                    k + 1,
                    HUMAN_COLOR.load(Ordering::Relaxed)
                );
            }
        }
        humans
    }

    // resend everything a reattached guest missed, then mark it synced
    fn send_resume(&mut self, lobby_seat: usize, g: &Game, seed: u64, diff: Difficulty) {
        use std::io::Write;
        let mut sh = self.shared.lock().unwrap();
        let Some(&game_seat) = self.remap.get(&lobby_seat) else {
            return;
        };
        if let Some(s) = sh.seats[lobby_seat - 1].as_mut() {
            let _ = writeln!(
                s,
                "RESUME {} {} {} {} {} {}",
                seed,
                g.players,
                g.humans,
                diff_idx(diff),
                game_seat,
                HUMAN_COLOR.load(Ordering::Relaxed)
            );
            for ev in &g.events {
                let line = match ev {
                    RepEvent::Attack {
                        a,
                        d,
                        ra,
                        rd,
                        captured,
                    } => format!("ATK {} {} {} {} {}", a, d, ra, rd, *captured as u8),
                    RepEvent::Reinforce { player, lands } => {
                        let ls: Vec<String> =
                            lands.iter().map(|t| t.to_string()).collect();
                        format!("REINF {} {}", player, ls.join(","))
                    }
                };
                let _ = writeln!(s, "{}", line);
            }
            let _ = writeln!(s, "SYNCED");
        }
    }

    // send any newly recorded game events to all guests
    fn flush_events(&mut self, g: &Game) {
        while self.sent < g.events.len() {
            let line = match &g.events[self.sent] {
                RepEvent::Attack {
                    a,
                    d,
                    ra,
                    rd,
                    captured,
                } => format!("ATK {} {} {} {} {}", a, d, ra, rd, *captured as u8),
                RepEvent::Reinforce { player, lands } => {
                    let ls: Vec<String> = lands.iter().map(|t| t.to_string()).collect();
                    format!("REINF {} {}", player, ls.join(","))
                }
            };
            self.sent += 1;
            self.broadcast(&line);
        }
    }
}

fn host_handle_client(
    stream: std::net::TcpStream,
    ip: std::net::IpAddr,
    code: String,
    shared: std::sync::Arc<std::sync::Mutex<HostShared>>,
    tx: std::sync::mpsc::Sender<HostMsg>,
) {
    let release = |sh: &std::sync::Arc<std::sync::Mutex<HostShared>>| {
        sh.lock().unwrap().conns -= 1;
    };
    let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(10)));
    let mut reader = match stream.try_clone() {
        Ok(s) => std::io::BufReader::new(s),
        Err(_) => return release(&shared),
    };
    let mut w = stream;
    // auth: JOIN <code> for a fresh seat, REJOIN <code> <token> to reclaim one
    let mut rejoin_seat: Option<usize> = None;
    let ok = match read_net_line(&mut reader, NET_MAX_LINE) {
        Some(l) => {
            let mut it = l.split_whitespace();
            match it.next() {
                Some("JOIN") => {
                    if it.next() != Some(code.as_str()) {
                        false
                    } else if it.next() != Some(NET_PROTO) {
                        send_net_line(&mut w, "BYE version mismatch - download the latest release");
                        release(&shared);
                        return;
                    } else {
                        true
                    }
                }
                Some("REJOIN") => {
                    if it.next() == Some(code.as_str()) {
                        if let Some(tok) = it.next() {
                            let mut sh = shared.lock().unwrap();
                            if let Some(&slot_seat) = sh.tokens.get(tok) {
                                if let Ok(clone) = w.try_clone() {
                                    sh.seats[slot_seat - 1] = Some(clone);
                                    sh.gens[slot_seat - 1] += 1;
                                    rejoin_seat = Some(slot_seat);
                                }
                            }
                        }
                    }
                    rejoin_seat.is_some()
                }
                _ => false,
            }
        }
        None => false,
    };
    if !ok {
        {
            let mut sh = shared.lock().unwrap();
            let n = sh.attempts.entry(ip).or_insert(0);
            *n += 1;
            if *n >= 5 {
                sh.banned.insert(ip); // five strikes: ignored for the session
            }
            sh.conns -= 1;
        }
        send_net_line(&mut w, "BYE wrong code");
        return;
    }
    let seat = if let Some(s) = rejoin_seat {
        Some(s)
    } else {
        let mut sh = shared.lock().unwrap();
        if sh.started {
            None
        } else {
            match sh.seats.iter().position(|s| s.is_none()) {
                Some(i) => match w.try_clone() {
                    Ok(clone) => {
                        sh.seats[i] = Some(clone);
                        sh.gens[i] += 1;
                        Some(i + 1)
                    }
                    Err(_) => None,
                },
                None => None,
            }
        }
    };
    let Some(seat) = seat else {
        send_net_line(&mut w, "BYE full");
        return release(&shared);
    };
    let my_gen = shared.lock().unwrap().gens[seat - 1];
    let _ = reader.get_ref().set_read_timeout(None);
    if rejoin_seat.is_some() {
        let _ = tx.send(HostMsg::Rejoined(seat));
    } else {
        // fresh join: issue the reconnect token
        let token = {
            let t = format!(
                "{:08x}{:08x}",
                gen_range(0u32, u32::MAX),
                gen_range(0u32, u32::MAX)
            );
            let mut sh = shared.lock().unwrap();
            sh.tokens.insert(t.clone(), seat);
            t
        };
        send_net_line(&mut w, &format!("WELCOME {} {}", seat, token));
        let _ = tx.send(HostMsg::Joined(seat));
    }
    loop {
        use std::io::Read;
        let mut buf = Vec::new();
        let mut dead = None;
        loop {
            let mut byte = [0u8; 1];
            match reader.read(&mut byte) {
                Ok(0) => {
                    dead = Some("EOF".to_string());
                    break;
                }
                Err(e) => match e.kind() {
                    std::io::ErrorKind::Interrupted
                    | std::io::ErrorKind::WouldBlock
                    | std::io::ErrorKind::TimedOut => continue,
                    k => {
                        dead = Some(format!("ERR {:?} {}", k, e));
                        break;
                    }
                },
                Ok(_) => {
                    if byte[0] == b'\n' {
                        break;
                    }
                    if buf.len() >= NET_MAX_LINE {
                        dead = Some("LINE TOO LONG".to_string());
                        break;
                    }
                    buf.push(byte[0]);
                }
            }
        }
        if let Some(why) = dead {
            eprintln!("CLIENT_READ_END seat {} reason {}", seat, why);
            break;
        }
        if let Ok(l) = String::from_utf8(buf) {
            let _ = tx.send(HostMsg::Intent(seat, l));
        }
    }
    {
        let mut sh = shared.lock().unwrap();
        sh.conns -= 1;
        if sh.gens[seat - 1] != my_gen {
            return; // the seat was reclaimed by a reconnect; don't touch it
        }
        sh.seats[seat - 1] = None;
    }
    let _ = tx.send(HostMsg::Left(seat));
}

type NetParts = (
    std::net::TcpStream,
    std::sync::mpsc::Receiver<String>,
);

struct GuestNet {
    stream: std::net::TcpStream, // write half
    rx: std::sync::mpsc::Receiver<String>,
    seat: usize,
    pending: std::collections::VecDeque<RepEvent>,
    addr: String,
    code: String,
    token: String,
    catching_up: bool, // replaying the backlog after a reconnect
    rec_rx: Option<std::sync::mpsc::Receiver<Result<NetParts, String>>>,
}

// spawn a reader thread over a connected socket, yielding its line channel
fn spawn_reader(
    stream: &std::net::TcpStream,
) -> Result<std::sync::mpsc::Receiver<String>, String> {
    let mut reader = std::io::BufReader::new(
        stream.try_clone().map_err(|_| "Socket error".to_string())?,
    );
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || loop {
        match read_net_line(&mut reader, NET_MAX_HOST_LINE) {
            Some(l) => {
                if tx.send(l).is_err() {
                    break;
                }
            }
            None => {
                let _ = tx.send("LOST".to_string());
                break;
            }
        }
    });
    Ok(rx)
}

// background auto-reconnect: keep trying to reclaim our seat with the token
fn spawn_reconnect(
    addr: String,
    code: String,
    token: String,
) -> std::sync::mpsc::Receiver<Result<NetParts, String>> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let full = if addr.contains(':') {
            addr.clone()
        } else {
            format!("{}:{}", addr, net_port())
        };
        for _ in 0..10 {
            std::thread::sleep(std::time::Duration::from_secs(3));
            let Ok(sock) = full.parse::<std::net::SocketAddr>() else {
                break;
            };
            if let Ok(stream) = std::net::TcpStream::connect_timeout(
                &sock,
                std::time::Duration::from_secs(3),
            ) {
                let mut w = match stream.try_clone() {
                    Ok(w) => w,
                    Err(_) => continue,
                };
                send_net_line(&mut w, &format!("REJOIN {} {}", code, token));
                if let Ok(lines) = spawn_reader(&stream) {
                    let _ = tx.send(Ok((w, lines)));
                    return;
                }
            }
        }
        let _ = tx.send(Err("Could not reconnect".to_string()));
    });
    rx
}

fn guest_connect(addr: &str, code: &str) -> Result<GuestNet, String> {
    let addr_owned = addr.to_string();
    let code_owned = code.to_string();
    let full = if addr.contains(':') {
        addr.to_string()
    } else {
        format!("{}:{}", addr, net_port())
    };
    let sock: std::net::SocketAddr = full.parse().map_err(|_| "Bad address".to_string())?;
    let stream = std::net::TcpStream::connect_timeout(&sock, std::time::Duration::from_secs(5))
        .map_err(|_| "Could not connect — wrong address, or firewall blocking port 7777".to_string())?;
    let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(8)));
    let mut w = stream.try_clone().map_err(|_| "Socket error".to_string())?;
    send_net_line(&mut w, &format!("JOIN {} {}", code, NET_PROTO));
    let mut reader =
        std::io::BufReader::new(stream.try_clone().map_err(|_| "Socket error".to_string())?);
    let hello = read_net_line(&mut reader, NET_MAX_HOST_LINE).ok_or_else(|| {
        "Host unreachable — check the code, and the host's firewall (port 7777)".to_string()
    })?;
    let mut it = hello.split_whitespace();
    match it.next() {
        Some("WELCOME") => {}
        Some("BYE") => {
            return Err(format!("Rejected: {}", it.collect::<Vec<_>>().join(" ")));
        }
        _ => return Err("Bad reply".to_string()),
    }
    let seat: usize = it
        .next()
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| "Bad reply".to_string())?;
    let token = it.next().unwrap_or("").to_string();
    let _ = stream.set_read_timeout(None);
    drop(reader);
    let rx = spawn_reader(&stream)?;
    Ok(GuestNet {
        stream: w,
        rx,
        seat,
        pending: std::collections::VecDeque::new(),
        addr: addr_owned,
        code: code_owned,
        token,
        catching_up: false,
        rec_rx: None,
    })
}

// miniquad's clipboard is flaky on Linux desktops; fall back to the
// standard clipboard tools when they exist
fn copy_to_clipboard(text: &str) {
    miniquad::window::clipboard_set(text);
    #[cfg(target_os = "linux")]
    {
        use std::io::Write;
        use std::process::{Command, Stdio};
        for (cmd, args) in [
            ("xclip", vec!["-selection", "clipboard"]),
            ("wl-copy", vec![]),
            ("xsel", vec!["-ib"]),
        ] {
            if let Ok(mut child) = Command::new(cmd)
                .args(&args)
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
            {
                let ok = child
                    .stdin
                    .as_mut()
                    .map(|s| s.write_all(text.as_bytes()).is_ok())
                    .unwrap_or(false);
                drop(child.stdin.take());
                let _ = child.wait();
                if ok {
                    return;
                }
            }
        }
    }
}

fn paste_from_clipboard() -> Option<String> {
    if let Some(t) = miniquad::window::clipboard_get() {
        if !t.trim().is_empty() {
            return Some(t);
        }
    }
    #[cfg(target_os = "linux")]
    {
        use std::process::Command;
        for (cmd, args) in [
            ("xclip", vec!["-selection", "clipboard", "-o"]),
            ("wl-paste", vec!["-n"]),
            ("xsel", vec!["-ob"]),
        ] {
            if let Ok(out) = Command::new(cmd).args(&args).output() {
                if out.status.success() {
                    if let Ok(s) = String::from_utf8(out.stdout) {
                        if !s.trim().is_empty() {
                            return Some(s);
                        }
                    }
                }
            }
        }
    }
    None
}

// figure out the LAN address to show in the host lobby
fn local_ip() -> String {
    std::net::UdpSocket::bind("0.0.0.0:0")
        .and_then(|s| {
            s.connect("8.8.8.8:80")?;
            s.local_addr()
        })
        .map(|a| a.ip().to_string())
        .unwrap_or_else(|_| "?".to_string())
}

// ---------------------------------------------------------------- replay & gif

struct Replay {
    g: Game,
    events: Vec<RepEvent>,
    idx: usize,
    timer: f32,
    saving: bool,          // gif-capture mode: no animations, one frame per event
    frames: Vec<Vec<u8>>,  // 216-color indexed frames
    rt: Option<RenderTarget>,
    saved: Option<String>, // status message once the gif is written
}

impl Replay {
    fn new(src: &Game, saving: bool) -> Self {
        let mut g = gen_map(src.seed, src.players, src.humans, src.difficulty);
        g.recording = false;
        g.banner_t = 0.0;
        Replay {
            g,
            events: src.events.clone(),
            idx: 0,
            timer: 0.0,
            saving,
            frames: Vec::new(),
            rt: if saving {
                Some(render_target(GIF_W * 2, GIF_H * 2))
            } else {
                None
            },
            saved: None,
        }
    }

    fn finished(&self) -> bool {
        self.idx >= self.events.len() && self.g.battle.is_none()
    }

    // animated playback step
    fn step_watch(&mut self, dt: f32) {
        if self.g.battle.is_some() || self.idx >= self.events.len() {
            return;
        }
        self.timer += dt;
        if self.timer < 0.4 {
            return;
        }
        self.timer = 0.0;
        let ev = self.events[self.idx].clone();
        self.idx += 1;
        match ev {
            RepEvent::Attack {
                a,
                d,
                ra,
                rd,
                captured,
            } => {
                let ad = split_sum(ra, self.g.terrs[a].dice);
                let dd = split_sum(rd, self.g.terrs[d].dice);
                self.g.battle = Some(Battle {
                    a,
                    d,
                    ra,
                    rd,
                    ad,
                    dd,
                    captured,
                    t: 0.0,
                });
                self.g.fx[a].shake = 1.0;
                self.g.fx[d].shake = 1.0;
                self.g.snd.push(Snd::Roll);
            }
            RepEvent::Reinforce { player, lands } => {
                let n = lands.len();
                for t in lands {
                    if self.g.terrs[t].dice < MAX_DICE {
                        self.g.terrs[t].dice += 1;
                        self.g.fx[t].bounce = 1.0;
                    }
                }
                self.g
                    .push_log(format!("{} reinforced with {} dice.", player_name(player), n));
            }
        }
    }

    // instant application, used while recording the gif
    fn apply_instant(&mut self) {
        if self.idx >= self.events.len() {
            return;
        }
        let ev = self.events[self.idx].clone();
        self.idx += 1;
        match ev {
            RepEvent::Attack { a, d, captured, .. } => {
                if captured {
                    self.g.terrs[d].owner = self.g.terrs[a].owner;
                    self.g.terrs[d].dice = self.g.terrs[a].dice - 1;
                    self.g.fx[d].flash = 1.0;
                }
                self.g.terrs[a].dice = 1;
            }
            RepEvent::Reinforce { lands, .. } => {
                for t in lands {
                    if self.g.terrs[t].dice < MAX_DICE {
                        self.g.terrs[t].dice += 1;
                    }
                }
            }
        }
    }
}

// box-downsample an RGBA image by an integer factor (poor man's AA for
// render targets, which get no MSAA)
fn downsample(img: &Image, f: usize) -> Image {
    let w = img.width as usize / f;
    let h = img.height as usize / f;
    let mut bytes = vec![0u8; w * h * 4];
    for y in 0..h {
        for x in 0..w {
            let mut acc = [0u32; 4];
            for dy in 0..f {
                for dx in 0..f {
                    let i = ((y * f + dy) * img.width as usize + x * f + dx) * 4;
                    for (c, a) in acc.iter_mut().enumerate() {
                        *a += img.bytes[i + c] as u32;
                    }
                }
            }
            let o = (y * w + x) * 4;
            for (c, a) in acc.iter().enumerate() {
                bytes[o + c] = (a / (f * f) as u32) as u8;
            }
        }
    }
    Image {
        width: w as u16,
        height: h as u16,
        bytes,
    }
}

// map a render-target image to the 216-color web cube
fn quantize_image(img: &Image) -> Vec<u8> {
    let iw = img.width as usize;
    let ih = img.height as usize;
    let mut out = Vec::with_capacity(iw * ih);
    for y in 0..ih {
        for x in 0..iw {
            let i = (y * iw + x) * 4;
            let r = img.bytes[i] as usize;
            let g = img.bytes[i + 1] as usize;
            let b = img.bytes[i + 2] as usize;
            out.push(
                ((r * 5 + 127) / 255 * 36 + (g * 5 + 127) / 255 * 6 + (b * 5 + 127) / 255) as u8,
            );
        }
    }
    out
}

// the gif renders at 1.5x the virtual canvas for crisp output
const GIF_W: u32 = 1395;
const GIF_H: u32 = 1230;

struct BitWriter {
    out: Vec<u8>,
    cur: u32,
    nbits: u32,
}

impl BitWriter {
    fn put(&mut self, code: u16, size: u32) {
        self.cur |= (code as u32) << self.nbits;
        self.nbits += size;
        while self.nbits >= 8 {
            self.out.push((self.cur & 0xFF) as u8);
            self.cur >>= 8;
            self.nbits -= 8;
        }
    }
}

// standard GIF LZW with an 8-bit minimum code size
fn gif_lzw(data: &[u8]) -> Vec<u8> {
    let mut bw = BitWriter {
        out: Vec::new(),
        cur: 0,
        nbits: 0,
    };
    let clear: u16 = 256;
    let end: u16 = 257;
    let mut dict: HashMap<(u16, u8), u16> = HashMap::new();
    let mut code_size: u32 = 9;
    let mut next: u16 = 258;
    bw.put(clear, code_size);
    let mut cur = data[0] as u16;
    for &px in &data[1..] {
        if let Some(&c) = dict.get(&(cur, px)) {
            cur = c;
        } else {
            bw.put(cur, code_size);
            // the decoder's table lags one entry behind, so the width grows
            // BEFORE this insertion, not after it
            if u32::from(next) == (1 << code_size) && code_size < 12 {
                code_size += 1;
            }
            if next == 4096 {
                bw.put(clear, code_size);
                dict.clear();
                next = 258;
                code_size = 9;
            } else {
                dict.insert((cur, px), next);
                next += 1;
            }
            cur = px as u16;
        }
    }
    bw.put(cur, code_size);
    if u32::from(next) == (1 << code_size) && code_size < 12 {
        code_size += 1;
    }
    bw.put(end, code_size);
    if bw.nbits > 0 {
        bw.out.push((bw.cur & 0xFF) as u8);
    }
    bw.out
}

fn gif_save(path: &str, w: u16, h: u16, frames: &[Vec<u8>], delay_cs: u16) -> std::io::Result<()> {
    let mut out = Vec::new();
    out.extend_from_slice(b"GIF89a");
    out.extend_from_slice(&w.to_le_bytes());
    out.extend_from_slice(&h.to_le_bytes());
    out.extend_from_slice(&[0xF7, 0, 0]); // global 256-color table
    for i in 0..256usize {
        if i < 216 {
            out.push((i / 36 * 51) as u8);
            out.push((i / 6 % 6 * 51) as u8);
            out.push((i % 6 * 51) as u8);
        } else {
            out.extend_from_slice(&[0, 0, 0]);
        }
    }
    // loop forever
    out.extend_from_slice(&[0x21, 0xFF, 0x0B]);
    out.extend_from_slice(b"NETSCAPE2.0");
    out.extend_from_slice(&[3, 1, 0, 0, 0]);
    for f in frames {
        out.extend_from_slice(&[0x21, 0xF9, 4, 0]);
        out.extend_from_slice(&delay_cs.to_le_bytes());
        out.extend_from_slice(&[0, 0, 0x2C, 0, 0, 0, 0]);
        out.extend_from_slice(&w.to_le_bytes());
        out.extend_from_slice(&h.to_le_bytes());
        out.extend_from_slice(&[0, 8]); // no local table, 8-bit codes
        for chunk in gif_lzw(f).chunks(255) {
            out.push(chunk.len() as u8);
            out.extend_from_slice(chunk);
        }
        out.push(0);
    }
    out.push(0x3B);
    std::fs::write(path, out)
}

// a flat die face: rounded square with the given value's pips
fn draw_face_die(x: f32, y: f32, s: f32, val: usize, pal: &Palette) {
    draw_round_frame(x, y, s, s, s * 0.22, s * 0.06, pal.light, darken(pal.dark, 0.85));
    for &(u, v) in PIPS[val.clamp(1, 6) - 1] {
        draw_circle(x + u * s, y + v * s, s * 0.09, pal.pip);
    }
}

// the roll showcase: every die of both sides pops in, totals count up,
// and the winning total pulses
fn draw_battle_overlay(game: &Game, ui: &Ui) {
    let Some(b) = &game.battle else { return };
    let atk = game.terrs[b.a].owner;
    let def = game.terrs[b.d].owner;
    let w = 560.0;
    let h = 152.0;
    let x = (WIN_W - w) * 0.5;
    let y = (624.0 - h) * 0.5; // centered on the board
    let ap = ease_out((b.t / 0.2).min(1.0));
    // one surface: the veil dims the board and carries the duel directly
    draw_rectangle(0.0, 0.0, WIN_W, 624.0, with_alpha(SRF(), 0.82 * ap));
    let _ = h;
    let reveal = b.t - T_SHAKE * 0.6;
    for (row, (player, dice)) in [(atk, &b.ad), (def, &b.dd)].into_iter().enumerate() {
        let ry = y + 44.0 + row as f32 * 66.0;
        let pal = palette(player);
        draw_symbol(x + 32.0, ry, 9.0, player, with_alpha(pal.dark, ap));
        let ds = 32.0;
        let mut shown: u32 = 0;
        for (i, &v) in dice.iter().enumerate() {
            let td = reveal - i as f32 * 0.08;
            if td < 0.0 {
                continue;
            }
            let pop = ease_out((td / 0.14).min(1.0));
            let s = ds * (0.55 + 0.45 * pop);
            draw_face_die(
                x + 58.0 + i as f32 * (ds + 6.0) + (ds - s) * 0.5,
                ry - s * 0.5,
                s,
                v as usize,
                &pal,
            );
            shown += v as u32;
        }
        let done = reveal > dice.len() as f32 * 0.08 + 0.14;
        let win = if b.captured { row == 0 } else { row == 1 };
        let (size, col) = if done && win {
            (
                36.0 + 3.0 * (game.time * 9.0).sin(),
                with_alpha(pal.dark, ap),
            )
        } else if done {
            (30.0, with_alpha(INK(), 0.4 * ap))
        } else {
            (30.0, with_alpha(INK(), ap))
        };
        ui.text_centered(&format!("{}", shown), x + w - 172.0, ry + 11.0, size, col);
    }
}

fn draw_replay_hud(r: &Replay, ui: &Ui) {
    let txt = if let Some(msg) = &r.saved {
        format!("{} — click or ESC for menu", msg)
    } else if r.saving {
        format!("Recording GIF... {}/{}", r.idx, r.events.len())
    } else if r.finished() {
        "Replay finished — click or ESC for menu".to_string()
    } else {
        format!("REPLAY  {}/{}   ESC = exit", r.idx, r.events.len())
    };
    let w = ui.width(&txt, 19.0) + 44.0;
    draw_round_frame((WIN_W - w) * 0.5, 16.0, w, 44.0, 14.0, 2.5, SRF(), BORDER());
    ui.text_centered(&txt, WIN_W * 0.5, 45.0, 19.0, INK());
}

// ---------------------------------------------------------------- menu screen

enum MenuAction {
    None,
    Start,
    Host,
    Join,
}

struct Menu {
    players: usize,
    humans: usize,
    difficulty: Difficulty,
    seed: u64,
    preview: Game,
    editing: bool,
    seed_text: String,
    bookmarks: Vec<(u64, usize)>, // (seed, players)
    hold_t: f32,    // how long < or > has been held
    hold_next: f32, // countdown to the next auto-repeat step
    bm_scroll: f32, // bookmark list scroll offset, in rows
    host_err: f32,  // countdown showing the hosting-failed message
}

fn load_bookmarks() -> Vec<(u64, usize)> {
    std::fs::read_to_string(data_file(BOOKMARK_FILE))
        .ok()
        .map(|s| {
            s.lines()
                .filter_map(|l| {
                    let mut it = l.split_whitespace();
                    let seed: u64 = it.next()?.parse().ok()?;
                    let players: usize = it.next()?.parse().ok()?;
                    Some((seed % SEED_MOD, players.clamp(2, MAX_PLAYERS)))
                })
                .collect()
        })
        .unwrap_or_default()
}

fn save_bookmarks(b: &[(u64, usize)]) {
    let s: String = b.iter().map(|(s, p)| format!("{} {}\n", s, p)).collect();
    let _ = std::fs::write(data_file(BOOKMARK_FILE), s);
}

// ---------------------------------------------------------------- settings

const SETTINGS_FILE: &str = "settings.txt";

struct Settings {
    chances: bool, // show win-chance hints on attack options
    muted: bool,
    colorblind: bool, // alternative palette + shape badges on the map
    color: usize,     // which palette color the human plays as
    speed: f32,       // game speed multiplier (1x / 2x)
    dark: bool,       // dark mode
}

fn load_settings() -> Settings {
    let mut s = Settings {
        chances: true,
        muted: false,
        colorblind: false,
        color: 0,
        speed: 1.0,
        dark: false,
    };
    if let Ok(txt) = std::fs::read_to_string(data_file(SETTINGS_FILE)) {
        for l in txt.lines() {
            let mut it = l.split_whitespace();
            match (it.next(), it.next()) {
                (Some("chances"), Some(v)) => s.chances = v != "0",
                (Some("muted"), Some(v)) => s.muted = v != "0",
                (Some("colorblind"), Some(v)) => s.colorblind = v != "0",
                (Some("color"), Some(v)) => {
                    s.color = v.parse::<usize>().unwrap_or(0) % MAX_PLAYERS
                }
                (Some("speed"), Some(v)) => {
                    s.speed = if v == "2" { 2.0 } else { 1.0 }
                }
                (Some("dark"), Some(v)) => s.dark = v != "0",
                _ => {}
            }
        }
    }
    s
}

fn save_settings(s: &Settings) {
    let _ = std::fs::write(
        data_file(SETTINGS_FILE),
        format!(
            "chances {}\nmuted {}\ncolorblind {}\ncolor {}\nspeed {}\ndark {}\n",
            s.chances as u8, s.muted as u8, s.colorblind as u8, s.color, s.speed as u8, s.dark as u8
        ),
    );
}

impl Menu {
    fn init() -> Self {
        let players = 6;
        let humans = 1;
        let difficulty = Difficulty::Normal;
        let seed = random_seed();
        Menu {
            players,
            humans,
            difficulty,
            seed,
            preview: gen_map(seed, players, humans, difficulty),
            editing: false,
            seed_text: String::new(),
            bookmarks: load_bookmarks(),
            hold_t: 0.0,
            hold_next: 0.0,
            bm_scroll: 0.0,
            host_err: 0.0,
        }
    }

    fn regen(&mut self) {
        self.preview = gen_map(self.seed, self.players, self.humans, self.difficulty);
    }

    fn commit_seed(&mut self) {
        self.editing = false;
        if let Ok(s) = self.seed_text.parse::<u64>() {
            self.seed = s % SEED_MOD;
        }
        self.regen();
    }

    fn is_bookmarked(&self) -> bool {
        self.bookmarks.contains(&(self.seed, self.players))
    }
}

// menu layout: a centered main column plus a bookmarks column on the right
const COL_L: f32 = 300.0;
const COL_R: f32 = 728.0;

fn r_color(i: usize) -> Rect {
    Rect::new(WIN_W * 0.5 - 125.0 + i as f32 * 32.0, 100.0, 26.0, 14.0)
}
fn r_player(k: usize) -> Rect {
    // 7 buttons for 2..=8 players, centered on the main column
    let total = 7.0 * 58.0 + 6.0 * 12.0;
    Rect::new(COL_L - total * 0.5 + k as f32 * 70.0, 244.0, 58.0, 48.0)
}
fn r_diff(k: usize) -> Rect {
    Rect::new(COL_L - 196.0 + k as f32 * 136.0, 148.0, 120.0, 44.0)
}
fn r_preview() -> Rect {
    Rect::new(COL_L - 230.0, 304.0, 460.0, 240.0)
}
fn r_prev() -> Rect {
    Rect::new(COL_L - 200.0, 566.0, 48.0, 48.0)
}
fn r_seedbox() -> Rect {
    Rect::new(COL_L - 144.0, 566.0, 200.0, 48.0)
}
fn r_next() -> Rect {
    Rect::new(COL_L + 64.0, 566.0, 48.0, 48.0)
}
fn r_new() -> Rect {
    Rect::new(COL_L + 120.0, 566.0, 80.0, 48.0)
}
fn r_bookmark() -> Rect {
    Rect::new(COL_L + 230.0 - 54.0, 312.0, 42.0, 42.0)
}
fn r_start() -> Rect {
    Rect::new(COL_L - 234.0, 664.0, 300.0, 72.0)
}
fn r_host() -> Rect {
    Rect::new(COL_L + 78.0, 664.0, 156.0, 33.0)
}
fn r_join() -> Rect {
    Rect::new(COL_L + 78.0, 703.0, 156.0, 33.0)
}
fn r_bm_row(i: usize) -> Rect {
    Rect::new(COL_R - 160.0, 320.0 + i as f32 * 54.0, 320.0, 44.0)
}
fn r_bm_del(i: usize) -> Rect {
    let r = r_bm_row(i);
    Rect::new(r.x + r.w - 40.0, r.y + 6.0, 32.0, 32.0)
}
const BM_SHOWN: usize = 4;

// returns true when the game should start
fn update_menu(menu: &mut Menu, settings: &mut Settings, snd: &mut Vec<Snd>, dt: f32) -> MenuAction {
    menu.host_err = (menu.host_err - dt).max(0.0);
    // holding < or > cycles through seeds after a short delay
    let held_dir: i64 = if is_mouse_button_down(MouseButton::Left) && !menu.editing {
        let m = mouse_virtual();
        if r_prev().contains(m) {
            -1
        } else if r_next().contains(m) {
            1
        } else {
            0
        }
    } else {
        0
    };
    if held_dir != 0 {
        menu.hold_t += dt;
        if menu.hold_t > 0.4 {
            menu.hold_next -= dt;
            if menu.hold_next <= 0.0 {
                menu.hold_next = 0.08;
                menu.seed = (menu.seed as i64 + held_dir).rem_euclid(SEED_MOD as i64) as u64;
                menu.regen();
            }
        }
    } else {
        menu.hold_t = 0.0;
        menu.hold_next = 0.0;
    }

    if menu.editing {
        while let Some(c) = get_char_pressed() {
            if c.is_ascii_digit() && menu.seed_text.len() < 8 {
                menu.seed_text.push(c);
            }
        }
        if is_key_pressed(KeyCode::Backspace) {
            menu.seed_text.pop();
        }
        if is_key_pressed(KeyCode::Enter) {
            menu.commit_seed();
        }
    } else {
        while get_char_pressed().is_some() {} // drain stale chars
        if is_key_pressed(KeyCode::Enter) {
            return MenuAction::Start;
        }
        if is_key_pressed(KeyCode::N) {
            menu.seed = random_seed();
            menu.regen();
            snd.push(Snd::Click);
        }
    }
    // mouse wheel scrolls the bookmark list
    let (_, wheel) = mouse_wheel();
    if wheel != 0.0 {
        let mw = mouse_virtual();
        if mw.x > COL_R - 175.0 && mw.y > 130.0 && mw.y < 545.0 {
            let max_off = menu.bookmarks.len().saturating_sub(BM_SHOWN) as f32;
            menu.bm_scroll = (menu.bm_scroll - wheel.signum()).clamp(0.0, max_off);
        }
    }
    if !is_mouse_button_pressed(MouseButton::Left) {
        return MenuAction::None;
    }
    let m = mouse_virtual();
    if menu.editing && !r_seedbox().contains(m) {
        menu.commit_seed();
    }
    for i in 0..MAX_PLAYERS {
        if r_color(i).contains(m) {
            settings.color = i;
            HUMAN_COLOR.store(i, Ordering::Relaxed);
            save_settings(settings);
            snd.push(Snd::Click);
            return MenuAction::None;
        }
    }
    for k in 0..MAX_PLAYERS - 1 {
        if r_player(k).contains(m) {
            menu.players = k + 2;
            menu.regen();
            snd.push(Snd::Click);
            return MenuAction::None;
        }
    }
    for (k, d) in [Difficulty::Easy, Difficulty::Normal, Difficulty::Hard]
        .into_iter()
        .enumerate()
    {
        if r_diff(k).contains(m) {
            menu.difficulty = d;
            menu.regen();
            snd.push(Snd::Click);
            return MenuAction::None;
        }
    }
    if r_seedbox().contains(m) {
        if !menu.editing {
            menu.editing = true;
            menu.seed_text = menu.seed.to_string();
            snd.push(Snd::Click);
        }
        return MenuAction::None;
    }
    if r_prev().contains(m) {
        menu.seed = (menu.seed + SEED_MOD - 1) % SEED_MOD;
        menu.regen();
        snd.push(Snd::Click);
        return MenuAction::None;
    }
    if r_next().contains(m) {
        menu.seed = (menu.seed + 1) % SEED_MOD;
        menu.regen();
        snd.push(Snd::Click);
        return MenuAction::None;
    }
    if r_new().contains(m) {
        menu.seed = random_seed();
        menu.regen();
        snd.push(Snd::Click);
        return MenuAction::None;
    }
    if r_bookmark().contains(m) {
        if menu.is_bookmarked() {
            menu.bookmarks.retain(|&b| b != (menu.seed, menu.players));
        } else {
            menu.bookmarks.push((menu.seed, menu.players));
        }
        save_bookmarks(&menu.bookmarks);
        snd.push(Snd::Click);
        return MenuAction::None;
    }
    for i in 0..5 {
        if r_icon(i).contains(m) {
            match i {
                0 => settings.muted = !settings.muted,
                1 => settings.chances = !settings.chances,
                2 => {
                    settings.colorblind = !settings.colorblind;
                    COLORBLIND.store(settings.colorblind, Ordering::Relaxed);
                }
                3 => settings.speed = if settings.speed > 1.5 { 1.0 } else { 2.0 },
                _ => {
                    settings.dark = !settings.dark;
                    DARK.store(settings.dark, Ordering::Relaxed);
                }
            }
            save_settings(settings);
            snd.push(Snd::Click);
            return MenuAction::None;
        }
    }
    let bm_off = menu.bm_scroll as usize;
    for i in 0..menu.bookmarks.len().saturating_sub(bm_off).min(BM_SHOWN) {
        if r_bm_del(i).contains(m) {
            menu.bookmarks.remove(i + bm_off);
            let max_off = menu.bookmarks.len().saturating_sub(BM_SHOWN) as f32;
            menu.bm_scroll = menu.bm_scroll.min(max_off);
            save_bookmarks(&menu.bookmarks);
            snd.push(Snd::Click);
            return MenuAction::None;
        }
        if r_bm_row(i).contains(m) {
            let (s, p) = menu.bookmarks[i + bm_off];
            menu.seed = s;
            menu.players = p;
            menu.regen();
            snd.push(Snd::Select);
            return MenuAction::None;
        }
    }
    if r_start().contains(m) {
        return MenuAction::Start;
    }
    if r_host().contains(m) {
        snd.push(Snd::Click);
        return MenuAction::Host;
    }
    if r_join().contains(m) {
        snd.push(Snd::Click);
        return MenuAction::Join;
    }
    MenuAction::None
}

// small map rendering for the menu preview
fn draw_map_mini(game: &Game, area: Rect) {
    // virtual-canvas map bounds, padded above for dice towers and below
    // for their shadows so nothing pokes out of the preview frame
    let (bx, by, bw, bh) = (30.0, 9.0, 845.0, 615.0);
    let k = (area.w / bw).min(area.h / bh);
    let ox = area.x + (area.w - bw * k) * 0.5 - bx * k;
    let oy = area.y + (area.h - bh * k) * 0.5 - by * k;
    let tp = |p: Vec2| vec2(ox + p.x * k, oy + p.y * k);

    // water dots, like the real board
    for i in 0..COLS * ROWS {
        if game.cell_terr[i] < 0 {
            let p = tp(cell_pos(i));
            draw_circle(p.x, p.y, (2.2 * k).max(0.8), SEA());
        }
    }
    for i in 0..COLS * ROWS {
        let t = game.cell_terr[i];
        if t < 0 {
            continue;
        }
        fill_hex(tp(cell_pos(i)), HEX * k + 0.3, palette(game.terrs[t as usize].owner).fill);
    }
    for i in 0..COLS * ROWS {
        let t = game.cell_terr[i];
        if t < 0 {
            continue;
        }
        let (c, r) = (i % COLS, i / COLS);
        let pos = cell_pos(i);
        for d in 0..6 {
            let same = neighbor_of(c, r, d).map_or(false, |n| game.cell_terr[n] == t);
            if !same {
                let a0 = (d as f32 * 60.0 - 30.0).to_radians();
                let a1 = (d as f32 * 60.0 + 30.0).to_radians();
                let p0 = tp(pos + HEX * vec2(a0.cos(), a0.sin()));
                let p1 = tp(pos + HEX * vec2(a1.cos(), a1.sin()));
                draw_line(p0.x, p0.y, p1.x, p1.y, 1.3, BORDER());
            }
        }
    }
    // the real dice stacks in miniature, back to front, at exact map scale
    let mut order: Vec<usize> = (0..game.terrs.len()).collect();
    order.sort_by(|&a, &b| game.terrs[a].anchor.y.total_cmp(&game.terrs[b].anchor.y));
    let w = DIE_W * k;
    let ch = w * 0.62;
    for ti in order {
        let t = &game.terrs[ti];
        let pal = palette(t.owner);
        let base = tp(t.anchor) + vec2(0.0, 2.0);
        let n = t.dice as usize;
        let (fdx, bdx) = if n > 4 { (-w * 0.46, w * 0.46) } else { (0.0, 0.0) };
        draw_ellipse_fan(
            base.x,
            base.y - w * 0.21,
            if n > 4 { w * 1.04 } else { w * 0.62 },
            w * 0.34,
            Color::new(0.15, 0.16, 0.30, 0.26),
        );
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
            draw_die(base.x + fdx, base.y - i as f32 * ch, w, t.seed, i as u32, &pal);
        }
    }
}

// a dice-burst hero fills the right column, a nod to the 2001 original
// a spiky explosion: alternating outer/inner spokes with jittered lengths
fn draw_burst(cx: f32, cy: f32, r_out: f32, r_in: f32, spikes: usize, rot: f32, col: Color) {
    let tau = std::f32::consts::TAU;
    for i in 0..spikes {
        let a0 = rot + i as f32 / spikes as f32 * tau;
        let a1 = rot + (i as f32 + 0.5) / spikes as f32 * tau;
        let a2 = rot + (i as f32 + 1.0) / spikes as f32 * tau;
        let ro = r_out * (0.82 + 0.36 * (((i * 7919) % 13) as f32 / 13.0));
        draw_triangle(
            vec2(cx + r_in * a0.cos(), cy + r_in * a0.sin()),
            vec2(cx + ro * a1.cos(), cy + ro * a1.sin()),
            vec2(cx + r_in * a2.cos(), cy + r_in * a2.sin()),
            col,
        );
    }
    draw_poly(cx, cy, spikes as u8, r_in, rot.to_degrees(), col);
}

fn draw_menu_hero() {
    let cx = COL_R;
    let cy = 212.0;
    draw_burst(cx, cy, 118.0, 54.0, 12, 0.1, mix(base_color(0), WHITE, 0.12));
    draw_burst(cx, cy, 82.0, 40.0, 12, 0.36, mix(base_color(0), WHITE, 0.62));
    let sh = Color::new(0.15, 0.16, 0.30, 0.16);
    draw_ellipse_fan(cx + 8.0, cy - 12.0, 24.0, 7.0, sh);
    draw_ellipse_fan(cx - 38.0, cy + 58.0, 32.0, 9.0, sh);
    draw_ellipse_fan(cx + 46.0, cy + 62.0, 29.0, 8.5, sh);
    draw_die(cx + 4.0, cy - 16.0, 42.0, 5, 0, &palette(3));
    draw_die(cx - 46.0, cy + 54.0, 58.0, 11, 0, &palette(2));
    draw_die(cx + 42.0, cy + 58.0, 52.0, 23, 0, &palette(4));
}

fn draw_star(cx: f32, cy: f32, r: f32, col: Color) {
    let tau = std::f32::consts::TAU;
    let ri = r * 0.47;
    draw_poly(cx, cy, 5, ri, -126.0, col);
    for i in 0..5 {
        let ao = -0.25 * tau + i as f32 / 5.0 * tau;
        let ai0 = ao - tau / 10.0;
        let ai1 = ao + tau / 10.0;
        draw_triangle(
            vec2(cx + ri * ai0.cos(), cy + ri * ai0.sin()),
            vec2(cx + r * ao.cos(), cy + r * ao.sin()),
            vec2(cx + ri * ai1.cos(), cy + ri * ai1.sin()),
            col,
        );
    }
}

fn menu_button(r: Rect, fill: Color, edge: Color, hovered: bool) {
    let f = if hovered { mix(fill, WHITE, 0.35) } else { fill };
    draw_round_rect(r.x + 2.0, r.y + 4.0, r.w, r.h, 14.0, PANEL_SHADOW());
    draw_round_frame(r.x, r.y, r.w, r.h, 14.0, 2.5, f, edge);
}

fn draw_menu(menu: &Menu, settings: &Settings, ui: &Ui, time: f32) {
    let m = mouse_virtual();

    draw_rectangle(0.0, 0.0, WIN_W, WIN_H, SRF());

    // chunky extruded logo
    let title = "DICE WARS";
    let tw = ui.width(title, 58.0);
    let tx = WIN_W * 0.5 - tw * 0.5;
    // many faint offset copies blend into one soft drop shadow
    let sh = with_alpha(th(INK_ON_FILL, BLACK), if dark_mode() { 0.12 } else { 0.055 });
    for (dx, dy) in [
        (2.5, 3.5),
        (4.0, 4.5),
        (5.5, 5.5),
        (3.0, 5.5),
        (5.0, 3.5),
        (4.0, 6.5),
        (2.5, 5.0),
        (5.5, 4.5),
    ] {
        ui.text(title, tx + dx, 86.0 + dy, 58.0, sh);
    }
    ui.text_outlined(
        title,
        tx,
        86.0,
        58.0,
        mix(PLAYER_BASE[0], WHITE, 0.2),
        INK_ON_FILL,
    );
    draw_menu_hero();
    // the accent band doubles as the pick-your-color control
    for i in 0..MAX_PLAYERS {
        let r = r_color(i);
        let base = if colorblind_mode() {
            PLAYER_BASE_CB[i]
        } else {
            PLAYER_BASE[i]
        };
        let sel = HUMAN_COLOR.load(Ordering::Relaxed) == i;
        if sel {
            draw_round_frame(r.x - 3.0, r.y - 3.0, r.w + 6.0, r.h + 6.0, 8.0, 2.0, SRF(), BORDER());
        }
        let hov = r.contains(m);
        draw_round_rect(
            r.x,
            r.y,
            r.w,
            r.h,
            6.0,
            if hov { lighten(base, 0.25) } else { base },
        );
    }

    // players
    ui.text_centered("PLAYERS", COL_L, 232.0, 13.0, with_alpha(INK(), 0.55));
    for k in 0..MAX_PLAYERS - 1 {
        let r = r_player(k);
        let n = k + 2;
        let sel = menu.players == n;
        let fill = if sel { palette(HUMAN).fill } else { SRF() };
        let edge = if sel { BORDER() } else { CARD_EDGE() };
        menu_button(r, fill, edge, r.contains(m));
        ui.text_centered(
            &n.to_string(),
            r.x + r.w * 0.5,
            r.y + 33.0,
            25.0,
            if sel { INK_ON_FILL } else { INK() },
        );
    }

    // difficulty
    ui.text_centered("DIFFICULTY", COL_L, 136.0, 13.0, with_alpha(INK(), 0.55));
    for (k, (label, d, tone)) in [
        ("EASY", Difficulty::Easy, 3usize),
        ("NORMAL", Difficulty::Normal, 0usize),
        ("HARD", Difficulty::Hard, 2usize),
    ]
    .into_iter()
    .enumerate()
    {
        let r = r_diff(k);
        let sel = menu.difficulty == d;
        let fill = if sel {
            mix(base_color(tone), WHITE, 0.4)
        } else {
            SRF()
        };
        let edge = if sel { BORDER() } else { CARD_EDGE() };
        menu_button(r, fill, edge, r.contains(m));
        ui.text_centered(
            label,
            r.x + r.w * 0.5,
            r.y + 29.0,
            18.0,
            if sel { INK_ON_FILL } else { INK() },
        );
    }

    // map preview
    let pv = r_preview();
    draw_round_frame(pv.x, pv.y, pv.w, pv.h, 14.0, 2.0, SRF(), CARD_EDGE());
    draw_map_mini(&menu.preview, Rect::new(pv.x + 8.0, pv.y + 8.0, pv.w - 16.0, pv.h - 16.0));

    // seed row
    let (rp, rs, rn, rr) = (r_prev(), r_seedbox(), r_next(), r_new());
    menu_button(rp, SRF(), CARD_EDGE(), rp.contains(m));
    draw_poly(rp.x + rp.w * 0.5 - 1.5, rp.y + rp.h * 0.5, 3, 8.5, 180.0, INK());
    let seed_edge = if menu.editing { BORDER() } else { CARD_EDGE() };
    menu_button(rs, SRF(), seed_edge, rs.contains(m));
    let seed_str = if menu.editing {
        let caret = if (time * 2.5) as i32 % 2 == 0 { "|" } else { " " };
        format!("{}{}", menu.seed_text, caret)
    } else {
        format!("{:08}", menu.seed)
    };
    ui.text_centered(&seed_str, rs.x + rs.w * 0.5, rs.y + 32.0, 22.0, INK());
    menu_button(rn, SRF(), CARD_EDGE(), rn.contains(m));
    draw_poly(rn.x + rn.w * 0.5 + 1.5, rn.y + rn.h * 0.5, 3, 8.5, 0.0, INK());
    menu_button(rr, SRF(), CARD_EDGE(), rr.contains(m));
    ui.text_centered("NEW", rr.x + rr.w * 0.5, rr.y + 31.0, 18.0, INK());

    // bookmark star, frameless, tucked into the preview's corner
    let rb = r_bookmark();
    let marked = menu.is_bookmarked();
    let hovs = rb.contains(m);
    let scol = if marked {
        palette(HUMAN).mid
    } else if hovs {
        with_alpha(INK(), 0.6)
    } else {
        with_alpha(INK(), 0.28)
    };
    draw_star(rb.x + rb.w * 0.5, rb.y + rb.h * 0.5, if hovs { 15.0 } else { 13.0 }, scol);

    // the same icon toolbar as in-game (sound, chances, colorblind, speed)
    draw_icon_bar(settings, ui, m);

    if menu.host_err > 0.0 {
        ui.text_centered(
            "Could not open port 7777 — is another instance hosting?",
            COL_L,
            790.0,
            15.0,
            palette(2).dark,
        );
    }

    // start button
    let st = r_start();
    menu_button(st, palette(HUMAN).mid, BORDER(), st.contains(m));
    ui.text_centered("START GAME", st.x + st.w * 0.5, st.y + 45.0, 30.0, WHITE);
    let rh = r_host();
    menu_button(rh, SRF(), CARD_EDGE(), rh.contains(m));
    ui.text_centered("HOST GAME", rh.x + rh.w * 0.5, rh.y + 22.0, 14.0, INK());
    let rj = r_join();
    menu_button(rj, SRF(), CARD_EDGE(), rj.contains(m));
    ui.text_centered("JOIN GAME", rj.x + rj.w * 0.5, rj.y + 22.0, 14.0, INK());

    // bookmarks list (label-free: the rows speak for themselves)
    let bm_off = menu.bm_scroll as usize;
    for (i, &(seed, players)) in menu.bookmarks.iter().skip(bm_off).take(BM_SHOWN).enumerate() {
        let r = r_bm_row(i);
        let active = seed == menu.seed && players == menu.players;
        let fill = if active { palette(HUMAN).fill } else { SRF() };
        menu_button(r, fill, if active { BORDER() } else { CARD_EDGE() }, r.contains(m));
        ui.text(
            &format!("#{:08}", seed),
            r.x + 18.0,
            r.y + 29.0,
            19.0,
            if active { INK_ON_FILL } else { INK() },
        );
        ui.text(
            &format!("{}P", players),
            r.x + r.w - 82.0,
            r.y + 29.0,
            16.0,
            with_alpha(INK(), 0.55),
        );
        let del = r_bm_del(i);
        let dh = del.contains(m);
        ui.text_centered(
            "x",
            del.x + del.w * 0.5,
            del.y + 23.0,
            20.0,
            if dh { INK() } else { with_alpha(INK(), 0.35) },
        );
    }
    if menu.bookmarks.len() > BM_SHOWN {
        let total = menu.bookmarks.len() as f32;
        let track_y = 320.0;
        let track_h = BM_SHOWN as f32 * 54.0 - 10.0;
        draw_round_rect(COL_R + 166.0, track_y, 5.0, track_h, 2.5, CARD_EDGE());
        let th = (track_h * BM_SHOWN as f32 / total).max(24.0);
        let ty = track_y + (track_h - th) * (bm_off as f32 / (total - BM_SHOWN as f32));
        draw_round_rect(COL_R + 166.0, ty, 5.0, th, 2.5, with_alpha(INK(), 0.35));
    }

    // a quick rules card below the bookmark list
    {
        let hx = COL_R - 160.0;
        let hy = 560.0;
        draw_round_frame(hx, hy, 320.0, 218.0, 14.0, 2.0, SRF(), CARD_EDGE());
        ui.text_centered("HOW TO PLAY", COL_R, hy + 28.0, 13.0, with_alpha(INK(), 0.55));
        for (i, line) in [
            "1. Click one of your lands (2+ dice),",
            "    then a neighbor land to attack.",
            "2. Higher dice total wins the land.",
            "    Ties go to the defender.",
            "3. END TURN gives +1 die per land",
            "    in your biggest connected area.",
            "4. Conquer the whole map!",
        ]
        .iter()
        .enumerate()
        {
            ui.text(line, hx + 18.0, hy + 56.0 + i as f32 * 23.0, 13.5, with_alpha(INK(), 0.8));
        }
    }

}

// ---------------------------------------------------------------- lobby screens

#[derive(Default)]
struct JoinUi {
    addr: String,
    code: String,
    focus: usize, // 0 = address, 1 = code
    status: String,
}

fn r_lobby_prev() -> Rect {
    Rect::new(COL_L - 200.0, 480.0, 48.0, 48.0)
}
fn r_lobby_seed() -> Rect {
    Rect::new(COL_L - 144.0, 480.0, 200.0, 48.0)
}
fn r_lobby_next() -> Rect {
    Rect::new(COL_L + 64.0, 480.0, 48.0, 48.0)
}
fn r_lobby_new() -> Rect {
    Rect::new(COL_L + 120.0, 480.0, 80.0, 48.0)
}
fn r_lobby_copy() -> Rect {
    Rect::new(COL_R + 100.0, 250.0, 76.0, 28.0)
}
fn r_lobby_start() -> Rect {
    Rect::new(WIN_W * 0.5 - 160.0, 620.0, 320.0, 64.0)
}
fn r_lobby_cancel() -> Rect {
    Rect::new(WIN_W * 0.5 - 70.0, 700.0, 140.0, 40.0)
}
fn r_join_addr() -> Rect {
    Rect::new(WIN_W * 0.5 - 170.0, 330.0, 340.0, 48.0)
}
fn r_join_code() -> Rect {
    Rect::new(WIN_W * 0.5 - 170.0, 424.0, 340.0, 48.0)
}
fn r_join_paste() -> Rect {
    Rect::new(WIN_W * 0.5 + 178.0, 330.0, 76.0, 48.0)
}
fn r_join_go() -> Rect {
    Rect::new(WIN_W * 0.5 - 170.0, 500.0, 160.0, 48.0)
}
fn r_join_back() -> Rect {
    Rect::new(WIN_W * 0.5 + 10.0, 500.0, 160.0, 48.0)
}

fn draw_lobby_card(ui: &Ui, title: &str) {
    draw_rectangle(0.0, 0.0, WIN_W, WIN_H, SRF());
    ui.text_centered(title, WIN_W * 0.5, 160.0, 44.0, INK());
}

fn draw_host_lobby(h: &HostNet, menu: &Menu, ui: &Ui, time: f32, copied_t: f32) {
    draw_rectangle(0.0, 0.0, WIN_W, WIN_H, SRF());
    let m = mouse_virtual();
    ui.text_centered("HOSTING", WIN_W * 0.5, 96.0, 40.0, INK());

    // left: pick the map while people join
    ui.text_centered("MAP", COL_L, 196.0, 13.0, with_alpha(INK(), 0.55));
    let pv = Rect::new(COL_L - 230.0, 208.0, 460.0, 250.0);
    draw_round_frame(pv.x, pv.y, pv.w, pv.h, 14.0, 2.0, SRF(), CARD_EDGE());
    draw_map_mini(&menu.preview, Rect::new(pv.x + 8.0, pv.y + 8.0, pv.w - 16.0, pv.h - 16.0));
    let (rp, rs, rn, rr) = (r_lobby_prev(), r_lobby_seed(), r_lobby_next(), r_lobby_new());
    menu_button(rp, SRF(), CARD_EDGE(), rp.contains(m));
    draw_poly(rp.x + rp.w * 0.5 - 1.5, rp.y + rp.h * 0.5, 3, 8.5, 180.0, INK());
    menu_button(rs, SRF(), CARD_EDGE(), false);
    ui.text_centered(&format!("{:08}", menu.seed), rs.x + rs.w * 0.5, rs.y + 32.0, 22.0, INK());
    menu_button(rn, SRF(), CARD_EDGE(), rn.contains(m));
    draw_poly(rn.x + rn.w * 0.5 + 1.5, rn.y + rn.h * 0.5, 3, 8.5, 0.0, INK());
    menu_button(rr, SRF(), CARD_EDGE(), rr.contains(m));
    ui.text_centered("NEW", rr.x + rr.w * 0.5, rr.y + 31.0, 18.0, INK());

    // right: room info and who plays which color
    ui.text_centered("ROOM CODE", COL_R, 152.0, 13.0, with_alpha(INK(), 0.55));
    ui.text_centered(&h.code, COL_R, 214.0, 58.0, INK());
    ui.text(
        &format!("{}:{}", local_ip(), net_port()),
        COL_R - 160.0,
        270.0,
        17.0,
        with_alpha(INK(), 0.7),
    );
    let rc2 = r_lobby_copy();
    menu_button(rc2, SRF(), CARD_EDGE(), rc2.contains(m));
    ui.text_centered(
        if copied_t > 0.0 { "COPIED!" } else { "COPY" },
        rc2.x + rc2.w * 0.5,
        rc2.y + 19.0,
        12.0,
        INK(),
    );

    ui.text_centered("PLAYERS", COL_R, 316.0, 13.0, with_alpha(INK(), 0.55));
    let slots = h.slots();
    let joined: usize = slots.iter().filter(|&&s| s).count();
    let mut seat = 0usize;
    let mut rows: Vec<String> = vec!["P1  (you)".to_string()];
    for (i, &occ) in slots.iter().enumerate() {
        let _ = i;
        if occ {
            seat += 1;
            rows.push(format!("P{}  (connected)", seat + 1));
        }
    }
    while rows.len() < menu.players {
        rows.push("AI".to_string());
    }
    // center the roster block under its heading
    let roster_w = 150.0;
    let rx0 = COL_R - roster_w * 0.5;
    for (k, label) in rows.iter().enumerate() {
        let ry = 346.0 + k as f32 * 33.0;
        let pal = palette(k);
        draw_round_rect(rx0, ry - 14.0, 24.0, 18.0, 6.0, pal.mid);
        draw_symbol(rx0 + 40.0, ry - 5.0, 7.0, k, pal.dark);
        ui.text(label, rx0 + 58.0, ry, 17.0, INK());
    }
    let dots = ".".repeat(1 + (time * 2.0) as usize % 3);
    ui.text_centered(
        &format!(
            "{} player{} here — waiting for more{}",
            joined + 1,
            if joined == 0 { "" } else { "s" },
            dots
        ),
        WIN_W * 0.5,
        584.0,
        17.0,
        with_alpha(INK(), 0.6),
    );

    let rst = r_lobby_start();
    menu_button(rst, palette(HUMAN).mid, BORDER(), rst.contains(m));
    ui.text_centered("START GAME", rst.x + rst.w * 0.5, rst.y + 41.0, 26.0, WHITE);
    let rc = r_lobby_cancel();
    menu_button(rc, SRF(), CARD_EDGE(), rc.contains(m));
    ui.text_centered("CANCEL", rc.x + rc.w * 0.5, rc.y + 27.0, 16.0, INK());
}

fn draw_join_lobby(j: &JoinUi, connected: bool, ui: &Ui, time: f32) {
    draw_lobby_card(ui, "JOIN GAME");
    let m = mouse_virtual();
    ui.text_centered("HOST ADDRESS", WIN_W * 0.5, 316.0, 14.0, with_alpha(INK(), 0.55));
    let ra = r_join_addr();
    menu_button(ra, SRF(), if j.focus == 0 && !connected { BORDER() } else { CARD_EDGE() }, ra.contains(m));
    let caret = |on: bool| if on && (time * 2.5) as i32 % 2 == 0 { "|" } else { "" };
    ui.text_centered(
        &format!("{}{}", j.addr, caret(j.focus == 0 && !connected)),
        ra.x + ra.w * 0.5,
        ra.y + 31.0,
        20.0,
        INK(),
    );
    if !connected {
        let rpst = r_join_paste();
        menu_button(rpst, SRF(), CARD_EDGE(), rpst.contains(m));
        ui.text_centered("PASTE", rpst.x + rpst.w * 0.5, rpst.y + 30.0, 14.0, INK());
    }
    ui.text_centered("ROOM CODE", WIN_W * 0.5, 410.0, 14.0, with_alpha(INK(), 0.55));
    let rc = r_join_code();
    menu_button(rc, SRF(), if j.focus == 1 && !connected { BORDER() } else { CARD_EDGE() }, rc.contains(m));
    ui.text_centered(
        &format!("{}{}", j.code, caret(j.focus == 1 && !connected)),
        rc.x + rc.w * 0.5,
        rc.y + 31.0,
        22.0,
        INK(),
    );
    if !connected {
        let rg = r_join_go();
        menu_button(rg, palette(HUMAN).fill, BORDER(), rg.contains(m));
        ui.text_centered("CONNECT", rg.x + rg.w * 0.5, rg.y + 31.0, 18.0, INK());
    }
    let rb = r_join_back();
    menu_button(rb, SRF(), CARD_EDGE(), rb.contains(m));
    ui.text_centered("BACK", rb.x + rb.w * 0.5, rb.y + 31.0, 18.0, INK());
    if !j.status.is_empty() {
        ui.text_centered(&j.status, WIN_W * 0.5, 600.0, 18.0, with_alpha(INK(), 0.75));
    }
}

// ---------------------------------------------------------------- main loop

enum Screen {
    Menu,
    Play,
    Replay,
    HostLobby,
    JoinLobby,
}

fn window_icon() -> Option<miniquad::conf::Icon> {
    let img = Image::from_file_with_format(include_bytes!("../assets/icon.png"), Some(ImageFormat::Png)).ok()?;
    // nearest-neighbor downscale of the 256px icon to the sizes miniquad wants
    fn scale<const N: usize>(img: &Image, s: usize) -> [u8; N] {
        let mut out = [0u8; N];
        let iw = img.width as usize;
        for y in 0..s {
            for x in 0..s {
                let sx = x * iw / s;
                let sy = y * img.height as usize / s;
                let i = (sy * iw + sx) * 4;
                let o = (y * s + x) * 4;
                out[o..o + 4].copy_from_slice(&img.bytes[i..i + 4]);
            }
        }
        out
    }
    Some(miniquad::conf::Icon {
        small: scale::<1024>(&img, 16),
        medium: scale::<4096>(&img, 32),
        big: scale::<16384>(&img, 64),
    })
}

fn window_conf() -> Conf {
    Conf {
        window_title: "Dice Wars".to_string(),
        window_width: WIN_W as i32,
        window_height: WIN_H as i32,
        high_dpi: true,
        sample_count: 8, // MSAA
        icon: window_icon(),
        ..Default::default()
    }
}

#[macroquad::main(window_conf)]
async fn main() {
    macroquad::rand::srand(macroquad::miniquad::date::now() as u64);
    let ui = Ui {
        font: load_ttf_font_from_bytes(include_bytes!("../assets/font.ttf"))
            .expect("failed to load embedded font"),
    };
    let sounds = Sounds::load().await;
    let mut settings = load_settings();
    COLORBLIND.store(settings.colorblind, Ordering::Relaxed);
    HUMAN_COLOR.store(settings.color, Ordering::Relaxed);
    DARK.store(settings.dark, Ordering::Relaxed);
    if std::env::var("DW_EXPORT_ICON").is_ok() {
        let rt = render_target(1024, 1024);
        set_camera(&Camera2D {
            target: vec2(128.0, 128.0),
            zoom: vec2(2.0 / 256.0, 2.0 / 256.0),
            render_target: Some(rt.clone()),
            ..Default::default()
        });
        clear_background(Color::new(0.0, 0.0, 0.0, 0.0));
        let base = PLAYER_BASE[0]; // always buttercup, whatever the user picked
        let pal = Palette {
            fill: mix(base, WHITE, 0.46),
            mid: base,
            dark: darken(base, 0.78),
            light: mix(base, WHITE, 0.62),
            pip: Color::new(0.22, 0.23, 0.38, 1.0),
        };
        draw_die(128.0, 232.0, 176.0, 11, 0, &pal);
        set_default_camera();
        let mut img = downsample(&rt.texture.get_texture_data(), 4);
        let (w, h) = (img.width as usize, img.height as usize);
        for y in 0..h / 2 {
            for x in 0..w * 4 {
                img.bytes.swap(y * w * 4 + x, (h - 1 - y) * w * 4 + x);
            }
        }
        img.export_png("assets/icon.png");
        std::process::exit(0);
    }

    let mut menu = Menu::init();
    let mut screen = Screen::Menu;
    let mut game: Option<Game> = None;
    if std::env::var("DW_AUTOSTART").is_ok() {
        game = Some(gen_map(menu.seed, menu.players, 1, menu.difficulty));
        screen = Screen::Play;
    }
    let mut replay: Option<Replay> = None;
    let mut host: Option<HostNet> = None;
    let mut guest: Option<GuestNet> = None;
    let mut join = JoinUi::default();
    let mut menu_time = 0.0f32;
    let mut net_ping_t = 0.0f32;
    let mut awaiting: HashMap<usize, f32> = HashMap::new(); // game seat -> grace timer
    let mut copied_t = 0.0f32;
    let mut confirm: Option<Confirm> = None;
    let mut snd_queue: Vec<Snd> = Vec::new();
    let autoend = std::env::var("DW_TEST_AUTOEND").is_ok();
    let test_host_auto: usize = std::env::var("DW_TEST_HOST")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);

    // automation hooks for testing the network path
    if std::env::var("DW_TEST_HOST").is_ok() {
        match HostNet::start(menu.players - 1) {
            Ok(h) => {
                eprintln!("HOSTCODE {}", h.code);
                host = Some(h);
                screen = Screen::HostLobby;
            }
            Err(e) => eprintln!("HOSTFAIL {}", e),
        }
    }
    if let Ok(spec) = std::env::var("DW_TEST_JOIN") {
        let mut it = spec.split_whitespace();
        let addr = it.next().unwrap_or("").to_string();
        let code = it.next().unwrap_or("").to_string();
        join.addr = addr;
        join.code = code;
        match guest_connect(&join.addr, &join.code) {
            Ok(g) => {
                eprintln!("JOINED seat {}", g.seat);
                guest = Some(g);
                screen = Screen::JoinLobby;
            }
            Err(e) => eprintln!("JOIN FAILED: {}", e),
        }
    }

    loop {
        let dt = get_frame_time().min(0.05);
        if is_key_pressed(KeyCode::M) {
            settings.muted = !settings.muted;
            save_settings(&settings);
        }

        set_default_camera();
        clear_background(BG());
        set_camera(&view_camera());

        match screen {
            Screen::Menu => {
                menu_time += dt;
                match update_menu(&mut menu, &mut settings, &mut snd_queue, dt) {
                    MenuAction::Start => {
                        game = Some(gen_map(menu.seed, menu.players, 1, menu.difficulty));
                        screen = Screen::Play;
                        snd_queue.push(Snd::Click);
                    }
                    MenuAction::Host => match HostNet::start(menu.players - 1) {
                        Ok(h) => {
                            host = Some(h);
                            screen = Screen::HostLobby;
                        }
                        Err(_) => menu.host_err = 4.0,
                    },
                    MenuAction::Join => {
                        join = JoinUi::default();
                        screen = Screen::JoinLobby;
                    }
                    MenuAction::None => {}
                }
                draw_menu(&menu, &settings, &ui, menu_time);
            }
            Screen::Play => {
                let g = game.as_mut().unwrap();
                g.update(dt * settings.speed);
                let mut go_menu = false;
                let click = is_mouse_button_pressed(MouseButton::Left);
                if click && g.over.is_none() && confirm.is_none() {
                    let m = mouse_virtual();
                    if r_icon(0).contains(m) {
                        settings.muted = !settings.muted;
                        save_settings(&settings);
                    } else if r_icon(1).contains(m) {
                        settings.chances = !settings.chances;
                        save_settings(&settings);
                    } else if r_icon(2).contains(m) {
                        settings.colorblind = !settings.colorblind;
                        COLORBLIND.store(settings.colorblind, Ordering::Relaxed);
                        save_settings(&settings);
                    } else if r_icon(3).contains(m) {
                        settings.speed = if settings.speed > 1.5 { 1.0 } else { 2.0 };
                        save_settings(&settings);
                    } else if r_icon(4).contains(m) {
                        settings.dark = !settings.dark;
                        DARK.store(settings.dark, Ordering::Relaxed);
                        save_settings(&settings);
                    }
                }

                if autoend && g.current == 0 && g.battle.is_none() && g.over.is_none() {
                    g.end_turn(); // test hook: the host seat passes instantly
                }
                // ---- heartbeats: keep idle connections alive through NATs
                net_ping_t += dt;
                let do_ping = net_ping_t >= 2.0;
                if do_ping {
                    net_ping_t = 0.0;
                }
                // ---- hosting: apply guest intents, broadcast new events
                if let Some(h) = host.as_mut() {
                    if do_ping {
                        h.broadcast("PING");
                    }
                    while let Ok(msg) = h.rx.try_recv() {
                        match msg {
                            HostMsg::Intent(lobby_seat, line) => {
                                let Some(&seat) = h.remap.get(&lobby_seat) else {
                                    continue;
                                };
                                if g.battle.is_some() || g.over.is_some() || g.current != seat {
                                    continue;
                                }
                                let mut it = line.split_whitespace();
                                match it.next() {
                                    Some("ATTACK") => {
                                        let a: Option<usize> =
                                            it.next().and_then(|v| v.parse().ok());
                                        let d: Option<usize> =
                                            it.next().and_then(|v| v.parse().ok());
                                        if let (Some(a), Some(d)) = (a, d) {
                                            if a < g.terrs.len()
                                                && d < g.terrs.len()
                                                && g.terrs[a].owner == seat
                                                && g.terrs[a].dice > 1
                                                && g.terrs[d].owner != seat
                                                && g.terrs[a].neighbors.contains(&d)
                                            {
                                                g.begin_attack(a, d);
                                            }
                                        }
                                    }
                                    Some("END") => g.end_turn(),
                                    _ => {}
                                }
                            }
                            HostMsg::Left(lobby_seat) => {
                                let Some(&seat) = h.remap.get(&lobby_seat) else {
                                    continue;
                                };
                                if g.over.is_none() {
                                    // hold the seat and give them time to come back
                                    awaiting.insert(seat, 45.0);
                                    g.push_log(format!(
                                        "{} lost connection — waiting for them to reconnect...",
                                        HUMAN_LABELS[seat]
                                    ));
                                }
                            }
                            HostMsg::Rejoined(lobby_seat) => {
                                h.send_resume(lobby_seat, g, menu.seed, menu.difficulty);
                                if let Some(&seat) = h.remap.get(&lobby_seat) {
                                    awaiting.remove(&seat);
                                    g.push_log(format!(
                                        "{} reconnected!",
                                        HUMAN_LABELS[seat]
                                    ));
                                }
                            }
                            HostMsg::Joined(_) => {}
                        }
                    }
                    h.flush_events(g);
                    // grace timers: give up on seats that never came back
                    let mut expired: Vec<usize> = Vec::new();
                    for (seat, t) in awaiting.iter_mut() {
                        *t -= dt;
                        if *t <= 0.0 {
                            expired.push(*seat);
                        }
                    }
                    for seat in expired {
                        awaiting.remove(&seat);
                        if g.over.is_none() {
                            let msg = format!("{} disconnected", HUMAN_LABELS[seat]);
                            g.over = Some(msg.clone());
                            g.banner_t = 0.0;
                            h.broadcast(&format!("OVER {}", msg));
                        }
                    }
                }

                // ---- joined: apply the host's events, queue our own intents
                if let Some(gn) = guest.as_mut() {
                    if do_ping && gn.rec_rx.is_none() {
                        send_net_line(&mut gn.stream, "PING");
                    }
                    // reconnect attempt finished?
                    if let Some(rr) = &gn.rec_rx {
                        if let Ok(result) = rr.try_recv() {
                            match result {
                                Ok((w, rx)) => {
                                    gn.stream = w;
                                    gn.rx = rx;
                                    gn.rec_rx = None;
                                    gn.catching_up = true;
                                    gn.pending.clear();
                                }
                                Err(e) => {
                                    gn.rec_rx = None;
                                    if g.over.is_none() {
                                        g.over = Some(e);
                                        g.banner_t = 0.0;
                                    }
                                }
                            }
                        }
                    }
                    while let Ok(l) = gn.rx.try_recv() {
                        let mut it = l.split_whitespace();
                        match it.next() {
                            Some("ATK") => {
                                let v: Vec<u32> =
                                    it.filter_map(|x| x.parse().ok()).collect();
                                if v.len() == 5 {
                                    gn.pending.push_back(RepEvent::Attack {
                                        a: v[0] as usize,
                                        d: v[1] as usize,
                                        ra: v[2],
                                        rd: v[3],
                                        captured: v[4] == 1,
                                    });
                                }
                            }
                            Some("REINF") => {
                                let player: usize =
                                    it.next().and_then(|v| v.parse().ok()).unwrap_or(0);
                                let lands: Vec<usize> = it
                                    .next()
                                    .map(|csv| {
                                        csv.split(',')
                                            .filter_map(|x| x.parse().ok())
                                            .collect()
                                    })
                                    .unwrap_or_default();
                                gn.pending.push_back(RepEvent::Reinforce { player, lands });
                            }
                            Some("PING") => {}
                            Some("RESUME") => {
                                // rebuild the match and fast-forward the backlog
                                let seed: u64 =
                                    it.next().and_then(|v| v.parse().ok()).unwrap_or(0);
                                let players: usize =
                                    it.next().and_then(|v| v.parse().ok()).unwrap_or(2);
                                let humans: usize =
                                    it.next().and_then(|v| v.parse().ok()).unwrap_or(2);
                                let di: usize =
                                    it.next().and_then(|v| v.parse().ok()).unwrap_or(1);
                                let seat: usize =
                                    it.next().and_then(|v| v.parse().ok()).unwrap_or(gn.seat);
                                if let Some(c) =
                                    it.next().and_then(|v| v.parse::<usize>().ok())
                                {
                                    HUMAN_COLOR.store(c % MAX_PLAYERS, Ordering::Relaxed);
                                }
                                gn.seat = seat;
                                let mut ng =
                                    gen_map(seed, players.clamp(2, 8), humans, idx_diff(di));
                                ng.net_guest = true;
                                ng.my_seat = seat;
                                *g = ng;
                                gn.catching_up = true;
                            }
                            Some("SYNCED") => {
                                gn.catching_up = false;
                                g.push_log("Reconnected!".to_string());
                            }
                            Some("LOST") => {
                                // connection dropped: hold the game and retry quietly
                                if g.over.is_none() && gn.rec_rx.is_none() {
                                    gn.rec_rx = Some(spawn_reconnect(
                                        gn.addr.clone(),
                                        gn.code.clone(),
                                        gn.token.clone(),
                                    ));
                                    g.push_log(
                                        "Connection lost — reconnecting...".to_string(),
                                    );
                                }
                            }
                            Some("OVER") | Some("BYE") => {
                                if g.over.is_none() {
                                    let rest: Vec<&str> = it.collect();
                                    g.over = Some(if rest.is_empty() {
                                        "Connection lost".to_string()
                                    } else {
                                        rest.join(" ")
                                    });
                                    g.banner_t = 0.0;
                                }
                            }
                            _ => {}
                        }
                        // while catching up, apply events instantly
                        if gn.catching_up {
                            while let Some(ev) = gn.pending.pop_front() {
                                match ev {
                                    RepEvent::Attack { a, d, ra, rd, captured } => {
                                        g.net_apply_attack_fast(a, d, ra, rd, captured)
                                    }
                                    RepEvent::Reinforce { player, lands } => {
                                        g.net_apply_reinforce(player, lands)
                                    }
                                }
                            }
                        }
                    }
                    if g.battle.is_none() && g.over.is_none() {
                        if let Some(ev) = gn.pending.pop_front() {
                            match ev {
                                RepEvent::Attack {
                                    a,
                                    d,
                                    ra,
                                    rd,
                                    captured,
                                } => g.net_apply_attack(a, d, ra, rd, captured),
                                RepEvent::Reinforce { player, lands } => {
                                    g.net_apply_reinforce(player, lands)
                                }
                            }
                        }
                    }
                }
                if let Some(kind) = confirm {
                    // modal: destructive actions need explicit confirmation
                    if is_key_pressed(KeyCode::Escape) {
                        confirm = None;
                    } else if click {
                        let m = mouse_virtual();
                        if r_confirm_yes().contains(m) {
                            confirm = None;
                            match kind {
                                Confirm::NewMap => {
                                    menu.seed = random_seed();
                                    *g = gen_map(menu.seed, menu.players, 1, menu.difficulty);
                                }
                                Confirm::Menu => go_menu = true,
                            }
                        } else if r_confirm_no().contains(m) {
                            confirm = None;
                        }
                    }
                } else if click && g.over.is_none() && r_btn_menu().contains(mouse_virtual()) {
                    confirm = Some(Confirm::Menu);
                } else if click && g.over.is_none() && host.is_none() && guest.is_none() && r_btn_new().contains(mouse_virtual()) {
                    confirm = Some(Confirm::NewMap);
                } else if is_key_pressed(KeyCode::Escape) {
                    if g.selected.is_some() {
                        g.selected = None; // ESC first cancels the selection
                    } else if g.over.is_some() {
                        go_menu = true;
                    } else {
                        confirm = Some(Confirm::Menu);
                    }
                } else if is_key_pressed(KeyCode::R) && host.is_none() && guest.is_none() {
                    if g.over.is_some() {
                        menu.seed = random_seed();
                        *g = gen_map(menu.seed, menu.players, 1, menu.difficulty);
                    } else {
                        confirm = Some(Confirm::NewMap);
                    }
                } else if g.over.is_some() {
                    if g.over_t >= 0.6 && click {
                        let m = mouse_virtual();
                        if r_over_replay().contains(m) {
                            replay = Some(Replay::new(g, false));
                            screen = Screen::Replay;
                        } else if r_over_gif().contains(m) {
                            replay = Some(Replay::new(g, true));
                            screen = Screen::Replay;
                        } else {
                            go_menu = true;
                        }
                    }
                } else if g.battle.is_none() && g.over.is_none() {
                    let my_turn = if let Some(gn) = &guest {
                        g.current == gn.seat && gn.rec_rx.is_none() && !gn.catching_up
                    } else if host.is_some() {
                        g.current == 0
                    } else {
                        g.current < g.humans
                    };
                    if my_turn {
                        if guest.is_some() {
                            let mut outbox: Vec<String> = Vec::new();
                            handle_input(g, Some(&mut outbox));
                            if let Some(gn) = guest.as_mut() {
                                for l in outbox {
                                    send_net_line(&mut gn.stream, &l);
                                }
                            }
                        } else {
                            handle_input(g, None);
                        }
                    }
                }
                draw_game(g, &ui, &settings);
                if let Some(gn) = &guest {
                    if gn.rec_rx.is_some() && g.over.is_none() {
                        let txt = "Connection lost — reconnecting...";
                        let w = ui.width(txt, 22.0) + 56.0;
                        draw_round_frame(
                            (WIN_W - w) * 0.5,
                            270.0,
                            w,
                            56.0,
                            16.0,
                            2.5,
                            SRF(),
                            BORDER(),
                        );
                        ui.text_centered(txt, WIN_W * 0.5, 306.0, 22.0, INK());
                    }
                }
                if let Some(kind) = confirm {
                    draw_confirm(kind, &ui);
                }
                snd_queue.append(&mut g.snd);
                if go_menu {
                    confirm = None;
                    host = None;
                    guest = None;
                    awaiting.clear();
                    HUMAN_COLOR.store(settings.color, Ordering::Relaxed);
                    screen = Screen::Menu;
                    menu.regen();
                }
            }
            Screen::HostLobby => {
                menu_time += dt;
                let mut cancel = is_key_pressed(KeyCode::Escape);
                if is_mouse_button_pressed(MouseButton::Left)
                    && r_lobby_cancel().contains(mouse_virtual())
                {
                    cancel = true;
                }
                let h = host.as_mut().unwrap();
                while h.rx.try_recv().is_ok() {}
                copied_t = (copied_t - dt).max(0.0);
                let click2 = is_mouse_button_pressed(MouseButton::Left);
                let m2 = mouse_virtual();
                if click2 && r_lobby_copy().contains(m2) {
                    copy_to_clipboard(&format!("{}:{}", local_ip(), net_port()));
                    copied_t = 1.5;
                    snd_queue.push(Snd::Click);
                }
                if click2 && r_lobby_prev().contains(m2) {
                    menu.seed = (menu.seed + SEED_MOD - 1) % SEED_MOD;
                    menu.regen();
                    snd_queue.push(Snd::Click);
                }
                if click2 && r_lobby_next().contains(m2) {
                    menu.seed = (menu.seed + 1) % SEED_MOD;
                    menu.regen();
                    snd_queue.push(Snd::Click);
                }
                if click2 && r_lobby_new().contains(m2) {
                    menu.seed = random_seed();
                    menu.regen();
                    snd_queue.push(Snd::Click);
                }
                let want_start = (click2 && r_lobby_start().contains(mouse_virtual()))
                    || is_key_pressed(KeyCode::Enter)
                    || (test_host_auto > 0 && h.joined() >= test_host_auto);
                if want_start {
                    let humans = h.start_game(menu.seed, menu.players, menu.difficulty);
                    game = Some(gen_map(menu.seed, menu.players, humans, menu.difficulty));
                    screen = Screen::Play;
                    snd_queue.push(Snd::Turn);
                }
                draw_host_lobby(h, &menu, &ui, menu_time, copied_t);
                if cancel {
                    host = None;
                    screen = Screen::Menu;
                }
            }
            Screen::JoinLobby => {
                menu_time += dt;
                let mut back = is_key_pressed(KeyCode::Escape);
                let mut drop_guest = false;
                let click = is_mouse_button_pressed(MouseButton::Left);
                let m = mouse_virtual();
                if let Some(gn) = guest.as_mut() {
                    while let Ok(l) = gn.rx.try_recv() {
                        let mut it = l.split_whitespace();
                        match it.next() {
                            Some("START") => {
                                let seed: u64 =
                                    it.next().and_then(|v| v.parse().ok()).unwrap_or(0);
                                let players: usize =
                                    it.next().and_then(|v| v.parse().ok()).unwrap_or(2);
                                let humans: usize =
                                    it.next().and_then(|v| v.parse().ok()).unwrap_or(2);
                                let di: usize =
                                    it.next().and_then(|v| v.parse().ok()).unwrap_or(1);
                                if let Some(s) = it.next().and_then(|v| v.parse().ok()) {
                                    gn.seat = s;
                                }
                                if let Some(c) = it.next().and_then(|v| v.parse::<usize>().ok()) {
                                    // adopt the host's color mapping for this match
                                    HUMAN_COLOR.store(c % MAX_PLAYERS, Ordering::Relaxed);
                                }
                                let mut g =
                                    gen_map(seed, players.clamp(2, 8), humans, idx_diff(di));
                                g.net_guest = true;
                                g.my_seat = gn.seat;
                                game = Some(g);
                                screen = Screen::Play;
                                snd_queue.push(Snd::Turn);
                            }
                            Some("BYE") => {
                                join.status = "Disconnected by host".to_string();
                                drop_guest = true;
                            }
                            _ => {}
                        }
                    }
                } else {
                    let ctrl = is_key_down(KeyCode::LeftControl)
                        || is_key_down(KeyCode::RightControl);
                    let mut paste = ctrl && is_key_pressed(KeyCode::V);
                    if click && r_join_paste().contains(m) {
                        paste = true;
                    }
                    if paste {
                        if let Some(t) = paste_from_clipboard() {
                            if join.focus == 1 {
                                join.code =
                                    t.chars().filter(|c| c.is_ascii_digit()).take(4).collect();
                            } else {
                                join.addr = t
                                    .trim()
                                    .chars()
                                    .filter(|c| {
                                        c.is_ascii_alphanumeric()
                                            || *c == '.'
                                            || *c == ':'
                                            || *c == '-'
                                    })
                                    .take(40)
                                    .collect();
                            }
                        }
                    }
                    while let Some(c) = get_char_pressed() {
                        if ctrl {
                            continue; // don't type the 'v' from ctrl+v
                        }
                        if join.focus == 1 {
                            if c.is_ascii_digit() && join.code.len() < 4 {
                                join.code.push(c);
                            }
                        } else if (c.is_ascii_alphanumeric() || c == '.' || c == ':' || c == '-')
                            && join.addr.len() < 40
                        {
                            join.addr.push(c);
                        }
                    }
                    if is_key_pressed(KeyCode::Backspace) {
                        if join.focus == 1 {
                            join.code.pop();
                        } else {
                            join.addr.pop();
                        }
                    }
                    if is_key_pressed(KeyCode::Tab) {
                        join.focus ^= 1;
                    }
                    if click && r_join_addr().contains(m) {
                        join.focus = 0;
                    }
                    if click && r_join_code().contains(m) {
                        join.focus = 1;
                    }
                    if (click && r_join_go().contains(m)) || is_key_pressed(KeyCode::Enter) {
                        join.status = "Connecting...".to_string();
                        match guest_connect(&join.addr, &join.code) {
                            Ok(g) => {
                                guest = Some(g);
                                join.status = "Connected — waiting for the host...".to_string();
                            }
                            Err(e) => join.status = e,
                        }
                    }
                }
                if click && r_join_back().contains(m) {
                    back = true;
                }
                if drop_guest {
                    guest = None;
                }
                draw_join_lobby(&join, guest.is_some(), &ui, menu_time);
                if back {
                    guest = None;
                    screen = Screen::Menu;
                }
            }
            Screen::Replay => {
                let r = replay.as_mut().unwrap();
                let speed = if r.saving { 1.0 } else { 2.2 };
                r.g.update(dt * speed);
                let mut exit = is_key_pressed(KeyCode::Escape);
                if r.saving {
                    if r.saved.is_none() && r.idx < r.events.len() {
                        r.apply_instant();
                    }
                } else {
                    r.step_watch(dt * speed);
                }
                if r.saving && r.saved.is_none() {
                    if r.idx >= r.events.len() {
                        let path = replay_file(&format!("replay-{:08}.gif", r.g.seed));
                        r.saved = Some(
                            match gif_save(&path, GIF_W as u16, GIF_H as u16, &r.frames, 12) {
                                Ok(()) => format!("Saved {}", path),
                                Err(_) => "Could not save the GIF".to_string(),
                            },
                        );
                        snd_queue.push(Snd::Reinforce);
                    } else if let Some(rt) = r.rt.clone() {
                        // render the frame offscreen and read it back; capturing
                        // the live screen mid-frame corrupts macroquad's batching
                        set_camera(&Camera2D {
                            target: vec2(WIN_W * 0.5, WIN_H * 0.5),
                            zoom: vec2(2.0 / WIN_W, 2.0 / WIN_H),
                            render_target: Some(rt.clone()),
                            ..Default::default()
                        });
                        clear_background(BG());
                        draw_board(&r.g, &ui, false);
                        draw_panel_replay(&r.g, &ui);
                        set_camera(&view_camera());
                        if r.frames.len() < 600 {
                            let ss = downsample(&rt.texture.get_texture_data(), 2);
                            r.frames.push(quantize_image(&ss));
                        }
                    }
                }
                draw_board(&r.g, &ui, false);
                draw_battle_overlay(&r.g, &ui);
                draw_panel_replay(&r.g, &ui);
                draw_replay_hud(r, &ui);
                if (r.finished() || r.saved.is_some()) && is_mouse_button_pressed(MouseButton::Left)
                {
                    exit = true;
                }
                if r.saving {
                    r.g.snd.clear();
                } else {
                    snd_queue.append(&mut r.g.snd);
                }
                if exit {
                    replay = None;
                    screen = Screen::Menu;
                    menu.regen();
                }
            }
        }

        if let Some(s) = &sounds {
            if settings.muted {
                snd_queue.clear();
            }
            for e in snd_queue.drain(..) {
                s.play(e);
            }
        } else {
            snd_queue.clear();
        }

        set_default_camera();
        next_frame().await
    }
}
