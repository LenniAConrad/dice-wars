//! X11 transport for the software framebuffer. All Xlib calls stay on the UI thread.
use super::{gamepad, raster::Surface, Input, KeyCode};
use std::ffi::{c_char, c_int, c_long, c_uint, c_ulong, c_void, CString};
use std::ptr;
type Display = c_void;
#[repr(C)]
struct KeyEvent {
    kind: c_int,
    serial: c_ulong,
    send: c_int,
    display: *mut Display,
    window: c_ulong,
    root: c_ulong,
    subwindow: c_ulong,
    time: c_ulong,
    x: c_int,
    y: c_int,
    x_root: c_int,
    y_root: c_int,
    state: c_uint,
    code: c_uint,
    same: c_int,
}
#[repr(C)]
struct ConfigureEvent {
    kind: c_int,
    serial: c_ulong,
    send: c_int,
    display: *mut Display,
    event: c_ulong,
    window: c_ulong,
    x: c_int,
    y: c_int,
    width: c_int,
    height: c_int,
    border: c_int,
    above: c_ulong,
    override_redirect: c_int,
}
#[repr(C)]
struct ClientEvent {
    kind: c_int,
    serial: c_ulong,
    send: c_int,
    display: *mut Display,
    window: c_ulong,
    message: c_ulong,
    format: c_int,
    data: [c_long; 5],
}
#[repr(C)]
struct XImage {
    width: c_int,
    height: c_int,
    xoffset: c_int,
    format: c_int,
    data: *mut c_char,
    byte_order: c_int,
    bitmap_unit: c_int,
    bitmap_bit_order: c_int,
    bitmap_pad: c_int,
    depth: c_int,
    bytes_per_line: c_int,
    bits_per_pixel: c_int,
    red_mask: c_ulong,
    green_mask: c_ulong,
    blue_mask: c_ulong,
    obdata: *mut c_void,
    functions: [usize; 6],
}
#[link(name = "libX11.so.6", kind = "dylib", modifiers = "+verbatim")]
extern "C" {
    fn XOpenDisplay(name: *const c_char) -> *mut Display;
    fn XCloseDisplay(d: *mut Display) -> c_int;
    fn XDefaultScreen(d: *mut Display) -> c_int;
    fn XRootWindow(d: *mut Display, s: c_int) -> c_ulong;
    fn XDefaultVisual(d: *mut Display, s: c_int) -> *mut c_void;
    fn XDefaultDepth(d: *mut Display, s: c_int) -> c_int;
    fn XCreateSimpleWindow(
        d: *mut Display,
        parent: c_ulong,
        x: c_int,
        y: c_int,
        w: c_uint,
        h: c_uint,
        border: c_uint,
        border_pixel: c_ulong,
        bg: c_ulong,
    ) -> c_ulong;
    fn XDestroyWindow(d: *mut Display, w: c_ulong) -> c_int;
    fn XStoreName(d: *mut Display, w: c_ulong, name: *const c_char) -> c_int;
    fn XSelectInput(d: *mut Display, w: c_ulong, mask: c_long) -> c_int;
    fn XMapWindow(d: *mut Display, w: c_ulong) -> c_int;
    fn XInternAtom(d: *mut Display, name: *const c_char, only: c_int) -> c_ulong;
    fn XSetWMProtocols(d: *mut Display, w: c_ulong, atoms: *mut c_ulong, n: c_int) -> c_int;
    fn XChangeProperty(
        d: *mut Display,
        w: c_ulong,
        property: c_ulong,
        kind: c_ulong,
        format: c_int,
        mode: c_int,
        data: *const u8,
        n: c_int,
    ) -> c_int;
    fn XCreateGC(d: *mut Display, w: c_ulong, mask: c_ulong, values: *mut c_void) -> *mut c_void;
    fn XFreeGC(d: *mut Display, gc: *mut c_void) -> c_int;
    fn XPending(d: *mut Display) -> c_int;
    fn XNextEvent(d: *mut Display, event: *mut c_void) -> c_int;
    fn XPeekEvent(d: *mut Display, event: *mut c_void) -> c_int;
    fn XLookupString(
        event: *mut KeyEvent,
        buffer: *mut c_char,
        len: c_int,
        sym: *mut c_ulong,
        compose: *mut c_void,
    ) -> c_int;
    fn XCreateImage(
        d: *mut Display,
        visual: *mut c_void,
        depth: c_uint,
        format: c_int,
        offset: c_int,
        data: *mut c_char,
        w: c_uint,
        h: c_uint,
        pad: c_int,
        stride: c_int,
    ) -> *mut XImage;
    fn XDestroyImage(image: *mut XImage) -> c_int;
    fn XPutImage(
        d: *mut Display,
        w: c_ulong,
        gc: *mut c_void,
        image: *mut XImage,
        sx: c_int,
        sy: c_int,
        dx: c_int,
        dy: c_int,
        width: c_uint,
        height: c_uint,
    ) -> c_int;
    fn XSync(d: *mut Display, discard: c_int) -> c_int;
    fn XFlush(d: *mut Display) -> c_int;
    fn XSetWindowBackgroundPixmap(d: *mut Display, w: c_ulong, pixmap: c_ulong) -> c_int;
    fn XSetWMNormalHints(d: *mut Display, w: c_ulong, hints: *mut SizeHints);
    fn XCreateFontCursor(d: *mut Display, shape: c_uint) -> c_ulong;
    fn XDefineCursor(d: *mut Display, w: c_ulong, cursor: c_ulong) -> c_int;
    fn XFreeCursor(d: *mut Display, cursor: c_ulong) -> c_int;
    fn XOpenIM(
        d: *mut Display,
        db: *mut c_void,
        name: *mut c_char,
        class: *mut c_char,
    ) -> *mut c_void;
    fn XCloseIM(im: *mut c_void) -> c_int;
    fn XCreateIC(im: *mut c_void, ...) -> *mut c_void;
    fn XDestroyIC(ic: *mut c_void);
    fn XSetICFocus(ic: *mut c_void);
    fn XUnsetICFocus(ic: *mut c_void);
    fn Xutf8LookupString(
        ic: *mut c_void,
        event: *mut KeyEvent,
        buffer: *mut c_char,
        len: c_int,
        sym: *mut c_ulong,
        status: *mut c_int,
    ) -> c_int;
    fn XFilterEvent(event: *mut c_void, window: c_ulong) -> c_int;
    fn XSetLocaleModifiers(modifiers: *const c_char) -> *mut c_char;
}
extern "C" {
    fn setlocale(category: c_int, locale: *const c_char) -> *mut c_char;
    fn ioctl(fd: c_int, request: c_ulong, ...) -> c_int;
}
// XSizeHints, for the minimum window size.
#[repr(C)]
#[derive(Default)]
struct SizeHints {
    flags: c_long,
    x: c_int,
    y: c_int,
    width: c_int,
    height: c_int,
    min_width: c_int,
    min_height: c_int,
    max_width: c_int,
    max_height: c_int,
    width_inc: c_int,
    height_inc: c_int,
    min_aspect_x: c_int,
    min_aspect_y: c_int,
    max_aspect_x: c_int,
    max_aspect_y: c_int,
    base_width: c_int,
    base_height: c_int,
    win_gravity: c_int,
}
const MIN_SIZE: (c_int, c_int) = (560, 480);

pub(super) struct Window {
    display: *mut Display,
    window: c_ulong,
    gc: *mut c_void,
    cursor: c_ulong,
    delete: c_ulong,
    visual: *mut c_void,
    depth: c_int,
    size: (usize, usize),
    pixels: Vec<u32>,
    stale: bool, // the window was resized since the last full upload
    im: *mut c_void,
    ic: *mut c_void,
    joysticks: Joysticks,
}
impl Window {
    pub fn new(title: &str, width: usize, height: usize) -> Result<Self, String> {
        let title = CString::new(title).map_err(|e| e.to_string())?;
        // SAFETY: Xlib-owned handles are checked and released by Drop; C strings live through each call.
        unsafe {
            let d = XOpenDisplay(ptr::null());
            if d.is_null() {
                return Err("Could not open X11 display. Run in a desktop session (Wayland supports XWayland).".into());
            }
            let screen = XDefaultScreen(d);
            let visual = XDefaultVisual(d, screen);
            let depth = XDefaultDepth(d, screen);
            if depth != 24 && depth != 32 {
                XCloseDisplay(d);
                return Err("A 24-bit or 32-bit desktop is required.".into());
            }
            let window = XCreateSimpleWindow(
                d,
                XRootWindow(d, screen),
                0,
                0,
                width as u32,
                height as u32,
                0,
                0,
                0,
            );
            XStoreName(d, window, title.as_ptr());
            // No server-side background: while the window is resized, the
            // old frame stays visible instead of flashing a blank fill.
            XSetWindowBackgroundPixmap(d, window, 0);
            let mut hints = SizeHints {
                flags: 16, // PMinSize
                min_width: MIN_SIZE.0,
                min_height: MIN_SIZE.1,
                ..Default::default()
            };
            XSetWMNormalHints(d, window, &mut hints);
            XSelectInput(d, window, 1 | 2 | 4 | 8 | 64 | 32768 | 131072 | 2097152);
            let mut delete = XInternAtom(d, c"WM_DELETE_WINDOW".as_ptr(), 0);
            XSetWMProtocols(d, window, &mut delete, 1);
            let icon = include_bytes!("../../assets/icon.rgba");
            let mut property = vec![256 as c_ulong, 256];
            property.extend(icon.chunks_exact(4).map(|p| {
                ((p[3] as u32) << 24 | (p[0] as u32) << 16 | (p[1] as u32) << 8 | p[2] as u32)
                    as c_ulong
            }));
            let atom = XInternAtom(d, c"_NET_WM_ICON".as_ptr(), 0);
            XChangeProperty(
                d,
                window,
                atom,
                6,
                32,
                0,
                property.as_ptr().cast(),
                property.len() as c_int,
            );
            let gc = XCreateGC(d, window, 0, ptr::null_mut());
            let cursor = XCreateFontCursor(d, 68);
            XDefineCursor(d, window, cursor);
            setlocale(0, c"".as_ptr());
            XSetLocaleModifiers(c"".as_ptr());
            let im = XOpenIM(d, ptr::null_mut(), ptr::null_mut(), ptr::null_mut());
            let ic = if im.is_null() {
                ptr::null_mut()
            } else {
                XCreateIC(
                    im,
                    c"inputStyle".as_ptr(),
                    0x408 as c_ulong,
                    c"clientWindow".as_ptr(),
                    window,
                    c"focusWindow".as_ptr(),
                    window,
                    ptr::null::<c_char>(),
                )
            };
            if !ic.is_null() {
                XSetICFocus(ic);
            }
            XMapWindow(d, window);
            XSync(d, 0);
            Ok(Self {
                display: d,
                window,
                gc,
                cursor,
                delete,
                visual,
                depth,
                size: (width, height),
                pixels: Vec::new(),
                stale: true,
                im,
                ic,
                joysticks: Joysticks::default(),
            })
        }
    }
    pub fn poll(&mut self, input: &mut Input) {
        // SAFETY: XEvent is at most 24 native longs and aligned to a native long.
        unsafe {
            for _ in 0..4096 {
                if XPending(self.display) == 0 {
                    break;
                }
                let mut event = [0 as c_long; 24];
                XNextEvent(self.display, event.as_mut_ptr().cast());
                if XFilterEvent(event.as_mut_ptr().cast(), 0) != 0 {
                    continue;
                }
                let kind = *(event.as_ptr().cast::<c_int>());
                match kind {
                    2 | 3 => {
                        let key = &mut *event.as_mut_ptr().cast::<KeyEvent>();
                        if kind == 3 && XPending(self.display) > 0 {
                            let mut next = [0 as c_long; 24];
                            XPeekEvent(self.display, next.as_mut_ptr().cast());
                            let n = &*next.as_ptr().cast::<KeyEvent>();
                            if n.kind == 2 && n.code == key.code && n.time == key.time {
                                continue;
                            }
                        }
                        let mut sym = 0;
                        let mut bytes = [0u8; 128];
                        let n = if kind == 2 && !self.ic.is_null() {
                            let mut status = 0;
                            Xutf8LookupString(
                                self.ic,
                                key,
                                bytes.as_mut_ptr().cast(),
                                bytes.len() as c_int,
                                &mut sym,
                                &mut status,
                            )
                        } else {
                            XLookupString(
                                key,
                                bytes.as_mut_ptr().cast(),
                                bytes.len() as c_int,
                                &mut sym,
                                ptr::null_mut(),
                            )
                        };
                        if let Some(k) = key_code(sym) {
                            input.key(k, kind == 2);
                        }
                        if kind == 2 && n > 0 && n <= bytes.len() as c_int && key.state & 4 == 0 {
                            if let Ok(s) = std::str::from_utf8(&bytes[..n as usize]) {
                                input.chars.extend(s.chars().filter(|c| !c.is_control()));
                            }
                        }
                    }
                    4..=6 => {
                        let e = &*event.as_ptr().cast::<KeyEvent>();
                        input.mouse = (e.x as f32, e.y as f32);
                        if kind == 4 {
                            match e.code {
                                1 => {
                                    input.clicked = true;
                                    input.left = true
                                }
                                4 => input.wheel.1 += 1.0,
                                5 => input.wheel.1 -= 1.0,
                                _ => {}
                            }
                        }
                        if kind == 5 && e.code == 1 {
                            input.left = false;
                        }
                    }
                    9 => {
                        input.focused = true;
                        if !self.ic.is_null() {
                            XSetICFocus(self.ic);
                        }
                    }
                    10 => {
                        input.blur();
                        if !self.ic.is_null() {
                            XUnsetICFocus(self.ic);
                        }
                    }
                    12 => input.repaint = true,
                    17 => {
                        input.closed = true;
                        self.window = 0;
                    }
                    22 => {
                        let e = &*event.as_ptr().cast::<ConfigureEvent>();
                        let size = (e.width.max(1) as usize, e.height.max(1) as usize);
                        if size != self.size {
                            self.size = size;
                            self.stale = true;
                            // The framebuffer stops at 4096 px and is shown 1:1 at
                            // the top left, so layout and mouse use that size too.
                            input.size = (size.0.min(4096), size.1.min(4096));
                            input.repaint = true;
                        }
                    }
                    33 => {
                        let e = &*event.as_ptr().cast::<ClientEvent>();
                        if e.data[0] as c_ulong == self.delete {
                            input.closed = true;
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    /// Buttons held on any connected controller (`gamepad` bits).
    pub fn gamepad(&mut self) -> u16 {
        self.joysticks.poll()
    }
    pub fn present(&mut self, surface: &Surface, damage: &[(usize, usize, usize, usize)]) {
        let (w, h) = (self.size.0.min(4096), self.size.1.min(4096));
        if (w, h) != (surface.width, surface.height) {
            // A frame rendered for the previous size would appear stretched;
            // keep the old picture until the next frame matches the window.
            return;
        }
        // Bound queued X requests: wait until the server has taken the last
        // frame, so a slow display server cannot fall frames behind.
        // SAFETY: the display connection is open for the window's lifetime.
        unsafe {
            XSync(self.display, 0);
        }
        self.pixels.resize(w * h, 0);
        let full = [(0, 0, w, h)];
        let damage = if std::mem::take(&mut self.stale) {
            &full
        } else {
            damage
        };
        for &(x0, y0, x1, y1) in damage {
            for y in y0..y1.min(h) {
                let src = &surface.pixels[(y * w + x0) * 4..(y * w + x1) * 4];
                let dst = &mut self.pixels[y * w + x0..y * w + x1];
                for (dst, rgba) in dst.iter_mut().zip(src.chunks_exact(4)) {
                    // Opaque alpha for 32-bit (ARGB) visuals; 24-bit ones ignore it.
                    *dst = 0xff00_0000
                        | (rgba[0] as u32) << 16
                        | (rgba[1] as u32) << 8
                        | rgba[2] as u32;
                }
            }
        }
        // SAFETY: XPutImage copies the buffer before returning. Detach Vec-owned data before XDestroyImage.
        unsafe {
            let image = XCreateImage(
                self.display,
                self.visual,
                self.depth as u32,
                2,
                0,
                self.pixels.as_mut_ptr().cast(),
                w as u32,
                h as u32,
                32,
                (w * 4) as c_int,
            );
            if !image.is_null() {
                for &(x0, y0, x1, y1) in damage {
                    XPutImage(
                        self.display,
                        self.window,
                        self.gc,
                        image,
                        x0 as c_int,
                        y0 as c_int,
                        x0 as c_int,
                        y0 as c_int,
                        (x1 - x0) as u32,
                        (y1 - y0) as u32,
                    );
                }
                (*image).data = ptr::null_mut();
                XDestroyImage(image);
                // Send now; the round trip that waits for the server happens
                // before the next frame's upload, overlapping our rendering.
                XFlush(self.display);
            }
        }
    }
}
impl Drop for Window {
    fn drop(&mut self) {
        // SAFETY: these handles were created by this Window and are destroyed once.
        unsafe {
            if !self.ic.is_null() {
                XDestroyIC(self.ic);
            }
            if !self.im.is_null() {
                XCloseIM(self.im);
            }
            XFreeCursor(self.display, self.cursor);
            XFreeGC(self.display, self.gc);
            if self.window != 0 {
                XDestroyWindow(self.display, self.window);
            }
            XCloseDisplay(self.display);
        }
    }
}
fn key_code(sym: c_ulong) -> Option<KeyCode> {
    Some(match sym {
        32 => KeyCode::Space,
        0xff1b => KeyCode::Escape,
        0xff0d | 0xff8d => KeyCode::Enter,
        0xff08 => KeyCode::Backspace,
        0xff09 | 0xfe20 => KeyCode::Tab, // Tab, ISO_Left_Tab (Shift+Tab)
        77 | 109 => KeyCode::M,
        78 | 110 => KeyCode::N,
        82 | 114 => KeyCode::R,
        86 | 118 => KeyCode::V,
        0xffe1 => KeyCode::LeftShift,
        0xffe2 => KeyCode::RightShift,
        0xffe3 => KeyCode::LeftControl,
        0xffe4 => KeyCode::RightControl,
        0xffe7..=0xffea => KeyCode::Alt, // Meta_L/R, Alt_L/R (AltGr is separate)
        0xffeb | 0xffec => KeyCode::Super,
        0xffc5 => KeyCode::F8,
        0xffc6 => KeyCode::F9,
        0xffc7 => KeyCode::F10,
        0xff51 => KeyCode::Left,
        0xff52 => KeyCode::Up,
        0xff53 => KeyCode::Right,
        0xff54 => KeyCode::Down,
        _ => return None,
    })
}
// Linux clipboard helpers in src/clipboard.rs run off the render thread.
pub(super) fn clipboard_set(_: &str) {}
pub(super) fn clipboard_get() -> Option<String> {
    None
}

/// Controllers through the kernel joystick API (/dev/input/js0..3).
#[derive(Default)]
struct Joysticks {
    devices: [Option<Joystick>; 4],
    scanned: Option<std::time::Instant>,
}
impl Joysticks {
    fn poll(&mut self) -> u16 {
        let due = match self.scanned {
            Some(t) => t.elapsed() >= std::time::Duration::from_secs(2),
            None => true,
        };
        if due {
            self.scanned = Some(std::time::Instant::now());
            for (i, slot) in self.devices.iter_mut().enumerate() {
                if slot.is_none() {
                    *slot = Joystick::open(&format!("/dev/input/js{i}"));
                }
            }
        }
        let mut held = 0;
        for slot in &mut self.devices {
            if let Some(js) = slot {
                if js.read() {
                    held |= js.state.buttons();
                } else {
                    *slot = None; // unplugged
                }
            }
        }
        held
    }
}
/// What one controller's buttons and axes mean.
struct Layout {
    /// `gamepad` bit for each joystick button number.
    buttons: Vec<u16>,
    /// Left stick x, y and d-pad hat x, y axis numbers.
    axes: [usize; 4],
}
impl Layout {
    /// The xpad (Xbox) numbering, for when the driver gives no button map.
    fn xpad() -> Self {
        use gamepad::*;
        Self {
            buttons: vec![SOUTH, EAST, WEST, NORTH, LB, RB, BACK, START],
            axes: [0, 1, 6, 7],
        }
    }
    /// From the driver's button (input key code) and axis (ABS code) maps.
    /// Label-based drivers such as xpad report X as BTN_X (= BTN_NORTH) and
    /// Y as BTN_Y (= BTN_WEST); position-based ones such as Sony's report
    /// Triangle as BTN_NORTH and Square as BTN_WEST.
    fn from_maps(keys: &[u16], abs: &[u8], positional: bool) -> Option<Self> {
        use gamepad::*;
        if !keys.contains(&0x130) {
            return None; // not a gamepad-style button set
        }
        let (x, y) = if positional {
            (0x134, 0x133)
        } else {
            (0x133, 0x134)
        };
        let buttons = keys
            .iter()
            .map(|&k| match k {
                0x130 => SOUTH,
                0x131 => EAST,
                _ if k == x => WEST,
                _ if k == y => NORTH,
                0x136 => LB,
                0x137 => RB,
                0x13a => BACK,
                0x13b => START,
                0x220 => UP,
                0x221 => DOWN,
                0x222 => LEFT,
                0x223 => RIGHT,
                _ => 0,
            })
            .collect();
        let axis =
            |code: u8, fallback: usize| abs.iter().position(|&a| a == code).unwrap_or(fallback);
        Some(Self {
            buttons,
            axes: [
                axis(0, 0),
                axis(1, 1),
                axis(0x10, usize::MAX),
                axis(0x11, usize::MAX),
            ],
        })
    }
}
struct PadState {
    layout: Layout,
    pressed: Vec<bool>,
    axes: Vec<i16>,
}
impl PadState {
    fn new(layout: Layout) -> Self {
        Self {
            layout,
            pressed: vec![false; 64],
            axes: vec![0; 64],
        }
    }
    /// Applies one 8-byte js_event: u32 time, i16 value, u8 type, u8 number.
    fn event(&mut self, e: &[u8]) {
        let value = i16::from_ne_bytes([e[4], e[5]]);
        let (kind, n) = (e[6] & !0x80, e[7] as usize); // without JS_EVENT_INIT
        match kind {
            1 if n < self.pressed.len() => self.pressed[n] = value != 0,
            2 if n < self.axes.len() => self.axes[n] = value,
            _ => {}
        }
    }
    fn buttons(&self) -> u16 {
        let mut held = 0;
        for (n, &bit) in self.layout.buttons.iter().enumerate() {
            if self.pressed.get(n) == Some(&true) {
                held |= bit;
            }
        }
        let axis = |i: usize| {
            self.axes
                .get(self.layout.axes[i])
                .map_or(0.0, |&v| v as f32 / 32767.0)
        };
        held | gamepad::stick(axis(0), axis(1)) | gamepad::stick(axis(2), axis(3))
    }
}
struct Joystick {
    file: std::fs::File,
    state: PadState,
}
impl Joystick {
    fn open(path: &str) -> Option<Self> {
        use std::os::unix::fs::OpenOptionsExt;
        use std::os::unix::io::AsRawFd;
        let file = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(0o4000) // O_NONBLOCK
            .open(path)
            .ok()?;
        let fd = file.as_raw_fd();
        // _IOC(_IOC_READ, 'j', nr, size)
        let request =
            |nr: c_ulong, size: usize| (2 << 30) | (size as c_ulong) << 16 | 0x6a << 8 | nr;
        let mut name = [0u8; 128];
        let mut keys = [0u16; 0x200];
        let mut abs = [0u8; 0x40];
        let (mut buttons, mut axes) = (0u8, 0u8);
        // SAFETY: each ioctl writes at most the size encoded in its request
        // into the buffer passed with it.
        let maps = unsafe {
            ioctl(fd, request(0x13, name.len() - 1), name.as_mut_ptr());
            ioctl(fd, request(0x12, 1), &mut buttons as *mut u8) >= 0
                && ioctl(fd, request(0x11, 1), &mut axes as *mut u8) >= 0
                && ioctl(
                    fd,
                    request(0x34, std::mem::size_of_val(&keys)),
                    keys.as_mut_ptr(),
                ) >= 0
                && ioctl(fd, request(0x32, abs.len()), abs.as_mut_ptr()) >= 0
        };
        let name = String::from_utf8_lossy(&name).to_lowercase();
        let positional = ["sony", "playstation", "dualsense", "dualshock"]
            .iter()
            .any(|v| name.contains(v));
        let layout = maps
            .then(|| {
                Layout::from_maps(
                    &keys[..(buttons as usize).min(keys.len())],
                    &abs[..(axes as usize).min(abs.len())],
                    positional,
                )
            })
            .flatten()
            .unwrap_or_else(Layout::xpad);
        Some(Self {
            file,
            state: PadState::new(layout),
        })
    }
    /// Drains pending events; false once the device is gone.
    fn read(&mut self) -> bool {
        use std::io::Read;
        let mut buffer = [0u8; 8 * 64];
        loop {
            match self.file.read(&mut buffer) {
                Ok(0) => return false,
                Ok(n) => {
                    for e in buffer[..n - n % 8].chunks_exact(8) {
                        self.state.event(e);
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => return true,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                Err(_) => return false,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::gamepad::*;
    fn event(kind: u8, number: u8, value: i16) -> [u8; 8] {
        let mut e = [0u8; 8];
        e[4..6].copy_from_slice(&value.to_ne_bytes());
        e[6] = kind;
        e[7] = number;
        e
    }
    #[test]
    fn joystick_events_become_pad_buttons() {
        let mut pad = PadState::new(Layout::xpad());
        pad.event(&event(0x81, 0, 0)); // initial state
        pad.event(&event(0x82, 1, 0));
        assert_eq!(pad.buttons(), 0);
        pad.event(&event(1, 0, 1));
        pad.event(&event(1, 7, 1));
        assert_eq!(pad.buttons(), SOUTH | START);
        pad.event(&event(1, 0, 0));
        pad.event(&event(1, 7, 0));
        pad.event(&event(2, 1, -30000)); // stick up
        assert_eq!(pad.buttons(), UP);
        pad.event(&event(2, 1, 0));
        pad.event(&event(2, 6, 32767)); // d-pad right
        assert_eq!(pad.buttons(), RIGHT);
        pad.event(&event(1, 200, 1)); // out of range: ignored
        assert_eq!(pad.buttons(), RIGHT);
    }
    #[test]
    fn button_maps_follow_the_driver() {
        let abs = [0, 1, 2, 3, 4, 5, 0x10, 0x11];
        // xpad: A B X Y LB RB Back Start Guide LS RS
        let xpad = [
            0x130, 0x131, 0x133, 0x134, 0x136, 0x137, 0x13a, 0x13b, 0x13c, 0x13d, 0x13e,
        ];
        let l = Layout::from_maps(&xpad, &abs, false).unwrap();
        assert_eq!(&l.buttons[..8], &Layout::xpad().buttons[..]);
        assert_eq!(l.axes, [0, 1, 6, 7]);
        // hid-playstation: Cross Circle Triangle Square L1 R1 L2 R2 Share Options PS L3 R3
        let sony = [
            0x130, 0x131, 0x133, 0x134, 0x136, 0x137, 0x138, 0x139, 0x13a, 0x13b, 0x13c, 0x13d,
            0x13e,
        ];
        let l = Layout::from_maps(&sony, &abs, true).unwrap();
        assert_eq!(
            &l.buttons[..10],
            &[SOUTH, EAST, NORTH, WEST, LB, RB, 0, 0, BACK, START]
        );
        // a plain joystick (BTN_TRIGGER, ...) falls back to the xpad numbering
        assert!(Layout::from_maps(&[0x120, 0x121], &abs, false).is_none());
    }
}
