//! Cocoa transport for the software framebuffer. AppKit calls stay on the main thread.
//!
//! The Objective-C runtime is called directly: every `objc_msgSend` goes through a
//! function pointer with the exact argument and return types of the method, so the
//! arm64 and x86_64 calling conventions are both honoured. Frames are shown by
//! handing a CGImage to the content view's hosted CALayer; AppKit never draws it.
use super::{raster::Surface, Input, KeyCode};
use std::ffi::{c_char, c_void, CStr};
use std::ptr;
use std::sync::atomic::{AtomicBool, Ordering};

type Id = *mut c_void;
type Sel = *const c_void;

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct NSPoint {
    x: f64,
    y: f64,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct NSSize {
    width: f64,
    height: f64,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct NSRect {
    origin: NSPoint,
    size: NSSize,
}

#[link(name = "objc")]
extern "C" {
    fn objc_getClass(name: *const c_char) -> Id;
    fn sel_registerName(name: *const c_char) -> Sel;
    fn objc_msgSend();
    #[cfg(target_arch = "x86_64")]
    fn objc_msgSend_stret();
    fn objc_allocateClassPair(superclass: Id, name: *const c_char, extra: usize) -> Id;
    fn objc_registerClassPair(class: Id);
    fn class_addMethod(class: Id, name: Sel, imp: *const c_void, types: *const c_char) -> i8;
    fn objc_autoreleasePoolPush() -> *mut c_void;
    fn objc_autoreleasePoolPop(pool: *mut c_void);
}
#[link(name = "Foundation", kind = "framework")]
extern "C" {
    static NSDefaultRunLoopMode: Id;
}
#[link(name = "AppKit", kind = "framework")]
extern "C" {
    static NSPasteboardTypeString: Id;
    static NSModalPanelRunLoopMode: Id;
}
#[link(name = "QuartzCore", kind = "framework")]
extern "C" {
    static kCAGravityResize: Id;
}
#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    static kCGColorSpaceSRGB: *const c_void;
    fn CGColorSpaceCreateWithName(name: *const c_void) -> *mut c_void;
    fn CGColorSpaceRelease(space: *mut c_void);
    fn CGDataProviderCreateWithData(
        info: *mut c_void,
        data: *const c_void,
        size: usize,
        release: Option<unsafe extern "C" fn(*mut c_void, *const c_void, usize)>,
    ) -> *mut c_void;
    fn CGDataProviderRelease(provider: *mut c_void);
    fn CGImageCreate(
        width: usize,
        height: usize,
        bits_per_component: usize,
        bits_per_pixel: usize,
        bytes_per_row: usize,
        space: *mut c_void,
        bitmap_info: u32,
        provider: *mut c_void,
        decode: *const f64,
        interpolate: bool,
        intent: i32,
    ) -> *mut c_void;
    fn CGImageRelease(image: *mut c_void);
}

/// `objc_msgSend` with a concrete signature: `send!(obj, c"sel:", arg => Type; Ret)`.
macro_rules! send {
    ($obj:expr, $sel:expr $(, $arg:expr => $ty:ty)* ; $ret:ty) => {{
        let f: unsafe extern "C" fn(Id, Sel $(, $ty)*) -> $ret =
            std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
        f($obj, sel($sel) $(, $arg)*)
    }};
}

unsafe fn sel(name: &CStr) -> Sel {
    sel_registerName(name.as_ptr())
}
unsafe fn class(name: &CStr) -> Id {
    objc_getClass(name.as_ptr())
}
/// Methods returning NSRect use the indirect-result entry point on x86_64 only.
unsafe fn send_rect(obj: Id, name: &CStr) -> NSRect {
    #[cfg(target_arch = "x86_64")]
    let f: unsafe extern "C" fn(Id, Sel) -> NSRect =
        std::mem::transmute(objc_msgSend_stret as unsafe extern "C" fn());
    #[cfg(not(target_arch = "x86_64"))]
    let f: unsafe extern "C" fn(Id, Sel) -> NSRect =
        std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
    f(obj, sel(name))
}
/// An autoreleased NSString copy of `s`.
unsafe fn ns_string(s: &str) -> Id {
    let obj: Id = send!(class(c"NSString"), c"alloc"; Id);
    let obj: Id = send!(obj, c"initWithBytes:length:encoding:",
        s.as_ptr().cast::<c_void>() => *const c_void, s.len() => usize, 4usize => usize; Id);
    if obj.is_null() {
        return obj;
    }
    send!(obj, c"autorelease"; Id)
}
unsafe fn rust_string(s: Id, limit: usize) -> Option<String> {
    if s.is_null() {
        return None;
    }
    let p: *const c_char = send!(s, c"UTF8String"; *const c_char);
    if p.is_null() {
        return None;
    }
    let bytes = CStr::from_ptr(p).to_bytes();
    Some(String::from_utf8_lossy(&bytes[..bytes.len().min(limit)]).into_owned())
}

struct Pool(*mut c_void);
impl Pool {
    fn new() -> Self {
        // SAFETY: push/pop are balanced by Drop on the same thread.
        Self(unsafe { objc_autoreleasePoolPush() })
    }
}
impl Drop for Pool {
    fn drop(&mut self) {
        unsafe { objc_autoreleasePoolPop(self.0) }
    }
}

// Set by the delegate (close button, Cmd+Q, Dock "Quit", logout) and read by poll.
static CLOSE_REQUESTED: AtomicBool = AtomicBool::new(false);
// AppKit is waiting for replyToApplicationShouldTerminate: (NSTerminateLater).
static TERMINATE_PENDING: AtomicBool = AtomicBool::new(false);
extern "C" fn window_should_close(_: Id, _: Sel, _: Id) -> i8 {
    CLOSE_REQUESTED.store(true, Ordering::Relaxed);
    0 // NO: the game loop ends and Drop closes the window.
}
/// Quit, logout and shutdown: the game loop ends first and Drop then lets
/// AppKit terminate. Cancelling instead would abort a logout or shutdown.
extern "C" fn application_should_terminate(this: Id, _: Sel, _: Id) -> usize {
    CLOSE_REQUESTED.store(true, Ordering::Relaxed);
    if !TERMINATE_PENDING.swap(true, Ordering::Relaxed) {
        // If AppKit waits for the reply in a nested modal-panel run loop, the
        // game loop cannot run until it returns; this timer only fires in
        // that mode and answers right away. Settings are saved as they change.
        // SAFETY: plain messages to Foundation objects on the main thread.
        unsafe {
            let modes: Id =
                send!(class(c"NSArray"), c"arrayWithObject:", NSModalPanelRunLoopMode => Id; Id);
            let _: () = send!(this, c"performSelector:withObject:afterDelay:inModes:",
                sel(c"diceWarsReplyTerminate:") => Sel, ptr::null_mut::<c_void>() => Id,
                0.0f64 => f64, modes => Id; ());
        }
    }
    2 // NSTerminateLater
}
extern "C" fn reply_terminate(_: Id, _: Sel, _: Id) {
    reply_to_terminate();
}
/// Let a pending quit/logout/shutdown proceed. AppKit then exits the process.
fn reply_to_terminate() {
    if TERMINATE_PENDING.swap(false, Ordering::Relaxed) {
        // SAFETY: called on the main thread; NSApp exists while a reply is pending.
        unsafe {
            let app: Id = send!(class(c"NSApplication"), c"sharedApplication"; Id);
            let _: () = send!(app, c"replyToApplicationShouldTerminate:", 1i8 => i8; ());
        }
    }
}
unsafe fn delegate_class() -> Id {
    let name = c"DiceWarsDelegate";
    let existing = objc_getClass(name.as_ptr());
    if !existing.is_null() {
        return existing;
    }
    let cls = objc_allocateClassPair(class(c"NSObject"), name.as_ptr(), 0);
    if cls.is_null() {
        return ptr::null_mut();
    }
    #[cfg(target_arch = "aarch64")]
    let bool_types = c"B@:@";
    #[cfg(not(target_arch = "aarch64"))]
    let bool_types = c"c@:@";
    class_addMethod(
        cls,
        sel(c"windowShouldClose:"),
        window_should_close as *const c_void,
        bool_types.as_ptr(),
    );
    class_addMethod(
        cls,
        sel(c"applicationShouldTerminate:"),
        application_should_terminate as *const c_void,
        c"Q@:@".as_ptr(),
    );
    class_addMethod(
        cls,
        sel(c"diceWarsReplyTerminate:"),
        reply_terminate as *const c_void,
        c"v@:@".as_ptr(),
    );
    objc_registerClassPair(cls);
    cls
}

unsafe fn menu_item(title: &str, action: &CStr, key: &str) -> Id {
    let item: Id = send!(class(c"NSMenuItem"), c"alloc"; Id);
    let item: Id = send!(item, c"initWithTitle:action:keyEquivalent:",
        ns_string(title) => Id, sel(action) => Sel, ns_string(key) => Id; Id);
    send!(item, c"autorelease"; Id)
}
unsafe fn install_menu(app: Id) {
    let bar: Id = send!(send!(class(c"NSMenu"), c"new"; Id), c"autorelease"; Id);
    let app_item: Id = send!(send!(class(c"NSMenuItem"), c"new"; Id), c"autorelease"; Id);
    let _: () = send!(bar, c"addItem:", app_item => Id; ());
    let menu: Id = send!(send!(class(c"NSMenu"), c"new"; Id), c"autorelease"; Id);
    let _: () = send!(menu, c"addItem:", menu_item("Hide Dice Wars", c"hide:", "h") => Id; ());
    let _: () =
        send!(menu, c"addItem:", send!(class(c"NSMenuItem"), c"separatorItem"; Id) => Id; ());
    let _: () = send!(menu, c"addItem:", menu_item("Quit Dice Wars", c"terminate:", "q") => Id; ());
    let _: () = send!(app_item, c"setSubmenu:", menu => Id; ());
    let _: () = send!(app, c"setMainMenu:", bar => Id; ());
}
/// Dock icon for an unbundled binary; a bundle's .icns is shown the same way.
unsafe fn install_icon(app: Id, space: *mut c_void) {
    let icon: &'static [u8] = include_bytes!("../../assets/icon.rgba");
    let provider =
        CGDataProviderCreateWithData(ptr::null_mut(), icon.as_ptr().cast(), icon.len(), None);
    if provider.is_null() {
        return;
    }
    // kCGImageAlphaLast: straight (non-premultiplied) RGBA bytes.
    let image = CGImageCreate(
        256,
        256,
        8,
        32,
        256 * 4,
        space,
        3,
        provider,
        ptr::null(),
        true,
        0,
    );
    CGDataProviderRelease(provider);
    if image.is_null() {
        return;
    }
    let ns: Id = send!(class(c"NSImage"), c"alloc"; Id);
    let ns: Id =
        send!(ns, c"initWithCGImage:size:", image => *mut c_void, NSSize::default() => NSSize; Id);
    CGImageRelease(image);
    if !ns.is_null() {
        let _: () = send!(app, c"setApplicationIconImage:", ns => Id; ());
        let _: () = send!(ns, c"release"; ());
    }
}

const COMMAND: usize = 1 << 20;
const CONTROL: usize = 1 << 18;
const OPTION: usize = 1 << 19;

pub(super) struct Window {
    app: Id,
    window: Id,
    view: Id,
    layer: Id,
    delegate: Id,
    space: *mut c_void,
    points: (f64, f64),
    scale: f64,
    size: (usize, usize),
    focused: bool,
    scroll: f64,
}
impl Window {
    pub fn new(title: &str, width: usize, height: usize) -> Result<Self, String> {
        // SAFETY: every message matches its AppKit signature; owned objects are released in Drop.
        unsafe {
            let _pool = Pool::new();
            CLOSE_REQUESTED.store(false, Ordering::Relaxed);
            TERMINATE_PENDING.store(false, Ordering::Relaxed);
            let app: Id = send!(class(c"NSApplication"), c"sharedApplication"; Id);
            if app.is_null() {
                return Err("Could not start the Cocoa application.".into());
            }
            // Regular: Dock icon, menu bar and keyboard focus, even without a bundle.
            let _: i8 = send!(app, c"setActivationPolicy:", 0isize => isize; i8);
            let space = CGColorSpaceCreateWithName(kCGColorSpaceSRGB);
            install_menu(app);
            install_icon(app, space);
            let cls = delegate_class();
            let delegate: Id = if cls.is_null() {
                ptr::null_mut()
            } else {
                send!(send!(cls, c"alloc"; Id), c"init"; Id)
            };
            let _: () = send!(app, c"setDelegate:", delegate => Id; ());
            let _: () = send!(app, c"finishLaunching"; ());

            let rect = NSRect {
                origin: NSPoint::default(),
                size: NSSize {
                    width: width as f64,
                    height: height as f64,
                },
            };
            // Titled | closable | miniaturizable | resizable; NSBackingStoreBuffered.
            let window: Id = send!(class(c"NSWindow"), c"alloc"; Id);
            let window: Id = send!(window, c"initWithContentRect:styleMask:backing:defer:",
                rect => NSRect, 15usize => usize, 2usize => usize, 0i8 => i8; Id);
            if window.is_null() {
                CGColorSpaceRelease(space);
                return Err("Could not create game window".into());
            }
            let _: () = send!(window, c"setReleasedWhenClosed:", 0i8 => i8; ());
            let _: () = send!(window, c"setTitle:", ns_string(title) => Id; ());
            let _: () = send!(window, c"setDelegate:", delegate => Id; ());
            // NSWindowCollectionBehaviorFullScreenPrimary: the green button enters full screen.
            let _: () = send!(window, c"setCollectionBehavior:", 1usize << 7 => usize; ());
            let _: () = send!(window, c"setContentMinSize:",
                NSSize { width: 240.0, height: 240.0 } => NSSize; ());

            // A layer-hosting view: the layer is ours, so AppKit never redraws its contents.
            let layer: Id = send!(send!(class(c"CALayer"), c"alloc"; Id), c"init"; Id);
            let _: () = send!(layer, c"setContentsGravity:", kCAGravityResize => Id; ());
            let _: () = send!(layer, c"setOpaque:", 1i8 => i8; ());
            let view: Id = send!(class(c"NSView"), c"alloc"; Id);
            let view: Id = send!(view, c"initWithFrame:", rect => NSRect; Id);
            let _: () = send!(view, c"setLayer:", layer => Id; ());
            let _: () = send!(view, c"setWantsLayer:", 1i8 => i8; ());
            let _: () = send!(window, c"setContentView:", view => Id; ());

            let _: () = send!(window, c"center"; ());
            let _: () =
                send!(window, c"makeKeyAndOrderFront:", ptr::null_mut::<c_void>() => Id; ());
            let _: () = send!(app, c"activateIgnoringOtherApps:", 1i8 => i8; ());
            Ok(Self {
                app,
                window,
                view,
                layer,
                delegate,
                space,
                points: (0.0, 0.0),
                scale: 0.0,
                size: (0, 0),
                focused: true,
                scroll: 0.0,
            })
        }
    }
    /// Converts window coordinates (bottom-left origin, points) to framebuffer pixels.
    fn to_pixels(&self, p: NSPoint) -> (f32, f32) {
        (
            (p.x * self.scale) as f32,
            ((self.points.1 - p.y) * self.scale) as f32,
        )
    }
    fn inside(&self, p: NSPoint) -> bool {
        p.x >= 0.0 && p.y >= 0.0 && p.x < self.points.0 && p.y < self.points.1
    }
    pub fn poll(&mut self, input: &mut Input) {
        // SAFETY: events are only inspected with selectors valid for their type.
        unsafe {
            let _pool = Pool::new();
            let bounds = send_rect(self.view, c"bounds");
            let scale: f64 = send!(self.window, c"backingScaleFactor"; f64);
            let scale = if scale > 0.0 { scale } else { 1.0 };
            self.points = (bounds.size.width, bounds.size.height);
            if scale != self.scale {
                self.scale = scale;
                let _: () = send!(self.layer, c"setContentsScale:", scale => f64; ());
                input.repaint = true;
            }
            let size = (
                ((self.points.0 * scale).round() as usize).max(1),
                ((self.points.1 * scale).round() as usize).max(1),
            );
            if size != self.size {
                self.size = size;
                input.repaint = true;
                // Keep the hosted layer on the view's bounds, without implicit animation.
                let transaction = class(c"CATransaction");
                let _: () = send!(transaction, c"begin"; ());
                let _: () = send!(transaction, c"setDisableActions:", 1i8 => i8; ());
                let _: () = send!(self.layer, c"setFrame:", bounds => NSRect; ());
                let _: () = send!(transaction, c"commit"; ());
            }
            input.size = self.size;

            let past: Id = send!(class(c"NSDate"), c"distantPast"; Id);
            for _ in 0..4096 {
                let event: Id = send!(self.app, c"nextEventMatchingMask:untilDate:inMode:dequeue:",
                    usize::MAX => usize, past => Id, NSDefaultRunLoopMode => Id, 1i8 => i8; Id);
                if event.is_null() {
                    break;
                }
                let kind: usize = send!(event, c"type"; usize);
                let mine = send!(event, c"window"; Id) == self.window;
                match kind {
                    // Key events never reach AppKit (an unhandled keyDown beeps);
                    // Command shortcuts go to the menu directly.
                    10 | 11 => {
                        let down = kind == 10;
                        let flags: usize = send!(event, c"modifierFlags"; usize);
                        let code: u16 = send!(event, c"keyCode"; u16);
                        let plain =
                            rust_string(send!(event, c"charactersIgnoringModifiers"; Id), 64);
                        if let Some(k) = key_code(code, plain.as_deref().unwrap_or("")) {
                            input.key(k, down);
                        }
                        if down && flags & (COMMAND | CONTROL) == 0 {
                            if let Some(s) = rust_string(send!(event, c"characters"; Id), 256) {
                                input.chars.extend(s.chars().filter(|c| {
                                    !c.is_control() && !('\u{f700}'..='\u{f8ff}').contains(c)
                                }));
                            }
                        }
                        if down && flags & COMMAND != 0 {
                            let menu: Id = send!(self.app, c"mainMenu"; Id);
                            if !menu.is_null() {
                                let _: i8 = send!(menu, c"performKeyEquivalent:", event => Id; i8);
                            }
                        }
                        continue;
                    }
                    // Modifiers: Command doubles as Control so Cmd+V pastes.
                    12 => {
                        let flags: usize = send!(event, c"modifierFlags"; usize);
                        let any = flags & (COMMAND | CONTROL) != 0;
                        // Device-dependent bits: left Ctrl/Cmd, right Ctrl/Cmd.
                        let mut left = any && flags & (0x0001 | 0x0008) != 0;
                        let right = any && flags & (0x2000 | 0x0010) != 0;
                        if any && !left && !right {
                            left = true;
                        }
                        input.key(KeyCode::LeftControl, left);
                        input.key(KeyCode::RightControl, right);
                        input.key(KeyCode::Alt, flags & OPTION != 0);
                        continue;
                    }
                    1 => {
                        let p: NSPoint = send!(event, c"locationInWindow"; NSPoint);
                        if mine && self.inside(p) {
                            input.mouse = self.to_pixels(p);
                            input.left = true;
                            input.clicked = true;
                        }
                    }
                    2 => {
                        if input.left {
                            let p: NSPoint = send!(event, c"locationInWindow"; NSPoint);
                            if mine {
                                input.mouse = self.to_pixels(p);
                            }
                        }
                        input.left = false;
                    }
                    5 | 6 => {
                        let p: NSPoint = send!(event, c"locationInWindow"; NSPoint);
                        if mine && (input.left || self.inside(p)) {
                            input.mouse = self.to_pixels(p);
                        }
                    }
                    22 if mine => {
                        let dy: f64 = send!(event, c"scrollingDeltaY"; f64);
                        let precise: i8 = send!(event, c"hasPreciseScrollingDeltas"; i8);
                        if precise != 0 {
                            // Trackpads report pixels: one step per ~24 points of travel.
                            if dy * self.scroll < 0.0 {
                                self.scroll = 0.0;
                            }
                            self.scroll += dy / 24.0;
                            let steps = self.scroll.trunc();
                            self.scroll -= steps;
                            input.wheel.1 += steps as f32;
                        } else {
                            input.wheel.1 += dy as f32;
                        }
                    }
                    _ => {}
                }
                let _: () = send!(self.app, c"sendEvent:", event => Id; ());
            }

            // Hover also tracks while unfocused, as on X11 and Win32.
            let p: NSPoint = send!(self.window, c"mouseLocationOutsideOfEventStream"; NSPoint);
            if self.inside(p) || input.left {
                input.mouse = self.to_pixels(p);
            }
            let key = send!(self.window, c"isKeyWindow"; i8) != 0;
            if key != self.focused {
                self.focused = key;
                if key {
                    input.focused = true;
                } else {
                    input.blur();
                }
            }
            if CLOSE_REQUESTED.swap(false, Ordering::Relaxed) {
                input.closed = true;
            }
        }
    }
    pub fn present(&mut self, surface: &Surface, _damage: &[(usize, usize, usize, usize)]) {
        let (w, h) = (surface.width, surface.height);
        if w == 0 || h == 0 {
            return;
        }
        // Core Animation keeps the image after this call returns, so each frame
        // owns its buffer; the data provider frees it when the layer lets go.
        let pixels: Box<Vec<u32>> = Box::new(
            surface
                .pixels
                .chunks_exact(4)
                .map(|p| (p[0] as u32) << 16 | (p[1] as u32) << 8 | p[2] as u32)
                .collect(),
        );
        unsafe extern "C" fn release(info: *mut c_void, _: *const c_void, _: usize) {
            drop(unsafe { Box::from_raw(info.cast::<Vec<u32>>()) });
        }
        // SAFETY: the provider takes ownership of `pixels` and frees it through `release`.
        unsafe {
            let _pool = Pool::new();
            let data = pixels.as_ptr().cast::<c_void>();
            let bytes = pixels.len() * 4;
            let info = Box::into_raw(pixels).cast::<c_void>();
            let provider = CGDataProviderCreateWithData(info, data, bytes, Some(release));
            if provider.is_null() {
                release(info, data, bytes);
                return;
            }
            // kCGBitmapByteOrder32Little | kCGImageAlphaNoneSkipFirst: 0x00RRGGBB words.
            let image = CGImageCreate(
                w,
                h,
                8,
                32,
                w * 4,
                self.space,
                (2 << 12) | 6,
                provider,
                ptr::null(),
                false,
                0,
            );
            CGDataProviderRelease(provider);
            if image.is_null() {
                return;
            }
            let transaction = class(c"CATransaction");
            let _: () = send!(transaction, c"begin"; ());
            let _: () = send!(transaction, c"setDisableActions:", 1i8 => i8; ());
            let _: () = send!(self.layer, c"setContents:", image => *mut c_void; ());
            let _: () = send!(transaction, c"commit"; ());
            CGImageRelease(image);
        }
    }
}
impl Drop for Window {
    fn drop(&mut self) {
        // SAFETY: these objects were created by this Window and are released once.
        unsafe {
            let _pool = Pool::new();
            let none = ptr::null_mut::<c_void>();
            let _: () = send!(self.window, c"setDelegate:", none => Id; ());
            let _: () = send!(self.app, c"setDelegate:", none => Id; ());
            let _: () = send!(self.window, c"orderOut:", none => Id; ());
            let _: () = send!(self.window, c"close"; ());
            let _: () = send!(self.window, c"release"; ());
            let _: () = send!(self.view, c"release"; ());
            let _: () = send!(self.layer, c"release"; ());
            if !self.delegate.is_null() {
                let _: () = send!(self.delegate, c"release"; ());
            }
            if !self.space.is_null() {
                CGColorSpaceRelease(self.space);
            }
        }
        // Last: with a quit pending, AppKit exits the process here.
        reply_to_terminate();
    }
}
/// Virtual key codes (Carbon `kVK_*`). Letters follow the active keyboard layout.
fn key_code(code: u16, plain: &str) -> Option<KeyCode> {
    match plain.chars().next().map(|c| c.to_ascii_lowercase()) {
        Some('m') => return Some(KeyCode::M),
        Some('n') => return Some(KeyCode::N),
        Some('r') => return Some(KeyCode::R),
        Some('v') => return Some(KeyCode::V),
        _ => {}
    }
    Some(match code {
        0x31 => KeyCode::Space,
        0x35 => KeyCode::Escape,
        0x24 | 0x4c => KeyCode::Enter,
        0x33 => KeyCode::Backspace,
        0x30 => KeyCode::Tab,
        0x64 => KeyCode::F8,
        0x65 => KeyCode::F9,
        0x6d => KeyCode::F10,
        0x7b => KeyCode::Left,
        0x7c => KeyCode::Right,
        0x7d => KeyCode::Down,
        0x7e => KeyCode::Up,
        _ => return None,
    })
}
pub(super) fn clipboard_set(text: &str) {
    // SAFETY: NSPasteboard is used inside a local autorelease pool.
    unsafe {
        let _pool = Pool::new();
        let board: Id = send!(class(c"NSPasteboard"), c"generalPasteboard"; Id);
        if board.is_null() {
            return;
        }
        let _: isize = send!(board, c"clearContents"; isize);
        let _: i8 = send!(board, c"setString:forType:",
            ns_string(text) => Id, NSPasteboardTypeString => Id; i8);
    }
}
pub(super) fn clipboard_get() -> Option<String> {
    // SAFETY: the returned NSString is copied before the pool drains.
    unsafe {
        let _pool = Pool::new();
        let board: Id = send!(class(c"NSPasteboard"), c"generalPasteboard"; Id);
        if board.is_null() {
            return None;
        }
        let text: Id = send!(board, c"stringForType:", NSPasteboardTypeString => Id; Id);
        rust_string(text, 65536)
    }
}
