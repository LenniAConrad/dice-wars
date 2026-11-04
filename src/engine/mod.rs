//! Small native 2D runtime. No Cargo dependencies or GPU context.
//! The render and input state belong to the main thread; audio uses one bounded worker.
pub mod audio;
mod font;
mod image;
mod math;
#[cfg(target_os = "linux")]
#[path = "linux.rs"]
mod platform;
#[cfg(windows)]
#[path = "windows.rs"]
mod platform;
#[cfg(target_os = "macos")]
#[path = "macos.rs"]
mod platform;
pub mod random;
mod raster;
#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
compile_error!("Dice Wars currently supports Linux, macOS and Windows.");

pub use font::*;
pub use image::*;
pub use math::*;
use std::cell::RefCell;
use std::collections::{HashSet, VecDeque};
use std::rc::Rc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub mod prelude {
    pub use super::{
        clear_background, draw_circle, draw_convex, draw_fan, draw_line, draw_mesh, draw_poly,
        draw_rectangle, draw_text_ex, draw_triangle, get_char_pressed, get_frame_time, is_key_down,
        is_key_pressed, is_mouse_button_down, is_mouse_button_pressed, measure_text, modifier_down,
        mouse_position, mouse_wheel, render_target, screen_height, screen_width, set_camera,
        set_default_camera, vec2, vec3, Camera2D, Color, Font, Image, KeyCode, Mesh, MouseButton,
        Rect, RenderTarget, TextParams, Vec2, Vertex, BLACK, WHITE,
    };
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum KeyCode {
    Space,
    Escape,
    Enter,
    Backspace,
    Tab,
    M,
    N,
    R,
    V,
    LeftControl,
    RightControl,
    /// Either Alt (Option on macOS).
    Alt,
    /// Either Windows/Super key. Command on macOS reports as Control.
    Super,
    F8,
    F9,
    F10,
    Left,
    Right,
    Up,
    Down,
}
#[derive(Clone, Copy)]
pub enum MouseButton {
    Left,
}
#[derive(Default)]
pub(super) struct Input {
    pub keys: HashSet<KeyCode>,
    pub pressed: HashSet<KeyCode>,
    pub chars: VecDeque<char>,
    pub mouse: (f32, f32),
    pub left: bool,
    pub clicked: bool,
    pub wheel: (f32, f32),
    pub closed: bool,
    pub focused: bool,
    pub repaint: bool,
    pub size: (usize, usize),
}
impl Input {
    pub fn key(&mut self, key: KeyCode, down: bool) {
        if down {
            if self.keys.insert(key)
                || matches!(
                    key,
                    KeyCode::Backspace
                        | KeyCode::Left
                        | KeyCode::Right
                        | KeyCode::Up
                        | KeyCode::Down
                )
            {
                self.pressed.insert(key);
            }
        } else {
            self.keys.remove(&key);
        }
    }
    pub fn reset(&mut self) {
        self.pressed.clear();
        self.chars.clear();
        self.clicked = false;
        self.wheel = (0.0, 0.0);
        self.repaint = false;
    }
    pub fn modifier_down(&self) -> bool {
        [
            KeyCode::LeftControl,
            KeyCode::RightControl,
            KeyCode::Alt,
            KeyCode::Super,
        ]
        .iter()
        .any(|k| self.keys.contains(k))
    }
    pub fn blur(&mut self) {
        self.keys.clear();
        self.left = false;
        self.focused = false;
    }
}

#[derive(Clone)]
pub struct Texture {
    surface: Rc<RefCell<raster::Surface>>,
}
impl Texture {
    pub fn get_texture_data(&self) -> Image {
        let mut s = self.surface.borrow_mut();
        s.settle();
        // Images use top-down rows on both native platforms and offscreen targets.
        let bytes = s.pixels.clone();
        Image {
            width: s.width as u16,
            height: s.height as u16,
            bytes,
        }
    }
}
#[derive(Clone)]
pub struct RenderTarget {
    pub texture: Texture,
}
pub fn render_target(w: u32, h: u32) -> RenderTarget {
    assert!(
        w > 0 && h > 0 && w <= 4096 && h <= 4096,
        "render target exceeds budget"
    );
    RenderTarget {
        texture: Texture {
            surface: Rc::new(RefCell::new(raster::Surface::new(w as usize, h as usize))),
        },
    }
}
#[derive(Clone)]
pub struct Camera2D {
    pub target: Vec2,
    pub zoom: Vec2,
    pub render_target: Option<RenderTarget>,
}
impl Default for Camera2D {
    fn default() -> Self {
        Self {
            target: Vec2::ZERO,
            zoom: vec2(1.0, 1.0),
            render_target: None,
        }
    }
}
#[derive(Clone, Copy)]
pub struct Vec3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}
pub fn vec3(x: f32, y: f32, z: f32) -> Vec3 {
    Vec3 { x, y, z }
}
pub struct Vertex {
    pub position: Vec3,
    pub color: [u8; 4],
}
pub struct Mesh {
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u16>,
    pub texture: Option<Texture>,
}

// A short blend from the picture on screen to whatever is drawn next (theme
// changes). Only the presented image is blended; captures stay exact.
struct Crossfade {
    from: Vec<u8>,
    out: raster::Surface,
    started: Instant,
    duration: f32,
}

struct Runtime {
    fade: Option<Crossfade>,
    surface: raster::Surface,
    presentation: raster::Surface,
    previous_raster: Vec<u8>,
    camera: Option<Camera2D>,
    input: Input,
    native: Option<platform::Window>,
    started: Instant,
    dt: f32,
    frames: usize,
    frame_limit: Option<usize>,
    capture: Option<String>,
    headless: bool,
    capture_frames: Vec<usize>,
    layers: std::collections::HashMap<u8, (u64, RenderTarget)>,
    layer_parts: std::collections::HashMap<u8, Vec<(u64, Rect)>>,
    sprites: std::collections::HashMap<u64, RenderTarget>,
    sprite_bytes: usize,
    actions: Vec<(usize, Vec<String>)>,
    input_dirty: bool,
    painted: bool,
    profile: Option<Vec<[f64; 4]>>,
    layer_blit_ms: f64,
}
thread_local! { static RUNTIME: RefCell<Option<Runtime>> = const { RefCell::new(None) }; }

pub fn now() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
}
// The raster is never below the window's own pixel grid: edges are
// anti-aliased analytically (triangles, circles, rounded rectangles, glyphs).
// Small windows (up to about a megapixel) also render at twice the size and
// average down, which keeps tiny details such as dice pips crisp.
fn raster_size(w: usize, h: usize) -> (usize, usize) {
    let (w, h) = (w.clamp(1, 4096), h.clamp(1, 4096));
    if w * h <= 1_100_000 && w * 2 <= 4096 && h * 2 <= 4096 {
        (w * 2, h * 2)
    } else {
        (w, h)
    }
}
pub fn init(title: &str, width: usize, height: usize) -> Result<(), String> {
    let headless = std::env::var_os("DW_HEADLESS").is_some();
    // Automated checks set DW_FORBID_WINDOW so a missing DW_HEADLESS fails
    // loudly instead of opening a window on someone's desktop.
    if !headless && std::env::var_os("DW_FORBID_WINDOW").is_some() {
        return Err("DW_FORBID_WINDOW is set: run with DW_HEADLESS=1".to_string());
    }
    let native = if headless {
        None
    } else {
        Some(platform::Window::new(title, width, height)?)
    };
    let (width, height) = std::env::var("DW_SIZE")
        .ok()
        .and_then(|s| {
            let (w, h) = s.split_once('x')?;
            Some((
                w.parse::<usize>().ok()?.clamp(240, 4096),
                h.parse::<usize>().ok()?.clamp(240, 4096),
            ))
        })
        .filter(|_| headless)
        .unwrap_or((width, height));
    let frame_limit = std::env::var("DW_FRAME_LIMIT")
        .ok()
        .and_then(|s| s.parse().ok())
        .or(headless.then_some(1));
    let capture = std::env::var("DW_CAPTURE").ok();
    let capture_frames = std::env::var("DW_CAPTURE_FRAMES")
        .unwrap_or_default()
        .split(',')
        .filter_map(|v| v.parse().ok())
        .collect();
    let (rw, rh) = raster_size(width, height);
    let mut rt = Runtime {
        fade: None,
        surface: raster::Surface::new(rw, rh),
        presentation: raster::Surface::new(width, height),
        previous_raster: Vec::new(),
        layer_blit_ms: 0.0,
        camera: None,
        input: Input {
            size: (width, height),
            focused: true,
            ..Default::default()
        },
        native,
        started: Instant::now(),
        dt: 1.0 / 60.0,
        frames: 0,
        frame_limit,
        capture,
        headless,
        capture_frames,
        layers: std::collections::HashMap::new(),
        layer_parts: std::collections::HashMap::new(),
        sprites: std::collections::HashMap::new(),
        sprite_bytes: 0,
        input_dirty: true,
        painted: false,
        profile: std::env::var_os("DW_PROFILE").map(|_| Vec::new()),
        actions: std::env::var("DW_INPUT")
            .unwrap_or_default()
            .split(';')
            .filter_map(|action| {
                let parts: Vec<String> = action.split(':').map(str::to_string).collect();
                Some((parts.first()?.parse().ok()?, parts[1..].to_vec()))
            })
            .collect(),
    };
    if let Some(w) = &mut rt.native {
        w.poll(&mut rt.input);
    }
    apply_actions(&mut rt);
    RUNTIME.with(|r| *r.borrow_mut() = Some(rt));
    Ok(())
}
fn read<T>(fallback: T, f: impl FnOnce(&Runtime) -> T) -> T {
    RUNTIME.with(|r| r.borrow().as_ref().map(f).unwrap_or(fallback))
}
pub fn screen_width() -> f32 {
    read(930.0, |r| r.input.size.0 as f32)
}
pub fn screen_height() -> f32 {
    read(820.0, |r| r.input.size.1 as f32)
}
pub fn mouse_position() -> (f32, f32) {
    read((0.0, 0.0), |r| r.input.mouse)
}
pub fn mouse_wheel() -> (f32, f32) {
    read((0.0, 0.0), |r| r.input.wheel)
}
pub fn input_changed() -> bool {
    read(true, |r| r.input_dirty)
}
/// Frames presented so far; lets per-frame animation state step once per frame.
pub fn frame_index() -> usize {
    read(0, |r| r.frames)
}
pub fn get_frame_time() -> f32 {
    read(1.0 / 60.0, |r| r.dt)
}
pub fn is_key_pressed(key: KeyCode) -> bool {
    read(false, |r| r.input.pressed.contains(&key))
}
pub fn is_key_down(key: KeyCode) -> bool {
    read(false, |r| r.input.keys.contains(&key))
}
/// Ctrl, Alt or Super/Command is held: plain-letter shortcuts should not fire.
pub fn modifier_down() -> bool {
    read(false, |r| r.input.modifier_down())
}
/// Swallow this frame's click, e.g. when an overlay already handled it.
pub fn consume_click() {
    RUNTIME.with(|r| {
        if let Some(r) = r.borrow_mut().as_mut() {
            r.input.clicked = false;
        }
    });
}
/// Swallow this frame's press of `key`.
pub fn consume_key(key: KeyCode) {
    RUNTIME.with(|r| {
        if let Some(r) = r.borrow_mut().as_mut() {
            r.input.pressed.remove(&key);
        }
    });
}
pub fn is_mouse_button_pressed(_: MouseButton) -> bool {
    read(false, |r| r.input.clicked)
}
pub fn is_mouse_button_down(_: MouseButton) -> bool {
    read(false, |r| r.input.left)
}
pub fn get_char_pressed() -> Option<char> {
    RUNTIME.with(|r| {
        r.borrow_mut()
            .as_mut()
            .and_then(|r| r.input.chars.pop_front())
    })
}
pub fn set_camera(camera: &Camera2D) {
    RUNTIME.with(|r| {
        if let Some(r) = r.borrow_mut().as_mut() {
            r.camera = Some(camera.clone());
        }
    });
}
pub fn set_default_camera() {
    RUNTIME.with(|r| {
        if let Some(r) = r.borrow_mut().as_mut() {
            r.camera = None;
        }
    });
}

fn paint(f: impl FnOnce(&mut raster::Surface, raster::Transform)) {
    RUNTIME.with(|r| {
        let mut guard = r.borrow_mut();
        let r = guard.as_mut().expect("engine not initialized");
        r.painted = true;
        let cam = r.camera.clone();
        if let Some(target) = cam.as_ref().and_then(|c| c.render_target.as_ref()) {
            let mut surface = target.texture.surface.borrow_mut();
            let transform = raster::Transform::camera(
                cam.as_ref(),
                surface.width,
                surface.height,
                surface.width,
                surface.height,
            );
            f(&mut surface, transform);
        } else {
            let transform = raster::Transform::camera(
                cam.as_ref(),
                r.input.size.0,
                r.input.size.1,
                r.surface.width,
                r.surface.height,
            );
            f(&mut r.surface, transform);
        }
    });
}
pub fn clear_background(c: Color) {
    paint(|s, _| s.clear(c));
}
pub fn draw_rectangle(x: f32, y: f32, w: f32, h: f32, c: Color) {
    paint(|s, t| {
        let a = t.point(vec2(x, y));
        let b = t.point(vec2(x + w, y + h));
        s.rect(a, b, c);
    });
}
#[allow(clippy::too_many_arguments)]
pub fn rounded_frame(
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    radius: f32,
    border_width: f32,
    fill: Color,
    border: Color,
) {
    paint(|s, t| {
        s.rounded(
            t.point(vec2(x, y)),
            t.point(vec2(x + w, y + h)),
            radius * t.sx.abs().min(t.sy.abs()),
            border_width * t.sx.abs().min(t.sy.abs()),
            fill,
            border,
        )
    });
}
/// An anti-aliased triangle. Shapes built from several triangles should use
/// `draw_convex`, `draw_poly` or `draw_line`, which keep their inner edges
/// seamless.
pub fn draw_triangle(a: Vec2, b: Vec2, c: Vec2, color: Color) {
    paint(|s, t| s.triangle_aa([t.point(a), t.point(b), t.point(c)], [color; 3], [true; 3]));
}

/// Triangles from `center` to each pair of neighboring `rim` points (a
/// closed loop): stars, ellipses, pips. Only the rim is anti-aliased, so the
/// spokes never show.
pub fn draw_fan(center: Vec2, rim: &[Vec2], color: Color) {
    let n = rim.len();
    paint(|s, t| {
        for i in 0..n {
            s.triangle_aa(
                [t.point(center), t.point(rim[i]), t.point(rim[(i + 1) % n])],
                [color; 3],
                [true, false, false],
            );
        }
    });
}

/// A convex polygon; `aa` smooths its outline. Pass false for tiles that
/// meet neighbors of the same color (their shared edges must not blend).
pub fn draw_convex(points: &[Vec2], color: Color, aa: bool) {
    let n = points.len();
    if n < 3 {
        return;
    }
    paint(|s, t| {
        for k in 1..n - 1 {
            // fan from point 0: only the polygon's own sides are outer edges
            let mask = [aa, aa && k + 1 == n - 1, aa && k == 1];
            s.triangle_aa(
                [
                    t.point(points[0]),
                    t.point(points[k]),
                    t.point(points[k + 1]),
                ],
                [color; 3],
                mask,
            );
        }
    });
}
/// Meshes are tessellations: no anti-aliasing, so faces tile seamlessly.
pub fn draw_mesh(mesh: &Mesh) {
    paint(|s, t| {
        for face in mesh.indices.chunks_exact(3) {
            let v = face.map_vertices(mesh);
            if let Some(v) = v {
                s.triangle(
                    v.map(|v| t.point(vec2(v.position.x, v.position.y))),
                    v.map(|v| Color::from_rgba(v.color[0], v.color[1], v.color[2], v.color[3])),
                );
            }
        }
    });
}
trait Face {
    fn map_vertices<'a>(&self, mesh: &'a Mesh) -> Option<[&'a Vertex; 3]>;
}
impl Face for [u16] {
    fn map_vertices<'a>(&self, m: &'a Mesh) -> Option<[&'a Vertex; 3]> {
        Some([
            m.vertices.get(self[0] as usize)?,
            m.vertices.get(self[1] as usize)?,
            m.vertices.get(self[2] as usize)?,
        ])
    }
}
pub fn draw_circle(x: f32, y: f32, radius: f32, color: Color) {
    paint(|s, t| {
        s.ellipse(
            t.point(vec2(x, y)),
            radius * t.sx.abs(),
            radius * t.sy.abs(),
            color,
        )
    });
}
pub fn draw_line(x: f32, y: f32, x2: f32, y2: f32, width: f32, color: Color) {
    let a = vec2(x, y);
    let b = vec2(x2, y2);
    let d = (b - a).normalize_or_zero();
    let p = vec2(-d.y, d.x) * (width * 0.5);
    // the two long sides are smooth; the shared diagonal and the butt ends
    // stay hard so the halves meet seamlessly and segments join cleanly
    paint(|s, t| {
        s.triangle_aa(
            [t.point(a + p), t.point(a - p), t.point(b - p)],
            [color; 3],
            [true, false, false],
        );
        s.triangle_aa(
            [t.point(a + p), t.point(b - p), t.point(b + p)],
            [color; 3],
            [false, true, false],
        );
    });
}
pub fn draw_poly(x: f32, y: f32, sides: u8, radius: f32, rotation: f32, color: Color) {
    if sides < 3 {
        return;
    }
    paint(|s, t| {
        let center = t.point(vec2(x, y));
        let point = |i: u8| {
            let angle = rotation.to_radians() + i as f32 * std::f32::consts::TAU / sides as f32;
            t.point(vec2(x + radius * angle.cos(), y + radius * angle.sin()))
        };
        for i in 0..sides {
            s.triangle_aa(
                [center, point(i), point(i + 1)],
                [color; 3],
                [true, false, false],
            );
        }
    });
}
pub fn draw_text_ex(text: &str, x: f32, y: f32, params: TextParams<'_>) {
    let fallback;
    let font = match params.font {
        Some(f) => f,
        None => {
            fallback = Font::embedded();
            &fallback
        }
    };
    paint(|s, t| {
        s.text(
            text,
            vec2(x, y),
            font,
            params.font_size as f32 * params.font_scale,
            params.color,
            t,
        )
    });
}

/// Cache a static layer at the current render resolution. Animation callers change the key.
pub fn cached_layer(id: u8, key: u64, draw: impl FnOnce()) {
    cached_layer_parts(id, key, &[], draw);
}

/// A cached layer made of independently changing parts. `key` covers what
/// affects the whole picture; each part has its own key and the region it
/// can touch (in drawing coordinates). When only some part keys change, just
/// their regions (old and new) are repainted: `draw` runs clipped to them.
pub fn cached_layer_parts(id: u8, key: u64, parts: &[(u64, Rect)], draw: impl FnOnce()) {
    use std::hash::{Hash, Hasher};
    let (camera, target, redraw, full_key) = RUNTIME.with(|r| {
        let mut guard = r.borrow_mut();
        let r = guard.as_mut().unwrap();
        let camera = r.camera.clone();
        let (w, h) = camera
            .as_ref()
            .and_then(|c| c.render_target.as_ref())
            .map(|rt| {
                let s = rt.texture.surface.borrow();
                (s.width, s.height)
            })
            .unwrap_or((r.surface.width, r.surface.height));
        let mut hash = std::collections::hash_map::DefaultHasher::new();
        (key, w, h, parts.len()).hash(&mut hash);
        if let Some(c) = &camera {
            (
                c.target.x.to_bits(),
                c.target.y.to_bits(),
                c.zoom.x.to_bits(),
                c.zoom.y.to_bits(),
            )
                .hash(&mut hash);
        }
        let full_key = hash.finish();
        let cached = r
            .layers
            .get(&id)
            .filter(|(k, _)| *k == full_key)
            .map(|(_, target)| target.clone());
        let mut layer_camera = camera.clone().unwrap_or(Camera2D {
            target: vec2(r.input.size.0 as f32 * 0.5, r.input.size.1 as f32 * 0.5),
            zoom: vec2(2.0 / r.input.size.0 as f32, 2.0 / r.input.size.1 as f32),
            render_target: None,
        });
        // None: reuse as is; Some(None): repaint all; Some(Some(rect)): repaint a region
        let redraw = match &cached {
            None => Some(None),
            Some(_) => {
                let old = r.layer_parts.get(&id);
                let t = raster::Transform::camera(Some(&layer_camera), w, h, w, h);
                let mut region: Option<(f32, f32, f32, f32)> = None;
                let mut grow = |rect: Rect| {
                    let a = t.point(vec2(rect.x, rect.y));
                    let b = t.point(vec2(rect.x + rect.w, rect.y + rect.h));
                    let (x0, y0, x1, y1) = (a.x.min(b.x), a.y.min(b.y), a.x.max(b.x), a.y.max(b.y));
                    region = Some(match region {
                        Some((u0, v0, u1, v1)) => (u0.min(x0), v0.min(y0), u1.max(x1), v1.max(y1)),
                        None => (x0, y0, x1, y1),
                    });
                };
                for (i, &(k, rect)) in parts.iter().enumerate() {
                    match old.and_then(|o| o.get(i)) {
                        Some(&(ok, _)) if ok == k => {}
                        Some(&(_, orect)) => {
                            grow(orect);
                            grow(rect);
                        }
                        None => grow(rect),
                    }
                }
                match region {
                    None => None,
                    Some((x0, y0, x1, y1)) => {
                        let rx0 = (x0.floor() - 2.0).max(0.0) as usize;
                        let ry0 = (y0.floor() - 2.0).max(0.0) as usize;
                        let rx1 = ((x1.ceil() + 2.0).max(0.0) as usize).min(w);
                        let ry1 = ((y1.ceil() + 2.0).max(0.0) as usize).min(h);
                        if rx0 >= rx1 || ry0 >= ry1 {
                            None
                        } else if (rx1 - rx0) * (ry1 - ry0) * 2 > w * h
                            || std::env::var_os("DW_FULL_LAYERS").is_some()
                        {
                            Some(None) // most of it: a full repaint is as cheap
                        } else {
                            Some(Some((rx0, ry0, rx1, ry1)))
                        }
                    }
                }
            }
        };
        let target = cached
            .clone()
            .or_else(|| {
                r.layers
                    .get(&id)
                    .map(|(_, target)| target.clone())
                    .filter(|target| {
                        let s = target.texture.surface.borrow();
                        (s.width, s.height) == (w, h)
                    })
            })
            .unwrap_or_else(|| render_target(w as u32, h as u32));
        if let Some(region) = redraw {
            let mut surface = target.texture.surface.borrow_mut();
            surface.set_clip(region);
            if region.is_none() {
                surface.clear(Color::new(0.0, 0.0, 0.0, 0.0));
            }
            drop(surface);
            layer_camera.render_target = Some(target.clone());
            r.camera = Some(layer_camera);
        }
        (camera, target, redraw.is_some(), full_key)
    });
    if redraw {
        draw();
        {
            let mut surface = target.texture.surface.borrow_mut();
            surface.set_clip(None);
            surface.settle();
        }
        RUNTIME.with(|r| {
            let mut guard = r.borrow_mut();
            let r = guard.as_mut().unwrap();
            r.camera = camera;
            r.layers.insert(id, (full_key, target.clone()));
            r.layer_parts.insert(id, parts.to_vec());
        });
    }
    let profiling = RUNTIME.with(|r| r.borrow().as_ref().unwrap().profile.is_some());
    let started = profiling.then(Instant::now);
    paint(|surface, _| surface.blit(&target.texture.surface.borrow()));
    if let Some(started) = started {
        RUNTIME.with(|r| {
            r.borrow_mut().as_mut().unwrap().layer_blit_ms +=
                started.elapsed().as_secs_f64() * 1000.0
        });
    }
}

/// Cache a small repeated illustration at the current raster scale. Movement
/// changes its destination, not its geometry. The cache has a 64 MiB ceiling.
pub fn cached_sprite(key: u64, bounds: Rect, draw: impl FnOnce()) {
    use std::hash::{Hash, Hasher};
    let (camera, scale, full_key, cached) = RUNTIME.with(|r| {
        let guard = r.borrow();
        let r = guard.as_ref().unwrap();
        let camera = r.camera.clone();
        let (w, h) = camera
            .as_ref()
            .and_then(|c| c.render_target.as_ref())
            .map(|rt| {
                let s = rt.texture.surface.borrow();
                (s.width, s.height)
            })
            .unwrap_or((r.surface.width, r.surface.height));
        let t = raster::Transform::camera(camera.as_ref(), r.input.size.0, r.input.size.1, w, h);
        let mut hash = std::collections::hash_map::DefaultHasher::new();
        (
            key,
            t.sx.to_bits(),
            t.sy.to_bits(),
            bounds.w.to_bits(),
            bounds.h.to_bits(),
        )
            .hash(&mut hash);
        let full_key = hash.finish();
        (
            camera,
            (t.sx, t.sy),
            full_key,
            r.sprites.get(&full_key).cloned(),
        )
    });
    let (sx, sy) = scale;
    let w = (bounds.w * sx).ceil() as u32;
    let h = (bounds.h * sy).ceil() as u32;
    if sx <= 0.0 || sy <= 0.0 || w == 0 || h == 0 || w > 2048 || h > 2048 {
        draw();
        return;
    }
    let target = cached.unwrap_or_else(|| {
        let target = render_target(w, h);
        set_camera(&Camera2D {
            target: vec2(
                bounds.x + w as f32 / (2.0 * sx),
                bounds.y + h as f32 / (2.0 * sy),
            ),
            zoom: vec2(2.0 * sx / w as f32, 2.0 * sy / h as f32),
            render_target: Some(target.clone()),
        });
        draw();
        target.texture.surface.borrow_mut().settle();
        RUNTIME.with(|r| {
            let mut guard = r.borrow_mut();
            let r = guard.as_mut().unwrap();
            r.camera = camera;
            let bytes = w as usize * h as usize * 4;
            if r.sprite_bytes + bytes > 64 * 1024 * 1024 || r.sprites.len() >= 1024 {
                r.sprites.clear();
                r.sprite_bytes = 0;
            }
            r.sprite_bytes += bytes;
            r.sprites.insert(full_key, target.clone());
        });
        target
    });
    paint(|surface, t| {
        surface.blit_at(
            &target.texture.surface.borrow(),
            t.point(vec2(bounds.x, bounds.y)),
        )
    });
}

fn apply_actions(r: &mut Runtime) {
    for (_, action) in r.actions.iter().filter(|(frame, _)| *frame == r.frames + 1) {
        match action.first().map(String::as_str) {
            Some("click" | "move") if action.len() == 3 => {
                if let (Ok(x), Ok(y)) = (action[1].parse(), action[2].parse()) {
                    r.input.mouse = (x, y);
                    r.input.clicked = action[0] == "click";
                }
            }
            Some("key") if action.len() == 2 => {
                let key = match action[1].as_str() {
                    "F8" => Some(KeyCode::F8),
                    "F10" => Some(KeyCode::F10),
                    "F9" => Some(KeyCode::F9),
                    "Enter" => Some(KeyCode::Enter),
                    "Space" => Some(KeyCode::Space),
                    "Escape" => Some(KeyCode::Escape),
                    "Right" => Some(KeyCode::Right),
                    "Left" => Some(KeyCode::Left),
                    "Up" => Some(KeyCode::Up),
                    "Down" => Some(KeyCode::Down),
                    _ => None,
                };
                if let Some(key) = key {
                    r.input.pressed.insert(key);
                }
            }
            _ => {}
        }
    }
}

/// Blend from the frame now on screen into the frames that follow, over
/// `seconds`. Restarting mid-fade continues from the blended picture.
pub fn crossfade(seconds: f32) {
    RUNTIME.with(|r| {
        let mut guard = r.borrow_mut();
        let Some(r) = guard.as_mut() else { return };
        if r.native.is_none() {
            return; // headless runs and captures stay exact
        }
        let from = match &r.fade {
            Some(f) if f.out.pixels.len() == r.presentation.pixels.len() => f.out.pixels.clone(),
            _ => r.presentation.pixels.clone(),
        };
        let mut out = raster::Surface::new(r.presentation.width, r.presentation.height);
        out.pixels.copy_from_slice(&from);
        r.fade = Some(Crossfade {
            from,
            out,
            started: Instant::now(),
            duration: seconds.max(0.01),
        });
    });
}

/// Present at most 60 fps (15 while unfocused), and release all native resources on exit.
pub fn next_frame() -> bool {
    RUNTIME.with(|r| {
        let mut guard = r.borrow_mut();
        let r = guard.as_mut().unwrap();
        if r.input.closed {
            return false;
        }
        r.frames += 1;
        let draw_ms = r
            .profile
            .as_ref()
            .map(|_| r.started.elapsed().as_secs_f64() * 1000.0)
            .unwrap_or(0.0);
        let resolve_started = r.profile.as_ref().map(|_| Instant::now());
        let last = r.frame_limit.is_some_and(|limit| r.frames >= limit);
        let mut damage = if r.painted {
            r.surface
                .resolve_changed_into(&mut r.presentation, &mut r.previous_raster)
        } else {
            Vec::new()
        };
        if r.input.repaint {
            damage.clear();
            damage.push((0, 0, r.presentation.width, r.presentation.height));
        }
        let resolve_ms = resolve_started
            .map(|s| s.elapsed().as_secs_f64() * 1000.0)
            .unwrap_or(0.0);
        let capture_now = last || r.capture_frames.contains(&r.frames);
        if let Some(path) = r.capture.as_ref().filter(|_| capture_now) {
            let path = if last {
                path.clone()
            } else {
                format!("{}-{}.png", path.trim_end_matches(".png"), r.frames)
            };
            let image = Image {
                width: r.presentation.width as u16,
                height: r.presentation.height as u16,
                bytes: r.presentation.pixels.clone(),
            };
            if let Err(e) = image.export_png(&path) {
                eprintln!("Capture failed: {e}");
            }
        }
        let mut faded = false;
        if let Some(fade) = &mut r.fade {
            let t = fade.started.elapsed().as_secs_f32() / fade.duration;
            if t >= 1.0 || fade.from.len() != r.presentation.pixels.len() {
                r.fade = None;
                damage = vec![(0, 0, r.presentation.width, r.presentation.height)];
            } else {
                // smoothstep keeps both ends gentle
                let k = (t * t * (3.0 - 2.0 * t) * 256.0) as u16;
                for ((o, &a), &b) in fade
                    .out
                    .pixels
                    .iter_mut()
                    .zip(&fade.from)
                    .zip(&r.presentation.pixels)
                {
                    *o = ((a as u16 * (256 - k) + b as u16 * k) >> 8) as u8;
                }
                if let Some(w) = &mut r.native {
                    let all = [(0, 0, fade.out.width, fade.out.height)];
                    w.present(&fade.out, &all);
                }
                faded = true;
            }
        }
        if !damage.is_empty() && !faded {
            if let Some(w) = &mut r.native {
                w.present(&r.presentation, &damage);
            }
        }
        r.painted = false;
        if let Some(samples) = &mut r.profile {
            if r.frames > 5 && samples.len() < 6000 {
                samples.push([
                    r.started.elapsed().as_secs_f64() * 1000.0,
                    draw_ms,
                    resolve_ms,
                    r.layer_blit_ms,
                ]);
            }
        }
        r.layer_blit_ms = 0.0;
        if last {
            return false;
        }
        let period = Duration::from_secs_f64(if r.input.focused {
            1.0 / 60.0
        } else {
            1.0 / 15.0
        });
        // Headless runs are unpaced, except when they produce real-time audio.
        if !r.headless || audio::headless_audio() {
            if let Some(remaining) = period.checked_sub(r.started.elapsed()) {
                std::thread::sleep(remaining);
            }
        }
        r.dt = if r.headless && !audio::headless_audio() {
            1.0 / 60.0
        } else {
            r.started.elapsed().as_secs_f32().min(0.1)
        };
        r.started = Instant::now();
        let previous = (r.input.mouse, r.input.size, r.input.focused, r.input.left);
        r.input.reset();
        if let Some(w) = &mut r.native {
            w.poll(&mut r.input);
        }
        apply_actions(r);
        r.input_dirty = previous != (r.input.mouse, r.input.size, r.input.focused, r.input.left)
            || r.input.left
            || r.input.clicked
            || !r.input.pressed.is_empty()
            || !r.input.chars.is_empty()
            || r.input.wheel != (0.0, 0.0);
        r.input_dirty |= r.input.repaint;
        // Limit render memory and work even on giant desktop resolutions.
        let (w, h) = r.input.size;
        let size = raster_size(w, h);
        if size != (r.surface.width, r.surface.height) {
            r.surface = raster::Surface::new(size.0, size.1);
            r.previous_raster.clear();
        }
        let output_size = (w.clamp(1, 4096), h.clamp(1, 4096));
        if output_size != (r.presentation.width, r.presentation.height) {
            r.presentation = raster::Surface::new(output_size.0, output_size.1);
            r.previous_raster.clear();
        }
        !r.input.closed
    })
}
pub fn shutdown() {
    RUNTIME.with(|r| {
        if let Some(rt) = r.borrow_mut().take() {
            if let Some(mut samples) = rt.profile.filter(|s| !s.is_empty()) {
                let means: [f64; 4] = std::array::from_fn(|i| samples.iter().map(|s| s[i]).sum::<f64>() / samples.len() as f64);
                let mean = means[0];
                samples.sort_by(|a, b| a[0].total_cmp(&b[0]));
                let slow = samples.iter().filter(|s| s[0] > 1000.0 / 60.0).count();
                eprintln!(
                    "Frame work: mean {mean:.2} ms, p95 {:.2} ms, max {:.2} ms, {slow} over 16.7 ms ({} frames)",
                    samples[(samples.len() * 95 / 100).min(samples.len() - 1)][0],
                    samples[samples.len() - 1][0],
                    samples.len()
                );
                eprintln!("Frame stages: update/draw {:.2} ms (layer copies {:.2}), resolve {:.2} ms, present/capture {:.2} ms", means[1], means[3], means[2], (means[0] - means[1] - means[2]).max(0.0));
            }
        }
    });
}
pub fn clipboard_set(text: &str) {
    platform::clipboard_set(text);
}
/// Report an error that stops the game from starting. Windows release builds
/// have no console, so the message is also shown in a dialog there (never in
/// headless or window-forbidden runs).
pub fn fatal_error(message: &str) {
    eprintln!("{message}");
    #[cfg(windows)]
    if std::env::var_os("DW_HEADLESS").is_none() && std::env::var_os("DW_FORBID_WINDOW").is_none() {
        platform::show_error("Dice Wars", message);
    }
}
pub fn clipboard_get() -> Option<String> {
    platform::clipboard_get()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn the_raster_is_never_below_the_window_resolution() {
        assert_eq!(raster_size(930, 820), (1860, 1640));
        for (w, h) in [(1920, 1080), (3840, 2160), (4096, 4096), (1601, 821)] {
            assert_eq!(raster_size(w, h), (w, h));
        }
        assert_eq!(raster_size(5120, 2880), (4096, 2880));
    }
    #[test]
    fn readback_keeps_top_down_rows_for_exports() {
        let target = render_target(2, 2);
        let pixels = vec![
            255, 0, 0, 255, 255, 0, 0, 255, 0, 0, 255, 255, 0, 0, 255, 255,
        ];
        target.texture.surface.borrow_mut().pixels = pixels.clone();
        assert_eq!(target.texture.get_texture_data().bytes, pixels);
    }
    #[test]
    fn holding_a_shortcut_does_not_toggle_it_every_frame() {
        let mut input = Input::default();
        input.key(KeyCode::F10, true);
        assert!(input.pressed.contains(&KeyCode::F10));
        input.reset();
        input.key(KeyCode::F10, true);
        assert!(!input.pressed.contains(&KeyCode::F10));
        input.key(KeyCode::F10, false);
        input.key(KeyCode::F10, true);
        assert!(input.pressed.contains(&KeyCode::F10));
        input.blur();
        assert!(input.keys.is_empty());
    }
    #[test]
    fn modifiers_block_plain_shortcuts() {
        let mut input = Input::default();
        input.key(KeyCode::R, true);
        assert!(!input.modifier_down());
        for key in [KeyCode::RightControl, KeyCode::Alt, KeyCode::Super] {
            input.key(key, true);
            assert!(input.modifier_down());
            input.key(key, false);
            assert!(!input.modifier_down());
        }
        input.key(KeyCode::LeftControl, true);
        input.blur();
        assert!(!input.modifier_down());
    }
}
