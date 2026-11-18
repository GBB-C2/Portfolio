//! Win32 multiline type editor and grayscale glyph rasterization at document resolution.
use super::*;
use crate::document::{ParagraphAlign, TextLayer};

const BODY: u16 = 101;
const FONT: u16 = 102;
const SIZE: u16 = 103;
const COLOR: u16 = 104;
const BOLD: u16 = 105;
const ITALIC: u16 = 106;
const ALIGN: u16 = 107;
const X: u16 = 108;
const Y: u16 = 109;

struct Editor {
    text: TextLayer,
    width: u32,
    height: u32,
    language: Language,
    result: Option<(TextLayer, Vec<Color>)>,
}

pub(super) unsafe fn open(
    owner: Hwnd,
    text: TextLayer,
    width: u32,
    height: u32,
    language: Language,
) -> Result<Option<(TextLayer, Vec<Color>)>, String> {
    let mut bytes = Vec::new();
    push_dword(&mut bytes, 0x80c8_08c0);
    push_dword(&mut bytes, 0);
    for word in [18, 0, 0, 336, 222] {
        push_word(&mut bytes, word);
    }
    push_word(&mut bytes, 0);
    push_word(&mut bytes, 0);
    push_utf16(&mut bytes, &language.translate("Текст на холсте"));
    push_word(&mut bytes, 9);
    push_utf16(&mut bytes, "Segoe UI");

    let mut label = |id, title: &str, x, y, w| {
        dialog_item(
            &mut bytes,
            id,
            0x82,
            &language.translate(title),
            0x5000_0000,
            (x, y, w, 12),
        )
    };
    label(200, "Текст", 10, 7, 316);
    label(201, "Шрифт", 10, 100, 136);
    label(202, "Размер (px)", 154, 100, 73);
    label(203, "Цвет (HEX)", 236, 100, 90);
    label(204, "Выравнивание", 10, 158, 137);
    label(205, "X (px)", 154, 158, 73);
    label(206, "Y (px)", 236, 158, 90);
    // Multiline EDIT owns Enter, IME, selection, clipboard and ordinary text undo.
    dialog_item(&mut bytes, BODY, 0x81, "", 0x50a1_1044, (10, 21, 316, 72));
    let combo = 0x5021_0103; // dropdown list, tab stop, scrollbar, strings
    let field = 0x5081_0080; // bordered, horizontally scrolling edit
    dialog_item(&mut bytes, FONT, 0x85, "", combo, (10, 113, 136, 112));
    dialog_item(
        &mut bytes,
        SIZE,
        0x81,
        "",
        field | 0x2000,
        (154, 113, 73, 16),
    );
    dialog_item(&mut bytes, COLOR, 0x81, "", field, (236, 113, 90, 16));
    dialog_item(
        &mut bytes,
        BOLD,
        0x80,
        &language.translate("Полужирный"),
        0x5001_0003,
        (10, 135, 136, 16),
    );
    dialog_item(
        &mut bytes,
        ITALIC,
        0x80,
        &language.translate("Курсив"),
        0x5001_0003,
        (154, 135, 122, 16),
    );
    dialog_item(&mut bytes, ALIGN, 0x85, "", combo, (10, 171, 136, 68));
    dialog_item(&mut bytes, X, 0x81, "", field | 0x2000, (154, 171, 73, 16));
    dialog_item(&mut bytes, Y, 0x81, "", field | 0x2000, (236, 171, 90, 16));
    dialog_item(
        &mut bytes,
        1,
        0x80,
        &language.translate("Применить"),
        0x5001_0001,
        (177, 196, 72, 18),
    );
    dialog_item(
        &mut bytes,
        2,
        0x80,
        &language.translate("Отмена"),
        0x5001_0000,
        (254, 196, 72, 18),
    );
    bytes.resize(bytes.len().next_multiple_of(4), 0);
    let template: Vec<u32> = bytes
        .chunks_exact(4)
        .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
        .collect();
    let mut state = Editor {
        text,
        width,
        height,
        language,
        result: None,
    };
    let result = DialogBoxIndirectParamW(
        GetModuleHandleW(null()),
        template.as_ptr().cast(),
        owner,
        Some(editor_proc),
        (&mut state as *mut Editor) as isize,
    );
    if result == -1 {
        return Err("Не удалось открыть поле ввода".into());
    }
    Ok(state.result)
}

unsafe fn field(hwnd: Hwnd, id: u16) -> Hwnd {
    GetDlgItem(hwnd, id as i32)
}

unsafe fn read_field(hwnd: Hwnd, id: u16) -> String {
    let handle = field(hwnd, id);
    let len = GetWindowTextLengthW(handle).clamp(0, 8192) as usize;
    let mut value = vec![0u16; len + 1];
    let n = GetWindowTextW(handle, value.as_mut_ptr(), value.len() as i32).max(0) as usize;
    String::from_utf16_lossy(&value[..n])
}

unsafe fn initialize(hwnd: Hwnd, state: &Editor) {
    let t = &state.text;
    for (id, value) in [
        (BODY, t.body.replace('\n', "\r\n")),
        (SIZE, t.size.to_string()),
        (COLOR, t.color.to_hex()),
        (X, t.x.to_string()),
        (Y, t.y.to_string()),
    ] {
        SetWindowTextW(field(hwnd, id), wide(&value).as_ptr());
    }
    // Windows EDIT represents each stored newline as two UTF-16 units.
    SendMessageW(field(hwnd, BODY), 0x00c5, 8192, 0);
    for f in UiFont::ALL {
        SendMessageW(
            field(hwnd, FONT),
            0x0143,
            0,
            wide(f.label()).as_ptr() as isize,
        );
    }
    let font = UiFont::ALL
        .iter()
        .position(|f| f.key() == t.font)
        .unwrap_or(1);
    SendMessageW(field(hwnd, FONT), 0x014e, font, 0);
    for label in ["Слева", "По центру", "Справа"] {
        SendMessageW(
            field(hwnd, ALIGN),
            0x0143,
            0,
            wide(&state.language.translate(label)).as_ptr() as isize,
        );
    }
    SendMessageW(field(hwnd, ALIGN), 0x014e, t.align as usize, 0);
    for (id, on) in [(BOLD, t.bold), (ITALIC, t.italic)] {
        SendMessageW(field(hwnd, id), 0x00f1, usize::from(on), 0);
    }
    SendMessageW(field(hwnd, BODY), 0x00b1, 0, -1);
    SetFocus(field(hwnd, BODY));
}

unsafe fn read_text(hwnd: Hwnd, state: &Editor) -> Result<TextLayer, (u16, &'static str)> {
    let number = |id, lo, hi| {
        read_field(hwnd, id)
            .parse::<u32>()
            .ok()
            .filter(|n| (lo..=hi).contains(n))
            .ok_or((
                id,
                if id == SIZE {
                    "Введите размер от 4 до 512 px."
                } else {
                    "Координата должна находиться внутри холста."
                },
            ))
    };
    let index = SendMessageW(field(hwnd, FONT), 0x0147, 0, 0) as usize;
    let body = read_field(hwnd, BODY)
        .replace("\r\n", "\n")
        .replace('\r', "\n");
    let text = TextLayer {
        body,
        font: UiFont::ALL
            .get(index)
            .unwrap_or(&UiFont::SegoeUi)
            .key()
            .into(),
        size: number(SIZE, 4, 512)?,
        x: number(X, 0, state.width - 1)?,
        y: number(Y, 0, state.height - 1)?,
        color: Color::parse_hex(&read_field(hwnd, COLOR))
            .ok_or((COLOR, "Шесть символов HEX, например #365AD7"))?,
        bold: SendMessageW(field(hwnd, BOLD), 0x00f0, 0, 0) == 1,
        italic: SendMessageW(field(hwnd, ITALIC), 0x00f0, 0, 0) == 1,
        align: match SendMessageW(field(hwnd, ALIGN), 0x0147, 0, 0) {
            1 => ParagraphAlign::Center,
            2 => ParagraphAlign::Right,
            _ => ParagraphAlign::Left,
        },
    };
    text.validate(state.width, state.height)
        .map_err(|_| (BODY, "Введите текст длиной до 4096 знаков."))?;
    Ok(text)
}

unsafe extern "system" fn editor_proc(
    hwnd: Hwnd,
    message: u32,
    wparam: Wparam,
    lparam: Lparam,
) -> Lresult {
    const USER: i32 = (2 * size_of::<isize>()) as i32;
    catch_unwind(AssertUnwindSafe(|| {
        if message == 0x0110 {
            SetWindowLongPtrW(hwnd, USER, lparam);
            initialize(hwnd, &*(lparam as *const Editor));
            return 0;
        }
        let ptr = GetWindowLongPtrW(hwnd, USER) as *mut Editor;
        if ptr.is_null() {
            return 0;
        }
        if message == WM_CLOSE || (message == 0x0111 && wparam & 0xffff == 2) {
            EndDialog(hwnd, 0);
            return 1;
        }
        if message == 0x0111 && wparam & 0xffff == 1 {
            let language = (*ptr).language;
            let result = read_text(hwnd, &*ptr)
                .map_err(|(id, msg)| (id, msg.to_owned()))
                .and_then(|text| {
                    let pixels = rasterize(&text, (*ptr).width, (*ptr).height, language)
                        .map_err(|e| (BODY, e))?;
                    Ok((text, pixels))
                });
            match result {
                Ok(value) => {
                    (*ptr).result = Some(value);
                    EndDialog(hwnd, 1);
                }
                Err((id, msg)) => {
                    show_message(
                        hwnd,
                        &language.translate("Текст на холсте"),
                        &language.translate(&msg),
                        true,
                    );
                    SetFocus(field(hwnd, id));
                }
            }
            return 1;
        }
        0
    }))
    .unwrap_or_else(|_| {
        EndDialog(hwnd, 0);
        1
    })
}

unsafe fn rasterize(
    text: &TextLayer,
    width: u32,
    height: u32,
    language: Language,
) -> Result<Vec<Color>, String> {
    let surface = Surface::new(width, height)?;
    let count = width as usize * height as usize;
    std::ptr::write_bytes(surface.bits, 0, count);
    let mut fonts = Fonts::default();
    let family = UiFont::from_key(&text.font).unwrap_or(UiFont::SegoeUi);
    let base = fonts.get(
        surface.dc,
        text.size as i32,
        text.bold,
        false,
        family,
        &text.body,
        language.key(),
    );
    if base.is_null() {
        return Err("Не удалось отрисовать текст.".into());
    }
    let previous = SelectObject(surface.dc, base);
    if previous.is_null() || previous == -1isize as Handle {
        return Err("Не удалось отрисовать текст.".into());
    }
    let mut face = [0u16; 64];
    GetTextFaceW(surface.dc, face.len() as i32, face.as_mut_ptr());
    SelectObject(surface.dc, previous);
    let font = CreateFontW(
        -(text.size as i32),
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
