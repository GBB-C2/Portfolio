//! Searchable appearance settings. This is an app child module so the drawing
//! and input paths share the editor's controls, clipping and focus machinery.
use super::*;

#[derive(Clone, Debug, Default)]
pub(super) struct SettingsUi {
    pub query: crate::tool_browser::SearchText,
    pub focused: bool,
    pub scroll: crate::tool_browser::Scroll,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Field {
    Theme,
    Accent,
    Animations,
    Font,
    FontSize,
    Sample,
    Language,
}

impl Field {
    fn section(self) -> u8 {
        match self {
            Self::Theme | Self::Accent | Self::Animations => 0,
            Self::Font | Self::FontSize | Self::Sample => 1,
            Self::Language => 2,
        }
    }

    fn height(self, extra: f32) -> f32 {
        match self {
            Self::Theme => 72. + extra,
            Self::Accent => 86. + extra,
            Self::Animations => 102. + extra * 2.,
            Self::Font | Self::Language => 82. + extra,
            Self::FontSize => 62. + extra,
            Self::Sample => 38. + extra,
        }
    }

    fn labels(self) -> &'static [&'static str] {
        match self {
            Self::Theme => &["Тема", "Графит", "Бумага"],
            Self::Accent => &["Акцент"],
            Self::Animations => &[
                "Анимации панелей",
                "Плавно",
                "Без анимаций",
                "Учитываются настройки Windows",
            ],
            Self::Font => &["Шрифт интерфейса"],
            Self::FontSize => &["Размер текста"],
            Self::Sample => &["Образец текста · Portfolio 0123"],
            Self::Language => &["Язык интерфейса"],
        }
    }

    fn aliases(self) -> &'static str {
        match self {
            Self::Theme => "theme dark light graphite paper",
            Self::Accent => "accent color colour red blue cobalt violet purple amber coral",
            Self::Animations => "animation animations motion smooth reduced motion panels windows",
            Self::Font => "font fonts family typeface interface",
            Self::FontSize => "font text size scale px",
            Self::Sample => "sample preview example portfolio 0123",
            Self::Language => {
                "language locale translation russian english german korean chinese japanese"
            }
        }
    }

    fn action(self, app: &PortfolioApp) -> Option<Action> {
        Some(match self {
            Self::Theme => Action::SetTheme(app.dark),
            Self::Accent => Action::SetAccent(app.accent),
            Self::Animations => Action::SetAnimations(app.animations),
            Self::Font => Action::FontMenu,
            Self::FontSize => Action::FontSize,
            Self::Language => Action::LanguageMenu,
            Self::Sample => return None,
        })
    }
}

const FIELDS: [Field; 7] = [
    Field::Theme,
    Field::Accent,
    Field::Animations,
    Field::Font,
    Field::FontSize,
    Field::Sample,
    Field::Language,
];
const SECTIONS: [&str; 3] = ["Вид", "Текст", "Язык"];
const SECTION_ALIASES: [&str; 3] = [
    "appearance view look design",
    "text typography",
    "language locale localization localisation",
];

struct Row {
    field: Field,
    top: f32,
    height: f32,
}

struct Group {
    section: u8,
    top: f32,
    rows: Vec<Row>,
}

struct Layout {
    groups: Vec<Group>,
    extent: f32,
}

fn folded(value: &str) -> String {
    value.to_lowercase().replace('ß', "ss").replace('ς', "σ")
}

fn matches_tokens(value: &str, tokens: &[String]) -> bool {
    let value = folded(value);
    tokens.iter().all(|token| value.contains(token))
}

impl PortfolioApp {
    fn settings_layout(&self) -> Layout {
        let tokens: Vec<String> = self
            .settings_ui
            .query
            .text()
            .split_whitespace()
            .map(folded)
            .collect();
        let extra = (self.font_size as f32 - 13.).max(0.) * 1.5;
        let mut top = 0.;
        let mut groups = Vec::new();
        for section in 0..3u8 {
            let name = SECTIONS[section as usize];
            let group_terms = format!(
                "{} {} {}",
                name,
                self.language.translate(name),
                SECTION_ALIASES[section as usize]
            );
            let whole_group = matches_tokens(&group_terms, &tokens);
            let mut matching = Vec::new();
            for field in FIELDS
                .into_iter()
                .filter(|field| field.section() == section)
            {
                let mut terms = format!("{} {}", group_terms, field.aliases());
                for label in field.labels() {
                    terms.push(' ');
                    terms.push_str(label);
                    terms.push(' ');
                    terms.push_str(&self.language.translate(label));
                }
                match field {
                    Field::Accent => {
                        for accent in Accent::ALL {
                            terms.push(' ');
                            terms.push_str(accent.label());
                            terms.push(' ');
                            terms.push_str(&self.language.translate(accent.label()));
                        }
                    }
                    Field::Font => {
                        for font in UiFont::ALL {
                            terms.push(' ');
                            terms.push_str(font.label());
                        }
                    }
                    Field::Language => {
                        for language in Language::ALL {
                            terms.push(' ');
                            terms.push_str(language.label());
                            terms.push(' ');
                            terms.push_str(language.key());
                        }
                    }
                    _ => {}
                }
                if whole_group || matches_tokens(&terms, &tokens) {
                    matching.push(field);
                }
            }
            if matching.is_empty() {
                continue;
            }
            if !groups.is_empty() {
                top += 14.;
            }
            let group_top = top;
            top += 32. + extra;
            let mut rows = Vec::new();
            for field in matching {
                let height = field.height(extra);
                rows.push(Row { field, top, height });
                top += height;
            }
            groups.push(Group {
                section,
                top: group_top,
                rows,
            });
        }
        Layout {
            groups,
            extent: if top > 0. { top + 4. } else { 0. },
        }
    }

    pub(super) fn settings_rect(&self) -> Rect {
        Rect::new(
            (self.w() - 416.).max(16.),
            72.,
            400.,
            (self.h() - 88.).clamp(112., 680.),
        )
    }

    pub(super) fn settings_search_rect(&self) -> Rect {
        let panel = self.settings_rect();
        Rect::new(panel.x + 12., panel.y + 12., panel.w - 60., 32.)
    }

    pub(super) fn settings_viewport(&self) -> Rect {
        let panel = self.settings_rect();
        Rect::new(
            panel.x + 12.,
            panel.y + 56.,
            panel.w - 36.,
            (panel.h - 112.).max(0.),
        )
    }

    pub(super) fn settings_scrollbar_track(&self) -> Rect {
        let panel = self.settings_rect();
        let viewport = self.settings_viewport();
        Rect::new(panel.x + panel.w - 20., viewport.y, 12., viewport.h)
    }

    pub(super) fn settings_scrollbar_thumb(&self) -> Rect {
        let track = self.settings_scrollbar_track();
        let scroll = self.settings_ui.scroll;
        let height = if scroll.extent <= 0. {
            track.h
        } else {
            (track.h * (scroll.viewport / scroll.extent).clamp(0., 1.))
                .max(32.)
                .min(track.h)
        };
        let fraction = if scroll.max() > 0. {
            scroll.offset / scroll.max()
        } else {
            0.
        };
        Rect::new(
            track.x + 2.,
            track.y + (track.h - height) * fraction,
            track.w - 4.,
            height,
        )
    }

    pub(super) fn prepare_settings_layout(&mut self) {
        let extent = self.settings_layout().extent;
        self.settings_ui
            .scroll
            .set_extent(extent, self.settings_viewport().h);
    }

    fn settings_invalidate_hits(&mut self) {
        self.controls.clear();
        self.focus = None;
        self.pending_focus = None;
        self.hover = None;
    }

    pub(super) fn settings_scroll_to(&mut self, offset: f32) -> bool {
        self.prepare_settings_layout();
        let changed = self.settings_ui.scroll.set(offset);
        if changed {
            self.settings_invalidate_hits();
        }
        changed
    }

    pub(super) fn settings_scroll_by(&mut self, delta: f32) -> bool {
        self.prepare_settings_layout();
        let changed = self.settings_ui.scroll.scroll_by(delta);
        if changed {
            self.settings_invalidate_hits();
        }
        changed
    }

    #[cfg(test)]
    pub(super) fn settings_section_rect(&self, section: u8) -> Rect {
        let viewport = self.settings_viewport();
        let extra = (self.font_size as f32 - 13.).max(0.) * 1.5;
        self.settings_layout()
            .groups
            .iter()
            .find(|group| group.section == section)
            .map_or(Rect::new(viewport.x, viewport.y, viewport.w, 0.), |group| {
                Rect::new(
                    viewport.x + 8.,
                    viewport.y + group.top - self.settings_ui.scroll.offset,
                    viewport.w - 16.,
                    28. + extra,
                )
            })
    }

    fn settings_row(&self, field: Field) -> Option<Row> {
        self.settings_layout()
            .groups
            .into_iter()
            .flat_map(|group| group.rows)
            .find(|row| row.field == field)
    }

    pub(super) fn settings_focus_action(&self) -> Action {
        Action::SettingsSearch
    }

    /// Logical reading order includes rows below the viewport. A caller reveals
    /// the target first and applies pending_focus when that row is next drawn.
    pub(super) fn settings_focus_order(&self) -> Vec<Action> {
        let layout = self.settings_layout();
        let mut actions = vec![Action::SettingsSearch];
        if !self.settings_ui.query.text().is_empty() {
            actions.push(Action::ClearSettingsSearch);
        }
        actions.push(Action::CloseSettings);
        for row in layout.groups.into_iter().flat_map(|group| group.rows) {
            match row.field {
                Field::Theme => actions.extend([Action::SetTheme(true), Action::SetTheme(false)]),
                Field::Accent => actions.extend(Accent::ALL.into_iter().map(Action::SetAccent)),
                Field::Animations => {
                    actions.extend([Action::SetAnimations(true), Action::SetAnimations(false)])
                }
                Field::Font => actions.push(Action::FontMenu),
                Field::FontSize => {
                    actions.extend([Action::FontSize, Action::ResetSlider(Slider::FontSize)])
                }
                Field::Language => actions.push(Action::LanguageMenu),
                Field::Sample => {}
            }
        }
        if layout.extent > self.settings_viewport().h {
            actions.push(Action::SettingsScroll);
        }
        actions.push(Action::ResetAppearance);
        actions
    }

    pub(super) fn settings_reveal_action(&mut self, action: Action) -> bool {
        let field = match action {
            Action::SetTheme(_) => Field::Theme,
            Action::SetAccent(_) => Field::Accent,
            Action::SetAnimations(_) => Field::Animations,
            Action::FontMenu => Field::Font,
            Action::FontSize | Action::ResetSlider(Slider::FontSize) => Field::FontSize,
            Action::LanguageMenu => Field::Language,
            Action::ClearSettingsSearch => return !self.settings_ui.query.text().is_empty(),
            Action::SettingsSearch | Action::CloseSettings | Action::ResetAppearance => {
                return true
            }
            Action::SettingsScroll => {
                self.prepare_settings_layout();
                return self.settings_ui.scroll.max() > 0.;
            }
            Action::SettingsSection(section) => return self.settings_reveal_section(section),
            _ => return false,
        };
        let Some(row) = self.settings_row(field) else {
            return false;
        };
        self.prepare_settings_layout();
        if self
            .settings_ui
            .scroll
            .ensure_visible(row.top, row.top + row.height)
        {
            self.settings_invalidate_hits();
        }
        true
    }

    pub(super) fn choice_field_rect(&self, choice: SettingsChoice) -> Rect {
        let viewport = self.settings_viewport();
        let extra = (self.font_size as f32 - 13.).max(0.) * 1.5;
        let field = match choice {
            SettingsChoice::Font => Field::Font,
            SettingsChoice::Language => Field::Language,
        };
        self.settings_row(field).map_or(
            Rect::new(viewport.x + 8., viewport.y, viewport.w - 16., 0.),
            |row| {
                Rect::new(
                    viewport.x + 8.,
                    viewport.y + row.top + 28. + extra - self.settings_ui.scroll.offset,
                    viewport.w - 16.,
                    38.,
                )
            },
        )
    }

    /// Whether the field belongs to the current result set. The separate reveal
    /// method scrolls its complete row into view before opening a dropdown.
    pub(super) fn settings_field_visible(&self, choice: SettingsChoice) -> bool {
        self.settings_row(match choice {
            SettingsChoice::Font => Field::Font,
            SettingsChoice::Language => Field::Language,
        })
        .is_some()
    }

    pub(super) fn settings_reveal_field(&mut self, choice: SettingsChoice) -> bool {
        let Some(row) = self.settings_row(match choice {
            SettingsChoice::Font => Field::Font,
            SettingsChoice::Language => Field::Language,
        }) else {
            return false;
        };
        self.prepare_settings_layout();
        if self
            .settings_ui
            .scroll
            .ensure_visible(row.top, row.top + row.height)
        {
            self.settings_invalidate_hits();
        }
        true
    }

    pub(super) fn settings_reveal_section(&mut self, section: u8) -> bool {
        let Some(group) = self
            .settings_layout()
            .groups
            .into_iter()
            .find(|group| group.section == section)
        else {
            return false;
        };
        self.prepare_settings_layout();
        if self.settings_ui.scroll.set(group.top) {
            self.settings_invalidate_hits();
        }
        true
    }

    pub(super) fn settings_query_changed(&mut self) {
        self.settings_ui.scroll.set(0.);
        self.settings_choice = None;
        self.closing_choice = None;
        self.choice_motion.snap(false);
        self.prepare_settings_layout();
        self.settings_invalidate_hits();
    }

    pub(super) fn settings_text_input(&mut self, value: &str) {
        if self.overlay == Overlay::Settings
            && self.settings_choice.is_none()
            && self.settings_ui.focused
            && self.settings_ui.query.insert(value)
        {
            self.settings_query_changed();
        }
    }

    pub(super) fn settings_search_key(
        &mut self,
        code: u32,
        pressed: bool,
        repeat: bool,
        modifiers: Modifiers,
        out: &mut Vec<Effect>,
    ) -> bool {
        if self.overlay != Overlay::Settings || self.settings_choice.is_some() {
            return false;
        }
        if modifiers.ctrl && code == 70 {
            if pressed && !repeat {
                self.finish_drag();
                self.settings_ui.focused = true;
                self.settings_ui.query.end(false);
                self.focus = None;
                self.pending_focus = None;
                self.space = false;
            }
            return true;
        }
        if !self.settings_ui.focused {
            return false;
        }
        self.space = false;
        if !pressed {
            return true;
        }
        if matches!(code, 9 | 117 | 115) && !modifiers.ctrl && !modifiers.alt {
            self.settings_ui.focused = false;
            self.focus = self
                .controls
                .iter()
                .position(|control| control.enabled && control.action == Action::SettingsSearch);
            if self.focus.is_none() {
                // Text input may have invalidated the previous controls. Keep
                // the semantic origin for the parent's Tab/F6 traversal.
                self.pending_focus = Some(Action::SettingsSearch);
            }
            return false;
        }
        if code == 27 {
            self.settings_ui.focused = false;
            self.focus = None;
            self.pending_focus = Some(Action::SettingsSearch);
            return true;
        }
        if code == 13 && !modifiers.ctrl && !modifiers.alt {
            let first = self
                .settings_layout()
                .groups
                .into_iter()
                .flat_map(|group| group.rows)
                .find_map(|row| row.field.action(self).map(|action| (row, action)));
            if let Some((row, action)) = first {
                self.settings_ui.focused = false;
                self.prepare_settings_layout();
                self.settings_ui
                    .scroll
                    .ensure_visible(row.top, row.top + row.height);
                self.settings_invalidate_hits();
                self.pending_focus = Some(action);
            }
            return true;
        }
        let query = &mut self.settings_ui.query;
        let changed = if modifiers.ctrl {
            match code {
                65 => {
                    query.select_all();
                    false
                }
                67 | 88 => {
                    let selected = query.selected_text();
                    if selected.is_empty() {
                        false
                    } else {
                        out.push(Effect::CopyText(selected));
                        code == 88 && query.backspace()
                    }
                }
                86 => {
                    out.push(Effect::PasteText);
                    false
                }
                37 => {
                    query.move_left(modifiers.shift, true);
                    false
                }
                39 => {
                    query.move_right(modifiers.shift, true);
                    false
                }
                36 => {
                    query.home(modifiers.shift);
                    false
                }
                35 => {
                    query.end(modifiers.shift);
                    false
                }
                _ => false,
            }
        } else {
            match code {
                8 => query.backspace(),
                46 => query.delete(),
                37 => {
                    query.move_left(modifiers.shift, false);
                    false
                }
                39 => {
                    query.move_right(modifiers.shift, false);
                    false
                }
                36 => {
                    query.home(modifiers.shift);
                    false
                }
                35 => {
                    query.end(modifiers.shift);
                    false
                }
                _ => false,
            }
        };
        if changed {
            self.settings_query_changed();
        }
        true
    }

    pub(super) fn draw_settings(&mut self, p: &mut Painter) {
        let choice_input = if self.settings_choice.is_some() {
            let saved = (self.focus, self.hover, self.pending_focus);
            self.focus = None;
            self.hover = None;
            self.pending_focus = None;
            Some(saved)
        } else {
            None
        };
        self.prepare_settings_layout();
        let layout = self.settings_layout();
        let t = Theme::with_accent(self.dark, self.accent);
        let panel = self.settings_rect();
        let viewport = self.settings_viewport();
        self.controls.clear();
        let visible = self
            .motion_hit_clip
            .map_or(panel, |clip| panel.intersection(clip));
        p.frame
            .texts
            .retain(|run| !visible.intersects(Rect::new(run.x, run.y, run.width, run.height)));
        self.dropdown_surface(p, panel, self.w() - 165., "Настройки");
        p.clip = panel.inset(1.);
        self.hit_clip = Some(p.clip);
        self.draw_settings_search(p);
        self.icon_button(
            p,
            Rect::new(panel.x + panel.w - 40., panel.y + 14., 28., 28.),
            Action::CloseSettings,
            "close",
            "Закрыть настройки · Esc",
            false,
            true,
        );
        p.clip = viewport;
        self.hit_clip = Some(viewport);
        if layout.groups.is_empty() {
            p.text(
                Rect::new(viewport.x + 8., viewport.y + 28., viewport.w - 16., 30.),
                "Ничего не найдено",
                13.,
                t.text,
                true,
                false,
                TextAlign::Center,
            );
            p.text(
                Rect::new(viewport.x + 8., viewport.y + 62., viewport.w - 16., 30.),
                "Измените запрос или очистите поиск",
                10.,
                t.muted,
                false,
                false,
                TextAlign::Center,
            );
        }
        let extra = (self.font_size as f32 - 13.).max(0.) * 1.5;
        for group in layout.groups {
            p.text(
                Rect::new(
                    viewport.x + 8.,
                    viewport.y + group.top - self.settings_ui.scroll.offset,
                    viewport.w - 16.,
                    28. + extra,
                ),
                SECTIONS[group.section as usize],
                14.,
                t.text,
                true,
                false,
                TextAlign::Left,
            );
            for row in group.rows {
                let rect = Rect::new(
                    viewport.x + 8.,
                    viewport.y + row.top - self.settings_ui.scroll.offset,
                    viewport.w - 16.,
                    row.height,
                );
                if rect.intersects(viewport) {
                    self.draw_settings_row(p, row.field, rect, extra);
                }
            }
        }
        p.clip = panel.inset(1.);
        self.hit_clip = Some(p.clip);
        if self.settings_ui.scroll.max() > 0. {
            let track = self.settings_scrollbar_track();
            let thumb = self.settings_scrollbar_thumb();
            let highlighted =
                self.focus == Some(self.controls.len()) || self.hover == Some(self.controls.len());
            p.rounded(Rect::new(track.x + 4., track.y, 4., track.h), 2., t.raised);
            p.rounded(thumb, 4., if highlighted { t.accent } else { t.muted });
            self.control_with_kind(
                track,
                Action::SettingsScroll,
                "Прокрутка настроек",
                true,
                ControlKind::Slider,
            );
        }
        let footer = panel.y + panel.h - 48.;
        self.button(
            p,
            Rect::new(panel.x + 20., footer + 2., 142., 30.),
            Action::ResetAppearance,
            "Сбросить вид",
            None,
            false,
            true,
            false,
        );
        p.text(
            Rect::new(panel.x + 174., footer + 2., 206., 30.),
            "Сохраняется автоматически",
            10.,
            t.muted,
            false,
            false,
            TextAlign::Right,
        );
        self.hit_clip = None;
        p.clip = Rect::new(0., 0., self.w(), self.h());
        if let Some((focus, hover, pending)) = choice_input {
            self.focus = focus;
            self.hover = hover;
            self.pending_focus = pending;
        }
        if self.settings_choice.is_some() || self.closing_choice.is_some() {
            self.draw_choice_layer(p);
        }
    }

    fn draw_settings_row(&mut self, p: &mut Painter, field: Field, r: Rect, extra: f32) {
        let t = Theme::with_accent(self.dark, self.accent);
        let half = (r.w - 12.) / 2.;
        let label = |p: &mut Painter, text: &str| {
            p.text(
                Rect::new(r.x, r.y, r.w, 22. + extra),
                text,
                11.,
                t.muted,
                false,
                false,
                TextAlign::Left,
            );
        };
        match field {
            Field::Theme => {
                label(p, "Тема");
                for (i, (dark, name)) in [(true, "Графит"), (false, "Бумага")]
                    .into_iter()
                    .enumerate()
                {
                    self.button(
                        p,
                        Rect::new(r.x + i as f32 * (half + 12.), r.y + 28. + extra, half, 34.),
                        Action::SetTheme(dark),
                        name,
                        None,
                        self.dark == dark,
                        true,
                        false,
                    );
                }
            }
            Field::Accent => {
                label(p, "Акцент");
                let gap = 6.;
                let width = (r.w - gap * (Accent::ALL.len() - 1) as f32) / Accent::ALL.len() as f32;
                for (i, accent) in Accent::ALL.into_iter().enumerate() {
                    let card = Rect::new(
                        r.x + i as f32 * (width + gap),
                        r.y + 28. + extra,
                        width,
                        48.,
                    );
                    let before = self.controls.len();
                    self.button(
                        p,
                        card,
                        Action::SetAccent(accent),
                        "",
                        None,
                        self.accent == accent,
                        true,
                        false,
                    );
                    if self.controls.len() > before {
                        self.controls.last_mut().unwrap().label = accent.label().into();
                    }
                    p.circle(
                        card.x + card.w / 2.,
                        card.y + 13.,
                        6.,
                        accent.color(self.dark),
                        true,
                    );
                    p.text(
                        Rect::new(card.x + 2., card.y + 24., card.w - 4., 22.),
                        accent.label(),
                        9.,
                        t.text,
                        false,
                        false,
                        TextAlign::Center,
                    );
                }
            }
            Field::Animations => {
                label(p, "Анимации панелей");
                for (i, (animated, name)) in [(true, "Плавно"), (false, "Без анимаций")]
                    .into_iter()
                    .enumerate()
                {
                    self.button(
                        p,
                        Rect::new(r.x + i as f32 * (half + 12.), r.y + 28. + extra, half, 34.),
                        Action::SetAnimations(animated),
                        name,
                        None,
                        self.animations == animated,
                        true,
                        false,
                    );
                }
                p.text(
                    Rect::new(r.x, r.y + 68. + extra, r.w, 24. + extra),
                    "Учитываются настройки Windows",
                    10.,
                    t.muted,
                    false,
                    false,
                    TextAlign::Left,
                );
            }
            Field::Font | Field::Language => {
                let (choice, value, text) = if field == Field::Font {
                    (
                        SettingsChoice::Font,
                        self.ui_font.label(),
                        "Шрифт интерфейса",
                    )
                } else {
                    (
                        SettingsChoice::Language,
                        self.language.label(),
                        "Язык интерфейса",
                    )
                };
                label(p, text);
                // draw_choice_field updates the newly registered control label.
                // Do not call it when clipping would omit that control.
                let viewport = self.settings_viewport();
                let visible = self
                    .motion_hit_clip
                    .map_or(viewport, |clip| viewport.intersection(clip));
                if self.choice_field_rect(choice).intersects(visible) {
                    self.draw_choice_field(p, choice, value);
                }
            }
            Field::FontSize => self.parameter_slider(
                p,
                Rect::new(r.x, r.y + extra / 2., r.w, 46.),
                Action::FontSize,
            ),
            Field::Sample => p.text(
                Rect::new(r.x, r.y, r.w, 30. + extra),
                "Образец текста · Portfolio 0123",
                13.,
                t.text,
                false,
                false,
                TextAlign::Left,
            ),
        }
    }

    fn draw_settings_search(&mut self, p: &mut Painter) {
        let r = self.settings_search_rect();
        let t = Theme::with_accent(self.dark, self.accent);
        let focused = self.settings_ui.focused && self.settings_choice.is_none();
        let text = self.settings_ui.query.text().to_owned();
        let caret = self.settings_ui.query.caret();
        let selection = self.settings_ui.query.selection();
        p.rounded(r, 6., t.raised);
        p.icon(Rect::new(r.x + 6., r.y + 6., 20., 20.), "search", t.muted);
        self.control(r, Action::SettingsSearch, "Поиск настроек", true);
        let input = Rect::new(
            r.x + 32.,
            r.y + 3.,
            r.w - if text.is_empty() { 40. } else { 66. },
            26.,
        );
        let clip = p.clip;
        p.clip = input.intersection(clip);
        if text.is_empty() && !focused {
            p.text(
                input,
                "Поиск настроек…",
                11.,
                t.muted,
                false,
                false,
                TextAlign::Left,
            );
        } else {
            let mut start = 0;
            if focused {
                while start < caret && self.browser_text_width(&text[start..caret]) > input.w - 4. {
                    start += text[start..].chars().next().unwrap().len_utf8();
                }
                if let Some((a, b)) = selection {
                    let a = a.max(start);
                    let b = b.max(start);
                    let x = input.x + self.browser_text_width(&text[start..a]);
                    let width = self.browser_text_width(&text[a..b]);
                    p.rect(Rect::new(x, input.y + 3., width, input.h - 6.), t.selected);
                }
            }
            p.text_raw(
                input,
                &text[start..],
                11.,
                t.text,
                false,
                false,
                TextAlign::Left,
            );
            if focused {
                let x = input.x + self.browser_text_width(&text[start..caret]);
                p.rect(
                    Rect::new(
                        x.min(input.x + input.w - 1.),
                        input.y + 4.,
                        1.,
                        input.h - 8.,
                    ),
                    t.accent,
                );
            }
        }
        p.clip = clip;
        if !text.is_empty() {
            self.icon_button(
                p,
                Rect::new(r.x + r.w - 28., r.y + 5., 22., 22.),
                Action::ClearSettingsSearch,
                "close",
                "Очистить поиск",
                false,
                true,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fields(app: &mut PortfolioApp, query: &str) -> Vec<Field> {
        app.settings_ui.query.clear();
        app.settings_ui.query.insert(query);
        app.settings_query_changed();
        app.settings_layout()
            .groups
            .into_iter()
            .flat_map(|group| group.rows.into_iter().map(|row| row.field))
            .collect()
    }

    #[test]
    fn settings_search_matches_groups_localized_fields_and_all_tokens() {
        let mut app = PortfolioApp::new(false);
        assert_eq!(fields(&mut app, ""), FIELDS);
        assert_eq!(
            fields(&mut app, "ТЕКСТ"),
            vec![Field::Font, Field::FontSize, Field::Sample]
        );
        assert_eq!(fields(&mut app, "font size"), vec![Field::FontSize]);
        assert_eq!(fields(&mut app, "красный"), vec![Field::Accent]);
        assert_eq!(fields(&mut app, "CoZeTtE"), vec![Field::Font]);
        assert_eq!(fields(&mut app, "motion"), vec![Field::Animations]);
        app.language = Language::German;
        assert_eq!(fields(&mut app, "OBERFLÄCHENSCHRIFT"), vec![Field::Font]);
        assert_eq!(fields(&mut app, "Sprache"), vec![Field::Language]);
        assert_eq!(fields(&mut app, "ТЕМА GRAPHITE"), vec![Field::Theme]);
        app.language = Language::Japanese;
        assert_eq!(fields(&mut app, "言語"), vec![Field::Language]);
        assert!(fields(&mut app, "unmatched-setting-xyz").is_empty());
        assert_eq!(app.settings_ui.scroll.max(), 0.);
    }

    #[test]
    fn settings_scroll_reveals_offscreen_fields_and_filters_focus_order() {
        let mut app = PortfolioApp::new(false);
        app.resize(884, 581, 1.);
        app.font_size = 17;
        app.prepare_settings_layout();
        let panel = app.settings_rect();
        let viewport = app.settings_viewport();
        assert_eq!((panel.w, panel.h), (400., 493.));
        assert!(app.settings_ui.scroll.max() > 0.);
        let order = app.settings_focus_order();
        assert!(order.contains(&Action::FontMenu));
        assert!(order.contains(&Action::FontSize));
        assert!(order.contains(&Action::LanguageMenu));
        assert_eq!(
            order
                .iter()
                .filter(|action| **action == Action::FontSize)
                .count(),
            1
        );
        for choice in [SettingsChoice::Language, SettingsChoice::Font] {
            assert!(app.settings_reveal_action(choice.trigger()));
            let field = app.choice_field_rect(choice);
            assert!(field.y >= viewport.y);
            assert!(field.y + field.h <= viewport.y + viewport.h);
            let track = app.settings_scrollbar_track();
            let thumb = app.settings_scrollbar_thumb();
            assert!(thumb.y >= track.y && thumb.y + thumb.h <= track.y + track.h + 0.001);
        }
        fields(&mut app, "font size");
        assert_eq!(app.settings_ui.scroll.offset, 0.);
        assert_eq!(
            app.settings_focus_order(),
            vec![
                Action::SettingsSearch,
                Action::ClearSettingsSearch,
                Action::CloseSettings,
                Action::FontSize,
                Action::ResetSlider(Slider::FontSize),
                Action::ResetAppearance,
            ]
        );
        assert!(!app.settings_field_visible(SettingsChoice::Language));
        assert!(!app.settings_reveal_field(SettingsChoice::Language));
    }
}
