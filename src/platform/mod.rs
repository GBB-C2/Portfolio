//! Small operating-system boundary. Document and UI state never own Win32 handles.

use crate::document::{Color, TextLayer};
use std::path::{Path, PathBuf};

#[cfg(any(target_os = "windows", test))]
mod frame_cache;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Modifiers {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MouseButton {
    Left,
    Middle,
    Right,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileKind {
    Project,
    Image,
    Png,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Answer {
    Yes,
    No,
    Cancel,
}

#[derive(Clone, Debug)]
pub enum Event {
    MouseMove {
        x: f32,
        y: f32,
        modifiers: Modifiers,
    },
    MouseDown {
        x: f32,
        y: f32,
        button: MouseButton,
        modifiers: Modifiers,
    },
    MouseUp {
        x: f32,
        y: f32,
        button: MouseButton,
        modifiers: Modifiers,
    },
    /// One unit is one wheel detent. Coordinates are relative to the client area.
    Wheel {
        x: f32,
        y: f32,
        delta: f32,
        modifiers: Modifiers,
    },
    /// `code` is the Windows virtual-key code; text comes through `Text`.
    Key {
        code: u32,
        pressed: bool,
        repeat: bool,
        modifiers: Modifiers,
    },
    Text(String),
    Paste(String),
    CanvasTextChanged {
        session: u64,
        body: String,
    },
    FinishCanvasText(bool),
    FocusTextSettings,
    FocusLost,
    CaptureLost,
    /// OS accessibility preference; true disables nonessential transitions.
    ReducedMotion(bool),
    FilesDropped {
        paths: Vec<PathBuf>,
    },
    Tick,
    CloseRequested,
    FileSelected {
        kind: FileKind,
        path: Option<PathBuf>,
    },
    Confirmed {
        id: u32,
        answer: Answer,
    },
    Prompted {
        id: u32,
        value: Option<String>,
    },
}

#[derive(Clone, Debug)]
pub enum Effect {
    SetTitle(String),
    SetDarkMode(bool),
    OpenFile(FileKind),
    SaveFile {
        kind: FileKind,
        suggested_name: String,
    },
    Confirm {
        id: u32,
        title: String,
        message: String,
    },
    Prompt {
        id: u32,
        title: String,
        label: String,
        value: String,
    },
    Message {
        title: String,
        message: String,
        error: bool,
    },
    FocusCanvasText,
    CopyText(String),
    PasteText,
    Quit,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TextAlign {
    #[default]
    Left,
    Center,
    Right,
}

/// Installed Windows UI font families. The native backend checks availability
/// and falls back to Segoe UI when a selected family is not installed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum UiFont {
    #[default]
    Bahnschrift,
    SegoeUi,
    Tahoma,
    Verdana,
    SegoeScript,
    Gabriola,
    ProggyForever,
    CozetteVector,
    ProggyClean,
    ProggySquare,
}

impl UiFont {
    pub const ALL: [Self; 10] = [
        Self::Bahnschrift,
        Self::SegoeUi,
        Self::Tahoma,
        Self::Verdana,
        Self::SegoeScript,
        Self::Gabriola,
        Self::ProggyForever,
        Self::CozetteVector,
        Self::ProggyClean,
        Self::ProggySquare,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Bahnschrift => "Bahnschrift",
            Self::SegoeUi => "Segoe UI",
            Self::Tahoma => "Tahoma",
            Self::Verdana => "Verdana",
            Self::SegoeScript => "Segoe Script",
            Self::Gabriola => "Gabriola",
            Self::ProggyForever => "Proggy Forever",
            Self::CozetteVector => "Cozette Vector",
            Self::ProggyClean => "Proggy Clean",
            Self::ProggySquare => "Proggy Square",
        }
    }

    pub const fn key(self) -> &'static str {
        match self {
            Self::Bahnschrift => "bahnschrift",
            Self::SegoeUi => "segoe_ui",
            Self::Tahoma => "tahoma",
            Self::Verdana => "verdana",
            Self::SegoeScript => "segoe_script",
            Self::Gabriola => "gabriola",
            Self::ProggyForever => "proggy_forever",
            Self::CozetteVector => "cozette_vector",
            Self::ProggyClean => "proggy_clean",
            Self::ProggySquare => "proggy_square",
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        match key.trim().to_ascii_lowercase().as_str() {
            "bahnschrift" => Some(Self::Bahnschrift),
            "segoe_ui" | "segoe ui" | "segoeui" => Some(Self::SegoeUi),
            "tahoma" => Some(Self::Tahoma),
            "verdana" => Some(Self::Verdana),
            "segoe_script" | "segoe script" | "segoescript" => Some(Self::SegoeScript),
            "gabriola" => Some(Self::Gabriola),
            "proggy_forever" | "proggyforever" => Some(Self::ProggyForever),
            "cozette_vector" | "cozettevector" => Some(Self::CozetteVector),
            "proggy_clean" | "proggyclean" => Some(Self::ProggyClean),
            "proggy_square" | "proggysquare" => Some(Self::ProggySquare),
            _ => None,
        }
    }

    pub const fn is_calligraphic(self) -> bool {
        matches!(self, Self::SegoeScript | Self::Gabriola)
    }

    pub const fn is_bundled(self) -> bool {
        matches!(
            self,
            Self::ProggyForever | Self::CozetteVector | Self::ProggyClean | Self::ProggySquare
        )
    }

    /// The registered OpenType family may differ from the concise menu label.
    pub const fn family(self) -> &'static str {
        match self {
            Self::ProggyForever => "ProggyForever",
            Self::CozetteVector => "CozetteVector",
            Self::ProggyClean => "ProggyCleanTT",
            Self::ProggySquare => "ProggySquareTT",
            _ => self.label(),
        }
    }
}

/// Text geometry is in logical pixels, and `color` is 0x00RRGGBB.
#[derive(Clone, Debug)]
pub struct TextRun {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub text: String,
    pub color: u32,
    pub size: f32,
    pub bold: bool,
    pub font: UiFont,
    /// Serif display text keeps its Georgia family independently of UI settings.
    pub serif: bool,
    pub align: TextAlign,
    /// Optional parent clip rectangle in logical x, y, width, height coordinates.
    pub clip: Option<(f32, f32, f32, f32)>,
}

/// An opaque, top-down RGB framebuffer in physical pixels.
#[derive(Clone, Debug, Default)]
pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u32>,
    pub texts: Vec<TextRun>,
}

/// Placement in physical client pixels. Exclusions keep floating panels above the editor.
#[derive(Clone, Debug, PartialEq)]
pub struct CanvasEditView {
    pub session: u64,
    pub text: TextLayer,
    pub anchor: (i32, i32),
    pub font_px: i32,
    pub clip: [i32; 4],
    pub panels: Vec<[i32; 4]>,
}

pub trait NativeApp {
    fn canvas_editor(&self) -> Option<CanvasEditView> {
        None
    }
    /// The currently selected UI family also applies to application-owned
    /// native input dialogs. OS-owned common dialogs keep Windows typography.
    fn ui_font(&self) -> UiFont {
        UiFont::default()
    }
    fn ui_font_scale(&self) -> f32 {
        1.0
    }
    fn preferred_language(&self) -> &'static str {
        "ru"
    }
    fn needs_frequent_ticks(&self) -> bool {
        false
    }
    fn wants_animation(&self) -> bool {
        false
    }
    fn resize(&mut self, width: u32, height: u32, scale: f32);
    fn render(&mut self) -> Frame;
    fn event(&mut self, event: Event) -> Vec<Effect>;
}

/// Bound allocations before handing client dimensions to the renderer.
pub const MAX_FRAME_PIXELS: u64 = 32 * 1024 * 1024;
pub fn bounded_frame_size(width: u32, height: u32) -> (u32, u32) {
    let width = width.clamp(1, 16_384);
    let height = height.clamp(1, 16_384);
    if u64::from(width) * u64::from(height) <= MAX_FRAME_PIXELS {
        return (width, height);
    }
    let ratio = (MAX_FRAME_PIXELS as f64 / (f64::from(width) * f64::from(height))).sqrt();
    (
        (f64::from(width) * ratio).floor().max(1.0) as u32,
        (f64::from(height) * ratio).floor().max(1.0) as u32,
    )
}

/// Text can contain a language name independent of the currently selected UI.
pub fn preferred_text_script(text: &str, locale: &str) -> &'static str {
    if text
        .chars()
        .any(|c| matches!(c as u32, 0x1100..=0x11ff | 0x3130..=0x318f | 0xac00..=0xd7af))
    {
        "ko"
    } else if text
        .chars()
        .any(|c| matches!(c as u32, 0x3040..=0x30ff | 0x31f0..=0x31ff))
    {
        "ja"
    } else if text
        .chars()
        .any(|c| matches!(c as u32, 0x3400..=0x9fff | 0xf900..=0xfaff))
    {
        if locale == "ja" {
            "ja"
        } else if locale == "ko" {
            "ko"
        } else {
            "zh"
        }
    } else {
        "latin"
    }
}

#[cfg(test)]
mod native_contract_tests {
    use super::*;
    #[test]
    fn frame_budget_preserves_normal_and_8k_sizes() {
        assert_eq!(bounded_frame_size(1440, 940), (1440, 940));
        assert_eq!(bounded_frame_size(7680, 4320), (7680, 4320));
        for (w, h) in [(0, 0), (u32::MAX, u32::MAX), (16000, 16000), (1, u32::MAX)] {
            let (w, h) = bounded_frame_size(w, h);
            assert!(w > 0 && h > 0 && u64::from(w) * u64::from(h) <= MAX_FRAME_PIXELS);
        }
    }
    #[test]
    fn script_selection_handles_language_names_and_regional_han() {
        assert_eq!(preferred_text_script("한국어", "ru"), "ko");
        assert_eq!(preferred_text_script("サイズ", "ru"), "ja");
        assert_eq!(preferred_text_script("日本語", "ja"), "ja");
        assert_eq!(preferred_text_script("简体中文", "ru"), "zh");
        assert_eq!(preferred_text_script("Ваш штрих", "zh"), "latin");
    }
}

pub struct ImportedImage {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<crate::document::Color>,
}

#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "windows")]
pub use windows::{load_image, rasterize_text, run};

#[cfg(not(target_os = "windows"))]
pub fn rasterize_text(
    _: &TextLayer,
    _: u32,
    _: u32,
    _: crate::i18n::Language,
) -> Result<Vec<Color>, String> {
    Err("Для вывода шрифтов требуется Windows.".into())
}

#[cfg(not(target_os = "windows"))]
pub fn run(_app: Box<dyn NativeApp>) -> Result<(), String> {
    Err("Нативное окно Portfolio доступно в этой версии для Windows 10/11 (x64).".into())
}

#[cfg(not(target_os = "windows"))]
pub fn load_image(_path: &Path) -> Result<ImportedImage, String> {
    Err("Импорт изображений через Windows GDI+ доступен только в Windows.".into())
}
