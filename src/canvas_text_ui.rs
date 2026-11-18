//! An uncommitted text layer belongs to the canvas. Its inspector edits the same draft.
use super::*;

pub(super) struct TextDraft {
    pub session: u64,
    pub target: Option<u64>,
    pub text: TextLayer,
}

impl PortfolioApp {
    pub(super) fn current_text(&self) -> &TextLayer {
        self.draft.as_ref().map_or(&self.text_style, |d| &d.text)
    }

    pub(super) fn current_text_mut(&mut self) -> &mut TextLayer {
        self.draft
            .as_mut()
            .map_or(&mut self.text_style, |d| &mut d.text)
    }

    pub(super) fn show_text_properties(&mut self) {
        if self.tool_popup.is_some() {
            self.tool_popup = Some(ToolPanel::Text);
        }
        self.closing_popup = None;
        self.closing_overlay = None;
        self.overlay = Overlay::ToolProperties;
        self.focused_mode = false;
        self.focus = None;
        self.pending_focus = None;
        self.hover = None;
        self.controls.clear();
    }

    pub(super) fn open_canvas_text(
        &mut self,
        target: Option<u64>,
        x: u32,
        y: u32,
        out: &mut Vec<Effect>,
    ) {
        if !self.finish_canvas_text(true, out) {
            return;
        }
        let text = if let Some(id) = target {
            let Some(layer) = self.doc.layers.iter().find(|l| l.id == id) else {
                return;
            };
            if !layer.visible || layer.locked {
                self.feedback("Слой скрыт или заблокирован. Выберите доступный слой.");
                return;
            }
            let Some(text) = layer.text.clone() else {
                return;
            };
            text
        } else {
            if self.doc.layers.len() >= document::MAX_LAYERS {
                self.feedback("Достигнут предел слоёв.");
                return;
            }
            TextLayer {
                x,
                y,
                body: String::new(),
                ..self.text_style.clone()
            }
        };
        let entering = self.tool != Tool::Text;
        self.switch_tool(Tool::Text);
        self.focused_mode = false;
        if entering {
            self.tool_popup = Some(ToolPanel::Text);
        }
        self.focus = None;
        self.pending_focus = None;
        // A text layer reopened from the layer list can be offscreen or underneath a panel.
        let canvas = self.canvas();
        let ax = canvas.x + text.x as f32 * self.zoom;
        let ay = canvas.y + text.y as f32 * self.zoom;
        let stage = self.stage();
        if !stage.contains(ax, ay) || self.canvas_obscured(ax, ay) {
            let panel = self.tool_settings_rect();
            let left = if self.tool_settings_visible() {
                panel.x + panel.w
            } else {
                stage.x
            };
            let x = (left + self.right_panel_rect().x) / 2.;
            if let Some(y) = [
                ay,
                stage.y + stage.h * 0.45,
                stage.y + stage.h * 0.7,
                stage.y + stage.h * 0.25,
            ]
            .into_iter()
            .find(|&y| stage.contains(x, y) && !self.canvas_obscured(x, y))
            {
                self.view_x += x - ax;
                self.view_y += y - ay;
            }
        }
        self.text_session = self.text_session.wrapping_add(1);
        self.draft = Some(TextDraft {
            session: self.text_session,
            target,
            text,
        });
        self.place_new_text = false;
        self.composite_dirty = true;
    }

    pub(super) fn finish_canvas_text(&mut self, apply: bool, out: &mut Vec<Effect>) -> bool {
        let Some(draft) = self.draft.as_ref() else {
            return true;
        };
        let unchanged = draft
            .target
            .and_then(|id| self.doc.layers.iter().find(|l| l.id == id))
            .and_then(|l| l.text.as_ref())
            == Some(&draft.text);
        if apply && !unchanged && (!draft.text.body.trim().is_empty() || draft.target.is_some()) {
            let text = draft.text.clone();
            let target = draft.target;
            let result = text
                .validate(self.doc.width, self.doc.height)
                .and_then(|()| {
                    (self.text_renderer)(&text, self.doc.width, self.doc.height, self.language)
                })
                .and_then(|pixels| self.doc.set_text_layer(target, text.clone(), pixels));
            match result {
                Ok(changed) => {
                    self.text_style = TextLayer {
                        body: String::new(),
                        x: 0,
                        y: 0,
                        ..text
                    };
                    self.inspector_layers = true;
                    self.layer_scroll = 0;
                    if changed {
                        self.changed();
                    }
                }
                Err(error) => {
                    self.error(error, out);
                    return false;
                }
            }
        }
        self.draft = None;
        self.composite_dirty = true;
        self.pending_focus = None;
        self.focus = None;
        true
    }

    pub(super) fn canvas_edit_view(&self) -> Option<platform::CanvasEditView> {
        let draft = self.draft.as_ref()?;
        let canvas = self.canvas();
        let px = |n: f32| (n * self.scale).round() as i32;
        let bounds = |r: Rect| [px(r.x), px(r.y), px(r.x + r.w), px(r.y + r.h)];
        let mut panels = Vec::new();
        if !self.focused_mode {
            panels.extend([
                bounds(self.top_tools_rect()),
                bounds(self.right_panel_rect()),
            ]);
        }
        if self.quick_controls_visible() {
            panels.push(bounds(self.quick_controls_rect()));
        }
        if self.overlay != Overlay::None {
            panels.push(bounds(self.overlay_bounds(self.overlay)));
        }
        if let Some(r) = self.tool_popup_rect() {
            panels.push(bounds(r));
        }
        Some(platform::CanvasEditView {
            session: draft.session,
            text: draft.text.clone(),
            anchor: (
                px(canvas.x + draft.text.x as f32 * self.zoom),
                px(canvas.y + draft.text.y as f32 * self.zoom),
            ),
            font_px: px(draft.text.size as f32 * self.zoom).max(1),
            clip: bounds(canvas.intersection(self.stage())),
            panels,
        })
    }

    pub(super) fn text_action(&mut self, a: Action, out: &mut Vec<Effect>) {
        match a {
            Action::TextPreset(style) => {
                style.apply(self.current_text_mut());
            }
            Action::EffectColor => out.push(Self::prompt(
                29,
                "Цвет эффекта",
                "Шесть символов HEX, например #365AD7",
                hex(self.current_text().effects.color),
            )),
            Action::CycleTextAlign => {
                let t = self.current_text_mut();
                t.align = match t.align {
                    ParagraphAlign::Left => ParagraphAlign::Center,
                    ParagraphAlign::Center => ParagraphAlign::Right,
                    ParagraphAlign::Right => ParagraphAlign::Left,
                };
            }
            Action::NewText => {
                if self.finish_canvas_text(true, out) {
                    self.place_new_text = true;
                    self.feedback("Щёлкните по холсту для новой надписи.");
                }
            }
            Action::ApplyText | Action::CancelText => {
                self.finish_canvas_text(a == Action::ApplyText, out);
            }
            Action::TextFont(forward) => {
                let text = self.current_text_mut();
                let fonts = UiFont::ALL;
                let index = fonts.iter().position(|f| f.key() == text.font).unwrap_or(1);
                let step = if forward { 1 } else { fonts.len() - 1 };
                text.font = fonts[(index + step) % fonts.len()].key().into();
            }
            Action::TextBold => {
                let t = self.current_text_mut();
                t.bold = !t.bold;
            }
            Action::TextItalic => {
                let t = self.current_text_mut();
                t.italic = !t.italic;
            }
            Action::TextAlign(align) => self.current_text_mut().align = align,
            Action::TextColor => out.push(Self::prompt(
                26,
                "Цвет текста",
                "Шесть символов HEX, например #365AD7",
                hex(self.current_text().color),
            )),
            Action::TextX | Action::TextY if self.draft.is_some() => {
                let (id, axis, value) = if a == Action::TextX {
                    (27, "X", self.current_text().x)
                } else {
                    (28, "Y", self.current_text().y)
                };
                out.push(Self::prompt(
                    id,
                    axis,
                    "Координата в пикселях",
                    value.to_string(),
                ));
            }
            _ => {}
        }
    }

    pub(super) fn text_row(action: Action) -> Option<usize> {
        Some(match action {
            Action::TextFont(_) => 0,
            Action::TextSize | Action::ResetSlider(Slider::TextSize) => 1,
            Action::TextBold | Action::TextItalic => 2,
            Action::TextAlign(_) => 3,
            Action::TextColor => 4,
            Action::TextX | Action::TextY => 5,
            Action::TextEffect(_)
            | Action::EffectColor
            | Action::ResetSlider(Slider::TextEffect(_))
            | Action::CycleTextAlign => 0,
            _ => return None,
        })
    }

    pub(super) fn text_focus_order(&self) -> Vec<Action> {
        use ParagraphAlign::*;
        let mut items = vec![
            Action::CloseToolProperties,
            Action::ToolSection(0),
            Action::ToolSection(1),
            Action::ToolSection(2),
        ];
        if self.tool_settings_section() == 0 {
            items.extend([
                Action::TextFont(false),
                Action::TextFont(true),
                Action::TextSize,
                Action::ResetSlider(Slider::TextSize),
                Action::TextBold,
                Action::TextItalic,
                Action::TextAlign(Left),
                Action::TextAlign(Center),
                Action::TextAlign(Right),
                Action::TextColor,
            ]);
            if self.draft.is_some() {
                items.extend([Action::TextX, Action::TextY]);
            }
        } else {
            for &p in self.text_effect_params() {
                items.extend([
                    Action::TextEffect(p),
                    Action::ResetSlider(Slider::TextEffect(p)),
                ]);
            }
            items.push(Action::EffectColor);
        }
        if self.tool_settings_scroll().max() > 0. {
            items.push(Action::ToolPropertiesScroll);
        }
        items.push(Action::NewText);
        if self.draft.is_some() {
            items.extend([Action::CancelText, Action::ApplyText]);
        }
        items
    }

    pub(super) fn text_effect_params(&self) -> &'static [EffectParam] {
        use EffectParam::*;
        if self.tool_settings_section() == 2 {
            &[Depth, Direction]
        } else {
            &[Outline, Shadow, ShadowOpacity]
        }
    }
    pub(super) const TEXT_ROWS: [f32; 6] = [0., 42., 97., 137., 177., 217.];

    pub(super) fn draw_text_settings(&mut self, p: &mut Painter, panel: Rect, body: Rect) {
        let t = Theme::with_accent(self.dark, self.accent);
        let text = self.current_text().clone();
        let active = self.draft.is_some();
        let enabled = !self.busy;
        let x = body.x;
        let y = body.y - self.tool_settings_scroll().offset;
        let w = body.w
            - if self.tool_settings_scroll().max() > 0. {
                15.
            } else {
                0.
            };
        let field = x + 96.;
        let fw = w - 96.;
        let row = |i: usize| y + Self::TEXT_ROWS[i];
        p.clip = body;
        self.hit_clip = Some(body);
        if self.tool_settings_section() == 0 {
            for (i, label) in [
                (0, "Шрифт"),
                (2, "Начертание"),
                (3, "Выравнивание"),
                (4, "Цвет текста"),
                (5, "Позиция"),
            ] {
                p.text(
                    Rect::new(x, row(i), 92., 30.),
                    label,
                    11.,
                    t.muted,
                    false,
                    false,
                    TextAlign::Left,
                );
            }
            self.icon_button(
                p,
                Rect::new(field, row(0), 28., 30.),
                Action::TextFont(false),
                "left",
                "Предыдущий шрифт",
                false,
                enabled,
            );
            p.text(
                Rect::new(field + 32., row(0), fw - 64., 30.),
                UiFont::from_key(&text.font)
                    .unwrap_or(UiFont::SegoeUi)
                    .label(),
                13.,
                t.text,
                false,
                false,
                TextAlign::Center,
            );
            self.icon_button(
                p,
                Rect::new(field + fw - 28., row(0), 28., 30.),
                Action::TextFont(true),
                "right",
                "Следующий шрифт",
                false,
                enabled,
            );
            self.parameter_slider(p, Rect::new(x, row(1), w, 46.), Action::TextSize);
            let half = (fw - 4.) / 2.;
            for (i, action, label, selected) in [
                (0, Action::TextBold, "Полужирный", text.bold),
                (1, Action::TextItalic, "Курсив", text.italic),
            ] {
                self.button(
                    p,
                    Rect::new(field + i as f32 * (half + 4.), row(2), half, 30.),
                    action,
                    label,
                    None,
                    selected,
                    enabled,
                    false,
                );
            }
            let third = (fw - 8.) / 3.;
            for (i, (align, label)) in [
                (ParagraphAlign::Left, "Слева"),
                (ParagraphAlign::Center, "По центру"),
                (ParagraphAlign::Right, "Справа"),
            ]
            .into_iter()
            .enumerate()
            {
                self.button(
                    p,
                    Rect::new(field + i as f32 * (third + 4.), row(3), third, 30.),
                    Action::TextAlign(align),
                    label,
                    None,
                    text.align == align,
                    enabled,
                    false,
                );
            }
            self.button(
                p,
                Rect::new(field, row(4), fw, 30.),
                Action::TextColor,
                &hex(text.color),
                None,
                false,
                enabled,
                false,
            );
            p.rounded(Rect::new(field + 8., row(4) + 7., 16., 16.), 3., t.border);
            p.rounded(
                Rect::new(field + 9., row(4) + 8., 14., 14.),
                2.,
                rgb(text.color),
            );
            for (i, action, axis, value) in [
                (0, Action::TextX, "X", text.x),
                (1, Action::TextY, "Y", text.y),
            ] {
                let value = if active {
                    format!("{axis}  {value}")
                } else {
                    format!("{axis}  —")
                };
                self.button(
                    p,
                    Rect::new(field + i as f32 * (half + 4.), row(5), half, 30.),
                    action,
                    &value,
                    None,
                    false,
                    active && enabled,
                    false,
                );
            }
        } else {
            for (i, &param) in self.text_effect_params().iter().enumerate() {
                self.parameter_slider(
                    p,
                    Rect::new(x, y + i as f32 * 54., w, 46.),
                    Action::TextEffect(param),
                );
            }
            let cy = y + self.text_effect_params().len() as f32 * 54.;
            p.text(
                Rect::new(x, cy, 92., 30.),
                "Цвет эффекта",
                11.,
                t.muted,
                false,
                false,
                TextAlign::Left,
            );
            self.button(
                p,
                Rect::new(field, cy, fw, 30.),
                Action::EffectColor,
                &hex(text.effects.color),
                None,
                false,
                enabled,
                false,
            );
            p.rounded(
                Rect::new(field + 8., cy + 7., 16., 16.),
                3.,
                rgb(text.effects.color),
            );
        }
        p.clip = panel.inset(1.);
        self.hit_clip = Some(p.clip);
        let y = panel.y + panel.h - 44.;
        let width = (body.w - 12.) / 3.;
        for (i, action, label, available) in [
            (0, Action::NewText, "Новая надпись", true),
            (1, Action::CancelText, "Отмена", active),
            (2, Action::ApplyText, "Применить", active),
        ] {
            self.button(
                p,
                Rect::new(x + i as f32 * (width + 6.), y, width, 32.),
                action,
                label,
                None,
                false,
                available && enabled,
                action == Action::ApplyText && active,
            );
        }
    }
}

impl PortfolioApp {
    pub(super) fn draw_text_sample(
        &mut self,
        p: &mut Painter,
        rect: Rect,
        mut text: TextLayer,
        preset: Option<TextPreset>,
    ) {
        let card = preset.is_some();
        let align = if card {
            ParagraphAlign::Center
        } else {
            text.align
        };
        text.body = if card {
            "Aa".into()
        } else {
            let line = text
                .body
                .lines()
                .find(|l| !l.trim().is_empty())
                .unwrap_or("")
                .trim();
            if line.is_empty() {
                self.language.translate("Образец")
            } else {
                line.chars().take(32).collect()
            }
        };
        let w = (rect.w * self.scale).round().clamp(1., 2048.) as u32;
        let h = (rect.h * self.scale).round().clamp(1., 512.) as u32;
        let point_size = if card {
            (18. * (text.size as f32 / 48.).powf(0.25)).clamp(10., 22.)
        } else {
            (23. * (text.size as f32 / 48.).sqrt()).clamp(8., 32.)
        };
        let size = (point_size * self.scale).round().max(1.) as u32;
        text.effects = text.effects.scaled(size as f32 / text.size as f32);
        text.size = size;
        text.align = align;
        let pad = (5. * self.scale).round() as u32;
        text.x = match align {
            ParagraphAlign::Left => pad.min(w - 1),
            ParagraphAlign::Center => w / 2,
            ParagraphAlign::Right => w.saturating_sub(pad + 1),
        };
        text.y = (h.saturating_sub((size as f32 * 1.32) as u32) / 2)
            .saturating_sub(u32::from(text.effects.shadow.max(text.effects.depth)) / 3);
        if self
            .text_samples
            .get(&preset)
            .is_none_or(|(old, ow, oh, lang, _)| {
                old != &text || *ow != w || *oh != h || *lang != self.language
            })
        {
            let pixels = platform::rasterize_text(&text, w, h, self.language).ok();
            self.text_samples
                .insert(preset, (text, w, h, self.language, pixels));
        }
        if let Some((text, _, _, _, bitmap)) = self.text_samples.get(&preset) {
            if let Some(bitmap) = bitmap {
                p.image(rect, w, h, bitmap, false, 1.);
            } else {
                p.text_raw(
                    rect,
                    &text.body,
                    point_size,
                    rgb(text.color),
                    text.bold,
                    false,
                    TextAlign::Center,
                );
            }
        }
    }

    pub(super) fn draw_text_header(&mut self, p: &mut Painter, panel: Rect) {
        let text = self.current_text().clone();
        let preview = Rect::new(panel.x + 94., panel.y + 7., panel.w - 152., 39.);
        let ink = text.color;
        let luma =
            0.2126 * f32::from(ink.r) + 0.7152 * f32::from(ink.g) + 0.0722 * f32::from(ink.b);
        p.rounded(preview, 4., if luma < 135. { 0xeeeae3 } else { 0x26262b });
        self.draw_text_sample(p, preview.inset(1.), text, None);
        let width = (panel.w - 40.) / 3.;
        for (i, label) in ["Шрифт", "Эффекты", "3D"].into_iter().enumerate() {
            self.button(
                p,
                Rect::new(
                    panel.x + 16. + i as f32 * (width + 4.),
                    panel.y + 53.,
                    width,
                    29.,
                ),
                Action::ToolSection(i as u8),
                label,
                None,
                self.tool_settings_section() == i as u8,
                true,
                false,
            );
        }
    }

    pub(super) fn draw_text_basics(&mut self, p: &mut Painter) {
        let row = self.quick_controls_rect();
        let text = self.current_text().clone();
        let t = Theme::with_accent(self.dark, self.accent);
        self.occlude_underlying_surface(p, row);
        self.floating_surface(p, row, "Оформление текста");
        let clip = p.clip;
        p.clip = row.inset(1.);
        self.hit_clip = Some(p.clip);
        let size_width = (row.w * 0.19).clamp(50., 82.);
        let button_width = (row.w - size_width - 26.) / 7.;
        let size = Rect::new(row.x + 6., row.y + 4., size_width, row.h - 8.);
        self.button(
            p,
            size,
            Action::TextSize,
            "",
            None,
            false,
            !self.busy,
            false,
        );
        if let Some(c) = self.controls.last_mut() {
            c.label = "Размер шрифта".into();
        }
        p.icon(Rect::new(size.x, size.y, 24., size.h), "type_size", t.text);
        p.text(
            Rect::new(size.x + 22., size.y, size.w - 23., size.h),
            text.size.to_string(),
            12.,
            t.text,
            false,
            false,
            TextAlign::Center,
        );
        let align = match text.align {
            ParagraphAlign::Left => "align_left",
            ParagraphAlign::Center => "align_center",
            ParagraphAlign::Right => "align_right",
        };
        for (i, (a, icon, label, selected, enabled)) in [
            (
                Action::TextFont(true),
                "typeface",
                "Следующий шрифт",
                false,
                true,
            ),
            (Action::TextBold, "bold", "Полужирный", text.bold, true),
            (Action::TextItalic, "italic", "Курсив", text.italic, true),
            (Action::CycleTextAlign, align, "Выравнивание", false, true),
            (Action::TextColor, "", "Цвет текста", false, true),
            (
                Action::NewText,
                "plus",
                "Новая надпись",
                self.place_new_text,
                true,
            ),
            (
                Action::ApplyText,
                "check",
                "Применить",
                false,
                self.draft.is_some(),
            ),
        ]
        .into_iter()
        .enumerate()
        {
            let r = Rect::new(
                size.x + size.w + 2. + i as f32 * (button_width + 2.),
                size.y,
                button_width,
                size.h,
            );
            self.icon_button(p, r, a, icon, label, selected, enabled && !self.busy);
            if a == Action::TextColor {
                p.circle(r.x + r.w / 2., r.y + r.h / 2., 8., t.muted, true);
                p.circle(r.x + r.w / 2., r.y + r.h / 2., 6.5, rgb(text.color), true);
            }
        }
        p.clip = clip;
        self.hit_clip = None;
    }

    pub(super) fn draw_draft_effects(&mut self, p: &mut Painter, canvas: Rect, stage: Rect) {
        let Some(draft) = self
            .draft
            .as_ref()
            .filter(|d| d.text.effects.visible() && !d.text.body.trim().is_empty())
        else {
            self.draft_effects = None;
            return;
        };
        let (w, h) = (self.doc.width, self.doc.height);
        if self
            .draft_effects
            .as_ref()
            .is_none_or(|(old, ow, oh, lang, _)| {
                old != &draft.text || *ow != w || *oh != h || *lang != self.language
            })
        {
            let tile = platform::render_text_tile(&draft.text, w, h, self.language, false).ok();
            self.draft_effects = Some((draft.text.clone(), w, h, self.language, tile));
        }
        if let Some((_, _, _, _, Some(tile))) = &self.draft_effects {
            let clip = p.clip;
            p.clip = canvas.intersection(stage);
            let r = Rect::new(
                canvas.x + tile.x as f32 * self.zoom,
                canvas.y + tile.y as f32 * self.zoom,
                tile.w as f32 * self.zoom,
                tile.h as f32 * self.zoom,
            );
            let opacity = draft
                .target
                .and_then(|id| self.doc.layers.iter().find(|l| l.id == id))
                .map_or(1., |l| l.opacity as f32 / 255.);
            p.image(r, tile.w, tile.h, &tile.pixels, self.grayscale, opacity);
            p.clip = clip;
        }
    }
}
