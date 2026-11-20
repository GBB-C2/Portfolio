//! Editable type treatments. All distances are expressed in document pixels.
use crate::document::{blend, Color, TextLayer};
use crate::platform::UiFont;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TextEffects {
    pub outline: u8,
    pub shadow: u8,
    pub shadow_opacity: u8,
    pub shadow_angle: u16,
    pub underline: bool,
    pub strikeout: bool,
    pub depth: u8,
    pub angle: u16,
    pub color: Color,
}

impl Default for TextEffects {
    fn default() -> Self {
        Self {
            outline: 0,
            shadow: 0,
            shadow_opacity: 45,
            shadow_angle: 45,
            underline: false,
            strikeout: false,
            depth: 0,
            angle: 45,
            color: Color::from_hex(0x363646),
        }
    }
}

impl TextEffects {
    pub fn valid(self) -> bool {
        self.outline <= 12
            && self.shadow <= 32
            && self.shadow_opacity <= 100
            && self.depth <= 48
            && self.shadow_angle <= 360
            && self.angle <= 360
    }

    pub fn visible(self) -> bool {
        self.outline != 0 || self.depth != 0 || (self.shadow != 0 && self.shadow_opacity != 0)
    }

    pub fn margin(self) -> i32 {
        i32::from(self.outline.max(self.shadow).max(self.depth)) + 2
    }

    pub fn scaled(self, scale: f32) -> Self {
        let length = |v: u8| {
            if v == 0 {
                0
            } else {
                (f32::from(v) * scale).round().max(1.) as u8
            }
        };
        Self {
            depth: length(self.depth),
            outline: length(self.outline),
            shadow: length(self.shadow),
            ..self
        }
    }

    pub fn extrusion(self) -> Vec<(i32, i32)> {
        let (sin, cos) = f32::from(self.angle).to_radians().sin_cos();
        let mut offsets: Vec<_> = (1..=self.depth)
            .map(|d| {
                (
                    (cos * f32::from(d)).round() as i32,
                    (sin * f32::from(d)).round() as i32,
                )
            })
            .collect();
        offsets.dedup();
        offsets
    }

    /// Render straight-alpha RGBA from an antialiased glyph mask. The in-place
    /// editor supplies its own foreground, so it can request only the effects.
    pub fn paint(self, mask: &[u8], w: usize, h: usize, ink: Color, face: bool) -> Vec<Color> {
        let mut out = vec![Color::TRANSPARENT; mask.len()];
        let over = |out: &mut [Color], alpha: &[u8], color: Color| {
            for (dst, &a) in out.iter_mut().zip(alpha) {
                if a > 0 {
                    *dst = blend(*dst, color, a);
                }
            }
        };
        if self.shadow > 0 && self.shadow_opacity > 0 {
            let (sin, cos) = f32::from(self.shadow_angle).to_radians().sin_cos();
            // Use the dominant-axis distance to preserve the original 45° shadow.
            let d = f32::from(self.shadow) / sin.abs().max(cos.abs());
            let (dx, dy) = ((cos * d).round() as i32, (sin * d).round() as i32);
            let color = Color::rgba(0, 0, 0, (u16::from(self.shadow_opacity) * 255 / 100) as u8);
            shifted_rows(mask, w, h, dx, dy, |dest, src| {
                over(&mut out[dest..dest + src.len()], src, color)
            });
        }
        if self.depth > 0 {
            let mut sides = vec![0; mask.len()];
            for (dx, dy) in self.extrusion() {
                shifted_rows(mask, w, h, dx, dy, |dest, src| {
                    for (dst, &src) in sides[dest..dest + src.len()].iter_mut().zip(src) {
                        *dst = src.max(*dst);
                    }
                });
            }
            over(&mut out, &sides, self.color);
        }
        if self.outline > 0 {
            let edge = stroke_coverage(mask, w, h, self.outline);
            over(&mut out, &edge, self.color);
        }
        if face {
            over(&mut out, mask, ink);
        }
        out
    }
}

fn shifted_rows(
    mask: &[u8],
    w: usize,
    h: usize,
    dx: i32,
    dy: i32,
    mut row: impl FnMut(usize, &[u8]),
) {
    let (sx, sy) = ((-dx).max(0) as usize, (-dy).max(0) as usize);
    let (tx, ty) = (dx.max(0) as usize, dy.max(0) as usize);
    let cw = w.saturating_sub(dx.unsigned_abs() as usize);
    if cw == 0 {
        return;
    }
    for y in 0..h.saturating_sub(dy.unsigned_abs() as usize) {
        let source = (sy + y) * w + sx;
        row((ty + y) * w + tx, &mask[source..source + cw]);
    }
}

// Two chamfer passes give rounded outlines with the original subpixel edge.
fn stroke_coverage(mask: &[u8], w: usize, h: usize, r: u8) -> Vec<u8> {
    let mut d: Vec<u16> = mask
        .iter()
        .map(|&a| {
            if a == 0 {
                u16::MAX / 2
            } else {
                u16::from(255 - a)
            }
        })
        .collect();
    for y in 0..h {
        for x in 0..w {
            let i = y * w + x;
            let mut v = d[i];
            if x > 0 {
                v = v.min(d[i - 1] + 256);
            }
            if y > 0 {
                v = v.min(d[i - w] + 256);
                if x > 0 {
                    v = v.min(d[i - w - 1] + 362);
                }
                if x + 1 < w {
                    v = v.min(d[i - w + 1] + 362);
                }
            }
            d[i] = v;
        }
    }
    for y in (0..h).rev() {
        for x in (0..w).rev() {
            let i = y * w + x;
            let mut v = d[i];
            if x + 1 < w {
                v = v.min(d[i + 1] + 256);
            }
            if y + 1 < h {
                v = v.min(d[i + w] + 256);
                if x > 0 {
                    v = v.min(d[i + w - 1] + 362);
                }
                if x + 1 < w {
                    v = v.min(d[i + w + 1] + 362);
                }
            }
            d[i] = v;
        }
    }
    let limit = u16::from(r) * 256 + 255;
    d.into_iter()
        .map(|v| limit.saturating_sub(v).min(255) as u8)
        .collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TextPreset {
    Body,
    Caption,
    Heading,
    Poster,
    Script,
    Signature,
    Outline,
    Shadow,
    Block,
    Relief,
}

impl TextPreset {
    pub const ALL: [Self; 10] = [
        Self::Body,
        Self::Caption,
        Self::Heading,
        Self::Poster,
        Self::Script,
        Self::Signature,
        Self::Outline,
        Self::Shadow,
        Self::Block,
        Self::Relief,
    ];
    pub const fn label(self) -> &'static str {
        match self {
            Self::Body => "Обычный",
            Self::Caption => "Подпись",
            Self::Heading => "Заголовок",
            Self::Poster => "Афиша",
            Self::Script => "Рукопись",
            Self::Signature => "Автограф",
            Self::Outline => "Контур",
            Self::Shadow => "С тенью",
            Self::Block => "Объёмный",
            Self::Relief => "Рельеф",
        }
    }
    pub const fn aliases(self) -> &'static str {
        match self {
            Self::Body => "body text normal regular",
            Self::Caption => "caption label small",
            Self::Heading => "headline title bold",
            Self::Poster => "poster display",
            Self::Script => "script handwritten lettering",
            Self::Signature => "signature italic calligraphy",
            Self::Outline => "outline stroke contour",
            Self::Shadow => "drop shadow",
            Self::Block => "3d block extrusion объём объем",
            Self::Relief => "3d relief emboss объём объем",
        }
    }
    pub fn apply(self, text: &mut TextLayer) {
        let (font, size, bold, italic) = match self {
            Self::Body => (UiFont::SegoeUi, 48, false, false),
            Self::Caption => (UiFont::Tahoma, 22, false, false),
            Self::Heading => (UiFont::Bahnschrift, 76, true, false),
            Self::Poster => (UiFont::Verdana, 96, true, false),
            Self::Script => (UiFont::SegoeScript, 56, false, false),
            Self::Signature => (UiFont::Gabriola, 64, false, true),
            Self::Outline => (UiFont::Verdana, 66, true, false),
            Self::Shadow => (UiFont::SegoeUi, 64, true, false),
            Self::Block => (UiFont::Bahnschrift, 84, true, false),
            Self::Relief => (UiFont::Verdana, 72, true, true),
        };
        text.font = font.key().into();
        text.size = size;
        text.bold = bold;
        text.italic = italic;
        let mut effects = TextEffects::default();
        match self {
            Self::Outline => {
                effects.outline = 3;
                effects.color = Color::from_hex(0x9a7896);
            }
            Self::Shadow => effects.shadow = 6,
            Self::Block => {
                effects.depth = 12;
                effects.color = Color::from_hex(0x6651a0);
            }
            Self::Relief => {
                effects.depth = 6;
                effects.angle = 225;
                effects.outline = 1;
                effects.shadow = 5;
                effects.color = Color::from_hex(0xb08b55);
            }
            _ => {}
        }
        text.effects = effects;
    }

    pub fn current(text: &TextLayer) -> Option<Self> {
        let mut candidate = TextLayer::default();
        Self::ALL.into_iter().find(|preset| {
            preset.apply(&mut candidate);
            (
                candidate.font.as_str(),
                candidate.size,
                candidate.bold,
                candidate.italic,
                candidate.effects,
            ) == (
                text.font.as_str(),
                text.size,
                text.bold,
                text.italic,
                text.effects,
            )
        })
    }
}

pub(crate) struct TextLayout<'a> {
    pub lines: Vec<(&'a str, i32, i32)>,
    pub x: i32,
    pub y: i32,
    pub w: usize,
    pub h: usize,
}

impl<'a> TextLayout<'a> {
    pub fn measure(
        text: &'a TextLayer,
        width: u32,
        height: u32,
        mut measure: impl FnMut(&str) -> (i32, i32),
    ) -> Option<Self> {
        use crate::document::ParagraphAlign::*;
        let pad = text.effects.margin();
        let overhang = text.size as i32 / 3 + 2;
        let (mut x0, mut y0, mut x1, mut y1) = (i32::MAX, i32::MAX, i32::MIN, i32::MIN);
        let mut lines = Vec::new();
        let mut y = text.y as i32;
        for line in text.body.split('\n') {
            if y >= height as i32 + pad {
                break;
            }
            let (advance, line_height) = measure(if line.is_empty() { " " } else { line });
            let x = text.x as i32
                - match text.align {
                    Left => 0,
                    Center => advance / 2,
                    Right => advance,
                };
            if !line.trim().is_empty() {
                x0 = x0.min(x - overhang);
                x1 = x1.max(x + advance + overhang);
                y0 = y0.min(y);
                y1 = y1.max(y + line_height + 1);
                lines.push((line, x, y));
            }
            y += line_height.max(1);
        }
        if lines.is_empty() {
            return None;
        }
        // Keep nearby off-canvas glyphs that can still cast a shadow onto the canvas.
        x0 = (x0 - pad).max(-pad);
        y0 = (y0 - pad).max(-pad);
        x1 = (x1 + pad).min(width as i32 + pad);
        y1 = (y1 + pad).min(height as i32 + pad);
        if x0 >= x1 || y0 >= y1 {
            return None;
        }
        Some(Self {
            lines,
            x: x0,
            y: y0,
            w: (x1 - x0) as usize,
            h: (y1 - y0) as usize,
        })
    }
    pub fn paint(&self, text: &TextLayer, mask: &[u8], face: bool) -> TextTile {
        TextTile {
            x: self.x,
            y: self.y,
            w: self.w as u32,
            h: self.h as u32,
            pixels: text.effects.paint(mask, self.w, self.h, text.color, face),
        }
    }
}

pub struct TextTile {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
    pub pixels: Vec<Color>,
}

impl TextTile {
    pub fn into_canvas(self, w: u32, h: u32) -> Result<Vec<Color>, String> {
        let mut pixels = vec![Color::TRANSPARENT; w as usize * h as usize];
        let left = self.x.max(0);
        let right = (self.x + self.w as i32).min(w as i32);
        let mut visible = false;
        if right > left {
            for y in self.y.max(0)..(self.y + self.h as i32).min(h as i32) {
                let src = ((y - self.y) * self.w as i32 + left - self.x) as usize;
                let dst = (y * w as i32 + left) as usize;
                let row = &self.pixels[src..src + (right - left) as usize];
                visible |= row.iter().any(|p| p.a != 0);
                pixels[dst..dst + row.len()].copy_from_slice(row);
            }
        }
        if !visible {
            return Err("Текст выходит за границы холста. Измените координаты или размер.".into());
        }
        Ok(pixels)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EffectParam {
    Outline,
    Shadow,
    ShadowOpacity,
    Depth,
    Direction,
    ShadowAngle,
}
impl EffectParam {
    pub const ALL: [Self; 6] = [
        Self::Outline,
        Self::Shadow,
        Self::ShadowOpacity,
        Self::Depth,
        Self::Direction,
        Self::ShadowAngle,
    ];
    pub fn info(self) -> (f32, f32, &'static str, u32, &'static str) {
        let (max, label, id, unit) = match self {
            Self::Outline => (12., "Толщина обводки", 30, "px"),
            Self::Shadow => (32., "Смещение тени", 31, "px"),
            Self::ShadowOpacity => (100., "Непрозрачность тени", 32, "%"),
            Self::Depth => (48., "Глубина 3D", 33, "px"),
            Self::ShadowAngle => (360., "Направление тени", 35, "°"),
            Self::Direction => (360., "Направление объёма", 34, "°"),
        };
        (0., max, label, id, unit)
    }
    pub fn value(self, e: TextEffects) -> f32 {
        match self {
            Self::Outline => e.outline as f32,
            Self::Shadow => e.shadow as f32,
            Self::ShadowOpacity => e.shadow_opacity as f32,
            Self::Depth => e.depth as f32,
            Self::Direction => e.angle as f32,
            Self::ShadowAngle => e.shadow_angle as f32,
        }
    }
    pub fn set(self, e: &mut TextEffects, v: f32) {
        let n = v.round() as u16;
        match self {
            Self::Outline => e.outline = n as u8,
            Self::Shadow => e.shadow = n as u8,
            Self::ShadowOpacity => e.shadow_opacity = n as u8,
            Self::Depth => e.depth = n as u8,
            Self::Direction => e.angle = n,
            Self::ShadowAngle => e.shadow_angle = n,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn masks_produce_directional_extrusion_outline_and_translucent_shadow() {
        let mut mask = vec![0; 13 * 13];
        mask[6 * 13 + 6] = 255;
        mask[6 * 13 + 7] = 124;
        let ink = Color::rgb(18, 52, 90);
        let side = Color::rgb(150, 60, 10);
        let mut e = TextEffects {
            depth: 3,
            angle: 90,
            shadow: 4,
            shadow_opacity: 50,
            color: side,
            ..Default::default()
        };
        let under = e.paint(&mask, 13, 13, ink, false);
        assert_eq!(under[9 * 13 + 6], side);
        assert_eq!(under[10 * 13 + 10], Color::rgba(0, 0, 0, 127));
        assert_eq!(under[5 * 13 + 6], Color::TRANSPARENT);
        let layer = e.paint(&mask, 13, 13, ink, true);
        assert_eq!(layer[6 * 13 + 6], ink);
        assert_eq!(layer[9 * 13 + 7].a, 124);
        e.angle = 180;
        e.outline = 1;
        let rotated = e.paint(&mask, 13, 13, ink, true);
        assert_eq!(rotated[6 * 13 + 3], side);
        assert_eq!(rotated[5 * 13 + 6], side);
        assert_eq!(rotated[9 * 13 + 6], Color::TRANSPARENT);
    }

    #[cfg(any(target_os = "windows", target_os = "linux"))]
    #[test]
    fn real_multiline_glyphs_keep_their_extrusion_across_reopen_and_canvas_edges() {
        use crate::{
            document::{Document, ParagraphAlign},
            i18n::Language,
            platform::rasterize_text,
        };
        #[cfg(target_os = "linux")]
        if !crate::platform::preview_fonts_available() {
            return;
        }
        let (w, h) = (240, 144);
        let render = |t: &TextLayer| rasterize_text(t, w, h, Language::Russian);
        let mut text = TextLayer {
            body: "Объём\nAa".into(),
            x: 30,
            y: 12,
            size: 36,
            color: Color::from_hex(0xd96134),
            ..Default::default()
        };
        let plain = render(&text).unwrap();
        text.effects.depth = 14;
        text.effects.angle = 0;
        let pixels = render(&text).unwrap();
        assert!(pixels.contains(&text.color));
        assert!(pixels.contains(&text.effects.color));
        assert!(plain
            .iter()
            .zip(&pixels)
            .any(|(p, q)| p.a == 0 && q.a == 255));
        let mut doc = Document::new(w, h).unwrap();
        doc.set_text_layer(None, text.clone(), pixels.clone())
            .unwrap();
        let reopened = Document::from_bytes(&doc.to_bytes().unwrap()).unwrap();
        assert_eq!(
            render(reopened.active_layer().text.as_ref().unwrap()).unwrap(),
            pixels
        );
        assert_eq!(reopened.composite(), pixels);
        text.align = ParagraphAlign::Right;
        text.x = 0;
        let edge = render(&text).unwrap();
        assert!(edge.contains(&text.effects.color));
        assert!(edge
            .chunks(w as usize)
            .all(|row| row[20..].iter().all(|p| p.a == 0)));
    }

    #[test]
    fn underlining_strikeout_and_shadow_direction_survive_a_decorated_project() {
        use crate::{document::Document, i18n::Language, platform::rasterize_text};
        #[cfg(target_os = "linux")]
        if !crate::platform::preview_fonts_available() {
            return;
        }
        let mut text = TextLayer {
            body: "ab  cd\nEF".into(),
            size: 30,
            x: 10,
            y: 10,
            ..Default::default()
        };
        let render = |t: &TextLayer| rasterize_text(t, 180, 110, Language::Russian).unwrap();
        let plain = render(&text);
        text.effects.underline = true;
        let underline = render(&text);
        assert!(plain != underline);
        text.effects.strikeout = true;
        assert!(render(&text) != underline);
        text.effects.shadow = 7;
        text.effects.shadow_angle = 180;
        let left = render(&text);
        text.effects.shadow_angle = 0;
        let right = render(&text);
        assert!(left != right);
        let mut doc = Document::new(180, 110).unwrap();
        doc.set_text_layer(None, text.clone(), right.clone())
            .unwrap();
        let bytes = doc.to_bytes().unwrap();
        assert_eq!(&bytes[..8], b"PRTFOL04");
        let restored = Document::from_bytes(&bytes).unwrap();
        assert_eq!(restored.active_layer().text.as_ref(), Some(&text));
        assert!(restored.composite() == right);
        assert!(doc.undo() && doc.redo());
        assert_eq!(doc.to_bytes().unwrap(), bytes);
    }

    #[test]
    fn old_projects_keep_their_version_and_effects_round_trip_with_history_and_export() {
        use crate::document::{encode_png, Document, ParagraphAlign};
        let mut doc = Document::new(50, 50).unwrap();
        assert_eq!(&doc.to_bytes().unwrap()[..8], b"PRTFOL01");
        let mut t = TextLayer {
            body: "Абв".into(),
            x: 23,
            y: 12,
            align: ParagraphAlign::Right,
            ..Default::default()
        };
        let mut mask = vec![0; 2500];
        mask[12 * 50 + 23] = 255;
        let plain = t.effects.paint(&mask, 50, 50, t.color, true);
        doc.set_text_layer(None, t.clone(), plain).unwrap();
        let old = doc.to_bytes().unwrap();
        assert_eq!(&old[..8], b"PRTFOL02");
        let mut loaded = Document::from_bytes(&old).unwrap();
        let id = loaded.active_layer().id;
        assert_eq!(
            loaded.active_layer().text.as_ref().unwrap().effects,
            TextEffects::default()
        );
        TextPreset::Block.apply(&mut t);
        assert_eq!(
            (&*t.body, t.x, t.y, t.align),
            ("Абв", 23, 12, ParagraphAlign::Right)
        );
        let styled = t.effects.paint(&mask, 50, 50, t.color, true);
        loaded
            .set_text_layer(Some(id), t.clone(), styled.clone())
            .unwrap();
        let bytes = loaded.to_bytes().unwrap();
        assert_eq!(&bytes[..8], b"PRTFOL03");
        let reopened = Document::from_bytes(&bytes).unwrap();
        assert_eq!(reopened.active_layer().text.as_ref(), Some(&t));
        assert_eq!(reopened.composite(), styled);
        assert_eq!(
            encode_png(50, 50, &reopened.composite()).unwrap(),
            encode_png(50, 50, &styled).unwrap()
        );
        assert!(loaded.undo());
        assert_eq!(loaded.to_bytes().unwrap(), old);
        assert!(loaded.redo());
        assert_eq!(loaded.to_bytes().unwrap(), bytes);
        let mut invalid = t;
        invalid.effects.depth = 49;
        assert!(invalid.validate(50, 50).is_err());
    }
}
