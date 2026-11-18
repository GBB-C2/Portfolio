//! In-place Win32 text entry and glyph rasterization at document resolution.
use super::*;
use crate::document::{ParagraphAlign, TextLayer};

const EM_SETSEL: u32 = 0x00b1;
const EM_SETCHARFORMAT: u32 = 0x0400 + 68;
const EM_SETPARAFORMAT: u32 = 0x0400 + 71;

#[derive(Default)]
pub(super) struct CanvasInput {
    pub(super) hwnd: Hwnd,
    module: Handle,
    view: Option<CanvasEditView>,
    face: [u16; 32],
    syncing: bool,
}

impl Drop for CanvasInput {
    fn drop(&mut self) {
        // The main window destroys its child before its message loop exits.
        if !self.module.is_null() {
            unsafe {
                FreeLibrary(self.module);
            }
        }
    }
}

#[repr(C)]
struct CharFormat {
    bytes: u32,
    mask: u32,
    effects: u32,
    height: i32,
    offset: i32,
    color: u32,
    charset: u8,
    pitch: u8,
    face: [u16; 32],
}

#[repr(C)]
struct ParaFormat {
    bytes: u32,
    mask: u32,
    numbering: u16,
    reserved: u16,
    start: i32,
    end: i32,
    offset: i32,
    align: u16,
    tab_count: i16,
    tabs: [i32; 32],
}

unsafe fn read_body(hwnd: Hwnd) -> String {
    // Each logical paragraph break can be returned as a CR/LF pair.
    let mut buffer = [0u16; 8193];
    let used = GetWindowTextW(hwnd, buffer.as_mut_ptr(), buffer.len() as i32).max(0) as usize;
    String::from_utf16_lossy(&buffer[..used])
        .replace("\r\n", "\n")
        .replace('\r', "\n")
}

pub(super) unsafe fn changed(ptr: *mut WindowState) {
    if (*ptr).canvas_input.syncing {
        return;
    }
    if let Some(session) = (*ptr).canvas_input.view.as_ref().map(|v| v.session) {
        let body = read_body((*ptr).canvas_input.hwnd);
        dispatch(ptr, Event::CanvasTextChanged { session, body });
    }
}

pub(super) unsafe fn shortcut(ptr: *mut WindowState, msg: &Msg) -> bool {
    if msg.hwnd.is_null() || msg.hwnd != (*ptr).canvas_input.hwnd || msg.message != WM_KEYDOWN {
        return false;
    }
    // Let the IME handle Escape/Enter while the user is choosing a candidate.
    let m = modifiers();
    match msg.wparam {
        13 if m.ctrl => dispatch(ptr, Event::FinishCanvasText(true)),
        27 if !is_composing(msg.hwnd) => dispatch(ptr, Event::FinishCanvasText(false)),
        9 | 117 => {
            SetFocus((*ptr).hwnd);
            dispatch(ptr, Event::FocusTextSettings);
        }
        115 | 83 if msg.wparam == 115 || m.ctrl => dispatch(
            ptr,
            Event::Key {
                code: msg.wparam as u32,
                pressed: true,
                repeat: false,
                modifiers: m,
            },
        ),
        _ => return false,
    }
    true
}

#[link(name = "imm32")]
extern "system" {
    fn ImmGetContext(hwnd: Hwnd) -> Handle;
    fn ImmReleaseContext(hwnd: Hwnd, context: Handle) -> i32;
    fn ImmGetCompositionStringW(context: Handle, index: u32, buffer: *mut c_void, size: u32)
        -> i32;
}
unsafe fn is_composing(hwnd: Hwnd) -> bool {
    let ime = ImmGetContext(hwnd);
    if ime.is_null() {
        return false;
    }
    let composing = ImmGetCompositionStringW(ime, 8, null_mut(), 0) > 0;
    ImmReleaseContext(hwnd, ime);
    composing
}

unsafe fn make_input(ptr: *mut WindowState) -> Result<Hwnd, String> {
    if (*ptr).canvas_input.module.is_null() {
        (*ptr).canvas_input.module =
            LoadLibraryExW(wide("Msftedit.dll").as_ptr(), null_mut(), 0x800); // System32
    }
    if (*ptr).canvas_input.module.is_null() {
        return Err("Не удалось открыть поле ввода".into());
    }
    let child = CreateWindowExW(
        0x20,
        wide("RICHEDIT50W").as_ptr(),
        null(),                                          // WS_EX_TRANSPARENT
        0x4000_0000 | 0x0004 | 0x0040 | 0x0080 | 0x1000, // child, multiline, auto-scroll, want return
        0,
        0,
        1,
        1,
        (*ptr).hwnd,
        null_mut(),
        GetModuleHandleW(null()),
        null_mut(),
    );
    if child.is_null() {
        return Err("Не удалось открыть поле ввода".into());
    }
    SendMessageW(child, 0x0400 + 89, 1, 0); // EM_SETTEXTMODE, TM_PLAINTEXT
    SendMessageW(child, 0x0400 + 53, 0, 4096); // EM_EXLIMITTEXT, logical UTF-16 units
    SendMessageW(child, 0x0400 + 69, 0, 1); // EM_SETEVENTMASK, ENM_CHANGE
    SendMessageW(child, 0x0400 + 72, 0, 1); // EM_SETTARGETDEVICE: no automatic word wrap
    Ok(child)
}

unsafe fn paragraph(hwnd: Hwnd, align: ParagraphAlign) {
    let (mut start, mut end) = (0u32, 0u32);
    SendMessageW(
        hwnd,
        0x00b0,
        &mut start as *mut _ as usize,
        &mut end as *mut _ as isize,
    );
    SendMessageW(hwnd, EM_SETSEL, 0, -1);
    let mut format: ParaFormat = zeroed();
    format.bytes = size_of::<ParaFormat>() as u32;
    format.mask = 8; // PFM_ALIGNMENT
    format.align = match align {
        ParagraphAlign::Left => 1,
        ParagraphAlign::Right => 2,
        ParagraphAlign::Center => 3,
    };
    SendMessageW(hwnd, EM_SETPARAFORMAT, 0, &format as *const _ as isize);
    SendMessageW(hwnd, EM_SETSEL, start as usize, end as isize);
}

unsafe fn measure(
    ptr: *mut WindowState,
    view: &CanvasEditView,
) -> Result<([u16; 32], i32, i32), String> {
    let dc = GetDC((*ptr).hwnd);
    if dc.is_null() {
        return Err("Не удалось отрисовать текст.".into());
    }
    let language = locale(ptr);
    let font = style_font(dc, &mut (*ptr).fonts, &view.text, view.font_px, language);
    let measured = font.map(|(font, face)| {
        let previous = SelectObject(dc, font);
        let mut width = 0;
        let mut height: i32 = 0;
        for line in view.text.body.split('\n') {
            let units: Vec<u16> = if line.is_empty() { " " } else { line }
                .encode_utf16()
                .collect();
            let mut rect = Rect::default();
            DrawTextW(
                dc,
                units.as_ptr(),
                units.len() as i32,
                &mut rect,
                0x0400 | 0x0800 | 0x0020 | 0x0040,
            );
            width = width.max(rect.right);
            height = height.saturating_add(rect.bottom.max(1));
        }
        SelectObject(dc, previous);
        DeleteObject(font);
        (face, width, height)
    });
    ReleaseDC((*ptr).hwnd, dc);
    measured
}

/// Intersect the child with the canvas and subtract floating panels, also for hit testing.
unsafe fn clip_child(hwnd: Hwnd, rect: [i32; 4], view: &CanvasEditView) {
    let [x, y, w, h] = rect;
    let region = CreateRectRgn(0, 0, w, h);
    if region.is_null() {
        return;
    }
    for (r, mode) in std::iter::once((&view.clip, 1)).chain(view.panels.iter().map(|r| (r, 4))) {
        let part = CreateRectRgn(r[0] - x, r[1] - y, r[2] - x, r[3] - y);
        if !part.is_null() {
            CombineRgn(region, region, part, mode);
            DeleteObject(part);
        }
    }
    if SetWindowRgn(hwnd, region, 1) == 0 {
        DeleteObject(region);
    }
}

unsafe fn update_input(
    ptr: *mut WindowState,
    view: &CanvasEditView,
    initial: bool,
) -> Result<(), String> {
    let hwnd = (*ptr).canvas_input.hwnd;
    let (face, ink_w, ink_h) = measure(ptr, view)?;
    let dpi = GetDpiForWindow((*ptr).hwnd).max(96);
    let pad = ((dpi as f32 / 96.) * 3.).round() as i32;
    let (ax, ay) = view.anchor;
    let room = match view.text.align {
        ParagraphAlign::Left => view.clip[2] - ax,
        ParagraphAlign::Center => 2 * (view.clip[2] - ax).min(ax - view.clip[0]),
        ParagraphAlign::Right => ax - view.clip[0],
    };
    let max_width = (view.clip[2] - view.clip[0]).max(2 * pad + 2);
    // Keep a usable edit rectangle even at the edge; the window region clips its visible portion.
    let width = (ink_w + 2 * pad + 6)
        .max(72 * dpi as i32 / 96)
        .min(room.max(72).min(max_width));
    let height = (ink_h + 2 * pad + 2).min((view.clip[3] - ay + pad).max(view.font_px + 2 * pad));
    let x = ax
        - match view.text.align {
            ParagraphAlign::Left => pad,
            ParagraphAlign::Center => width / 2,
            ParagraphAlign::Right => width - pad,
        };
    let y = ay - pad;
    SetWindowPos(hwnd, null_mut(), x, y, width, height, 0x0010); // SWP_NOACTIVATE
    clip_child(hwnd, [x, y, width, height], view);
    let format_rect = Rect {
        left: pad,
        top: pad,
        right: (width - pad).max(pad + 1),
        bottom: (height - pad).max(pad + 1),
    };
    SendMessageW(hwnd, 0x00b4, 0, &format_rect as *const _ as isize); // EM_SETRECTNP

    let old = (*ptr).canvas_input.view.as_ref();
    let format_changed = initial
        || (*ptr).canvas_input.face != face
        || old.is_none_or(|old| {
            (old.font_px, old.text.bold, old.text.italic, old.text.color)
                != (
                    view.font_px,
                    view.text.bold,
                    view.text.italic,
                    view.text.color,
                )
        });
    let align_changed = initial || old.is_none_or(|old| old.text.align != view.text.align);
    if format_changed {
        let format = CharFormat {
            bytes: size_of::<CharFormat>() as u32,
            mask: 0xe0000003,
            effects: u32::from(view.text.bold) | (u32::from(view.text.italic) << 1),
            height: ((view.font_px * 1440 + dpi as i32 / 2) / dpi as i32).max(1),
            offset: 0,
            color: rgb_to_colorref(crate::render::rgb(view.text.color)),
            charset: 1,
            pitch: 0,
            face,
        };
        SendMessageW(hwnd, EM_SETCHARFORMAT, 4, &format as *const _ as isize); // SCF_ALL
        (*ptr).canvas_input.face = face;
    }
    if align_changed {
        paragraph(hwnd, view.text.align);
    }
    if initial {
        let end = GetWindowTextLengthW(hwnd).max(0);
        SendMessageW(hwnd, EM_SETSEL, end as usize, end as isize);
        ShowWindow(hwnd, 5);
        SetFocus(hwnd);
    }
    InvalidateRect(hwnd, null(), 0);
    Ok(())
}

pub(super) unsafe fn sync(ptr: *mut WindowState) {
    if (*ptr).canvas_input.syncing {
        return;
    }
    let next = (*ptr).app.as_ref().and_then(|app| app.canvas_editor());
    if next == (*ptr).canvas_input.view {
        return;
    }
    (*ptr).canvas_input.syncing = true;
    let initial =
        next.as_ref().map(|v| v.session) != (*ptr).canvas_input.view.as_ref().map(|v| v.session);
    if initial && !(*ptr).canvas_input.hwnd.is_null() {
        let previous = std::mem::replace(&mut (*ptr).canvas_input.hwnd, null_mut());
        let focused = GetFocus() == previous;
        DestroyWindow(previous);
        if focused {
            SetFocus((*ptr).hwnd);
        }
    }
    let result = if let Some(view) = &next {
        (|| {
            if initial {
                let hwnd = make_input(ptr)?;
                (*ptr).canvas_input.hwnd = hwnd;
                SetWindowTextW(hwnd, wide(&view.text.body.replace('\n', "\r\n")).as_ptr());
            }
            update_input(ptr, view, initial)
        })()
    } else {
        Ok(())
    };
    (*ptr).canvas_input.view = next;
    (*ptr).canvas_input.syncing = false;
    if let Err(error) = result {
        dispatch(ptr, Event::FinishCanvasText(false));
        let language = locale(ptr);
        show_message(
            (*ptr).hwnd,
            &language.translate("Текст на холсте"),
            &language.translate(&error),
            true,
        );
    }
}

unsafe fn style_font(
    dc: Hdc,
    fonts: &mut Fonts,
    text: &TextLayer,
    pixels: i32,
    language: Language,
) -> Result<(Handle, [u16; 32]), String> {
    let family = UiFont::from_key(&text.font).unwrap_or(UiFont::SegoeUi);
    let base = fonts.get(
        dc,
        pixels,
        text.bold,
        false,
        family,
        &text.body,
        language.key(),
    );
    if base.is_null() {
        return Err("Не удалось отрисовать текст.".into());
    }
    let previous = SelectObject(dc, base);
    if previous.is_null() || previous == -1isize as Handle {
        return Err("Не удалось отрисовать текст.".into());
    }
    let mut face = [0u16; 32];
    GetTextFaceW(dc, face.len() as i32, face.as_mut_ptr());
    SelectObject(dc, previous);
    let font = CreateFontW(
        -(pixels),
        0,
        0,
        0,
        if text.bold { 700 } else { 400 },
        text.italic as u32,
        0,
        0,
        1,
        0,
        0,
        4,
        0,
        face.as_ptr(),
    );
    if font.is_null() {
        return Err("Не удалось отрисовать текст.".into());
    }
    Ok((font, face))
}

pub(super) unsafe fn rasterize(
    text: &TextLayer,
    width: u32,
    height: u32,
    language: Language,
) -> Result<Vec<Color>, String> {
    let surface = Surface::new(width, height)?;
    let count = width as usize * height as usize;
    std::ptr::write_bytes(surface.bits, 0, count);
    let mut fonts = Fonts::default();
    let _bundled = register_bundled_fonts(&mut fonts);
    let (font, _) = style_font(surface.dc, &mut fonts, text, text.size as i32, language)?;
    if font.is_null() {
        return Err("Не удалось отрисовать текст.".into());
    }
    let previous = SelectObject(surface.dc, font);
    if previous.is_null() || previous == -1isize as Handle {
        DeleteObject(font);
        return Err("Не удалось отрисовать текст.".into());
    }
    SetBkMode(surface.dc, 1);
    SetTextColor(surface.dc, 0x00ff_ffff);
    let flags = 0x0800 | 0x0020 | 0x0040; // DT_NOPREFIX | DT_SINGLELINE | DT_EXPANDTABS
    let mut top = text.y as i32;
    for line in text.body.split('\n') {
        if top >= height as i32 {
            break;
        }
        let units: Vec<u16> = if line.is_empty() { " " } else { line }
            .encode_utf16()
            .collect();
        let mut rect = Rect::default();
        DrawTextW(
            surface.dc,
            units.as_ptr(),
            units.len() as i32,
            &mut rect,
            flags | 0x0400,
        );
        let advance = rect.right.max(1);
        let line_height = rect.bottom.max(1);
        let left = text.x as i32
            - match text.align {
                ParagraphAlign::Left => 0,
                ParagraphAlign::Center => advance / 2,
                ParagraphAlign::Right => advance,
            };
        rect = Rect {
            left,
            top,
            right: left + advance + text.size as i32,
            bottom: top + line_height,
        };
        DrawTextW(
            surface.dc,
            units.as_ptr(),
            units.len() as i32,
            &mut rect,
            flags,
        );
        top += line_height;
    }
    GdiFlush();
    SelectObject(surface.dc, previous);
    DeleteObject(font);
    let bits = std::slice::from_raw_parts(surface.bits, count);
    let pixels: Vec<_> = bits
        .iter()
        .map(|v| {
            let alpha = ((v & 255) * u32::from(text.color.a) + 127) / 255;
            if alpha == 0 {
                Color::TRANSPARENT
            } else {
                Color {
                    a: alpha as u8,
                    ..text.color
                }
            }
        })
        .collect();
    if !pixels.iter().any(|p| p.a != 0) {
        return Err("Текст выходит за границы холста. Измените координаты или размер.".into());
    }
    Ok(pixels)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_canvas_input_is_a_multiline_child_and_keeps_selection_when_restyled() {
        unsafe {
            let owner = CreateWindowExW(
                0,
                wide("STATIC").as_ptr(),
                null(),
                0x8000_0000,
                0,
                0,
                640,
                480,
                null_mut(),
                null_mut(),
                GetModuleHandleW(null()),
                null_mut(),
            );
            assert!(!owner.is_null());
            let mut state = WindowState::new(Box::new(crate::app::PortfolioApp::new(false)), 1.);
            state.hwnd = owner;
            let input = make_input(&mut state).unwrap();
            state.canvas_input.hwnd = input;
            assert_ne!(GetWindowLongPtrW(input, -16) & 0x4000_0000, 0); // WS_CHILD
            let mut view = CanvasEditView {
                session: 7,
                text: TextLayer {
                    body: String::new(),
                    font: UiFont::SegoeUi.key().into(),
                    size: 24,
                    color: Color::BLACK,
                    bold: false,
                    italic: false,
                    align: ParagraphAlign::Left,
                    x: 0,
                    y: 0,
                },
                anchor: (90, 40),
                font_px: 24,
                clip: [0, 0, 640, 480],
                panels: vec![[300, 0, 400, 150]],
            };
            update_input(&mut state, &view, true).unwrap();
            SendMessageW(
                input,
                0x00c2,
                1,
                wide("Москва 🌿\r\n東京").as_ptr() as isize,
            ); // EM_REPLACESEL
            assert_eq!(read_body(input), "Москва 🌿\n東京");
            view.text.body = read_body(input);
            state.canvas_input.view = Some(view.clone());
            SendMessageW(input, EM_SETSEL, 1, 5);
            view.text.bold = true;
            view.text.italic = true;
            view.text.align = ParagraphAlign::Right;
            update_input(&mut state, &view, false).unwrap();
            let selection = SendMessageW(input, 0x00b0, 0, 0) as u32;
            assert_eq!((selection & 0xffff, selection >> 16), (1, 5));
            assert_eq!(read_body(input), view.text.body);
            DestroyWindow(owner);
            assert_eq!(IsWindow(input), 0);
        }
    }

    #[test]
    fn native_glyphs_preserve_multiline_alignment_color_and_alpha() {
        let mut text = TextLayer {
            body: "Тип &\nAa".into(),
            font: UiFont::SegoeUi.key().into(),
            size: 24,
            color: Color::rgb(23, 45, 67),
            bold: false,
            italic: false,
            align: ParagraphAlign::Left,
            x: 75,
            y: 4,
        };
        let render =
            |text: &TextLayer| unsafe { rasterize(text, 190, 90, Language::Russian).unwrap() };
        let left = render(&text);
        assert!(left
            .iter()
            .filter(|p| p.a > 0)
            .all(|p| (p.r, p.g, p.b) == (23, 45, 67)));
        assert!(left[40 * 190..].iter().any(|p| p.a > 0));
        text.align = ParagraphAlign::Right;
        let right = render(&text);
        assert_ne!(left, right);
        assert!(right
            .chunks(190)
            .all(|row| row[80..].iter().all(|p| p.a == 0)));
        text.bold = true;
        text.italic = true;
        assert_ne!(render(&text), right);
    }
}
