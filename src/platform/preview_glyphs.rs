//! Optional Linux design-preview glyphs. Windows uses GDI. Look up the system
//! Cairo library at runtime so other headless workflows need no native package.
use super::*;
use crate::lettering::{TextLayout, TextTile};
use std::{
    ffi::{c_char, c_int, c_void, CString},
    sync::OnceLock,
};
type Handle = *mut c_void;

#[link(name = "dl")]
extern "C" {
    fn dlopen(name: *const c_char, flags: c_int) -> Handle;
    fn dlsym(handle: Handle, name: *const c_char) -> Handle;
}
macro_rules! cairo_api {
    ($($name:ident($($arg:ty),*) -> $ret:ty),+ $(,)?) => {
        struct Cairo {
            $($name: unsafe extern "C" fn($($arg),*) -> $ret),+
        }
        impl Cairo {
            unsafe fn load() -> Option<Self> {
                let library = dlopen(c"libcairo.so.2".as_ptr(), 1);
                if library.is_null() {
                    return None;
                }
                Some(Self {
                    $($name: {
                        let symbol = concat!("cairo_", stringify!($name), "\0");
                        let address = dlsym(library, symbol.as_ptr().cast());
                        if address.is_null() {
                            return None;
                        }
                        std::mem::transmute::<Handle, unsafe extern "C" fn($($arg),*) -> $ret>(address)
                    }),+
                })
            }
        }
    };
}
#[repr(C)]
#[derive(Default)]
struct Extents {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    advance: f64,
    dy: f64,
}
#[repr(C)]
#[derive(Default)]
struct FontExtents {
    ascent: f64,
    descent: f64,
    height: f64,
    dx: f64,
    dy: f64,
}
cairo_api! {
    image_surface_create(c_int, c_int, c_int) -> Handle,
    surface_destroy(Handle) -> (),
    surface_status(Handle) -> c_int,
    create(Handle) -> Handle,
    destroy(Handle) -> (),
    select_font_face(Handle, *const c_char, c_int, c_int) -> (),
    set_font_size(Handle, f64) -> (),
    text_extents(Handle, *const c_char, *mut Extents) -> (),
    font_extents(Handle, *mut FontExtents) -> (),
    set_source_rgba(Handle, f64, f64, f64, f64) -> (),
    move_to(Handle, f64, f64) -> (),
    show_text(Handle, *const c_char) -> (),
    surface_flush(Handle) -> (),
    image_surface_get_data(Handle) -> *const u8,
    image_surface_get_stride(Handle) -> c_int,
}

fn cairo() -> Option<&'static Cairo> {
    static API: OnceLock<Option<Cairo>> = OnceLock::new();
    API.get_or_init(|| unsafe { Cairo::load() }).as_ref()
}

#[cfg(test)]
pub(crate) fn preview_fonts_available() -> bool {
    cairo().is_some()
}

struct Surface<'a> {
    api: &'a Cairo,
    surface: Handle,
    dc: Handle,
}
impl Drop for Surface<'_> {
    fn drop(&mut self) {
        unsafe {
            (self.api.destroy)(self.dc);
            (self.api.surface_destroy)(self.surface);
        }
    }
}
impl<'a> Surface<'a> {
    unsafe fn new(
        api: &'a Cairo,
        w: usize,
        h: usize,
        text: &TextLayer,
        language: crate::i18n::Language,
    ) -> Result<Self, String> {
        let surface = (api.image_surface_create)(2, w as i32, h as i32); // CAIRO_FORMAT_A8
        if (api.surface_status)(surface) != 0 {
            (api.surface_destroy)(surface);
            return Err("Не удалось отрисовать текст.".into());
        }
        let dc = (api.create)(surface);
        let font = UiFont::from_key(&text.font);
        let family = if text
            .body
            .chars()
            .any(|c| matches!(c as u32, 0x2e80..=0x9fff | 0xac00..=0xd7ff | 0xf900..=0xfaff))
        {
            match language {
                crate::i18n::Language::Korean => c"Noto Sans CJK KR",
                crate::i18n::Language::Japanese => c"Noto Sans CJK JP",
                _ => c"Noto Sans CJK SC",
            }
        } else {
            match font {
                Some(UiFont::Gabriola | UiFont::SegoeScript) => c"DejaVu Serif",
                Some(font) if font.is_bundled() => c"DejaVu Sans Mono",
                _ => c"DejaVu Sans",
            }
        };
        (api.select_font_face)(
            dc,
            family.as_ptr(),
            i32::from(text.italic || font == Some(UiFont::SegoeScript)),
            i32::from(text.bold),
        );
        (api.set_font_size)(dc, text.size as f64);
        Ok(Self { api, surface, dc })
    }
}
pub(crate) fn render_text_tile(
    text: &TextLayer,
    w: u32,
    h: u32,
    language: crate::i18n::Language,
    face: bool,
) -> Result<TextTile, String> {
    let api = cairo().ok_or("Не удалось отрисовать текст.")?;
    unsafe {
        let metrics = Surface::new(api, 1, 1, text, language)?;
        let mut font = FontExtents::default();
        (api.font_extents)(metrics.dc, &mut font);
        let layout = TextLayout::measure(text, w, h, |line| {
            let mut ext = Extents::default();
            if let Ok(line) = CString::new(line.replace('\t', "        ")) {
                (api.text_extents)(metrics.dc, line.as_ptr(), &mut ext);
            }
            (ext.advance.ceil() as i32, font.height.ceil() as i32)
        })
        .ok_or("Текст выходит за границы холста. Измените координаты или размер.")?;
        let tile = Surface::new(api, layout.w, layout.h, text, language)?;
        (api.set_source_rgba)(tile.dc, 1., 1., 1., 1.);
        for &(line, x, y) in &layout.lines {
            let line = CString::new(line.replace('\t', "        "))
                .map_err(|_| "Не удалось отрисовать текст.")?;
            (api.move_to)(
                tile.dc,
                (x - layout.x) as f64,
                (y - layout.y) as f64 + font.ascent,
            );
            (api.show_text)(tile.dc, line.as_ptr());
        }
        (api.surface_flush)(tile.surface);
        let stride = (api.image_surface_get_stride)(tile.surface) as usize;
        let data = std::slice::from_raw_parts(
            (api.image_surface_get_data)(tile.surface),
            stride * layout.h,
        );
        let mask: Vec<_> = data
            .chunks_exact(stride)
            .flat_map(|row| row[..layout.w].iter().copied())
            .collect();
        Ok(layout.paint(text, &mask, face))
    }
}
