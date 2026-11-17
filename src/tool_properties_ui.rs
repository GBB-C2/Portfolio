//! Modeless tool properties with fixed section navigation and a scrolling body.
//! Drawing, pointer scrolling and keyboard reveal share the same geometry.
use super::*;

#[derive(Default)]
pub(super) struct ToolPropertiesUi {
    pub active_section: [u8; 4],
    pub scrolls: [[crate::tool_browser::Scroll; 3]; 4],
}

impl PortfolioApp {
    fn tool_settings_family_index(&self) -> usize {
        ToolPanel::for_tool(self.tool).map_or(3, ToolPanel::index)
    }

    fn tool_settings_brushes(&self) -> bool {
        matches!(self.tool, Tool::Brush | Tool::Eraser)
    }

    fn tool_settings_section_count(&self) -> u8 {
        if self.tool_settings_brushes() {
            3
        } else if self.tool == Tool::Fill {
            1
        } else {
            2
        }
    }

    pub(super) fn tool_settings_section(&self) -> u8 {
        self.tool_properties_ui.active_section[self.tool_settings_family_index()]
            .min(self.tool_settings_section_count() - 1)
    }

    pub(super) fn tool_section(&mut self, section: u8) -> bool {
        if section >= self.tool_settings_section_count() {
            return false;
        }
        let family = self.tool_settings_family_index();
        if self.tool_properties_ui.active_section[family] == section {
            return false;
        }
        self.tool_properties_ui.active_section[family] = section;
        self.invalidate_tool_settings_geometry();
        self.prepare_tool_settings_layout();
        true
    }

    fn tool_settings_section_label(&self, section: u8) -> &'static str {
        match section {
            1 if self.tool_settings_brushes() => "Отпечаток",
            1 => "Заливка",
            2 => "Образец",
            _ => "Основные",
        }
    }

    fn tool_settings_content_height(&self) -> f32 {
        if self.tool_settings_brushes() {
            match self.tool_settings_section() {
                0 => 244.,
                1 => 190.,
                _ => 146.,
            }
        } else if self.tool == Tool::Fill {
            84.
        } else if self.tool_settings_section() == 0 {
            136.
        } else {
            76.
        }
    }

    pub(super) fn tool_settings_rect(&self) -> Rect {
        let left = 16.;
        let width = (self.w() - 336.)
            .clamp(360., 448.)
            .min((self.w() - 32.).max(0.));
        let preferred_height: f32 = if self.tool_settings_brushes() {
            384.
        } else if self.tool == Tool::Fill {
            192.
        } else {
            256.
        };
        let bottom = (self.h() - 16.).max(0.);
        let first = 80.0_f32.min(bottom);
        let minimum = 180.0_f32.min(bottom - first);
        let mut obstacles = [(0.0_f32, 0.0_f32); 3];
        let mut count = 0;
        for rect in [
            (!self.focused_mode).then(|| self.top_tools_rect()),
            self.tool_popup_rect(),
            self.quick_controls_visible()
                .then(|| self.quick_controls_rect()),
        ]
        .into_iter()
        .flatten()
        {
            if rect.w > 0. && rect.h > 0. && rect.x < left + width && rect.x + rect.w > left {
                obstacles[count] = (rect.y, rect.y + rect.h);
                count += 1;
            }
        }
        obstacles[..count].sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut top = 136.0_f32.max(first);
        for &(start, end) in &obstacles[..count] {
            if top < end && top + preferred_height > start {
                top = end + 12.;
            }
        }
        if bottom - top >= minimum {
            return Rect::new(left, top, width, preferred_height.min(bottom - top));
        }

        // A user may place the movable rows below the preferred panel position.
        // Look above them as well; keep navigation usable if no full gap remains.
        let mut largest = (first, first);
        let mut cursor = first;
        for &(start, end) in &obstacles[..count] {
            let gap_end = (start - 12.).clamp(first, bottom);
            if gap_end - cursor > largest.1 - largest.0 {
                largest = (cursor, gap_end);
            }
            cursor = cursor.max((end + 12.).clamp(first, bottom));
        }
        if bottom - cursor > largest.1 - largest.0 {
            largest = (cursor, bottom);
        }
        let height = preferred_height.min((largest.1 - largest.0).max(minimum));
        top = if largest.1 - largest.0 >= height {
            136.0_f32.clamp(largest.0, largest.1 - height)
        } else {
            largest.0.clamp(first, (bottom - height).max(first))
        };
        Rect::new(left, top, width, height)
    }

    pub(super) fn tool_settings_viewport(&self) -> Rect {
        let panel = self.tool_settings_rect();
        let (header, footer) = if self.tool_settings_brushes() {
            (68., 64.)
        } else {
            (60., 28.)
        };
        Rect::new(
            panel.x + 128.,
            panel.y + header,
            (panel.w - 140.).max(0.),
            (panel.h - header - footer).max(0.),
        )
    }

    pub(super) fn tool_settings_scrollbar_track(&self) -> Rect {
        let panel = self.tool_settings_rect();
        let body = self.tool_settings_viewport();
        Rect::new(
            panel.x + panel.w - 18.,
            body.y + 4.,
            10.,
            (body.h - 8.).max(0.),
        )
    }

    pub(super) fn tool_settings_scrollbar_thumb(&self) -> Rect {
        let track = self.tool_settings_scrollbar_track();
        let scroll = self.tool_settings_scroll();
        let height = if scroll.extent > 0. {
            (track.h * scroll.viewport / scroll.extent)
                .max(28.)
                .min(track.h)
        } else {
            track.h
        };
        let fraction = if scroll.max() > 0. {
            scroll.offset / scroll.max()
        } else {
            0.
        };
        Rect::new(
            track.x,
            track.y + (track.h - height) * fraction,
            track.w,
            height,
        )
    }

    pub(super) fn tool_settings_scroll(&self) -> crate::tool_browser::Scroll {
        self.tool_properties_ui.scrolls[self.tool_settings_family_index()]
            [self.tool_settings_section() as usize]
    }

    pub(super) fn prepare_tool_settings_layout(&mut self) {
        let content = self.tool_settings_content_height();
        let viewport = self.tool_settings_viewport().h;
        let index = self.tool_settings_family_index();
        let section = self.tool_settings_section() as usize;
        self.tool_properties_ui.scrolls[index][section].set_extent(content, viewport);
    }

    fn invalidate_tool_settings_geometry(&mut self) {
        self.controls.clear();
        self.focus = None;
        self.pending_focus = None;
        self.hover = None;
    }

    pub(super) fn tool_settings_scroll_by(&mut self, delta: f32) -> bool {
        self.pending_focus = None;
        self.prepare_tool_settings_layout();
        let index = self.tool_settings_family_index();
        let section = self.tool_settings_section() as usize;
        let changed = self.tool_properties_ui.scrolls[index][section].scroll_by(delta);
        if changed {
            self.invalidate_tool_settings_geometry();
        }
        changed
    }

    pub(super) fn tool_settings_scroll_to(&mut self, offset: f32) -> bool {
        self.pending_focus = None;
        self.prepare_tool_settings_layout();
        let index = self.tool_settings_family_index();
        let section = self.tool_settings_section() as usize;
        let changed = self.tool_properties_ui.scrolls[index][section].set(offset);
        if changed {
            self.invalidate_tool_settings_geometry();
        }
        changed
    }

    fn tool_settings_action_section(&self, action: Action) -> Option<u8> {
        if self.tool_settings_brushes() {
            match action {
                Action::Size | Action::Opacity | Action::Hardness | Action::Flow => Some(0),
                Action::Spacing | Action::Angle | Action::Scatter => Some(1),
                _ => None,
            }
        } else if self.tool == Tool::Fill {
            (action == Action::Opacity).then_some(0)
        } else {
            match action {
                Action::Size | Action::Opacity => Some(0),
                Action::ShapeFill => Some(1),
                _ => None,
            }
        }
    }

    fn tool_settings_action_span(&self, action: Action) -> Option<(f32, f32)> {
        if self.tool_settings_action_section(action)? != self.tool_settings_section() {
            return None;
        }
        let start = if self.tool_settings_brushes() {
            match action {
                Action::Size => 28.,
                Action::Opacity => 82.,
                Action::Hardness => 136.,
                Action::Flow => 190.,
                Action::Spacing => 28.,
                Action::Angle => 82.,
                Action::Scatter => 136.,
                _ => return None,
            }
        } else if self.tool == Tool::Fill {
            match action {
                Action::Opacity => 28.,
                _ => return None,
            }
        } else {
            match action {
                Action::Size => 28.,
                Action::Opacity => 82.,
                Action::ShapeFill => return Some((28., 68.)),
                _ => return None,
            }
        };
        Some((start, start + 46.))
    }

    pub(super) fn tool_settings_reveal_action(&mut self, action: Action) -> bool {
        if let Action::BrushDetail(detail) = action {
            if !self.tool_settings_brushes() {
                return false;
            }
            self.tool_section(u8::from(detail));
            return true;
        }
        match action {
            Action::CloseToolProperties => return true,
            Action::ResetBrush => return self.tool_settings_brushes(),
            Action::ToolSection(section) => return section < self.tool_settings_section_count(),
            Action::ToolPropertiesScroll => {
                self.prepare_tool_settings_layout();
                return self.tool_settings_scroll().max() > 0.;
            }
            Action::ShapeFill if !matches!(self.tool, Tool::Rectangle | Tool::Ellipse) => {
                return false
            }
            _ => {}
        }
        let Some(section) = self.tool_settings_action_section(action) else {
            return false;
        };
        self.tool_section(section);
        self.prepare_tool_settings_layout();
        let Some((start, end)) = self.tool_settings_action_span(action) else {
            return false;
        };
        let index = self.tool_settings_family_index();
        let changed =
            self.tool_properties_ui.scrolls[index][section as usize].ensure_visible(start, end);
        if changed {
            self.invalidate_tool_settings_geometry();
        }
        true
    }

    pub(super) fn tool_settings_focus_order(&self) -> Vec<Action> {
        let mut actions = vec![Action::CloseToolProperties];
        actions.extend((0..self.tool_settings_section_count()).map(Action::ToolSection));
        if self.tool_settings_brushes() {
            match self.tool_settings_section() {
                0 => actions.extend([
                    Action::Size,
                    Action::Opacity,
                    Action::Hardness,
                    Action::Flow,
                ]),
                1 => actions.extend([Action::Spacing, Action::Angle, Action::Scatter]),
                _ => {}
            }
        } else if self.tool == Tool::Fill {
            actions.push(Action::Opacity);
        } else {
            if self.tool_settings_section() == 0 {
                actions.extend([Action::Size, Action::Opacity]);
            } else if matches!(self.tool, Tool::Rectangle | Tool::Ellipse) {
                actions.push(Action::ShapeFill);
            }
        }
        if self.tool_settings_content_height() > self.tool_settings_viewport().h {
            actions.push(Action::ToolPropertiesScroll);
        }
        if self.tool_settings_brushes() {
            actions.push(Action::ResetBrush);
        }
        if self.busy {
            actions.retain(|action| {
                matches!(
                    action,
                    Action::CloseToolProperties
                        | Action::ToolPropertiesScroll
                        | Action::ToolSection(_)
                )
            });
        }
        actions
    }

    pub(super) fn tool_settings_navigation_key(
        &mut self,
        code: u32,
        modifiers: Modifiers,
        out: &mut Vec<Effect>,
    ) -> bool {
        if modifiers.ctrl
            || modifiers.alt
            || !matches!(code, 35..=40)
            || !self.tool_properties_has_focus()
        {
            return false;
        }
        let action = self
            .focus
            .and_then(|index| self.controls.get(index))
            .filter(|control| control.enabled)
            .map(|control| control.action)
            .or(self.pending_focus);
        let Some(Action::ToolSection(section)) = action else {
            return false;
        };
        let last = self.tool_settings_section_count() - 1;
        if section > last {
            return false;
        }
        let next = match code {
            35 => last,
            36 => 0,
            37 | 38 => section.saturating_sub(1),
            39 | 40 => (section + 1).min(last),
            _ => unreachable!(),
        };
        // Activation preserves the navigation focus while changing the body;
        // the pending action also handles key repeats before the next frame.
        self.activate(Action::ToolSection(next), out);
        self.pending_properties_focus = true;
        self.pending_focus = Some(Action::ToolSection(next));
        true
    }

    pub(super) fn draw_tool_settings(&mut self, p: &mut Painter) {
        self.prepare_tool_settings_layout();
        let t = Theme::with_accent(self.dark, self.accent);
        let panel = self.tool_settings_rect();
        for control in &mut self.controls {
            if control.hit.intersects(panel) {
                control.enabled = false;
            }
        }
        let visible = self
            .motion_hit_clip
            .map_or(panel, |clip| panel.intersection(clip));
        p.frame
            .texts
            .retain(|text| !visible.intersects(Rect::new(text.x, text.y, text.width, text.height)));
        self.floating_surface(p, panel, "Параметры инструмента");
        p.clip = panel.inset(1.);
        self.hit_clip = Some(p.clip);
        p.text(
            Rect::new(panel.x + 20., panel.y + 12., panel.w - 84., 32.),
            self.tool_properties_title(),
            21.,
            t.text,
            true,
            false,
            TextAlign::Left,
        );
        self.icon_button(
            p,
            Rect::new(panel.x + panel.w - 44., panel.y + 16., 28., 28.),
            Action::CloseToolProperties,
            "close",
            "Закрыть параметры · F4 / Esc",
            false,
            true,
        );
        let brushes = self.tool_settings_brushes();
        if brushes {
            p.label(
                panel.x + 20.,
                panel.y + 44.,
                panel.w - 40.,
                if self.tool == Tool::Eraser {
                    ItemId::Eraser(self.brush.kind).label()
                } else {
                    ItemId::Brush(self.brush.kind).label()
                },
                11.,
                t.muted,
            );
        }

        let body = self.tool_settings_viewport();
        let section = self.tool_settings_section();
        for index in 0..self.tool_settings_section_count() {
            let row = Rect::new(panel.x + 12., body.y + index as f32 * 34., 104., 32.);
            let focused = self.focus == Some(self.controls.len());
            let hovered = self.hover == Some(self.controls.len());
            let selected = section == index;
            let background = if selected {
                t.selected
            } else if hovered {
                t.raised
            } else {
                t.surface
            };
            p.rounded(row, 4., background);
            if focused {
                p.rounded(row, 4., t.accent);
                p.rounded(row.inset(1.), 3., background);
            }
            let label = self.tool_settings_section_label(index);
            p.text(
                Rect::new(row.x + 10., row.y, row.w - 20., row.h),
                label,
                11.,
                if selected { t.accent } else { t.text },
                selected,
                false,
                TextAlign::Left,
            );
            self.control(row, Action::ToolSection(index), label, true);
        }

        let origin = body.y - self.tool_settings_scroll().offset;
        p.clip = body;
        self.hit_clip = Some(body);
        self.draw_tool_settings_heading(p, origin, self.tool_settings_section_label(section));
        let actions: &[Action] = if brushes {
            match section {
                0 => &[
                    Action::Size,
                    Action::Opacity,
                    Action::Hardness,
                    Action::Flow,
                ],
                1 => &[Action::Spacing, Action::Angle, Action::Scatter],
                _ => &[],
            }
        } else if self.tool == Tool::Fill {
            &[Action::Opacity]
        } else if section == 0 {
            &[Action::Size, Action::Opacity]
        } else {
            &[]
        };
        let row_width = (body.w
            - if self.tool_settings_scroll().max() > 0. {
                14.
            } else {
                0.
            })
        .max(0.);
        for &action in actions {
            let (start, _) = self.tool_settings_action_span(action).unwrap();
            let row = Rect::new(body.x, origin + start, row_width, 46.);
            if row.intersects(body) {
                self.parameter_slider(p, row, action);
            }
        }
        if brushes && section == 2 {
            let sample_rect = Rect::new(body.x, origin + 28., row_width, row_width * 40. / 112.);
            if sample_rect.intersects(body) {
                self.draw_brush_sample(p, sample_rect, t.text);
            }
        } else if !brushes && self.tool != Tool::Fill && section == 1 {
            self.draw_shape_fill_switch(p, Rect::new(body.x, origin + 28., row_width, 40.));
        }

        p.clip = panel.inset(1.);
        self.hit_clip = Some(p.clip);
        if self.tool_settings_scroll().max() > 0. {
            let track = self.tool_settings_scrollbar_track();
            let thumb = self.tool_settings_scrollbar_thumb();
            let focused = self.focus == Some(self.controls.len());
            let hovered = self.hover == Some(self.controls.len());
            p.rounded(Rect::new(track.x + 3., track.y, 4., track.h), 2., t.raised);
            p.rounded(
                Rect::new(thumb.x + 2., thumb.y, 6., thumb.h),
                3.,
                if hovered || focused {
                    t.accent
                } else {
                    t.muted
                },
            );
            self.control(
                track,
                Action::ToolPropertiesScroll,
                "Прокрутить параметры",
                true,
            );
        }
        if brushes {
            self.button(
                p,
                Rect::new(body.x, panel.y + panel.h - 60., body.w, 32.),
                Action::ResetBrush,
                if self.tool == Tool::Eraser {
                    "Сбросить ластик"
                } else {
                    "Сбросить кисть"
                },
                None,
                false,
                !self.busy,
                false,
            );
        }
        p.text(
            Rect::new(body.x, panel.y + panel.h - 24., body.w, 16.),
            "Число — точный ввод",
            10.,
            t.muted,
            false,
            false,
            TextAlign::Left,
        );
        self.hit_clip = None;
        p.clip = Rect::new(0., 0., self.w(), self.h());
    }

    fn draw_tool_settings_heading(&self, p: &mut Painter, y: f32, label: &str) {
        let body = self.tool_settings_viewport();
        let rect = Rect::new(body.x, y, body.w, 24.);
        if rect.intersects(self.tool_settings_viewport()) {
            p.text(
                rect,
                label,
                12.,
                Theme::with_accent(self.dark, self.accent).text,
                true,
                false,
                TextAlign::Left,
            );
        }
    }

    fn draw_shape_fill_switch(&mut self, p: &mut Painter, row: Rect) {
        if !row.intersects(self.tool_settings_viewport()) {
            return;
        }
        let t = Theme::with_accent(self.dark, self.accent);
        let enabled = matches!(self.tool, Tool::Rectangle | Tool::Ellipse) && !self.busy;
        let focused = self.focus == Some(self.controls.len());
        let hovered = self.hover == Some(self.controls.len()) && enabled;
        let background = if hovered { t.raised } else { t.surface };
        if focused {
            p.rounded(row, 4., t.accent);
            p.rounded(row.inset(1.), 3., background);
        } else if hovered {
            p.rounded(row, 4., background);
        }
        let track = Rect::new(row.x + row.w - 44., row.y + 8., 44., 24.);
        let track_color = if enabled && self.shape_filled {
            t.accent
        } else {
            t.raised
        };
        p.rounded(track, 12., track_color);
        let thumb_x = track.x + if self.shape_filled { 32. } else { 12. };
        let thumb_y = track.y + 12.;
        p.soft_shadow(
            Rect::new(thumb_x - 8., thumb_y - 8., 16., 16.),
            8.,
            if enabled { 0.12 } else { 0.04 },
        );
        p.circle(
            thumb_x,
            thumb_y,
            8.,
            if enabled { 0xf5f5f5 } else { t.muted },
            true,
        );
        p.text(
            Rect::new(row.x, row.y, row.w - 58., row.h),
            "Заливка фигуры",
            12.,
            if enabled { t.text } else { t.muted },
            false,
            false,
            TextAlign::Left,
        );
        self.control(row, Action::ShapeFill, "Заливка фигуры", enabled);
    }
}
