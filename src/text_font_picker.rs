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
    highlighted: usize,
}

impl TextFontPicker {
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
        let content = (52. + self.font_picker.fonts.len().max(1) as f32 * ROW).min(238.);
        let h = (if down { space_below } else { space_above }).clamp(1., content);
        Rect::new(
            anchor.x.clamp(8., (self.w() - width - 8.).max(8.)),
            if down { below } else { anchor.y - h - 8. },
            width,
            h,
        )
    }
    fn font_list_rect(&self) -> Rect {
        let panel = self.font_menu_rect();
        Rect::new(
            panel.x + 6.,
            panel.y + 46.,
            panel.w - 24.,
            (panel.h - 52.).max(0.),
        )
    }
    pub(super) fn font_scroll_track(&self) -> Rect {
        let list = self.font_list_rect();
        Rect::new(list.x + list.w + 2., list.y, 10., list.h)
    }
    pub(super) fn font_scroll_thumb(&self) -> Rect {
        let track = self.font_scroll_track();
        let scroll = self.font_picker.scroll;
        let h =
            (track.h * scroll.viewport / scroll.extent.max(1.)).clamp(20_f32.min(track.h), track.h);
        let dy = if scroll.max() > 0. {
            scroll.offset / scroll.max() * (track.h - h)
        } else {
            0.
        };
        Rect::new(track.x, track.y + dy, track.w, h)
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
            search_focus: true,
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
        if self.font_picker.search_focus && self.font_picker.query.insert(value) {
            self.font_query_changed();
        }
    }
    fn font_search_rect(&self) -> Rect {
        let panel = self.font_menu_rect();
        Rect::new(panel.x + 8., panel.y + 8., panel.w - 16., 30.)
    }
    fn font_query_rect(&self) -> Rect {
        let r = self.font_search_rect();
        Rect::new(r.x + 28., r.y + 4., r.w - 55., r.h - 8.)
    }

    pub(super) fn draw_text_fonts(&mut self, p: &mut Painter) {
        self.prepare_font_list();
        let panel = self.font_menu_rect();
        let list = self.font_list_rect();
        let t = Theme::with_accent(self.dark, self.accent);
        self.occlude_underlying_surface(p, panel);
        self.floating_surface(p, panel, "Выбор шрифта");
        p.clip = panel.inset(1.);
        self.hit_clip = Some(p.clip);
        let search = self.font_search_rect();
        p.rounded(
            search,
            4.,
            if self.font_picker.search_focus {
                t.accent
            } else {
                t.border
            },
        );
        p.rounded(search.inset(1.), 3., t.raised);
        p.icon(
            Rect::new(search.x + 3., search.y, 23., search.h),
            "search",
            t.muted,
        );
        self.control(search, Action::FontQuery, "Поиск шрифта", true);
        let input = self.font_query_rect();
        if self.font_picker.query.text().is_empty() {
            p.text(
                Rect::new(input.x + 3., input.y, input.w - 3., input.h),
                "Поиск шрифта",
                12.,
                t.muted,
                false,
                false,
                TextAlign::Left,
            );
        }
        self.draw_inline_text(
            p,
            input,
            &self.font_picker.query,
            self.font_picker.search_focus,
        );
        if !self.font_picker.query.text().is_empty() {
            self.icon_button(
                p,
                Rect::new(search.x + search.w - 26., search.y + 3., 24., 24.),
                Action::ClearFontQuery,
                "close",
                "Очистить поиск",
                false,
                true,
            );
        }

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
        let track = self.font_scroll_track();
        if self.font_picker.scroll.max() > 0. {
            p.rounded(Rect::new(track.x + 3., track.y, 4., track.h), 2., t.raised);
            let thumb = self.font_scroll_thumb();
            p.rounded(Rect::new(thumb.x + 2., thumb.y, 6., thumb.h), 3., t.muted);
            self.control(track, Action::FontListScroll, "Прокрутка шрифтов", true);
        }
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
        if self.font_search_rect().contains(x, y) {
            self.font_picker.search_focus = true;
            let search = self.font_search_rect();
            if x > search.x + search.w - 26. && !self.font_picker.query.text().is_empty() {
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
        if self.font_scroll_track().contains(x, y) && self.font_picker.scroll.max() > 0. {
            let thumb = self.font_scroll_thumb();
            let grab = if thumb.contains(x, y) {
                y - thumb.y
            } else {
                thumb.h / 2.
            };
            self.drag = Drag::VerticalScroll {
                surface: ScrollSurface::DocumentFonts,
                initial: self.font_picker.scroll.offset,
                grab,
            };
            self.update_drag(x, y, m);
            return;
        }
        let list = self.font_list_rect();
        if list.contains(x, y) {
            let index = ((y - list.y + self.font_picker.scroll.offset) / ROW).floor() as usize;
            self.choose_text_font(index, out);
        }
    }
    pub(super) fn text_fonts_key(&mut self, code: u32, m: Modifiers, out: &mut Vec<Effect>) {
        if code == 27 {
            if !matches!(self.drag, Drag::None) {
                self.cancel_drag();
            } else {
                self.close_text_fonts(out);
            }
            return;
        }
        if !matches!(self.drag, Drag::None) {
            return;
        }
        if m.ctrl && code == 70 {
            self.font_picker.search_focus = true;
            self.font_picker.query.select_all();
            return;
        }
        if matches!(code, 9 | 117) {
            self.font_picker.search_focus = !self.font_picker.search_focus;
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
