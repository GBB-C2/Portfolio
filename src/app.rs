//! Application commands, layout and interaction state. No operating-system handles.
use crate::{
    appearance::{Accent, Appearance},
    demo,
    document::{self, Brush, BrushKind, Color, Document, ParagraphAlign, ShapeKind, TextLayer},
    i18n::Language,
    motion::Reveal,
    platform::{
        self, Answer, Effect, Event, FileKind, Frame, Modifiers, MouseButton, NativeApp, TextAlign,
        UiFont,
    },
    render::{hex, hsv, parse_hex, rgb, to_hsv, Painter, Rect, Theme},
    tool_browser::{Browser, Category, Family as ToolPanel, ItemId},
};
use std::{
    collections::VecDeque,
    fs,
    path::PathBuf,
    sync::mpsc::{self, Receiver},
    time::{Duration, Instant},
};

#[cfg(test)]
#[path = "app_tests.rs"]
mod tests;

#[path = "brush_presets.rs"]
mod brush_presets;
use brush_presets::Presets;

#[path = "settings_ui.rs"]
mod settings_ui;
#[path = "tool_properties_ui.rs"]
mod tool_properties_ui;
use settings_ui::SettingsUi;
use tool_properties_ui::ToolPropertiesUi;

pub const TOOLBAR_WIDTH: f32 = 756.;
pub const TOOLBAR_GRIP_OFFSET: f32 = 472.;

const PALETTE: [u32; 8] = [
    0x242b25, 0xf1e9d2, 0xd84d38, 0x5d6a43, 0xc8aa69, 0x365ad7, 0xd3e98a, 0xc297a0,
];
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tool {
    Brush,
    Eraser,
    Fill,
    Picker,
    Line,
    Rectangle,
    Ellipse,
    Hand,
    Text,
}
impl Tool {
    fn info(self) -> (&'static str, &'static str, &'static str) {
        match self {
            Self::Brush => ("brush", "Кисть", "B"),
            Self::Eraser => ("eraser", "Ластик", "E"),
            Self::Fill => ("fill", "Заливка", "G"),
            Self::Picker => ("picker", "Пипетка", "I"),
            Self::Line => ("line", "Линия", "L"),
            Self::Rectangle => ("rect", "Прямоугольник", "R"),
            Self::Ellipse => ("ellipse", "Эллипс", "O"),
            Self::Text => ("text", "Текст", "T"),
            Self::Hand => ("hand", "Перемещение", "H"),
        }
    }
}

impl ToolPanel {
    fn for_tool(tool: Tool) -> Option<Self> {
        match tool {
            Tool::Brush => Some(Self::Brush),
            Tool::Eraser => Some(Self::Eraser),
            Tool::Line | Tool::Rectangle | Tool::Ellipse => Some(Self::Shapes),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct ToolSettings {
    kind: BrushKind,
    size: f32,
    opacity: f32,
    hardness: f32,
    flow: f32,
    spacing: f32,
    angle: f32,
    scatter: f32,
    roundness: f32,
    grain: f32,
}
impl ToolSettings {
    fn from_brush(b: Brush) -> Self {
        Self {
            kind: b.kind,
            size: b.size,
            opacity: b.opacity,
            hardness: b.hardness,
            flow: b.flow,
            spacing: b.spacing,
            angle: b.angle,
            scatter: b.scatter,
            roundness: b.roundness,
            grain: b.grain,
        }
    }
    fn apply(self, b: &mut Brush) {
        b.kind = self.kind;
        b.size = self.size;
        b.opacity = self.opacity;
        b.hardness = self.hardness;
        b.flow = self.flow;
        b.spacing = self.spacing;
        b.angle = self.angle;
        b.scatter = self.scatter;
        b.roundness = self.roundness;
        b.grain = self.grain;
    }
    fn preset(kind: BrushKind, erase: bool) -> Self {
        let (size, hardness, flow, spacing, angle, scatter) = match kind {
            BrushKind::Ink => (if erase { 40. } else { 24. }, 1., 1., 0.12, 0., 0.),
            BrushKind::Soft => (80., 0.1, 0.65, 0.1, 0., 0.),
            BrushKind::Marker => (40., 0.9, 0.8, 0.1, 12., 0.),
            BrushKind::Pencil => (5., 0.9, 0.8, 0.12, 0., 0.),
            BrushKind::Charcoal => (36., 0.5, 0.8, 0.18, 10., 0.05),
            BrushKind::Chalk => (28., 0.8, 0.9, 0.18, 0., 0.),
            BrushKind::Airbrush => (120., 0.05, 0.45, 0.12, 0., 0.),
            BrushKind::Calligraphy => (36., 1., 1., 0.08, 40., 0.),
            BrushKind::Spray => (72., 0.7, 0.8, 0.2, 0., 0.1),
            BrushKind::Flat => (48., 0.9, 0.9, 0.12, 20., 0.),
        };
        Self {
            kind,
            size,
            opacity: 1.,
            hardness,
            flow,
            spacing,
            angle,
            scatter,
            roundness: 1.,
            grain: 0.,
        }
    }
}
impl PortfolioApp {
    fn draw_editor(&mut self, p: &mut Painter) {
        // Modal surfaces own a separate control list. Their focus indices must
        // never highlight unrelated controls in the editor underneath.
        let overlay_input = if !self.editor_interactive() {
            let input = (self.focus, self.hover, self.pending_focus);
            self.focus = None;
            self.hover = None;
            self.pending_focus = None;
            Some(input)
        } else {
            None
        };
        let t = Theme::with_accent(self.dark, self.accent);
        let (w, h) = (self.w(), self.h());
        let stage = self.stage();
        let c = self.canvas();
        let raster_key = CanvasRasterKey {
            size: (self.width, self.height),
            viewport: [
                c.x.to_bits(),
                c.y.to_bits(),
                c.w.to_bits(),
                c.h.to_bits(),
                self.scale.to_bits(),
            ],
            document_size: (self.doc.width, self.doc.height),
            dark: self.dark,
            grayscale: self.grayscale,
        };
        let damage = self.doc.take_paint_damage();
        let raster_changed = self.composite_dirty
            || damage.is_some()
            || self.composite.len() != (self.doc.width * self.doc.height) as usize;
        if self.composite_dirty
            || self.composite.len() != (self.doc.width * self.doc.height) as usize
        {
            self.composite.resize(
                (self.doc.width * self.doc.height) as usize,
                Color::TRANSPARENT,
            );
            self.doc.composite_into(&mut self.composite);
            self.composite_dirty = false;
        } else if let Some(bounds) = damage {
            if !self.doc.composite_region_into(&mut self.composite, bounds) {
                self.doc.composite_into(&mut self.composite);
            }
        }
        if !raster_changed
            && self
                .canvas_raster
                .as_ref()
                .is_some_and(|cache| cache.key == raster_key)
        {
            p.frame
                .pixels
                .copy_from_slice(&self.canvas_raster.as_ref().unwrap().pixels);
        } else {
            p.rect(Rect::new(0., 64., w, h - 64.), t.workspace);
            // Quiet dotted work surface, separate from document pixels.
            let dot = if self.dark { 0x252525 } else { 0xbfbfbf };
            for y in (88..h as i32).step_by(24) {
                for x in (stage.x as i32 + 16..(stage.x + stage.w) as i32).step_by(24) {
                    p.rect(Rect::new(x as f32, y as f32, 1., 1.), dot);
                }
            }
            p.clip = stage;
            p.rounded(
                Rect::new(c.x - 7., c.y + 9., c.w + 14., c.h + 6.),
                4.,
                if self.dark { 0x090909 } else { 0xb9b9b9 },
            );
            for yy in ((c.y.max(stage.y) - c.y) / 12.).floor() as i32
                ..((c.y + c.h).min(stage.y + stage.h) - c.y).ceil() as i32 / 12 + 1
            {
                for xx in ((c.x.max(stage.x) - c.x) / 12.).floor() as i32
                    ..((c.x + c.w).min(stage.x + stage.w) - c.x).ceil() as i32 / 12 + 1
                {
                    let r = Rect::new(
                        (c.x + xx as f32 * 12.).max(c.x),
                        (c.y + yy as f32 * 12.).max(c.y),
                        12_f32.min(c.w - xx as f32 * 12.),
                        12_f32.min(c.h - yy as f32 * 12.),
                    );
                    p.rect(
                        r,
                        if (xx + yy) % 2 == 0 {
                            0xe0e0e0
                        } else {
                            0xf4f4f4
                        },
                    );
                }
            }
            p.image(
                c,
                self.doc.width,
                self.doc.height,
                &self.composite,
                self.grayscale,
                1.,
            );
            let cache = self.canvas_raster.get_or_insert_with(|| CanvasRaster {
                key: raster_key,
                pixels: Vec::new(),
            });
            cache.key = raster_key;
            cache.pixels.clone_from(&p.frame.pixels);
        }
        p.clip = stage;
        if self.grid {
            let step = (100. * self.zoom).max(12.);
            let color = if self.dark { 0x686868 } else { 0x858585 };
            let mut x = c.x;
            while x < c.x + c.w {
                p.line(x, c.y, x, c.y + c.h, 0.5, color);
                x += step;
            }
            let mut y = c.y;
            while y < c.y + c.h {
                p.line(c.x, y, c.x + c.w, y, 0.5, color);
                y += step;
            }
        }
        if self.brush.mirror {
            let x = c.x + c.w / 2.;
            let mut y = c.y;
            while y < c.y + c.h {
                p.line(x, y, x, (y + 5.).min(c.y + c.h), 1., 0xaaaaaa);
                y += 10.;
            }
        }
        if stage.contains(self.pointer.0, self.pointer.1)
            && c.contains(self.pointer.0, self.pointer.1)
            && self.editor_interactive()
            && (self.tool_popup.is_none() || self.tool_panel_pinned || self.tool_settings_visible())
            && !self.canvas_obscured(self.pointer.0, self.pointer.1)
            && !self.space
            && matches!(self.tool, Tool::Brush | Tool::Eraser)
        {
            let r = (self.brush.size * self.zoom / 2.).max(2.);
            p.circle(self.pointer.0, self.pointer.1, r + 1., 0x111111, false);
            p.circle(self.pointer.0, self.pointer.1, r, 0xf5f5f5, false);
        }
        p.clip = Rect::new(0., 0., w, h);
        // Application header.
        p.rect(Rect::new(0., 0., w, 64.), t.base);
        let brand = self.brand_rect();
        if self.overlay == Overlay::File || brand.contains(self.pointer.0, self.pointer.1) {
            p.rounded(brand, 4., t.raised);
        }
        self.control(brand, Action::File, "Portfolio · меню проекта", true);
        p.icon(Rect::new(20., 19., 26., 26.), "star", t.accent);
        p.text(
            Rect::new(54., 10., 180., 42.),
            "Portfolio",
            23.,
            t.text,
            true,
            false,
            TextAlign::Left,
        );
        let titlew = (w - 570.).max(125.);
        p.circle(
            286.,
            32.,
            3.,
            if self.dirty() { t.accent } else { t.muted },
            true,
        );
        p.label_raw(
            299.,
            21.,
            titlew,
            format!(
                "{}{}",
                self.doc.title,
                if self.dirty() { "  ·" } else { "" }
            ),
            13.,
            t.text,
        );
        let bx = w - 347.;
        self.button(
            p,
            Rect::new(bx + 94., 16., 30., 32.),
            Action::Help,
            "?",
            None,
            false,
            true,
            false,
        );
        self.icon_button(
            p,
            Rect::new(bx + 132., 16., 32., 32.),
            Action::Theme,
            "sun",
            if self.dark {
                "Тема «Бумага»"
            } else {
                "Тема «Графит»"
            },
            false,
            true,
        );
        self.icon_button(
            p,
            Rect::new(w - 179., 18., 28., 28.),
            Action::Settings,
            "settings",
            "Настройки внешнего вида · Ctrl+,",
            self.overlay == Overlay::Settings,
            true,
        );
        self.button(
            p,
            Rect::new(w - 146., 16., 126., 32.),
            Action::Export,
            "Экспорт",
            Some("export"),
            false,
            !self.busy,
            true,
        );
        if !self.focused_mode || self.chrome_motion.active() {
            let hidden = self.focused_mode;
            self.focused_mode = false;
            self.begin_reveal(p, self.right_panel_rect(), 0);
            self.draw_panel(p);
            self.end_reveal(p);
            self.begin_reveal(p, self.top_tools_rect(), 0);
            self.draw_top_tools(p);
            self.end_reveal(p);
            if self.quick_controls_visible() {
                self.begin_reveal(p, self.quick_controls_rect(), 0);
                self.draw_quick_controls(p);
                self.end_reveal(p);
            }
            self.focused_mode = hidden;
            if hidden {
                self.controls.retain(|c| c.rect.y < 64.);
            }
        }
        self.draw_popup_layer(p);
        let label = if self.busy {
            self.status.clone()
        } else if let Some(i) = self.hover {
            self.controls
                .get(i)
                .map(|c| c.label.clone())
                .unwrap_or_default()
        } else if Instant::now() < self.status_until {
            self.status.clone()
        } else {
            format!(
                "{} × {} px  ·  {} сл.  ·  {}",
                self.doc.width,
                self.doc.height,
                self.doc.layers.len(),
                self.tool.info().1
            )
        };
        p.label(299., 39., titlew, label, 10., t.muted);
        if self.overlay != Overlay::None {
            if let Some((focus, hover, pending_focus)) = overlay_input {
                self.focus = focus;
                self.hover = hover;
                self.pending_focus = pending_focus;
            }
            self.draw_animated_overlay(p);
        }
        // Transient size/opacity feedback belongs above modeless properties,
        // matching its pointer hit priority. Modal dialogs suppress it entirely.
        if let Some(action) = self.quick_preview_action() {
            self.draw_quick_preview(p, action);
        }
    }
    fn draw_top_tools(&mut self, p: &mut Painter) {
        let t = Theme::with_accent(self.dark, self.accent);
        let bar = self.top_tools_rect();
        self.occlude_underlying_surface(p, bar);
        self.floating_surface(p, bar, "Инструменты и масштаб");
        p.clip = bar.inset(1.);
        self.hit_clip = Some(p.clip);
        for (family, icon, tip) in [
            (ToolPanel::Brush, "brush", "Кисть · B · нажмите для выбора"),
            (
                ToolPanel::Eraser,
                "eraser",
                "Ластик · E · нажмите для выбора",
            ),
            (
                ToolPanel::Shapes,
                self.last_shape.info().0,
                "Фигуры · L / R / O · нажмите для выбора",
            ),
        ] {
            let r = self.tool_family_rect(family);
            self.button(
                p,
                r,
                Action::ToolPanel(family),
                "",
                Some(icon),
                ToolPanel::for_tool(self.tool) == Some(family),
                !self.busy,
                false,
            );
            if let Some(c) = self.controls.last_mut() {
                c.label = tip.into();
            }
        }
        self.icon_button(
            p,
            Rect::new(bar.x + 128., bar.y + 8., 36., 32.),
            Action::Tool(Tool::Text),
            "text",
            "Текст · T",
            self.tool == Tool::Text,
            !self.busy,
        );
        let color_button = Rect::new(bar.x + 168., bar.y + 8., 36., 32.);
        let before = self.controls.len();
        self.button(
            p,
            color_button,
            Action::Inspector(false),
            "",
            None,
            !self.inspector_layers,
            true,
            false,
        );
        if self.controls.len() > before {
            if let Some(control) = self.controls.last_mut() {
                control.label = "Цвет · открыть палитру".into();
            }
        }
        let (cx, cy) = (
            color_button.x + color_button.w * 0.5,
            color_button.y + color_button.h * 0.5,
        );
        p.circle(cx, cy, 9., t.text, true);
        p.circle(cx, cy, 7.5, rgb(self.brush.color), true);

        let x = bar.x + 224.;
        p.rect(Rect::new(x - 9., bar.y + 14., 1., 20.), t.border);
        for (i, tool) in [Tool::Fill, Tool::Picker, Tool::Hand]
            .into_iter()
            .enumerate()
        {
            let (icon, label, key) = tool.info();
            self.icon_button(
                p,
                Rect::new(x + i as f32 * 40., bar.y + 8., 36., 32.),
                Action::Tool(tool),
                icon,
                &format!("{label} · {key}"),
                self.tool == tool,
                !self.busy,
            );
        }
        let modes = x + 128.;
        p.rect(Rect::new(modes - 9., bar.y + 14., 1., 20.), t.border);
        for (i, (action, icon, tip, selected)) in [
            (
                Action::Mirror,
                "mirror",
                "Зеркальная кисть · M",
                self.brush.mirror,
            ),
            (Action::Value, "tone", "Просмотр тона · V", self.grayscale),
            (Action::Grid, "grid", "Сетка · Ctrl+'", self.grid),
        ]
        .into_iter()
        .enumerate()
        {
            self.icon_button(
                p,
                Rect::new(modes + i as f32 * 40., bar.y + 8., 36., 32.),
                action,
                icon,
                tip,
                selected,
                true,
            );
        }
        let grip = self.toolbar_grip_rect();
        let grip_active = self.hover == Some(self.controls.len())
            || self.focus == Some(self.controls.len())
            || matches!(
                self.drag,
                Drag::Panel {
                    kind: PanelKind::Toolbar,
                    ..
                }
            );
        if grip_active {
            p.rounded(grip, 4., t.raised);
        }
        for row in 0..3 {
            for col in 0..2 {
                p.circle(
                    grip.x + 7. + col as f32 * 6.,
                    grip.y + 10. + row as f32 * 6.,
                    1.25,
                    if grip_active { t.accent } else { t.muted },
                    true,
                );
            }
        }
        self.control(
            grip,
            Action::MoveToolbar,
            "Переместить панель · стрелки / Home",
            true,
        );
        for (offset, action, icon, label, enabled) in [
            (
                492.,
                Action::Undo,
                "undo",
                "Отменить · Ctrl+Z",
                self.doc.can_undo(),
            ),
            (
                528.,
                Action::Redo,
                "redo",
                "Повторить · Ctrl+Shift+Z",
                self.doc.can_redo(),
            ),
        ] {
            self.icon_button(
                p,
                Rect::new(bar.x + offset, bar.y + 8., 32., 32.),
                action,
                icon,
                label,
                false,
                enabled,
            );
        }
        let ux = bar.x + bar.w - 168.;
        p.rect(Rect::new(ux - 9., bar.y + 14., 1., 20.), t.border);
        self.icon_button(
            p,
            Rect::new(ux, bar.y + 8., 28., 32.),
            Action::ZoomOut,
            "minus",
            "Уменьшить масштаб · −",
            false,
            true,
        );
        self.button(
            p,
            Rect::new(ux + 32., bar.y + 8., 56., 32.),
            Action::Actual,
            &format!("{:.0}%", self.zoom * 100.),
            None,
            false,
            true,
            false,
        );
        if let Some(c) = self.controls.last_mut() {
            c.label = "Масштаб 100% · 1".into();
        }
        self.icon_button(
            p,
            Rect::new(ux + 92., bar.y + 8., 28., 32.),
            Action::ZoomIn,
            "plus",
            "Увеличить масштаб · +",
            false,
            true,
        );
        self.icon_button(
            p,
            Rect::new(ux + 128., bar.y + 8., 32., 32.),
            Action::Fit,
            "fit",
            "Вписать холст · 0",
            false,
            true,
        );
        self.hit_clip = None;
        p.clip = Rect::new(0., 0., self.w(), self.h());
    }

    fn draw_quick_controls(&mut self, p: &mut Painter) {
        let t = Theme::with_accent(self.dark, self.accent);
        for action in [Action::Size, Action::Opacity] {
            let card = self.quick_control_rect(action);
            let track = self.quick_slider_rect(action);
            let (min, max, label, _, _) = self.parameter_info(action);
            self.occlude_underlying_surface(p, card);
            self.floating_surface_with_radius(p, card, label, 12.);
            p.clip = card.inset(1.);
            self.hit_clip = Some(p.clip);
            let value = self.parameter_value(action);
            let enabled = !self.busy && !(self.tool == Tool::Fill && action == Action::Size);
            let progress = ((value - min) / (max - min)).clamp(0., 1.);
            let active =
                matches!(self.drag, Drag::Slider { action: current, .. } if current == action);
            let accent = if enabled { t.accent } else { t.muted };
            let track_hit = Rect::new(track.x - 5., card.y + 7., track.w + 10., 26.);
            if self.focus == Some(self.controls.len()) {
                p.rounded(track_hit.inset(-2.), 4., t.accent);
                p.rounded(track_hit.inset(-1.), 3., t.surface);
            }
            p.rounded(track, 2., t.border);
            p.rounded(
                Rect::new(track.x, track.y, track.w * progress, track.h),
                2.,
                accent,
            );
            let cx = track.x + track.w * progress;
            p.circle(cx, track.y + 2., if active { 6. } else { 5. }, accent, true);
            p.circle(cx, track.y + 2., 2., t.on_accent, true);
            self.control_with_kind(
                track_hit,
                action,
                format!("{label}: стрелки — изменить, Enter — точное значение"),
                enabled,
                ControlKind::Slider,
            );
            let number = Rect::new(card.x + card.w - 56., card.y + 4., 44., 32.);
            if self.focus == Some(self.controls.len()) {
                p.rounded(number, 4., t.accent);
                p.rounded(number.inset(1.), 3., t.surface);
            } else if enabled && self.hover == Some(self.controls.len()) {
                p.rounded(number, 4., t.raised);
            }
            p.text(
                number,
                if action == Action::Opacity {
                    format!("{value:.0}%")
                } else {
                    format!("{value:.0}")
                },
                12.,
                if enabled { t.text } else { t.muted },
                false,
                false,
                TextAlign::Right,
            );
            self.control_with_kind(
                number,
                action,
                format!("Точное значение: {label}"),
                enabled,
                ControlKind::Value,
            );
            self.hit_clip = None;
            p.clip = Rect::new(0., 0., self.w(), self.h());
        }
    }

    fn draw_quick_preview(&mut self, p: &mut Painter, action: Action) {
        let t = Theme::with_accent(self.dark, self.accent);
        let r = self.quick_preview_rect(action);
        let area = r.inset(12.);
        let radius = self.quick_preview_radius(action);
        if action == Action::Size {
            let (cx, cy) = (r.x + r.w / 2., r.y + r.h / 2.);
            // The size sample is a cursor-like outline on the artwork. Keep its
            // old panel's transparent corners free of invisible controls.
            self.quick_preview_painted = Some(PreviewHit::Circle {
                cx,
                cy,
                radius: radius + 1. + 1.2_f32.max(1.2 / self.scale),
            });
            p.circle(cx, cy, radius + 1., 0x111111, false);
            p.circle(cx, cy, radius, 0xf5f5f5, false);
            return;
        }
        self.quick_preview_painted = Some(PreviewHit::Panel(r));
        self.occlude_underlying_surface(p, r);
        self.floating_surface(p, r, self.parameter_info(action).2);
        p.clip = area;
        {
            for y in 0..11 {
                for x in 0..11 {
                    p.rect(
                        Rect::new(area.x + x as f32 * 8., area.y + y as f32 * 8., 8., 8.),
                        if (x + y) % 2 == 0 {
                            t.raised
                        } else {
                            t.surface
                        },
                    );
                }
            }
            let color = if self.tool == Tool::Eraser {
                t.text
            } else {
                rgb(self.brush.color)
            };
            if self
                .quick_disc
                .as_ref()
                .is_none_or(|(cached, _)| *cached != color)
            {
                let mut disc = vec![Color::TRANSPARENT; 88 * 88];
                let base = Color::from_hex(color);
                for y in 0..88 {
                    for x in 0..88 {
                        let distance = (x as f32 + 0.5 - 44.).hypot(y as f32 + 0.5 - 44.);
                        let alpha = ((radius + 0.5 - distance).clamp(0., 1.) * 255.).round() as u8;
                        disc[y * 88 + x] = Color::rgba(base.r, base.g, base.b, alpha);
                    }
                }
                self.quick_disc = Some((color, disc));
            }
            p.image(
                area,
                88,
                88,
                &self.quick_disc.as_ref().unwrap().1,
                false,
                self.brush.opacity,
            );
        }
        p.clip = Rect::new(0., 0., self.w(), self.h());
    }

    fn draw_tool_popup(&mut self, p: &mut Painter) {
        let (Some(family), Some(r)) = (self.tool_popup, self.tool_popup_rect()) else {
            return;
        };
        self.prepare_browser_layout(family);
        let t = Theme::with_accent(self.dark, self.accent);
        let visible = self.motion_hit_clip.map_or(r, |clip| r.intersection(clip));
        p.frame
            .texts
            .retain(|v| !visible.intersects(Rect::new(v.x, v.y, v.width, v.height)));
        for c in &mut self.controls {
            if c.hit.intersects(r) {
                c.enabled = false;
            }
        }
        let anchor = self.tool_family_rect(family);
        if self.tool_panel_pinned {
            self.floating_surface(p, r, "Выбор инструмента");
        } else if r.y < self.top_tools_rect().y {
            self.floating_surface(p, r, "Выбор инструмента");
            p.triangle_down(
                (anchor.x + anchor.w / 2.).clamp(r.x + 12., r.x + r.w - 12.),
                r.y + r.h - 1.,
                8.,
                9.,
                t.surface,
            );
        } else {
            self.dropdown_surface(p, r, anchor.x + anchor.w / 2., "Выбор инструмента");
        }
        p.clip = r.inset(1.);
        self.hit_clip = Some(p.clip);
        self.icon_button(
            p,
            Rect::new(r.x + r.w - 36., r.y + 12., 28., 28.),
            Action::PinToolPanel,
            if self.tool_panel_pinned {
                "pin_active"
            } else {
                "pin"
            },
            if self.tool_panel_pinned {
                "Перетащить панель · щелчок — открепить"
            } else {
                "Закрепить панель"
            },
            self.tool_panel_pinned,
            true,
        );
        self.icon_button(
            p,
            self.tool_settings_button_rect(),
            Action::ToolProperties,
            "gear",
            match family {
                ToolPanel::Brush => "Параметры кисти · F4",
                ToolPanel::Eraser => "Параметры ластика · F4",
                ToolPanel::Shapes => "Параметры фигуры · F4",
            },
            false,
            true,
        );

        let items = self.picker_items(family);
        let viewport = self.browser_items_rect(family);
        let (card_width, step) = self.browser_item_metrics(family);
        let offset = self.browser.view(family).items.offset;
        p.clip = viewport;
        self.hit_clip = Some(viewport);
        for (index, (action, item)) in items.iter().copied().enumerate() {
            let card = Rect::new(
                viewport.x + index as f32 * step - offset,
                viewport.y,
                card_width,
                52.,
            );
            if !card.intersects(viewport) {
                continue;
            }
            let label = match action {
                Action::Brush(slot) => self.preset_name(family.index(), slot),
                _ => self.language.translate(item.label()),
            };
            let selected = action == self.selected_tool_option();
            let before = self.controls.len();
            self.button(p, card, action, "", None, selected, !self.busy, false);
            if self.controls.len() > before {
                self.controls.last_mut().unwrap().label = label.clone();
            }
            match item {
                ItemId::Shape(kind) => {
                    let shape = Self::tool_from_shape(kind);
                    p.icon(
                        Rect::new(card.x + (card.w - 32.) / 2., card.y + 3., 32., 30.),
                        shape.info().0,
                        t.text,
                    );
                }
                ItemId::Brush(_) | ItemId::Eraser(_) => {
                    let Action::Brush(slot) = action else {
                        unreachable!()
                    };
                    let erase = family == ToolPanel::Eraser;
                    let key = (erase, slot, self.dark);
                    let settings = if self.preset_family() == Some(family.index())
                        && self.presets.selected[family.index()] == slot
                    {
                        ToolSettings::from_brush(self.brush)
                    } else {
                        self.presets.entries[family.index()][slot as usize].settings
                    };
                    if self
                        .brush_samples
                        .get(&key)
                        .is_none_or(|(old, _)| *old != settings)
                    {
                        self.brush_samples
                            .insert(key, (settings, Self::tool_sample(settings, t.text, erase)));
                    }
                    let sample_rect = Rect::new(card.x + 3., card.y + 3., card.w - 6., 30.);
                    if erase {
                        self.draw_sample_checker(p, sample_rect);
                    }
                    p.image(sample_rect, 112, 40, &self.brush_samples[&key].1, false, 1.);
                }
            }
            p.text_raw(
                Rect::new(card.x + 3., card.y + 31., card.w - 6., 19.),
                label,
                10.,
                t.text,
                false,
                false,
                TextAlign::Center,
            );
        }
        if items.is_empty() {
            p.text(
                viewport,
                "Ничего не найдено",
                11.,
                t.muted,
                false,
                false,
                TextAlign::Center,
            );
        }

        if !self.browser.view(family).search_open {
            let categories = self.browser_categories_rect(family);
            let view = self.browser.view(family);
            let offset = view.categories.offset;
            let selected = view.category;
            p.clip = r.inset(1.);
            self.hit_clip = Some(p.clip);
            self.icon_button(
                p,
                self.browser_all_category_rect(family),
                Action::BrowserCategory(Category::All),
                Category::All.icon(),
                Category::All.label(),
                selected == Category::All,
                true,
            );
            p.clip = categories;
            self.hit_clip = Some(categories);
            for (index, category) in Category::for_family(family)
                .iter()
                .copied()
                .skip(1)
                .enumerate()
            {
                let card = Rect::new(
                    categories.x + index as f32 * 36. - offset,
                    categories.y,
                    32.,
                    24.,
                );
                if !card.intersects(categories) {
                    continue;
                }
                self.icon_button(
                    p,
                    card,
                    Action::BrowserCategory(category),
                    category.icon(),
                    category.label(),
                    category == selected,
                    true,
                );
            }
        }
        p.clip = r.inset(1.);
        self.hit_clip = Some(p.clip);
        self.draw_browser_search(p, family);
        self.hit_clip = None;
        p.clip = Rect::new(0., 0., self.w(), self.h());
    }

    fn draw_browser_search(&mut self, p: &mut Painter, family: ToolPanel) {
        let toggle = self.browser_search_button_rect(family);
        if !self.browser.view(family).search_open {
            self.flat_search_button(
                p,
                toggle,
                Action::BrowserSearch,
                "search",
                Self::browser_search_label(family),
                false,
            );
            return;
        }
        let full = self.browser_search_rect(family);
        let progress = if self.motion_enabled() {
            self.browser_search_motion.value()
        } else {
            1.
        };
        let width = toggle.w + (full.w - toggle.w) * progress;
        let r = Rect::new(full.x + full.w - width, full.y, width, full.h);
        let outer_clip = p.clip;
        let outer_hit_clip = self.hit_clip;
        p.clip = p.clip.intersection(r);
        self.hit_clip = Some(p.clip);
        let t = Theme::with_accent(self.dark, self.accent);
        let focused = self.browser_search_focus;
        let has_text = !self.browser.view(family).query.text().is_empty();
        p.rounded(r, 4., t.raised);
        self.control(
            r,
            Action::BrowserSearch,
            Self::browser_search_label(family),
            true,
        );
        let input = Rect::new(
            r.x + 8.,
            r.y + 2.,
            (r.w - if has_text { 68. } else { 44. }).max(0.),
            20.,
        );
        if !has_text && !focused {
            p.text(input, "Поиск…", 11., t.muted, false, false, TextAlign::Left);
        } else {
            self.draw_inline_text(p, input, &self.browser.view(family).query, focused);
        }
        if has_text {
            self.flat_search_button(
                p,
                Rect::new(r.x + r.w - 56., r.y + 3., 18., 18.),
                Action::ClearBrowserSearch,
                "close",
                "Очистить поиск",
                false,
            );
        }
        self.flat_search_button(
            p,
            toggle,
            Action::CloseBrowserSearch,
            "search",
            "Закрыть поиск",
            true,
        );
        p.clip = outer_clip;
        self.hit_clip = outer_hit_clip;
    }

    fn draw_inline_text(
        &self,
        p: &mut Painter,
        input: Rect,
        edit: &crate::tool_browser::SearchText,
        focused: bool,
    ) {
        let old_clip = p.clip;
        p.clip = old_clip.intersection(input);
        let t = Theme::with_accent(self.dark, self.accent);
        let (text, caret, selection) = (edit.text(), edit.caret(), edit.selection());
        let mut start = 0;
        if focused {
            while start < caret && self.browser_text_width(&text[start..caret]) > input.w - 4. {
                start += text[start..].chars().next().unwrap().len_utf8();
            }
        }
        if focused {
            if let Some((a, b)) = selection {
                let a = a.max(start);
                let b = b.max(start);
                let x = input.x + self.browser_text_width(&text[start..a]);
                let width = self.browser_text_width(&text[a..b]);
                p.rect(Rect::new(x, input.y + 2., width, input.h - 4.), t.selected);
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
                    input.y + 3.,
                    1.,
                    input.h - 6.,
                ),
                t.accent,
            );
        }
        p.clip = old_clip;
    }

    fn browser_search_label(family: ToolPanel) -> &'static str {
        match family {
            ToolPanel::Brush => "Поиск кистей",
            ToolPanel::Eraser => "Поиск ластиков",
            ToolPanel::Shapes => "Поиск фигур",
        }
    }

    fn flat_search_button(
        &mut self,
        p: &mut Painter,
        r: Rect,
        action: Action,
        icon: &str,
        label: &str,
        active: bool,
    ) {
        let t = Theme::with_accent(self.dark, self.accent);
        let index = self.controls.len();
        if self.hover == Some(index) || self.focus == Some(index) {
            p.rounded(r, 4., t.selected);
        }
        p.icon(r, icon, if active { t.accent } else { t.muted });
        self.control(r, action, label, true);
    }

    fn draw_panel(&mut self, p: &mut Painter) {
        let t = Theme::with_accent(self.dark, self.accent);
        let (w, h) = (self.w(), self.h());
        let panel = self.right_panel_rect();
        let x = panel.x;
        let sx = x + 20.;
        let sw = 248.;
        self.floating_surface(p, panel, "Цвет и слои");
        self.button(
            p,
            Rect::new(x + 12., panel.y + 10., 128., 30.),
            Action::Inspector(false),
            "Цвет",
            None,
            !self.inspector_layers,
            true,
            false,
        );
        self.button(
            p,
            Rect::new(x + 148., panel.y + 10., 128., 30.),
            Action::Inspector(true),
            "Слои",
            None,
            self.inspector_layers,
            true,
            false,
        );
        let off = if self.inspector_layers {
            0.
        } else {
            self.panel_scroll.clamp(0., (782. - h).max(0.))
        };
        let offset = if self.inspector_layers { 393. } else { 149. };
        let yy = |v: f32| v - offset - off;
        p.clip = Rect::new(x + 1., panel.y + 1., panel.w - 2., (h - 207.).max(0.));
        self.hit_clip = Some(p.clip);
        if !self.inspector_layers {
            p.text(
                Rect::new(sx, yy(351.), 90., 24.),
                "ЦВЕТ",
                10.,
                t.muted,
                true,
                false,
                TextAlign::Left,
            );
            p.rounded(Rect::new(sx + 108., yy(348.), 140., 28.), 4., t.raised);
            p.rounded(
                Rect::new(sx + 114., yy(354.), 16., 16.),
                4.,
                rgb(self.brush.color),
            );
            p.text(
                Rect::new(sx + 140., yy(348.), 102., 28.),
                hex(self.brush.color),
                12.,
                t.text,
                false,
                false,
                TextAlign::Left,
            );
            self.control(
                Rect::new(sx + 108., yy(348.), 140., 28.),
                Action::Hex,
                "HEX · точный цвет",
                true,
            );
            let sv = Rect::new(sx, yy(388.), sw, 84.);
            let stepsx = (sv.w * self.scale) as u32;
            let stepsy = (sv.h * self.scale) as u32;
            for py in 0..stepsy {
                for px in 0..stepsx {
                    let c = hsv(
                        self.color_h,
                        px as f32 / stepsx.max(1) as f32,
                        1. - py as f32 / stepsy.max(1) as f32,
                    );
                    p.pixel(
                        (sv.x * self.scale) as i32 + px as i32,
                        (sv.y * self.scale) as i32 + py as i32,
                        rgb(c),
                        1.,
                    );
                }
            }
            p.circle(
                sv.x + self.color_s * sv.w,
                sv.y + (1. - self.color_v) * sv.h,
                5.,
                0x141414,
                false,
            );
            p.circle(
                sv.x + self.color_s * sv.w,
                sv.y + (1. - self.color_v) * sv.h,
                3.5,
                0xffffff,
                false,
            );
            self.control(
                sv,
                Action::SV,
                "Цвет: стрелки меняют насыщенность и яркость",
                true,
            );
            let hue = Rect::new(sx, yy(485.), sw, 10.);
            for i in 0..sw as i32 {
                p.rect(
                    Rect::new(sx + i as f32, hue.y, 1., 10.),
                    rgb(hsv(i as f32 / sw * 360., 1., 1.)),
                );
            }
            let hx = sx + self.color_h / 360. * sw;
            p.rounded(Rect::new(hx - 3., hue.y - 3., 6., 16.), 3., 0xf6f6f6);
            p.rounded(
                Rect::new(hx - 1., hue.y - 1., 2., 12.),
                1.,
                rgb(hsv(self.color_h, 1., 1.)),
            );
            self.control(
                Rect::new(hue.x, hue.y - 4., hue.w, hue.h + 8.),
                Action::Hue,
                "Оттенок: стрелки меняют значение",
                true,
            );
            for (i, c) in PALETTE.iter().enumerate() {
                let r = Rect::new(sx + i as f32 * 32., yy(510.), 24., 24.);
                p.rounded(r, 4., *c);
                if rgb(self.brush.color) == *c {
                    p.rounded(r.inset(-2.), 4., t.text);
                    p.rounded(r.inset(-0.5), 4., *c);
                }
                self.control(r, Action::Swatch(i), format!("Цвет #{c:06X}"), true);
            }
            p.label(sx, yy(546.), 90., "НЕДАВНИЕ", 9., t.muted);
            for i in 0..8 {
                let r = Rect::new(sx + 88. + i as f32 * 20., yy(547.), 14., 14.);
                if let Some(c) = self.recent.get(i) {
                    let c = *c;
                    p.rounded(r, 3., rgb(c));
                    self.control(r, Action::Recent(i), format!("Недавний {}", hex(c)), true);
                } else {
                    p.rounded(r, 3., t.raised);
                }
            }
            p.rect(Rect::new(sx, yy(579.), sw, 1.), t.border);
        }
        p.text(
            Rect::new(sx, yy(585.), 155., 30.),
            format!("СЛОИ  /  {:02}", self.doc.layers.len()),
            10.,
            t.muted,
            true,
            false,
            TextAlign::Left,
        );
        self.icon_button(
            p,
            Rect::new(sx + 216., yy(584.), 32., 28.),
            Action::AddLayer,
            "plus",
            "Добавить слой · Ctrl+Shift+N",
            false,
            self.doc.layers.len() < document::MAX_LAYERS,
        );
        let opacity = self.doc.layers[self.doc.active].opacity as f32 / 255.;
        self.slider(
            p,
            Rect::new(sx, yy(615.), sw, 42.),
            Action::LayerOpacity,
            opacity,
            1.,
            "Непрозрачность слоя",
            format!("{:.0}%", opacity * 100.),
            true,
        );
        let row_top = yy(662.);
        let row_bottom = (h - 62.).max(row_top + 40.);
        let count = (((row_bottom - row_top - 40.) / 44.).floor() + 1.).max(1.) as usize;
        self.layer_scroll = self
            .layer_scroll
            .min(self.doc.layers.len().saturating_sub(count));
        for pos in 0..count {
            let n = self.doc.layers.len();
            if pos + self.layer_scroll >= n {
                break;
            }
            let i = n - 1 - pos - self.layer_scroll;
            let y = row_top + pos as f32 * 44.;
            let selected = i == self.doc.active;
            let visible = self.doc.layers[i].visible;
            let locked = self.doc.layers[i].locked;
            let r = Rect::new(sx - 8., y, sw + 16., 40.);
            if selected {
                p.rounded(r, 4., t.accent);
                p.rounded(r.inset(1.), 3., t.selected);
            }
            self.control(
                Rect::new(sx + 26., y, sw - 56., 40.),
                Action::Layer(i),
                format!("Слой: {} · F2 — имя", self.doc.layers[i].name),
                true,
            );
            self.icon_button(
                p,
                Rect::new(sx - 6., y + 7., 26., 26.),
                Action::Visible(i),
                if visible { "eye" } else { "hidden" },
                "Показать / скрыть слой",
                false,
                true,
            );
            let thumb = Rect::new(sx + 28., y + 6., 38., 28.);
            if self.doc.layers[i].text.is_some() {
                self.icon_button(
                    p,
                    thumb,
                    Action::EditTextLayer(self.doc.layers[i].id),
                    "text",
                    "Редактировать надпись",
                    false,
                    visible && !locked && !self.busy,
                );
            } else {
                p.rounded(thumb, 4., t.workspace);
                let old = p.clip;
                p.clip = thumb;
                p.image(
                    thumb,
                    self.doc.width,
                    self.doc.height,
                    &self.doc.layers[i].pixels,
                    false,
                    1.,
                );
                p.clip = old;
            }
            p.label_raw(
                sx + 77.,
                y + 8.,
                135.,
                self.doc.layers[i].name.clone(),
                12.,
                if visible { t.text } else { t.muted },
            );
            self.icon_button(
                p,
                Rect::new(sx + 220., y + 7., 26., 26.),
                Action::Lock(i),
                if locked { "lock" } else { "unlock" },
                "Заблокировать / разблокировать слой",
                false,
                true,
            );
        }
        if self.doc.layers.len() > count {
            let trackh = (row_bottom - row_top).max(10.);
            p.rounded(Rect::new(x + 278., row_top, 2., trackh), 1., t.border);
            p.rounded(
                Rect::new(
                    x + 278.,
                    row_top + trackh * self.layer_scroll as f32 / self.doc.layers.len() as f32,
                    3.,
                    (trackh * count as f32 / self.doc.layers.len() as f32).max(10.),
                ),
                1.,
                t.muted,
            );
        }
        if !self.inspector_layers && h < 782. {
            let track = Rect::new(x + 283., panel.y + 8., 2., (h - panel.y - 78.).max(20.));
            let maximum = 782. - h;
            let thumb = (track.h * track.h / (track.h + maximum)).max(24.);
            p.rounded(track, 1., t.raised);
            p.rounded(
                Rect::new(
                    track.x,
                    track.y + (track.h - thumb) * off / maximum,
                    2.,
                    thumb,
                ),
                1.,
                t.muted,
            );
        }
        p.clip = Rect::new(0., 0., w, h);
        self.hit_clip = None;
        let by = h - 52.;
        p.clip = panel.inset(1.);
        self.hit_clip = Some(panel.inset(1.));
        p.rect(Rect::new(x + 1., by - 8., 286., 43.), t.surface);
        p.rect(Rect::new(sx, by - 9., sw, 1.), t.border);
        for (i, (act, icon, tip)) in [
            (Action::Rename, "brush", "Имя слоя · F2"),
            (Action::Duplicate, "copy", "Дублировать слой"),
            (Action::Raise, "up", "Переместить слой выше"),
            (Action::Lower, "down", "Переместить слой ниже"),
            (Action::DeleteLayer, "trash", "Удалить слой · Ctrl+Delete"),
        ]
        .into_iter()
        .enumerate()
        {
            let enabled = match act {
                Action::Raise => self.doc.active + 1 < self.doc.layers.len(),
                Action::Lower => self.doc.active > 0,
                Action::DeleteLayer => self.doc.layers.len() > 1,
                _ => true,
            };
            self.icon_button(
                p,
                Rect::new(sx + i as f32 * 50., by, 40., 28.),
                act,
                icon,
                tip,
                false,
                enabled,
            );
        }
        p.clip = Rect::new(0., 0., w, h);
        self.hit_clip = None;
    }
    fn draw_overlay(&mut self, p: &mut Painter) {
        if self.overlay == Overlay::ToolProperties {
            self.draw_tool_settings(p);
            return;
        }
        if self.overlay == Overlay::Settings {
            self.draw_settings(p);
            return;
        }
        let t = Theme::with_accent(self.dark, self.accent);
        let (w, h) = (self.w(), self.h());
        self.controls.clear();
        self.hover = None;
        if self.overlay == Overlay::File {
            let r = self.file_menu_rect();
            p.frame.texts.retain(|v| {
                !Rect::new(v.x, v.y, v.width, v.height).contains(r.x, r.y)
                    && !(v.x < r.x + r.w
                        && v.x + v.width > r.x
                        && v.y < r.y + r.h
                        && v.y + v.height > r.y)
            });
            p.soft_shadow(r, 4., if self.dark { 0.14 } else { 0.10 });
            p.rounded(r, 4., t.surface);
            p.triangle_up(105., r.y + 1., 8., 9., t.surface);
            let items = [
                (Action::New, "Новый холст", "Ctrl+N"),
                (Action::Open, "Открыть проект…", "Ctrl+O"),
                (Action::Import, "Добавить изображение…", "Ctrl+Shift+O"),
                (Action::Save, "Сохранить", "Ctrl+S"),
                (Action::SaveAs, "Сохранить как…", "Ctrl+Shift+S"),
                (Action::Export, "Экспорт PNG…", "Ctrl+Shift+E"),
                (Action::Demo, "Открыть этюд FORM 01", ""),
                (Action::Help, "Клавиши и инструменты", "F1"),
            ];
            for (i, (a, label, key)) in items.into_iter().enumerate() {
                let item = Rect::new(r.x + 8., r.y + 8. + i as f32 * 40., r.w - 16., 36.);
                if i == self.menu_index {
                    p.rounded(item, 4., t.accent);
                    p.rounded(item.inset(1.), 3., t.selected);
                }
                p.label(item.x + 9., item.y + 7., item.w - 110., label, 12., t.text);
                p.text(
                    Rect::new(item.x + item.w - 102., item.y, 93., 36.),
                    key,
                    10.,
                    t.muted,
                    false,
                    false,
                    TextAlign::Right,
                );
                self.control(item, a, label, !self.busy);
            }
        } else {
            // Clear underlying text; dim the exact framebuffer before adding dialog content.
            p.frame.texts.clear();
            for c in &mut p.frame.pixels {
                let r = (*c >> 16 & 255) * 38 / 100;
                let g = (*c >> 8 & 255) * 38 / 100;
                let b = (*c & 255) * 38 / 100;
                *c = (r << 16) | (g << 8) | b;
            }
            let dh = if self.overlay == Overlay::New {
                390.
            } else {
                530.
            };
            let r = Rect::new((w - 540.) / 2., (h - dh) / 2., 540., dh);
            p.soft_shadow(r, 4., if self.dark { 0.14 } else { 0.10 });
            p.rounded(r, 4., t.surface);
            self.icon_button(
                p,
                Rect::new(r.x + r.w - 48., r.y + 16., 32., 32.),
                Action::Cancel,
                "close",
                "Закрыть · Escape",
                false,
                true,
            );
            if self.overlay == Overlay::New {
                p.text(
                    Rect::new(r.x + 32., r.y + 22., 430., 40.),
                    "Место для новой идеи.",
                    24.,
                    t.text,
                    true,
                    false,
                    TextAlign::Left,
                );
                p.label(
                    r.x + 32.,
                    r.y + 69.,
                    440.,
                    "Выберите формат. Остальное — ваш почерк.",
                    12.,
                    t.muted,
                );
                for (i, (label, sub)) in [
                    ("Этюд", "1200 × 840"),
                    ("Квадрат", "1080 × 1080"),
                    ("Постер", "1200 × 1600"),
                ]
                .into_iter()
                .enumerate()
                {
                    let rr = Rect::new(r.x + 32. + i as f32 * 161., r.y + 115., 152., 76.);
                    self.button(
                        p,
                        rr,
                        Action::NewPreset(i as u8),
                        label,
                        None,
                        matches!(
                            (self.new_width, self.new_height, i),
                            (1200, 840, 0) | (1080, 1080, 1) | (1200, 1600, 2)
                        ),
                        true,
                        false,
                    );
                    p.label(rr.x + 32., rr.y + 52., 120., sub, 10., t.muted);
                }
                p.label(r.x + 32., r.y + 215., 120., "Ширина / px", 11., t.muted);
                p.label(r.x + 283., r.y + 215., 140., "Высота / px", 11., t.muted);
                self.button(
                    p,
                    Rect::new(r.x + 32., r.y + 240., 225., 40.),
                    Action::NewWidth,
                    &self.new_width.to_string(),
                    None,
                    false,
                    true,
                    false,
                );
                self.button(
                    p,
                    Rect::new(r.x + 283., r.y + 240., 225., 40.),
                    Action::NewHeight,
                    &self.new_height.to_string(),
                    None,
                    false,
                    true,
                    false,
                );
                p.label(
                    r.x + 32.,
                    r.y + 300.,
                    320.,
                    "Бумага + пустой слой · до 4096 px на сторону",
                    11.,
                    t.muted,
                );
                self.button(
                    p,
                    Rect::new(r.x + 352., r.y + 330., 156., 36.),
                    Action::Create,
                    "Создать холст",
                    None,
                    false,
                    true,
                    true,
                );
            } else {
                p.text(
                    Rect::new(r.x + 32., r.y + 24., 420., 40.),
                    "Рисуйте в своём ритме.",
                    24.,
                    t.text,
                    true,
                    false,
                    TextAlign::Left,
                );
                p.label(
                    r.x + 32.,
                    r.y + 73.,
                    420.,
                    "Portfolio / Краткая карта студии",
                    12.,
                    t.muted,
                );
                let help = [
                    ("B / E / G / I", "Кисть / ластик / заливка / пипетка"),
                    ("L / R / O", "Линия / прямоугольник / эллипс"),
                    ("Пробел + движение", "Перемещение холста"),
                    ("Колесо / 0 / 1", "Масштаб / вписать / 100%"),
                    ("[ / ]", "Уменьшить / увеличить кисть"),
                    ("M / V / Tab", "Зеркало / тон / режим фокуса"),
                    ("Ctrl+Z / Ctrl+Shift+Z", "Отмена / повтор"),
                    ("Ctrl+S / Ctrl+Shift+E", "Сохранить проект / PNG"),
                    ("F6 / Shift+F6", "Фокус по элементам интерфейса"),
                    ("Enter / стрелки", "Активировать / точное изменение"),
                    ("Escape", "Отменить жест / закрыть панель"),
                ];
                for (i, (key, label)) in help.into_iter().enumerate() {
                    let y = r.y + 121. + i as f32 * 31.;
                    p.label(r.x + 32., y, 185., key, 11., t.accent);
                    p.label(r.x + 218., y, 290., label, 11., t.text);
                }
                p.label(
                    r.x + 32.,
                    r.y + 482.,
                    468.,
                    "F2 — имя слоя. Alt — временная пипетка. Цвета рисунка не зависят от темы.",
                    10.,
                    t.muted,
                );
            }
        }
    }
}
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Action {
    NoOp,
    File,
    New,
    Open,
    Import,
    Save,
    SaveAs,
    Export,
    Demo,
    Undo,
    Redo,
    Theme,
    Settings,
    CloseSettings,
    SettingsSearch,
    ClearSettingsSearch,
    SettingsScroll,
    FontMenu,
    LanguageMenu,
    SetTheme(bool),
    SetFont(UiFont),
    SetAccent(Accent),
    #[allow(dead_code)] // Programmatic section navigation retained for regressions.
    SettingsSection(u8),
    SetLanguage(Language),
    FontSize,
    SetAnimations(bool),
    Inspector(bool),
    BrowserSearch,
    CloseBrowserSearch,
    ClearBrowserSearch,
    BrowserCategory(Category),
    ToolSection(u8),
    MoveToolbar,
    ResetPanelLayout,
    #[allow(dead_code)] // Programmatic jump to the vertical stamp section.
    BrushDetail(bool),
    ResetBrush,
    RenameBrush,
    AddBrush,
    Roundness,
    Grain,
    Hardness,
    Flow,
    Spacing,
    Angle,
    Scatter,
    ResetAppearance,
    Help,
    Focus,
    Fit,
    Actual,
    ZoomIn,
    ZoomOut,
    Tool(Tool),
    ToolProperties,
    CloseToolProperties,
    ToolPropertiesScroll,
    ToolPanel(ToolPanel),
    PinToolPanel,
    Shape(Tool),
    ShapeFill,
    Mirror,
    Value,
    Grid,
    Brush(u8),
    Size,
    Opacity,
    Hex,
    SV,
    Hue,
    Swatch(usize),
    Recent(usize),
    AddLayer,
    Layer(usize),
    EditTextLayer(u64),
    Visible(usize),
    Lock(usize),
    LayerOpacity,
    Rename,
    Duplicate,
    DeleteLayer,
    Raise,
    Lower,
    NewPreset(u8),
    NewWidth,
    NewHeight,
    Create,
    Cancel,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ControlKind {
    Button,
    Slider,
    Value,
}
#[derive(Clone, Copy, Debug)]
enum PreviewHit {
    Panel(Rect),
    Circle { cx: f32, cy: f32, radius: f32 },
}
impl PreviewHit {
    fn contains(self, x: f32, y: f32) -> bool {
        match self {
            Self::Panel(rect) => rect.contains(x, y),
            Self::Circle { cx, cy, radius } => {
                (x - cx).powi(2) + (y - cy).powi(2) <= radius * radius
            }
        }
    }
}
#[derive(Clone)]
struct Control {
    rect: Rect,
    hit: Rect,
    action: Action,
    label: String,
    enabled: bool,
    kind: ControlKind,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Overlay {
    None,
    File,
    New,
    Help,
    Settings,
    ToolProperties,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SettingsChoice {
    Font,
    Language,
}
impl SettingsChoice {
    fn row_height(self) -> f32 {
        match self {
            Self::Font => 28.,
            Self::Language => 32.,
        }
    }
    fn trigger(self) -> Action {
        match self {
            Self::Font => Action::FontMenu,
            Self::Language => Action::LanguageMenu,
        }
    }
    fn len(self) -> usize {
        match self {
            Self::Font => UiFont::ALL.len(),
            Self::Language => Language::ALL.len(),
        }
    }
    fn action(self, index: usize) -> Action {
        match self {
            Self::Font => Action::SetFont(UiFont::ALL[index]),
            Self::Language => Action::SetLanguage(Language::ALL[index]),
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BrowserRow {
    Items,
    Categories,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ScrollSurface {
    Settings,
    ToolProperties,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PanelKind {
    Toolbar,
    Chooser,
}
#[derive(Clone, Copy)]
enum Drag {
    None,
    Panel {
        kind: PanelKind,
        start_x: f32,
        start_y: f32,
        initial: Option<(f32, f32)>,
        origin: (f32, f32),
        moved: bool,
    },
    Stroke,
    Pan {
        x: f32,
        y: f32,
        ox: f32,
        oy: f32,
    },
    Shape {
        x: f32,
        y: f32,
        ex: f32,
        ey: f32,
    },
    Browser {
        family: ToolPanel,
        row: BrowserRow,
        start_x: f32,
        start_y: f32,
        initial: f32,
        moved: bool,
        pending: Option<Action>,
    },
    VerticalScroll {
        surface: ScrollSurface,
        initial: f32,
        grab: f32,
    },
    Slider {
        action: Action,
        rect: Rect,
        initial: f32,
    },
    Color {
        action: Action,
        rect: Rect,
        initial: Color,
        initial_h: f32,
    },
}
#[derive(Clone, Copy)]
enum Pending {
    None,
    New,
    Open,
    Demo,
    Quit,
}
enum IoMessage {
    Saved {
        path: PathBuf,
        serial: u64,
        result: Result<(), String>,
    },
    Exported(Result<(), String>),
    Loaded {
        path: PathBuf,
        result: Result<Document, String>,
    },
    Imported {
        path: PathBuf,
        result: Result<platform::ImportedImage, String>,
    },
    Recovered(Result<Document, String>),
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct CanvasRasterKey {
    size: (u32, u32),
    viewport: [u32; 5],
    document_size: (u32, u32),
    dark: bool,
    grayscale: bool,
}
struct CanvasRaster {
    key: CanvasRasterKey,
    pixels: Vec<u32>,
}

pub struct PortfolioApp {
    pub doc: Document,
    pub dark: bool,
    ui_font: UiFont,
    accent: Accent,
    language: Language,
    font_size: u16,
    preferences_dirty: bool,
    animations: bool,
    system_reduced_motion: bool,
    motion_runtime: bool,
    chrome_motion: Reveal,
    popup_motion: Reveal,
    browser_search_motion: Reveal,
    overlay_motion: Reveal,
    choice_motion: Reveal,
    closing_overlay: Option<Overlay>,
    closing_popup: Option<ToolPanel>,
    closing_choice: Option<SettingsChoice>,
    motion_hit_clip: Option<Rect>,
    motion_preview: Option<f32>,
    settings_ui: SettingsUi,
    tool_properties_ui: ToolPropertiesUi,
    settings_choice: Option<SettingsChoice>,
    choice_index: usize,
    inspector_layers: bool,
    browser: Browser,
    browser_search_focus: bool,
    presets: Presets,
    brush_samples: std::collections::HashMap<(bool, u8, bool), (ToolSettings, Vec<Color>)>,
    brush_preview: Option<(ToolSettings, u32, bool, Vec<Color>)>,
    quick_preview: Option<(Action, Instant)>,
    quick_preview_painted: Option<PreviewHit>,
    quick_disc: Option<(u32, Vec<Color>)>,
    pub tool: Tool,
    brush: Brush,
    text_style: TextLayer,
    tool_popup: Option<ToolPanel>,
    tool_panel_pinned: bool,
    toolbar_position: Option<(f32, f32)>,
    chooser_position: Option<(f32, f32)>,
    tool_row_relayout_guard: Option<Rect>,
    shape_filled: bool,
    shape_settings: ToolSettings,
    last_shape: Tool,
    color_h: f32,
    color_s: f32,
    color_v: f32,
    recent: Vec<Color>,
    width: u32,
    height: u32,
    scale: f32,
    view_x: f32,
    view_y: f32,
    zoom: f32,
    initialized: bool,
    pub focused_mode: bool,
    grayscale: bool,
    grid: bool,
    controls: Vec<Control>,
    hit_clip: Option<Rect>,
    focus: Option<usize>,
    pending_focus: Option<Action>,
    pending_properties_focus: bool,
    hover: Option<usize>,
    pointer: (f32, f32),
    space: bool,
    alt: bool,
    overlay: Overlay,
    menu_index: usize,
    drag: Drag,
    layer_scroll: usize,
    panel_scroll: f32,
    status: String,
    status_until: Instant,
    path: Option<PathBuf>,
    unsaved_origin: bool,
    pending: Pending,
    file_is_save: bool,
    new_width: u32,
    new_height: u32,
    serial: u64,
    last_edit: Instant,
    last_auto: Instant,
    io: Option<Receiver<IoMessage>>,
    auto_io: Option<Receiver<(u64, Result<(), String>)>>,
    document_id: u64,
    import_queue: VecDeque<PathBuf>,
    importing: bool,
    import_failures: Vec<String>,
    import_count: usize,
    auto_revision: Option<(u64, u64)>,
    last_auto_saved: Option<(u64, u64)>,
    recovery_requested: bool,
    preserve_recovery: bool,
    busy: bool,
    prefs: Option<PathBuf>,
    recovery: Option<PathBuf>,
    recover_available: bool,
    first_tick: bool,
    composite: Vec<Color>,
    composite_dirty: bool,
    canvas_raster: Option<CanvasRaster>,
    title_dirty: bool,
}

impl PortfolioApp {
    fn activate(&mut self, a: Action, out: &mut Vec<Effect>) {
        let keyboard = self.focus.is_some() || self.pending_focus.is_some();
        let properties_keyboard = keyboard && self.tool_properties_has_focus();
        self.pending_properties_focus = properties_keyboard;
        self.finish_drag();
        if !matches!(a, Action::BrowserSearch | Action::ClearBrowserSearch) {
            self.browser_search_focus = false;
        }
        if !matches!(a, Action::SettingsSearch | Action::ClearSettingsSearch) {
            self.settings_ui.focused = false;
        }
        self.focus = None;
        if self.busy
            && !matches!(
                a,
                Action::Theme
                    | Action::ToolProperties
                    | Action::CloseToolProperties
                    | Action::Settings
                    | Action::CloseSettings
                    | Action::SettingsSearch
                    | Action::ClearSettingsSearch
                    | Action::SettingsScroll
                    | Action::ToolPropertiesScroll
                    | Action::ToolSection(_)
                    | Action::MoveToolbar
                    | Action::ResetPanelLayout
                    | Action::FontMenu
                    | Action::LanguageMenu
                    | Action::SetTheme(_)
                    | Action::SetFont(_)
                    | Action::SetAccent(_)
                    | Action::SettingsSection(_)
                    | Action::SetLanguage(_)
                    | Action::SetAnimations(_)
                    | Action::FontSize
                    | Action::Inspector(_)
                    | Action::ResetAppearance
                    | Action::Help
                    | Action::Focus
                    | Action::Fit
                    | Action::Actual
                    | Action::ZoomIn
                    | Action::ZoomOut
                    | Action::Cancel
                    | Action::PinToolPanel
                    | Action::BrowserSearch
                    | Action::CloseBrowserSearch
                    | Action::ClearBrowserSearch
                    | Action::BrowserCategory(_)
            )
        {
            self.feedback("Дождитесь окончания записи.");
            return;
        }
        if self.overlay == Overlay::File && a != Action::File {
            self.overlay = Overlay::None;
            self.hover = None;
        }
        if properties_keyboard && a.is_brush_parameter() {
            self.pending_focus = Some(a);
        }
        let keeps_chooser = self.tool_settings_visible()
            && !matches!(
                a,
                Action::Settings | Action::File | Action::Help | Action::Focus
            );
        if !keeps_chooser
            && !matches!(
                a,
                Action::ToolPanel(_)
                    | Action::ToolProperties
                    | Action::CloseToolProperties
                    | Action::ToolSection(_)
                    | Action::MoveToolbar
                    | Action::ResetPanelLayout
                    | Action::PinToolPanel
                    | Action::Shape(_)
                    | Action::Brush(_)
                    | Action::BrowserSearch
                    | Action::CloseBrowserSearch
                    | Action::ClearBrowserSearch
                    | Action::BrowserCategory(_)
                    | Action::Size
                    | Action::Opacity
            )
        {
            self.close_tool_popup();
        }
        match a {
            Action::NoOp => {}
            Action::File => {
                self.overlay = if self.overlay == Overlay::File {
                    Overlay::None
                } else {
                    Overlay::File
                };
                self.menu_index = 0;
                self.hover = None;
            }
            Action::New => self.request_replace(Pending::New, out),
            Action::Open => self.request_replace(Pending::Open, out),
            Action::Demo => self.request_replace(Pending::Demo, out),
            Action::Import => out.push(Effect::OpenFile(FileKind::Image)),
            Action::Save => self.save(false, out),
            Action::SaveAs => self.save(true, out),
            Action::Export => out.push(Effect::SaveFile {
                kind: FileKind::Png,
                suggested_name: format!("{}.png", self.doc.title),
            }),
            Action::Undo => {
                if self.doc.undo() {
                    self.changed();
                    self.feedback("Действие отменено");
                }
            }
            Action::Redo => {
                if self.doc.redo() {
                    self.changed();
                    self.feedback("Действие повторено");
                }
            }
            Action::Theme => {
                self.dark = !self.dark;
                self.persist_appearance(out);
                out.push(Effect::SetDarkMode(self.dark));
                self.feedback(if self.dark {
                    "Тема «Графит»"
                } else {
                    "Тема «Бумага»"
                });
            }
            Action::Settings => {
                if self.overlay == Overlay::Settings {
                    self.close_settings();
                } else {
                    self.overlay = Overlay::Settings;
                    self.controls.clear();
                    self.hover = None;
                    if keyboard {
                        self.pending_focus = Some(self.settings_focus_action());
                    }
                }
            }
            Action::CloseSettings => self.close_settings(),
            Action::SettingsSearch => {
                if self.overlay == Overlay::Settings {
                    self.close_settings_choice();
                    self.settings_ui.focused = true;
                    self.settings_ui.query.end(false);
                    self.pending_focus = None;
                }
            }
            Action::ClearSettingsSearch => {
                self.settings_ui.query.clear();
                self.settings_query_changed();
                self.settings_ui.focused = true;
            }
            Action::SettingsScroll | Action::ToolPropertiesScroll => {}
            Action::FontMenu => self.toggle_settings_choice(SettingsChoice::Font),
            Action::LanguageMenu => self.toggle_settings_choice(SettingsChoice::Language),
            Action::SettingsSection(section) => {
                self.settings_choice = None;
                self.closing_choice = None;
                self.choice_motion.snap(false);
                self.settings_reveal_section(section.min(2));
                if keyboard {
                    self.pending_focus = Some(self.settings_focus_action());
                }
            }
            Action::SetLanguage(language) => {
                self.language = language;
                let dropdown = self.settings_choice.is_some();
                self.close_settings_choice();
                self.persist_appearance(out);
                if keyboard || dropdown {
                    self.pending_focus = Some(Action::LanguageMenu);
                }
            }
            Action::SetAnimations(value) => {
                self.animations = value;
                self.persist_appearance(out);
                if keyboard {
                    self.pending_focus = Some(a);
                }
            }
            Action::Inspector(layers) => {
                self.inspector_layers = layers;
                self.focused_mode = false;
                self.panel_scroll = 0.;
                self.layer_scroll = 0;
                self.controls.clear();
            }
            Action::BrushDetail(detail) => {
                self.tool_settings_reveal_action(Action::BrushDetail(detail));
                if keyboard {
                    self.pending_focus = Some(if detail {
                        Action::Spacing
                    } else {
                        Action::Size
                    });
                }
            }
            Action::BrowserSearch => {
                if let Some(family) = self.tool_popup {
                    if !self.browser.view(family).search_open {
                        self.browser.view_mut(family).search_open = true;
                        self.browser_search_motion.snap(false);
                        self.browser_search_motion.set(true);
                        self.controls.clear();
                        self.hover = None;
                        self.pending_focus = None;
                    }
                    self.browser_search_focus = true;
                    self.browser.view_mut(family).query.end(false);
                }
            }
            Action::ToolSection(section) => {
                self.tool_section(section);
                self.pending_properties_focus = true;
                if keyboard {
                    self.pending_focus = Some(Action::ToolSection(section));
                }
            }
            Action::MoveToolbar => {
                self.pending_properties_focus = false;
                self.pending_focus = Some(Action::MoveToolbar);
                self.feedback("Переместить панель · стрелки / Home");
            }
            Action::ResetPanelLayout => {
                self.reset_panel_positions();
                self.feedback("Панели возвращены на место");
            }
            Action::CloseBrowserSearch => {
                if let Some(family) = self.tool_popup {
                    self.browser.view_mut(family).search_open = false;
                    self.browser.view_mut(family).query.clear();
                    self.browser_query_changed(family);
                    self.browser_search_motion.snap(false);
                    self.browser_search_focus = false;
                    self.pending_focus = Some(Action::BrowserSearch);
                }
            }
            Action::ClearBrowserSearch => {
                if let Some(family) = self.tool_popup {
                    self.browser.view_mut(family).query.clear();
                    self.browser_query_changed(family);
                    self.browser_search_focus = true;
                }
            }
            Action::BrowserCategory(category) => {
                if let Some(family) = self.tool_popup {
                    self.browser.set_category(family, category);
                    self.controls.clear();
                    self.hover = None;
                    if keyboard {
                        self.pending_focus = Some(Action::BrowserCategory(category));
                    }
                }
            }
            Action::AddBrush | Action::RenameBrush => self.edit_preset_name(a == Action::AddBrush),
            Action::ResetBrush => {
                ToolSettings::preset(self.brush.kind, self.tool == Tool::Eraser)
                    .apply(&mut self.brush);
            }
            Action::FontSize
            | Action::Hardness
            | Action::Flow
            | Action::Spacing
            | Action::Angle
            | Action::Roundness
            | Action::Grain
            | Action::Scatter => {
                let (min, max, label, id, _) = self.parameter_info(a);
                out.push(Self::prompt(
                    id,
                    label,
                    &format!("Введите число от {min:.0} до {max:.0}."),
                    format!("{:.0}", self.parameter_value(a)),
                ));
            }
            Action::SetTheme(dark) => {
                self.dark = dark;
                out.push(Effect::SetDarkMode(dark));
                self.persist_appearance(out);
                if keyboard {
                    self.pending_focus = Some(Action::SetTheme(dark));
                }
            }
            Action::SetFont(font) => {
                self.ui_font = font;
                let dropdown = self.settings_choice.is_some();
                self.close_settings_choice();
                self.persist_appearance(out);
                if keyboard || dropdown {
                    self.pending_focus = Some(Action::FontMenu);
                }
            }
            Action::SetAccent(accent) => {
                self.accent = accent;
                self.persist_appearance(out);
                if keyboard {
                    self.pending_focus = Some(Action::SetAccent(accent));
                }
            }
            Action::ResetAppearance => {
                let defaults = Appearance::default();
                self.dark = defaults.dark;
                self.ui_font = defaults.font;
                self.accent = defaults.accent;
                self.font_size = defaults.font_size;
                self.animations = defaults.animations;
                out.push(Effect::SetDarkMode(self.dark));
                self.persist_appearance(out);
                if keyboard {
                    self.pending_focus = Some(Action::ResetAppearance);
                }
            }
            Action::Help => {
                self.overlay = Overlay::Help;
                self.hover = None;
            }
            Action::Focus => {
                self.focused_mode = !self.focused_mode;
                self.controls.clear();
                self.hover = None;
                self.feedback("Tab — вернуть / скрыть панели");
            }
            Action::Fit => self.fit(),
            Action::Actual => {
                let s = self.stage();
                self.zoom_at(1. / self.zoom, s.x + s.w / 2., s.y + s.h / 2.);
            }
            Action::ZoomIn | Action::ZoomOut => {
                let s = self.stage();
                self.zoom_at(
                    if a == Action::ZoomIn { 1.2 } else { 1. / 1.2 },
                    s.x + s.w / 2.,
                    s.y + s.h / 2.,
                );
            }
            Action::Tool(tool) => {
                self.switch_tool(tool);
                if tool == Tool::Text {
                    self.feedback("Щёлкните по холсту для надписи или по буквам для правки.");
                } else {
                    self.feedback(format!("{} · {}", tool.info().1, tool.info().2));
                }
            }
            Action::ToolProperties => {
                if self.tool == Tool::Text {
                    if !self.busy {
                        let target = self
                            .doc
                            .active_layer()
                            .text
                            .as_ref()
                            .map(|_| self.doc.active_layer().id);
                        self.open_canvas_text(target, self.doc.width / 2, self.doc.height / 2, out);
                    }
                    return;
                }
                if self.tool_has_properties() {
                    if self.tool_settings_visible() {
                        self.close_tool_settings();
                    } else {
                        self.overlay = Overlay::ToolProperties;
                        self.focused_mode = false;
                        self.controls.clear();
                        self.hover = None;
                        self.pending_focus = if keyboard {
                            self.pending_properties_focus = true;
                            Some(if self.tool == Tool::Fill {
                                Action::Opacity
                            } else {
                                Action::Size
                            })
                        } else {
                            None
                        };
                    }
                }
            }
            Action::CloseToolProperties => self.close_tool_settings(),
            Action::PinToolPanel => {
                self.tool_row_relayout_guard = self.current_tool_row_rect();
                self.tool_panel_pinned = !self.tool_panel_pinned;
                self.closing_popup = None;
                self.popup_motion.snap(true);
                self.quick_preview = None;
                self.quick_preview_painted = None;
                self.controls.clear();
                self.hover = None;
                if keyboard {
                    self.pending_focus = Some(Action::PinToolPanel);
                }
            }
            Action::ToolPanel(family) => {
                let was_open = self.tool_popup == Some(family) && !self.tool_panel_pinned;
                let tool = match family {
                    ToolPanel::Brush => Tool::Brush,
                    ToolPanel::Eraser => Tool::Eraser,
                    ToolPanel::Shapes => self.last_shape,
                };
                self.switch_tool(tool);
                if !was_open {
                    self.panel_scroll = 0.;
                }
                self.tool_popup = if was_open { None } else { Some(family) };
                self.controls.clear();
                self.hover = None;
                if keyboard {
                    self.pending_focus = Some(if was_open {
                        Action::ToolPanel(family)
                    } else {
                        self.browser_initial_focus(family)
                    });
                }
            }
            Action::Shape(tool) => {
                if matches!(tool, Tool::Line | Tool::Rectangle | Tool::Ellipse) {
                    self.switch_tool(tool);
                    self.tool_popup = Some(ToolPanel::Shapes);
                    if keyboard {
                        self.pending_focus = Some(Action::Shape(tool));
                    }
                }
            }
            Action::ShapeFill => {
                if matches!(self.tool, Tool::Rectangle | Tool::Ellipse) {
                    self.shape_filled = !self.shape_filled;
                    if keyboard {
                        self.pending_focus = Some(Action::ShapeFill);
                    }
                }
            }
            Action::Mirror => {
                self.brush.mirror = !self.brush.mirror;
                self.feedback(if self.brush.mirror {
                    "Зеркало: штрих отражается по центру холста"
                } else {
                    "Зеркало выключено"
                });
            }
            Action::Value => {
                self.grayscale = !self.grayscale;
                self.feedback(if self.grayscale {
                    "Просмотр тона. Исходные цвета и PNG остаются цветными."
                } else {
                    "Цветной просмотр"
                });
            }
            Action::Grid => self.grid = !self.grid,
            Action::Brush(kind) => {
                self.choose_preset(kind);
                if keyboard {
                    self.pending_focus = Some(Action::Brush(kind));
                }
            }
            Action::Size => out.push(Self::prompt(
                11,
                match self.tool {
                    Tool::Eraser => "Размер ластика",
                    Tool::Line | Tool::Rectangle | Tool::Ellipse => "Толщина контура",
                    _ => "Размер кисти",
                },
                "Размер в пикселях: от 1 до 512",
                format!("{:.0}", self.brush.size),
            )),
            Action::Opacity => out.push(Self::prompt(
                12,
                if self.tool == Tool::Eraser {
                    "Сила ластика"
                } else {
                    "Непрозрачность"
                },
                "Проценты: от 0 до 100",
                format!("{:.0}", self.brush.opacity * 100.),
            )),
            Action::LayerOpacity => out.push(Self::prompt(
                13,
                "Непрозрачность слоя",
                "Проценты: от 0 до 100",
                format!(
                    "{:.0}",
                    self.doc.layers[self.doc.active].opacity as f32 / 255. * 100.
                ),
            )),
            Action::Hex => out.push(Self::prompt(
                10,
                "Точный цвет",
                "Шесть символов HEX, например #365AD7",
                hex(self.brush.color),
            )),
            Action::SV => self.feedback("Стрелки: насыщенность и яркость. HEX — точный цвет."),
            Action::Hue => self.feedback("Стрелки: оттенок. HEX — точный цвет."),
            Action::Swatch(i) => {
                if let Some(c) = PALETTE.get(i) {
                    self.update_color(Color::from_hex(*c));
                }
            }
            Action::Recent(i) => {
                if let Some(c) = self.recent.get(i) {
                    self.update_color(*c);
                }
            }
            Action::AddLayer => match self
                .doc
                .add_layer(&format!("Слой {}", self.doc.layers.len() + 1))
            {
                Ok(_) => {
                    self.changed();
                    self.layer_scroll = 0;
                }
                Err(e) => self.error(e, out),
            },
            Action::EditTextLayer(id) => self.open_canvas_text(Some(id), 0, 0, out),
            Action::Layer(i) => {
                if i < self.doc.layers.len() {
                    self.doc.active = i;
                    self.feedback(self.doc.layers[i].name.clone());
                }
            }
            Action::Visible(i) => {
                if let Some(layer) = self.doc.layers.get(i) {
                    let value = !layer.visible;
                    if self.doc.set_layer_visibility(i, value) {
                        self.changed();
                    }
                }
            }
            Action::Lock(i) => {
                if let Some(layer) = self.doc.layers.get(i) {
                    let value = !layer.locked;
                    if self.doc.set_layer_locked(i, value) {
                        self.changed();
                    }
                }
            }
            Action::Rename => out.push(Self::prompt(
                14,
                "Имя слоя",
                "Название слоя",
                self.doc.layers[self.doc.active].name.clone(),
            )),
            Action::Duplicate => match self.doc.duplicate_layer() {
                Ok(_) => {
                    self.changed();
                    self.layer_scroll = 0;
                }
                Err(e) => self.error(e, out),
            },
            Action::DeleteLayer => match self.doc.delete_layer() {
                Ok(()) => {
                    self.changed();
                    self.feedback("Слой удалён. Ctrl+Z — вернуть.");
                }
                Err(e) => self.error(e, out),
            },
            Action::Raise | Action::Lower => {
                if self.doc.move_layer(if a == Action::Raise { 1 } else { -1 }) {
                    self.changed();
                }
            }
            Action::NewPreset(i) => {
                let (w, h) = match i {
                    1 => (1080, 1080),
                    2 => (1200, 1600),
                    _ => (1200, 840),
                };
                self.new_width = w;
                self.new_height = h;
            }
            Action::NewWidth => out.push(Self::prompt(
                15,
                "Ширина холста",
                "Пиксели: от 1 до 4096",
                self.new_width.to_string(),
            )),
            Action::NewHeight => out.push(Self::prompt(
                16,
                "Высота холста",
                "Пиксели: от 1 до 4096",
                self.new_height.to_string(),
            )),
            Action::Create => match Document::with_background(
                self.new_width,
                self.new_height,
                Color::from_hex(0xfaf8f0),
            ) {
                Ok(mut doc) => {
                    doc.title = "Новый этюд".into();
                    doc.clear_history();
                    doc.mark_saved();
                    self.reset_document(doc, None, false);
                    self.clear_recovery();
                    self.feedback("Новый холст готов.");
                }
                Err(e) => self.error(e, out),
            },
            Action::Cancel => {
                self.overlay = Overlay::None;
                self.hover = None;
            }
        }
    }
    fn open_canvas_text(&mut self, target: Option<u64>, x: u32, y: u32, out: &mut Vec<Effect>) {
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
            TextLayer {
                body: String::new(),
                x,
                y,
                color: self.brush.color,
                ..self.text_style.clone()
            }
        };
        out.push(Effect::EditTextLayer {
            document: self.document_id,
            target,
            width: self.doc.width,
            height: self.doc.height,
            text,
        });
    }

    fn accept_prompt(&mut self, id: u32, value: Option<String>, out: &mut Vec<Effect>) {
        let Some(value) = value else {
            return;
        };
        match id {
            10 => {
                if let Some(c) = parse_hex(&value) {
                    self.update_color(c);
                } else {
                    self.error("Введите HEX в формате #RRGGBB. Например: #365AD7", out);
                }
            }
            11 | 12 | 13 => {
                let n = value.trim().replace(',', ".").parse::<f32>();
                let (min, max) = if id == 11 { (1., 512.) } else { (0., 100.) };
                match n {
                    Ok(n) if n.is_finite() && n >= min && n <= max => {
                        if id == 11 {
                            self.brush.size = n;
                            self.show_quick_preview(Action::Size);
                        } else if id == 12 {
                            self.brush.opacity = n / 100.;
                            self.show_quick_preview(Action::Opacity);
                        } else if self
                            .doc
                            .set_layer_opacity(self.doc.active, (n * 2.55).round() as u8)
                        {
                            self.changed();
                        }
                    }
                    _ => self.error(format!("Введите число от {min:.0} до {max:.0}."), out),
                }
            }
            17..=24 => {
                let action = match id {
                    17 => Action::Hardness,
                    18 => Action::Flow,
                    19 => Action::Spacing,
                    20 => Action::Angle,
                    21 => Action::Scatter,
                    23 => Action::Roundness,
                    24 => Action::Grain,
                    _ => Action::FontSize,
                };
                let (min, max, _, _, _) = self.parameter_info(action);
                match value.trim().replace(',', ".").parse::<f32>() {
                    Ok(v) if v.is_finite() && v >= min && v <= max => self.set_parameter(action, v),
                    _ => self.error(format!("Введите число от {min:.0} до {max:.0}."), out),
                }
            }
            14 => match self.doc.rename_layer(self.doc.active, value.trim()) {
                Ok(()) => self.changed(),
                Err(e) => self.error(e, out),
            },
            15 | 16 => match value.trim().parse::<u32>() {
                Ok(n) if (1..=4096).contains(&n) => {
                    if id == 15 {
                        self.new_width = n
                    } else {
                        self.new_height = n
                    }
                }
                _ => self.error("Размер стороны должен быть от 1 до 4096 пикселей.", out),
            },
            _ => {}
        }
    }
    fn finish_drag(&mut self) {
        self.quick_preview = None;
        let d = std::mem::replace(&mut self.drag, Drag::None);
        match d {
            Drag::Stroke => {
                if self.doc.end_stroke() {
                    self.changed_raster();
                    if self.tool != Tool::Eraser {
                        self.remember();
                    }
                }
            }
            Drag::Shape { x, y, ex, ey } => {
                // The last pointer event can arrive before another frame. Bring
                // the live raster up to date once, then commit those same pixels.
                self.preview_shape_at(x, y, ex, ey);
                if self.doc.commit_shape_preview() {
                    self.changed_raster();
                    self.remember();
                }
            }
            Drag::Slider {
                action: Action::LayerOpacity,
                ..
            } => {
                if self.doc.commit_edit() {
                    self.changed();
                }
            }
            _ => {}
        }
    }
    fn cancel_drag(&mut self) {
        self.quick_preview = None;
        let d = std::mem::replace(&mut self.drag, Drag::None);
        match d {
            Drag::Stroke => {
                self.doc.cancel_stroke();
                self.composite_dirty = true;
            }
            Drag::Shape { .. } => {
                self.doc.cancel_shape_preview();
            }
            Drag::Browser {
                family,
                row,
                initial,
                ..
            } => {
                self.browser_scroll_mut(family, row).set(initial);
                self.controls.clear();
                self.hover = None;
            }
            Drag::Panel { kind, initial, .. } => {
                self.set_panel_position(kind, initial);
            }
            Drag::VerticalScroll {
                surface, initial, ..
            } => {
                self.set_vertical_scroll(surface, initial);
            }
            Drag::Slider {
                action: Action::LayerOpacity,
                ..
            } => {
                self.doc.cancel_edit();
                self.composite_dirty = true;
            }
            Drag::Slider {
                action, initial, ..
            } => self.set_parameter(action, initial),
            Drag::Color {
                initial, initial_h, ..
            } => {
                self.color_h = initial_h;
                self.update_color(initial);
            }
            Drag::Pan { ox, oy, .. } => {
                self.view_x = ox;
                self.view_y = oy;
            }
            _ => {}
        }
    }
    fn update_drag(&mut self, x: f32, y: f32, modifiers: Modifiers) {
        if self.canvas_obscured(x, y) && matches!(self.drag, Drag::Stroke | Drag::Shape { .. }) {
            self.finish_drag();
            return;
        }
        match self.drag {
            Drag::Stroke => {
                let (dx, dy) = self.doc_point(x, y);
                self.doc.continue_stroke(dx, dy);
            }
            Drag::Pan {
                x: ax,
                y: ay,
                ox,
                oy,
            } => {
                self.view_x = ox + x - ax;
                self.view_y = oy + y - ay;
            }
            Drag::Shape { x: ax, y: ay, .. } => {
                let (mut ex, mut ey) = self.doc_point(x, y);
                if modifiers.shift {
                    let (dx, dy) = (ex - ax, ey - ay);
                    if self.tool == Tool::Line {
                        let a = (dy.atan2(dx) / (std::f32::consts::PI / 4.)).round()
                            * (std::f32::consts::PI / 4.);
                        let d = dx.hypot(dy);
                        ex = ax + a.cos() * d;
                        ey = ay + a.sin() * d;
                    } else {
                        let d = dx.abs().max(dy.abs());
                        ex = ax + d * dx.signum();
                        ey = ay + d * dy.signum();
                    }
                }
                self.drag = Drag::Shape {
                    x: ax,
                    y: ay,
                    ex,
                    ey,
                };
                // Rasterize the newest geometry once per frame, rather than
                // blocking on every queued high-frequency pointer event.
            }
            Drag::Browser {
                family,
                row,
                start_x,
                start_y,
                initial,
                moved,
                pending,
            } => {
                let moved = moved || (x - start_x).hypot(y - start_y) > 5.;
                if moved {
                    self.browser_scroll_mut(family, row)
                        .set(initial + start_x - x);
                    self.controls.clear();
                    self.hover = None;
                    self.focus = None;
                }
                self.drag = Drag::Browser {
                    family,
                    row,
                    start_x,
                    start_y,
                    initial,
                    moved,
                    pending,
                };
            }
            Drag::Panel {
                kind,
                start_x,
                start_y,
                initial,
                origin,
                moved,
            } => {
                let moved = moved || (x - start_x).hypot(y - start_y) > 4.;
                if moved {
                    self.set_panel_position(
                        kind,
                        Some((origin.0 + x - start_x, origin.1 + y - start_y)),
                    );
                }
                self.drag = Drag::Panel {
                    kind,
                    start_x,
                    start_y,
                    initial,
                    origin,
                    moved,
                };
            }
            Drag::VerticalScroll { surface, grab, .. } => {
                self.prepare_vertical_scroll(surface);
                let track = self.vertical_scroll_track(surface);
                let thumb = self.vertical_scroll_thumb(surface);
                let maximum = self.vertical_scroll_state(surface).max();
                let travel = (track.h - thumb.h).max(0.);
                let offset = if travel > 0. {
                    (y - track.y - grab).clamp(0., travel) / travel * maximum
                } else {
                    0.
                };
                self.set_vertical_scroll(surface, offset);
            }
            Drag::Slider { action, rect, .. } => {
                let v = ((x - rect.x - 5.) / (rect.w - 10.)).clamp(0., 1.);
                match action {
                    a if a.is_parameter() => {
                        let (min, max, _, _, _) = self.parameter_info(a);
                        self.set_parameter(a, (min + v * (max - min)).round());
                    }
                    Action::LayerOpacity => {
                        self.doc
                            .set_layer_opacity(self.doc.active, (v * 255.).round() as u8);
                        self.composite_dirty = true;
                    }
                    _ => {}
                }
            }
            Drag::Color { action, rect, .. } => {
                if action == Action::SV {
                    self.color_s = ((x - rect.x) / rect.w).clamp(0., 1.);
                    self.color_v = (1. - (y - rect.y) / rect.h).clamp(0., 1.);
                } else {
                    self.color_h = (((x - rect.x) / rect.w).clamp(0., 1.) * 360.).rem_euclid(360.);
                }
                self.brush.color = hsv(self.color_h, self.color_s, self.color_v);
            }
            Drag::None => {}
        }
    }
    fn mouse_down(
        &mut self,
        x: f32,
        y: f32,
        button: MouseButton,
        modifiers: Modifiers,
        out: &mut Vec<Effect>,
    ) {
        self.pointer = (x, y);
        if self.tool_properties_ui.name_edit.is_some() {
            if self.preset_name_rect().contains(x, y) {
                if button == MouseButton::Left {
                    self.place_name_caret(x, modifiers.shift);
                }
                return;
            }
            let accepting = self
                .controls
                .iter()
                .any(|c| c.action == Action::RenameBrush && c.enabled && c.hit.contains(x, y));
            self.finish_preset_name(true);
            if accepting {
                return;
            }
            if !self.tool_settings_rect().contains(x, y) && !self.tool_popup_contains(x, y) {
                return;
            }
        }
        if self
            .tool_row_relayout_guard
            .is_some_and(|r| r.contains(x, y))
        {
            return;
        }
        if matches!(
            self.drag,
            Drag::Slider { .. }
                | Drag::Browser { .. }
                | Drag::VerticalScroll { .. }
                | Drag::Panel { .. }
        ) && button != MouseButton::Left
        {
            return;
        }
        // Test the visible feedback before clearing it; its dismissing click
        // must not start a stroke on the canvas underneath.
        if self.quick_preview_contains(x, y) {
            self.finish_drag();
            return;
        }
        if self.closing_overlay.is_some()
            || self.closing_popup.is_some()
            || self.closing_choice.is_some()
        {
            let only_modeless_ghost = self.closing_overlay == Some(Overlay::ToolProperties)
                && self.closing_popup.is_none()
                && self.closing_choice.is_none();
            let blocked_by_ghost = self.closing_surface_contains(x, y);
            let visible_control = self
                .controls
                .iter()
                .rev()
                .find(|c| c.hit.contains(x, y))
                .is_some_and(|c| c.enabled && c.action != Action::NoOp);
            self.closing_overlay = None;
            self.closing_popup = None;
            self.closing_choice = None;
            self.overlay_motion.snap(self.overlay != Overlay::None);
            self.popup_motion.snap(self.tool_popup.is_some());
            self.choice_motion.snap(self.settings_choice.is_some());
            if blocked_by_ghost || (!visible_control && !only_modeless_ghost) {
                return;
            }
        }
        if let Some(choice) = self.settings_choice {
            // A popup owns input above its trigger, even if a constrained
            // viewport forces the menu to overlap that trigger.
            if !self.choice_menu_rect(choice).contains(x, y) {
                self.close_settings_choice();
                return;
            }
        }
        self.focus = None;
        self.pending_focus = None;
        self.finish_drag();
        let control = self
            .controls
            .iter()
            .rev()
            .find(|c| c.hit.contains(x, y))
            .cloned();
        if !self.tool_panel_pinned
            && !self.tool_settings_visible()
            && self.tool_popup.is_some()
            && !self.tool_popup_contains(x, y)
        {
            // A click outside dismisses the popup without leaving a paint mark.
            // Other toolbar controls remain reachable in the same click.
            let family_button = control.as_ref().is_some_and(|c| {
                c.enabled
                    && matches!(
                        c.action,
                        Action::ToolPanel(_) | Action::Size | Action::Opacity | Action::MoveToolbar
                    )
            });
            if !family_button {
                self.close_tool_popup();
                if !control
                    .as_ref()
                    .is_some_and(|c| c.enabled && c.action != Action::NoOp)
                {
                    return;
                }
            }
        }
        if let Some(family) = self
            .tool_popup
            .filter(|_| !self.tool_settings_visible() || !self.tool_settings_rect().contains(x, y))
        {
            let visible = self.revealed_bounds(self.tool_popup_bounds(family), 1);
            let row = if self.browser_items_rect(family).contains(x, y) {
                Some(BrowserRow::Items)
            } else if !self.browser.view(family).search_open
                && self.browser_categories_rect(family).contains(x, y)
            {
                Some(BrowserRow::Categories)
            } else {
                None
            };
            if visible.contains(x, y) {
                if let Some(row) = row {
                    if button == MouseButton::Left {
                        self.prepare_browser_layout(family);
                        self.browser_search_focus = false;
                        let pending = control.as_ref().filter(|c| c.enabled).and_then(|c| {
                            match (row, c.action) {
                                (BrowserRow::Items, a @ (Action::Brush(_) | Action::Shape(_)))
                                | (BrowserRow::Categories, a @ Action::BrowserCategory(_)) => {
                                    Some(a)
                                }
                                _ => None,
                            }
                        });
                        let initial = self.browser_scroll_mut(family, row).offset;
                        self.drag = Drag::Browser {
                            family,
                            row,
                            start_x: x,
                            start_y: y,
                            initial,
                            moved: false,
                            pending,
                        };
                    }
                    return;
                }
            }
        }
        if !control
            .as_ref()
            .is_some_and(|c| matches!(c.action, Action::BrowserSearch | Action::ClearBrowserSearch))
        {
            self.browser_search_focus = false;
        }
        if !control.as_ref().is_some_and(|c| {
            matches!(
                c.action,
                Action::SettingsSearch | Action::ClearSettingsSearch
            )
        }) {
            self.settings_ui.focused = false;
        }
        if let Some(c) = control {
            if !c.enabled {
                return;
            }
            let movable = match c.action {
                Action::MoveToolbar => Some(PanelKind::Toolbar),
                Action::PinToolPanel if self.tool_panel_pinned => Some(PanelKind::Chooser),
                _ => None,
            };
            if let Some(kind) = movable {
                if button == MouseButton::Right {
                    self.set_panel_position(kind, None);
                } else if button == MouseButton::Left {
                    self.begin_panel_drag(kind, x, y);
                }
                return;
            }
            if button != MouseButton::Left {
                return;
            }
            match c.action {
                Action::SettingsScroll | Action::ToolPropertiesScroll => {
                    let surface = if c.action == Action::SettingsScroll {
                        ScrollSurface::Settings
                    } else {
                        ScrollSurface::ToolProperties
                    };
                    self.prepare_vertical_scroll(surface);
                    let thumb = self.vertical_scroll_thumb(surface);
                    let initial = self.vertical_scroll_state(surface).offset;
                    let grab = if thumb.contains(x, y) {
                        y - thumb.y
                    } else {
                        thumb.h / 2.
                    };
                    self.drag = Drag::VerticalScroll {
                        surface,
                        initial,
                        grab,
                    };
                    self.update_drag(x, y, modifiers);
                }
                a if (a.is_parameter() || a == Action::LayerOpacity)
                    && c.kind == ControlKind::Slider =>
                {
                    if self.busy && c.action != Action::FontSize {
                        return;
                    }
                    self.pending_focus = Some(c.action);
                    self.pending_properties_focus =
                        self.tool_settings_visible() && self.tool_settings_rect().contains(x, y);
                    let initial = if c.action == Action::LayerOpacity {
                        self.doc.layers[self.doc.active].opacity as f32 / 255.
                    } else {
                        self.parameter_value(c.action)
                    };
                    if c.action == Action::LayerOpacity {
                        self.doc.begin_layer_metadata_edit();
                    }
                    self.drag = Drag::Slider {
                        action: c.action,
                        rect: c.rect,
                        initial,
                    };
                    self.update_drag(x, y, modifiers);
                }
                Action::SV | Action::Hue => {
                    self.drag = Drag::Color {
                        action: c.action,
                        rect: c.rect,
                        initial: self.brush.color,
                        initial_h: self.color_h,
                    };
                    self.update_drag(x, y, modifiers);
                }
                _ => self.activate(c.action, out),
            }
            return;
        }
        if self.tool_settings_visible() && self.tool_settings_rect().contains(x, y) {
            return;
        }
        if !self.editor_interactive() {
            if matches!(self.overlay, Overlay::Settings | Overlay::ToolProperties)
                && self.overlay_bounds(self.overlay).contains(x, y)
            {
                // A search or scroll can invalidate controls before the next
                // frame. The visible panel still owns its entire surface.
                return;
            }
            if self.overlay == Overlay::Settings {
                self.close_settings();
            } else if self.overlay == Overlay::ToolProperties {
                self.close_tool_settings();
            } else {
                self.overlay = Overlay::None;
                self.controls.clear();
                self.hover = None;
            }
            return;
        }
        if !self.stage().contains(x, y) || self.canvas_obscured(x, y) {
            return;
        }
        if button == MouseButton::Middle || self.space || self.tool == Tool::Hand {
            self.drag = Drag::Pan {
                x,
                y,
                ox: self.view_x,
                oy: self.view_y,
            };
            return;
        }
        if self.busy {
            return;
        }
        let (dx, dy) = self.doc_point(x, y);
        if dx < 0. || dy < 0. || dx >= self.doc.width as f32 || dy >= self.doc.height as f32 {
            return;
        }
        if button == MouseButton::Right || modifiers.alt || self.alt || self.tool == Tool::Picker {
            let c = self.doc.sample(dx as u32, dy as u32);
            if c.a > 0 {
                self.update_color(c);
                self.feedback(format!("Цвет взят с холста: {}", hex(c)));
            } else {
                self.feedback("В этой точке прозрачный пиксель.");
            }
            return;
        }
        if button != MouseButton::Left {
            return;
        }
        if self.tool == Tool::Text {
            let (x, y) = (dx as u32, dy as u32);
            let target = if modifiers.shift {
                None
            } else {
                self.doc.text_layer_at(x, y).map(|l| l.id)
            };
            self.open_canvas_text(target, x, y, out);
            return;
        }
        if self.doc.active_layer().text.is_some() {
            self.feedback("Для рисования выберите растровый слой.");
            return;
        }
        if !self.doc.can_paint() {
            self.feedback("Слой скрыт или заблокирован. Выберите доступный слой.");
            return;
        }
        match self.tool {
            Tool::Brush | Tool::Eraser => {
                match self.doc.begin_stroke(dx, dy, self.brush_for_tool()) {
                    Ok(()) => {
                        self.drag = Drag::Stroke;
                    }
                    Err(e) => self.error(e, out),
                }
            }
            Tool::Fill => {
                let mut c = self.brush.color;
                c.a = (self.brush.opacity * 255.).round() as u8;
                match self.doc.flood_fill(dx as u32, dy as u32, c, 24) {
                    Ok(true) => {
                        self.changed_raster();
                        self.remember();
                    }
                    Ok(false) => {}
                    Err(e) => self.error(e, out),
                }
            }
            Tool::Line | Tool::Rectangle | Tool::Ellipse => {
                self.drag = Drag::Shape {
                    x: dx,
                    y: dy,
                    ex: dx,
                    ey: dy,
                };
            }
            _ => {}
        }
    }
    fn key(&mut self, code: u32, pressed: bool, repeat: bool, m: Modifiers, out: &mut Vec<Effect>) {
        if self.preset_name_key(code, pressed, m, out) {
            return;
        }
        if code == 18 {
            self.alt = pressed;
            return;
        }
        if pressed
            && !repeat
            && m.ctrl
            && m.alt
            && matches!(code, 48 | 96)
            && self.editor_interactive()
        {
            self.cancel_drag();
            self.activate(Action::ResetPanelLayout, out);
            return;
        }
        if self.settings_search_key(code, pressed, repeat, m, out) {
            return;
        }
        if self.browser_key(code, pressed, repeat, m, out) {
            return;
        }
        if code == 32
            && self.settings_choice.is_none()
            && (!pressed || (self.focus.is_none() && self.pending_focus.is_none()))
        {
            self.space = pressed;
            if !pressed && matches!(self.drag, Drag::Pan { .. }) {
                self.finish_drag();
            }
            return;
        }
        if !pressed {
            return;
        }
        if let Some(choice) = self.settings_choice {
            match code {
                27 | 115 | 9 => self.close_settings_choice(),
                38 if m.alt => self.close_settings_choice(),
                38 | 37 => self.move_choice(-1),
                40 | 39 => self.move_choice(1),
                36 | 35 => {
                    self.choice_index = if code == 36 { 0 } else { choice.len() - 1 };
                    self.pending_focus = Some(choice.action(self.choice_index));
                    self.hover = None;
                }
                13 | 32 => self.activate(choice.action(self.choice_index), out),
                _ => {}
            }
            return;
        }
        if self.overlay == Overlay::Settings && code == 115 && !repeat {
            if let Some(action @ (Action::FontMenu | Action::LanguageMenu)) = self
                .focus
                .and_then(|i| self.controls.get(i))
                .map(|c| c.action)
            {
                self.activate(action, out);
                return;
            }
        }
        if code == 27 {
            let cancelling_gesture = !matches!(self.drag, Drag::None);
            let family = self.tool_popup;
            let previous_overlay = self.overlay;
            self.cancel_drag();
            if !cancelling_gesture {
                if previous_overlay == Overlay::ToolProperties {
                    self.close_tool_settings();
                    return;
                }
                self.overlay = Overlay::None;
                self.close_tool_popup();
                self.pending_focus = match previous_overlay {
                    Overlay::Settings => Some(Action::Settings),
                    Overlay::ToolProperties => Some(self.tool_properties_return_action()),
                    Overlay::File => Some(Action::File),
                    _ => family.map(Action::ToolPanel),
                };
                self.controls.clear();
            }
            self.focus = None;
            self.hover = None;
            return;
        }
        if m.ctrl
            && code == 188
            && !repeat
            && matches!(
                self.overlay,
                Overlay::None | Overlay::Settings | Overlay::ToolProperties
            )
        {
            self.activate(Action::Settings, out);
            if self.overlay == Overlay::Settings {
                self.pending_focus = Some(self.settings_focus_action());
            }
            return;
        }
        if code == 115
            && !repeat
            && !m.ctrl
            && matches!(
                self.overlay,
                Overlay::None | Overlay::Settings | Overlay::ToolProperties
            )
        {
            self.activate(Action::ToolProperties, out);
            if self.tool_settings_visible() {
                self.pending_properties_focus = true;
                self.pending_focus = Some(if self.tool == Tool::Fill {
                    Action::Opacity
                } else {
                    Action::Size
                });
            } else if self.tool_has_properties() {
                self.pending_focus = Some(self.tool_properties_return_action());
            }
            return;
        }
        if code == 118 && !repeat && !m.ctrl {
            self.activate(Action::Inspector(true), out);
            return;
        }
        if code == 117
            || (code == 9
                && (self.tool_popup.is_some()
                    || matches!(self.overlay, Overlay::Settings | Overlay::ToolProperties)))
        {
            if self.move_panel_focus(m.shift) {
                return;
            }
            // F6 traverses controls; Tab also traverses an open settings dropdown.
            if self.controls.is_empty() {
                return;
            }
            let len = self.controls.len();
            let start = self.focus.unwrap_or(if m.shift { 0 } else { len - 1 });
            let mut next = start;
            for _ in 0..len {
                next = if m.shift {
                    (next + len - 1) % len
                } else {
                    (next + 1) % len
                };
                if self.controls[next].enabled {
                    break;
                }
            }
            self.focus = Some(next);
            self.browser_search_focus = self
                .tool_popup
                .is_some_and(|family| self.browser.view(family).search_open)
                && self.controls[next].action == Action::BrowserSearch;
            if self.browser_search_focus {
                if let Some(family) = self.tool_popup {
                    self.browser.view_mut(family).query.end(false);
                }
            }
            self.feedback(self.controls[next].label.clone());
            return;
        }
        if self.vertical_scroll_key(code, m) {
            return;
        }
        if self.panel_position_key(code, m) {
            return;
        }
        if self.tool_settings_navigation_key(code, m, out) {
            return;
        }
        if !self.editor_interactive() || self.tool_properties_has_focus() {
            if self.tool_properties_has_focus() {
                if !m.ctrl && !m.alt && !repeat {
                    let tool = match code {
                        84 => Some(Tool::Text),
                        66 => Some(Tool::Brush),
                        69 => Some(Tool::Eraser),
                        71 => Some(Tool::Fill),
                        73 => Some(Tool::Picker),
                        72 => Some(Tool::Hand),
                        76 => Some(Tool::Line),
                        82 => Some(Tool::Rectangle),
                        79 => Some(Tool::Ellipse),
                        _ => None,
                    };
                    if let Some(tool) = tool {
                        self.activate(Action::Tool(tool), out);
                        return;
                    }
                }
                if [37, 38, 39, 40].contains(&code) {
                    if let Some(c) = self
                        .focus
                        .and_then(|i| self.controls.get(i))
                        .filter(|c| c.enabled)
                    {
                        let step = if code == 37 || code == 40 { -1. } else { 1. };
                        let step = step * if m.shift { 10. } else { 1. };
                        if !self.busy {
                            let action = c.action;
                            if action.is_brush_parameter() {
                                self.set_parameter(action, self.parameter_value(action) + step);
                            }
                        }
                    }
                    return;
                }
            }
            if self.overlay == Overlay::Settings && [37, 38, 39, 40].contains(&code) {
                let current = self
                    .focus
                    .and_then(|i| self.controls.get(i))
                    .map(|c| c.action);
                let step: isize = if code == 37 || code == 38 { -1 } else { 1 };
                let action = match current {
                    Some(Action::SetTheme(dark)) => Some(Action::SetTheme(!dark)),
                    Some(Action::FontSize) => {
                        self.set_parameter(Action::FontSize, self.font_size as f32 + step as f32);
                        None
                    }
                    Some(Action::SetAnimations(v)) => Some(Action::SetAnimations(!v)),
                    Some(Action::FontMenu) => Some(Action::FontMenu),
                    Some(Action::LanguageMenu) => Some(Action::LanguageMenu),
                    Some(Action::SetAccent(accent)) => {
                        let i = Accent::ALL.iter().position(|a| *a == accent).unwrap_or(0) as isize;
                        Some(Action::SetAccent(
                            Accent::ALL[(i + step).rem_euclid(Accent::ALL.len() as isize) as usize],
                        ))
                    }
                    _ => None,
                };
                if let Some(action) = action {
                    self.activate(action, out);
                }
                return;
            }
            if self.overlay == Overlay::File && (code == 38 || code == 40) {
                self.menu_index = if code == 38 {
                    (self.menu_index + 7) % 8
                } else {
                    (self.menu_index + 1) % 8
                };
                self.focus = Some(self.menu_index);
                return;
            }
            if code == 13 || code == 32 {
                let action = self
                    .focus
                    .and_then(|i| self.controls.get(i))
                    .filter(|c| c.enabled)
                    .map(|c| c.action)
                    .or(self.pending_focus);
                if let Some(action) = action {
                    self.activate(action, out);
                    return;
                }
            }
            if self.overlay == Overlay::File && code == 13 {
                if let Some(c) = self.controls.get(self.menu_index).cloned() {
                    self.activate(c.action, out);
                }
                return;
            }
            if !self.editor_interactive() {
                return;
            }
        }
        if let Some(i) = self.focus {
            if let Some(c) = self.controls.get(i).filter(|c| c.enabled).cloned() {
                if code == 13 || code == 32 {
                    self.activate(c.action, out);
                    return;
                }
                if [37, 38, 39, 40].contains(&code) {
                    let d = if code == 37 || code == 40 { -1. } else { 1. };
                    let k = if m.shift { 10. } else { 1. };
                    match c.action {
                        Action::Size | Action::Opacity => {
                            if self.busy {
                                return;
                            }
                            self.set_parameter(c.action, self.parameter_value(c.action) + d * k);
                            self.show_quick_preview(c.action);
                        }
                        Action::LayerOpacity => {
                            if self.busy {
                                return;
                            }
                            let v = (self.doc.layers[self.doc.active].opacity as f32 + d * k * 2.55)
                                .clamp(0., 255.) as u8;
                            if self.doc.set_layer_opacity(self.doc.active, v) {
                                self.changed();
                            }
                        }
                        Action::Hue => {
                            self.color_h = (self.color_h + d * k).rem_euclid(360.);
                            self.brush.color = hsv(self.color_h, self.color_s, self.color_v);
                        }
                        Action::SV => {
                            if code == 37 || code == 39 {
                                self.color_s = (self.color_s + d * k / 100.).clamp(0., 1.);
                            } else {
                                self.color_v = (self.color_v + d * k / 100.).clamp(0., 1.);
                            }
                            self.brush.color = hsv(self.color_h, self.color_s, self.color_v);
                        }
                        _ => {}
                    }
                    return;
                }
            }
        }
        if repeat && !matches!(code, 219 | 221 | 187 | 189) {
            return;
        }
        let action = if m.ctrl {
            match code {
                78 => Some(if m.shift {
                    Action::AddLayer
                } else {
                    Action::New
                }),
                79 => Some(if m.shift {
                    Action::Import
                } else {
                    Action::Open
                }),
                83 => Some(if m.shift {
                    Action::SaveAs
                } else {
                    Action::Save
                }),
                69 if m.shift => Some(Action::Export),
                90 => Some(if m.shift { Action::Redo } else { Action::Undo }),
                89 => Some(Action::Redo),
                46 => Some(Action::DeleteLayer),
                222 => Some(Action::Grid),
                _ => None,
            }
        } else {
            match code {
                84 => Some(Action::Tool(Tool::Text)),
                66 => Some(Action::Tool(Tool::Brush)),
                69 => Some(Action::Tool(Tool::Eraser)),
                71 => Some(Action::Tool(Tool::Fill)),
                73 => Some(Action::Tool(Tool::Picker)),
                72 => Some(Action::Tool(Tool::Hand)),
                76 => Some(Action::Tool(Tool::Line)),
                82 => Some(Action::Tool(Tool::Rectangle)),
                79 => Some(Action::Tool(Tool::Ellipse)),
                77 => Some(Action::Mirror),
                86 => Some(Action::Value),
                9 => Some(Action::Focus),
                48 | 96 => Some(Action::Fit),
                49 | 97 => Some(Action::Actual),
                187 | 107 => Some(Action::ZoomIn),
                189 | 109 => Some(Action::ZoomOut),
                112 => Some(Action::Help),
                113 => Some(Action::Rename),
                219 => {
                    self.brush.size = (self.brush.size / 1.15).round().max(1.);
                    self.show_quick_preview(Action::Size);
                    None
                }
                221 => {
                    self.brush.size = (self.brush.size * 1.15).round().min(512.);
                    self.show_quick_preview(Action::Size);
                    None
                }
                _ => None,
            }
        };
        if let Some(a) = action {
            self.activate(a, out);
        }
    }
    fn selected_file(&mut self, kind: FileKind, path: Option<PathBuf>, out: &mut Vec<Effect>) {
        let Some(mut path) = path else {
            self.pending = Pending::None;
            return;
        };
        if self.busy {
            self.feedback("Дождитесь окончания записи.");
            return;
        }
        match kind {
            FileKind::Project => {
                if self.file_is_save {
                    if path.extension().is_none() {
                        path.set_extension("portfolio");
                    }
                    self.save_to(path, out);
                } else {
                    self.feedback("Открываем проект…");
                    self.launch_io(
                        "portfolio-open",
                        move || IoMessage::Loaded {
                            result: Document::load(&path),
                            path,
                        },
                        out,
                    );
                }
            }
            FileKind::Image => self.queue_imports(vec![path], out),
            FileKind::Png => {
                if path.extension().is_none() {
                    path.set_extension("png");
                }
                self.export_to(path, out);
            }
        }
    }
}

impl NativeApp for PortfolioApp {
    fn preferred_language(&self) -> &'static str {
        self.language.key()
    }
    fn ui_font_scale(&self) -> f32 {
        self.font_size as f32 / 13.
    }
    fn needs_frequent_ticks(&self) -> bool {
        self.io.is_some() || self.auto_io.is_some() || self.quick_preview.is_some()
    }
    fn wants_animation(&self) -> bool {
        self.motion_enabled()
            && (self.chrome_motion.active()
                || self.popup_motion.active()
                || self.browser_search_motion.active()
                || self.overlay_motion.active()
                || self.choice_motion.active())
    }

    fn ui_font(&self) -> UiFont {
        self.ui_font
    }

    fn resize(&mut self, width: u32, height: u32, scale: f32) {
        let previous_tool_row = self.current_tool_row_rect();
        self.cancel_drag();
        self.focus = None;
        self.controls.clear();
        let old = self.stage();
        let (width, height) = platform::bounded_frame_size(width, height);
        self.width = width;
        self.height = height;
        self.scale = scale.max(0.5);
        self.clamp_panel_positions();
        if !self.initialized {
            self.initialized = true;
            self.fit();
        } else {
            let new = self.stage();
            self.view_x += (new.x + new.w / 2.) - (old.x + old.w / 2.);
            self.view_y += (new.y + new.h / 2.) - (old.y + old.h / 2.);
        }
        self.hover = None;
        self.guard_tool_row_relayout(previous_tool_row);
    }
    fn render(&mut self) -> Frame {
        self.tool_row_relayout_guard = None;
        self.restore_pinned_panel();
        if !self.initialized {
            self.initialized = true;
            self.fit();
        }
        if let Drag::Shape { x, y, ex, ey } = self.drag {
            self.preview_shape_at(x, y, ex, ey);
        }
        // Reveal pending fields before any surface builds its controls. The
        // modeless properties list must not invalidate its neighbours' hits.
        if self.tool_settings_visible() && self.pending_properties_focus {
            if let Some(action) = self.pending_focus {
                self.tool_settings_reveal_action(action);
                self.pending_focus = Some(action);
            }
        }
        self.controls.clear();
        self.quick_preview_painted = None;
        let mut p = Painter::new(
            self.width,
            self.height,
            self.scale,
            Theme::with_accent(self.dark, self.accent).base,
        );
        p.font = self.ui_font;
        p.font_scale = self.font_size as f32 / 13.;
        p.language = self.language;
        self.draw_editor(&mut p);
        self.draw_closing_overlay(&mut p);
        p.frame
    }
    fn event(&mut self, event: Event) -> Vec<Effect> {
        let previous_tool_row = self.current_tool_row_rect();
        let before = (
            self.overlay,
            self.tool_popup,
            self.focused_mode,
            self.settings_choice,
        );
        let mut out = Vec::new();
        match event {
            Event::ReducedMotion(value) => {
                self.system_reduced_motion = value;
            }
            Event::FilesDropped { paths } => self.queue_imports(paths, &mut out),
            Event::MouseMove { x, y, modifiers } => {
                self.pointer = (x, y);
                self.alt = modifiers.alt;
                self.update_drag(x, y, modifiers);
                self.hover = self
                    .controls
                    .iter()
                    .enumerate()
                    .rev()
                    .find(|(_, c)| c.hit.contains(x, y))
                    .map(|(i, _)| i);
                if let Some(choice) = self.settings_choice {
                    if let Some(action) = self
                        .hover
                        .and_then(|i| self.controls.get(i))
                        .map(|c| c.action)
                    {
                        if let Some(index) = (0..choice.len()).find(|i| choice.action(*i) == action)
                        {
                            self.choice_index = index;
                        }
                    }
                }
            }
            Event::MouseDown {
                x,
                y,
                button,
                modifiers,
            } => self.mouse_down(x, y, button, modifiers, &mut out),
            Event::MouseUp {
                x,
                y,
                button,
                modifiers,
            } => {
                if !matches!(
                    self.drag,
                    Drag::Slider { .. }
                        | Drag::Browser { .. }
                        | Drag::VerticalScroll { .. }
                        | Drag::Panel { .. }
                ) || button == MouseButton::Left
                {
                    self.update_drag(x, y, modifiers);
                    let selected = if let Drag::Browser {
                        family,
                        row,
                        moved: false,
                        pending,
                        ..
                    } = self.drag
                    {
                        let viewport = match row {
                            BrowserRow::Items => self.browser_items_rect(family),
                            BrowserRow::Categories => self.browser_categories_rect(family),
                        };
                        pending.filter(|a| {
                            viewport.contains(x, y)
                                && self
                                    .controls
                                    .iter()
                                    .any(|c| c.enabled && c.action == *a && c.hit.contains(x, y))
                        })
                    } else if matches!(
                        self.drag,
                        Drag::Panel {
                            kind: PanelKind::Chooser,
                            moved: false,
                            ..
                        }
                    ) {
                        self.tool_popup_rect().and_then(|r| {
                            Rect::new(r.x + r.w - 36., r.y + 12., 28., 28.)
                                .contains(x, y)
                                .then_some(Action::PinToolPanel)
                        })
                    } else if matches!(
                        self.drag,
                        Drag::Panel {
                            kind: PanelKind::Toolbar,
                            ..
                        }
                    ) {
                        Some(Action::MoveToolbar)
                    } else {
                        None
                    };
                    self.finish_drag();
                    if let Some(action) = selected {
                        self.activate(action, &mut out);
                    }
                }
            }
            Event::Wheel {
                x,
                y,
                delta,
                modifiers,
            } => {
                if self
                    .tool_row_relayout_guard
                    .is_some_and(|r| r.contains(x, y))
                    || self.closing_surface_contains(x, y)
                    || self.quick_preview_contains(x, y)
                {
                    // A panel owns wheel input until its visible part has gone.
                } else if let Some(choice) = self.settings_choice {
                    if self.choice_menu_rect(choice).contains(x, y) && delta != 0. {
                        self.move_choice(if delta < 0. { 1 } else { -1 });
                    }
                } else if matches!(self.drag, Drag::None) && self.overlay == Overlay::Settings {
                    if self.settings_rect().contains(x, y) {
                        self.settings_scroll_by(-delta * 44.);
                    }
                } else if matches!(self.drag, Drag::None)
                    && self.tool_settings_visible()
                    && self.tool_settings_rect().contains(x, y)
                {
                    self.tool_settings_scroll_by(-delta * 44.);
                } else if matches!(self.drag, Drag::None) && self.editor_interactive() {
                    if self.tool_popup_contains(x, y) {
                        // Each strip owns its wheel input, including Ctrl+wheel.
                        // Neither padding nor a clamped edge may zoom the canvas.
                        if let Some(family) = self.tool_popup {
                            if self.browser_items_rect(family).contains(x, y) {
                                self.scroll_browser(family, BrowserRow::Items, -delta * 72.);
                            } else if !self.browser.view(family).search_open
                                && self.browser_categories_rect(family).contains(x, y)
                            {
                                self.scroll_browser(family, BrowserRow::Categories, -delta * 36.);
                            }
                        }
                    } else if !self.focused_mode
                        && (self.top_tools_rect().contains(x, y)
                            || (self.quick_controls_visible()
                                && [Action::Size, Action::Opacity]
                                    .into_iter()
                                    .any(|action| self.quick_control_rect(action).contains(x, y))))
                    {
                        // Movable rows remain above the inspector and own the
                        // wheel as well as pointer clicks in their visible area.
                    } else if self.panel() && self.right_panel_rect().contains(x, y) {
                        let maximum = if self.inspector_layers {
                            0.
                        } else {
                            (782. - self.h()).max(0.)
                        };
                        let off = self.panel_scroll.clamp(0., maximum);
                        let previous_layer = self.layer_scroll;
                        let row_top = if self.inspector_layers {
                            269.
                        } else {
                            513. - off
                        };
                        if y >= row_top {
                            let count = (((self.h() - 62. - row_top - 40.) / 44.).floor() + 1.)
                                .max(1.) as usize;
                            if delta < 0. {
                                self.layer_scroll = (self.layer_scroll + 1)
                                    .min(self.doc.layers.len().saturating_sub(count));
                            } else {
                                self.layer_scroll = self.layer_scroll.saturating_sub(1);
                            }
                        }
                        if !self.inspector_layers
                            && (y < row_top || previous_layer == self.layer_scroll)
                        {
                            self.panel_scroll = (off - delta * 48.).clamp(0., maximum);
                            self.focus = None;
                            self.hover = None;
                        }
                    } else if self.stage().contains(x, y) && !self.canvas_obscured(x, y) {
                        if modifiers.ctrl {
                            self.brush.size = (self.brush.size + delta * 2.).clamp(1., 512.);
                            self.show_quick_preview(Action::Size);
                        } else {
                            self.zoom_at(1.15_f32.powf(delta), x, y);
                        }
                    }
                }
            }
            Event::Key {
                code,
                pressed,
                repeat,
                modifiers,
            } => self.key(code, pressed, repeat, modifiers, &mut out),
            Event::FocusLost => {
                self.finish_preset_name(true);
                self.cancel_drag();
                self.close_settings_choice();
                self.space = false;
                self.alt = false;
                self.browser_search_focus = false;
                self.settings_ui.focused = false;
            }
            Event::CaptureLost => self.cancel_drag(),
            Event::Tick => self.tick(&mut out),
            Event::CloseRequested => self.request_replace(Pending::Quit, &mut out),
            Event::Confirmed { id, answer } => {
                if id == 1 {
                    match answer {
                        Answer::Yes => self.save(false, &mut out),
                        Answer::No => self.continue_pending(&mut out),
                        Answer::Cancel => self.pending = Pending::None,
                    }
                } else if id == 2 {
                    self.recover_available = false;
                    match answer {
                        Answer::Yes => {
                            self.preserve_recovery = true;
                            self.recovery_requested = true;
                            self.start_recovery(&mut out);
                        }
                        Answer::No => {
                            self.preserve_recovery = false;
                            self.clear_recovery();
                        }
                        Answer::Cancel => {
                            self.preserve_recovery = true;
                            self.feedback(
                                "Предыдущая резервная копия сохранена до следующего запуска.",
                            );
                        }
                    }
                }
            }
            Event::TextLayerEdited {
                document,
                target,
                text,
                pixels,
            } => {
                if document == self.document_id && !self.busy {
                    match self.doc.set_text_layer(target, text.clone(), pixels) {
                        Ok(true) => {
                            self.text_style = text;
                            self.inspector_layers = true;
                            self.layer_scroll = 0;
                            self.changed();
                        }
                        Ok(false) => {}
                        Err(error) => self.error(error, &mut out),
                    }
                }
            }
            Event::Prompted { id, value } => self.accept_prompt(id, value, &mut out),
            Event::FileSelected { kind, path } => self.selected_file(kind, path, &mut out),
            Event::Text(value) | Event::Paste(value) => {
                if let Some(edit) = self.tool_properties_ui.name_edit.as_mut() {
                    edit.insert(&value);
                } else if self.overlay == Overlay::Settings {
                    self.settings_text_input(&value);
                } else {
                    self.browser_text_input(&value);
                }
            }
        }
        // Keep the file-dialog intention and native document title synchronized.
        for effect in &out {
            match effect {
                Effect::OpenFile(FileKind::Project) => self.file_is_save = false,
                Effect::SaveFile {
                    kind: FileKind::Project,
                    ..
                } => self.file_is_save = true,
                _ => {}
            }
        }
        if self.title_dirty {
            out.push(Effect::SetTitle(format!(
                "{}{} — Portfolio",
                if self.dirty() { "● " } else { "" },
                self.doc.title
            )));
            self.title_dirty = false;
        }
        if matches!(self.drag, Drag::None) {
            self.save_presets(&mut out);
        }
        if self.preferences_dirty && matches!(self.drag, Drag::None) {
            self.persist_appearance(&mut out);
            self.preferences_dirty = false;
        }
        if self.overlay == Overlay::Settings {
            self.prepare_settings_layout();
            let missing_field = match self.pending_focus {
                Some(Action::FontMenu) => !self.settings_field_visible(SettingsChoice::Font),
                Some(Action::LanguageMenu) => {
                    !self.settings_field_visible(SettingsChoice::Language)
                }
                _ => false,
            };
            if missing_field {
                self.pending_focus = Some(Action::SettingsSearch);
            }
        }
        self.restore_pinned_panel();
        self.guard_tool_row_relayout(previous_tool_row);
        self.sync_motion(before);
        for effect in &mut out {
            match effect {
                Effect::Message { title, message, .. } | Effect::Confirm { title, message, .. } => {
                    *title = self.language.translate(title);
                    *message = self.language.translate(message);
                }
                Effect::Prompt { title, label, .. } => {
                    *title = self.language.translate(title);
                    *label = self.language.translate(label);
                }
                _ => {}
            }
        }
        out
    }
}
impl PortfolioApp {
    pub fn new(persistent: bool) -> Self {
        let root = if persistent {
            std::env::var_os("LOCALAPPDATA")
                .map(PathBuf::from)
                .map(|p| p.join("Portfolio"))
        } else {
            None
        };
        let prefs = root.as_ref().map(|p| p.join("settings.txt"));
        let appearance = prefs
            .as_ref()
            .and_then(|p| fs::read_to_string(p).ok())
            .map(|s| Appearance::decode(&s))
            .unwrap_or_default();
        let recovery = root.as_ref().map(|p| p.join("recovery.portfolio"));
        let recover_available = recovery.as_ref().is_some_and(|p| p.is_file());
        let presets = root
            .as_ref()
            .and_then(|p| fs::read_to_string(p.join("brushes.txt")).ok())
            .and_then(|s| Presets::decode(&s))
            .unwrap_or_default();
        let mut brush = Brush {
            color: Color::from_hex(0x365ad7),
            size: 24.,
            opacity: 1.,
            kind: BrushKind::Ink,
            erase: false,
            mirror: false,
            ..Brush::default()
        };
        presets.current(0).settings.apply(&mut brush);
        let (h, s, v) = to_hsv(brush.color, 220.);
        Self {
            doc: demo::document(),
            dark: appearance.dark,
            ui_font: appearance.font,
            accent: appearance.accent,
            language: appearance.language,
            font_size: appearance.font_size,
            preferences_dirty: false,
            animations: appearance.animations,
            system_reduced_motion: false,
            motion_runtime: persistent,
            chrome_motion: Reveal::new(true),
            popup_motion: Reveal::new(false),
            browser_search_motion: Reveal::new(false),
            overlay_motion: Reveal::new(false),
            choice_motion: Reveal::new(false),
            closing_overlay: None,
            closing_popup: None,
            closing_choice: None,
            motion_hit_clip: None,
            motion_preview: None,
            settings_ui: SettingsUi::default(),
            tool_properties_ui: ToolPropertiesUi::default(),
            settings_choice: None,
            choice_index: 0,
            inspector_layers: false,
            browser: Browser::default(),
            browser_search_focus: false,
            presets,
            brush_samples: Default::default(),
            brush_preview: None,
            quick_preview: None,
            quick_preview_painted: None,
            quick_disc: None,
            tool: Tool::Brush,
            text_style: TextLayer {
                body: String::new(),
                font: UiFont::SegoeUi.key().into(),
                size: 48,
                bold: false,
                italic: false,
                color: brush.color,
                align: ParagraphAlign::Left,
                x: 0,
                y: 0,
            },
            brush,
            tool_popup: None,
            tool_panel_pinned: false,
            toolbar_position: None,
            chooser_position: None,
            tool_row_relayout_guard: None,
            shape_filled: false,
            shape_settings: ToolSettings {
                kind: BrushKind::Ink,
                size: 4.,
                opacity: 1.,
                ..ToolSettings::preset(BrushKind::Ink, false)
            },
            last_shape: Tool::Rectangle,
            color_h: h,
            color_s: s,
            color_v: v,
            recent: Vec::new(),
            width: 1440,
            height: 940,
            scale: 1.,
            view_x: 0.,
            view_y: 0.,
            zoom: 1.,
            initialized: false,
            focused_mode: false,
            grayscale: false,
            grid: false,
            controls: Vec::new(),
            hit_clip: None,
            focus: None,
            pending_focus: None,
            pending_properties_focus: false,
            hover: None,
            pointer: (-1., -1.),
            space: false,
            alt: false,
            overlay: Overlay::None,
            menu_index: 0,
            drag: Drag::None,
            layer_scroll: 0,
            panel_scroll: 0.,
            status: "Ваш первый этюд. Рисуйте поверх или создайте чистый холст.".into(),
            status_until: Instant::now() + Duration::from_secs(20),
            path: None,
            unsaved_origin: false,
            pending: Pending::None,
            file_is_save: false,
            new_width: 1200,
            new_height: 840,
            serial: 0,
            last_edit: Instant::now(),
            last_auto: Instant::now(),
            io: None,
            auto_io: None,
            document_id: 0,
            import_queue: VecDeque::new(),
            importing: false,
            import_failures: Vec::new(),
            import_count: 0,
            auto_revision: None,
            last_auto_saved: None,
            recovery_requested: false,
            preserve_recovery: recover_available,
            busy: false,
            prefs,
            recovery,
            recover_available,
            first_tick: true,
            composite: Vec::new(),
            composite_dirty: true,
            canvas_raster: None,
            title_dirty: true,
        }
    }
    fn w(&self) -> f32 {
        self.width as f32 / self.scale
    }
    fn h(&self) -> f32 {
        self.height as f32 / self.scale
    }
    fn panel(&self) -> bool {
        !self.focused_mode
    }
    fn stage(&self) -> Rect {
        Rect::new(0., 64., self.w(), (self.h() - 64.).max(0.))
    }
    fn top_tools_rect(&self) -> Rect {
        let width = (self.w() - 96.).max(0.).min(TOOLBAR_WIDTH);
        let position = self
            .toolbar_position
            .unwrap_or(((self.w() - width) / 2., 80.));
        let (x, y) = self.constrain_panel_position(position, (width, 48.));
        Rect::new(x, y, width, 48.)
    }
    fn toolbar_grip_rect(&self) -> Rect {
        let bar = self.top_tools_rect();
        Rect::new(bar.x + TOOLBAR_GRIP_OFFSET, bar.y + 8., 20., 32.)
    }
    fn tool_family_rect(&self, family: ToolPanel) -> Rect {
        let index = match family {
            ToolPanel::Brush => 0,
            ToolPanel::Eraser => 1,
            ToolPanel::Shapes => 2,
        };
        Rect::new(
            self.top_tools_rect().x + 8. + index as f32 * 40.,
            self.top_tools_rect().y + 8.,
            36.,
            32.,
        )
    }
    fn quick_controls_visible(&self) -> bool {
        !self.focused_mode
            && self.tool_has_properties()
            && (self.tool_popup.is_none() || self.tool_panel_pinned || self.tool_settings_visible())
            && self.closing_popup.is_none()
    }
    fn quick_controls_rect(&self) -> Rect {
        if self.tool_panel_pinned {
            if let Some(family) = self.tool_popup {
                let chooser = self.tool_popup_bounds(family);
                return Rect::new(chooser.x, chooser.y + chooser.h + 12., chooser.w, 40.);
            }
        }
        let width = (self.w() - 640.)
            .clamp(244., 544.)
            .min((self.w() - 32.).max(0.));
        let bar = self.top_tools_rect();
        let y = if let Some(chooser) = self
            .tool_popup_rect()
            .filter(|_| self.tool_settings_visible())
        {
            if chooser.y < bar.y {
                chooser.y - 52.
            } else {
                chooser.y + chooser.h + 12.
            }
        } else {
            self.follow_toolbar_y(40.)
        };
        let (x, y) = self.constrain_panel_position((bar.x + (bar.w - width) / 2., y), (width, 40.));
        Rect::new(x, y, width, 40.)
    }
    fn quick_control_rect(&self, action: Action) -> Rect {
        let group = self.quick_controls_rect();
        let width = (group.w - 12.) / 2.;
        let index = if action == Action::Opacity { 1. } else { 0. };
        Rect::new(group.x + index * (width + 12.), group.y, width, group.h)
    }
    fn quick_slider_rect(&self, action: Action) -> Rect {
        let card = self.quick_control_rect(action);
        Rect::new(card.x + 12., card.y + 18., card.w - 76., 4.)
    }
    fn quick_preview_rect(&self, action: Action) -> Rect {
        let card = self.quick_control_rect(action);
        Rect::new(
            (card.x + card.w / 2. - 56.).clamp(16., (self.w() - 128.).max(16.)),
            (card.y + card.h + 12.).min((self.h() - 128.).max(64.)),
            112.,
            112.,
        )
    }
    fn quick_preview_radius(&self, action: Action) -> f32 {
        if action == Action::Size {
            40. * (self.brush.size.clamp(1., 512.) / 512.).sqrt()
        } else {
            34.
        }
    }
    fn quick_preview_action(&self) -> Option<Action> {
        if !self.quick_controls_visible()
            || !self.editor_interactive()
            || self.closing_overlay.is_some()
            || self.busy
        {
            return None;
        }
        match self.drag {
            Drag::Slider {
                action: action @ (Action::Size | Action::Opacity),
                ..
            } => Some(action),
            Drag::None => self
                .quick_preview
                .filter(|(_, until)| Instant::now() < *until)
                .map(|(a, _)| a),
            _ => None,
        }
    }
    fn show_quick_preview(&mut self, action: Action) {
        if matches!(action, Action::Size | Action::Opacity)
            && self.quick_controls_visible()
            && self.editor_interactive()
            && matches!(self.drag, Drag::None)
            && !self.busy
            && !(action == Action::Size && self.tool == Tool::Fill)
        {
            self.quick_preview = Some((action, Instant::now() + Duration::from_millis(600)));
        }
    }
    fn quick_preview_contains(&self, x: f32, y: f32) -> bool {
        // Input follows the last presented surface, including the short interval
        // between expiry/release and the frame that removes the preview.
        self.quick_preview_painted.is_some_and(|r| r.contains(x, y))
    }
    fn tool_settings_button_rect(&self) -> Rect {
        let family = self
            .tool_popup
            .or_else(|| ToolPanel::for_tool(self.tool))
            .unwrap_or(ToolPanel::Brush);
        let popup = self.tool_popup_bounds(family);
        Rect::new(popup.x + popup.w - 36., popup.y + 48., 28., 28.)
    }
    fn tool_settings_visible(&self) -> bool {
        self.overlay == Overlay::ToolProperties && self.tool_has_properties()
    }
    fn editor_interactive(&self) -> bool {
        matches!(self.overlay, Overlay::None | Overlay::ToolProperties)
    }
    fn tool_properties_has_focus(&self) -> bool {
        self.tool_settings_visible()
            && (self
                .focus
                .and_then(|i| self.controls.get(i))
                .is_some_and(|c| {
                    c.enabled
                        && self
                            .tool_settings_rect()
                            .contains(c.rect.x + c.rect.w / 2., c.rect.y + c.rect.h / 2.)
                })
                || (self.pending_properties_focus && self.pending_focus.is_some()))
    }
    fn tool_has_properties(&self) -> bool {
        ToolPanel::for_tool(self.tool).is_some() || self.tool == Tool::Fill
    }
    fn tool_properties_title(&self) -> &'static str {
        self.tool.info().1
    }
    fn close_tool_settings(&mut self) {
        self.dismiss_tool_settings();
        self.pending_focus = Some(self.tool_properties_return_action());
    }
    fn dismiss_tool_settings(&mut self) {
        self.overlay = Overlay::None;
        self.controls.clear();
        self.focus = None;
        self.hover = None;
        self.pending_focus = None;
        self.pending_properties_focus = false;
    }
    fn tool_properties_return_action(&self) -> Action {
        if self.tool_popup.is_some() {
            Action::ToolProperties
        } else {
            ToolPanel::for_tool(self.tool).map_or(Action::Tool(self.tool), Action::ToolPanel)
        }
    }
    fn right_panel_rect(&self) -> Rect {
        Rect::new(self.w() - 304., 144., 288., (self.h() - 160.).max(0.))
    }
    fn close_settings(&mut self) {
        self.settings_ui.focused = false;
        self.settings_choice = None;
        self.closing_choice = None;
        self.choice_motion.snap(false);
        self.overlay = Overlay::None;
        self.controls.clear();
        self.focus = None;
        self.hover = None;
        self.pending_focus = Some(Action::Settings);
    }
    fn tool_popup_rect(&self) -> Option<Rect> {
        if self.focused_mode {
            return None;
        }
        self.tool_popup.map(|family| self.tool_popup_bounds(family))
    }
    fn tool_popup_contains(&self, x: f32, y: f32) -> bool {
        !self.focused_mode
            && self
                .tool_popup
                .is_some_and(|family| self.tool_popup_surface_contains(family, x, y))
    }
    fn tool_popup_surface_contains(&self, family: ToolPanel, x: f32, y: f32) -> bool {
        let rect = self.tool_popup_bounds(family);
        if rect.contains(x, y) {
            return true;
        }
        if self.tool_panel_pinned {
            return false;
        }
        let anchor = self.tool_family_rect(family);
        let center = (anchor.x + anchor.w / 2.).clamp(rect.x + 12., rect.x + rect.w - 12.);
        let upward = rect.y >= self.top_tools_rect().y;
        let progress = if upward {
            (y - (rect.y - 8.)) / 9.
        } else {
            (y - (rect.y + rect.h - 1.)) / 9.
        };
        (0. ..=1.).contains(&progress)
            && (x - center).abs() <= 8. * if upward { progress } else { 1. - progress }
    }
    fn tool_popup_bounds(&self, family: ToolPanel) -> Rect {
        let width = self.tool_chooser_width(family);
        if self.tool_panel_pinned {
            let position = self
                .chooser_position
                .unwrap_or(((self.w() - width) / 2., 136.));
            let (x, y) = self.constrain_panel_position(position, (width, 148.));
            return Rect::new(x, y, width, 96.);
        }
        Rect::new(
            self.tool_family_rect(family)
                .x
                .clamp(16., (self.w() - width - 16.).max(16.)),
            self.follow_toolbar_y(96.),
            width,
            96.,
        )
    }
    fn tool_chooser_width(&self, family: ToolPanel) -> f32 {
        let width: f32 = if family == ToolPanel::Shapes {
            380.
        } else {
            464.
        };
        let width = if self.tool_panel_pinned {
            width.min((self.w() - 640.).max(244.))
        } else {
            width
        };
        width.min((self.w() - 32.).max(0.))
    }
    fn constrain_panel_position(&self, position: (f32, f32), size: (f32, f32)) -> (f32, f32) {
        (
            position.0.clamp(16., (self.w() - size.0 - 16.).max(16.)),
            position.1.clamp(72., (self.h() - size.1 - 16.).max(72.)),
        )
    }
    fn follow_toolbar_y(&self, height: f32) -> f32 {
        let bar = self.top_tools_rect();
        let below = bar.y + bar.h + 8.;
        let y = if below + height <= self.h() - 16. {
            below
        } else {
            bar.y - 8. - height
        };
        y.clamp(72., (self.h() - height - 16.).max(72.))
    }
    fn panel_rect(&self, kind: PanelKind) -> Rect {
        match kind {
            PanelKind::Toolbar => self.top_tools_rect(),
            PanelKind::Chooser => self.tool_popup_bounds(
                self.tool_popup
                    .or_else(|| ToolPanel::for_tool(self.tool))
                    .unwrap_or(ToolPanel::Brush),
            ),
        }
    }
    fn panel_size(&self, kind: PanelKind) -> (f32, f32) {
        let rect = self.panel_rect(kind);
        (
            rect.w,
            if kind == PanelKind::Chooser {
                148.
            } else {
                rect.h
            },
        )
    }
    fn begin_panel_drag(&mut self, kind: PanelKind, x: f32, y: f32) {
        let rect = self.panel_rect(kind);
        let initial = match kind {
            PanelKind::Toolbar => self.toolbar_position,
            PanelKind::Chooser => self.chooser_position,
        };
        self.browser_search_focus = false;
        self.settings_ui.focused = false;
        self.pending_properties_focus = false;
        self.pending_focus = None;
        self.focus = None;
        self.hover = None;
        self.quick_preview = None;
        self.drag = Drag::Panel {
            kind,
            start_x: x,
            start_y: y,
            initial,
            origin: (rect.x, rect.y),
            moved: false,
        };
    }
    fn set_panel_position(&mut self, kind: PanelKind, position: Option<(f32, f32)>) {
        let previous = self.current_tool_row_rect();
        let position = position.map(|p| self.constrain_panel_position(p, self.panel_size(kind)));
        match kind {
            PanelKind::Toolbar => self.toolbar_position = position,
            PanelKind::Chooser => self.chooser_position = position,
        }
        // Keep the actual last painted area guarded even when moving one panel
        // does not change the bounding box of the entire row.
        self.tool_row_relayout_guard = self.tool_row_relayout_guard.or(previous);
        self.controls.clear();
        self.hover = None;
        self.focus = None;
        self.pending_focus = None;
        self.pending_properties_focus = false;
        self.quick_preview = None;
    }
    fn reset_panel_positions(&mut self) {
        self.set_panel_position(PanelKind::Toolbar, None);
        self.set_panel_position(PanelKind::Chooser, None);
    }
    fn clamp_panel_positions(&mut self) {
        if let Some(position) = self.toolbar_position {
            self.toolbar_position =
                Some(self.constrain_panel_position(position, self.panel_size(PanelKind::Toolbar)));
        }
        if self.tool_panel_pinned {
            if let Some(position) = self.chooser_position {
                self.chooser_position = Some(
                    self.constrain_panel_position(position, self.panel_size(PanelKind::Chooser)),
                );
            }
        }
    }
    fn panel_position_key(&mut self, code: u32, modifiers: Modifiers) -> bool {
        if !self.editor_interactive() || modifiers.ctrl || modifiers.alt || !matches!(code, 36..=40)
        {
            return false;
        }
        let action = self
            .focus
            .and_then(|i| self.controls.get(i))
            .filter(|c| c.enabled)
            .map(|c| c.action)
            .or(self.pending_focus);
        let (kind, action) = match action {
            Some(Action::MoveToolbar) => (PanelKind::Toolbar, Action::MoveToolbar),
            Some(Action::PinToolPanel) if self.tool_panel_pinned => {
                (PanelKind::Chooser, Action::PinToolPanel)
            }
            _ => return false,
        };
        let rect = self.panel_rect(kind);
        let distance = if modifiers.shift { 24. } else { 8. };
        let next = match code {
            36 => None,
            37 => Some((rect.x - distance, rect.y)),
            38 => Some((rect.x, rect.y - distance)),
            39 => Some((rect.x + distance, rect.y)),
            40 => Some((rect.x, rect.y + distance)),
            _ => return false,
        };
        self.set_panel_position(kind, next);
        self.pending_focus = Some(action);
        true
    }
    fn browser_items_rect(&self, family: ToolPanel) -> Rect {
        let r = self.tool_popup_bounds(family);
        Rect::new(r.x + 8., r.y + 8., r.w - 56., 52.)
    }
    fn browser_categories_rect(&self, family: ToolPanel) -> Rect {
        let r = self.tool_popup_bounds(family);
        Rect::new(r.x + 44., r.y + 66., r.w - 128., 24.)
    }
    fn browser_all_category_rect(&self, family: ToolPanel) -> Rect {
        let r = self.tool_popup_bounds(family);
        Rect::new(r.x + 8., r.y + 66., 32., 24.)
    }
    fn browser_search_button_rect(&self, family: ToolPanel) -> Rect {
        let r = self.tool_popup_bounds(family);
        Rect::new(r.x + r.w - 80., r.y + 66., 32., 24.)
    }
    fn browser_search_rect(&self, family: ToolPanel) -> Rect {
        let r = self.tool_popup_bounds(family);
        Rect::new(r.x + 8., r.y + 66., r.w - 56., 24.)
    }
    fn browser_item_metrics(&self, family: ToolPanel) -> (f32, f32) {
        if family == ToolPanel::Shapes {
            (112., 120.)
        } else {
            (96., 104.)
        }
    }
    fn prepare_browser_layout(&mut self, family: ToolPanel) {
        let count = self.picker_items(family).len();
        let (card, step) = self.browser_item_metrics(family);
        let item_width = if count == 0 {
            0.
        } else {
            (count - 1) as f32 * step + card
        };
        let category_width = (Category::for_family(family).len() - 2) as f32 * 36. + 32.;
        let item_viewport = self.browser_items_rect(family).w;
        let category_viewport = self.browser_categories_rect(family).w;
        let view = self.browser.view_mut(family);
        view.items.set_extent(item_width, item_viewport);
        view.categories
            .set_extent(category_width, category_viewport);
    }
    fn browser_scroll_mut(
        &mut self,
        family: ToolPanel,
        row: BrowserRow,
    ) -> &mut crate::tool_browser::Scroll {
        let view = self.browser.view_mut(family);
        match row {
            BrowserRow::Items => &mut view.items,
            BrowserRow::Categories => &mut view.categories,
        }
    }
    fn scroll_browser(&mut self, family: ToolPanel, row: BrowserRow, delta: f32) {
        self.prepare_browser_layout(family);
        if self.browser_scroll_mut(family, row).scroll_by(delta) {
            // Hit geometry belongs to the rendered scroll position. Discard it
            // until the next frame, so queued clicks cannot choose an old card.
            self.controls.clear();
            self.focus = None;
            self.pending_focus = None;
            self.hover = None;
        }
    }
    fn browser_item_action(item: ItemId) -> Action {
        match item {
            ItemId::Brush(kind) | ItemId::Eraser(kind) => Action::Brush(kind.index()),
            ItemId::Shape(kind) => Action::Shape(Self::tool_from_shape(kind)),
        }
    }
    fn browser_initial_focus(&mut self, family: ToolPanel) -> Action {
        self.prepare_browser_layout(family);
        let items = self.picker_items(family);
        let scroll = self.browser.view(family).items;
        let (card, step) = self.browser_item_metrics(family);
        let mut visible = items.iter().enumerate().filter_map(|(i, item)| {
            let x = i as f32 * step;
            (x + card > scroll.offset && x < scroll.offset + scroll.viewport).then_some(item.0)
        });
        let first = visible.clone().next();
        visible
            .find(|a| *a == self.selected_tool_option())
            .or(first)
            .unwrap_or(Action::BrowserSearch)
    }
    fn browser_text_width(&self, value: &str) -> f32 {
        // Logical advance for the small editable strip. All slicing uses UTF-8
        // scalar boundaries; a long query scrolls its caret into the field.
        value
            .chars()
            .map(|c| if c as u32 >= 0x2e80 { 11. } else { 6. })
            .sum::<f32>()
            * self.font_size as f32
            / 13.
    }
    fn browser_query_changed(&mut self, family: ToolPanel) {
        self.browser.query_changed(family);
        self.controls.clear();
        self.focus = None;
        self.pending_focus = None;
        self.hover = None;
    }
    fn browser_text_input(&mut self, value: &str) {
        if !self.browser_search_focus || !self.editor_interactive() {
            return;
        }
        if let Some(family) = self.tool_popup {
            if self.browser.view_mut(family).query.insert(value) {
                self.browser_query_changed(family);
            }
        }
    }
    fn browser_key(
        &mut self,
        code: u32,
        pressed: bool,
        repeat: bool,
        m: Modifiers,
        out: &mut Vec<Effect>,
    ) -> bool {
        let Some(family) = self
            .tool_popup
            .filter(|_| self.editor_interactive() && !self.focused_mode)
        else {
            return false;
        };
        if m.ctrl && code == 70 {
            if pressed && !repeat {
                self.finish_drag();
                self.activate(Action::BrowserSearch, out);
                self.space = false;
            }
            return true;
        }
        if pressed
            && code == 27
            && self.browser.view(family).search_open
            && matches!(self.drag, Drag::None)
        {
            self.activate(Action::CloseBrowserSearch, out);
            return true;
        }
        if !self.browser_search_focus {
            // Arrow navigation also reveals cards that have scrolled outside
            // the viewport; changing keyboard focus does not select a preset.
            if pressed && !m.ctrl && !m.alt && [37, 39, 36, 35].contains(&code) {
                let action = self
                    .focus
                    .and_then(|i| self.controls.get(i))
                    .filter(|c| c.enabled)
                    .map(|c| c.action);
                let row = match action {
                    Some(Action::Brush(_) | Action::Shape(_)) => Some(BrowserRow::Items),
                    Some(Action::BrowserCategory(_)) => Some(BrowserRow::Categories),
                    _ => None,
                };
                if let (Some(row), Some(action)) = (row, action) {
                    self.prepare_browser_layout(family);
                    let actions: Vec<Action> = match row {
                        BrowserRow::Items => self
                            .picker_items(family)
                            .into_iter()
                            .map(|item| item.0)
                            .collect(),
                        BrowserRow::Categories => Category::for_family(family)
                            .iter()
                            .copied()
                            .map(Action::BrowserCategory)
                            .collect(),
                    };
                    if let Some(index) = actions.iter().position(|a| *a == action) {
                        let next = match code {
                            36 => 0,
                            35 => actions.len() - 1,
                            37 => index.saturating_sub(1),
                            _ => (index + 1).min(actions.len() - 1),
                        };
                        let (width, step) = match row {
                            BrowserRow::Items => self.browser_item_metrics(family),
                            BrowserRow::Categories => (32., 36.),
                        };
                        if row == BrowserRow::Items || next > 0 {
                            let position = if row == BrowserRow::Categories {
                                next - 1
                            } else {
                                next
                            };
                            self.browser_scroll_mut(family, row).ensure_visible(
                                position as f32 * step,
                                position as f32 * step + width,
                            );
                        }
                        self.controls.clear();
                        self.focus = None;
                        self.hover = None;
                        self.pending_focus = Some(actions[next]);
                        return true;
                    }
                }
            }
            return false;
        }
        self.space = false;
        if !pressed {
            return true;
        }
        if matches!(code, 9 | 117 | 115) && !m.ctrl && !m.alt {
            self.browser_search_focus = false;
            if code != 115 && self.controls.is_empty() {
                // A Text event can invalidate hit geometry before Tab arrives
                // in the same frame. Preserve the intended focus transition.
                let next = if m.shift {
                    self.prepare_browser_layout(family);
                    let scroll = self.browser.view(family).items;
                    let (card, step) = self.browser_item_metrics(family);
                    self.picker_items(family)
                        .iter()
                        .enumerate()
                        .rev()
                        .find(|(i, _)| {
                            let start = *i as f32 * step;
                            start + card > scroll.offset && start < scroll.offset + scroll.viewport
                        })
                        .map_or(Action::ToolProperties, |(_, item)| item.0)
                } else if self.browser.view(family).query.text().is_empty() {
                    Action::CloseBrowserSearch
                } else {
                    Action::ClearBrowserSearch
                };
                self.focus = None;
                self.pending_focus = Some(next);
                return true;
            }
            self.focus = self
                .controls
                .iter()
                .position(|c| c.enabled && c.action == Action::BrowserSearch);
            return false;
        }
        if code == 13 && !m.ctrl && !m.alt {
            if let Some(item) = self
                .browser
                .filtered_ids(family, self.language)
                .first()
                .copied()
            {
                let action = Self::browser_item_action(item);
                self.activate(action, out);
                self.pending_focus = Some(action);
            }
            return true;
        }
        let query = &mut self.browser.view_mut(family).query;
        let changed = if m.ctrl {
            match code {
                65 => {
                    query.select_all();
                    false
                }
                67 | 88 => {
                    let value = query.selected_text();
                    if !value.is_empty() {
                        out.push(Effect::CopyText(value));
                        if code == 88 {
                            query.backspace()
                        } else {
                            false
                        }
                    } else {
                        false
                    }
                }
                86 => {
                    out.push(Effect::PasteText);
                    false
                }
                37 => {
                    query.move_left(m.shift, true);
                    false
                }
                39 => {
                    query.move_right(m.shift, true);
                    false
                }
                36 => {
                    query.home(m.shift);
                    false
                }
                35 => {
                    query.end(m.shift);
                    false
                }
                _ => false,
            }
        } else {
            match code {
                8 => query.backspace(),
                46 => query.delete(),
                37 => {
                    query.move_left(m.shift, false);
                    false
                }
                39 => {
                    query.move_right(m.shift, false);
                    false
                }
                36 => {
                    query.home(m.shift);
                    false
                }
                35 => {
                    query.end(m.shift);
                    false
                }
                _ => false,
            }
        };
        if changed {
            self.browser_query_changed(family);
        }
        // Text arrives through Text/Paste; while this field owns focus no
        // letter, Space, or document shortcut reaches the canvas commands.
        true
    }
    fn shape_kind(&self) -> Option<ShapeKind> {
        match self.tool {
            Tool::Line => Some(ShapeKind::Line),
            Tool::Rectangle => Some(ShapeKind::Rectangle),
            Tool::Ellipse => Some(ShapeKind::Ellipse),
            _ => None,
        }
    }
    fn tool_from_shape(kind: ShapeKind) -> Tool {
        match kind {
            ShapeKind::Line => Tool::Line,
            ShapeKind::Rectangle => Tool::Rectangle,
            ShapeKind::Ellipse => Tool::Ellipse,
        }
    }
    fn preview_shape_at(&mut self, x: f32, y: f32, ex: f32, ey: f32) {
        if let Some(kind) = self.shape_kind() {
            let brush = self.brush_for_tool();
            if let Err(error) =
                self.doc
                    .preview_shape_with_fill(kind, x, y, ex, ey, brush, self.shape_filled)
            {
                self.doc.cancel_shape_preview();
                self.drag = Drag::None;
                self.feedback(error);
            }
        }
    }
    fn restore_pinned_panel(&mut self) {
        if !self.tool_panel_pinned {
            return;
        }
        let wanted = if self.editor_interactive() && !self.focused_mode {
            ToolPanel::for_tool(self.tool)
        } else {
            None
        };
        if self.tool_popup != wanted {
            self.tool_popup = wanted;
            self.controls.clear();
            self.hover = None;
        }
    }
    fn current_tool_row_rect(&self) -> Option<Rect> {
        let chooser = self.tool_popup_rect();
        let quick = self
            .quick_controls_visible()
            .then(|| self.quick_controls_rect());
        let properties = self
            .tool_settings_visible()
            .then(|| self.tool_settings_rect());
        let toolbar = (!self.focused_mode).then(|| self.top_tools_rect());
        [toolbar, chooser, quick, properties]
            .into_iter()
            .flatten()
            .reduce(|a, b| {
                Rect::new(
                    a.x.min(b.x),
                    a.y.min(b.y),
                    (a.x + a.w).max(b.x + b.w) - a.x.min(b.x),
                    (a.y + a.h).max(b.y + b.h) - a.y.min(b.y),
                )
            })
    }
    fn guard_tool_row_relayout(&mut self, previous: Option<Rect>) {
        let geometry = |r: Option<Rect>| {
            r.map(|r| (r.x.to_bits(), r.y.to_bits(), r.w.to_bits(), r.h.to_bits()))
        };
        if geometry(previous) != geometry(self.current_tool_row_rect())
            && self.tool_row_relayout_guard.is_none()
        {
            // A moved/hidden surface owns its previous painted area until the
            // next frame replaces it. Rapid clicks cannot paint through it.
            self.tool_row_relayout_guard = previous;
        }
    }
    fn prepare_vertical_scroll(&mut self, surface: ScrollSurface) {
        match surface {
            ScrollSurface::Settings => self.prepare_settings_layout(),
            ScrollSurface::ToolProperties => self.prepare_tool_settings_layout(),
        }
    }
    fn vertical_scroll_state(&self, surface: ScrollSurface) -> crate::tool_browser::Scroll {
        match surface {
            ScrollSurface::Settings => self.settings_ui.scroll,
            ScrollSurface::ToolProperties => self.tool_settings_scroll(),
        }
    }
    fn vertical_scroll_track(&self, surface: ScrollSurface) -> Rect {
        match surface {
            ScrollSurface::Settings => self.settings_scrollbar_track(),
            ScrollSurface::ToolProperties => self.tool_settings_scrollbar_track(),
        }
    }
    fn vertical_scroll_thumb(&self, surface: ScrollSurface) -> Rect {
        match surface {
            ScrollSurface::Settings => self.settings_scrollbar_thumb(),
            ScrollSurface::ToolProperties => self.tool_settings_scrollbar_thumb(),
        }
    }
    fn set_vertical_scroll(&mut self, surface: ScrollSurface, offset: f32) -> bool {
        match surface {
            ScrollSurface::Settings => self.settings_scroll_to(offset),
            ScrollSurface::ToolProperties => self.tool_settings_scroll_to(offset),
        }
    }
    fn move_panel_focus(&mut self, backwards: bool) -> bool {
        let order = match self.overlay {
            Overlay::Settings => {
                self.prepare_settings_layout();
                self.settings_focus_order()
            }
            Overlay::ToolProperties if self.tool_properties_has_focus() => {
                self.prepare_tool_settings_layout();
                self.tool_settings_focus_order()
            }
            _ => return false,
        };
        if order.is_empty() {
            return true;
        }
        let current = self
            .focus
            .and_then(|i| self.controls.get(i))
            .map(|c| c.action)
            .or(self.pending_focus);
        let index = current.and_then(|a| order.iter().position(|candidate| *candidate == a));
        let next = match (index, backwards) {
            (Some(i), true) => (i + order.len() - 1) % order.len(),
            (Some(i), false) => (i + 1) % order.len(),
            (None, true) => order.len() - 1,
            (None, false) => 0,
        };
        let action = order[next];
        if self.overlay == Overlay::Settings {
            self.settings_reveal_action(action);
            self.settings_ui.focused = action == Action::SettingsSearch;
            if self.settings_ui.focused {
                self.settings_ui.query.end(false);
            }
        } else {
            self.pending_properties_focus = true;
            self.tool_settings_reveal_action(action);
        }
        self.focus = None;
        self.hover = None;
        self.pending_focus = Some(action);
        true
    }
    fn vertical_scroll_key(&mut self, code: u32, modifiers: Modifiers) -> bool {
        if modifiers.ctrl || modifiers.alt {
            return false;
        }
        let surface = match self.overlay {
            Overlay::Settings => ScrollSurface::Settings,
            Overlay::ToolProperties if self.tool_properties_has_focus() => {
                ScrollSurface::ToolProperties
            }
            _ => return false,
        };
        let focused = self
            .focus
            .and_then(|i| self.controls.get(i))
            .map(|c| c.action)
            .or(self.pending_focus);
        let scroll_focused = matches!(
            focused,
            Some(Action::SettingsScroll | Action::ToolPropertiesScroll)
        );
        if !matches!(code, 33 | 34) && !(scroll_focused && matches!(code, 35..=40)) {
            return false;
        }
        self.prepare_vertical_scroll(surface);
        let state = self.vertical_scroll_state(surface);
        let offset = match code {
            33 => state.offset - state.viewport * 0.85,
            34 => state.offset + state.viewport * 0.85,
            35 => state.max(),
            36 => 0.,
            37 | 38 => state.offset - 44.,
            _ => state.offset + 44.,
        };
        self.set_vertical_scroll(surface, offset);
        self.pending_focus = Some(match surface {
            ScrollSurface::Settings => Action::SettingsScroll,
            ScrollSurface::ToolProperties => Action::ToolPropertiesScroll,
        });
        true
    }
    fn canvas_obscured(&self, x: f32, y: f32) -> bool {
        self.tool_row_relayout_guard
            .is_some_and(|r| r.contains(x, y))
            || self.closing_surface_contains(x, y)
            || !self.focused_mode
                && (self.top_tools_rect().contains(x, y)
                    || (self.quick_controls_visible()
                        && [Action::Size, Action::Opacity]
                            .into_iter()
                            .any(|action| self.quick_control_rect(action).contains(x, y)))
                    || self.quick_preview_contains(x, y)
                    || self.right_panel_rect().contains(x, y)
                    || (self.tool_settings_visible() && self.tool_settings_rect().contains(x, y))
                    || self.tool_popup_contains(x, y))
    }
    fn closing_surface_contains(&self, x: f32, y: f32) -> bool {
        self.closing_overlay.is_some_and(|overlay| {
            self.revealed_bounds(self.overlay_bounds(overlay), 2)
                .contains(x, y)
        }) || self.closing_popup.is_some_and(|family| {
            let rect = self.tool_popup_bounds(family);
            self.revealed_bounds(Rect::new(rect.x, rect.y - 9., rect.w, rect.h + 18.), 1)
                .contains(x, y)
                && self.tool_popup_surface_contains(family, x, y)
        }) || self.closing_choice.is_some_and(|choice| {
            self.revealed_bounds(self.choice_menu_rect(choice), 3)
                .contains(x, y)
        })
    }
    fn close_tool_popup(&mut self) {
        self.browser_search_focus = false;
        if self.tool_popup.take().is_some() {
            self.controls.clear();
            self.focus = None;
            self.pending_focus = None;
            self.hover = None;
        }
    }
    fn selected_tool_option(&self) -> Action {
        if matches!(self.tool, Tool::Line | Tool::Rectangle | Tool::Ellipse) {
            Action::Shape(self.tool)
        } else {
            Action::Brush(self.presets.selected[self.preset_family().unwrap_or(0)])
        }
    }
    fn switch_tool(&mut self, tool: Tool) {
        self.quick_preview = None;
        let current = ToolPanel::for_tool(self.tool);
        let next = ToolPanel::for_tool(tool);
        if self.tool_settings_visible() && self.tool_popup.is_some() && current != next {
            self.tool_popup = next;
        }
        if current != next {
            let settings = ToolSettings::from_brush(self.brush);
            self.sync_preset();
            if current == Some(ToolPanel::Shapes) {
                self.shape_settings = settings;
            }
            let settings = match next {
                Some(ToolPanel::Brush) => self.presets.current(0).settings,
                Some(ToolPanel::Eraser) => self.presets.current(1).settings,
                Some(ToolPanel::Shapes) => self.shape_settings,
                None => settings,
            };
            settings.apply(&mut self.brush);
        }
        if next == Some(ToolPanel::Shapes) {
            self.last_shape = tool;
        }
        self.tool = tool;
        self.panel_scroll = self.panel_scroll.clamp(0., (782. - self.h()).max(0.));
        if self.overlay == Overlay::ToolProperties && !self.tool_has_properties() {
            self.dismiss_tool_settings();
        }
        self.controls.clear();
        self.hover = None;
    }
    fn occlude_underlying_surface(&mut self, p: &mut Painter, rect: Rect) {
        let visible = self
            .motion_hit_clip
            .map_or(rect, |clip| rect.intersection(clip));
        p.frame
            .texts
            .retain(|text| !visible.intersects(Rect::new(text.x, text.y, text.width, text.height)));
        for control in &mut self.controls {
            if control.hit.intersects(rect) {
                control.enabled = false;
            }
        }
    }
    fn floating_surface(&mut self, p: &mut Painter, r: Rect, label: &str) {
        self.floating_surface_with_radius(p, r, label, 4.);
    }
    fn floating_surface_with_radius(&mut self, p: &mut Painter, r: Rect, label: &str, radius: f32) {
        let t = Theme::with_accent(self.dark, self.accent);
        p.soft_shadow(r, radius, if self.dark { 0.14 } else { 0.10 });
        p.rounded(r, radius, t.surface);
        // Register beneath actual controls; blank padding must also consume input.
        self.control(r, Action::NoOp, label, false);
    }
    fn canvas(&self) -> Rect {
        Rect::new(
            self.view_x,
            self.view_y,
            self.doc.width as f32 * self.zoom,
            self.doc.height as f32 * self.zoom,
        )
    }
    fn doc_point(&self, x: f32, y: f32) -> (f32, f32) {
        ((x - self.view_x) / self.zoom, (y - self.view_y) / self.zoom)
    }
    fn fit(&mut self) {
        let s = self.stage();
        self.zoom = ((s.w - 80.) / self.doc.width as f32)
            .min((s.h - 80.) / self.doc.height as f32)
            .clamp(0.05, 16.);
        self.view_x = s.x + (s.w - self.doc.width as f32 * self.zoom) / 2.;
        self.view_y = s.y + (s.h - self.doc.height as f32 * self.zoom) / 2.;
    }
    fn zoom_at(&mut self, factor: f32, x: f32, y: f32) {
        let (dx, dy) = self.doc_point(x, y);
        self.zoom = (self.zoom * factor).clamp(0.05, 16.);
        self.view_x = x - dx * self.zoom;
        self.view_y = y - dy * self.zoom;
    }
    fn dirty(&self) -> bool {
        self.unsaved_origin || self.doc.is_dirty()
    }
    fn feedback(&mut self, text: impl Into<String>) {
        self.status = text.into();
        self.status_until = Instant::now() + Duration::from_secs(6);
    }
    fn changed(&mut self) {
        self.serial = self.serial.wrapping_add(1);
        self.last_edit = Instant::now();
        self.composite_dirty = true;
        self.title_dirty = true;
    }
    fn update_color(&mut self, c: Color) {
        self.brush.color = c;
        let (h, s, v) = to_hsv(c, self.color_h);
        self.color_h = h;
        self.color_s = s;
        self.color_v = v;
    }
    fn remember(&mut self) {
        let c = self.brush.color;
        self.recent.retain(|v| *v != c);
        self.recent.insert(0, c);
        self.recent.truncate(8);
    }
    fn brush_for_tool(&self) -> Brush {
        Brush {
            erase: self.tool == Tool::Eraser,
            ..self.brush
        }
    }
    fn reset_document(&mut self, doc: Document, path: Option<PathBuf>, unsaved: bool) {
        self.document_id = self.document_id.wrapping_add(1);
        if !unsaved {
            self.clear_recovery();
        }
        self.doc = doc;
        self.path = path;
        self.unsaved_origin = unsaved;
        self.drag = Drag::None;
        self.quick_preview = None;
        self.overlay = Overlay::None;
        self.close_tool_popup();
        self.pending = Pending::None;
        self.layer_scroll = 0;
        self.changed();
        self.fit();
        self.last_auto = Instant::now();
    }
    fn prompt(id: u32, title: &str, label: &str, value: String) -> Effect {
        Effect::Prompt {
            id,
            title: title.into(),
            label: label.into(),
            value,
        }
    }
    fn error(&mut self, e: impl Into<String>, out: &mut Vec<Effect>) {
        let e = e.into();
        self.feedback(&e);
        out.push(Effect::Message {
            title: "Portfolio".into(),
            message: e,
            error: true,
        });
    }
    fn request_replace(&mut self, pending: Pending, out: &mut Vec<Effect>) {
        self.cancel_drag();
        if self.busy {
            self.feedback("Дождитесь завершения сохранения.");
            return;
        }
        self.overlay = Overlay::None;
        self.pending = pending;
        if self.dirty() {
            out.push(Effect::Confirm{id:1,title:"Сохранить изменения?".into(),message:"Сохранить рисунок перед продолжением?\n\nДа — сохранить. Нет — продолжить без сохранения. Отмена — вернуться к рисунку.".into()});
        } else {
            self.continue_pending(out);
        }
    }
    fn continue_pending(&mut self, out: &mut Vec<Effect>) {
        let p = self.pending;
        self.pending = Pending::None;
        match p {
            Pending::New => {
                self.overlay = Overlay::New;
                self.focus = None;
            }
            Pending::Open => out.push(Effect::OpenFile(FileKind::Project)),
            Pending::Demo => {
                self.reset_document(demo::document(), None, false);
                self.feedback("Этюд открыт. Все элементы доступны на слоях.");
            }
            Pending::Quit => {
                self.clear_recovery();
                out.push(Effect::Quit);
            }
            Pending::None => {}
        }
    }
    fn save(&mut self, save_as: bool, out: &mut Vec<Effect>) {
        self.finish_drag();
        if self.busy {
            return;
        }
        if !save_as {
            if let Some(path) = self.path.clone() {
                self.save_to(path, out);
                return;
            }
        }
        out.push(Effect::SaveFile {
            kind: FileKind::Project,
            suggested_name: format!("{}.portfolio", self.doc.title),
        });
    }
    fn save_to(&mut self, path: PathBuf, out: &mut Vec<Effect>) {
        if self.busy {
            return;
        }
        match self.doc.snapshot_for_io() {
            Ok(doc) => {
                let serial = self.serial;
                self.feedback("Сохраняем проект…");
                self.launch_io(
                    "portfolio-save",
                    move || {
                        let result = doc.to_bytes().and_then(|bytes| {
                            document::atomic_write(&path, &bytes).map_err(|e| e.to_string())
                        });
                        IoMessage::Saved {
                            path,
                            serial,
                            result,
                        }
                    },
                    out,
                );
            }
            Err(e) => self.error(e, out),
        }
    }
    fn export_to(&mut self, path: PathBuf, out: &mut Vec<Effect>) {
        if self.busy {
            return;
        }
        match self.doc.snapshot_for_io() {
            Ok(doc) => {
                self.feedback("Экспортируем PNG…");
                self.launch_io(
                    "portfolio-export",
                    move || IoMessage::Exported(doc.export_png(&path)),
                    out,
                );
            }
            Err(e) => self.error(e, out),
        }
    }
    fn clear_recovery(&self) {
        if self.preserve_recovery {
            return;
        }
        if let Some(p) = &self.recovery {
            let _ = fs::remove_file(p);
        }
    }
    fn persist_appearance(&mut self, out: &mut Vec<Effect>) {
        let Some(path) = self.prefs.clone() else {
            return;
        };
        let appearance = Appearance {
            dark: self.dark,
            font: self.ui_font,
            accent: self.accent,
            language: self.language,
            font_size: self.font_size,
            animations: self.animations,
        };
        let result = (|| {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            document::atomic_write(&path, appearance.encode().as_bytes())
        })();
        if let Err(error) = result {
            self.error(format!("Не удалось сохранить настройки: {error}"), out);
        }
    }
    fn tick(&mut self, out: &mut Vec<Effect>) {
        if self
            .quick_preview
            .is_some_and(|(_, until)| Instant::now() >= until)
        {
            self.quick_preview = None;
        }
        if self.first_tick
            && !self.busy
            && self.io.is_none()
            && self.auto_io.is_none()
            && matches!(self.drag, Drag::None)
        {
            self.first_tick = false;
            out.push(Effect::SetDarkMode(self.dark));
            if self.recover_available {
                out.push(Effect::Confirm{id:2,title:"Найден незавершённый этюд".into(),message:"Восстановить рисунок из резервной копии?\n\nДа — восстановить. Нет — удалить резервную копию. Отмена — оставить копию до следующего запуска.".into()});
            }
        }
        let received = self.io.as_ref().map(|rx| rx.try_recv());
        let msg = match received {
            Some(Ok(msg)) => Some(msg),
            Some(Err(mpsc::TryRecvError::Disconnected)) => {
                self.io = None;
                self.busy = false;
                self.importing = false;
                self.import_queue.clear();
                self.import_failures.clear();
                self.import_count = 0;
                self.pending = Pending::None;
                self.error("Фоновая операция прервана. Попробуйте ещё раз.", out);
                None
            }
            _ => None,
        };
        if let Some(msg) = msg {
            self.io = None;
            self.busy = false;
            match msg {
                IoMessage::Saved {
                    path,
                    serial,
                    result,
                } => match result {
                    Ok(()) => {
                        self.path = Some(path);
                        if serial == self.serial {
                            self.doc.mark_saved();
                            self.unsaved_origin = false;
                            self.clear_recovery();
                            self.feedback("Проект сохранён");
                            self.continue_pending(out);
                        } else {
                            self.pending = Pending::None;
                            self.feedback(
                                "Сохранён предыдущий снимок. Новые изменения ещё не сохранены.",
                            );
                        }
                        self.title_dirty = true;
                    }
                    Err(e) => {
                        self.pending = Pending::None;
                        self.error(format!("Не удалось сохранить проект: {e}"), out);
                    }
                },
                IoMessage::Exported(result) => match result {
                    Ok(()) => self.feedback("PNG сохранён. Проект со слоями сохраняется отдельно."),
                    Err(e) => self.error(format!("Не удалось экспортировать PNG: {e}"), out),
                },
                IoMessage::Loaded { path, result } => match result {
                    Ok(doc) => {
                        self.reset_document(doc, Some(path), false);
                        self.feedback("Проект открыт");
                    }
                    Err(e) => self.error(e, out),
                },
                IoMessage::Recovered(result) => match result {
                    Ok(doc) => {
                        self.preserve_recovery = false;
                        self.reset_document(doc, None, true);
                        self.feedback("Этюд восстановлен. Сохраните его в нужную папку.");
                    }
                    Err(e) => self.error(e, out),
                },
                IoMessage::Imported { path, result } => {
                    let result = result.and_then(|image| {
                        let name = path
                            .file_stem()
                            .map(|s| s.to_string_lossy().into_owned())
                            .unwrap_or_else(|| "Изображение".into());
                        self.doc
                            .add_image_layer(&name, image.width, image.height, &image.pixels)
                            .map(|_| ())
                    });
                    match result {
                        Ok(()) => {
                            self.import_count += 1;
                            self.changed();
                            self.inspector_layers = true;
                            self.focused_mode = false;
                            self.panel_scroll = 0.;
                            self.layer_scroll = 0;
                        }
                        Err(e) => {
                            let name = path
                                .file_name()
                                .unwrap_or_default()
                                .to_string_lossy()
                                .chars()
                                .take(120)
                                .collect::<String>();
                            self.import_failures.push(format!(
                                "{name}: {}",
                                self.language
                                    .translate(&e)
                                    .chars()
                                    .take(400)
                                    .collect::<String>()
                            ));
                        }
                    }
                    if self.doc.layers.len() >= document::MAX_LAYERS
                        && !self.import_queue.is_empty()
                    {
                        self.import_failures
                            .push(self.language.translate("Достигнут предел числа слоёв."));
                        self.import_queue.clear();
                    }
                    self.start_next_import(out);
                }
            }
        }
        let auto = self.auto_io.as_ref().map(|rx| rx.try_recv());
        match auto {
            Some(Ok((id, result))) => {
                self.auto_io = None;
                let revision = self.auto_revision.take();
                if id != self.document_id || !self.dirty() {
                    self.clear_recovery();
                } else if result.is_ok() {
                    self.last_auto_saved = revision;
                }
                if let Err(e) = result {
                    self.feedback(format!("Резервная копия не записана: {e}"));
                }
            }
            Some(Err(mpsc::TryRecvError::Disconnected)) => {
                self.auto_io = None;
                self.auto_revision = None;
                self.feedback("Резервная копия не записана: фоновая операция прервана.");
            }
            _ => {}
        }
        self.start_recovery(out);
        if !self.busy
            && self.io.is_none()
            && self.auto_io.is_none()
            && !self.preserve_recovery
            && !self.recover_available
            && self.dirty()
            && matches!(self.drag, Drag::None)
            && self.last_auto.elapsed() > Duration::from_secs(30)
            && self.last_edit.elapsed() > Duration::from_secs(3)
            && self.last_auto_saved != Some((self.document_id, self.serial))
        {
            if let Some(path) = self.recovery.clone() {
                self.last_auto = Instant::now();
                match self.doc.snapshot_for_io() {
                    Ok(doc) => {
                        let (tx, rx) = mpsc::channel();
                        let id = self.document_id;
                        let revision = (id, self.serial);
                        let worker = std::thread::Builder::new()
                            .name("portfolio-recovery-save".into())
                            .spawn(move || {
                                let result = (|| {
                                    if let Some(parent) = path.parent() {
                                        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
                                    }
                                    let bytes = doc.to_bytes()?;
                                    document::atomic_write(&path, &bytes).map_err(|e| e.to_string())
                                })();
                                let _ = tx.send((id, result));
                            });
                        match worker {
                            Ok(_) => {
                                self.auto_io = Some(rx);
                                self.auto_revision = Some(revision);
                            }
                            Err(e) => self.feedback(format!("Резервная копия не записана: {e}")),
                        }
                    }
                    Err(e) => self.feedback(format!("Резервная копия не записана: {e}")),
                }
            }
        }
    }
    fn control(&mut self, rect: Rect, action: Action, label: impl Into<String>, enabled: bool) {
        self.control_with_kind(rect, action, label, enabled, ControlKind::Button);
    }
    fn control_with_kind(
        &mut self,
        rect: Rect,
        action: Action,
        label: impl Into<String>,
        enabled: bool,
        kind: ControlKind,
    ) {
        let clip = self
            .hit_clip
            .unwrap_or(Rect::new(0., 0., self.w(), self.h()));
        let clip = self.motion_hit_clip.map_or(clip, |m| clip.intersection(m));
        let x = rect.x.max(clip.x);
        let y = rect.y.max(clip.y);
        let right = (rect.x + rect.w).min(clip.x + clip.w);
        let bottom = (rect.y + rect.h).min(clip.y + clip.h);
        if right <= x || bottom <= y {
            return;
        }
        let hit = Rect::new(x, y, right - x, bottom - y);
        let correct_scope = !matches!(action, Action::Size | Action::Opacity)
            || !self.tool_settings_visible()
            || self.pending_properties_focus
                == self
                    .tool_settings_rect()
                    .contains(rect.x + rect.w / 2., rect.y + rect.h / 2.);
        if self.pending_focus == Some(action) && enabled && correct_scope {
            self.focus = Some(self.controls.len());
            self.pending_focus = None;
        }
        self.controls.push(Control {
            rect,
            hit,
            action,
            label: label.into(),
            enabled,
            kind,
        });
    }
    fn button(
        &mut self,
        p: &mut Painter,
        r: Rect,
        action: Action,
        label: &str,
        icon: Option<&str>,
        selected: bool,
        enabled: bool,
        primary: bool,
    ) {
        let t = Theme::with_accent(self.dark, self.accent);
        let index = self.controls.len();
        let hover = self.hover == Some(index);
        let focus = self.focus == Some(index);
        let bg = if primary {
            t.accent
        } else if selected {
            t.selected
        } else if hover && enabled {
            t.raised
        } else {
            t.surface
        };
        p.rounded(r, 4., bg);
        if selected || focus {
            let thickness = if focus { 1.5 } else { 1. };
            p.rounded(r, 4., t.accent);
            p.rounded(r.inset(thickness), 4. - thickness, bg);
        }
        let c = if !enabled {
            t.muted
        } else if primary {
            t.on_accent
        } else if selected {
            t.accent
        } else {
            t.text
        };
        if let Some(icon) = icon {
            p.icon(
                if label.is_empty() {
                    r
                } else {
                    Rect::new(r.x + 6., r.y, 28., r.h)
                },
                icon,
                c,
            );
        }
        if !label.is_empty() {
            let ir = if icon.is_some() {
                Rect::new(r.x + 34., r.y, r.w - 39., r.h)
            } else {
                r
            };
            p.text(ir, label, 12., c, primary, false, TextAlign::Center);
        }
        self.control(
            r,
            action,
            if label.is_empty() {
                format!("{:?}", action)
            } else {
                label.into()
            },
            enabled,
        );
    }
    fn icon_button(
        &mut self,
        p: &mut Painter,
        r: Rect,
        action: Action,
        icon: &str,
        tip: &str,
        selected: bool,
        enabled: bool,
    ) {
        let before = self.controls.len();
        self.button(p, r, action, "", Some(icon), selected, enabled, false);
        if self.controls.len() > before {
            if let Some(c) = self.controls.last_mut() {
                c.label = tip.into();
            }
        }
    }
    fn slider(
        &mut self,
        p: &mut Painter,
        r: Rect,
        action: Action,
        value: f32,
        max: f32,
        label: &str,
        display: String,
        enabled: bool,
    ) {
        let t = Theme::with_accent(self.dark, self.accent);
        p.label(r.x, r.y, r.w - 60., label, 12., t.muted);
        p.text(
            Rect::new(r.x + r.w - 64., r.y - 1., 64., 22.),
            display,
            12.,
            t.text,
            false,
            false,
            TextAlign::Right,
        );
        let track = Rect::new(r.x, r.y + 31., r.w, 4.);
        p.rounded(track, 2., t.border);
        p.rounded(
            Rect::new(track.x, track.y, track.w * (value / max).clamp(0., 1.), 4.),
            2.,
            if enabled { t.accent } else { t.muted },
        );
        let cx = track.x + track.w * (value / max).clamp(0., 1.);
        p.circle(cx, track.y + 2., 5., t.accent, true);
        p.circle(cx, track.y + 2., 2., t.on_accent, true);
        if self.focus == Some(self.controls.len()) {
            p.border(r.inset(-3.), t.accent);
        }
        self.control_with_kind(
            Rect::new(r.x - 5., r.y + 20., r.w + 10., 26.),
            action,
            format!("{label}: стрелки — изменить, Enter — точное значение"),
            enabled,
            ControlKind::Slider,
        );
        self.control_with_kind(
            Rect::new(r.x + r.w - 64., r.y, 64., 20.),
            action,
            format!("Точное значение: {label}"),
            enabled,
            ControlKind::Value,
        );
    }
}

impl Action {
    fn is_brush_parameter(self) -> bool {
        matches!(
            self,
            Self::Size
                | Self::Opacity
                | Self::Hardness
                | Self::Flow
                | Self::Spacing
                | Self::Angle
                | Self::Grain
                | Self::Roundness
                | Self::Scatter
        )
    }
    fn is_parameter(self) -> bool {
        self.is_brush_parameter() || self == Self::FontSize
    }
}
impl PortfolioApp {
    fn file_menu_rect(&self) -> Rect {
        Rect::new(
            20.,
            64.,
            344. + (self.font_size as f32 - 13.).max(0.) * 12.,
            344.,
        )
    }
    fn choice_menu_rect(&self, choice: SettingsChoice) -> Rect {
        let field = self.choice_field_rect(choice);
        let height = choice.len() as f32 * choice.row_height() + 8.;
        let below = field.y + field.h + 6.;
        let above = field.y - height - 6.;
        let y = if below + height <= self.h() - 12. {
            below
        } else if above >= 12. {
            above
        } else {
            below.clamp(12., (self.h() - height - 12.).max(12.))
        };
        Rect::new(field.x - 6., y, field.w + 12., height)
    }
    fn toggle_settings_choice(&mut self, choice: SettingsChoice) {
        if self.overlay != Overlay::Settings || !self.settings_field_visible(choice) {
            return;
        }
        if self.settings_choice == Some(choice) {
            self.close_settings_choice();
            return;
        }
        self.settings_reveal_field(choice);
        self.settings_ui.focused = false;
        self.settings_choice = Some(choice);
        self.closing_choice = None;
        self.choice_index = match choice {
            SettingsChoice::Font => UiFont::ALL
                .iter()
                .position(|v| *v == self.ui_font)
                .unwrap_or(0),
            SettingsChoice::Language => Language::ALL
                .iter()
                .position(|v| *v == self.language)
                .unwrap_or(0),
        };
        self.overlay_motion.snap(true);
        self.controls.clear();
        self.focus = None;
        self.hover = None;
        self.pending_focus = Some(choice.action(self.choice_index));
    }
    fn close_settings_choice(&mut self) {
        if let Some(choice) = self.settings_choice.take() {
            self.controls.clear();
            self.focus = None;
            self.hover = None;
            self.pending_focus = Some(choice.trigger());
        }
    }
    fn move_choice(&mut self, step: isize) {
        if let Some(choice) = self.settings_choice {
            self.choice_index =
                (self.choice_index as isize + step).rem_euclid(choice.len() as isize) as usize;
            self.focus = None;
            self.hover = None;
            self.pending_focus = Some(choice.action(self.choice_index));
        }
    }
    fn draw_choice_field(&mut self, p: &mut Painter, choice: SettingsChoice, value: &str) {
        let t = Theme::with_accent(self.dark, self.accent);
        let r = self.choice_field_rect(choice);
        let selected = self.settings_choice == Some(choice);
        self.button(p, r, choice.trigger(), "", None, selected, true, false);
        if let Some(control) = self.controls.last_mut() {
            control.label = match choice {
                SettingsChoice::Font => "Шрифт интерфейса",
                SettingsChoice::Language => "Язык интерфейса",
            }
            .into();
        }
        p.text_raw(
            Rect::new(r.x + 12., r.y, r.w - 48., r.h),
            value,
            13.,
            t.text,
            false,
            false,
            TextAlign::Left,
        );
        p.icon(
            Rect::new(r.x + r.w - 32., r.y + 7., 24., 24.),
            if selected { "up" } else { "down" },
            t.muted,
        );
    }
    fn draw_choice_layer(&mut self, p: &mut Painter) {
        let actual = self.settings_choice;
        let Some(choice) = actual.or(self.closing_choice) else {
            return;
        };
        let ghost = actual.is_none();
        let saved = (
            self.controls.clone(),
            self.focus,
            self.hover,
            self.pending_focus,
        );
        let parent_limit = p.limit;
        let parent_hit = self.motion_hit_clip;
        let r = self.choice_menu_rect(choice);
        self.begin_reveal(p, r, 3);
        if let Some(parent) = parent_limit {
            p.limit = Some(p.limit.map_or(parent, |clip| parent.intersection(clip)));
            self.motion_hit_clip = p.limit;
        }
        let visible = p.limit.map_or(r, |clip| r.intersection(clip));
        p.frame
            .texts
            .retain(|run| !visible.intersects(Rect::new(run.x, run.y, run.width, run.height)));
        self.controls.clear();
        let t = Theme::with_accent(self.dark, self.accent);
        self.dropdown_surface(
            p,
            r,
            r.x + r.w - 20.,
            match choice {
                SettingsChoice::Font => "Шрифт интерфейса",
                SettingsChoice::Language => "Язык интерфейса",
            },
        );
        p.clip = r.inset(1.);
        self.hit_clip = Some(p.clip);
        for i in 0..choice.len() {
            let row = Rect::new(
                r.x + 4.,
                r.y + 4. + i as f32 * choice.row_height(),
                r.w - 8.,
                choice.row_height(),
            );
            let action = choice.action(i);
            let (label, selected) = match choice {
                SettingsChoice::Font => (UiFont::ALL[i].label(), self.ui_font == UiFont::ALL[i]),
                SettingsChoice::Language => {
                    (Language::ALL[i].label(), self.language == Language::ALL[i])
                }
            };
            if !ghost && i == self.choice_index {
                self.focus = Some(self.controls.len());
            }
            self.button(p, row, action, "", None, selected, !ghost, false);
            if let Some(control) = self.controls.last_mut() {
                control.label = label.into();
            }
            p.text_raw(
                Rect::new(row.x + 28., row.y, row.w - 144., row.h),
                label,
                12.,
                t.text,
                false,
                false,
                TextAlign::Left,
            );
            if selected {
                let center = row.y + row.h / 2.;
                p.line(row.x + 9., center, row.x + 13., center + 4., 1.5, t.accent);
                p.line(
                    row.x + 13.,
                    center + 4.,
                    row.x + 21.,
                    center - 5.,
                    1.5,
                    t.accent,
                );
            }
            if choice == SettingsChoice::Font {
                let old = p.font;
                p.font = UiFont::ALL[i];
                p.text_raw(
                    Rect::new(row.x + row.w - 104., row.y, 92., row.h),
                    "Aa · 0123",
                    12.,
                    t.muted,
                    false,
                    false,
                    TextAlign::Right,
                );
                p.font = old;
            } else {
                p.text_raw(
                    Rect::new(row.x + row.w - 60., row.y, 48., row.h),
                    Language::ALL[i].key().to_uppercase(),
                    10.,
                    t.muted,
                    false,
                    false,
                    TextAlign::Right,
                );
            }
        }
        self.hit_clip = None;
        p.clip = Rect::new(0., 0., self.w(), self.h());
        p.limit = parent_limit;
        self.motion_hit_clip = parent_hit;
        if ghost {
            self.controls = saved.0;
            self.focus = saved.1;
            self.hover = saved.2;
            self.pending_focus = saved.3;
        }
    }
    fn brand_rect(&self) -> Rect {
        Rect::new(16., 10., 224., 44.)
    }
    fn dropdown_surface(&mut self, p: &mut Painter, r: Rect, anchor: f32, label: &str) {
        self.floating_surface(p, r, label);
        p.triangle_up(
            anchor.clamp(r.x + 12., r.x + r.w - 12.),
            r.y + 1.,
            8.,
            9.,
            Theme::with_accent(self.dark, self.accent).surface,
        );
    }
    fn tool_sample(mut settings: ToolSettings, color: u32, erase: bool) -> Vec<Color> {
        let Ok(mut doc) = Document::new(112, 40) else {
            return vec![Color::TRANSPARENT; 4480];
        };
        settings.size = 2. + settings.size.clamp(1., 512.).sqrt() * 0.9;
        if erase {
            doc.layers[0].pixels.fill(Color::from_hex(color));
        }
        let mut brush = Brush {
            color: Color::from_hex(color),
            erase,
            ..Brush::default()
        };
        settings.apply(&mut brush);
        let _ = doc.begin_stroke(12., 20., brush);
        for i in 1..=28 {
            let u = i as f32 / 28.;
            doc.continue_stroke(12. + 88. * u, 20. + (u * std::f32::consts::TAU).sin() * 5.);
        }
        doc.end_stroke();
        doc.composite()
    }
    fn draw_sample_checker(&self, p: &mut Painter, r: Rect) {
        let (a, b) = if self.dark {
            (0x303030, 0x414141)
        } else {
            (0xe6e6e6, 0xc9c9c9)
        };
        let mut y = r.y;
        let mut row = 0;
        while y < r.y + r.h {
            let mut x = r.x;
            let mut column = 0;
            while x < r.x + r.w {
                p.rect(
                    Rect::new(x, y, (r.x + r.w - x).min(5.), (r.y + r.h - y).min(5.)),
                    if (row + column) % 2 == 0 { a } else { b },
                );
                x += 5.;
                column += 1;
            }
            y += 5.;
            row += 1;
        }
    }
    fn draw_brush_sample(&mut self, p: &mut Painter, r: Rect, color: u32) {
        let settings = ToolSettings::from_brush(self.brush);
        let erase = self.tool == Tool::Eraser;
        if self
            .brush_preview
            .as_ref()
            .is_none_or(|(s, c, e, _)| *s != settings || *c != color || *e != erase)
        {
            self.brush_preview = Some((
                settings,
                color,
                erase,
                Self::tool_sample(settings, color, erase),
            ));
        }
        if erase {
            self.draw_sample_checker(p, r);
        }
        if let Some((_, _, _, sample)) = &self.brush_preview {
            p.image(r, 112, 40, sample, false, 1.);
        }
    }
    fn parameter_info(&self, a: Action) -> (f32, f32, &'static str, u32, &'static str) {
        match a {
            Action::Size => (1., 512., "Размер", 11, "px"),
            Action::Opacity => (
                0.,
                100.,
                if self.tool == Tool::Eraser {
                    "Сила стирания"
                } else {
                    "Непрозрачность"
                },
                12,
                "%",
            ),
            Action::Hardness => (0., 100., "Жёсткость", 17, "%"),
            Action::Flow => (0., 100., "Подача краски", 18, "%"),
            Action::Spacing => (2., 200., "Интервал", 19, "%"),
            Action::Angle => (0., 360., "Угол", 20, "°"),
            Action::Roundness => (10., 100., "Округлость", 23, "%"),
            Action::Grain => (0., 100., "Зерно", 24, "%"),
            Action::Scatter => (0., 100., "Разброс", 21, "%"),
            Action::FontSize => (11., 17., "Размер текста", 22, "px"),
            _ => (0., 1., "", 0, ""),
        }
    }
    fn parameter_value(&self, a: Action) -> f32 {
        match a {
            Action::Size => self.brush.size,
            Action::Opacity => self.brush.opacity * 100.,
            Action::Hardness => self.brush.hardness * 100.,
            Action::Flow => self.brush.flow * 100.,
            Action::Spacing => self.brush.spacing * 100.,
            Action::Angle => self.brush.angle,
            Action::Roundness => self.brush.roundness * 100.,
            Action::Grain => self.brush.grain * 100.,
            Action::Scatter => self.brush.scatter * 100.,
            Action::FontSize => self.font_size as f32,
            _ => 0.,
        }
    }
    fn set_parameter(&mut self, a: Action, value: f32) {
        if !value.is_finite() {
            return;
        }
        let (min, max, _, _, _) = self.parameter_info(a);
        let value = value.clamp(min, max);
        match a {
            Action::Size => self.brush.size = value,
            Action::Opacity => self.brush.opacity = value / 100.,
            Action::Hardness => self.brush.hardness = value / 100.,
            Action::Flow => self.brush.flow = value / 100.,
            Action::Spacing => self.brush.spacing = value / 100.,
            Action::Angle => self.brush.angle = value,
            Action::Roundness => self.brush.roundness = value / 100.,
            Action::Grain => self.brush.grain = value / 100.,
            Action::Scatter => self.brush.scatter = value / 100.,
            Action::FontSize => {
                self.font_size = value.round() as u16;
                self.preferences_dirty = true;
            }
            _ => {}
        }
    }
    fn parameter_slider(&mut self, p: &mut Painter, r: Rect, a: Action) {
        let (min, max, label, _, unit) = self.parameter_info(a);
        let value = self.parameter_value(a);
        self.slider(
            p,
            r,
            a,
            value - min,
            max - min,
            label,
            format!("{value:.0} {unit}"),
            !self.busy || a == Action::FontSize,
        );
    }
    fn changed_raster(&mut self) {
        self.serial = self.serial.wrapping_add(1);
        self.last_edit = Instant::now();
        self.title_dirty = true;
    }
    fn motion_enabled(&self) -> bool {
        self.motion_runtime && self.animations && !self.system_reduced_motion
    }
    fn motion_value(&self, key: u8) -> f32 {
        if let Some(value) = self.motion_preview {
            if key != 0 {
                return value;
            }
        }
        let motion = match key {
            0 => &self.chrome_motion,
            1 => &self.popup_motion,
            2 => &self.overlay_motion,
            _ => &self.choice_motion,
        };
        if self.motion_enabled() {
            motion.value()
        } else {
            motion.target()
        }
    }
    fn begin_reveal(&mut self, p: &mut Painter, r: Rect, key: u8) {
        let progress = self.motion_value(key);
        if progress < 0.999 {
            let clip = Rect::new(r.x - 18., r.y - 12., r.w + 36., (r.h + 32.) * progress);
            p.limit = Some(clip);
            self.motion_hit_clip = Some(clip);
        }
    }
    fn revealed_bounds(&self, r: Rect, key: u8) -> Rect {
        let progress = self.motion_value(key);
        if progress >= 0.999 {
            r
        } else {
            r.intersection(Rect::new(
                r.x - 18.,
                r.y - 12.,
                r.w + 36.,
                (r.h + 32.) * progress,
            ))
        }
    }
    fn end_reveal(&mut self, p: &mut Painter) {
        p.limit = None;
        self.motion_hit_clip = None;
    }
    fn overlay_bounds(&self, overlay: Overlay) -> Rect {
        match overlay {
            Overlay::Settings => self.settings_rect(),
            Overlay::ToolProperties => self.tool_settings_rect(),
            Overlay::File => self.file_menu_rect(),
            _ => Rect::new(0., 64., self.w(), self.h() - 64.),
        }
    }
    fn sync_motion(&mut self, before: (Overlay, Option<ToolPanel>, bool, Option<SettingsChoice>)) {
        if self.overlay != Overlay::Settings {
            self.settings_choice = None;
            self.closing_choice = None;
            self.choice_motion.snap(false);
        } else if before.3 != self.settings_choice {
            self.closing_choice = if self.settings_choice.is_none() {
                before
                    .3
                    .filter(|choice| self.settings_field_visible(*choice))
            } else {
                None
            };
            // A collapsed section no longer owns the dropdown's anchor. Do not
            // animate its old menu over the section that took its place.
            if self.settings_choice.is_none() && self.closing_choice.is_none() {
                self.choice_motion.snap(false);
            }
            if before.3.is_some() && self.settings_choice.is_some() {
                self.choice_motion.snap(false);
            }
            self.choice_motion.set(self.settings_choice.is_some());
        }
        if before.0 != self.overlay {
            self.closing_overlay = if self.overlay == Overlay::None && before.0 != Overlay::None {
                Some(before.0)
            } else {
                None
            };
            if self.overlay != Overlay::None && before.0 != Overlay::None {
                self.overlay_motion.snap(false);
            }
            self.overlay_motion.set(self.overlay != Overlay::None);
        }
        if before.1 != self.tool_popup {
            self.browser_search_motion.snap(
                self.tool_popup
                    .is_some_and(|family| self.browser.view(family).search_open),
            );
            self.closing_popup = if self.tool_popup.is_none() {
                before.1
            } else {
                None
            };
            if self.tool_popup.is_some() && before.1.is_some() {
                self.popup_motion.snap(false);
            }
            self.popup_motion.set(self.tool_popup.is_some());
        }
        self.chrome_motion.set(!self.focused_mode);
        if !self.motion_enabled() {
            self.chrome_motion.snap(!self.focused_mode);
            self.popup_motion.snap(self.tool_popup.is_some());
            self.browser_search_motion.snap(
                self.tool_popup
                    .is_some_and(|family| self.browser.view(family).search_open),
            );
            self.overlay_motion.snap(self.overlay != Overlay::None);
            self.choice_motion.snap(self.settings_choice.is_some());
            self.closing_overlay = None;
            self.closing_popup = None;
            self.closing_choice = None;
        }
        if !self.overlay_motion.active() && self.overlay_motion.target() == 0. {
            self.closing_overlay = None;
        }
        if !self.popup_motion.active() && self.popup_motion.target() == 0. {
            self.closing_popup = None;
        }
        if !self.choice_motion.active() && self.choice_motion.target() == 0. {
            self.closing_choice = None;
        }
    }
    fn draw_popup_layer(&mut self, p: &mut Painter) {
        let actual = self.tool_popup;
        let Some(family) = actual.or(self.closing_popup) else {
            return;
        };
        let ghost = actual.is_none();
        let saved = if ghost {
            Some((
                self.controls.clone(),
                self.focus,
                self.hover,
                self.pending_focus,
            ))
        } else {
            None
        };
        self.tool_popup = Some(family);
        if let Some(r) = self.tool_popup_rect() {
            self.begin_reveal(p, r, 1);
            self.draw_tool_popup(p);
            self.end_reveal(p);
        }
        self.tool_popup = actual;
        if let Some((controls, focus, hover, pending)) = saved {
            self.controls = controls;
            self.focus = focus;
            self.hover = hover;
            self.pending_focus = pending;
        }
    }
    fn draw_animated_overlay(&mut self, p: &mut Painter) {
        self.begin_reveal(p, self.overlay_bounds(self.overlay), 2);
        self.draw_overlay(p);
        self.end_reveal(p);
    }
    fn draw_closing_overlay(&mut self, p: &mut Painter) {
        if self.overlay != Overlay::None {
            return;
        }
        let Some(overlay) = self.closing_overlay else {
            return;
        };
        let saved = (
            self.controls.clone(),
            self.focus,
            self.hover,
            self.pending_focus,
        );
        self.overlay = overlay;
        self.draw_animated_overlay(p);
        self.overlay = Overlay::None;
        self.controls = saved.0;
        self.focus = saved.1;
        self.hover = saved.2;
        self.pending_focus = saved.3;
    }
    /// Deterministic design previews never wait or sleep for an animation.
    pub fn set_preview_motion(&mut self, progress: f32) {
        self.motion_preview = Some(progress.clamp(0., 1.));
    }
}

impl PortfolioApp {
    fn launch_io<F>(&mut self, name: &str, job: F, out: &mut Vec<Effect>) -> bool
    where
        F: FnOnce() -> IoMessage + Send + 'static,
    {
        let (tx, rx) = mpsc::channel();
        match std::thread::Builder::new()
            .name(name.into())
            .spawn(move || {
                let _ = tx.send(job());
            }) {
            Ok(_) => {
                self.io = Some(rx);
                self.busy = true;
                true
            }
            Err(e) => {
                self.error(format!("Не удалось запустить фоновую операцию: {e}"), out);
                false
            }
        }
    }
    fn queue_imports(&mut self, paths: Vec<PathBuf>, out: &mut Vec<Effect>) {
        if paths.is_empty() {
            return;
        }
        if self.busy && !self.importing {
            self.feedback("Дождитесь окончания записи.");
            return;
        }
        if paths.len() + self.import_queue.len() + usize::from(self.busy) > 32 {
            self.error(
                "Можно добавить не более 32 изображений за одну операцию.",
                out,
            );
            return;
        }
        self.finish_drag();
        self.overlay = Overlay::None;
        self.close_tool_popup();
        if !self.importing {
            self.import_count = 0;
            self.import_failures.clear();
        }
        self.importing = true;
        self.import_queue.extend(paths);
        if !self.busy {
            self.start_next_import(out);
        }
    }
    fn start_next_import(&mut self, out: &mut Vec<Effect>) {
        if let Some(path) = self.import_queue.pop_front() {
            self.feedback("Добавляем изображения…");
            if !self.launch_io(
                "portfolio-import",
                move || IoMessage::Imported {
                    result: platform::load_image(&path),
                    path,
                },
                out,
            ) {
                self.import_queue.clear();
                self.importing = false;
                self.import_count = 0;
                self.import_failures.clear();
            }
        } else {
            self.importing = false;
            if self.import_failures.is_empty() {
                self.feedback(format!("Добавлено изображений: {}", self.import_count));
            } else {
                let errors = std::mem::take(&mut self.import_failures);
                self.error(
                    format!(
                        "{}\n\n{}",
                        self.language
                            .translate("Некоторые изображения не удалось импортировать."),
                        errors.join("\n")
                    ),
                    out,
                );
            }
            self.import_count = 0;
        }
    }
    fn start_recovery(&mut self, out: &mut Vec<Effect>) {
        if !self.recovery_requested || self.busy || self.io.is_some() || self.auto_io.is_some() {
            return;
        }
        self.recovery_requested = false;
        if let Some(path) = self.recovery.clone() {
            self.feedback("Восстанавливаем проект…");
            self.launch_io(
                "portfolio-recovery-open",
                move || IoMessage::Recovered(Document::load(&path)),
                out,
            );
        }
    }
}
