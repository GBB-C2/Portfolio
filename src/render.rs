use crate::document::Color;
use crate::platform::{Frame, TextAlign, TextRun, UiFont};
use crate::{appearance::Accent, i18n::Language};
use std::{fs, io::Write, path::Path};

#[derive(Clone, Copy, Debug, Default)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}
impl Rect {
    pub const fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }
    pub fn contains(self, x: f32, y: f32) -> bool {
        x >= self.x && y >= self.y && x < self.x + self.w && y < self.y + self.h
    }
    pub fn intersection(self, other: Self) -> Self {
        let x = self.x.max(other.x);
        let y = self.y.max(other.y);
        Self::new(
            x,
            y,
            (self.x + self.w).min(other.x + other.w).max(x) - x,
            (self.y + self.h).min(other.y + other.h).max(y) - y,
        )
    }
    pub fn intersects(self, other: Self) -> bool {
        self.x < other.x + other.w
            && self.x + self.w > other.x
            && self.y < other.y + other.h
            && self.y + self.h > other.y
    }
    pub fn inset(self, n: f32) -> Self {
        Self::new(
            self.x + n,
            self.y + n,
            (self.w - n * 2.).max(0.),
            (self.h - n * 2.).max(0.),
        )
    }
}

#[derive(Clone, Copy)]
pub struct Theme {
    pub base: u32,
    pub surface: u32,
    pub raised: u32,
    pub workspace: u32,
    pub text: u32,
    pub muted: u32,
    pub border: u32,
    pub accent: u32,
    pub on_accent: u32,
    pub selected: u32,
    pub danger: u32,
}
impl Theme {
    pub fn with_accent(dark: bool, accent: Accent) -> Self {
        let mut theme = Self::new(dark);
        theme.accent = accent.color(dark);
        theme.on_accent = if dark { 0x18151f } else { 0xffffff };
        let blend = |shift: u32| -> u32 {
            (((theme.surface >> shift) & 255) as f32 * 0.86
                + ((theme.accent >> shift) & 255) as f32 * 0.14)
                .round() as u32
        };
        theme.selected = (blend(16) << 16) | (blend(8) << 8) | blend(0);
        theme
    }
    pub fn new(dark: bool) -> Self {
        if dark {
            Self {
                base: 0x191919,
                surface: 0x242424,
                raised: 0x323232,
                workspace: 0x101010,
                text: 0xf2f2f2,
                muted: 0xababab,
                border: 0x414141,
                accent: 0xefefef,
                on_accent: 0x202020,
                selected: 0x393939,
                danger: 0xf28f7c,
            }
        } else {
            Self {
                base: 0xf3f3f3,
                surface: 0xfafafa,
                raised: 0xe7e7e7,
                workspace: 0xd7d7d7,
                text: 0x232323,
                muted: 0x626262,
                border: 0xc7c7c7,
                accent: 0x282828,
                on_accent: 0xffffff,
                selected: 0xdddddd,
                danger: 0xa93427,
            }
        }
    }
}

pub struct Painter {
    pub frame: Frame,
    pub scale: f32,
    pub clip: Rect,
    pub font: UiFont,
    pub language: Language,
    pub font_scale: f32,
    pub limit: Option<Rect>,
}
impl Painter {
    pub fn new(width: u32, height: u32, scale: f32, color: u32) -> Self {
        Self {
            frame: Frame {
                width,
                height,
                pixels: vec![color; (width * height) as usize],
                texts: Vec::new(),
            },
            scale,
            clip: Rect::new(0., 0., width as f32 / scale, height as f32 / scale),
            font: UiFont::default(),
            language: Language::default(),
            font_scale: 1.,
            limit: None,
        }
    }
    fn effective_clip(&self) -> Rect {
        self.limit
            .map_or(self.clip, |limit| self.clip.intersection(limit))
    }

    /// Bounds for primitives which use pixel-center clipping. Keep the outer
    /// floor/ceil coverage of image/shadow drawing, then reject the clip-edge
    /// pixels once here instead of repeating logical-coordinate divisions and
    /// the optional animation-clip intersection for every destination pixel.
    fn pixel_bounds(&self, r: Rect) -> Option<(usize, usize, usize, usize)> {
        let clip = self.effective_clip();
        let sc = self.scale;
        let mut x0 = (r.x.max(clip.x) * sc).floor().max(0.) as usize;
        let mut y0 = (r.y.max(clip.y) * sc).floor().max(0.) as usize;
        let mut x1 = ((r.x + r.w).min(clip.x + clip.w) * sc)
            .ceil()
            .max(0.)
            .min(self.frame.width as f32) as usize;
        let mut y1 = ((r.y + r.h).min(clip.y + clip.h) * sc)
            .ceil()
            .max(0.)
            .min(self.frame.height as f32) as usize;
        // Use the same expression as pixel() at fractional DPI and clip edges;
        // rounding clip * scale - 0.5 can otherwise differ by a boundary pixel.
        while x0 < x1 && (x0 as f32 + 0.5) / sc < clip.x {
            x0 += 1;
        }
        while y0 < y1 && (y0 as f32 + 0.5) / sc < clip.y {
            y0 += 1;
        }
        while x1 > x0 && (x1 as f32 - 0.5) / sc >= clip.x + clip.w {
            x1 -= 1;
        }
        while y1 > y0 && (y1 as f32 - 0.5) / sc >= clip.y + clip.h {
            y1 -= 1;
        }
        (x0 < x1 && y0 < y1).then_some((x0, y0, x1, y1))
    }
    pub fn pixel(&mut self, x: i32, y: i32, color: u32, alpha: f32) {
        if x < 0 || y < 0 || x >= self.frame.width as i32 || y >= self.frame.height as i32 {
            return;
        }
        let lx = (x as f32 + 0.5) / self.scale;
        let ly = (y as f32 + 0.5) / self.scale;
        if !self.effective_clip().contains(lx, ly) {
            return;
        }
        blend_pixel(
            &mut self.frame.pixels[(y as u32 * self.frame.width + x as u32) as usize],
            color,
            alpha,
        );
    }
    pub fn rect(&mut self, r: Rect, color: u32) {
        let x0 = (r.x.max(self.effective_clip().x) * self.scale)
            .floor()
            .max(0.) as i32;
        let y0 = (r.y.max(self.effective_clip().y) * self.scale)
            .floor()
            .max(0.) as i32;
        let x1 = ((r.x + r.w).min(self.effective_clip().x + self.effective_clip().w) * self.scale)
            .ceil()
            .min(self.frame.width as f32) as i32;
        let y1 = ((r.y + r.h).min(self.effective_clip().y + self.effective_clip().h) * self.scale)
            .ceil()
            .min(self.frame.height as f32) as i32;
        if x1 <= x0 || y1 <= y0 {
            return;
        }
        for y in y0..y1 {
            let row = (y as u32 * self.frame.width) as usize;
            self.frame.pixels[row + x0 as usize..row + x1 as usize].fill(color);
        }
    }
    pub fn rounded(&mut self, r: Rect, radius: f32, color: u32) {
        if r.w <= 0. || r.h <= 0. {
            return;
        }
        let rr = radius.min(r.w / 2.).min(r.h / 2.).max(0.);
        let bounds = r.intersection(self.effective_clip());
        let sc = self.scale;
        let x0 = (bounds.x * sc - 0.5).ceil().max(0.) as i32;
        let x1 = ((bounds.x + bounds.w) * sc - 0.5)
            .ceil()
            .min(self.frame.width as f32) as i32;
        let y0 = (bounds.y * sc - 0.5).ceil().max(0.) as i32;
        let y1 = ((bounds.y + bounds.h) * sc - 0.5)
            .ceil()
            .min(self.frame.height as f32) as i32;
        if x1 <= x0 || y1 <= y0 {
            return;
        }
        let cx = r.x + r.w / 2.;
        let cy = r.y + r.h / 2.;
        for y in y0..y1 {
            let ly = (y as f32 + 0.5) / sc;
            if ly >= r.y + rr && ly <= r.y + r.h - rr {
                let row = y as usize * self.frame.width as usize;
                self.frame.pixels[row + x0 as usize..row + x1 as usize].fill(color);
                continue;
            }
            for x in x0..x1 {
                let qx = (((x as f32 + 0.5) / sc - cx).abs() - (r.w / 2. - rr)).max(0.);
                let qy = ((ly - cy).abs() - (r.h / 2. - rr)).max(0.);
                let a = (0.5 + (rr - qx.hypot(qy)) * sc).clamp(0., 1.);
                if a > 0. {
                    self.pixel(x, y, color, a);
                }
            }
        }
    }
    pub fn border(&mut self, r: Rect, color: u32) {
        self.rect(Rect::new(r.x, r.y, r.w, 1.), color);
        self.rect(Rect::new(r.x, r.y + r.h - 1., r.w, 1.), color);
        self.rect(Rect::new(r.x, r.y, 1., r.h), color);
        self.rect(Rect::new(r.x + r.w - 1., r.y, 1., r.h), color);
    }
    /// A low-opacity, softly falling shadow; the surface itself has no outline.
    pub fn soft_shadow(&mut self, r: Rect, radius: f32, opacity: f32) {
        let sigma = 5.;
        let spread = 16.;
        let shifted = Rect::new(r.x, r.y + 3., r.w, r.h);
        let cx = shifted.x + shifted.w / 2.;
        let cy = shifted.y + shifted.h / 2.;
        let bounds = shifted.inset(-spread);
        let Some((x0, y0, x1, y1)) = self.pixel_bounds(bounds) else {
            return;
        };
        let inside = r.inset(radius);
        let falloff = |distance: f32| opacity * (-0.5 * (distance.max(0.) / sigma).powi(2)).exp();
        let xs: Vec<_> = (x0..x1)
            .map(|x| {
                let lx = (x as f32 + 0.5) / self.scale;
                let qx = (lx - cx).abs() - (shifted.w / 2. - radius);
                (lx, qx, falloff(qx.max(0.) - radius))
            })
            .collect();
        let inside_x0 = xs.partition_point(|&(lx, _, _)| lx < inside.x);
        let inside_x1 = xs.partition_point(|&(lx, _, _)| lx < inside.x + inside.w);
        let stride = self.frame.width as usize;
        for y in y0..y1 {
            let ly = (y as f32 + 0.5) / self.scale;
            let qy = (ly - cy).abs() - (shifted.h / 2. - radius);
            let vertical_alpha = falloff(qy - radius);
            // The opaque panel covers its inset. Skip those entire row spans,
            // and reuse straight-edge falloff; only corners need hypot/exp.
            let spans = if ly >= inside.y && ly < inside.y + inside.h {
                [0..inside_x0, inside_x1..xs.len()]
            } else {
                [0..xs.len(), xs.len()..xs.len()]
            };
            for span in spans {
                let target = &mut self.frame.pixels
                    [y * stride + x0 + span.start..y * stride + x0 + span.end];
                for (&(_, qx, horizontal_alpha), pixel) in xs[span].iter().zip(target) {
                    let alpha = if qy <= 0. && radius >= 0. {
                        horizontal_alpha
                    } else if qx <= 0. && qy >= 0. {
                        vertical_alpha
                    } else {
                        falloff(qx.max(0.).hypot(qy.max(0.)) + qx.max(qy).min(0.) - radius)
                    };
                    if alpha > 0.001 {
                        blend_pixel(pixel, 0x000000, alpha);
                    }
                }
            }
        }
    }
    pub fn circle(&mut self, x: f32, y: f32, r: f32, color: u32, fill: bool) {
        if ![x, y, r].iter().all(|v| v.is_finite()) || r < 0. {
            return;
        }
        let clip = self.effective_clip();
        let sc = self.scale as f64;
        let (cx, cy, rad) = (x as f64, y as f64, r as f64);
        let edge = 1.2_f64.max(1.2 / sc);
        let outer = rad + edge;
        let inner = (rad - edge).max(0.);
        let top = ((cy - outer).max(clip.y as f64) * sc).floor().max(0.) as i32;
        let bottom = ((cy + outer).min((clip.y + clip.h) as f64) * sc)
            .ceil()
            .min(self.frame.height as f64) as i32;
        for py in top..bottom {
            let dy = (py as f64 + 0.5) / sc - cy;
            if dy.abs() > outer {
                continue;
            }
            let ox = (outer * outer - dy * dy).max(0.).sqrt();
            let ix = if !fill && dy.abs() < inner {
                (inner * inner - dy * dy).max(0.).sqrt()
            } else {
                0.
            };
            let spans = if ix > 0. {
                [(cx - ox, cx - ix), (cx + ix, cx + ox)]
            } else {
                [(cx - ox, cx + ox), (0., 0.)]
            };
            for (left, right) in spans {
                let x0 = (left.max(clip.x as f64) * sc).floor().max(0.) as i32;
                let x1 = (right.min((clip.x + clip.w) as f64) * sc)
                    .ceil()
                    .min(self.frame.width as f64) as i32;
                for px in x0..x1 {
                    let dx = (px as f64 + 0.5) / sc - cx;
                    let d = dx.hypot(dy);
                    let alpha = if fill {
                        0.5 + (rad - d) * sc
                    } else {
                        1.1 * sc - (d - rad).abs() * sc
                    };
                    if alpha > 0. {
                        self.pixel(px, py, color, alpha.clamp(0., 1.) as f32);
                    }
                }
            }
        }
    }
    pub fn line(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, width: f32, color: u32) {
        if ![x0, y0, x1, y1, width].iter().all(|v| v.is_finite()) || width <= 0. {
            return;
        }
        let (ax, ay, bx, by, r) = (
            x0 as f64,
            y0 as f64,
            x1 as f64,
            y1 as f64,
            width as f64 / 2.,
        );
        let dx = bx - ax;
        let dy = by - ay;
        let len2 = dx * dx + dy * dy;
        let clip = self.effective_clip();
        let sc = self.scale as f64;
        let edge = r + 1. / sc;
        let top = ((ay.min(by) - edge).max(clip.y as f64) * sc)
            .floor()
            .max(0.) as i32;
        let bottom = ((ay.max(by) + edge).min((clip.y + clip.h) as f64) * sc)
            .ceil()
            .min(self.frame.height as f64) as i32;
        for py in top..bottom {
            let y = (py as f64 + 0.5) / sc;
            let (lo, hi) = if dy.abs() > 1e-12 {
                let a = ((y - edge - ay) / dy).clamp(0., 1.);
                let b = ((y + edge - ay) / dy).clamp(0., 1.);
                let u = ax + a * dx;
                let v = ax + b * dx;
                (u.min(v) - edge, u.max(v) + edge)
            } else {
                (ax.min(bx) - edge, ax.max(bx) + edge)
            };
            let left = (lo.max(clip.x as f64) * sc).floor().max(0.) as i32;
            let right = (hi.min((clip.x + clip.w) as f64) * sc)
                .ceil()
                .min(self.frame.width as f64) as i32;
            for px in left..right {
                let x = (px as f64 + 0.5) / sc;
                let t = if len2 > 0. {
                    ((x - ax) * dx + (y - ay) * dy) / len2
                } else {
                    0.
                }
                .clamp(0., 1.);
                let d = (x - ax - t * dx).hypot(y - ay - t * dy);
                let a = (0.5 + (r - d) * sc).clamp(0., 1.);
                if a > 0. {
                    self.pixel(px, py, color, a as f32);
                }
            }
        }
    }
    pub fn polyline(&mut self, points: &[(f32, f32)], width: f32, color: u32) {
        for p in points.windows(2) {
            self.line(p[0].0, p[0].1, p[1].0, p[1].1, width, color);
        }
    }
    pub fn text(
        &mut self,
        r: Rect,
        text: impl Into<String>,
        size: f32,
        color: u32,
        bold: bool,
        serif: bool,
        align: TextAlign,
    ) {
        let text = self.language.translate(&text.into());
        self.text_raw(r, text, size, color, bold, serif, align);
    }
    pub fn text_raw(
        &mut self,
        r: Rect,
        text: impl Into<String>,
        size: f32,
        color: u32,
        bold: bool,
        serif: bool,
        align: TextAlign,
    ) {
        let clip = self.effective_clip();
        let size = size * self.font_scale;
        self.frame.texts.push(TextRun {
            x: r.x,
            y: r.y,
            width: r.w,
            height: r.h.max(size * 1.25),
            text: text.into(),
            size,
            color,
            bold,
            font: self.font,
            serif,
            align,
            clip: Some((clip.x, clip.y, clip.w, clip.h)),
        });
    }
    pub fn label_raw(
        &mut self,
        x: f32,
        y: f32,
        width: f32,
        text: impl Into<String>,
        size: f32,
        color: u32,
    ) {
        self.text_raw(
            Rect::new(x, y, width, size * 1.7),
            text,
            size,
            color,
            false,
            false,
            TextAlign::Left,
        );
    }
    pub fn label(
        &mut self,
        x: f32,
        y: f32,
        width: f32,
        text: impl Into<String>,
        size: f32,
        color: u32,
    ) {
        self.text(
            Rect::new(x, y, width, size * 1.7),
            text,
            size,
            color,
            false,
            false,
            TextAlign::Left,
        );
    }
    pub fn image(
        &mut self,
        r: Rect,
        width: u32,
        height: u32,
        pixels: &[Color],
        gray: bool,
        opacity: f32,
    ) {
        if width == 0
            || height == 0
            || pixels.len() < width as usize * height as usize
            || r.w <= 0.
            || r.h <= 0.
        {
            return;
        }
        let Some((x0, y0, x1, y1)) = self.pixel_bounds(r) else {
            return;
        };
        let smooth = r.w * self.scale / (width as f32) < 4.;
        let source_width = width as usize;
        // The source x coordinate is identical in every row. Reuse its precise
        // mapping, including rounding at source edges, for the whole image.
        let xs: Vec<_> = (x0..x1)
            .map(|x| {
                let fx = ((((x as f32 + 0.5) / self.scale - r.x) / r.w) * width as f32 - 0.5)
                    .clamp(0., width as f32 - 1.);
                let sx = fx.floor() as usize;
                (
                    sx,
                    (sx + 1).min(source_width - 1),
                    fx - sx as f32,
                    fx.round() as usize,
                )
            })
            .collect();
        let frame_width = self.frame.width as usize;
        for y in y0..y1 {
            let fy = ((((y as f32 + 0.5) / self.scale - r.y) / r.h) * height as f32 - 0.5)
                .clamp(0., height as f32 - 1.);
            let sy = fy.floor() as usize;
            let sy1 = (sy + 1).min(height as usize - 1);
            let ty = fy - sy as f32;
            let target = &mut self.frame.pixels[y * frame_width + x0..y * frame_width + x1];
            if smooth {
                let top = &pixels[sy * source_width..(sy + 1) * source_width];
                let bottom = &pixels[sy1 * source_width..(sy1 + 1) * source_width];
                for (&(sx, sx1, tx, _), target) in xs.iter().zip(target) {
                    let samples = [top[sx], top[sx1], bottom[sx], bottom[sx1]];
                    // Flat paint/backgrounds are common. Interpolating four
                    // identical RGBA samples yields that exact source color.
                    if samples[1..].iter().all(|c| *c == samples[0]) {
                        blend_image_pixel(target, samples[0], gray, opacity);
                        continue;
                    }
                    let weights = [
                        (1. - tx) * (1. - ty),
                        tx * (1. - ty),
                        (1. - tx) * ty,
                        tx * ty,
                    ];
                    let (mut alpha, mut red, mut green, mut blue) = (0., 0., 0., 0.);
                    for (i, c) in samples.into_iter().enumerate() {
                        let a = c.a as f32 * weights[i];
                        alpha += a;
                        red += c.r as f32 * a;
                        green += c.g as f32 * a;
                        blue += c.b as f32 * a;
                    }
                    let c = if alpha > 0. {
                        Color::rgba(
                            (red / alpha).round() as u8,
                            (green / alpha).round() as u8,
                            (blue / alpha).round() as u8,
                            alpha.round() as u8,
                        )
                    } else {
                        Color::TRANSPARENT
                    };
                    blend_image_pixel(target, c, gray, opacity);
                }
            } else {
                let source_y = fy.round() as usize;
                let source = &pixels[source_y * source_width..(source_y + 1) * source_width];
                for (&(_, _, _, nearest), target) in xs.iter().zip(target) {
                    blend_image_pixel(target, source[nearest], gray, opacity);
                }
            }
        }
    }
    pub fn triangle_up(&mut self, x: f32, base_y: f32, half: f32, height: f32, color: u32) {
        let steps = (height * self.scale).ceil().max(1.) as usize;
        for i in 0..steps {
            let v = (i as f32 + 0.5) / steps as f32;
            self.rect(
                Rect::new(
                    x - half * v,
                    base_y - height + v * height,
                    2. * half * v,
                    1. / self.scale,
                ),
                color,
            );
        }
    }
    pub fn triangle_down(&mut self, x: f32, base_y: f32, half: f32, height: f32, color: u32) {
        let steps = (height * self.scale).ceil().max(1.) as usize;
        for i in 0..steps {
            let v = (i as f32 + 0.5) / steps as f32;
            self.rect(
                Rect::new(
                    x - half * (1. - v),
                    base_y + v * height,
                    2. * half * (1. - v),
                    1. / self.scale,
                ),
                color,
            );
        }
    }
    pub fn icon(&mut self, r: Rect, name: &str, c: u32) {
        let x = r.x + r.w / 2. - 10.;
        let y = r.y + r.h / 2. - 10.;
        let mut pts = |v: &[(f32, f32)]| {
            let q: Vec<_> = v.iter().map(|(a, b)| (x + a, y + b)).collect();
            self.polyline(&q, 1.6, c);
        };
        match name {
            "left" => pts(&[(13., 4.), (7., 10.), (13., 16.)]),
            "right" => pts(&[(7., 4.), (13., 10.), (7., 16.)]),
            "brush" => {
                pts(&[(5., 14.), (14., 3.), (18., 7.), (8., 17.), (5., 14.)]);
                pts(&[(5., 14.), (2., 18.), (7., 18.)]);
            }
            "search" => {
                self.circle(x + 8., y + 8., 5.5, c, false);
                self.line(x + 12., y + 12., x + 18., y + 18., 1.6, c);
            }
            "pin" | "pin_active" => {
                pts(&[
                    (6., 2.),
                    (14., 2.),
                    (14., 4.),
                    (12.5, 4.),
                    (12.5, 9.),
                    (16., 12.),
                    (16., 13.),
                    (4., 13.),
                    (4., 12.),
                    (7.5, 9.),
                    (7.5, 4.),
                    (6., 4.),
                    (6., 2.),
                ]);
                self.line(x + 10., y + 13., x + 10., y + 19., 1.6, c);
                if name == "pin_active" {
                    self.rect(Rect::new(x + 7., y + 2., 6., 2.), c);
                    self.rect(Rect::new(x + 8., y + 4., 4., 6.), c);
                    self.triangle_up(x + 10., y + 13., 5., 4., c);
                }
            }
            "nib" => {
                pts(&[
                    (4., 3.),
                    (16., 3.),
                    (18., 10.),
                    (10., 19.),
                    (2., 10.),
                    (4., 3.),
                ]);
                pts(&[(10., 11.), (10., 18.)]);
                self.circle(x + 10., y + 9., 1.7, c, false);
            }
            "pencil" => {
                pts(&[
                    (3., 17.),
                    (5., 11.),
                    (14., 2.),
                    (18., 6.),
                    (9., 15.),
                    (3., 17.),
                ]);
                pts(&[(5., 11.), (9., 15.)]);
                pts(&[(12., 4.), (16., 8.)]);
                pts(&[(3., 17.), (5., 16.)]);
            }
            "soft" => {
                self.circle(x + 10., y + 10., 8., c, false);
                self.circle(x + 10., y + 10., 4.5, c, false);
                self.circle(x + 10., y + 10., 1.4, c, true);
            }
            "texture" => {
                for (px, py, radius) in [
                    (3., 4., 1.3),
                    (9., 3., 1.6),
                    (16., 4., 1.),
                    (5., 10., 1.8),
                    (12., 9., 1.1),
                    (18., 11., 1.3),
                    (2., 17., 1.),
                    (10., 16., 1.6),
                    (16., 17., 1.),
                ] {
                    self.circle(x + px, y + py, radius, c, true);
                }
            }
            "spray" => {
                pts(&[(2., 8.), (8., 8.), (8., 18.), (2., 18.), (2., 8.)]);
                pts(&[(3., 8.), (3., 5.), (7., 5.), (7., 8.)]);
                pts(&[(6., 5.), (9., 4.)]);
                for (px, py, radius) in [
                    (12., 4., 1.),
                    (17., 2., 1.2),
                    (15., 7., 1.4),
                    (19., 10., 1.),
                    (12., 11., 1.),
                    (17., 15., 1.2),
                ] {
                    self.circle(x + px, y + py, radius, c, true);
                }
            }
            "calligraphy" => {
                pts(&[
                    (3., 18.),
                    (5., 11.),
                    (9., 5.),
                    (16., 2.),
                    (18., 3.),
                    (15., 10.),
                    (9., 15.),
                    (3., 18.),
                ]);
                pts(&[(4., 17.), (16., 4.)]);
                pts(&[(8., 13.), (8., 9.)]);
                pts(&[(11., 10.), (15., 10.)]);
            }
            "flat" => {
                pts(&[(3., 2.), (17., 2.), (17., 10.), (3., 10.), (3., 2.)]);
                pts(&[(8., 10.), (7., 18.), (13., 18.), (12., 10.)]);
                pts(&[(6., 3.), (6., 8.)]);
                pts(&[(10., 3.), (10., 8.)]);
                pts(&[(14., 3.), (14., 8.)]);
            }
            "eraser" => {
                pts(&[(2., 12.), (12., 2.), (19., 9.), (9., 19.), (2., 12.)]);
                pts(&[(7., 7.), (14., 14.)]);
                pts(&[(9., 19.), (19., 19.)]);
            }
            "fill" => {
                pts(&[(3., 8.), (10., 1.), (18., 9.), (10., 17.), (3., 8.)]);
                pts(&[(3., 8.), (17., 8.)]);
                pts(&[(17., 14.), (19., 17.), (17., 20.), (15., 17.), (17., 14.)]);
            }
            "picker" => {
                pts(&[
                    (2., 18.),
                    (6., 12.),
                    (14., 4.),
                    (18., 8.),
                    (10., 16.),
                    (2., 18.),
                ]);
                pts(&[(11., 3.), (19., 11.)]);
            }
            "line" => {
                pts(&[(3., 17.), (17., 3.)]);
                self.circle(x + 3., y + 17., 1.8, c, true);
                self.circle(x + 17., y + 3., 1.8, c, true);
            }
            "rect" => {
                pts(&[(3., 3.), (17., 3.), (17., 17.), (3., 17.), (3., 3.)]);
            }
            "ellipse" => {
                self.circle(x + 10., y + 10., 7.5, c, false);
            }
            "text" => {
                pts(&[(3., 5.), (3., 3.), (17., 3.), (17., 5.)]);
                pts(&[(10., 3.), (10., 18.)]);
                pts(&[(6., 18.), (14., 18.)]);
            }
            "hand" => {
                pts(&[
                    (5., 10.),
                    (5., 5.),
                    (8., 5.),
                    (8., 1.),
                    (11., 1.),
                    (11., 5.),
                    (14., 4.),
                    (16., 6.),
                    (17., 12.),
                    (15., 18.),
                    (8., 19.),
                    (2., 12.),
                    (3., 10.),
                    (5., 11.),
                ]);
            }
            "mirror" => {
                pts(&[(10., 1.), (10., 19.)]);
                pts(&[(6., 4.), (2., 16.), (6., 16.), (6., 4.)]);
                pts(&[(14., 4.), (18., 16.), (14., 16.), (14., 4.)]);
            }
            "tone" => {
                self.circle(x + 10., y + 10., 8., c, false);
                self.line(x + 10., y + 3., x + 10., y + 17., 2., c);
                for a in 0..6 {
                    self.line(
                        x + 10.,
                        y + 4. + a as f32 * 2.,
                        x + 15.,
                        y + 4. + a as f32 * 2.,
                        1.,
                        c,
                    );
                }
            }
            "grid" => {
                pts(&[(2., 6.), (18., 6.)]);
                pts(&[(2., 14.), (18., 14.)]);
                pts(&[(6., 2.), (6., 18.)]);
                pts(&[(14., 2.), (14., 18.)]);
            }
            "undo" => {
                pts(&[(7., 3.), (2., 8.), (7., 12.)]);
                pts(&[
                    (3., 8.),
                    (13., 8.),
                    (17., 11.),
                    (17., 15.),
                    (14., 18.),
                    (9., 18.),
                ]);
            }
            "redo" => {
                pts(&[(13., 3.), (18., 8.), (13., 12.)]);
                pts(&[
                    (17., 8.),
                    (7., 8.),
                    (3., 11.),
                    (3., 15.),
                    (6., 18.),
                    (11., 18.),
                ]);
            }
            "sun" => {
                self.circle(x + 10., y + 10., 4., c, false);
                for i in 0..8 {
                    let a = i as f32 * std::f32::consts::PI / 4.;
                    self.line(
                        x + 7. * a.cos() + 10.,
                        y + 7. * a.sin() + 10.,
                        x + 9. * a.cos() + 10.,
                        y + 9. * a.sin() + 10.,
                        1.3,
                        c,
                    );
                }
            }
            "plus" => {
                pts(&[(10., 3.), (10., 17.)]);
                pts(&[(3., 10.), (17., 10.)]);
            }
            "minus" => {
                pts(&[(3., 10.), (17., 10.)]);
            }
            "up" => {
                pts(&[(4., 13.), (10., 7.), (16., 13.)]);
            }
            "down" => {
                pts(&[(4., 7.), (10., 13.), (16., 7.)]);
            }
            "eye" => {
                pts(&[
                    (1., 10.),
                    (5., 5.),
                    (10., 3.),
                    (15., 5.),
                    (19., 10.),
                    (15., 15.),
                    (10., 17.),
                    (5., 15.),
                    (1., 10.),
                ]);
                self.circle(x + 10., y + 10., 3., c, false);
            }
            "hidden" => {
                pts(&[(2., 18.), (18., 2.)]);
                pts(&[(2., 10.), (6., 5.), (10., 4.), (15., 6.)]);
                pts(&[(7., 16.), (12., 17.), (18., 10.)]);
            }
            "lock" => {
                pts(&[
                    (6., 9.),
                    (6., 5.),
                    (8., 2.),
                    (12., 2.),
                    (14., 5.),
                    (14., 9.),
                ]);
                pts(&[(3., 9.), (17., 9.), (17., 18.), (3., 18.), (3., 9.)]);
            }
            "unlock" => {
                pts(&[(7., 9.), (7., 5.), (9., 2.), (13., 2.), (16., 5.)]);
                pts(&[(3., 9.), (17., 9.), (17., 18.), (3., 18.), (3., 9.)]);
            }
            "copy" => {
                pts(&[(7., 7.), (18., 7.), (18., 18.), (7., 18.), (7., 7.)]);
                pts(&[(13., 4.), (13., 2.), (2., 2.), (2., 13.), (4., 13.)]);
            }
            "trash" => {
                pts(&[(3., 5.), (17., 5.)]);
                pts(&[(7., 5.), (7., 2.), (13., 2.), (13., 5.)]);
                pts(&[(5., 5.), (6., 18.), (14., 18.), (15., 5.)]);
                pts(&[(9., 8.), (9., 15.)]);
            }
            "fit" => {
                pts(&[(2., 7.), (2., 2.), (7., 2.)]);
                pts(&[(13., 2.), (18., 2.), (18., 7.)]);
                pts(&[(18., 13.), (18., 18.), (13., 18.)]);
                pts(&[(7., 18.), (2., 18.), (2., 13.)]);
            }
            "export" => {
                pts(&[(10., 2.), (10., 13.)]);
                pts(&[(6., 9.), (10., 13.), (14., 9.)]);
                pts(&[(3., 13.), (3., 18.), (17., 18.), (17., 13.)]);
            }
            "close" => {
                pts(&[(4., 4.), (16., 16.)]);
                pts(&[(16., 4.), (4., 16.)]);
            }
            "settings" => {
                // Three precise sliders are readable at small toolbar sizes.
                pts(&[(2., 5.), (7., 5.)]);
                pts(&[(12., 5.), (18., 5.)]);
                pts(&[(2., 10.), (11., 10.)]);
                pts(&[(16., 10.), (18., 10.)]);
                pts(&[(2., 15.), (4., 15.)]);
                pts(&[(9., 15.), (18., 15.)]);
                self.circle(x + 9.5, y + 5., 2.5, c, false);
                self.circle(x + 13.5, y + 10., 2.5, c, false);
                self.circle(x + 6.5, y + 15., 2.5, c, false);
            }
            "gear" => {
                let teeth: Vec<_> = (0..=32)
                    .map(|i| {
                        let a = i as f32 * std::f32::consts::TAU / 32.;
                        let radius = if i % 4 < 2 { 9. } else { 7. };
                        (10. + radius * a.cos(), 10. + radius * a.sin())
                    })
                    .collect();
                pts(&teeth);
                self.circle(x + 10., y + 10., 3., c, false);
            }
            "save" => {
                pts(&[
                    (3., 2.),
                    (15., 2.),
                    (18., 5.),
                    (18., 18.),
                    (3., 18.),
                    (3., 2.),
                ]);
                pts(&[(6., 2.), (6., 8.), (14., 8.), (14., 2.)]);
                pts(&[(6., 18.), (6., 12.), (14., 12.), (14., 18.)]);
            }
            "star" => {
                pts(&[
                    (10., 1.),
                    (12., 8.),
                    (19., 10.),
                    (12., 12.),
                    (10., 19.),
                    (8., 12.),
                    (1., 10.),
                    (8., 8.),
                    (10., 1.),
                ]);
            }
            _ => {
                self.circle(x + 10., y + 5., 1.5, c, true);
                self.circle(x + 10., y + 10., 1.5, c, true);
                self.circle(x + 10., y + 15., 1.5, c, true);
            }
        }
    }
}
#[inline]
fn blend_pixel(pixel: &mut u32, color: u32, alpha: f32) {
    let a = alpha.clamp(0., 1.);
    if a >= 0.999 {
        *pixel = color;
        return;
    }
    let mix = |shift: u32| -> u32 {
        (((color >> shift & 255) as f32 * a) + ((*pixel >> shift & 255) as f32 * (1. - a))).round()
            as u32
    };
    *pixel = (mix(16) << 16) | (mix(8) << 8) | mix(0);
}

#[inline]
fn blend_image_pixel(pixel: &mut u32, c: Color, gray: bool, opacity: f32) {
    let color = if gray {
        let g = (c.r as f32 * 0.2126 + c.g as f32 * 0.7152 + c.b as f32 * 0.0722) as u32;
        g * 0x010101
    } else {
        rgb(c)
    };
    blend_pixel(pixel, color, c.a as f32 / 255. * opacity);
}

pub fn rgb(c: Color) -> u32 {
    (c.r as u32) << 16 | (c.g as u32) << 8 | c.b as u32
}
pub fn hex(c: Color) -> String {
    format!("#{:02X}{:02X}{:02X}", c.r, c.g, c.b)
}
pub fn parse_hex(s: &str) -> Option<Color> {
    let s = s.trim().trim_start_matches('#');
    if s.len() != 6 {
        return None;
    }
    let v = u32::from_str_radix(s, 16).ok()?;
    Some(Color::rgb((v >> 16) as u8, (v >> 8) as u8, v as u8))
}
pub fn hsv(h: f32, s: f32, v: f32) -> Color {
    let h = (h.rem_euclid(360.) / 60.).min(5.99999);
    let c = v * s;
    let x = c * (1. - (h.rem_euclid(2.) - 1.).abs());
    let m = v - c;
    let (r, g, b) = match h as u32 {
        0 => (c, x, 0.),
        1 => (x, c, 0.),
        2 => (0., c, x),
        3 => (0., x, c),
        4 => (x, 0., c),
        _ => (c, 0., x),
    };
    Color::rgb(
        ((r + m) * 255.).round() as u8,
        ((g + m) * 255.).round() as u8,
        ((b + m) * 255.).round() as u8,
    )
}
pub fn to_hsv(c: Color, old_h: f32) -> (f32, f32, f32) {
    let (r, g, b) = (c.r as f32 / 255., c.g as f32 / 255., c.b as f32 / 255.);
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let d = max - min;
    let h = if d < 0.00001 {
        old_h
    } else if max == r {
        60. * ((g - b) / d).rem_euclid(6.)
    } else if max == g {
        60. * ((b - r) / d + 2.)
    } else {
        60. * ((r - g) / d + 4.)
    };
    (h, if max == 0. { 0. } else { d / max }, max)
}

pub fn write_preview(frame: &Frame, path: &Path) -> std::io::Result<()> {
    fs::create_dir_all(path)?;
    let mut f = fs::File::create(path.join("frame.ppm"))?;
    write!(f, "P6\n{} {}\n255\n", frame.width, frame.height)?;
    let mut bytes = Vec::with_capacity(frame.pixels.len() * 3);
    for c in &frame.pixels {
        bytes.extend_from_slice(&[(c >> 16) as u8, (c >> 8) as u8, *c as u8]);
    }
    f.write_all(&bytes)?;
    let mut out = String::from("[");
    for (i, t) in frame.texts.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        let escaped = t
            .text
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', "\\n")
            .replace('\r', "");
        out.push_str(&format!("{{\"x\":{},\"y\":{},\"w\":{},\"h\":{},\"size\":{},\"color\":{},\"bold\":{},\"serif\":{},\"font\":\"{}\",\"align\":\"{}\",\"text\":\"{}\",\"clip\":{}}}",t.x,t.y,t.width,t.height,t.size,t.color,t.bold,t.serif,t.font.key(),match t.align{TextAlign::Left=>"left",TextAlign::Center=>"center",TextAlign::Right=>"right"},escaped,match t.clip{Some((x,y,w,h))=>format!("[{x},{y},{w},{h}]"),None=>"null".into()}));
    }
    out.push(']');
    fs::write(path.join("text.json"), out)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hex_and_hsv_stay_consistent() {
        for c in [
            Color::rgb(255, 0, 0),
            Color::rgb(39, 122, 214),
            Color::rgb(0, 0, 0),
            Color::rgb(255, 255, 255),
        ] {
            assert_eq!(parse_hex(&hex(c)), Some(c));
            let (h, s, v) = to_hsv(c, 230.);
            let d = hsv(h, s, v);
            assert!((c.r as i16 - d.r as i16).abs() <= 1);
            assert!((c.g as i16 - d.g as i16).abs() <= 1);
            assert!((c.b as i16 - d.b as i16).abs() <= 1);
        }
    }
    #[test]
    fn achromatic_color_remembers_hue() {
        assert_eq!(to_hsv(Color::rgb(88, 88, 88), 210.).0, 210.);
        assert_eq!(hsv(360., 1., 1.), hsv(0., 1., 1.));
        assert!(parse_hex("#GG0000").is_none());
    }
}
