//! Toolbar color popup. Painting and pointer input share the same HSV geometry.
use super::*;

const POPUP_HEIGHT: f32 = 332.;
const RING_INNER: f32 = 0.83;
const FIELD_RADIUS: f32 = 0.76;

#[derive(Default)]
pub(super) struct ColorDisc {
    pub open: bool,
    bitmap: Option<(u32, f32, Vec<Color>)>,
}

// Smooth, invertible square-to-disc mapping. Saturation increases to the right,
// value upwards. All four extremes remain selectable, including pure white.
fn field_point(s: f32, v: f32) -> (f32, f32) {
    let (u, w) = (2. * s - 1., 1. - 2. * v);
    (u * (1. - w * w * 0.5).sqrt(), w * (1. - u * u * 0.5).sqrt())
}
fn field_value(x: f32, y: f32) -> (f32, f32) {
    let length = x.hypot(y).max(1.);
    let (x, y) = (x / length, y / length);
    let axis = |a: f32, b: f32| {
        let base = 2. + a * a - b * b;
        let shift = 2. * std::f32::consts::SQRT_2 * a;
        ((base + shift).max(0.).sqrt() - (base - shift).max(0.).sqrt()) * 0.5
    };
    (
        ((axis(x, y) + 1.) * 0.5).clamp(0., 1.),
        ((1. - axis(y, x)) * 0.5).clamp(0., 1.),
    )
}

fn hue_angle(x: f32, y: f32) -> f32 {
    x.atan2(-y).to_degrees().rem_euclid(360.)
}

impl ColorDisc {
    fn pixels(&mut self, size: u32, hue: f32) -> &[Color] {
        if self
            .bitmap
            .as_ref()
            .is_none_or(|(n, h, _)| *n != size || *h != hue)
        {
            let half = size as f32 * 0.5;
            let mut pixels = Vec::with_capacity((size * size) as usize);
            for row in 0..size {
                let y = (row as f32 + 0.5) / half - 1.;
                for col in 0..size {
                    let x = (col as f32 + 0.5) / half - 1.;
                    let d = x.hypot(y);
                    let (mut color, coverage) = if d < (FIELD_RADIUS + RING_INNER) * 0.5 {
                        let (s, v) = field_value(x / FIELD_RADIUS, y / FIELD_RADIUS);
                        (hsv(hue, s, v), (FIELD_RADIUS - d) * half + 0.5)
                    } else {
                        let edge = (1. - d).min(d - RING_INNER);
                        (hsv(hue_angle(x, y), 1., 1.), edge * half + 0.5)
                    };
                    color.a = (coverage.clamp(0., 1.) * 255.).round() as u8;
                    pixels.push(color);
                }
            }
            self.bitmap = Some((size, hue, pixels));
        }
        &self.bitmap.as_ref().unwrap().2
    }
}

impl PortfolioApp {
    pub fn color_trigger(&self) -> Rect {
        let bar = self.top_tools_rect();
        Rect::new(bar.x + 168., bar.y + 8., 36., 32.)
    }

    pub(super) fn color_popup_rect(&self) -> Rect {
        let trigger = self.color_trigger();
        let width = 264_f32.min((self.w() - 16.).max(1.));
        let x =
            (trigger.x + trigger.w * 0.5 - width * 0.5).clamp(8., (self.w() - width - 8.).max(8.));
        let below = trigger.y + trigger.h + 12.;
        let space_below = self.h() - 8. - below;
        let space_above = trigger.y - 20.;
        let opens_down = space_below >= POPUP_HEIGHT || space_below >= space_above;
        let height = (if opens_down { space_below } else { space_above }).clamp(1., POPUP_HEIGHT);
        let y = if opens_down {
            below
        } else {
            trigger.y - 12. - height
        };
        Rect::new(x, y, width, height)
    }

    pub(super) fn wheel_rect(&self) -> Rect {
        let panel = self.color_popup_rect();
        let size = (panel.w - 32.).min(panel.h - 110.).max(1.);
        Rect::new(panel.x + (panel.w - size) / 2., panel.y + 44., size, size)
    }

    pub(super) fn draw_color_disc(&mut self, p: &mut Painter) {
        let panel = self.color_popup_rect();
        let wheel = self.wheel_rect();
        let t = Theme::with_accent(self.dark, self.accent);
        self.occlude_underlying_surface(p, panel);
        self.floating_surface(p, panel, "Цветовой круг");
        let trigger = self.color_trigger();
        let anchor = trigger.x + trigger.w * 0.5;
        if panel.y > trigger.y {
            p.triangle_up(anchor, panel.y + 1., 8., 8., t.surface);
        } else {
            p.triangle_down(anchor, panel.y + panel.h - 1., 8., 8., t.surface);
        }
        p.clip = panel.inset(1.);
        self.hit_clip = Some(p.clip);
        p.text(
            Rect::new(panel.x + 16., panel.y + 8., panel.w - 178., 28.),
            "Цвет",
            16.,
            t.text,
            true,
            false,
            TextAlign::Left,
        );
        let ink = self.brush.color;
        let chip = Rect::new(panel.x + panel.w - 122., panel.y + 8., 106., 28.);
        self.slider_reset_button(
            p,
            Rect::new(chip.x - 32., chip.y + 2., 24., 24.),
            Slider::Hue,
            "Оттенок",
            true,
        );
        let size = (wheel.w * self.scale).round().clamp(1., 1024.) as u32;
        let pixels = self.color_disc.pixels(size, self.color_h);
        p.image(wheel, size, size, pixels, false, 1.);
        let r = wheel.w / 2.;
        let (cx, cy) = (wheel.x + r, wheel.y + r);
        let angle = self.color_h.to_radians();
        let mid = (1. + RING_INNER) * 0.5 * r;
        let (x, y) = field_point(self.color_s, self.color_v);
        let field = Rect::new(
            cx - r * FIELD_RADIUS,
            cy - r * FIELD_RADIUS,
            2. * r * FIELD_RADIUS,
            2. * r * FIELD_RADIUS,
        );
        for (action, area, label, px, py, fill) in [
            (
                Action::DiscHue,
                wheel,
                "Оттенок",
                cx + angle.sin() * mid,
                cy - angle.cos() * mid,
                hsv(self.color_h, 1., 1.),
            ),
            (
                Action::DiscSV,
                field,
                "Насыщенность и яркость",
                cx + x * r * FIELD_RADIUS,
                cy + y * r * FIELD_RADIUS,
                ink,
            ),
        ] {
            self.control(area, action, label, true);
            if self.focus == Some(self.controls.len() - 1) {
                p.circle(px, py, 11., t.accent, true);
            }
            p.circle(px, py, 8.5, 0x222222, true);
            p.circle(px, py, 7.3, 0xffffff, true);
            p.circle(px, py, 5.3, rgb(fill), true);
        }
        self.button(p, chip, Action::Hex, "", None, false, true, false);
        self.controls.last_mut().unwrap().label = "HEX · точный цвет".into();
        p.rounded(Rect::new(chip.x + 4., chip.y + 5., 18., 18.), 4., t.border);
        p.rounded(Rect::new(chip.x + 5., chip.y + 6., 16., 16.), 3., rgb(ink));
        p.text(
            Rect::new(chip.x + 26., chip.y, chip.w - 28., chip.h),
            hex(ink),
            12.,
            t.text,
            false,
            false,
            TextAlign::Center,
        );
        let width = (panel.w - 32. - 7. * 4.) / 8.;
        for (i, &raw) in PALETTE.iter().enumerate() {
            let rect = Rect::new(
                panel.x + 16. + i as f32 * (width + 4.),
                panel.y + panel.h - 60.,
                width,
                22.,
            );
            let c = Color::from_hex(raw);
            let focused = self.focus == Some(self.controls.len())
                || self.pending_focus == Some(Action::Swatch(i));
            p.rounded(rect, 4., if focused { t.accent } else { t.border });
            p.rounded(rect.inset(if focused { 2. } else { 1. }), 3., raw);
            if ink == c {
                p.icon(
                    rect,
                    "check",
                    if 3 * u32::from(c.r) + 6 * u32::from(c.g) + u32::from(c.b) > 1450 {
                        0x171717
                    } else {
                        0xffffff
                    },
                );
            }
            self.control(rect, Action::Swatch(i), format!("Цвет #{raw:06X}"), true);
        }
        let recent_y = panel.y + panel.h - 30.;
        p.text(
            Rect::new(panel.x + 16., recent_y, 76., 20.),
            "НЕДАВНИЕ",
            10.,
            t.muted,
            false,
            false,
            TextAlign::Left,
        );
        let step = (panel.w - 106.) / 8.;
        for i in 0..8 {
            let r = Rect::new(panel.x + 90. + i as f32 * step, recent_y, step - 3., 20.);
            if let Some(color) = self.recent.get(i).copied() {
                let action = Action::Recent(i);
                let focused =
                    self.focus == Some(self.controls.len()) || self.pending_focus == Some(action);
                p.rounded(r, 3., if focused { t.accent } else { t.border });
                p.rounded(r.inset(if focused { 2. } else { 1. }), 2., rgb(color));
                self.control(r, action, format!("Недавний {}", hex(color)), true);
            } else {
                p.rounded(r, 3., t.raised);
            }
        }
        self.hit_clip = None;
        p.clip = Rect::new(0., 0., self.w(), self.h());
    }

    pub(super) fn close_color_disc(&mut self, out: &mut Vec<Effect>) {
        self.color_disc.open = false;
        self.controls.clear();
        self.focus = None;
        self.hover = None;
        if self.draft.is_some() {
            self.pending_focus = None;
            out.push(Effect::FocusCanvasText);
        } else {
            self.pending_focus = Some(Action::ColorDisc);
        }
    }

    pub(super) fn color_disc_mouse(
        &mut self,
        x: f32,
        y: f32,
        button: MouseButton,
        modifiers: Modifiers,
        out: &mut Vec<Effect>,
    ) {
        if !matches!(self.drag, Drag::None) || button != MouseButton::Left {
            return;
        }
        if !self.color_popup_rect().contains(x, y) {
            self.close_color_disc(out);
            return;
        }
        let rect = self.wheel_rect();
        let r = rect.w / 2.;
        let d = (x - rect.x - r).hypot(y - rect.y - r) / r;
        let part = if (RING_INNER..=1.).contains(&d) {
            Some(Action::DiscHue)
        } else if d <= FIELD_RADIUS {
            Some(Action::DiscSV)
        } else {
            None
        };
        if let Some(action) = part {
            self.focus = None;
            self.pending_focus = Some(action);
            self.begin_color_drag(action, rect, x, y, modifiers);
        } else if let Some(action) = self
            .controls
            .iter()
            .rev()
            .find(|c| c.enabled && c.hit.contains(x, y))
            .map(|c| c.action)
        {
            if matches!(
                action,
                Action::Hex
                    | Action::Swatch(_)
                    | Action::Recent(_)
                    | Action::ResetSlider(Slider::Hue)
            ) {
                self.activate(action, out);
                self.pending_focus = Some(action);
            }
        }
    }

    pub(super) fn begin_color_drag(
        &mut self,
        action: Action,
        rect: Rect,
        x: f32,
        y: f32,
        modifiers: Modifiers,
    ) {
        self.drag = Drag::Color {
            action,
            rect,
            initial: self.brush.color,
            initial_hsv: (self.color_h, self.color_s, self.color_v),
        };
        self.update_drag(x, y, modifiers);
    }

    pub(super) fn update_color_drag(&mut self, action: Action, rect: Rect, x: f32, y: f32) {
        let nx = (x - rect.x) / rect.w;
        let ny = (y - rect.y) / rect.h;
        match action {
            Action::DiscHue => {
                let (x, y) = (2. * nx - 1., 2. * ny - 1.);
                if x.hypot(y) > 0.001 {
                    self.color_h = hue_angle(x, y);
                }
            }
            Action::DiscSV => {
                (self.color_s, self.color_v) =
                    field_value((2. * nx - 1.) / FIELD_RADIUS, (2. * ny - 1.) / FIELD_RADIUS);
            }
            _ => return,
        }
        self.apply_hsv();
    }

    pub(super) fn color_disc_key(&mut self, code: u32, m: Modifiers, out: &mut Vec<Effect>) {
        if code == 27 {
            if matches!(self.drag, Drag::Color { .. }) {
                self.cancel_drag();
            } else {
                self.close_color_disc(out);
            }
            return;
        }
        if !matches!(self.drag, Drag::None) {
            return;
        }
        let current = self
            .pending_focus
            .or_else(|| {
                self.focus
                    .and_then(|i| self.controls.get(i))
                    .map(|c| c.action)
            })
            .unwrap_or(Action::DiscHue);
        if matches!(code, 9 | 117) {
            let mut choices = vec![Action::DiscHue, Action::DiscSV, Action::Hex];
            choices.extend((0..PALETTE.len()).map(Action::Swatch));
            choices.extend((0..self.recent.len()).map(Action::Recent));
            choices.push(Action::ResetSlider(Slider::Hue));
            let index = choices.iter().position(|&a| a == current).unwrap_or(0);
            let next = if m.shift {
                index + choices.len() - 1
            } else {
                index + 1
            };
            self.focus = None;
            self.pending_focus = Some(choices[next % choices.len()]);
        } else if (37..=40).contains(&code) {
            let step = if m.shift { 10. } else { 1. };
            let delta = if matches!(code, 37 | 40) { -step } else { step };
            match current {
                Action::DiscHue => self.color_h = (self.color_h + delta).rem_euclid(360.),
                Action::DiscSV if matches!(code, 37 | 39) => {
                    self.color_s = (self.color_s + delta * 0.01).clamp(0., 1.)
                }
                Action::DiscSV => self.color_v = (self.color_v + delta * 0.01).clamp(0., 1.),
                _ => return,
            }
            self.apply_hsv();
        } else if matches!(code, 13 | 32) {
            if matches!(
                current,
                Action::Hex
                    | Action::Swatch(_)
                    | Action::Recent(_)
                    | Action::ResetSlider(Slider::Hue)
            ) {
                self.activate(current, out);
                self.pending_focus = Some(current);
            } else {
                self.close_color_disc(out);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saturation_value_and_markers_round_trip_on_the_disc_including_its_boundary() {
        for si in 0..=20 {
            for vi in 0..=20 {
                let (s, v) = (si as f32 / 20., vi as f32 / 20.);
                let (x, y) = field_point(s, v);
                assert!(x.hypot(y) <= 1.00001);
                let (rs, rv) = field_value(x, y);
                assert!((rs - s).abs() < 0.001 && (rv - v).abs() < 0.001);
            }
        }
        assert_eq!(field_value(0., 5.), field_value(0., 1.));
        assert_eq!(hue_angle(0., -1.), 0.);
        assert_eq!(hue_angle(1., 0.), 90.);
        let mut disc = ColorDisc::default();
        let px = disc.pixels(201, 122.);
        assert_eq!(px[100 * 201 + 100], hsv(122., 0.5, 0.5));
        assert_eq!(px[100 * 201 + 194], hsv(90., 1., 1.));
        assert_eq!(px[0].a, 0);
        assert_eq!(px[100 * 201 + 180].a, 0);
    }
}
