//! User-named presets keep their own settings even when they share a brush tip.
use super::*;
use crate::tool_browser::SearchText;

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Preset {
    pub name: String,
    pub settings: ToolSettings,
}

#[derive(Debug)]
pub(super) struct Presets {
    pub entries: [Vec<Preset>; 2],
    pub selected: [usize; 2],
    dirty: bool,
}

impl Default for Presets {
    fn default() -> Self {
        Self {
            entries: std::array::from_fn(|family| {
                BrushKind::ALL
                    .into_iter()
                    .map(|kind| Preset {
                        name: String::new(),
                        settings: ToolSettings::preset(kind, family == 1),
                    })
                    .collect()
            }),
            selected: [0; 2],
            dirty: false,
        }
    }
}

impl Presets {
    const ORIGINAL_TIPS: usize = 10;
    pub(super) const MAX: usize = 246 + BrushKind::ALL.len();
    fn encode(&self) -> String {
        use std::fmt::Write;
        let mut output = format!(
            "Portfolio brushes 2\n{}\t{}\n",
            self.selected[0], self.selected[1]
        );
        for (family, entries) in self.entries.iter().enumerate() {
            for entry in entries {
                let s = entry.settings;
                writeln!(
                    output,
                    "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
                    family,
                    s.kind.index(),
                    s.size,
                    s.opacity,
                    s.hardness,
                    s.flow,
                    s.spacing,
                    s.angle,
                    s.scatter,
                    s.roundness,
                    s.grain,
                    s.texture_scale,
                    s.opacity_jitter,
                    entry.name
                )
                .unwrap();
            }
        }
        output
    }

    pub fn decode(input: &str) -> Option<Self> {
        let mut lines = input.lines();
        let old = match lines.next()? {
            "Portfolio brushes 1" => true,
            "Portfolio brushes 2" => false,
            _ => return None,
        };
        let builtin = if old {
            Self::ORIGINAL_TIPS
        } else {
            BrushKind::ALL.len()
        };
        let (paint, erase) = lines.next()?.split_once('\t')?;
        let mut selected = [paint.parse::<usize>().ok()?, erase.parse::<usize>().ok()?];
        let mut entries: [Vec<Preset>; 2] = Default::default();
        for line in lines {
            let mut fields = line.split('\t');
            let family: usize = fields.next()?.parse().ok()?;
            let list = entries.get_mut(family)?;
            let kind = *BrushKind::ALL.get(fields.next()?.parse::<usize>().ok()?)?;
            let mut number = || fields.next()?.parse::<f32>().ok().filter(|v| v.is_finite());
            let settings = ToolSettings {
                kind,
                size: number()?,
                opacity: number()?,
                hardness: number()?,
                flow: number()?,
                spacing: number()?,
                angle: number()?,
                scatter: number()?,
                roundness: number()?,
                grain: number()?,
                texture_scale: if old { 1. } else { number()? },
                opacity_jitter: if old { 0. } else { number()? },
            };
            let name = fields.next()?;
            if fields.next().is_some()
                || name.chars().count() > SearchText::MAX_SCALARS
                || name.chars().any(char::is_control)
                || list.len() >= builtin + 246
                || kind.index() as usize >= builtin
                || (list.len() < builtin && kind.index() as usize != list.len())
                || (list.len() >= builtin && name.trim().is_empty())
            {
                return None;
            }
            let mut brush = Brush::default();
            settings.apply(&mut brush);
            let settings = ToolSettings::from_brush(brush.sanitized());
            list.push(Preset {
                name: name.into(),
                settings,
            });
        }
        if (0..2).any(|f| entries[f].len() < builtin || selected[f] >= entries[f].len()) {
            return None;
        }
        if old {
            for family in 0..2 {
                let new_tips = BrushKind::ALL[builtin..].iter().map(|&kind| Preset {
                    name: String::new(),
                    settings: ToolSettings::preset(kind, family == 1),
                });
                entries[family].splice(builtin..builtin, new_tips);
                if selected[family] >= builtin {
                    selected[family] += BrushKind::ALL.len() - builtin;
                }
            }
        }
        Some(Self {
            entries,
            selected,
            dirty: old,
        })
    }

    pub fn current(&self, family: usize) -> &Preset {
        &self.entries[family][self.selected[family]]
    }
}

impl PortfolioApp {
    pub(super) fn preset_family(&self) -> Option<usize> {
        match self.tool {
            Tool::Brush => Some(0),
            Tool::Eraser => Some(1),
            _ => None,
        }
    }

    pub(super) fn sync_preset(&mut self) {
        if let Some(f) = self.preset_family() {
            let entry = &mut self.presets.entries[f][self.presets.selected[f]];
            let live = ToolSettings::from_brush(self.brush);
            if entry.settings != live {
                entry.settings = live;
                self.presets.dirty = true;
            }
        }
    }

    pub(super) fn save_presets(&mut self, out: &mut Vec<Effect>) {
        self.sync_preset();
        if !std::mem::take(&mut self.presets.dirty) {
            return;
        }
        let Some(path) = self.prefs.as_ref().map(|p| p.with_file_name("brushes.txt")) else {
            return;
        };
        if let Err(error) = fs::create_dir_all(path.parent().unwrap())
            .and_then(|_| document::atomic_write(&path, self.presets.encode().as_bytes()))
        {
            self.error(
                format!(
                    "{} {error}",
                    self.language.translate("Не удалось сохранить кисти.")
                ),
                out,
            );
        }
    }

    pub(super) fn choose_preset(&mut self, slot: usize) {
        let Some(f) = self.preset_family() else {
            return;
        };
        self.sync_preset();
        if let Some(preset) = self.presets.entries[f].get(slot) {
            preset.settings.apply(&mut self.brush);
            self.presets.dirty |= self.presets.selected[f] != slot;
            self.presets.selected[f] = slot;
        }
    }

    pub(super) fn preset_name(&self, family: usize, slot: usize) -> String {
        let entry = &self.presets.entries[family][slot];
        if !entry.name.is_empty() {
            return entry.name.clone();
        }
        let item = if family == 1 {
            ItemId::Eraser(entry.settings.kind)
        } else {
            ItemId::Brush(entry.settings.kind)
        };
        self.language.translate(item.label())
    }

    pub(super) fn picker_items(&self, family: ToolPanel) -> Vec<(Action, ItemId)> {
        if matches!(family, ToolPanel::Shapes | ToolPanel::Text) {
            return self
                .browser
                .filtered_ids(family, self.language)
                .into_iter()
                .map(|id| (Self::browser_item_action(id), id))
                .collect();
        }
        self.browser.filter_items(
            family,
            self.language,
            self.presets.entries[family.index()]
                .iter()
                .enumerate()
                .map(|(slot, p)| {
                    let item = if family == ToolPanel::Eraser {
                        ItemId::Eraser(p.settings.kind)
                    } else {
                        ItemId::Brush(p.settings.kind)
                    };
                    ((Action::Brush(slot), item), item, p.name.as_str())
                }),
        )
    }

    pub(super) fn edit_preset_name(&mut self, add: bool) {
        let Some(f) = self.preset_family() else {
            return;
        };
        self.sync_preset();
        if !add && self.tool_properties_ui.name_edit.is_some() {
            self.finish_preset_name(true);
            return;
        }
        if add {
            let next = self.presets.entries[f].len();
            if next >= Presets::MAX {
                return;
            }
            let name = format!(
                "{} {}",
                self.language
                    .translate(if f == 1 { "Ластик" } else { "Кисть" }),
                next + 1
            );
            self.presets.entries[f].push(Preset {
                name,
                settings: ToolSettings::from_brush(self.brush),
            });
            self.presets.selected[f] = next;
            self.presets.dirty = true;
            let family = ToolPanel::for_tool(self.tool).unwrap();
            self.browser.set_category(family, Category::All);
            self.browser.set_query(family, "");
            self.prepare_browser_layout(family);
            self.browser.view_mut(family).items.set(f32::MAX);
            self.controls.clear();
        }
        let mut name = SearchText::default();
        name.insert(&self.preset_name(f, self.presets.selected[f]));
        name.select_all();
        self.tool_properties_ui.name_edit = Some(name);
        self.space = false;
    }

    pub(super) fn finish_preset_name(&mut self, commit: bool) {
        let Some(edit) = self.tool_properties_ui.name_edit.take() else {
            return;
        };
        if let Some(f) = self.preset_family() {
            let name = edit.text().trim();
            if commit && !name.is_empty() && name != self.preset_name(f, self.presets.selected[f]) {
                self.presets.entries[f][self.presets.selected[f]].name = name.to_owned();
                self.presets.dirty = true;
            }
        }
    }

    pub(super) fn place_name_caret(&mut self, x: f32, extend: bool) {
        let rect = self.preset_name_rect().inset(4.);
        let edit = self.tool_properties_ui.name_edit.as_ref().unwrap();
        let caret = self.inline_caret_at(rect, edit, x);
        self.tool_properties_ui
            .name_edit
            .as_mut()
            .unwrap()
            .move_to(caret, extend);
    }

    pub(super) fn inline_caret_at(&self, rect: Rect, edit: &SearchText, x: f32) -> usize {
        let text = edit.text();
        let mut start = 0;
        while start < edit.caret()
            && self.browser_text_width(&text[start..edit.caret()]) > rect.w - 4.
        {
            start += text[start..].chars().next().unwrap().len_utf8();
        }
        let mut advance = rect.x;
        let mut caret = text.len();
        for (offset, ch) in text[start..].char_indices() {
            let width =
                self.browser_text_width(&text[start + offset..start + offset + ch.len_utf8()]);
            if x < advance + width / 2. {
                caret = start + offset;
                break;
            }
            advance += width;
        }
        caret
    }

    pub(super) fn preset_name_key(
        &mut self,
        code: u32,
        pressed: bool,
        m: Modifiers,
        out: &mut Vec<Effect>,
    ) -> bool {
        let Some(edit) = self.tool_properties_ui.name_edit.as_mut() else {
            return false;
        };
        if !pressed {
            return true;
        }
        match code {
            13 | 27 | 9 => {
                self.finish_preset_name(code != 27);
                self.pending_focus = Some(if code == 9 && !m.shift {
                    Action::AddBrush
                } else {
                    Action::RenameBrush
                });
                self.pending_properties_focus = true;
            }
            65 if m.ctrl => {
                edit.select_all();
            }
            67 | 88 if m.ctrl => {
                if edit.selection().is_some() {
                    out.push(Effect::CopyText(edit.selected_text()));
                    if code == 88 {
                        edit.delete();
                    }
                }
            }
            86 if m.ctrl => out.push(Effect::PasteText),
            8 => {
                edit.backspace();
            }
            46 => {
                edit.delete();
            }
            37 => {
                edit.move_left(m.shift, m.ctrl);
            }
            39 => {
                edit.move_right(m.shift, m.ctrl);
            }
            36 => {
                edit.home(m.shift);
            }
            35 => {
                edit.end(m.shift);
            }
            _ => {}
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn version_one_custom_presets_follow_new_builtins_and_keep_their_selection() {
        use std::fmt::Write;
        let mut old = String::from("Portfolio brushes 1\n10\t4\n");
        for family in 0..2 {
            for (slot, kind) in (0..10).chain(std::iter::once(3)).enumerate() {
                let name = if slot == 10 {
                    "Перо 雪"
                } else if slot == 4 {
                    "Уголь № 1"
                } else {
                    ""
                };
                writeln!(
                    old,
                    "{family}\t{kind}\t18\t0.8\t0.7\t0.6\t0.23\t11\t0.1\t0.5\t0.3\t{name}"
                )
                .unwrap();
            }
        }
        let mut loaded = Presets::decode(&old).unwrap();
        let first_custom = BrushKind::ALL.len();
        assert_eq!(loaded.selected, [first_custom, 4]);
        for (family, entries) in loaded.entries.iter().enumerate() {
            assert_eq!(entries.len(), first_custom + 1);
            assert_eq!(entries[4].name, "Уголь № 1");
            assert_eq!(entries[first_custom].name, "Перо 雪");
            assert_eq!(entries[first_custom].settings.kind, BrushKind::Pencil);
            assert_eq!(entries[first_custom].settings.size, 18.);
            assert_eq!(entries[first_custom].settings.texture_scale, 1.);
            assert_eq!(entries[first_custom].settings.opacity_jitter, 0.);
            assert_eq!(
                entries[13].settings,
                ToolSettings::preset(BrushKind::Twin, family == 1)
            );
        }
        loaded.entries[0][first_custom].settings.texture_scale = 1.7;
        loaded.entries[0][first_custom].settings.opacity_jitter = 0.34;
        let saved = loaded.encode();
        assert!(saved.starts_with("Portfolio brushes 2\n"));
        let restored = Presets::decode(&saved).unwrap();
        assert_eq!(restored.selected, loaded.selected);
        assert_eq!(restored.entries, loaded.entries);
    }
}
