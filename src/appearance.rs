//! Small, versioned UI preferences. They never form part of a paint document.
use crate::{i18n::Language, platform::UiFont};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Accent {
    #[default]
    Violet,
    Cobalt,
    Coral,
    Amber,
    Red,
}

impl Accent {
    pub const ALL: [Self; 5] = [
        Self::Violet,
        Self::Cobalt,
        Self::Coral,
        Self::Amber,
        Self::Red,
    ];

    pub fn key(self) -> &'static str {
        match self {
            Self::Violet => "violet",
            Self::Cobalt => "cobalt",
            Self::Coral => "coral",
            Self::Amber => "amber",
            Self::Red => "red",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Violet => "Фиолетовый",
            Self::Cobalt => "Синий",
            Self::Coral => "Коралловый",
            Self::Amber => "Янтарный",
            Self::Red => "Красный",
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|a| a.key() == key)
    }

    pub fn color(self, dark: bool) -> u32 {
        match (self, dark) {
            (Self::Violet, true) => 0xb4a0ff,
            (Self::Violet, false) => 0x6942c6,
            (Self::Cobalt, true) => 0x8baeff,
            (Self::Cobalt, false) => 0x285cc4,
            (Self::Coral, true) => 0xffa18c,
            (Self::Coral, false) => 0xad402d,
            (Self::Amber, true) => 0xf1c174,
            (Self::Amber, false) => 0x8c570d,
            (Self::Red, true) => 0xff777f,
            (Self::Red, false) => 0xbc263c,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Appearance {
    pub dark: bool,
    pub font: UiFont,
    pub accent: Accent,
    pub language: Language,
    pub font_size: u16,
    pub animations: bool,
}

impl Default for Appearance {
    fn default() -> Self {
        Self {
            dark: true,
            font: UiFont::default(),
            accent: Accent::default(),
            language: Language::default(),
            font_size: 13,
            animations: true,
        }
    }
}

impl Appearance {
    pub fn decode(text: &str) -> Self {
        let mut appearance = Self::default();
        // Versions before the appearance panel stored just a theme name.
        if matches!(text.trim(), "paper" | "graphite") {
            appearance.dark = text.trim() != "paper";
            return appearance;
        }
        for line in text.lines().take(32) {
            let Some((key, value)) = line.trim().split_once('=') else {
                continue;
            };
            match key.trim() {
                "theme" => match value.trim() {
                    "paper" => appearance.dark = false,
                    "graphite" => appearance.dark = true,
                    _ => {}
                },
                "font" => {
                    if let Some(font) = UiFont::from_key(value.trim()) {
                        appearance.font = font;
                    }
                }
                "accent" => {
                    if let Some(accent) = Accent::from_key(value.trim()) {
                        appearance.accent = accent;
                    }
                }
                "language" => {
                    if let Some(language) = Language::from_key(value.trim()) {
                        appearance.language = language;
                    }
                }
                "font_size" => {
                    if let Ok(size) = value.trim().parse::<u16>() {
                        if (11..=17).contains(&size) {
                            appearance.font_size = size;
                        }
                    }
                }
                "animations" => match value.trim() {
                    "true" => appearance.animations = true,
                    "false" => appearance.animations = false,
                    _ => {}
                },
                _ => {}
            }
        }
        appearance
    }

    pub fn encode(self) -> String {
        format!(
            "version=3\ntheme={}\nfont={}\naccent={}\nlanguage={}\nfont_size={}\nanimations={}\n",
            if self.dark { "graphite" } else { "paper" },
            self.font.key(),
            self.accent.key(),
            self.language.key(),
            self.font_size,
            self.animations
        )
    }
}
