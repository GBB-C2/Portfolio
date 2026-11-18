//! Editable type stores its layout alongside a raster for portable compositing and export.
use super::*;
use crate::lettering::TextEffects;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ParagraphAlign {
    #[default]
    Left,
    Center,
    Right,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextLayer {
    pub body: String,
    pub font: String,
    pub size: u32,
    pub color: Color,
    pub bold: bool,
    pub italic: bool,
    pub align: ParagraphAlign,
    pub x: u32,
    pub y: u32,
    pub effects: TextEffects,
}

impl Default for TextLayer {
    fn default() -> Self {
        Self {
            body: String::new(),
            font: "segoe_ui".into(),
            size: 48,
            color: Color::BLACK,
            bold: false,
            italic: false,
            align: ParagraphAlign::Left,
            x: 0,
            y: 0,
            effects: TextEffects::default(),
        }
    }
}

impl TextLayer {
    pub fn validate(&self, width: u32, height: u32) -> Result<(), String> {
        if self.body.trim().is_empty()
            || self.body.encode_utf16().count() > 4096
            || self
                .body
                .chars()
                .any(|c| c.is_control() && !matches!(c, '\n' | '\t'))
        {
            return Err("Введите текст длиной до 4096 знаков.".into());
        }
        if !self.effects.valid()
            || !(4..=512).contains(&self.size)
            || self.x >= width
            || self.y >= height
            || self.font.is_empty()
            || self.font.len() > 128
            || self.font.chars().any(char::is_control)
        {
            return Err("Проверьте шрифт, размер и координаты текста.".into());
        }
        Ok(())
    }

    pub(super) fn write(&self, bytes: &mut Vec<u8>, extended: bool) {
        push_string(bytes, &self.body);
        push_string(bytes, &self.font);
        for v in [self.x, self.y, self.size] {
            push_u32(bytes, v);
        }
        bytes.extend_from_slice(&[
            self.color.r,
            self.color.g,
            self.color.b,
            self.color.a,
            self.bold as u8,
            self.italic as u8,
            self.align as u8,
        ]);
        if extended {
            let e = self.effects;
            bytes.extend_from_slice(&[e.outline, e.shadow, e.shadow_opacity, e.depth]);
            bytes.extend_from_slice(&e.angle.to_le_bytes());
            bytes.extend_from_slice(&[e.color.r, e.color.g, e.color.b, e.color.a]);
        }
    }

    pub(super) fn read(
        reader: &mut Reader,
        width: u32,
        height: u32,
        extended: bool,
    ) -> Result<Self, String> {
        let body = reader.utf8(16384)?;
        let font = reader.string(128)?;
        let (x, y, size) = (reader.u32()?, reader.u32()?, reader.u32()?);
        let fields = reader.take(7)?;
        let align = match fields[6] {
            0 => ParagraphAlign::Left,
            1 => ParagraphAlign::Center,
            2 => ParagraphAlign::Right,
            _ => return Err("Повреждены параметры слоя.".into()),
        };
        if fields[4] > 1 || fields[5] > 1 {
            return Err("Повреждены параметры слоя.".into());
        }
        let text = Self {
            body,
            font,
            x,
            y,
            size,
            align,
            effects: if extended {
                let v = reader.take(10)?;
                TextEffects {
                    outline: v[0],
                    shadow: v[1],
                    shadow_opacity: v[2],
                    depth: v[3],
                    angle: u16::from_le_bytes([v[4], v[5]]),
                    color: Color::rgba(v[6], v[7], v[8], v[9]),
                }
            } else {
                TextEffects::default()
            },
            color: Color::rgba(fields[0], fields[1], fields[2], fields[3]),
            bold: fields[4] != 0,
            italic: fields[5] != 0,
        };
        text.validate(width, height)?;
        Ok(text)
    }

    fn layer_name(&self) -> String {
        self.body
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .chars()
            .take(48)
            .collect()
    }
}

impl Document {
    pub fn text_layer_at(&self, x: u32, y: u32) -> Option<&Layer> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let pixel = (y * self.width + x) as usize;
        for layer in self
            .layers
            .iter()
            .rev()
            .filter(|l| l.visible && l.opacity > 0)
        {
            let alpha = layer.pixels[pixel].a;
            if alpha > 0 && layer.text.is_some() {
                return Some(layer);
            }
            if alpha == 255 && layer.opacity == 255 {
                break;
            }
        }
        None
    }

    /// Insert or replace only the intended type layer, with a single history entry.
    pub fn set_text_layer(
        &mut self,
        target: Option<u64>,
        text: TextLayer,
        pixels: Vec<Color>,
    ) -> Result<bool, String> {
        text.validate(self.width, self.height)?;
        if pixels.len() != pixel_count(self.width, self.height)? {
            return Err("Повреждён буфер изображения.".into());
        }
        let (at, id, changes) = if let Some(id) = target {
            let at = self
                .layers
                .iter()
                .position(|l| l.id == id && l.text.is_some())
                .ok_or("Выберите текстовый слой.")?;
            let layer = &self.layers[at];
            if layer.locked || !layer.visible {
                return Err("Слой скрыт или заблокирован. Выберите доступный слой.".into());
            }
            if layer.text.as_ref() == Some(&text) && layer.pixels == pixels {
                return Ok(false);
            }
            let changes = changed_tiles(
                &layer.pixels,
                &pixels,
                self.width as usize,
                self.height as usize,
            );
            (at, id, PixelDelta::Tiles(changes))
        } else {
            self.validate_new_layer()?;
            (
                self.active + 1,
                self.next_layer_id,
                PixelDelta::Whole(pixels.clone()),
            )
        };
        let before = self.begin_layer_structure_edit();
        if target.is_some() {
            let layer = &mut self.layers[at];
            if layer
                .text
                .as_ref()
                .is_some_and(|old| layer.name == old.layer_name())
            {
                layer.name = text.layer_name();
            }
            layer.text = Some(text);
            layer.pixels = pixels;
        } else {
            self.next_layer_id += 1;
            self.layers.insert(
                at,
                Layer {
                    id,
                    name: text.layer_name(),
                    text: Some(text),
                    visible: true,
                    locked: false,
                    opacity: 255,
                    pixels,
                },
            );
        }
        self.active = at;
        if let Some(before) = before {
            self.record_layer_structure_edit(
                before,
                vec![LayerDelta {
                    id,
                    pixels: changes,
                }],
            );
        }
        Ok(true)
    }
}
