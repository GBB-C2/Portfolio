//! A searchable dropdown for the document typeface, anchored to the quick text row.
use super::*;
use crate::tool_browser::{Scroll, SearchText};

const ROW: f32 = 32.;

#[derive(Default)]
pub(super) struct TextFontPicker {
    pub open: bool,
    pub query: SearchText,
    pub scroll: Scroll,
    fonts: Vec<UiFont>,
    search_focus: bool,
    search_reveal: Option<Reveal>,
    highlighted: usize,
}

impl TextFontPicker {
    pub(super) fn search_animating(&self) -> bool {
        self.open && self.search_reveal.as_ref().is_some_and(Reveal::active)
    }

    fn focus_search(&mut self) {
        self.search_focus = true;
        self.search_reveal.get_or_insert_with(|| {
            let mut reveal = Reveal::new(false);
            reveal.set(true);
            reveal
        });
    }

    fn filter_fonts(&mut self) {
        let normalize = |s: &str| {
            s.chars()
                .filter(|c| c.is_alphanumeric())
                .flat_map(char::to_lowercase)
                .collect::<String>()
        };
        let key = normalize(self.query.text());
        self.fonts = UiFont::ALL
            .into_iter()
            .filter(|font| normalize(font.label()).contains(&key))
            .collect();
    }
}

impl PortfolioApp {
    pub fn quick_text_font_rect(&self) -> Rect {
        let row = self.quick_controls_rect();
        let size = (row.w * 0.19).clamp(50., 82.);
        Rect::new(
            row.x + 8. + size,
            row.y + 4.,
            (row.w * 0.32).clamp(62., 154.),
            row.h - 8.,
        )
    }
    pub(super) fn font_menu_rect(&self) -> Rect {
        let anchor = self.quick_text_font_rect();
        let width = 252_f32.min((self.w() - 16.).max(1.));
        let below = anchor.y + anchor.h + 8.;
        let space_below = self.h() - below - 8.;
        let space_above = anchor.y - 16.;
        let down = space_below >= 238. || space_below >= space_above;
        let h = (if down { space_below } else { space_above }).clamp(1., 238.);
        Rect::new(
            anchor.x.clamp(8., (self.w() - width - 8.).max(8.)),
            if down { below } else { anchor.y - h - 8. },
            width,
            h,
        )
    }
    pub(super) fn font_list_rect(&self) -> Rect {
        let panel = self.font_menu_rect();
        Rect::new(
            panel.x + 6.,
            panel.y + 6.,
            panel.w - 12.,
            (panel.h - 48.).max(0.),
        )
    }
    pub(super) fn font_at(&self, x: f32, y: f32) -> Option<UiFont> {
        let list = self.font_list_rect();
        if !list.contains(x, y) {
            return None;
        }
        let i = ((y - list.y + self.font_picker.scroll.offset) / ROW).floor() as usize;
        self.font_picker.fonts.get(i).copied()
    }
    pub(super) fn prepare_font_list(&mut self) {
        let height = self.font_list_rect().h;
        self.font_picker
            .scroll
            .set_extent(self.font_picker.fonts.len() as f32 * ROW, height);
    }
    fn reveal_font(&mut self) {
        self.prepare_font_list();
        let start = self.font_picker.highlighted as f32 * ROW;
        self.font_picker.scroll.ensure_visible(start, start + ROW);
    }
    pub(super) fn open_text_fonts(&mut self) {
        self.font_picker = TextFontPicker {
            open: true,
            fonts: UiFont::ALL.to_vec(),
            ..Default::default()
        };
        self.font_picker.highlighted = UiFont::ALL
            .iter()
            .position(|font| font.key() == self.current_text().font)
            .unwrap_or(0);
        self.reveal_font();
        self.pending_properties_focus = false;
        self.space = false;
        self.focus = None;
        self.pending_focus = None;
        self.hover = None;
        self.controls.clear();
    }
    pub(super) fn close_text_fonts(&mut self, out: &mut Vec<Effect>) {
        self.font_picker.open = false;
        self.focus = None;
        self.hover = None;
        self.controls.clear();
        if self.draft.is_some() {
            self.pending_focus = None;
            out.push(Effect::FocusCanvasText);
        } else {
            self.pending_focus = Some(Action::TextFonts);
        }
    }
    fn choose_text_font(&mut self, index: usize, out: &mut Vec<Effect>) {
        if let Some(font) = self.font_picker.fonts.get(index) {
            self.activate(Action::DocumentFont(*font), out);
        }
    }
    fn font_query_changed(&mut self) {
        self.font_picker.filter_fonts();
        self.font_picker.highlighted = 0;
        self.font_picker.scroll.set(0.);
        self.prepare_font_list();
    }
    pub(super) fn font_query_input(&mut self, value: &str) {
        if !matches!(self.drag, Drag::None) || value.is_empty() {
            return;
        }
        if self.font_picker.search_reveal.is_none() {
            self.font_picker.focus_search();
        }
        if self.font_picker.search_focus && self.font_picker.query.insert(value) {
            self.font_query_changed();
        }
    }
    fn font_search_toggle(&self) -> Rect {
        let panel = self.font_menu_rect();
        Rect::new(panel.x + panel.w - 36., panel.y + panel.h - 36., 28., 28.)
    }
    fn font_search_rect(&self) -> Rect {
        let panel = self.font_menu_rect();
        let toggle = self.font_search_toggle();
        let progress = if self.motion_enabled() {
            self.font_picker
                .search_reveal
                .as_ref()
                .map_or(0., Reveal::value)
        } else {
            1.
        };
        let width = toggle.w + (panel.w - 16. - toggle.w) * progress;
        Rect::new(toggle.x + toggle.w - width, toggle.y, width, toggle.h)
    }
    fn font_clear_rect(&self) -> Rect {
        let toggle = self.font_search_toggle();
        Rect::new(toggle.x - 26., toggle.y + 2., 24., 24.)
    }
    fn font_query_rect(&self) -> Rect {
        let r = self.font_search_rect();
        let right = if self.font_picker.query.text().is_empty() {
            self.font_search_toggle().x
        } else {
            self.font_clear_rect().x
        };
        Rect::new(r.x + 6., r.y + 3., (right - r.x - 8.).max(0.), r.h - 6.)
    }
    fn collapse_font_search(&mut self) {
        self.font_picker.search_reveal = None;
        self.font_picker.search_focus = false;
        self.font_picker.query.clear();
        self.font_query_changed();
        self.font_picker.highlighted = self
            .font_picker
            .fonts
            .iter()
            .position(|f| f.key() == self.current_text().font)
            .unwrap_or(0);
        self.reveal_font();
    }
    fn draw_font_search(&mut self, p: &mut Painter) {
        let t = Theme::with_accent(self.dark, self.accent);
        let toggle = self.font_search_toggle();
        let expanded = self.font_picker.search_reveal.is_some();
        if expanded {
            let r = self.font_search_rect();
            p.clip = p.clip.intersection(r);
            self.hit_clip = Some(p.clip);
            p.rounded(r, 4., t.raised);
            self.control(r, Action::FontQuery, "Поиск шрифта", true);
            let input = self.font_query_rect();
            if self.font_picker.query.text().is_empty() && !self.font_picker.search_focus {
                p.text(
                    input,
                    "Поиск шрифта",
                    12.,
                    t.muted,
                    false,
                    false,
                    TextAlign::Left,
                );
            } else {
                self.draw_inline_text(
                    p,
                    input,
                    &self.font_picker.query,
                    self.font_picker.search_focus,
                );
            }
            if !self.font_picker.query.text().is_empty() {
                self.flat_search_button(
                    p,
                    self.font_clear_rect(),
                    Action::ClearFontQuery,
                    "close",
                    "Очистить поиск",
                    false,
                );
            }
        }
        self.flat_search_button(
            p,
            toggle,
            Action::ToggleFontSearch,
            "search",
            if expanded {
                "Закрыть поиск"
            } else {
                "Поиск шрифта"
            },
            expanded,
        );
    }

    pub(super) fn draw_text_fonts(&mut self, p: &mut Painter) {
        self.prepare_font_list();
        let panel = self.font_menu_rect();
        let list = self.font_list_rect();
        let t = Theme::with_accent(self.dark, self.accent);
        self.occlude_underlying_surface(p, panel);
        self.floating_surface(p, panel, "Выбор шрифта");
        p.clip = list;
        self.hit_clip = Some(list);
        if self.font_picker.fonts.is_empty() {
            p.text(
                list,
                "Шрифты не найдены",
                13.,
                t.muted,
                false,
                false,
                TextAlign::Center,
            );
        }
        let ui = p.font;
        for i in 0..self.font_picker.fonts.len() {
            let font = self.font_picker.fonts[i];
            let row = Rect::new(
                list.x,
                list.y + i as f32 * ROW - self.font_picker.scroll.offset,
                list.w,
                ROW,
            );
            if !row.intersects(list) {
                continue;
            }
            let selected = font.key() == self.current_text().font;
            let highlight = !self.font_picker.search_focus && i == self.font_picker.highlighted;
            let hovered = row.contains(self.pointer.0, self.pointer.1)
                && list.contains(self.pointer.0, self.pointer.1);
            if selected || highlight || hovered {
                p.rounded(
                    row.inset(1.),
                    4.,
                    if selected || highlight {
                        t.selected
                    } else {
                        t.raised
                    },
                );
                if highlight {
                    p.rounded(row.inset(1.), 4., t.accent);
                    p.rounded(row.inset(2.), 3., t.selected);
                }
            }
            p.font = font;
            p.text_raw(
                Rect::new(row.x + 10., row.y, row.w - 40., row.h),
                font.label(),
                14.,
                t.text,
                false,
                false,
                TextAlign::Left,
            );
            if selected {
                p.icon(
                    Rect::new(row.x + row.w - 28., row.y, 24., row.h),
                    "check",
                    t.accent,
                );
            }
            self.control(row, Action::DocumentFont(font), font.label(), true);
        }
        p.font = ui;
        p.clip = panel.inset(1.);
        self.hit_clip = Some(p.clip);
        self.draw_font_search(p);
        self.hit_clip = None;
        p.clip = Rect::new(0., 0., self.w(), self.h());
    }
    pub(super) fn text_fonts_mouse(
        &mut self,
        x: f32,
        y: f32,
        button: MouseButton,
        m: Modifiers,
        out: &mut Vec<Effect>,
    ) {
        if button != MouseButton::Left || !matches!(self.drag, Drag::None) {
            return;
        }
        if !self.font_menu_rect().contains(x, y) {
            self.close_text_fonts(out);
            return;
        }
        if self.font_search_toggle().contains(x, y) {
            if self.font_picker.search_reveal.is_some() {
                self.collapse_font_search();
            } else {
                self.font_picker.focus_search();
            }
            return;
        }
        if self.font_picker.search_reveal.is_some() && self.font_search_rect().contains(x, y) {
            self.font_picker.focus_search();
            if self.font_clear_rect().contains(x, y) && !self.font_picker.query.text().is_empty() {
                self.font_picker.query.clear();
                self.font_query_changed();
            } else {
                let caret =
                    self.inline_caret_at(self.font_query_rect(), &self.font_picker.query, x);
                self.font_picker.query.move_to(caret, m.shift);
            }
            return;
        }
        self.prepare_font_list();
        if self.font_list_rect().contains(x, y) {
            let pending = self.font_at(x, y).map(Action::DocumentFont);
            self.begin_list_drag(ScrollList::Fonts, x, y, pending);
        }
    }
    pub(super) fn text_fonts_key(&mut self, code: u32, m: Modifiers, out: &mut Vec<Effect>) {
        if code == 27 {
            if !matches!(self.drag, Drag::None) {
                self.cancel_drag();
            } else if self.font_picker.search_reveal.is_some() {
                self.collapse_font_search();
            } else {
                self.close_text_fonts(out);
            }
            return;
        }
        if !matches!(self.drag, Drag::None) {
            return;
        }
        if m.ctrl && code == 70 {
            self.font_picker.focus_search();
            self.font_picker.query.select_all();
            return;
        }
        if matches!(code, 9 | 117) {
            if self.font_picker.search_focus {
                self.font_picker.search_focus = false;
            } else {
                self.font_picker.focus_search();
            }
            self.reveal_font();
            return;
        }
        if code == 13 || (code == 32 && !self.font_picker.search_focus) {
            self.choose_text_font(self.font_picker.highlighted, out);
            return;
        }
        if matches!(code, 38 | 40 | 33 | 34)
            || (!self.font_picker.search_focus && matches!(code, 35 | 36))
        {
            let last = self.font_picker.fonts.len().saturating_sub(1);
            let i = self.font_picker.highlighted;
            let page = (self.font_list_rect().h / ROW).floor().max(1.) as usize;
            self.font_picker.highlighted = match code {
                36 => 0,
                35 => last,
                33 => i.saturating_sub(page),
                34 => (i + page).min(last),
                38 => i.saturating_sub(1),
                _ => (i + 1).min(last),
            };
            self.font_picker.search_focus = false;
            self.reveal_font();
            return;
        }
        if !self.font_picker.search_focus {
            return;
        }
        let q = &mut self.font_picker.query;
        let changed = match (code, m.ctrl) {
            (65, true) => {
                q.select_all();
                false
            }
            (67 | 88, true) => {
                let v = q.selected_text();
                if !v.is_empty() {
                    out.push(Effect::CopyText(v));
                    code == 88 && q.backspace()
                } else {
                    false
                }
            }
            (86, true) => {
                out.push(Effect::PasteText);
                false
            }
            (8, _) => q.backspace(),
            (46, _) => q.delete(),
            (37, _) => {
                q.move_left(m.shift, m.ctrl);
                false
            }
            (39, _) => {
                q.move_right(m.shift, m.ctrl);
                false
            }
            (36, _) => {
                q.home(m.shift);
                false
            }
            (35, _) => {
                q.end(m.shift);
                false
            }
            _ => false,
        };
        if changed {
            self.font_query_changed();
        }
    }
}
