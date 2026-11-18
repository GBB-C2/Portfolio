//! Native Win32 window, input, buffered GDI presentation, and system dialogs.
//!
//! All handles are UI-thread confined. The window owns a stable Box<WindowState>
//! until its message loop has ended. No mutable application borrow is retained
//! across a modal OS call, because those calls can dispatch window messages.

#[path = "text_editor.rs"]
mod text_editor;

use super::frame_cache::{FontChoiceCache, FontContext, FramePacer};
use super::*;
use crate::i18n::Language;
use std::collections::HashMap;
use std::ffi::{c_void, OsString};
use std::mem::{size_of, zeroed};
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::ptr::{null, null_mut};
use std::time::Instant;

type Handle = *mut c_void;
type Hwnd = Handle;
type Hdc = Handle;
type Wparam = usize;
type Lparam = isize;
type Lresult = isize;
type WndProc = unsafe extern "system" fn(Hwnd, u32, Wparam, Lparam) -> Lresult;

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Point {
    x: i32,
    y: i32,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Rect {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
}
#[repr(C)]
struct WndClass {
    style: u32,
    wnd_proc: Option<WndProc>,
    cls_extra: i32,
    wnd_extra: i32,
    instance: Handle,
    icon: Handle,
    cursor: Handle,
    background: Handle,
    menu_name: *const u16,
    class_name: *const u16,
}
#[repr(C)]
struct CreateStruct {
    create_params: *mut c_void,
    instance: Handle,
    menu: Handle,
    parent: Hwnd,
    cy: i32,
    cx: i32,
    y: i32,
    x: i32,
    style: i32,
    name: *const u16,
    class: *const u16,
    ex_style: u32,
}
#[repr(C)]
struct Msg {
    hwnd: Hwnd,
    message: u32,
    wparam: Wparam,
    lparam: Lparam,
    time: u32,
    pt: Point,
    private: u32,
}
#[repr(C)]
struct PaintStruct {
    dc: Hdc,
    erase: i32,
    paint: Rect,
    restore: i32,
    inc_update: i32,
    reserved: [u8; 32],
}
#[repr(C)]
struct MinMaxInfo {
    reserved: Point,
    max_size: Point,
    max_position: Point,
    min_track_size: Point,
    max_track_size: Point,
}
#[repr(C)]
struct BitmapInfoHeader {
    size: u32,
    width: i32,
    height: i32,
    planes: u16,
    bit_count: u16,
    compression: u32,
    size_image: u32,
    x_pels_per_meter: i32,
    y_pels_per_meter: i32,
    colors_used: u32,
    colors_important: u32,
}
#[repr(C)]
struct BitmapInfo {
    header: BitmapInfoHeader,
    colors: [u32; 1],
}
#[repr(C)]
struct LogFont {
    height: i32,
    width: i32,
    escapement: i32,
    orientation: i32,
    weight: i32,
    italic: u8,
    underline: u8,
    strikeout: u8,
    charset: u8,
    output_precision: u8,
    clip_precision: u8,
    quality: u8,
    pitch_and_family: u8,
    face_name: [u16; 32],
}
/// The classic OPENFILENAMEW ABI (152 bytes on Windows x64). Windows retains
/// support for this size; newer SDK-only optional fields are deliberately absent.
#[repr(C)]
struct OpenFileName {
    struct_size: u32,
    owner: Hwnd,
    instance: Handle,
    filter: *const u16,
    custom_filter: *mut u16,
    max_custom_filter: u32,
    filter_index: u32,
    file: *mut u16,
    max_file: u32,
    file_title: *mut u16,
    max_file_title: u32,
    initial_dir: *const u16,
    title: *const u16,
    flags: u32,
    file_offset: u16,
    file_extension: u16,
    default_extension: *const u16,
    custom_data: Lparam,
    hook: Option<unsafe extern "system" fn(Hwnd, u32, Wparam, Lparam) -> usize>,
    template_name: *const u16,
    reserved: *mut c_void,
    reserved_dword: u32,
    flags_ex: u32,
}

#[link(name = "user32")]
extern "system" {
    fn RegisterClassW(class: *const WndClass) -> u16;
    fn CreateWindowExW(
        ex: u32,
        class: *const u16,
        name: *const u16,
        style: u32,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        parent: Hwnd,
        menu: Handle,
        instance: Handle,
        param: *mut c_void,
    ) -> Hwnd;
    fn DefWindowProcW(hwnd: Hwnd, message: u32, wparam: Wparam, lparam: Lparam) -> Lresult;
    fn GetMessageW(message: *mut Msg, hwnd: Hwnd, min: u32, max: u32) -> i32;
    fn TranslateMessage(message: *const Msg) -> i32;
    fn DispatchMessageW(message: *const Msg) -> Lresult;
    fn ShowWindow(hwnd: Hwnd, cmd: i32) -> i32;
    fn UpdateWindow(hwnd: Hwnd) -> i32;
    fn DestroyWindow(hwnd: Hwnd) -> i32;
    fn PostQuitMessage(code: i32);
    fn GetWindowLongPtrW(hwnd: Hwnd, index: i32) -> isize;
    fn SetWindowLongPtrW(hwnd: Hwnd, index: i32, value: isize) -> isize;
    fn SetWindowTextW(hwnd: Hwnd, text: *const u16) -> i32;
    fn GetClientRect(hwnd: Hwnd, rect: *mut Rect) -> i32;
    fn InvalidateRect(hwnd: Hwnd, rect: *const Rect, erase: i32) -> i32;
    fn BeginPaint(hwnd: Hwnd, paint: *mut PaintStruct) -> Hdc;
    fn EndPaint(hwnd: Hwnd, paint: *const PaintStruct) -> i32;
    fn LoadCursorW(instance: Handle, name: *const u16) -> Handle;
    fn LoadIconW(instance: Handle, name: *const u16) -> Handle;
    fn GetKeyState(key: i32) -> i16;
    fn SetCapture(hwnd: Hwnd) -> Hwnd;
    fn GetCapture() -> Hwnd;
    fn ReleaseCapture() -> i32;
    fn SetFocus(hwnd: Hwnd) -> Hwnd;
    fn GetFocus() -> Hwnd;
    fn GetDC(hwnd: Hwnd) -> Hdc;
    fn ReleaseDC(hwnd: Hwnd, dc: Hdc) -> i32;
    fn SetWindowRgn(hwnd: Hwnd, region: Handle, redraw: i32) -> i32;
    fn ScreenToClient(hwnd: Hwnd, point: *mut Point) -> i32;
    fn SetTimer(hwnd: Hwnd, id: usize, elapsed: u32, callback: *const c_void) -> usize;
    fn KillTimer(hwnd: Hwnd, id: usize) -> i32;
    fn MessageBoxW(hwnd: Hwnd, text: *const u16, caption: *const u16, kind: u32) -> i32;
    fn SetProcessDpiAwarenessContext(context: Handle) -> i32;
    fn GetDpiForWindow(hwnd: Hwnd) -> u32;
    fn GetDpiForSystem() -> u32;
    fn AdjustWindowRectExForDpi(rect: *mut Rect, style: u32, menu: i32, ex: u32, dpi: u32) -> i32;
    fn SetWindowPos(hwnd: Hwnd, after: Hwnd, x: i32, y: i32, cx: i32, cy: i32, flags: u32) -> i32;
    fn GetSystemMetrics(index: i32) -> i32;
    fn SystemParametersInfoW(action: u32, parameter: u32, value: *mut c_void, flags: u32) -> i32;
    fn DrawTextW(dc: Hdc, text: *const u16, count: i32, rect: *mut Rect, format: u32) -> i32;
    fn OpenClipboard(hwnd: Hwnd) -> i32;
    fn CloseClipboard() -> i32;
    fn EmptyClipboard() -> i32;
    fn SetClipboardData(format: u32, memory: Handle) -> Handle;
    fn GetClipboardData(format: u32) -> Handle;
    fn IsWindow(hwnd: Hwnd) -> i32;
    fn DialogBoxIndirectParamW(
        instance: Handle,
        template: *const c_void,
        owner: Hwnd,
        dialog_proc: Option<WndProc>,
        parameter: Lparam,
    ) -> isize;
    fn EndDialog(hwnd: Hwnd, result: isize) -> i32;
    fn GetDlgItem(hwnd: Hwnd, id: i32) -> Hwnd;
    fn GetWindowTextLengthW(hwnd: Hwnd) -> i32;
    fn GetWindowTextW(hwnd: Hwnd, text: *mut u16, max_count: i32) -> i32;
    fn SendMessageW(hwnd: Hwnd, message: u32, wparam: Wparam, lparam: Lparam) -> Lresult;
}
#[link(name = "kernel32")]
extern "system" {
    fn LoadLibraryExW(name: *const u16, file: Handle, flags: u32) -> Handle;
    fn FreeLibrary(module: Handle) -> i32;
    fn GetModuleHandleW(name: *const u16) -> Handle;
    fn GetLastError() -> u32;
    fn GlobalAlloc(flags: u32, bytes: usize) -> Handle;
    fn GlobalFree(memory: Handle) -> Handle;
    fn GlobalLock(memory: Handle) -> *mut c_void;
    fn GlobalUnlock(memory: Handle) -> i32;
    fn GlobalSize(memory: Handle) -> usize;
}
#[link(name = "gdi32")]
extern "system" {
    fn CreateRectRgn(left: i32, top: i32, right: i32, bottom: i32) -> Handle;
    fn CombineRgn(dest: Handle, left: Handle, right: Handle, mode: i32) -> i32;
    fn CreateCompatibleDC(dc: Hdc) -> Hdc;
    fn DeleteDC(dc: Hdc) -> i32;
    fn DeleteObject(object: Handle) -> i32;
    fn SelectObject(dc: Hdc, object: Handle) -> Handle;
    fn CreateDIBSection(
        dc: Hdc,
        info: *const BitmapInfo,
        usage: u32,
        bits: *mut *mut c_void,
        section: Handle,
        offset: u32,
    ) -> Handle;
    fn BitBlt(
        destination: Hdc,
        x: i32,
        y: i32,
        cx: i32,
        cy: i32,
        source: Hdc,
        sx: i32,
        sy: i32,
        operation: u32,
    ) -> i32;
    fn CreateFontW(
        height: i32,
        width: i32,
        escapement: i32,
        orientation: i32,
        weight: i32,
        italic: u32,
        underline: u32,
        strikeout: u32,
        charset: u32,
        output_precision: u32,
        clip_precision: u32,
        quality: u32,
        pitch_and_family: u32,
        face: *const u16,
    ) -> Handle;
    fn EnumFontFamiliesExW(
        dc: Hdc,
        font: *mut LogFont,
        callback: Option<
            unsafe extern "system" fn(*const LogFont, *const c_void, u32, Lparam) -> i32,
        >,
        parameter: Lparam,
        flags: u32,
    ) -> i32;
    fn SetTextColor(dc: Hdc, color: u32) -> u32;
    fn SetBkMode(dc: Hdc, mode: i32) -> i32;
    fn SaveDC(dc: Hdc) -> i32;
    fn IntersectClipRect(dc: Hdc, left: i32, top: i32, right: i32, bottom: i32) -> i32;
    fn SelectClipRgn(dc: Hdc, region: Handle) -> i32;
    fn RestoreDC(dc: Hdc, saved: i32) -> i32;
    fn GdiFlush() -> i32;
    fn GetGlyphIndicesW(dc: Hdc, text: *const u16, count: i32, glyphs: *mut u16, flags: u32)
        -> u32;
    fn GetTextFaceW(dc: Hdc, count: i32, face: *mut u16) -> i32;
    fn AddFontMemResourceEx(
        data: *const c_void,
        size: u32,
        reserved: *mut c_void,
        count: *mut u32,
    ) -> Handle;
    fn RemoveFontMemResourceEx(handle: Handle) -> i32;
}
#[link(name = "shell32")]
extern "system" {
    fn DragAcceptFiles(hwnd: Hwnd, accept: i32);
    fn DragQueryFileW(drop: Handle, index: u32, file: *mut u16, length: u32) -> u32;
    fn DragFinish(drop: Handle);
}
#[link(name = "comdlg32")]
extern "system" {
    fn GetOpenFileNameW(ofn: *mut OpenFileName) -> i32;
    fn GetSaveFileNameW(ofn: *mut OpenFileName) -> i32;
    fn CommDlgExtendedError() -> u32;
}
#[link(name = "dwmapi")]
extern "system" {
    fn DwmSetWindowAttribute(hwnd: Hwnd, attribute: u32, value: *const c_void, size: u32) -> i32;
}

const GWLP_USERDATA: i32 = -21;
const WS_OVERLAPPEDWINDOW: u32 = 0x00cf_0000;
const WM_NCCREATE: u32 = 0x0081;
const WM_NCDESTROY: u32 = 0x0082;
const WM_PAINT: u32 = 0x000f;
const WM_CLOSE: u32 = 0x0010;
const WM_DESTROY: u32 = 0x0002;
const WM_SIZE: u32 = 0x0005;
const WM_ERASEBKGND: u32 = 0x0014;
const WM_GETMINMAXINFO: u32 = 0x0024;
const WM_KILLFOCUS: u32 = 0x0008;
const WM_DPICHANGED: u32 = 0x02e0;
const WM_MOUSEMOVE: u32 = 0x0200;
const WM_LBUTTONDOWN: u32 = 0x0201;
const WM_LBUTTONUP: u32 = 0x0202;
const WM_RBUTTONDOWN: u32 = 0x0204;
const WM_RBUTTONUP: u32 = 0x0205;
const WM_MBUTTONDOWN: u32 = 0x0207;
const WM_MBUTTONUP: u32 = 0x0208;
const WM_MOUSEWHEEL: u32 = 0x020a;
const WM_CAPTURECHANGED: u32 = 0x0215;
const WM_CANCELMODE: u32 = 0x001f;
const WM_KEYDOWN: u32 = 0x0100;
const WM_KEYUP: u32 = 0x0101;
const WM_CHAR: u32 = 0x0102;
const WM_SYSKEYDOWN: u32 = 0x0104;
const WM_SYSKEYUP: u32 = 0x0105;
const WM_SYSCHAR: u32 = 0x0106;
const WM_UNICHAR: u32 = 0x0109;
const WM_TIMER: u32 = 0x0113;
const WM_DROPFILES: u32 = 0x0233;
const WM_SETTINGCHANGE: u32 = 0x001a;

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16()
        .filter(|&ch| ch != 0)
        .chain(Some(0))
        .collect()
}
fn rgb_to_colorref(rgb: u32) -> u32 {
    ((rgb >> 16) & 0xff) | (rgb & 0x00ff00) | ((rgb & 0xff) << 16)
}
fn signed_low(value: isize) -> i32 {
    (value as u16 as i16) as i32
}
fn signed_high(value: isize) -> i32 {
    ((value >> 16) as u16 as i16) as i32
}

struct Surface {
    dc: Hdc,
    bitmap: Handle,
    previous: Handle,
    bits: *mut u32,
    width: u32,
    height: u32,
}
impl Surface {
    unsafe fn new(width: u32, height: u32) -> Result<Self, String> {
        let dc = CreateCompatibleDC(null_mut());
        if dc.is_null() {
            return Err("Не удалось создать буфер окна.".into());
        }
        let info = BitmapInfo {
            header: BitmapInfoHeader {
                size: size_of::<BitmapInfoHeader>() as u32,
                width: width as i32,
                height: -(height as i32),
                planes: 1,
                bit_count: 32,
                compression: 0,
                size_image: 0,
                x_pels_per_meter: 0,
                y_pels_per_meter: 0,
                colors_used: 0,
                colors_important: 0,
            },
            colors: [0],
        };
        let mut bits = null_mut();
        let bitmap = CreateDIBSection(dc, &info, 0, &mut bits, null_mut(), 0);
        if bitmap.is_null() || bits.is_null() {
            if !bitmap.is_null() {
                DeleteObject(bitmap);
            }
            DeleteDC(dc);
            return Err("Недостаточно памяти для буфера окна.".into());
        }
        let previous = SelectObject(dc, bitmap);
        if previous.is_null() || previous == -1isize as Handle {
            DeleteObject(bitmap);
            DeleteDC(dc);
            return Err("Не удалось подключить буфер окна.".into());
        }
        Ok(Self {
            dc,
            bitmap,
            previous,
            bits: bits.cast(),
            width,
            height,
        })
    }
}
impl Drop for Surface {
    fn drop(&mut self) {
        unsafe {
            SelectObject(self.dc, self.previous);
            DeleteObject(self.bitmap);
            DeleteDC(self.dc);
        }
    }
}
struct FontSearch {
    target: [u16; 32],
    found: bool,
}

unsafe extern "system" fn font_family_match(
    font: *const LogFont,
    _metrics: *const c_void,
    _kind: u32,
    parameter: Lparam,
) -> i32 {
    // The enumeration is synchronous. `parameter` remains valid for the call;
    // this callback neither allocates nor invokes code that can panic or reenter.
    if font.is_null() || parameter == 0 {
        return 0;
    }
    let search = &mut *(parameter as *mut FontSearch);
    let fold = |unit: u16| {
        if (b'A' as u16..=b'Z' as u16).contains(&unit) {
            unit + 32
        } else {
            unit
        }
    };
    for i in 0..32 {
        let actual = fold((*font).face_name[i]);
        let expected = fold(search.target[i]);
        if actual != expected {
            return 1;
        }
        if actual == 0 {
            search.found = true;
            return 0;
        }
    }
    1
}

unsafe fn font_available(dc: Hdc, family: &str) -> bool {
    let mut filter: LogFont = zeroed();
    filter.charset = 1; // DEFAULT_CHARSET enumerates every charset in the family.
    for (destination, source) in filter
        .face_name
        .iter_mut()
        .take(31)
        .zip(family.encode_utf16())
    {
        *destination = source;
    }
    let mut search = FontSearch {
        target: filter.face_name,
        found: false,
    };
    EnumFontFamiliesExW(
        dc,
        &mut filter,
        Some(font_family_match),
        (&mut search as *mut FontSearch) as isize,
        0,
    );
    search.found
}

struct FontResource(Handle);
impl Drop for FontResource {
    fn drop(&mut self) {
        unsafe {
            RemoveFontMemResourceEx(self.0);
        }
    }
}

#[derive(Default)]
struct Fonts {
    handles: HashMap<(i32, bool, &'static str), Handle>,
    availability: HashMap<&'static str, bool>,
    choices: FontChoiceCache<Handle>,
    glyph_scratch: Vec<u16>,
}
impl Fonts {
    /// Called between completed frames, while no HFONT is selected into our DC.
    unsafe fn begin_frame(&mut self) {
        if self.handles.len() > 256 {
            GdiFlush();
            // Cached choices contain HFONT values, so invalidate them before
            // releasing handles that Windows may later reuse for other faces.
            self.choices.clear();
            for (_, handle) in self.handles.drain() {
                DeleteObject(handle);
            }
        }
    }
    unsafe fn face(&mut self, dc: Hdc, pixels: i32, bold: bool, requested: &'static str) -> Handle {
        let installed = *self
            .availability
            .entry(requested)
            .or_insert_with(|| font_available(dc, requested));
        let resolved = if installed { requested } else { "Segoe UI" };
        let key = (pixels.clamp(1, 256), bold, resolved);
        if let Some(handle) = self.handles.get(&key) {
            return *handle;
        }
        let face = wide(resolved);
        let handle = CreateFontW(
            -key.0,
            0,
            0,
            0,
            if bold && resolved != "CozetteVectorBold" {
                600
            } else {
                400
            },
            0,
            0,
            0,
            1,
            0,
            0,
            5,
            0,
            face.as_ptr(),
        );
        if !handle.is_null() {
            self.handles.insert(key, handle);
        }
        handle
    }
    unsafe fn covers(&mut self, dc: Hdc, font: Handle, units: &[u16]) -> bool {
        if font.is_null() {
            return false;
        }
        if units.is_empty() {
            return true;
        }
        let previous = SelectObject(dc, font);
        if previous.is_null() || previous == -1isize as Handle {
            return false;
        }
        self.glyph_scratch.resize(units.len(), 0);
        let result = GetGlyphIndicesW(
            dc,
            units.as_ptr(),
            units.len() as i32,
            self.glyph_scratch.as_mut_ptr(),
            1,
        );
        SelectObject(dc, previous);
        result == units.len() as u32 && self.glyph_scratch.iter().all(|g| *g != 0xffff)
    }
    unsafe fn get(
        &mut self,
        dc: Hdc,
        pixels: i32,
        bold: bool,
        serif: bool,
        family: UiFont,
        text: &str,
        locale: &str,
    ) -> Handle {
        let requested = if serif {
            "Georgia"
        } else if bold && family == UiFont::CozetteVector {
            "CozetteVectorBold"
        } else {
            family.family()
        };
        let context = FontContext {
            pixels: pixels.clamp(1, 256),
            bold,
            face: requested,
            script: preferred_text_script(text, locale),
        };
        if let Some(font) = self.choices.get(context, text) {
            return font;
        }
        // Encode only on a cache miss, and reuse the glyph output allocation
        // across all fallback candidates. Stable UI labels need no GDI coverage
        // query after their first frame at this font/size/language.
        let units: Vec<u16> = text.encode_utf16().filter(|c| *c >= 32).collect();
        let primary = self.face(dc, pixels, bold, requested);
        if self.covers(dc, primary, &units) {
            self.choices.insert(context, text, primary);
            return primary;
        }
        let candidates: &[&'static str] = match context.script {
            "ko" => &[
                "Malgun Gothic",
                "Gulim",
                "Noto Sans CJK KR",
                "Arial Unicode MS",
            ],
            "ja" => &[
                "Yu Gothic UI",
                "Meiryo",
                "MS UI Gothic",
                "Noto Sans CJK JP",
                "Arial Unicode MS",
            ],
            "zh" => &[
                "Microsoft YaHei UI",
                "Microsoft YaHei",
                "SimHei",
                "Noto Sans CJK SC",
                "Arial Unicode MS",
            ],
            _ => &[
                "Segoe UI",
                "Tahoma",
                "Arial",
                "Noto Sans",
                "Arial Unicode MS",
            ],
        };
        for &candidate in candidates {
            let fallback = self.face(dc, pixels, bold, candidate);
            if self.covers(dc, fallback, &units) {
                self.choices.insert(context, text, fallback);
                return fallback;
            }
        }
        if !primary.is_null() {
            self.choices.insert(context, text, primary);
        }
        primary
    }
}
impl Drop for Fonts {
    fn drop(&mut self) {
        for font in self.handles.values() {
            unsafe {
                DeleteObject(*font);
            }
        }
    }
}

pub fn rasterize_text(
    text: &TextLayer,
    width: u32,
    height: u32,
    language: Language,
) -> Result<Vec<Color>, String> {
    unsafe { text_editor::rasterize(text, width, height, language) }
}

struct WindowState {
    canvas_input: text_editor::CanvasInput,
    app: Option<Box<dyn NativeApp>>,
    hwnd: Hwnd,
    scale: f32,
    surface: Option<Surface>,
    fonts: Fonts,
    text_scratch: Vec<u16>,
    // HFONT handles above are destroyed before private font registrations.
    _font_resources: Vec<FontResource>,
    pacer: FramePacer,
    last_size: (u32, u32, u32),
    timer_ms: u32,
    surface_failure: Option<(u32, u32)>,
    captured_buttons: u8,
    high_surrogate: Option<u16>,
    fatal_error: Option<String>,
}

impl WindowState {
    unsafe fn new(app: Box<dyn NativeApp>, scale: f32) -> Self {
        let mut fonts = Fonts::default();
        let font_resources = register_bundled_fonts(&mut fonts);
        Self {
            canvas_input: text_editor::CanvasInput::default(),
            app: Some(app),
            hwnd: null_mut(),
            scale,
            surface: None,
            fonts,
            text_scratch: Vec::with_capacity(256),
            _font_resources: font_resources,
            pacer: FramePacer::new(Instant::now()),
            last_size: (0, 0, 0),
            timer_ms: 0,
            surface_failure: None,
            captured_buttons: 0,
            high_surrogate: None,
            fatal_error: None,
        }
    }
}

unsafe fn register_bundled_fonts(fonts: &mut Fonts) -> Vec<FontResource> {
    let data: &[(&str, &[u8])] = &[
        (
            UiFont::ProggyForever.family(),
            include_bytes!("../../assets/fonts/ProggyForever.ttf"),
        ),
        (
            UiFont::CozetteVector.family(),
            include_bytes!("../../assets/fonts/CozetteVector.ttf"),
        ),
        (
            UiFont::ProggyClean.family(),
            include_bytes!("../../assets/fonts/ProggyClean.ttf"),
        ),
        (
            UiFont::ProggySquare.family(),
            include_bytes!("../../assets/fonts/ProggySquare.ttf"),
        ),
        (
            "CozetteVectorBold",
            include_bytes!("../../assets/fonts/CozetteVectorBold.ttf"),
        ),
    ];
    let mut resources = Vec::new();
    for &(family, bytes) in data {
        let mut count = 0;
        let handle = AddFontMemResourceEx(
            bytes.as_ptr().cast(),
            bytes.len() as u32,
            null_mut(),
            &mut count,
        );
        if !handle.is_null() {
            resources.push(FontResource(handle));
            // Memory fonts are private and intentionally not enumerated by GDI.
            if count > 0 {
                fonts.availability.insert(family, true);
            }
        }
    }
    resources
}

unsafe fn locale(ptr: *mut WindowState) -> Language {
    (*ptr)
        .app
        .as_ref()
        .and_then(|app| Language::from_key(app.preferred_language()))
        .unwrap_or_default()
}

unsafe fn notify_motion_preference(ptr: *mut WindowState) {
    let mut animate: i32 = 1;
    // SPI_GETCLIENTAREAANIMATION, supported on our Windows 10/11 target.
    if SystemParametersInfoW(0x1042, 0, (&mut animate as *mut i32).cast(), 0) != 0 {
        dispatch(ptr, Event::ReducedMotion(animate == 0));
    }
}

/// Starts the sole UI-thread event loop. A failed native call returns an error;
/// user cancellation is represented as an application event, never as success.
pub fn run(app: Box<dyn NativeApp>) -> Result<(), String> {
    unsafe {
        // The build also supplies a DPI manifest. This is a harmless fallback
        // for users invoking rustc directly; ERROR_ACCESS_DENIED means already set.
        SetProcessDpiAwarenessContext(-4isize as Handle);
        let instance = GetModuleHandleW(null());
        let class_name = wide("Portfolio.Native.Editor.v1");
        // LoadIcon returns a shared resource handle, so it is not destroyed by
        // this module. A resource-free build keeps the standard Windows icon.
        let embedded_icon = LoadIconW(instance, 1usize as *const u16);
        let icon = if embedded_icon.is_null() {
            LoadIconW(null_mut(), 32512usize as *const u16)
        } else {
            embedded_icon
        };
        let class = WndClass {
            style: 0x0003,
            wnd_proc: Some(window_proc),
            cls_extra: 0,
            wnd_extra: 0,
            instance,
            icon,
            cursor: LoadCursorW(null_mut(), 32512usize as *const u16),
            background: null_mut(),
            menu_name: null(),
            class_name: class_name.as_ptr(),
        };
        if RegisterClassW(&class) == 0 {
            return Err(format!(
                "Не удалось зарегистрировать окно Windows ({}).",
                GetLastError()
            ));
        }
        let dpi = GetDpiForSystem().max(96);
        let scale = dpi as f32 / 96.0;
        let mut rect = Rect {
            left: 0,
            top: 0,
            right: (1440.0 * scale) as i32,
            bottom: (940.0 * scale) as i32,
        };
        AdjustWindowRectExForDpi(&mut rect, WS_OVERLAPPEDWINDOW, 0, 0, dpi);
        let screen_w = GetSystemMetrics(0).max(800);
        let screen_h = GetSystemMetrics(1).max(600);
        let width = (rect.right - rect.left).min(screen_w - 64);
        let height = (rect.bottom - rect.top).min(screen_h - 80);
        let state = Box::new(WindowState::new(app, scale));
        let ptr = Box::into_raw(state);
        let title = wide("Portfolio — студия рисования");
        let hwnd = CreateWindowExW(
            0,
            class_name.as_ptr(),
            title.as_ptr(),
            WS_OVERLAPPEDWINDOW,
            (screen_w - width) / 2,
            (screen_h - height) / 2,
            width,
            height,
            null_mut(),
            null_mut(),
            instance,
            ptr.cast(),
        );
        if hwnd.is_null() {
            let code = GetLastError();
            drop(Box::from_raw(ptr));
            return Err(format!(
                "Не удалось открыть Portfolio (ошибка Windows {code})."
            ));
        }
        (*ptr).hwnd = hwnd;
        (*ptr).scale = GetDpiForWindow(hwnd).max(96) as f32 / 96.0;
        notify_resize(ptr);
        DragAcceptFiles(hwnd, 1);
        notify_motion_preference(ptr);
        let dark: i32 = 1;
        DwmSetWindowAttribute(hwnd, 20, (&dark as *const i32).cast(), 4);
        sync_timer(ptr);
        ShowWindow(hwnd, 5);
        UpdateWindow(hwnd);
        let mut message: Msg = zeroed();
        let result = loop {
            let status = GetMessageW(&mut message, null_mut(), 0, 0);
            if status == 0 {
                break Ok(());
            }
            if status == -1 {
                break Err(format!(
                    "Ошибка очереди сообщений Windows ({}).",
                    GetLastError()
                ));
            }
            if !text_editor::shortcut(ptr, &message) {
                TranslateMessage(&message);
                DispatchMessageW(&message);
            }
            // Continuous mouse input must not starve an already-due frame.
            if !(*ptr).hwnd.is_null() && (*ptr).pacer.due(Instant::now()) {
                InvalidateRect((*ptr).hwnd, null(), 0);
                // WM_PAINT normally has lower priority than queued input.
                // UpdateWindow presents a due frame here without consuming or
                // coalescing any pending input message or its coordinates.
                UpdateWindow((*ptr).hwnd);
            }
        };
        if IsWindow(hwnd) != 0 {
            DestroyWindow(hwnd);
        }
        let state = Box::from_raw(ptr);
        if let Some(message) = &state.fatal_error {
            return Err(message.clone());
        }
        result
    }
}

unsafe fn modifiers() -> Modifiers {
    Modifiers {
        ctrl: GetKeyState(0x11) < 0,
        shift: GetKeyState(0x10) < 0,
        alt: GetKeyState(0x12) < 0,
    }
}
unsafe fn notify_resize(ptr: *mut WindowState) {
    let hwnd = (*ptr).hwnd;
    if hwnd.is_null() {
        return;
    }
    let mut rect = Rect::default();
    if GetClientRect(hwnd, &mut rect) == 0 {
        return;
    }
    let (width, height) = bounded_frame_size(
        (rect.right - rect.left).max(1) as u32,
        (rect.bottom - rect.top).max(1) as u32,
    );
    let scale = (*ptr).scale;
    let next = (width, height, scale.to_bits());
    if (*ptr).last_size == next {
        return;
    }
    (*ptr).last_size = next;
    (*ptr).surface_failure = None;
    if let Some(app) = (*ptr).app.as_mut() {
        app.resize(width, height, scale);
    }
    text_editor::sync(ptr);
    request_redraw(ptr);
}

unsafe fn sync_timer(ptr: *mut WindowState) {
    let state = &mut *ptr;
    if state.hwnd.is_null() {
        return;
    }
    let animate = state.app.as_ref().is_some_and(|app| app.wants_animation());
    let working = state
        .app
        .as_ref()
        .is_some_and(|app| app.needs_frequent_ticks());
    let interval = if animate || state.pacer.dirty() {
        16
    } else if working {
        100
    } else {
        1000
    };
    if interval != state.timer_ms && SetTimer(state.hwnd, 1, interval, null()) != 0 {
        state.timer_ms = interval;
    }
}

unsafe fn request_redraw(ptr: *mut WindowState) {
    (*ptr).pacer.request();
    if !(*ptr).hwnd.is_null() && (*ptr).pacer.due(Instant::now()) {
        InvalidateRect((*ptr).hwnd, null(), 0);
    }
    sync_timer(ptr);
}

/// Borrows the application only while it handles the pure state event. Effects
/// execute afterward so file dialogs, MessageBox, and capture messages can reenter.
unsafe fn dispatch(ptr: *mut WindowState, event: Event) {
    let effects = {
        let state = &mut *ptr;
        match state.app.as_mut() {
            Some(app) => app.event(event),
            None => Vec::new(),
        }
    };
    text_editor::sync(ptr);
    for effect in effects {
        apply_effect(ptr, effect);
    }
    request_redraw(ptr);
}
unsafe fn show_message(hwnd: Hwnd, title: &str, message: &str, error: bool) {
    let title = wide(title);
    let message = wide(message);
    MessageBoxW(
        hwnd,
        message.as_ptr(),
        title.as_ptr(),
        if error { 0x10 } else { 0x40 },
    );
}
unsafe fn apply_effect(ptr: *mut WindowState, effect: Effect) {
    let hwnd = (*ptr).hwnd;
    let language = locale(ptr);
    match effect {
        Effect::SetTitle(title) => {
            let title = wide(&title);
            SetWindowTextW(hwnd, title.as_ptr());
        }
        Effect::SetDarkMode(dark) => {
            let value = i32::from(dark);
            // Documented DWMWA_USE_IMMERSIVE_DARK_MODE. Older OS builds may
            // decline this cosmetic request; client rendering is unaffected.
            DwmSetWindowAttribute(hwnd, 20, (&value as *const i32).cast(), 4);
        }
        Effect::OpenFile(kind) => match choose_file(hwnd, kind, None, language) {
            Ok(path) => dispatch(ptr, Event::FileSelected { kind, path }),
            Err(error) => {
                show_message(
                    hwnd,
                    &language.translate("Не удалось открыть диалог"),
                    &language.translate(&error),
                    true,
                );
                dispatch(ptr, Event::FileSelected { kind, path: None });
            }
        },
        Effect::SaveFile {
            kind,
            suggested_name,
        } => match choose_file(hwnd, kind, Some(&suggested_name), language) {
            Ok(path) => dispatch(ptr, Event::FileSelected { kind, path }),
            Err(error) => {
                show_message(
                    hwnd,
                    &language.translate("Не удалось открыть диалог"),
                    &language.translate(&error),
                    true,
                );
                dispatch(ptr, Event::FileSelected { kind, path: None });
            }
        },
        Effect::Confirm { id, title, message } => {
            let title = wide(&title);
            let message = wide(&message);
            // YES/NO/CANCEL, question icon, Cancel initially focused.
            let result = MessageBoxW(hwnd, message.as_ptr(), title.as_ptr(), 0x0000_0223);
            let answer = match result {
                6 => Answer::Yes,
                7 => Answer::No,
                _ => Answer::Cancel,
            };
            dispatch(ptr, Event::Confirmed { id, answer });
        }
        Effect::Message {
            title,
            message,
            error,
        } => show_message(hwnd, &title, &message, error),
        Effect::Prompt {
            id,
            title,
            label,
            value,
        } => {
            // Copy the preference before entering the modal message loop;
            // no borrow of the application survives the native call.
            let font = (*ptr)
                .app
                .as_ref()
                .map(|app| app.ui_font())
                .unwrap_or_default();
            let font_scale = (*ptr)
                .app
                .as_ref()
                .map(|app| app.ui_font_scale())
                .unwrap_or(1.0);
            let bundled = (*ptr).fonts.availability.clone();
            match prompt(
                hwnd, &title, &label, &value, font, font_scale, language, bundled,
            ) {
                Ok(value) => dispatch(ptr, Event::Prompted { id, value }),
                Err(error) => {
                    show_message(
                        hwnd,
                        &language.translate("Не удалось открыть поле ввода"),
                        &language.translate(&error),
                        true,
                    );
                    dispatch(ptr, Event::Prompted { id, value: None });
                }
            }
        }
        Effect::FocusCanvasText => {
            let input = (*ptr).canvas_input.hwnd;
            if !input.is_null() {
                SetFocus(input);
            }
        }
        Effect::CopyText(text) => {
            if let Err(error) = copy_text(hwnd, &text) {
                show_message(
                    hwnd,
                    &language.translate("Буфер обмена"),
                    &language.translate(&error),
                    true,
                );
            }
        }
        Effect::PasteText => {
            if let Some(text) = paste_text(hwnd) {
                dispatch(ptr, Event::Paste(text));
            }
        }
        Effect::Quit => {
            DestroyWindow(hwnd);
        }
    }
}

unsafe extern "system" fn window_proc(
    hwnd: Hwnd,
    message: u32,
    wparam: Wparam,
    lparam: Lparam,
) -> Lresult {
    // The ABI callback never unwinds into Windows. Fatal errors stop this window
    // while the core's last autosaved or manually saved document remains on disk.
    match catch_unwind(AssertUnwindSafe(|| {
        window_proc_inner(hwnd, message, wparam, lparam)
    })) {
        Ok(result) => result,
        Err(_) => {
            let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut WindowState;
            if !ptr.is_null() {
                (*ptr).fatal_error = Some("Внутренняя ошибка интерфейса. Перезапустите Portfolio и откройте последний сохранённый проект.".into());
            }
            PostQuitMessage(1);
            0
        }
    }
}

struct FileDrop(Handle);
impl Drop for FileDrop {
    fn drop(&mut self) {
        unsafe {
            DragFinish(self.0);
        }
    }
}

unsafe fn dropped_paths(handle: Handle) -> Result<Vec<PathBuf>, String> {
    let _drop = FileDrop(handle);
    let count = DragQueryFileW(handle, u32::MAX, null_mut(), 0);
    if count > 32 {
        return Err("За один раз можно добавить до 32 изображений.".into());
    }
    let mut paths = Vec::new();
    for index in 0..count {
        let length = DragQueryFileW(handle, index, null_mut(), 0);
        if length == 0 || length > 32767 {
            return Err("Не удалось прочитать путь перетаскиваемого файла.".into());
        }
        let mut path = vec![0u16; length as usize + 1];
        let copied = DragQueryFileW(handle, index, path.as_mut_ptr(), path.len() as u32);
        if copied != length {
            return Err("Не удалось прочитать путь перетаскиваемого файла.".into());
        }
        paths.push(PathBuf::from(OsString::from_wide(&path[..length as usize])));
    }
    Ok(paths)
}

unsafe fn window_proc_inner(hwnd: Hwnd, message: u32, wparam: Wparam, lparam: Lparam) -> Lresult {
    if message == WM_NCCREATE {
        let creation = &*(lparam as *const CreateStruct);
        let ptr = creation.create_params as *mut WindowState;
        if ptr.is_null() {
            return 0;
        }
        (*ptr).hwnd = hwnd;
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, ptr as isize);
        return 1;
    }
    let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut WindowState;
    if ptr.is_null() {
        return DefWindowProcW(hwnd, message, wparam, lparam);
    }
    match message {
        0x0111 if lparam != 0 && lparam as Hwnd == (*ptr).canvas_input.hwnd => {
            if wparam >> 16 == 0x0300 {
                text_editor::changed(ptr);
            } // EN_CHANGE
            0
        }
        WM_DROPFILES => {
            // The HDROP guard finishes before app dispatch or a modal message.
            match dropped_paths(wparam as Handle) {
                Ok(paths) if !paths.is_empty() => dispatch(ptr, Event::FilesDropped { paths }),
                Ok(_) => {}
                Err(error) => {
                    let language = locale(ptr);
                    show_message(
                        hwnd,
                        &language.translate("Импорт изображений"),
                        &language.translate(&error),
                        true,
                    );
                }
            }
            0
        }
        WM_SETTINGCHANGE => {
            notify_motion_preference(ptr);
            0
        }
        WM_PAINT => {
            paint(ptr);
            0
        }
        WM_ERASEBKGND => 1,
        WM_SIZE => {
            if wparam != 1 {
                notify_resize(ptr);
            }
            0
        }
        WM_GETMINMAXINFO => {
            let limits = &mut *(lparam as *mut MinMaxInfo);
            let scale = (*ptr).scale;
            limits.min_track_size = Point {
                x: (900.0 * scale) as i32,
                y: (620.0 * scale) as i32,
            };
            0
        }
        WM_DPICHANGED => {
            (*ptr).scale = ((wparam & 0xffff) as f32 / 96.0).max(0.5);
            let rect = *(lparam as *const Rect);
            SetWindowPos(
                hwnd,
                null_mut(),
                rect.left,
                rect.top,
                rect.right - rect.left,
                rect.bottom - rect.top,
                0x0014,
            );
            notify_resize(ptr);
            0
        }
        WM_MOUSEMOVE => {
            let scale = (*ptr).scale;
            dispatch(
                ptr,
                Event::MouseMove {
                    x: signed_low(lparam) as f32 / scale,
                    y: signed_high(lparam) as f32 / scale,
                    modifiers: modifiers(),
                },
            );
            0
        }
        WM_LBUTTONDOWN | WM_MBUTTONDOWN | WM_RBUTTONDOWN => {
            let (button, mask) = match message {
                WM_LBUTTONDOWN => (MouseButton::Left, 1),
                WM_MBUTTONDOWN => (MouseButton::Middle, 2),
                _ => (MouseButton::Right, 4),
            };
            (*ptr).captured_buttons |= mask;
            SetFocus(hwnd);
            SetCapture(hwnd);
            let scale = (*ptr).scale;
            dispatch(
                ptr,
                Event::MouseDown {
                    x: signed_low(lparam) as f32 / scale,
                    y: signed_high(lparam) as f32 / scale,
                    button,
                    modifiers: modifiers(),
                },
            );
            0
        }
        WM_LBUTTONUP | WM_MBUTTONUP | WM_RBUTTONUP => {
            let (button, mask) = match message {
                WM_LBUTTONUP => (MouseButton::Left, 1),
                WM_MBUTTONUP => (MouseButton::Middle, 2),
                _ => (MouseButton::Right, 4),
            };
            // Clear the completed button before the application can open a
            // modal dialog, which may synchronously transfer pointer capture.
            (*ptr).captured_buttons &= !mask;
            let scale = (*ptr).scale;
            dispatch(
                ptr,
                Event::MouseUp {
                    x: signed_low(lparam) as f32 / scale,
                    y: signed_high(lparam) as f32 / scale,
                    button,
                    modifiers: modifiers(),
                },
            );
            // Clear before ReleaseCapture; its synchronous notification must not
            // commit or cancel the same editing transaction for a second time.
            if (*ptr).captured_buttons == 0 && GetCapture() == hwnd {
                ReleaseCapture();
            }
            0
        }
        WM_CAPTURECHANGED | WM_CANCELMODE => {
            if (*ptr).captured_buttons != 0 {
                (*ptr).captured_buttons = 0;
                dispatch(ptr, Event::CaptureLost);
            }
            if message == WM_CANCELMODE && GetCapture() == hwnd {
                ReleaseCapture();
            }
            0
        }
        WM_MOUSEWHEEL => {
            let mut point = Point {
                x: signed_low(lparam),
                y: signed_high(lparam),
            };
            ScreenToClient(hwnd, &mut point);
            let scale = (*ptr).scale;
            dispatch(
                ptr,
                Event::Wheel {
                    x: point.x as f32 / scale,
                    y: point.y as f32 / scale,
                    delta: ((wparam >> 16) as u16 as i16) as f32 / 120.0,
                    modifiers: modifiers(),
                },
            );
            0
        }
        WM_KEYDOWN | WM_KEYUP | WM_SYSKEYDOWN | WM_SYSKEYUP => {
            dispatch(
                ptr,
                Event::Key {
                    code: wparam as u32,
                    pressed: message == WM_KEYDOWN || message == WM_SYSKEYDOWN,
                    repeat: lparam & (1 << 30) != 0,
                    modifiers: modifiers(),
                },
            );
            if message == WM_SYSKEYDOWN || message == WM_SYSKEYUP {
                DefWindowProcW(hwnd, message, wparam, lparam)
            } else {
                0
            }
        }
        WM_CHAR => {
            let unit = wparam as u16;
            if (0xd800..=0xdbff).contains(&unit) {
                (*ptr).high_surrogate = Some(unit);
            } else {
                let scalar = if (0xdc00..=0xdfff).contains(&unit) {
                    (*ptr).high_surrogate.take().map(|high| {
                        0x1_0000 + (((high as u32 - 0xd800) << 10) | (unit as u32 - 0xdc00))
                    })
                } else {
                    (*ptr).high_surrogate = None;
                    Some(unit as u32)
                };
                if let Some(ch) = scalar
                    .and_then(char::from_u32)
                    .filter(|ch| !ch.is_control())
                {
                    dispatch(ptr, Event::Text(ch.to_string()));
                }
            }
            0
        }
        WM_UNICHAR => {
            if wparam == 0xffff {
                return 1;
            }
            if let Some(ch) = char::from_u32(wparam as u32).filter(|ch| !ch.is_control()) {
                dispatch(ptr, Event::Text(ch.to_string()));
            }
            0
        }
        WM_SYSCHAR => 0,
        WM_KILLFOCUS => {
            (*ptr).high_surrogate = None;
            (*ptr).captured_buttons = 0;
            if GetCapture() == hwnd {
                ReleaseCapture();
            }
            if wparam as Hwnd != (*ptr).canvas_input.hwnd || wparam == 0 {
                dispatch(ptr, Event::FocusLost);
            }
            0
        }
        WM_TIMER => {
            if wparam == 1 {
                dispatch(ptr, Event::Tick);
            }
            0
        }
        WM_CLOSE => {
            dispatch(ptr, Event::CloseRequested);
            0
        }
        WM_DESTROY => {
            KillTimer(hwnd, 1);
            PostQuitMessage(0);
            0
        }
        WM_NCDESTROY => {
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
            (*ptr).hwnd = null_mut();
            DefWindowProcW(hwnd, message, wparam, lparam)
        }
        _ => DefWindowProcW(hwnd, message, wparam, lparam),
    }
}

struct PaintGuard {
    hwnd: Hwnd,
    paint: PaintStruct,
}
struct DcStateGuard {
    dc: Hdc,
    state: i32,
}
impl Drop for DcStateGuard {
    fn drop(&mut self) {
        unsafe {
            RestoreDC(self.dc, self.state);
        }
    }
}
impl Drop for PaintGuard {
    fn drop(&mut self) {
        unsafe {
            EndPaint(self.hwnd, &self.paint);
        }
    }
}
unsafe fn paint(ptr: *mut WindowState) {
    let hwnd = (*ptr).hwnd;
    let mut guard = PaintGuard {
        hwnd,
        paint: zeroed(),
    };
    let target = BeginPaint(hwnd, &mut guard.paint);
    if target.is_null() {
        return;
    }
    let should_render = (*ptr).surface.is_none() || (*ptr).pacer.due(Instant::now());
    let mut error = None;
    if should_render {
        (*ptr).pacer.begin_frame(Instant::now());
        let frame = match (*ptr).app.as_mut() {
            Some(app) => app.render(),
            None => return,
        };
        let length = frame.width as usize * frame.height as usize;
        let valid = frame.width > 0
            && frame.height > 0
            && bounded_frame_size(frame.width, frame.height) == (frame.width, frame.height)
            && frame.pixels.len() == length;
        if valid {
            let replace = (*ptr)
                .surface
                .as_ref()
                .is_none_or(|s| s.width != frame.width || s.height != frame.height);
            let size = (frame.width, frame.height);
            if replace && (*ptr).surface_failure != Some(size) {
                match Surface::new(frame.width, frame.height) {
                    Ok(surface) => {
                        (*ptr).surface = Some(surface);
                        (*ptr).surface_failure = None;
                    }
                    Err(message) => {
                        (*ptr).surface_failure = Some(size);
                        error = Some(message);
                    }
                }
            }
            if (*ptr)
                .surface
                .as_ref()
                .is_some_and(|s| s.width == frame.width && s.height == frame.height)
            {
                let state = &mut *ptr;
                let language = state
                    .app
                    .as_ref()
                    .map(|app| app.preferred_language())
                    .unwrap_or("ru");
                let surface = state.surface.as_ref().unwrap();
                GdiFlush();
                state.fonts.begin_frame();
                std::ptr::copy_nonoverlapping(frame.pixels.as_ptr(), surface.bits, length);
                let saved = SaveDC(surface.dc);
                if saved != 0 {
                    let _dc_state = DcStateGuard {
                        dc: surface.dc,
                        state: saved,
                    };
                    SetBkMode(surface.dc, 1);
                    let mut active_font: Handle = null_mut();
                    let mut active_color = None;
                    let mut active_clip: Option<Option<(i32, i32, i32, i32)>> = None;
                    let mut clip_visible = true;
                    for run in frame.texts {
                        if run.text.is_empty() || run.width <= 0.0 || run.height <= 0.0 {
                            continue;
                        }
                        if run.clip.is_some_and(|(_, _, w, h)| w <= 0.0 || h <= 0.0) {
                            continue;
                        }
                        let clip = run.clip.map(|(x, y, w, h)| {
                            (
                                (x * state.scale).floor() as i32,
                                (y * state.scale).floor() as i32,
                                ((x + w) * state.scale).ceil() as i32,
                                ((y + h) * state.scale).ceil() as i32,
                            )
                        });
                        if active_clip != Some(clip) {
                            // Adjacent labels usually share a parent clip. Reset
                            // it only when that parent actually changes.
                            active_clip = None;
                            if SelectClipRgn(surface.dc, null_mut()) == 0 {
                                continue;
                            }
                            clip_visible = true;
                            if let Some((left, top, right, bottom)) = clip {
                                let kind = IntersectClipRect(surface.dc, left, top, right, bottom);
                                if kind == 0 {
                                    continue;
                                }
                                clip_visible = kind != 1; // NULLREGION
                            }
                            active_clip = Some(clip);
                        }
                        if !clip_visible {
                            continue;
                        }
                        let font = state.fonts.get(
                            surface.dc,
                            (run.size * state.scale).round().max(1.0) as i32,
                            run.bold,
                            run.serif,
                            run.font,
                            &run.text,
                            language,
                        );
                        if font.is_null() {
                            continue;
                        }
                        if font != active_font {
                            let previous = SelectObject(surface.dc, font);
                            if previous.is_null() || previous == -1isize as Handle {
                                continue;
                            }
                            active_font = font;
                        }
                        if active_color != Some(run.color) {
                            SetTextColor(surface.dc, rgb_to_colorref(run.color));
                            active_color = Some(run.color);
                        }
                        let mut rect = Rect {
                            left: (run.x * state.scale).round() as i32,
                            top: (run.y * state.scale).round() as i32,
                            right: ((run.x + run.width) * state.scale).round() as i32,
                            bottom: ((run.y + run.height) * state.scale).round() as i32,
                        };
                        state.text_scratch.clear();
                        state.text_scratch.extend(run.text.encode_utf16());
                        let align = match run.align {
                            TextAlign::Left => 0,
                            TextAlign::Center => 1,
                            TextAlign::Right => 2,
                        };
                        DrawTextW(
                            surface.dc,
                            state.text_scratch.as_ptr(),
                            state.text_scratch.len() as i32,
                            &mut rect,
                            align | 0x24 | 0x8000 | 0x0800,
                        );
                    }
                }
            }
        }
    }
    // Exposure can reuse the last completed framebuffer without calling render.
    if let Some(surface) = (*ptr).surface.as_ref() {
        BitBlt(
            target,
            0,
            0,
            surface.width as i32,
            surface.height as i32,
            surface.dc,
            0,
            0,
            0x00cc_0020,
        );
    }
    drop(guard);
    if !(*ptr).canvas_input.hwnd.is_null() {
        InvalidateRect((*ptr).canvas_input.hwnd, null(), 0);
    }
    sync_timer(ptr);
    if let Some(error) = error {
        let language = locale(ptr);
        show_message(
            hwnd,
            &language.translate("Не удалось обновить окно"),
            &language.translate(&error),
            true,
        );
    }
}

unsafe fn choose_file(
    hwnd: Hwnd,
    kind: FileKind,
    save_name: Option<&str>,
    language: Language,
) -> Result<Option<PathBuf>, String> {
    let (filter, extension, open_title, save_title) = match kind {
        FileKind::Project => (
            "Проекты Portfolio (*.portfolio)\0*.portfolio\0Все файлы\0*.*\0\0",
            "portfolio",
            "Открыть проект Portfolio",
            "Сохранить проект Portfolio",
        ),
        FileKind::Image => (
            "Изображения (*.png;*.jpg;*.jpeg;*.bmp)\0*.png;*.jpg;*.jpeg;*.bmp\0Все файлы\0*.*\0\0",
            "png",
            "Импортировать изображение",
            "Сохранить изображение",
        ),
        FileKind::Png => (
            "Изображение PNG (*.png)\0*.png\0\0",
            "png",
            "Открыть PNG",
            "Экспортировать PNG",
        ),
    };
    let filter: Vec<u16> = language.translate(filter).encode_utf16().collect();
    let extension = wide(extension);
    let title = wide(&language.translate(if save_name.is_some() {
        save_title
    } else {
        open_title
    }));
    let mut file = vec![0u16; 32_768];
    if let Some(name) = save_name {
        for (target, source) in file
            .iter_mut()
            .take(32_766)
            .zip(name.encode_utf16().filter(|&c| c != 0))
        {
            *target = source;
        }
    }
    let mut ofn: OpenFileName = zeroed();
    ofn.struct_size = size_of::<OpenFileName>() as u32;
    ofn.owner = hwnd;
    ofn.filter = filter.as_ptr();
    ofn.filter_index = 1;
    ofn.file = file.as_mut_ptr();
    ofn.max_file = file.len() as u32;
    ofn.title = title.as_ptr();
    ofn.default_extension = extension.as_ptr();
    // EXPLORER | NOCHANGEDIR | PATHMUSTEXIST; saving checks overwrite, opening checks existence.
    ofn.flags = 0x0008_0000
        | 0x0000_0008
        | 0x0000_0800
        | if save_name.is_some() {
            0x0000_0002
        } else {
            0x0000_1000
        };
    let result = if save_name.is_some() {
        GetSaveFileNameW(&mut ofn)
    } else {
        GetOpenFileNameW(&mut ofn)
    };
    if result == 0 {
        let error = CommDlgExtendedError();
        return if error == 0 {
            Ok(None)
        } else {
            Err(format!(
                "Диалог Windows вернул ошибку {error:#x}. Повторите действие."
            ))
        };
    }
    let end = file.iter().position(|&ch| ch == 0).unwrap_or(file.len());
    Ok(Some(PathBuf::from(OsString::from_wide(&file[..end]))))
}

// The numeric/HEX/layer-name editor uses an actual Windows EDIT control. Native
// text selection, IME, clipboard, accessibility and dialog navigation stay with
// Windows instead of being approximated by the hand-drawn editor surface.
struct PromptState {
    initial: String,
    result: Option<String>,
}

fn push_word(bytes: &mut Vec<u8>, value: u16) {
    bytes.extend_from_slice(&value.to_le_bytes());
}
fn push_dword(bytes: &mut Vec<u8>, value: u32) {
    bytes.extend_from_slice(&value.to_le_bytes());
}
fn push_utf16(bytes: &mut Vec<u8>, value: &str) {
    for unit in value
        .encode_utf16()
        .filter(|&unit| unit != 0)
        .chain(Some(0))
    {
        push_word(bytes, unit);
    }
}
fn dialog_item(
    bytes: &mut Vec<u8>,
    id: u16,
    class: u16,
    title: &str,
    style: u32,
    bounds: (u16, u16, u16, u16),
) {
    while bytes.len() % 4 != 0 {
        bytes.push(0);
    }
    push_dword(bytes, style);
    push_dword(bytes, 0);
    for value in [bounds.0, bounds.1, bounds.2, bounds.3, id] {
        push_word(bytes, value);
    }
    push_word(bytes, 0xffff);
    push_word(bytes, class);
    push_utf16(bytes, title);
    push_word(bytes, 0);
}
unsafe fn prompt(
    hwnd: Hwnd,
    title: &str,
    label: &str,
    value: &str,
    family: UiFont,
    font_scale: f32,
    language: Language,
    registered: HashMap<&'static str, bool>,
) -> Result<Option<String>, String> {
    // DLGTEMPLATE uses WORD alignment internally, with each child DWORD aligned.
    // Store the completed bytes in u32 words so the OS receives aligned memory.
    let mut face = "Segoe UI".to_string();
    let apply = language.translate("Применить");
    let cancel = language.translate("Отмена");
    let dc = CreateCompatibleDC(null_mut());
    if !dc.is_null() {
        let mut fonts = Fonts {
            availability: registered,
            handles: HashMap::new(),
            choices: FontChoiceCache::default(),
            glyph_scratch: Vec::new(),
        };
        let sample = format!("{label} {value} {apply} {cancel}");
        let handle = fonts.get(dc, 16, false, false, family, &sample, language.key());
        if !handle.is_null() {
            let previous = SelectObject(dc, handle);
            let mut name = [0u16; 64];
            if GetTextFaceW(dc, name.len() as i32, name.as_mut_ptr()) > 0 {
                let end = name.iter().position(|c| *c == 0).unwrap_or(name.len());
                face = String::from_utf16_lossy(&name[..end]);
            }
            SelectObject(dc, previous);
        }
        DeleteDC(dc);
    }
    let base_points = if family.is_calligraphic() && face == family.family() {
        12.0
    } else {
        9.0
    };
    let scale = if font_scale.is_finite() {
        font_scale.clamp(0.75, 2.0)
    } else {
        1.0
    };
    let point_size = (base_points * scale).round().max(7.0) as u16;
    let mut template = Vec::new();
    push_dword(&mut template, 0x80c8_08c0); // popup, caption, sysmenu, modal frame, font, center
    push_dword(&mut template, 0);
    for value in [4, 0, 0, 324, 114] {
        push_word(&mut template, value);
    }
    push_word(&mut template, 0); // no menu
    push_word(&mut template, 0); // default dialog class
    push_utf16(&mut template, title);
    push_word(&mut template, point_size);
    push_utf16(&mut template, &face);
    dialog_item(
        &mut template,
        1000,
        0x0082,
        label,
        0x5000_0000,
        (12, 10, 300, 30),
    );
    dialog_item(
        &mut template,
        1001,
        0x0081,
        "",
        0x5081_0080,
        (12, 43, 300, 21),
    );
    dialog_item(
        &mut template,
        1,
        0x0080,
        &apply,
        0x5001_0001,
        (178, 82, 64, 20),
    );
    dialog_item(
        &mut template,
        2,
        0x0080,
        &cancel,
        0x5001_0000,
        (248, 82, 64, 20),
    );
    while template.len() % 4 != 0 {
        template.push(0);
    }
    let aligned: Vec<u32> = template
        .chunks_exact(4)
        .map(|bytes| u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
        .collect();
    let mut state = PromptState {
        initial: value.to_owned(),
        result: None,
    };
    let result = DialogBoxIndirectParamW(
        GetModuleHandleW(null()),
        aligned.as_ptr().cast(),
        hwnd,
        Some(prompt_proc),
        (&mut state as *mut PromptState) as isize,
    );
    if result == -1 {
        return Err(format!(
            "Windows не смог открыть поле ввода ({}).",
            GetLastError()
        ));
    }
    Ok(if result == 1 { state.result } else { None })
}
unsafe extern "system" fn prompt_proc(
    hwnd: Hwnd,
    message: u32,
    wparam: Wparam,
    lparam: Lparam,
) -> Lresult {
    const DWLP_USER: i32 = (2 * size_of::<isize>()) as i32;
    let result = catch_unwind(AssertUnwindSafe(|| {
        if message == 0x0110 {
            // WM_INITDIALOG
            let ptr = lparam as *mut PromptState;
            SetWindowLongPtrW(hwnd, DWLP_USER, ptr as isize);
            let edit = GetDlgItem(hwnd, 1001);
            let value = wide(&(*ptr).initial);
            SetWindowTextW(edit, value.as_ptr());
            SendMessageW(edit, 0x00c5, 4096, 0); // EM_SETLIMITTEXT
            SendMessageW(edit, 0x00b1, 0, -1); // EM_SETSEL
            SetFocus(edit);
            return 0;
        }
        let ptr = GetWindowLongPtrW(hwnd, DWLP_USER) as *mut PromptState;
        if ptr.is_null() {
            return 0;
        }
        if message == 0x0111 {
            // WM_COMMAND
            match (wparam & 0xffff) as u16 {
                1 => {
                    let edit = GetDlgItem(hwnd, 1001);
                    let length = GetWindowTextLengthW(edit).clamp(0, 4096) as usize;
                    let mut value = vec![0u16; length + 1];
                    let actual = GetWindowTextW(edit, value.as_mut_ptr(), value.len() as i32).max(0)
                        as usize;
                    (*ptr).result = Some(String::from_utf16_lossy(&value[..actual]));
                    EndDialog(hwnd, 1);
                    return 1;
                }
                2 => {
                    EndDialog(hwnd, 0);
                    return 1;
                }
                _ => {}
            }
        } else if message == WM_CLOSE {
            EndDialog(hwnd, 0);
            return 1;
        }
        0
    }));
    match result {
        Ok(value) => value,
        Err(_) => {
            EndDialog(hwnd, -1);
            1
        }
    }
}

struct ClipboardGuard;
impl Drop for ClipboardGuard {
    fn drop(&mut self) {
        unsafe {
            CloseClipboard();
        }
    }
}
unsafe fn copy_text(hwnd: Hwnd, text: &str) -> Result<(), String> {
    if OpenClipboard(hwnd) == 0 {
        return Err("Буфер обмена занят другим приложением. Повторите копирование.".into());
    }
    let _guard = ClipboardGuard;
    let text = wide(text);
    let memory = GlobalAlloc(0x0002, text.len() * 2);
    if memory.is_null() {
        return Err("Недостаточно памяти для копирования.".into());
    }
    let destination = GlobalLock(memory) as *mut u16;
    if destination.is_null() {
        GlobalFree(memory);
        return Err("Не удалось подготовить текст для копирования.".into());
    }
    std::ptr::copy_nonoverlapping(text.as_ptr(), destination, text.len());
    GlobalUnlock(memory);
    if EmptyClipboard() == 0 || SetClipboardData(13, memory).is_null() {
        GlobalFree(memory);
        return Err("Не удалось передать текст в буфер обмена.".into());
    }
    // Ownership transfers to Windows after SetClipboardData succeeds.
    Ok(())
}
unsafe fn paste_text(hwnd: Hwnd) -> Option<String> {
    if OpenClipboard(hwnd) == 0 {
        return None;
    }
    let _guard = ClipboardGuard;
    let memory = GetClipboardData(13);
    if memory.is_null() {
        return None;
    }
    let len = (GlobalSize(memory) / 2).min(1_048_576);
    let data = GlobalLock(memory) as *const u16;
    if data.is_null() {
        return None;
    }
    let units = std::slice::from_raw_parts(data, len);
    let end = units.iter().position(|&unit| unit == 0).unwrap_or(len);
    let text = String::from_utf16_lossy(&units[..end]);
    GlobalUnlock(memory);
    Some(text)
}

#[repr(C)]
struct GdiplusStartupInput {
    version: u32,
    debug_callback: Option<unsafe extern "system" fn(i32, *const i8)>,
    suppress_background: i32,
    suppress_external_codecs: i32,
}
#[repr(C)]
struct GpRect {
    x: i32,
    y: i32,
    width: i32,
    height: i32,
}
#[repr(C)]
struct BitmapData {
    width: u32,
    height: u32,
    stride: i32,
    pixel_format: i32,
    scan0: *mut c_void,
    reserved: usize,
}

// Keep ABI mistakes a compile error even when the Windows test runner is not
// available on the build host. These are the x64 Win32 SDK structure sizes.
#[cfg(target_pointer_width = "64")]
const _: () = {
    assert!(size_of::<OpenFileName>() == 152);
    assert!(size_of::<PaintStruct>() == 72);
    assert!(size_of::<WndClass>() == 72);
    assert!(size_of::<BitmapInfoHeader>() == 40);
    assert!(size_of::<BitmapData>() == 32);
    assert!(size_of::<Msg>() == 48);
    assert!(size_of::<CreateStruct>() == 80);
    assert!(size_of::<LogFont>() == 92);
};
#[link(name = "gdiplus")]
extern "system" {
    fn GdiplusStartup(
        token: *mut usize,
        input: *const GdiplusStartupInput,
        output: *mut c_void,
    ) -> i32;
    fn GdiplusShutdown(token: usize);
    fn GdipCreateBitmapFromFile(file: *const u16, bitmap: *mut Handle) -> i32;
    fn GdipGetImageWidth(image: Handle, width: *mut u32) -> i32;
    fn GdipGetImageHeight(image: Handle, height: *mut u32) -> i32;
    fn GdipBitmapLockBits(
        bitmap: Handle,
        rect: *const GpRect,
        flags: u32,
        format: i32,
        data: *mut BitmapData,
    ) -> i32;
    fn GdipBitmapUnlockBits(bitmap: Handle, data: *mut BitmapData) -> i32;
    fn GdipDisposeImage(image: Handle) -> i32;
}
struct GdiPlus(usize);
impl Drop for GdiPlus {
    fn drop(&mut self) {
        unsafe {
            GdiplusShutdown(self.0);
        }
    }
}
struct ImageHandle(Handle);
impl Drop for ImageHandle {
    fn drop(&mut self) {
        unsafe {
            GdipDisposeImage(self.0);
        }
    }
}
struct ImageLock {
    image: Handle,
    data: BitmapData,
}
impl Drop for ImageLock {
    fn drop(&mut self) {
        unsafe {
            GdipBitmapUnlockBits(self.image, &mut self.data);
        }
    }
}

/// Uses Windows' image codecs for PNG/JPEG/BMP; the returned pixels own their
/// memory, so no decoder handle or file lock escapes this function.
pub fn load_image(path: &Path) -> Result<ImportedImage, String> {
    unsafe {
        let input = GdiplusStartupInput {
            version: 1,
            debug_callback: None,
            suppress_background: 0,
            suppress_external_codecs: 1,
        };
        let mut token = 0;
        if GdiplusStartup(&mut token, &input, null_mut()) != 0 {
            return Err("Не удалось запустить системный декодер изображений.".into());
        }
        let _gdiplus = GdiPlus(token);
        let file: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        let mut raw_image = null_mut();
        if GdipCreateBitmapFromFile(file.as_ptr(), &mut raw_image) != 0 || raw_image.is_null() {
            return Err(
                "Не удалось прочитать изображение. Используйте корректный PNG, JPEG или BMP."
                    .into(),
            );
        }
        let image = ImageHandle(raw_image);
        let mut width = 0;
        let mut height = 0;
        if GdipGetImageWidth(image.0, &mut width) != 0
            || GdipGetImageHeight(image.0, &mut height) != 0
        {
            return Err("Не удалось определить размер изображения.".into());
        }
        if width == 0 || height == 0 || width > 4096 || height > 4096 {
            return Err("Изображение должно быть от 1×1 до 4096×4096 пикселей. Уменьшите его перед импортом.".into());
        }
        let rect = GpRect {
            x: 0,
            y: 0,
            width: width as i32,
            height: height as i32,
        };
        let mut data: BitmapData = zeroed();
        // PixelFormat32bppARGB: straight alpha, little-endian BGRA memory.
        if GdipBitmapLockBits(image.0, &rect, 1, 0x0026_200a, &mut data) != 0 {
            return Err("Системный декодер не смог получить пиксели изображения.".into());
        }
        let locked = ImageLock {
            image: image.0,
            data,
        };
        if locked.data.scan0.is_null() || locked.data.stride.unsigned_abs() < width * 4 {
            return Err("Декодер вернул некорректные данные изображения.".into());
        }
        let count = width as usize * height as usize;
        let mut pixels = Vec::new();
        pixels
            .try_reserve_exact(count)
            .map_err(|_| "Недостаточно памяти для импорта изображения.".to_string())?;
        for y in 0..height {
            // Scan0 is the first logical scan line; a signed stride also covers
            // a bottom-up decoder buffer without reversing the document.
            let row =
                (locked.data.scan0 as *const u8).offset(y as isize * locked.data.stride as isize);
            for x in 0..width {
                let p = row.add(x as usize * 4);
                pixels.push(crate::document::Color {
                    r: *p.add(2),
                    g: *p.add(1),
                    b: *p,
                    a: *p.add(3),
                });
            }
        }
        Ok(ImportedImage {
            width,
            height,
            pixels,
        })
    }
}

#[cfg(all(test, target_pointer_width = "64"))]
mod tests {
    use super::*;
    #[test]
    fn win32_abi_layouts_match_x64_sdk() {
        assert_eq!(size_of::<OpenFileName>(), 152);
        assert_eq!(size_of::<PaintStruct>(), 72);
        assert_eq!(size_of::<WndClass>(), 72);
        assert_eq!(size_of::<BitmapInfoHeader>(), 40);
        assert_eq!(size_of::<BitmapData>(), 32);
        assert_eq!(size_of::<Msg>(), 48);
        assert_eq!(size_of::<CreateStruct>(), 80);
        assert_eq!(size_of::<LogFont>(), 92);
    }
}
