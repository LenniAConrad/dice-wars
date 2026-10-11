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
pub mod random;
mod raster;
#[cfg(not(any(target_os = "linux", windows)))]
compile_error!("Dice Wars currently supports Linux and Windows.");

pub use font::*;
pub use image::*;
pub use math::*;
use std::cell::RefCell;
use std::collections::{HashSet, VecDeque};
use std::rc::Rc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub mod prelude {
    pub use super::{
        clear_background, draw_circle, draw_line, draw_mesh, draw_poly, draw_rectangle,
        draw_text_ex, draw_triangle, get_char_pressed, get_frame_time, is_key_down, is_key_pressed,
        is_mouse_button_down, is_mouse_button_pressed, measure_text, mouse_position, mouse_wheel,
        render_target, screen_height, screen_width, set_camera, set_default_camera, vec2, vec3,
        vec4, Camera2D, Color, Font, Image, KeyCode, Mesh, MouseButton, Rect, RenderTarget,
        TextParams, Vec2, Vertex, BLACK, WHITE,
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
        let s = self.surface.borrow();
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
#[derive(Clone, Copy)]
pub struct Vec4;
pub fn vec4(_: f32, _: f32, _: f32, _: f32) -> Vec4 {
    Vec4
}
pub struct Vertex {
    pub position: Vec3,
    pub uv: Vec2,
    pub color: [u8; 4],
    pub normal: Vec4,
}
pub struct Mesh {
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u16>,
    pub texture: Option<Texture>,
}

struct Runtime {
    surface: raster::Surface,
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
    actions: Vec<(usize, Vec<String>)>,
    input_dirty: bool,
    painted: bool,
}
thread_local! { static RUNTIME: RefCell<Option<Runtime>> = const { RefCell::new(None) }; }

pub fn now() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
}
pub fn init(title: &str, width: usize, height: usize) -> Result<(), String> {
    let headless = std::env::var_os("DW_HEADLESS").is_some();
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
    let mut rt = Runtime {
        surface: raster::Surface::new(width, height),
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
        input_dirty: true,
        painted: false,
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
pub fn get_frame_time() -> f32 {
    read(1.0 / 60.0, |r| r.dt)
}
pub fn is_key_pressed(key: KeyCode) -> bool {
    read(false, |r| r.input.pressed.contains(&key))
}
pub fn is_key_down(key: KeyCode) -> bool {
    read(false, |r| r.input.keys.contains(&key))
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
pub fn draw_triangle(a: Vec2, b: Vec2, c: Vec2, color: Color) {
    paint(|s, t| s.triangle([t.point(a), t.point(b), t.point(c)], [color; 3]));
}
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
    paint(|s, t| {
        s.triangle([t.point(a + p), t.point(a - p), t.point(b - p)], [color; 3]);
        s.triangle([t.point(a + p), t.point(b - p), t.point(b + p)], [color; 3]);
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
            s.triangle([center, point(i), point(i + 1)], [color; 3]);
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
    use std::hash::{Hash, Hasher};
    let (camera, target, hit, full_key) = RUNTIME.with(|r| {
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
        (key, w, h).hash(&mut hash);
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
        let hit = cached.is_some();
        let target = cached
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
        if !hit {
            target
                .texture
                .surface
                .borrow_mut()
                .clear(Color::new(0.0, 0.0, 0.0, 0.0));
        }
        let mut layer_camera = camera.clone().unwrap_or(Camera2D {
            target: vec2(r.input.size.0 as f32 * 0.5, r.input.size.1 as f32 * 0.5),
            zoom: vec2(2.0 / r.input.size.0 as f32, 2.0 / r.input.size.1 as f32),
            render_target: None,
        });
        layer_camera.render_target = Some(target.clone());
        if !hit {
            r.camera = Some(layer_camera);
        }
        (camera, target, hit, full_key)
    });
    if !hit {
        draw();
        RUNTIME.with(|r| {
            let mut guard = r.borrow_mut();
            let r = guard.as_mut().unwrap();
            r.camera = camera;
            r.layers.insert(id, (full_key, target.clone()));
        });
    }
    paint(|surface, _| surface.blit(&target.texture.surface.borrow()));
}

fn apply_actions(r: &mut Runtime) {
    for (_, action) in r.actions.iter().filter(|(frame, _)| *frame == r.frames + 1) {
        match action.first().map(String::as_str) {
            Some("click") if action.len() == 3 => {
                if let (Ok(x), Ok(y)) = (action[1].parse(), action[2].parse()) {
                    r.input.mouse = (x, y);
                    r.input.clicked = true;
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

/// Present at most 60 fps (15 while unfocused), and release all native resources on exit.
pub fn next_frame() -> bool {
    RUNTIME.with(|r| {
        let mut guard = r.borrow_mut();
        let r = guard.as_mut().unwrap();
        if r.input.closed {
            return false;
        }
        r.frames += 1;
        let last = r.frame_limit.is_some_and(|limit| r.frames >= limit);
        if (last || r.capture_frames.contains(&r.frames)) && r.capture.is_some() {
            let path = r.capture.as_ref().unwrap();
            let path = if last {
                path.clone()
            } else {
                format!("{}-{}.png", path.trim_end_matches(".png"), r.frames)
            };
            let image = Image {
                width: r.surface.width as u16,
                height: r.surface.height as u16,
                bytes: r.surface.pixels.clone(),
            };
            if let Err(e) = image.export_png(&path) {
                eprintln!("Capture failed: {e}");
            }
        }
        if r.painted {
            if let Some(w) = &mut r.native {
                w.present(&r.surface);
            }
        }
        r.painted = false;
        if last {
            return false;
        }
        let period = Duration::from_secs_f64(if r.input.focused {
            1.0 / 60.0
        } else {
            1.0 / 15.0
        });
        if !r.headless {
            if let Some(remaining) = period.checked_sub(r.started.elapsed()) {
                std::thread::sleep(remaining);
            }
        }
        r.dt = if r.headless {
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
        let scale = (1600.0 / w as f32).min(1400.0 / h as f32).min(1.0);
        let size = (
            (w as f32 * scale).round().max(1.0) as usize,
            (h as f32 * scale).round().max(1.0) as usize,
        );
        if size != (r.surface.width, r.surface.height) {
            r.surface = raster::Surface::new(size.0, size.1);
        }
        !r.input.closed
    })
}
pub fn shutdown() {
    RUNTIME.with(|r| {
        r.borrow_mut().take();
    });
}
pub fn clipboard_set(text: &str) {
    platform::clipboard_set(text);
}
pub fn clipboard_get() -> Option<String> {
    platform::clipboard_get()
}

#[cfg(test)]
mod tests {
    use super::*;
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
}
