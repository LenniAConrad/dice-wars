//! Win32/GDI transport for the same software renderer used on Linux.
use super::{gamepad, raster::Surface, Input, KeyCode};
use std::cell::{Cell, RefCell};
use std::ffi::c_void;
use std::ptr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
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
struct PaintStruct {
    dc: Handle,
    erase: i32,
    paint: Rect,
    restore: i32,
    inc_update: i32,
    reserved: [u8; 32],
}
#[repr(C)]
#[derive(Default)]
struct XInputState {
    packet: u32,
    buttons: u16,
    left_trigger: u8,
    right_trigger: u8,
    thumb_lx: i16,
    thumb_ly: i16,
    thumb_rx: i16,
    thumb_ry: i16,
}
type XInputGetState = unsafe extern "system" fn(u32, *mut XInputState) -> u32;
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
    fn MessageBoxW(window: Handle, text: *const u16, caption: *const u16, kind: u32) -> i32;
    fn BeginPaint(window: Handle, paint: *mut PaintStruct) -> Handle;
    fn EndPaint(window: Handle, paint: *const PaintStruct) -> i32;
    fn InvalidateRect(window: Handle, rect: *const Rect, erase: i32) -> i32;
    fn SetTimer(window: Handle, id: usize, ms: u32, proc: *const c_void) -> usize;
    fn KillTimer(window: Handle, id: usize) -> i32;
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
    fn SetStretchBltMode(dc: Handle, mode: i32) -> i32;
    fn PatBlt(dc: Handle, x: i32, y: i32, w: i32, h: i32, operation: u32) -> i32;
}
#[link(name = "kernel32")]
extern "system" {
    fn GetModuleHandleW(name: *const u16) -> Handle;
    fn GlobalAlloc(flags: u32, bytes: usize) -> Handle;
    fn GlobalLock(memory: Handle) -> *mut c_void;
    fn GlobalUnlock(memory: Handle) -> i32;
    fn GlobalFree(memory: Handle) -> Handle;
    fn GlobalSize(memory: Handle) -> usize;
    fn LoadLibraryW(name: *const u16) -> Handle;
    fn GetProcAddress(module: Handle, name: *const u8) -> *const c_void;
}
thread_local! {static HWND:std::cell::Cell<Handle>=const{std::cell::Cell::new(ptr::null_mut())};}
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}
// Sent messages (close, focus) reach window_proc directly instead of the
// PeekMessage loop; window_proc records them here and poll applies them.
static CLOSE_REQUESTED: AtomicBool = AtomicBool::new(false);
static FOCUSED: AtomicBool = AtomicBool::new(false);
static FOCUS_LOST: AtomicBool = AtomicBool::new(false);

/// The last presented frame as 0x00RRGGBB words. Besides `present`, the
/// window procedure paints it (stretched to the client area) whenever the
/// game loop cannot: WM_PAINT, and while Windows runs its modal move/size
/// loop inside DispatchMessage, where `next_frame` is blocked.
#[derive(Default)]
struct Frame {
    pixels: Vec<u32>,
    width: usize,
    height: usize,
}
thread_local! {
    static FRAME: RefCell<Frame> = RefCell::new(Frame::default());
    // the client size the frame was last painted at
    static PAINTED: Cell<(i32, i32)> = const { Cell::new((0, 0)) };
}
const SIZE_MOVE_TIMER: usize = 0x4457;
fn client_size(window: Handle) -> (i32, i32) {
    let mut rect = Rect {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };
    // SAFETY: GetClientRect writes exactly one RECT.
    unsafe { GetClientRect(window, &mut rect) };
    (rect.right, rect.bottom)
}
/// Shows the last frame on `dc`, stretched to the client area; black before
/// the first frame. Never re-enters the game or the engine runtime.
unsafe fn paint_frame(window: Handle, dc: Handle) {
    let (w, h) = client_size(window);
    let painted = FRAME.with(|f| {
        // `present` holds the frame only while it converts and blits, and
        // neither sends messages; skip a repaint rather than ever panic.
        let Ok(f) = f.try_borrow() else { return true };
        if f.width == 0 || f.height == 0 || f.pixels.len() != f.width * f.height {
            return false;
        }
        unsafe { blit(dc, (w, h), &f) };
        true
    });
    if !painted {
        unsafe { PatBlt(dc, 0, 0, w, h, 0x0042_0021) }; // BLACKNESS
    }
    PAINTED.with(|p| p.set((w, h)));
}
unsafe fn blit(dc: Handle, (w, h): (i32, i32), frame: &Frame) {
    let info = BitmapInfo {
        size: 40,
        width: frame.width as i32,
        height: -(frame.height as i32),
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
    // SAFETY: the pixels and header outlive the synchronous blit.
    unsafe {
        if (w, h) != (frame.width as i32, frame.height as i32) {
            SetStretchBltMode(dc, 3); // COLORONCOLOR: no darkening when shrinking
        }
        StretchDIBits(
            dc,
            0,
            0,
            w,
            h,
            0,
            0,
            frame.width as i32,
            frame.height as i32,
            frame.pixels.as_ptr().cast(),
            &info,
            0,
            0x00cc0020,
        );
    }
}
unsafe extern "system" fn window_proc(w: Handle, k: u32, wp: usize, lp: isize) -> isize {
    match k {
        // WM_PAINT: the game loop also sees posted paints and presents a
        // fresh full frame; until then (and during a modal move/size loop)
        // the last frame fills the window instead of garbage or black.
        0x0f => {
            // SAFETY: BeginPaint fills the PAINTSTRUCT that EndPaint takes back.
            unsafe {
                let mut ps: PaintStruct = std::mem::zeroed();
                let dc = BeginPaint(w, &mut ps);
                if !dc.is_null() {
                    paint_frame(w, dc);
                }
                EndPaint(w, &ps);
            }
            return 0;
        }
        // WM_ERASEBKGND: every paint covers the whole client area.
        0x14 => return 1,
        // WM_SIZE: the stretched frame must cover the new size at once.
        0x05 => unsafe {
            InvalidateRect(w, ptr::null(), 0);
        },
        // WM_ENTERSIZEMOVE / WM_EXITSIZEMOVE: Windows runs a modal loop
        // until the drag ends. A ~60 Hz timer keeps the picture matched to
        // the window there; the game itself resumes after the drag.
        0x231 => unsafe {
            SetTimer(w, SIZE_MOVE_TIMER, 16, ptr::null());
        },
        0x232 => unsafe {
            KillTimer(w, SIZE_MOVE_TIMER);
        },
        // WM_TIMER
        0x113 if wp == SIZE_MOVE_TIMER => {
            if PAINTED.with(|p| p.get()) != client_size(w) {
                // SAFETY: the window DC is released right after the paint.
                unsafe {
                    let dc = GetDC(w);
                    if !dc.is_null() {
                        paint_frame(w, dc);
                        ReleaseDC(w, dc);
                    }
                }
            }
            return 0;
        }
        // WM_CLOSE (close button, Alt+F4, taskbar): end the game loop, which
        // then destroys the window in Drop. DefWindowProc would destroy it now.
        0x10 => {
            CLOSE_REQUESTED.store(true, Ordering::Relaxed);
            return 0;
        }
        // WM_DESTROY: the window is gone, so the loop must end too.
        0x02 => {
            CLOSE_REQUESTED.store(true, Ordering::Relaxed);
            return 0;
        }
        // WM_SETFOCUS / WM_KILLFOCUS
        0x07 => FOCUSED.store(true, Ordering::Relaxed),
        0x08 => {
            FOCUSED.store(false, Ordering::Relaxed);
            FOCUS_LOST.store(true, Ordering::Relaxed);
        }
        // WM_SYSCOMMAND SC_KEYMENU from the keyboard alone (F10 or a lone Alt):
        // there is no menu bar, and the modal menu loop would freeze the game.
        // Alt+Space (lparam = ' ') still opens the system menu.
        0x112 if wp & 0xfff0 == 0xf100 && lp == 0 => return 0,
        _ => {}
    }
    // WM_GETMINMAXINFO: keep the window large enough for a readable layout.
    // MINMAXINFO.ptMinTrackSize sits after three POINTs; sizes include the
    // frame, so add a typical border and caption to the 560x480 client area.
    if k == 0x24 && lp != 0 {
        // SAFETY: Windows passes a valid, writable MINMAXINFO for this message.
        unsafe {
            let min = (lp as *mut i32).add(6);
            *min = 576;
            *min.add(1) = 519;
        }
        return 0;
    }
    unsafe { DefWindowProcW(w, k, wp, lp) }
}
pub(super) struct Window {
    window: Handle,
    instance: Handle,
    class: Vec<u16>,
    icon: Handle,
    surrogate: Option<u16>,
    xinput: XInput,
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
                    // CreateIcon builds device-dependent bitmaps: rows are top-down.
                    let i = (y * 64 + x) * 4;
                    let a = raw[p + 3] as u16;
                    xor[i..i + 4].copy_from_slice(&[
                        (raw[p + 2] as u16 * a / 255) as u8,
                        (raw[p + 1] as u16 * a / 255) as u8,
                        (raw[p] as u16 * a / 255) as u8,
                        a as u8,
                    ]);
                    if a == 0 {
                        and[y * 8 + x / 8] |= 0x80 >> (x % 8);
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
            FOCUSED.store(false, Ordering::Relaxed);
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
            CLOSE_REQUESTED.store(false, Ordering::Relaxed);
            FOCUS_LOST.store(false, Ordering::Relaxed);
            ShowWindow(window, 5);
            Ok(Self {
                window,
                instance,
                class,
                icon,
                surrogate: None,
                xinput: XInput::default(),
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
                        if m.wparam == 0x10 {
                            input.key(KeyCode::LeftShift, GetKeyState(0xa0) < 0);
                            input.key(KeyCode::RightShift, GetKeyState(0xa1) < 0);
                        }
                        // F10 arrives as WM_SYSKEYDOWN/UP; DefWindowProc would open
                        // the window menu. It is the game's "next theme" key.
                        if m.wparam == 0x79 {
                            continue;
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
            if FOCUS_LOST.swap(false, Ordering::Relaxed) {
                input.blur();
            }
            input.focused = FOCUSED.load(Ordering::Relaxed);
            if CLOSE_REQUESTED.load(Ordering::Relaxed) {
                input.closed = true;
            }
        }
    }
    /// Buttons held on any connected XInput controller (`gamepad` bits).
    pub fn gamepad(&mut self) -> u16 {
        self.xinput.poll()
    }
    pub fn present(&mut self, surface: &Surface, damage: &[(usize, usize, usize, usize)]) {
        let window = self.window;
        FRAME.with(|frame| {
            let Ok(mut frame) = frame.try_borrow_mut() else {
                return;
            };
            // Only damaged rectangles change; convert those (everything after
            // a size change) and keep the rest of the last frame.
            let (w, h) = (surface.width, surface.height);
            let all = [(0, 0, w, h)];
            let regions = if (frame.width, frame.height) != (w, h) || frame.pixels.len() != w * h {
                frame.pixels.clear();
                frame.pixels.resize(w * h, 0);
                frame.width = w;
                frame.height = h;
                &all[..]
            } else {
                damage
            };
            for &(x0, y0, x1, y1) in regions {
                let (x1, y1) = (x1.min(w), y1.min(h));
                for y in y0..y1 {
                    let src = &surface.pixels[(y * w + x0) * 4..(y * w + x1) * 4];
                    for (dst, p) in frame.pixels[y * w + x0..y * w + x1]
                        .iter_mut()
                        .zip(src.chunks_exact(4))
                    {
                        *dst = (p[0] as u32) << 16 | (p[1] as u32) << 8 | p[2] as u32;
                    }
                }
            }
            // SAFETY: the window DC is released right after the blit.
            unsafe {
                let dc = GetDC(window);
                if !dc.is_null() {
                    let size = client_size(window);
                    blit(dc, size, &frame);
                    PAINTED.with(|p| p.set(size));
                    ReleaseDC(window, dc);
                }
            }
        });
    }
}
impl Drop for Window {
    fn drop(&mut self) {
        HWND.with(|h| h.set(ptr::null_mut()));
        unsafe {
            KillTimer(self.window, SIZE_MOVE_TIMER);
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
        0xa0 => KeyCode::LeftShift,
        0xa1 => KeyCode::RightShift,
        0xa2 => KeyCode::LeftControl,
        0xa3 => KeyCode::RightControl,
        0x77 => KeyCode::F8,
        0x78 => KeyCode::F9,
        0x79 => KeyCode::F10,
        0x12 => KeyCode::Alt,
        0x5b | 0x5c => KeyCode::Super,
        0x25 => KeyCode::Left,
        0x26 => KeyCode::Up,
        0x27 => KeyCode::Right,
        0x28 => KeyCode::Down,
        _ => return None,
    })
}
/// XInput controllers 0..3, loaded at run time (xinput1_4.dll, Windows 8+,
/// or xinput9_1_0.dll). Asking about an empty slot can stall for a while, so
/// slots without a controller are only probed every two seconds.
#[derive(Default)]
struct XInput {
    get_state: Option<Option<XInputGetState>>,
    connected: [bool; 4],
    probed: Option<Instant>,
}
impl XInput {
    fn poll(&mut self) -> u16 {
        let get_state = *self.get_state.get_or_insert_with(|| {
            // SAFETY: the library stays loaded for the process; the symbol
            // has XInputGetState's documented signature.
            unsafe {
                ["xinput1_4.dll", "xinput9_1_0.dll"]
                    .iter()
                    .map(|name| LoadLibraryW(wide(name).as_ptr()))
                    .find(|module| !module.is_null())
                    .and_then(|module| {
                        let f = GetProcAddress(module, c"XInputGetState".as_ptr().cast());
                        (!f.is_null())
                            .then(|| std::mem::transmute::<*const c_void, XInputGetState>(f))
                    })
            }
        });
        let Some(get_state) = get_state else {
            return 0;
        };
        let probe = self
            .probed
            .is_none_or(|t| t.elapsed() >= Duration::from_secs(2));
        if probe {
            self.probed = Some(Instant::now());
        }
        let mut held = 0;
        for (i, connected) in self.connected.iter_mut().enumerate() {
            if !*connected && !probe {
                continue;
            }
            let mut state = XInputState::default();
            // SAFETY: XInputGetState writes one XINPUT_STATE.
            *connected = unsafe { get_state(i as u32, &mut state) } == 0;
            if *connected {
                held |= xinput_buttons(&state);
            }
        }
        held
    }
}
fn xinput_buttons(state: &XInputState) -> u16 {
    let b = state.buttons;
    let mut held = 0;
    for (mask, bit) in [
        (0x0001, gamepad::UP),
        (0x0002, gamepad::DOWN),
        (0x0004, gamepad::LEFT),
        (0x0008, gamepad::RIGHT),
        (0x0010, gamepad::START),
        (0x0020, gamepad::BACK),
        (0x0100, gamepad::LB),
        (0x0200, gamepad::RB),
        (0x1000, gamepad::SOUTH),
        (0x2000, gamepad::EAST),
        (0x4000, gamepad::WEST),
        (0x8000, gamepad::NORTH),
    ] {
        if b & mask != 0 {
            held |= bit;
        }
    }
    // XInput's stick y points up.
    held | gamepad::stick(
        state.thumb_lx as f32 / 32767.0,
        -(state.thumb_ly as f32) / 32767.0,
    )
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
/// A blocking error dialog: release builds have no console for stderr.
pub(super) fn show_error(title: &str, message: &str) {
    let (title, message) = (wide(title), wide(message));
    // SAFETY: both strings are NUL-terminated and outlive the call.
    unsafe {
        // MB_OK | MB_ICONERROR | MB_SETFOREGROUND
        MessageBoxW(
            ptr::null_mut(),
            message.as_ptr(),
            title.as_ptr(),
            0x10 | 0x10000,
        );
    }
}
