//! Win32/GDI transport for the same software renderer used on Linux.
use super::{raster::Surface, Input, KeyCode};
use std::ffi::c_void;
use std::ptr;
type Handle = *mut c_void;
#[repr(C)]
struct Point {
    x: i32,
    y: i32,
}
#[repr(C)]
struct Rect {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
}
#[repr(C)]
struct Message {
    window: Handle,
    kind: u32,
    wparam: usize,
    lparam: isize,
    time: u32,
    point: Point,
    private: u32,
}
#[repr(C)]
struct Class {
    style: u32,
    proc: Option<unsafe extern "system" fn(Handle, u32, usize, isize) -> isize>,
    class_extra: i32,
    window_extra: i32,
    instance: Handle,
    icon: Handle,
    cursor: Handle,
    background: Handle,
    menu: *const u16,
    name: *const u16,
}
#[repr(C)]
struct BitmapInfo {
    size: u32,
    width: i32,
    height: i32,
    planes: u16,
    bits: u16,
    compression: u32,
    image_size: u32,
    xppm: i32,
    yppm: i32,
    used: u32,
    important: u32,
    color: u32,
}
#[link(name = "user32")]
extern "system" {
    fn SetProcessDPIAware() -> i32;
    fn RegisterClassW(class: *const Class) -> u16;
    fn UnregisterClassW(name: *const u16, instance: Handle) -> i32;
    fn CreateWindowExW(
        ex: u32,
        class: *const u16,
        name: *const u16,
        style: u32,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        parent: Handle,
        menu: Handle,
        instance: Handle,
        param: *mut c_void,
    ) -> Handle;
    fn DefWindowProcW(window: Handle, kind: u32, wparam: usize, lparam: isize) -> isize;
    fn DestroyWindow(window: Handle) -> i32;
    fn ShowWindow(window: Handle, show: i32) -> i32;
    fn PeekMessageW(message: *mut Message, window: Handle, low: u32, high: u32, remove: u32)
        -> i32;
    fn TranslateMessage(message: *const Message) -> i32;
    fn DispatchMessageW(message: *const Message) -> isize;
    fn GetClientRect(window: Handle, rect: *mut Rect) -> i32;
    fn AdjustWindowRect(rect: *mut Rect, style: u32, menu: i32) -> i32;
    fn GetDC(window: Handle) -> Handle;
    fn ReleaseDC(window: Handle, dc: Handle) -> i32;
    fn LoadCursorW(instance: Handle, name: *const u16) -> Handle;
    fn CreateIcon(
        instance: Handle,
        w: i32,
        h: i32,
        planes: u8,
        bits: u8,
        and: *const u8,
        xor: *const u8,
    ) -> Handle;
    fn DestroyIcon(icon: Handle) -> i32;
    fn SetCapture(window: Handle) -> Handle;
    fn ReleaseCapture() -> i32;
    fn GetKeyState(key: i32) -> i16;
    fn OpenClipboard(window: Handle) -> i32;
    fn CloseClipboard() -> i32;
    fn EmptyClipboard() -> i32;
    fn SetClipboardData(format: u32, data: Handle) -> Handle;
    fn GetClipboardData(format: u32) -> Handle;
}
#[link(name = "gdi32")]
extern "system" {
    fn StretchDIBits(
        dc: Handle,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        sx: i32,
        sy: i32,
        sw: i32,
        sh: i32,
        pixels: *const c_void,
        info: *const BitmapInfo,
        usage: u32,
        operation: u32,
    ) -> i32;
}
#[link(name = "kernel32")]
extern "system" {
    fn GetModuleHandleW(name: *const u16) -> Handle;
    fn GlobalAlloc(flags: u32, bytes: usize) -> Handle;
    fn GlobalLock(memory: Handle) -> *mut c_void;
    fn GlobalUnlock(memory: Handle) -> i32;
    fn GlobalFree(memory: Handle) -> Handle;
    fn GlobalSize(memory: Handle) -> usize;
}
thread_local! {static HWND:std::cell::Cell<Handle>=const{std::cell::Cell::new(ptr::null_mut())};}
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}
unsafe extern "system" fn window_proc(w: Handle, k: u32, wp: usize, lp: isize) -> isize {
    unsafe { DefWindowProcW(w, k, wp, lp) }
}
pub(super) struct Window {
    window: Handle,
    instance: Handle,
    class: Vec<u16>,
    icon: Handle,
    pixels: Vec<u32>,
    surrogate: Option<u16>,
}
impl Window {
    pub fn new(title: &str, width: usize, height: usize) -> Result<Self, String> {
        let class = wide("DiceWarsSoftwareWindow");
        let title = wide(title);
        // SAFETY: all ABI structs match Win32; handles are owned until Drop.
        unsafe {
            SetProcessDPIAware();
            let instance = GetModuleHandleW(ptr::null());
            let raw = include_bytes!("../../assets/icon.rgba");
            let mut xor = vec![0u8; 64 * 64 * 4];
            let mut and = vec![0u8; 64 * 64 / 8];
            for y in 0..64 {
                for x in 0..64 {
                    let p = (y * 4 * 256 + x * 4) * 4;
                    let i = ((63 - y) * 64 + x) * 4;
                    let a = raw[p + 3] as u16;
                    xor[i..i + 4].copy_from_slice(&[
                        (raw[p + 2] as u16 * a / 255) as u8,
                        (raw[p + 1] as u16 * a / 255) as u8,
                        (raw[p] as u16 * a / 255) as u8,
                        a as u8,
                    ]);
                    if a == 0 {
                        and[(63 - y) * 8 + x / 8] |= 0x80 >> (x % 8);
                    }
                }
            }
            let icon = CreateIcon(instance, 64, 64, 1, 32, and.as_ptr(), xor.as_ptr());
            let c = Class {
                style: 0,
                proc: Some(window_proc),
                class_extra: 0,
                window_extra: 0,
                instance,
                icon,
                cursor: LoadCursorW(ptr::null_mut(), 32512usize as *const u16),
                background: ptr::null_mut(),
                menu: ptr::null(),
                name: class.as_ptr(),
            };
            if RegisterClassW(&c) == 0 {
                if !icon.is_null() {
                    DestroyIcon(icon);
                }
                return Err("Could not register game window".into());
            }
            let style = 0x00cf0000;
            let mut rect = Rect {
                left: 0,
                top: 0,
                right: width as i32,
                bottom: height as i32,
            };
            AdjustWindowRect(&mut rect, style, 0);
            let window = CreateWindowExW(
                0,
                class.as_ptr(),
                title.as_ptr(),
                style,
                i32::MIN,
                i32::MIN,
                rect.right - rect.left,
                rect.bottom - rect.top,
                ptr::null_mut(),
                ptr::null_mut(),
                instance,
                ptr::null_mut(),
            );
            if window.is_null() {
                UnregisterClassW(class.as_ptr(), instance);
                if !icon.is_null() {
                    DestroyIcon(icon);
                }
                return Err("Could not create game window".into());
            }
            HWND.with(|h| h.set(window));
            ShowWindow(window, 5);
            Ok(Self {
                window,
                instance,
                class,
                icon,
                pixels: Vec::new(),
                surrogate: None,
            })
        }
    }
    pub fn poll(&mut self, input: &mut Input) {
        // SAFETY: GetClientRect and PeekMessage write exactly the provided structs.
        unsafe {
            let mut rect = Rect {
                left: 0,
                top: 0,
                right: 0,
                bottom: 0,
            };
            GetClientRect(self.window, &mut rect);
            input.size = (rect.right.max(1) as usize, rect.bottom.max(1) as usize);
            for _ in 0..4096 {
                let mut m: Message = std::mem::zeroed();
                if PeekMessageW(&mut m, ptr::null_mut(), 0, 0, 1) == 0 {
                    break;
                }
                let x = (m.lparam as u16 as i16) as f32;
                let y = ((m.lparam >> 16) as u16 as i16) as f32;
                match m.kind {
                    0x10 | 0x12 => {
                        input.closed = true;
                        continue;
                    }
                    7 => input.focused = true,
                    8 => input.blur(),
                    0x0f => input.repaint = true,
                    0x100 | 0x104 | 0x101 | 0x105 => {
                        let down = m.kind == 0x100 || m.kind == 0x104;
                        if let Some(k) = key_code(m.wparam) {
                            input.key(k, down);
                        }
                        if m.wparam == 0x11 {
                            input.key(KeyCode::LeftControl, GetKeyState(0xa2) < 0);
                            input.key(KeyCode::RightControl, GetKeyState(0xa3) < 0);
                        }
                    }
                    0x102 => {
                        let ch = m.wparam as u16;
                        if (0xd800..=0xdbff).contains(&ch) {
                            self.surrogate = Some(ch);
                        } else {
                            let cp = if (0xdc00..=0xdfff).contains(&ch) {
                                self.surrogate.take().map(|high| {
                                    0x10000 + ((high as u32 - 0xd800) << 10) + (ch as u32 - 0xdc00)
                                })
                            } else {
                                self.surrogate = None;
                                Some(ch as u32)
                            };
                            if let Some(c) = cp.and_then(char::from_u32).filter(|c| !c.is_control())
                            {
                                input.chars.push_back(c);
                            }
                        }
                    }
                    0x200 => input.mouse = (x, y),
                    0x201 => {
                        input.mouse = (x, y);
                        input.left = true;
                        input.clicked = true;
                        SetCapture(self.window);
                    }
                    0x202 => {
                        input.mouse = (x, y);
                        input.left = false;
                        ReleaseCapture();
                    }
                    0x20a => input.wheel.1 += ((m.wparam >> 16) as u16 as i16) as f32 / 120.0,
                    _ => {}
                }
                TranslateMessage(&m);
                DispatchMessageW(&m);
            }
        }
    }
    pub fn present(&mut self, surface: &Surface, _damage: &[(usize, usize, usize, usize)]) {
        self.pixels.resize(surface.width * surface.height, 0);
        for (pixel, rgba) in self.pixels.iter_mut().zip(surface.pixels.chunks_exact(4)) {
            *pixel = (rgba[0] as u32) << 16 | (rgba[1] as u32) << 8 | rgba[2] as u32;
        }
        let info = BitmapInfo {
            size: 40,
            width: surface.width as i32,
            height: -(surface.height as i32),
            planes: 1,
            bits: 32,
            compression: 0,
            image_size: 0,
            xppm: 0,
            yppm: 0,
            used: 0,
            important: 0,
            color: 0,
        };
        // SAFETY: the pixel array and bitmap header live through the synchronous blit.
        unsafe {
            let mut rect = Rect {
                left: 0,
                top: 0,
                right: 0,
                bottom: 0,
            };
            GetClientRect(self.window, &mut rect);
            let dc = GetDC(self.window);
            if !dc.is_null() {
                StretchDIBits(
                    dc,
                    0,
                    0,
                    rect.right,
                    rect.bottom,
                    0,
                    0,
                    surface.width as i32,
                    surface.height as i32,
                    self.pixels.as_ptr().cast(),
                    &info,
                    0,
                    0x00cc0020,
                );
                ReleaseDC(self.window, dc);
            }
        }
    }
}
impl Drop for Window {
    fn drop(&mut self) {
        HWND.with(|h| h.set(ptr::null_mut()));
        unsafe {
            DestroyWindow(self.window);
            UnregisterClassW(self.class.as_ptr(), self.instance);
            if !self.icon.is_null() {
                DestroyIcon(self.icon);
            }
        }
    }
}
fn key_code(v: usize) -> Option<KeyCode> {
    Some(match v {
        0x20 => KeyCode::Space,
        0x1b => KeyCode::Escape,
        0x0d => KeyCode::Enter,
        8 => KeyCode::Backspace,
        9 => KeyCode::Tab,
        0x4d => KeyCode::M,
        0x4e => KeyCode::N,
        0x52 => KeyCode::R,
        0x56 => KeyCode::V,
        0xa2 => KeyCode::LeftControl,
        0xa3 => KeyCode::RightControl,
        0x77 => KeyCode::F8,
        0x78 => KeyCode::F9,
        0x79 => KeyCode::F10,
        0x25 => KeyCode::Left,
        0x26 => KeyCode::Up,
        0x27 => KeyCode::Right,
        0x28 => KeyCode::Down,
        _ => return None,
    })
}
pub(super) fn clipboard_set(text: &str) {
    let text = wide(text);
    unsafe {
        if OpenClipboard(HWND.with(|h| h.get())) == 0 {
            return;
        }
        let memory = GlobalAlloc(0x42, text.len() * 2);
        if !memory.is_null() {
            let data = GlobalLock(memory);
            if !data.is_null() {
                ptr::copy_nonoverlapping(text.as_ptr(), data.cast(), text.len());
                GlobalUnlock(memory);
                EmptyClipboard();
                if SetClipboardData(13, memory).is_null() {
                    GlobalFree(memory);
                }
            } else {
                GlobalFree(memory);
            }
        }
        CloseClipboard();
    }
}
pub(super) fn clipboard_get() -> Option<String> {
    unsafe {
        if OpenClipboard(HWND.with(|h| h.get())) == 0 {
            return None;
        }
        let memory = GetClipboardData(13);
        let mut result = None;
        if !memory.is_null() {
            let size = GlobalSize(memory).min(65536) / 2;
            let data = GlobalLock(memory);
            if !data.is_null() {
                let units = std::slice::from_raw_parts(data.cast::<u16>(), size);
                let end = units.iter().position(|&c| c == 0).unwrap_or(size);
                result = Some(String::from_utf16_lossy(&units[..end]));
                GlobalUnlock(memory);
            }
        }
        CloseClipboard();
        result
    }
}
