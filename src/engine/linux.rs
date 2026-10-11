//! X11 transport for the software framebuffer. All Xlib calls stay on the UI thread.
use super::{raster::Surface, Input, KeyCode};
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
}
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
    im: *mut c_void,
    ic: *mut c_void,
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
                im,
                ic,
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
                    4 | 5 | 6 => {
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
                        self.size = (e.width.max(1) as usize, e.height.max(1) as usize);
                        input.size = self.size;
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
    pub fn present(&mut self, surface: &Surface, damage: &[(usize, usize, usize, usize)]) {
        let (w, h) = (self.size.0.min(4096), self.size.1.min(4096));
        self.pixels.resize(w * h, 0);
        let fallback = [(0, 0, w, h)];
        let damage = if (w, h) == (surface.width, surface.height) {
            damage
        } else {
            &fallback
        };
        for &(x0, y0, x1, y1) in damage {
            for y in y0..y1.min(h) {
                if (w, h) == (surface.width, surface.height) {
                    // Avoid integer scaling divisions on every native pixel.
                    let src = &surface.pixels[(y * w + x0) * 4..(y * w + x1) * 4];
                    let dst = &mut self.pixels[y * w + x0..y * w + x1];
                    for (dst, rgba) in dst.iter_mut().zip(src.chunks_exact(4)) {
                        *dst = (rgba[0] as u32) << 16 | (rgba[1] as u32) << 8 | rgba[2] as u32;
                    }
                } else {
                    for x in x0..x1.min(w) {
                        let p =
                            (y * surface.height / h * surface.width + x * surface.width / w) * 4;
                        self.pixels[y * w + x] = (surface.pixels[p] as u32) << 16
                            | (surface.pixels[p + 1] as u32) << 8
                            | surface.pixels[p + 2] as u32;
                    }
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
                // Bound queued X requests: a slow display server cannot accumulate frames indefinitely.
                XSync(self.display, 0);
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
        0xff09 => KeyCode::Tab,
        77 | 109 => KeyCode::M,
        78 | 110 => KeyCode::N,
        82 | 114 => KeyCode::R,
        86 | 118 => KeyCode::V,
        0xffe3 => KeyCode::LeftControl,
        0xffe4 => KeyCode::RightControl,
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
// Linux clipboard helpers in main.rs run off the render thread.
pub(super) fn clipboard_set(_: &str) {}
pub(super) fn clipboard_get() -> Option<String> {
    None
}
