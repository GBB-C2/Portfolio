//! Dependency-free raster document, transactional edits, and portable file formats.
//!
//! Layer order is bottom to top. Pixel buffers contain straight (not premultiplied)
//! sRGB RGBA. The editor limits dimensions to 4096, total layer pixels to 64 Mi,
//! and retained undo patches to 128 MiB / 256 edits. History stores only changed
//! 64×64 pixel tiles; ordinary paint gestures capture only tiles they actually touch.

use std::collections::VecDeque;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

pub const MAX_DIMENSION: u32 = 4096;
pub const MAX_LAYERS: usize = 32;
pub const MAX_LAYER_PIXELS: usize = 64 * 1024 * 1024;
pub const HISTORY_BYTES: usize = 128 * 1024 * 1024;
pub const MAX_HISTORY_STEPS: usize = 256;
const TILE_SIZE: usize = 64;
const MAX_FILE_BYTES: u64 = (MAX_LAYER_PIXELS * 4 + 1024 * 1024) as u64;
#[path = "document_text.rs"]
mod document_text;
pub use document_text::{ParagraphAlign, TextLayer};

const TEXT_MAGIC: &[u8; 8] = b"PRTFOL02";
const MAGIC: &[u8; 8] = b"PRTFOL01";

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    pub const TRANSPARENT: Self = Self::rgba(0, 0, 0, 0);
    pub const WHITE: Self = Self::rgb(255, 255, 255);
    pub const BLACK: Self = Self::rgb(0, 0, 0);

    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self::rgba(r, g, b, 255)
    }

    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    pub const fn from_hex(hex: u32) -> Self {
        Self::rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
    }

    pub fn to_hex(self) -> String {
        format!("#{:02X}{:02X}{:02X}", self.r, self.g, self.b)
    }

    pub fn parse_hex(text: &str) -> Option<Self> {
        let digits = text.trim().trim_start_matches('#');
        if digits.len() != 6 || !digits.is_ascii() {
            return None;
        }
        let value = u32::from_str_radix(digits, 16).ok()?;
        Some(Self::from_hex(value))
    }
}

/// Source-over compositing using straight alpha. `coverage` is 0..=255.
pub fn blend(dst: Color, src: Color, coverage: u8) -> Color {
    let sa = (src.a as u32 * coverage as u32 + 127) / 255;
    if sa == 0 {
        return dst;
    }
    if sa == 255 {
        return Color { a: 255, ..src };
    }
    let da = dst.a as u32;
    if da == 0 {
        return Color { a: sa as u8, ..src };
    }
    let a255 = sa * 255 + da * (255 - sa);
    if a255 == 0 {
        return Color::TRANSPARENT;
    }
    if dst.r == src.r && dst.g == src.g && dst.b == src.b {
        return Color {
            a: ((a255 + 127) / 255).min(255) as u8,
            ..src
        };
    }
    if da == 255 {
        let channel = |s: u8, d: u8| ((s as u32 * sa + d as u32 * (255 - sa) + 127) / 255) as u8;
        return Color::rgba(
            channel(src.r, dst.r),
            channel(src.g, dst.g),
            channel(src.b, dst.b),
            255,
        );
    }
    let channel = |s: u8, d: u8| -> u8 {
        ((s as u32 * sa * 255 + d as u32 * da * (255 - sa) + a255 / 2) / a255).min(255) as u8
    };
    Color::rgba(
        channel(src.r, dst.r),
        channel(src.g, dst.g),
        channel(src.b, dst.b),
        ((a255 + 127) / 255).min(255) as u8,
    )
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Layer {
    pub text: Option<TextLayer>,
    pub id: u64,
    pub name: String,
    pub visible: bool,
    pub locked: bool,
    pub opacity: u8,
    pub pixels: Vec<Color>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BrushKind {
    Ink,
    Soft,
    Marker,
    Pencil,
    Charcoal,
    Chalk,
    Airbrush,
    Calligraphy,
    Spray,
    Flat,
}

impl BrushKind {
    pub const ALL: [Self; 10] = [
        Self::Ink,
        Self::Soft,
        Self::Marker,
        Self::Pencil,
        Self::Charcoal,
        Self::Chalk,
        Self::Airbrush,
        Self::Calligraphy,
        Self::Spray,
        Self::Flat,
    ];
    pub const fn index(self) -> u8 {
        self as u8
    }
    pub fn from_index(index: u8) -> Self {
        Self::ALL.get(index as usize).copied().unwrap_or(Self::Ink)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShapeKind {
    Line,
    Rectangle,
    Ellipse,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Brush {
    pub color: Color,
    pub size: f32,
    pub opacity: f32,
    pub hardness: f32,
    pub flow: f32,
    pub spacing: f32,
    pub angle: f32,
    pub scatter: f32,
    pub roundness: f32,
    pub grain: f32,
    pub kind: BrushKind,
    pub erase: bool,
    pub mirror: bool,
}

impl Default for Brush {
    fn default() -> Self {
        Self {
            color: Color::from_hex(0xd3e98a),
            size: 20.0,
            opacity: 1.0,
            hardness: 1.0,
            flow: 1.0,
            spacing: 0.12,
            angle: 0.0,
            scatter: 0.0,
            roundness: 1.0,
            grain: 0.0,
            kind: BrushKind::Ink,
            erase: false,
            mirror: false,
        }
    }
}

impl Brush {
    pub fn sanitized(self) -> Self {
        fn finite(value: f32, default: f32, min: f32, max: f32) -> f32 {
            if value.is_finite() {
                value.clamp(min, max)
            } else {
                default
            }
        }
        Self {
            size: if self.size.is_finite() {
                self.size.clamp(1.0, 512.0)
            } else {
                20.0
            },
            opacity: if self.opacity.is_finite() {
                self.opacity.clamp(0.0, 1.0)
            } else {
                1.0
            },
            hardness: finite(self.hardness, 1.0, 0.0, 1.0),
            flow: finite(self.flow, 1.0, 0.0, 1.0),
            spacing: finite(self.spacing, 0.12, 0.02, 2.0),
            angle: if self.angle.is_finite() {
                self.angle.rem_euclid(360.0)
            } else {
                0.0
            },
            roundness: finite(self.roundness, 1.0, 0.1, 1.0),
            grain: finite(self.grain, 0.0, 0.0, 1.0),
            scatter: finite(self.scatter, 0.0, 0.0, 1.0),
            ..self
        }
    }
}

#[derive(Clone)]
struct Snapshot {
    layers: Vec<Layer>,
    active: usize,
    title: String,
    revision: u64,
}

#[derive(Clone)]
struct LayerMetadata {
    text: Option<TextLayer>,
    id: u64,
    name: String,
    visible: bool,
    locked: bool,
    opacity: u8,
}

#[derive(Clone)]
struct StateMetadata {
    layers: Vec<LayerMetadata>,
    active: usize,
    title: String,
    revision: u64,
}

impl StateMetadata {
    fn from_parts(layers: &[Layer], active: usize, title: &str, revision: u64) -> Self {
        Self {
            layers: layers
                .iter()
                .map(|l| LayerMetadata {
                    text: l.text.clone(),
                    id: l.id,
                    name: l.name.clone(),
                    visible: l.visible,
                    locked: l.locked,
                    opacity: l.opacity,
                })
                .collect(),
            active,
            title: title.to_owned(),
            revision,
        }
    }

    fn bytes(&self) -> usize {
        self.layers
            .iter()
            .map(|l| {
                l.name.len()
                    + l.text
                        .as_ref()
                        .map_or(0, |t| t.body.len() + t.font.len() + 48)
                    + 64
            })
            .sum::<usize>()
            + self.title.len()
            + 64
    }

    fn differs(&self, layers: &[Layer], title: &str) -> bool {
        self.title != title
            || self.layers.len() != layers.len()
            || self.layers.iter().zip(layers).any(|(a, b)| {
                a.id != b.id
                    || a.text != b.text
                    || a.name != b.name
                    || a.visible != b.visible
                    || a.locked != b.locked
                    || a.opacity != b.opacity
            })
    }

    fn restore_layers(&self, layers: &mut [Layer]) {
        // Metadata transactions may reorder layers. Move their owned buffers,
        // without reading or copying their pixels, back to the saved order.
        layers.sort_unstable_by_key(|layer| {
            self.layers
                .iter()
                .position(|meta| meta.id == layer.id)
                .unwrap_or(usize::MAX)
        });
        for layer in layers {
            if let Some(meta) = self.layers.iter().find(|meta| meta.id == layer.id) {
                layer.text.clone_from(&meta.text);
                layer.name.clone_from(&meta.name);
                layer.visible = meta.visible;
                layer.locked = meta.locked;
                layer.opacity = meta.opacity;
            }
        }
    }
}

struct TilePatch {
    x: usize,
    y: usize,
    width: usize,
    height: usize,
    xor: Vec<Color>,
}

enum PixelDelta {
    Tiles(Vec<TilePatch>),
    // A layer appearing or disappearing needs its entire pixel buffer once.
    Whole(Vec<Color>),
}

struct LayerDelta {
    id: u64,
    pixels: PixelDelta,
}

struct HistoryEntry {
    before: StateMetadata,
    after: StateMetadata,
    layers: Vec<LayerDelta>,
}

impl HistoryEntry {
    fn between(before: Snapshot, after: &Document) -> Self {
        let before_meta = StateMetadata::from_parts(
            &before.layers,
            before.active,
            &before.title,
            before.revision,
        );
        let after_meta =
            StateMetadata::from_parts(&after.layers, after.active, &after.title, after.revision);
        let mut changes = Vec::new();
        for old in before.layers {
            if let Some(new) = after.layers.iter().find(|layer| layer.id == old.id) {
                let tiles = changed_tiles(
                    &old.pixels,
                    &new.pixels,
                    after.width as usize,
                    after.height as usize,
                );
                if !tiles.is_empty() {
                    changes.push(LayerDelta {
                        id: old.id,
                        pixels: PixelDelta::Tiles(tiles),
                    });
                }
            } else {
                changes.push(LayerDelta {
                    id: old.id,
                    pixels: PixelDelta::Whole(old.pixels),
                });
            }
        }
        for layer in &after.layers {
            if !before_meta.layers.iter().any(|old| old.id == layer.id) {
                changes.push(LayerDelta {
                    id: layer.id,
                    pixels: PixelDelta::Whole(layer.pixels.clone()),
                });
            }
        }
        Self {
            before: before_meta,
            after: after_meta,
            layers: changes,
        }
    }

    fn bytes(&self) -> usize {
        self.before.bytes()
            + self.after.bytes()
            + self
                .layers
                .iter()
                .map(|l| match &l.pixels {
                    PixelDelta::Whole(pixels) => pixels.len() * 4 + 64,
                    PixelDelta::Tiles(tiles) => {
                        tiles.iter().map(|t| t.xor.len() * 4 + 64).sum::<usize>() + 64
                    }
                })
                .sum::<usize>()
    }

    fn apply(&self, doc: &mut Document, forward: bool) {
        let target = if forward { &self.after } else { &self.before };
        let mut current = std::mem::take(&mut doc.layers);
        let mut restored = Vec::with_capacity(target.layers.len());
        for meta in &target.layers {
            let delta = self.layers.iter().find(|d| d.id == meta.id);
            let mut pixels = if let Some(index) = current.iter().position(|l| l.id == meta.id) {
                current.swap_remove(index).pixels
            } else {
                match delta.map(|d| &d.pixels) {
                    Some(PixelDelta::Whole(pixels)) => pixels.clone(),
                    _ => vec![Color::TRANSPARENT; doc.width as usize * doc.height as usize],
                }
            };
            if let Some(LayerDelta {
                pixels: PixelDelta::Tiles(tiles),
                ..
            }) = delta
            {
                for tile in tiles {
                    for row in 0..tile.height {
                        let start = (tile.y + row) * doc.width as usize + tile.x;
                        for col in 0..tile.width {
                            let xor = tile.xor[row * tile.width + col];
                            let p = &mut pixels[start + col];
                            p.r ^= xor.r;
                            p.g ^= xor.g;
                            p.b ^= xor.b;
                            p.a ^= xor.a;
                        }
                    }
                }
            }
            restored.push(Layer {
                text: meta.text.clone(),
                id: meta.id,
                name: meta.name.clone(),
                visible: meta.visible,
                locked: meta.locked,
                opacity: meta.opacity,
                pixels,
            });
        }
        doc.layers = restored;
        doc.active = target.active.min(doc.layers.len().saturating_sub(1));
        doc.title = target.title.clone();
        doc.revision = target.revision;
        doc.stroke = None;
        doc.shape_preview = None;
    }
}

fn changed_tiles(before: &[Color], after: &[Color], width: usize, height: usize) -> Vec<TilePatch> {
    let mut changed = Vec::new();
    for y in (0..height).step_by(TILE_SIZE) {
        let tile_height = TILE_SIZE.min(height - y);
        for x in (0..width).step_by(TILE_SIZE) {
            let tile_width = TILE_SIZE.min(width - x);
            let differs = (0..tile_height).any(|row| {
                let at = (y + row) * width + x;
                before[at..at + tile_width] != after[at..at + tile_width]
            });
            if !differs {
                continue;
            }
            let mut xor = Vec::with_capacity(tile_width * tile_height);
            for row in 0..tile_height {
                let at = (y + row) * width + x;
                for col in 0..tile_width {
                    let a = before[at + col];
                    let b = after[at + col];
                    xor.push(Color::rgba(a.r ^ b.r, a.g ^ b.g, a.b ^ b.b, a.a ^ b.a));
                }
            }
            changed.push(TilePatch {
                x,
                y,
                width: tile_width,
                height: tile_height,
                xor,
            });
        }
    }
    changed
}

struct SavedTile {
    x: usize,
    y: usize,
    width: usize,
    height: usize,
    pixels: Vec<Color>,
}

struct RasterEdit {
    before: StateMetadata,
    layer_id: u64,
    columns: usize,
    tiles: Vec<Option<SavedTile>>,
    // Shape previews recycle their previous tile buffers as the geometry moves.
    // This keeps only one preview's baseline, rather than every visited region.
    recycled_tiles: Vec<Vec<Color>>,
    changed: bool,
}

impl RasterEdit {
    fn new(doc: &Document) -> Self {
        let columns = (doc.width as usize).div_ceil(TILE_SIZE);
        let rows = (doc.height as usize).div_ceil(TILE_SIZE);
        Self {
            before: StateMetadata::from_parts(&doc.layers, doc.active, &doc.title, doc.revision),
            layer_id: doc.active_layer().id,
            columns,
            tiles: (0..columns * rows).map(|_| None).collect(),
            recycled_tiles: Vec::new(),
            changed: false,
        }
    }

    fn capture(&mut self, layer: &Layer, doc_width: usize, doc_height: usize, x: usize, y: usize) {
        let at = (y / TILE_SIZE) * self.columns + x / TILE_SIZE;
        if self.tiles[at].is_some() {
            return;
        }
        let x = x / TILE_SIZE * TILE_SIZE;
        let y = y / TILE_SIZE * TILE_SIZE;
        let width = TILE_SIZE.min(doc_width - x);
        let height = TILE_SIZE.min(doc_height - y);
        let mut pixels = self.recycled_tiles.pop().unwrap_or_default();
        pixels.clear();
        pixels.reserve(width * height);
        for row in 0..height {
            let start = (y + row) * doc_width + x;
            pixels.extend_from_slice(&layer.pixels[start..start + width]);
        }
        self.tiles[at] = Some(SavedTile {
            x,
            y,
            width,
            height,
            pixels,
        });
    }

    fn restore_pixels(&self, layers: &mut [Layer], doc_width: usize) {
        if let Some(layer) = layers.iter_mut().find(|l| l.id == self.layer_id) {
            for tile in self.tiles.iter().flatten() {
                for row in 0..tile.height {
                    let start = (tile.y + row) * doc_width + tile.x;
                    layer.pixels[start..start + tile.width]
                        .copy_from_slice(&tile.pixels[row * tile.width..(row + 1) * tile.width]);
                }
            }
        }
    }

    /// Restore the previous shape without ending its transaction. Its saved
    /// tile buffers can then capture the new geometry from the same baseline.
    fn reset_preview(
        &mut self,
        layers: &mut [Layer],
        doc_width: usize,
    ) -> Option<(u32, u32, u32, u32)> {
        let layer = layers.iter_mut().find(|l| l.id == self.layer_id)?;
        let mut bounds: Option<(usize, usize, usize, usize)> = None;
        for slot in &mut self.tiles {
            let Some(tile) = slot.take() else {
                continue;
            };
            for row in 0..tile.height {
                let start = (tile.y + row) * doc_width + tile.x;
                layer.pixels[start..start + tile.width]
                    .copy_from_slice(&tile.pixels[row * tile.width..(row + 1) * tile.width]);
            }
            let right = tile.x + tile.width;
            let bottom = tile.y + tile.height;
            bounds = Some(match bounds {
                Some((x0, y0, x1, y1)) => (
                    x0.min(tile.x),
                    y0.min(tile.y),
                    x1.max(right),
                    y1.max(bottom),
                ),
                None => (tile.x, tile.y, right, bottom),
            });
            self.recycled_tiles.push(tile.pixels);
        }
        self.changed = false;
        bounds.map(|(x0, y0, x1, y1)| (x0 as u32, y0 as u32, (x1 - x0) as u32, (y1 - y0) as u32))
    }

    fn patches(&self, layer: &Layer, doc_width: usize) -> Vec<TilePatch> {
        let mut patches = Vec::new();
        for tile in self.tiles.iter().flatten() {
            let mut xor = Vec::with_capacity(tile.pixels.len());
            let mut changed = false;
            for row in 0..tile.height {
                let at = (tile.y + row) * doc_width + tile.x;
                for col in 0..tile.width {
                    let before = tile.pixels[row * tile.width + col];
                    let after = layer.pixels[at + col];
                    changed |= before != after;
                    xor.push(Color::rgba(
                        before.r ^ after.r,
                        before.g ^ after.g,
                        before.b ^ after.b,
                        before.a ^ after.a,
                    ));
                }
            }
            if changed {
                patches.push(TilePatch {
                    x: tile.x,
                    y: tile.y,
                    width: tile.width,
                    height: tile.height,
                    xor,
                });
            }
        }
        patches
    }
}

enum PendingEdit {
    Snapshot(Snapshot),
    Raster(RasterEdit),
    Metadata(StateMetadata),
}

const MAX_STROKE_SAMPLES: usize = 8 * 1024 * 1024;

struct BrushMask {
    side: usize,
    coverage: Vec<u8>,
    alpha: [u8; 256],
    alpha_key: Option<(f32, f32, f32)>,
    row_bounds: Vec<(usize, usize)>,
}

fn grain(x: i32, y: i32, seed: u32) -> f32 {
    let mut n = (x as u32).wrapping_mul(0x8da6b343) ^ (y as u32).wrapping_mul(0xd8163841) ^ seed;
    n ^= n >> 16;
    n = n.wrapping_mul(0x7feb352d);
    n ^= n >> 15;
    n = n.wrapping_mul(0x846ca68b);
    n ^= n >> 16;
    (n & 0xffff) as f32 / 65535.0
}

impl BrushMask {
    fn new(brush: Brush) -> Self {
        // Rotation can expand a rectangular nib beyond its nominal diameter.
        let side = (brush.size * 1.08).ceil() as usize + 2;
        let mut coverage = vec![0; side * side];
        let center = side as f32 * 0.5;
        let radius = brush.size * 0.5;
        let (sin, cos) = brush.angle.to_radians().sin_cos();
        for y in 0..side {
            for x in 0..side {
                let dx = x as f32 + 0.5 - center;
                let dy = y as f32 + 0.5 - center;
                let u = cos * dx + sin * dy;
                let v = (-sin * dx + cos * dy) / brush.roundness;
                let distance = if brush.roundness == 1.0 {
                    dx.hypot(dy)
                } else {
                    u.hypot(v)
                };
                let radial = distance / radius.max(0.5);
                let aa = (radius + 0.5 - distance).clamp(0.0, 1.0);
                let softness = if radial <= brush.hardness || brush.hardness >= 0.999 {
                    1.0
                } else {
                    let t = ((1.0 - radial) / (1.0 - brush.hardness)).clamp(0.0, 1.0);
                    t * t * (3.0 - 2.0 * t)
                };
                let noise = grain(
                    u.floor() as i32,
                    v.floor() as i32,
                    brush.kind.index() as u32 + 17,
                );
                let edge = |a: f32, extent: f32| (extent + 0.5 - a.abs()).clamp(0.0, 1.0);
                let value = match brush.kind {
                    BrushKind::Ink => aa * softness,
                    BrushKind::Soft => aa * softness * (1.0 - radial).max(0.0).powi(2) * 0.27,
                    BrushKind::Marker => edge(u, radius) * edge(v, radius * 0.38) * softness * 0.28,
                    BrushKind::Pencil => {
                        aa * softness * (0.35 + noise * 0.65) * (1.0 - radial * 0.5).max(0.0)
                    }
                    BrushKind::Charcoal => {
                        aa * softness
                            * (1.0 - radial * 0.65).max(0.0)
                            * if noise > 0.28 {
                                0.3 + noise * 0.65
                            } else {
                                0.0
                            }
                    }
                    BrushKind::Chalk => {
                        aa * softness
                            * if noise > 0.36 {
                                0.55 + noise * 0.4
                            } else {
                                0.0
                            }
                    }
                    BrushKind::Airbrush => aa * softness * (-4.5 * radial * radial).exp() * 0.13,
                    BrushKind::Calligraphy => {
                        let d = (u / radius.max(0.5)).hypot(v / (radius * 0.18).max(0.5));
                        ((1.0 - d) * radius.min(3.0) + 0.5).clamp(0.0, 1.0) * softness
                    }
                    BrushKind::Spray => {
                        aa * softness
                            * if noise > 0.89 {
                                (noise - 0.89) / 0.11
                            } else {
                                0.0
                            }
                    }
                    BrushKind::Flat => {
                        edge(u, radius * 0.96)
                            * edge(v, radius * 0.27)
                            * softness
                            * (0.72 + grain(u.floor() as i32, 0, 91) * 0.28)
                    }
                };
                let value = value * (1.0 - brush.grain * noise);
                coverage[y * side + x] = (value * 255.0 + 0.5).clamp(0.0, 255.0) as u8;
            }
        }
        let mut mask = Self {
            side,
            coverage,
            alpha: [0; 256],
            alpha_key: None,
            row_bounds: vec![(0, 0); side],
        };
        mask.prepare_alpha(brush, 1.0);
        mask
    }

    /// Keep the exact deposition table used by every stamp in this run.
    /// Bounds exclude only zero alpha; source alpha is deliberately not folded
    /// in because an eraser can normalize invisible RGB even at zero strength.
    fn prepare_alpha(&mut self, brush: Brush, compensation: f32) {
        let compensation = if compensation > 1.0 {
            compensation.min(64.0)
        } else {
            1.0
        };
        let key = (brush.opacity, brush.flow, compensation);
        if self.alpha_key == Some(key) {
            return;
        }
        for (n, out) in self.alpha.iter_mut().enumerate() {
            let deposit = n as f32 / 255.0 * brush.opacity * brush.flow;
            let deposit = if compensation > 1.0 {
                1.0 - (1.0 - deposit).powf(compensation)
            } else {
                deposit
            };
            *out = (deposit * 255.0 + 0.5).clamp(0.0, 255.0) as u8;
        }
        for (row, bounds) in self
            .coverage
            .chunks_exact(self.side)
            .zip(&mut self.row_bounds)
        {
            *bounds = match row.iter().position(|&n| self.alpha[n as usize] != 0) {
                Some(start) => {
                    let end = row
                        .iter()
                        .rposition(|&n| self.alpha[n as usize] != 0)
                        .unwrap()
                        + 1;
                    (start, end)
                }
                None => (0, 0),
            };
        }
        self.alpha_key = Some(key);
    }
}

struct StrokeState {
    brush: Brush,
    mask: BrushMask,
    last_x: f32,
    last_y: f32,
    last_stamp: (f32, f32),
    remainder: f64,
    stamp_index: u32,
}

#[derive(Clone, Copy, PartialEq)]
struct ShapeGeometry {
    kind: ShapeKind,
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
    brush: Brush,
    filled: bool,
}

impl ShapeGeometry {
    fn new(
        kind: ShapeKind,
        x0: f32,
        y0: f32,
        x1: f32,
        y1: f32,
        brush: Brush,
        filled: bool,
    ) -> Result<Self, String> {
        if [x0, y0, x1, y1].iter().any(|n| !n.is_finite()) {
            return Err("Некорректные координаты фигуры.".into());
        }
        Ok(Self {
            kind,
            x0,
            y0,
            x1,
            y1,
            brush: brush.sanitized(),
            filled: filled && kind != ShapeKind::Line,
        })
    }
}

struct ShapePreview {
    geometry: ShapeGeometry,
    layer_id: u64,
    width: u32,
    height: u32,
    mask: BrushMask,
}

/// Clip in the dominant coordinate, not by nearly equal t parameters. With
/// endpoints near ±f32::MAX a parametric [t0,t1] can collapse to [0.5,0.5].
fn clip_paint_segment(
    x0: f64,
    y0: f64,
    x1: f64,
    y1: f64,
    width: f64,
    height: f64,
    margin: f64,
) -> Option<(f64, f64, f64, f64)> {
    let dx = x1 - x0;
    let dy = y1 - y0;
    if dx.abs() >= dy.abs() && dx != 0.0 {
        let slope = dy / dx;
        let intercept = (-slope).mul_add(x0, y0);
        let mut lo = x0.min(x1).max(-margin);
        let mut hi = x0.max(x1).min(width + margin);
        if slope == 0.0 {
            if intercept < -margin || intercept > height + margin {
                return None;
            }
        } else {
            let a = (-margin - intercept) / slope;
            let b = (height + margin - intercept) / slope;
            lo = lo.max(a.min(b));
            hi = hi.min(a.max(b));
        }
        if lo > hi {
            return None;
        }
        let (a, b) = if dx >= 0.0 { (lo, hi) } else { (hi, lo) };
        Some((
            a,
            slope.mul_add(a, intercept),
            b,
            slope.mul_add(b, intercept),
        ))
    } else if dy != 0.0 {
        let slope = dx / dy;
        let intercept = (-slope).mul_add(y0, x0);
        let mut lo = y0.min(y1).max(-margin);
        let mut hi = y0.max(y1).min(height + margin);
        if slope == 0.0 {
            if intercept < -margin || intercept > width + margin {
                return None;
            }
        } else {
            let a = (-margin - intercept) / slope;
            let b = (width + margin - intercept) / slope;
            lo = lo.max(a.min(b));
            hi = hi.min(a.max(b));
        }
        if lo > hi {
            return None;
        }
        let (a, b) = if dy >= 0.0 { (lo, hi) } else { (hi, lo) };
        Some((
            slope.mul_add(a, intercept),
            a,
            slope.mul_add(b, intercept),
            b,
        ))
    } else {
        None
    }
}

pub struct Document {
    pub width: u32,
    pub height: u32,
    pub layers: Vec<Layer>,
    pub active: usize,
    pub title: String,
    undo: VecDeque<HistoryEntry>,
    redo: VecDeque<HistoryEntry>,
    pending: Option<PendingEdit>,
    stroke: Option<StrokeState>,
    shape_preview: Option<ShapePreview>,
    revision: u64,
    next_revision: u64,
    saved_revision: Option<u64>,
    next_layer_id: u64,
    paint_damage: Option<(u32, u32, u32, u32)>,
    #[cfg(test)]
    last_sample_count: usize,
}

impl Document {
    pub fn new(width: u32, height: u32) -> Result<Self, String> {
        let count = pixel_count(width, height)?;
        let pixels = blank_pixels(count)?;
        Ok(Self {
            width,
            height,
            layers: vec![Layer {
                text: None,
                id: 1,
                name: "Слой 1".into(),
                visible: true,
                locked: false,
                opacity: 255,
                pixels,
            }],
            active: 0,
            title: "Без названия".into(),
            undo: VecDeque::new(),
            redo: VecDeque::new(),
            pending: None,
            stroke: None,
            shape_preview: None,
            revision: 0,
            next_revision: 1,
            saved_revision: Some(0),
            next_layer_id: 2,
            paint_damage: None,
            #[cfg(test)]
            last_sample_count: 0,
        })
    }

    pub fn with_background(width: u32, height: u32, color: Color) -> Result<Self, String> {
        let mut doc = Self::new(width, height)?;
        doc.layers[0].name = "Бумага".into();
        doc.layers[0].pixels.fill(color);
        doc.layers[0].locked = true;
        doc.add_layer("Слой 1")?;
        doc.clear_history();
        doc.mark_saved();
        Ok(doc)
    }

    pub fn from_pixels(width: u32, height: u32, pixels: Vec<Color>) -> Result<Self, String> {
        if pixels.len() != pixel_count(width, height)? {
            return Err("Размер изображения не совпадает с количеством пикселей.".into());
        }
        let mut doc = Self::new(width, height)?;
        doc.layers[0].pixels = pixels;
        doc.layers[0].name = "Изображение".into();
        Ok(doc)
    }

    pub fn active_layer(&self) -> &Layer {
        &self.layers[self.active.min(self.layers.len().saturating_sub(1))]
    }

    pub fn can_paint(&self) -> bool {
        self.layers
            .get(self.active)
            .is_some_and(|l| !l.locked && l.visible && l.text.is_none())
    }

    fn ensure_paintable(&self) -> Result<(), String> {
        let count = pixel_count(self.width, self.height)?;
        let layer = self.layers.get(self.active).ok_or("Выберите слой.")?;
        if layer.pixels.len() != count {
            return Err("Повреждён буфер слоя.".into());
        }
        if layer.locked {
            Err("Слой заблокирован. Снимите блокировку или выберите другой слой.".into())
        } else if layer.text.is_some() {
            Err("Для рисования выберите растровый слой.".into())
        } else if !layer.visible {
            Err("Слой скрыт. Включите его видимость перед рисованием.".into())
        } else {
            Ok(())
        }
    }

    fn snapshot(&self) -> Snapshot {
        Snapshot {
            layers: self.layers.clone(),
            active: self.active,
            title: self.title.clone(),
            revision: self.revision,
        }
    }

    fn restore(&mut self, snap: Snapshot) {
        self.layers = snap.layers;
        self.active = snap.active.min(self.layers.len().saturating_sub(1));
        self.title = snap.title;
        self.revision = snap.revision;
        self.stroke = None;
        self.shape_preview = None;
    }

    /// Starts a general edit. A raster transaction is promoted before a caller
    /// can change layer structure or directly write arbitrary pixel buffers.
    pub fn begin_edit(&mut self) {
        // A different edit can extend the pending transaction, but a later
        // preview cancellation must never roll back that unrelated operation.
        self.shape_preview = None;
        match self.pending.take() {
            None => self.pending = Some(PendingEdit::Snapshot(self.snapshot())),
            Some(PendingEdit::Snapshot(snapshot)) => {
                self.pending = Some(PendingEdit::Snapshot(snapshot))
            }
            Some(PendingEdit::Raster(raster)) => {
                let mut snapshot = self.snapshot();
                raster.restore_pixels(&mut snapshot.layers, self.width as usize);
                raster.before.restore_layers(&mut snapshot.layers);
                snapshot.active = raster.before.active;
                snapshot.title = raster.before.title;
                snapshot.revision = raster.before.revision;
                self.pending = Some(PendingEdit::Snapshot(snapshot));
            }
            Some(PendingEdit::Metadata(before)) => {
                let mut snapshot = self.snapshot();
                before.restore_layers(&mut snapshot.layers);
                snapshot.active = before.active;
                snapshot.title = before.title;
                snapshot.revision = before.revision;
                self.pending = Some(PendingEdit::Snapshot(snapshot));
            }
        }
    }

    /// Starts a transaction for layer names, order, visibility, locking and
    /// opacity. A slider can update repeatedly, then commit or cancel once,
    /// without copying any pixel buffers. Call begin_edit before direct writes.
    pub fn begin_layer_metadata_edit(&mut self) {
        self.shape_preview = None;
        if self.pending.is_none() {
            self.pending = Some(PendingEdit::Metadata(StateMetadata::from_parts(
                &self.layers,
                self.active,
                &self.title,
                self.revision,
            )));
        }
    }

    fn auto_begin_metadata(&mut self) -> bool {
        let owns = self.pending.is_none();
        self.begin_layer_metadata_edit();
        owns
    }

    fn begin_raster_edit(&mut self) -> bool {
        self.shape_preview = None;
        let owns = self.pending.is_none();
        self.pending = match self.pending.take() {
            None => Some(PendingEdit::Raster(RasterEdit::new(self))),
            Some(PendingEdit::Metadata(before)) => {
                let mut raster = RasterEdit::new(self);
                raster.before = before;
                Some(PendingEdit::Raster(raster))
            }
            pending => pending,
        };
        if self.pending.as_ref().is_some_and(
            |pending| matches!(pending, PendingEdit::Raster(raster) if raster.layer_id != self.active_layer().id),
        ) {
            self.begin_edit();
        }
        owns
    }

    fn auto_begin(&mut self) -> bool {
        let owns = self.pending.is_none();
        self.begin_edit();
        owns
    }

    fn snapshot_differs(&self, before: &Snapshot) -> bool {
        // Compare metadata first so a layer-opacity drag does not scan all
        // preceding unchanged layers on every dirty-state query.
        before.title != self.title
            || before.layers.len() != self.layers.len()
            || before.layers.iter().zip(&self.layers).any(|(a, b)| {
                a.id != b.id
                    || a.text != b.text
                    || a.name != b.name
                    || a.visible != b.visible
                    || a.locked != b.locked
                    || a.opacity != b.opacity
            })
            || before
                .layers
                .iter()
                .zip(&self.layers)
                .any(|(a, b)| a.pixels != b.pixels)
    }

    /// Records real changes only. Selecting a layer alone is not an edit.
    pub fn commit_edit(&mut self) -> bool {
        self.stroke = None;
        self.shape_preview = None;
        let Some(before) = self.pending.take() else {
            return false;
        };
        let entry = match before {
            PendingEdit::Snapshot(before) => {
                if !self.snapshot_differs(&before) {
                    return false;
                }
                self.revision = self.next_revision;
                self.add_damage(0, 0, self.width, self.height);
                HistoryEntry::between(before, self)
            }
            PendingEdit::Raster(raster) => {
                let patches = self
                    .layers
                    .iter()
                    .find(|l| l.id == raster.layer_id)
                    .map(|l| raster.patches(l, self.width as usize))
                    .unwrap_or_default();
                if patches.is_empty() && !raster.before.differs(&self.layers, &self.title) {
                    return false;
                }
                self.revision = self.next_revision;
                let layers = if patches.is_empty() {
                    Vec::new()
                } else {
                    vec![LayerDelta {
                        id: raster.layer_id,
                        pixels: PixelDelta::Tiles(patches),
                    }]
                };
                HistoryEntry {
                    before: raster.before,
                    after: StateMetadata::from_parts(
                        &self.layers,
                        self.active,
                        &self.title,
                        self.revision,
                    ),
                    layers,
                }
            }
            PendingEdit::Metadata(before) => {
                if !before.differs(&self.layers, &self.title) {
                    return false;
                }
                self.revision = self.next_revision;
                self.add_damage(0, 0, self.width, self.height);
                HistoryEntry {
                    before,
                    after: StateMetadata::from_parts(
                        &self.layers,
                        self.active,
                        &self.title,
                        self.revision,
                    ),
                    layers: Vec::new(),
                }
            }
        };
        self.next_revision = self.next_revision.wrapping_add(1).max(1);
        self.undo.push_back(entry);
        self.redo.clear();
        self.trim_history();
        true
    }

    pub fn cancel_edit(&mut self) {
        self.stroke = None;
        self.shape_preview = None;
        if let Some(before) = self.pending.take() {
            match before {
                PendingEdit::Snapshot(snapshot) => {
                    self.restore(snapshot);
                    self.add_damage(0, 0, self.width, self.height);
                }
                PendingEdit::Raster(raster) => {
                    raster.restore_pixels(&mut self.layers, self.width as usize);
                    for tile in raster.tiles.iter().flatten() {
                        self.add_damage(
                            tile.x as u32,
                            tile.y as u32,
                            tile.width as u32,
                            tile.height as u32,
                        );
                    }
                    if raster.before.differs(&self.layers, &self.title) {
                        self.add_damage(0, 0, self.width, self.height);
                    }
                    raster.before.restore_layers(&mut self.layers);
                    self.active = raster
                        .before
                        .active
                        .min(self.layers.len().saturating_sub(1));
                    self.title = raster.before.title;
                    self.revision = raster.before.revision;
                }
                PendingEdit::Metadata(before) => {
                    before.restore_layers(&mut self.layers);
                    self.active = before.active.min(self.layers.len().saturating_sub(1));
                    self.title = before.title;
                    self.revision = before.revision;
                    self.add_damage(0, 0, self.width, self.height);
                }
            }
        }
    }

    fn trim_history(&mut self) {
        let mut bytes: usize = self
            .undo
            .iter()
            .chain(self.redo.iter())
            .map(HistoryEntry::bytes)
            .sum();
        while bytes > HISTORY_BYTES || self.undo.len() + self.redo.len() > MAX_HISTORY_STEPS {
            let old = if !self.undo.is_empty() {
                self.undo.pop_front()
            } else {
                self.redo.pop_front()
            };
            match old {
                Some(s) => bytes = bytes.saturating_sub(s.bytes()),
                None => break,
            }
        }
    }

    pub fn clear_history(&mut self) {
        self.undo.clear();
        self.redo.clear();
        self.pending = None;
        self.stroke = None;
        self.shape_preview = None;
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }
    pub fn undo_count(&self) -> usize {
        self.undo.len()
    }
    pub fn redo_count(&self) -> usize {
        self.redo.len()
    }

    pub fn undo(&mut self) -> bool {
        if self.pending.is_some() {
            self.commit_edit();
        }
        let Some(entry) = self.undo.pop_back() else {
            return false;
        };
        entry.apply(self, false);
        self.add_damage(0, 0, self.width, self.height);
        self.redo.push_back(entry);
        self.trim_history();
        true
    }

    pub fn redo(&mut self) -> bool {
        if self.pending.is_some() {
            self.cancel_edit();
        }
        let Some(entry) = self.redo.pop_back() else {
            return false;
        };
        entry.apply(self, true);
        self.add_damage(0, 0, self.width, self.height);
        self.undo.push_back(entry);
        self.trim_history();
        true
    }

    pub fn is_dirty(&self) -> bool {
        self.saved_revision != Some(self.revision)
            || self.pending.as_ref().is_some_and(|pending| match pending {
                PendingEdit::Snapshot(snapshot) => self.snapshot_differs(snapshot),
                PendingEdit::Raster(raster) => {
                    raster.changed || raster.before.differs(&self.layers, &self.title)
                }
                PendingEdit::Metadata(before) => before.differs(&self.layers, &self.title),
            })
    }

    pub fn history_bytes(&self) -> usize {
        self.undo
            .iter()
            .chain(self.redo.iter())
            .map(HistoryEntry::bytes)
            .sum()
    }

    pub fn mark_saved(&mut self) {
        if self.pending.is_some() {
            self.commit_edit();
        }
        self.saved_revision = Some(self.revision);
    }

    fn begin_layer_structure_edit(&mut self) -> Option<StateMetadata> {
        if self.pending.is_none() {
            Some(StateMetadata::from_parts(
                &self.layers,
                self.active,
                &self.title,
                self.revision,
            ))
        } else {
            // Preserve an enclosing gesture, including its original pixels.
            self.begin_edit();
            None
        }
    }

    fn record_layer_structure_edit(&mut self, before: StateMetadata, layers: Vec<LayerDelta>) {
        self.revision = self.next_revision;
        self.next_revision = self.next_revision.wrapping_add(1).max(1);
        self.undo.push_back(HistoryEntry {
            before,
            after: StateMetadata::from_parts(&self.layers, self.active, &self.title, self.revision),
            layers,
        });
        self.redo.clear();
        self.trim_history();
        self.add_damage(0, 0, self.width, self.height);
    }

    pub fn add_layer(&mut self, name: &str) -> Result<usize, String> {
        self.validate_new_layer()?;
        let name = validated_name(name)?;
        let pixels = blank_pixels(self.width as usize * self.height as usize)?;
        let before = self.begin_layer_structure_edit();
        let at = (self.active + 1).min(self.layers.len());
        self.layers.insert(
            at,
            Layer {
                text: None,
                id: self.next_layer_id,
                name,
                visible: true,
                locked: false,
                opacity: 255,
                pixels,
            },
        );
        self.next_layer_id = self.next_layer_id.saturating_add(1);
        self.active = at;
        if let Some(before) = before {
            // A new empty layer can be recreated from its metadata on redo.
            self.record_layer_structure_edit(before, Vec::new());
        }
        Ok(at)
    }

    fn validate_new_layer(&self) -> Result<(), String> {
        let count = pixel_count(self.width, self.height)?;
        if self.layers.len() >= MAX_LAYERS
            || count.saturating_mul(self.layers.len() + 1) > MAX_LAYER_PIXELS
        {
            return Err("Достигнут лимит слоёв для этого размера холста (до 32 слоёв, до 256 МиБ пикселей).".into());
        }
        Ok(())
    }

    pub fn duplicate_layer(&mut self) -> Result<usize, String> {
        self.validate_new_layer()?;
        let mut layer = self.active_layer().clone();
        layer.name = format!(
            "{} — копия",
            layer.name.chars().take(100).collect::<String>()
        );
        layer.id = self.next_layer_id;
        self.next_layer_id = self.next_layer_id.saturating_add(1);
        let before = self.begin_layer_structure_edit();
        let pixels = before.as_ref().map(|_| layer.pixels.clone());
        let id = layer.id;
        let at = self.active + 1;
        self.layers.insert(at, layer);
        self.active = at;
        if let Some(before) = before {
            self.record_layer_structure_edit(
                before,
                vec![LayerDelta {
                    id,
                    pixels: PixelDelta::Whole(pixels.unwrap()),
                }],
            );
        }
        Ok(at)
    }

    pub fn delete_layer(&mut self) -> Result<(), String> {
        if self.layers.len() <= 1 {
            return Err("В документе должен остаться хотя бы один слой.".into());
        }
        if self.active_layer().locked {
            return Err("Сначала снимите блокировку слоя.".into());
        }
        let before = self.begin_layer_structure_edit();
        let removed = self.layers.remove(self.active);
        self.active = self.active.min(self.layers.len() - 1);
        if let Some(before) = before {
            self.record_layer_structure_edit(
                before,
                vec![LayerDelta {
                    id: removed.id,
                    pixels: PixelDelta::Whole(removed.pixels),
                }],
            );
        }
        Ok(())
    }

    /// Positive delta moves the layer toward the top of the painting stack.
    pub fn move_layer(&mut self, delta: i32) -> bool {
        let at =
            (self.active as i64 + delta as i64).clamp(0, self.layers.len() as i64 - 1) as usize;
        if at == self.active {
            return false;
        }
        let owns = self.auto_begin_metadata();
        let layer = self.layers.remove(self.active);
        self.layers.insert(at, layer);
        self.active = at;
        self.add_damage(0, 0, self.width, self.height);
        if owns {
            self.commit_edit();
        }
        true
    }

    pub fn set_layer_visibility(&mut self, index: usize, visible: bool) -> bool {
        if self.layers.get(index).is_none_or(|l| l.visible == visible) {
            return false;
        }
        let owns = self.auto_begin_metadata();
        self.layers[index].visible = visible;
        self.add_damage(0, 0, self.width, self.height);
        if owns {
            self.commit_edit();
        }
        true
    }

    pub fn set_layer_locked(&mut self, index: usize, locked: bool) -> bool {
        if self.layers.get(index).is_none_or(|l| l.locked == locked) {
            return false;
        }
        let owns = self.auto_begin_metadata();
        self.layers[index].locked = locked;
        if owns {
            self.commit_edit();
        }
        true
    }

    pub fn set_layer_opacity(&mut self, index: usize, opacity: u8) -> bool {
        if self.layers.get(index).is_none_or(|l| l.opacity == opacity) {
            return false;
        }
        let owns = self.auto_begin_metadata();
        self.layers[index].opacity = opacity;
        self.add_damage(0, 0, self.width, self.height);
        if owns {
            self.commit_edit();
        }
        true
    }

    pub fn rename_layer(&mut self, index: usize, name: &str) -> Result<(), String> {
        let name = validated_name(name)?;
        let layer = self.layers.get(index).ok_or("Слой не найден.")?;
        if layer.name == name {
            return Ok(());
        }
        let owns = self.auto_begin_metadata();
        self.layers[index].name = name;
        if owns {
            self.commit_edit();
        }
        Ok(())
    }

    /// Adds a centered image, reducing it to fit the canvas when necessary.
    pub fn add_image_layer(
        &mut self,
        name: &str,
        width: u32,
        height: u32,
        pixels: &[Color],
    ) -> Result<usize, String> {
        if pixels.len() != pixel_count(width, height)? {
            return Err("Повреждён буфер изображения.".into());
        }
        self.validate()?;
        self.validate_new_layer()?;
        let name = validated_name(name)?;
        let owns = self.auto_begin();
        let at = match self.add_layer(&name) {
            Ok(at) => at,
            Err(e) => {
                if owns {
                    self.cancel_edit();
                }
                return Err(e);
            }
        };
        let scale = (self.width as f32 / width as f32)
            .min(self.height as f32 / height as f32)
            .min(1.0);
        let out_w = (width as f32 * scale).round().max(1.0) as u32;
        let out_h = (height as f32 * scale).round().max(1.0) as u32;
        let offset_x = (self.width - out_w) / 2;
        let offset_y = (self.height - out_h) / 2;
        let target = &mut self.layers[at].pixels;
        for y in 0..out_h {
            for x in 0..out_w {
                let src_x = ((x as f32 + 0.5) * width as f32 / out_w as f32 - 0.5).max(0.0);
                let src_y = ((y as f32 + 0.5) * height as f32 / out_h as f32 - 0.5).max(0.0);
                target[((y + offset_y) * self.width + x + offset_x) as usize] =
                    bilinear(pixels, width, height, src_x, src_y);
            }
        }
        if owns {
            self.commit_edit();
        }
        Ok(at)
    }

    pub fn begin_stroke(&mut self, x: f32, y: f32, brush: Brush) -> Result<(), String> {
        if !x.is_finite() || !y.is_finite() {
            return Err("Некорректные координаты штриха.".into());
        }
        self.ensure_paintable()?;
        let brush = brush.sanitized();
        self.begin_stroke_with_mask(x, y, brush, BrushMask::new(brush));
        Ok(())
    }

    fn begin_stroke_with_mask(&mut self, x: f32, y: f32, brush: Brush, mut mask: BrushMask) {
        if self.stroke.is_some() {
            self.end_stroke();
        }
        self.begin_raster_edit();
        mask.prepare_alpha(brush, 1.0);
        #[cfg(test)]
        {
            self.last_sample_count = 0;
        }
        self.stamp(x, y, brush, &mask, 0);
        self.stroke = Some(StrokeState {
            brush,
            mask,
            last_x: x,
            last_y: y,
            last_stamp: (x, y),
            remainder: 0.0,
            stamp_index: 1,
        });
    }

    pub fn continue_stroke(&mut self, x: f32, y: f32) {
        if !x.is_finite() || !y.is_finite() {
            return;
        }
        let Some(mut stroke) = self.stroke.take() else {
            return;
        };
        #[cfg(test)]
        {
            self.last_sample_count = 0;
        }
        let dx = x as f64 - stroke.last_x as f64;
        let dy = y as f64 - stroke.last_y as f64;
        let length = dx.hypot(dy);
        let spacing = (stroke.brush.size as f64 * stroke.brush.spacing as f64).max(0.35);
        if length > 0.0001 {
            let margin = stroke.brush.size as f64 * 1.25 + 2.0;
            if let Some((ax, ay, bx, by)) = clip_paint_segment(
                stroke.last_x as f64,
                stroke.last_y as f64,
                x as f64,
                y as f64,
                self.width as f64,
                self.height as f64,
                margin,
            ) {
                let visible = (bx - ax).hypot(by - ay);
                let prefix = (ax - stroke.last_x as f64).hypot(ay - stroke.last_y as f64);
                let phase = (stroke.remainder + prefix).rem_euclid(spacing);
                let first = spacing - phase;
                let desired = if visible + 1e-7 >= first {
                    ((visible - first) / spacing).floor() as usize + 1
                } else {
                    0
                };
                let cost =
                    stroke.mask.side * stroke.mask.side * if stroke.brush.mirror { 2 } else { 1 };
                let limit = (MAX_STROKE_SAMPLES / cost.max(1)).max(1);
                let count = desired.min(limit);
                let compensation = if desired > limit {
                    desired as f32 / limit as f32
                } else {
                    1.0
                };
                stroke.mask.prepare_alpha(stroke.brush, compensation);
                for i in 0..count {
                    let distance = if desired > limit {
                        visible * (i as f64 + 0.5) / count as f64
                    } else {
                        first + i as f64 * spacing
                    };
                    let t = if visible > 0.0 {
                        distance / visible
                    } else {
                        0.0
                    };
                    let sx = (ax + (bx - ax) * t) as f32;
                    let sy = (ay + (by - ay) * t) as f32;
                    self.stamp(sx, sy, stroke.brush, &stroke.mask, stroke.stamp_index);
                    stroke.stamp_index = stroke.stamp_index.wrapping_add(1);
                    stroke.last_stamp = (sx, sy);
                }
            }
            stroke.remainder = (stroke.remainder + length).rem_euclid(spacing);
        }
        stroke.last_x = x;
        stroke.last_y = y;
        self.stroke = Some(stroke);
    }

    fn finish_stroke_pixels(&mut self) -> Option<BrushMask> {
        self.stroke.take().map(|mut stroke| {
            if (stroke.last_x - stroke.last_stamp.0).hypot(stroke.last_y - stroke.last_stamp.1)
                > 0.05
            {
                stroke.mask.prepare_alpha(stroke.brush, 1.0);
                self.stamp(
                    stroke.last_x,
                    stroke.last_y,
                    stroke.brush,
                    &stroke.mask,
                    stroke.stamp_index,
                );
            }
            stroke.mask
        })
    }

    pub fn end_stroke(&mut self) -> bool {
        self.finish_stroke_pixels();
        self.commit_edit()
    }

    pub fn cancel_stroke(&mut self) {
        self.cancel_edit();
    }

    fn stamp(&mut self, x: f32, y: f32, brush: Brush, mask: &BrushMask, index: u32) {
        if !self.can_paint() || !x.is_finite() || !y.is_finite() {
            return;
        }
        if self.pending.as_ref().is_some_and(
            |p| matches!(p, PendingEdit::Raster(r) if r.layer_id != self.active_layer().id),
        ) {
            self.begin_edit();
        }
        let scatter = brush.scatter * brush.size;
        let x = x + (grain(index as i32, 17, 0x91) - 0.5) * scatter;
        let y = y + (grain(index as i32, 31, 0xb7) - 0.5) * scatter;
        self.stamp_one(x, y, brush, mask, &mask.alpha, false);
        if brush.mirror {
            let mx = self.width as f32 - x;
            if (mx - x).abs() > 0.01 {
                self.stamp_one(x, y, brush, mask, &mask.alpha, true);
            }
        }
    }

    fn stamp_one(
        &mut self,
        x: f32,
        y: f32,
        brush: Brush,
        mask: &BrushMask,
        alpha: &[u8; 256],
        reflect: bool,
    ) {
        let half = mask.side as f64 * 0.5;
        let left = (x as f64 - half).round();
        let left = if reflect {
            self.width as f64 - mask.side as f64 - left
        } else {
            left
        };
        let top = (y as f64 - half).round();
        if left >= self.width as f64
            || top >= self.height as f64
            || left + mask.side as f64 <= 0.0
            || top + mask.side as f64 <= 0.0
        {
            return;
        }
        let left = left as i32;
        let top = top as i32;
        let min_x = left.max(0) as usize;
        let min_y = top.max(0) as usize;
        let max_x = (left + mask.side as i32).min(self.width as i32) as usize;
        let max_y = (top + mask.side as i32).min(self.height as i32) as usize;
        let opaque_source = Color {
            a: 255,
            ..brush.color
        };
        let mut changed = false;
        for py in min_y..max_y {
            let mask_y = (py as i32 - top) as usize;
            let (start, end) = mask.row_bounds[mask_y];
            if start == end {
                continue;
            }
            let (start, end) = if reflect {
                (mask.side - end, mask.side - start)
            } else {
                (start, end)
            };
            let row_min_x = (left + start as i32).max(min_x as i32) as usize;
            let row_max_x = (left + end as i32).min(max_x as i32).max(0) as usize;
            for px in row_min_x..row_max_x {
                #[cfg(test)]
                {
                    self.last_sample_count += 1;
                }
                let mask_x = (px as i32 - left) as usize;
                let mask_x = if reflect {
                    mask.side - 1 - mask_x
                } else {
                    mask_x
                };
                let coverage = mask.coverage[mask_y * mask.side + mask_x];
                let a = alpha[coverage as usize];
                if a == 0 {
                    continue;
                }
                let index = py * self.width as usize + px;
                let old = self.layers[self.active].pixels[index];
                let new = if brush.erase {
                    if old == Color::TRANSPARENT {
                        continue;
                    }
                    let amount = (a as u32 * brush.color.a as u32 + 127) / 255;
                    let a = ((old.a as u32 * (255 - amount) + 127) / 255) as u8;
                    if a == 0 {
                        Color::TRANSPARENT
                    } else {
                        Color { a, ..old }
                    }
                } else {
                    // A fully opaque pixel already at the source RGB is an
                    // exact source-over fixed point for every source alpha.
                    if old == opaque_source {
                        continue;
                    }
                    blend(old, brush.color, a)
                };
                if new == old {
                    continue;
                }
                if let Some(PendingEdit::Raster(raster)) = &mut self.pending {
                    raster.capture(
                        &self.layers[self.active],
                        self.width as usize,
                        self.height as usize,
                        px,
                        py,
                    );
                }
                self.layers[self.active].pixels[index] = new;
                changed = true;
            }
        }
        if changed {
            if let Some(PendingEdit::Raster(raster)) = &mut self.pending {
                raster.changed = true;
            }
            self.add_damage(
                min_x as u32,
                min_y as u32,
                (max_x - min_x) as u32,
                (max_y - min_y) as u32,
            );
        }
    }

    pub fn draw_line(
        &mut self,
        x0: f32,
        y0: f32,
        x1: f32,
        y1: f32,
        brush: Brush,
    ) -> Result<(), String> {
        self.draw_shape(ShapeKind::Line, x0, y0, x1, y1, brush)
    }

    pub fn draw_rectangle(
        &mut self,
        x0: f32,
        y0: f32,
        x1: f32,
        y1: f32,
        brush: Brush,
    ) -> Result<(), String> {
        self.draw_shape(ShapeKind::Rectangle, x0, y0, x1, y1, brush)
    }

    pub fn draw_ellipse(
        &mut self,
        x0: f32,
        y0: f32,
        x1: f32,
        y1: f32,
        brush: Brush,
    ) -> Result<(), String> {
        self.draw_shape(ShapeKind::Ellipse, x0, y0, x1, y1, brush)
    }

    fn draw_shape(
        &mut self,
        kind: ShapeKind,
        x0: f32,
        y0: f32,
        x1: f32,
        y1: f32,
        brush: Brush,
    ) -> Result<(), String> {
        self.draw_shape_with_fill(kind, x0, y0, x1, y1, brush, false)
    }

    /// Apply a closed shape's fill, then its unchanged brush contour, as one
    /// edit. Lines ignore `filled`; existing draw_* methods remain outlines.
    pub fn draw_shape_with_fill(
        &mut self,
        kind: ShapeKind,
        x0: f32,
        y0: f32,
        x1: f32,
        y1: f32,
        brush: Brush,
        filled: bool,
    ) -> Result<(), String> {
        let geometry = ShapeGeometry::new(kind, x0, y0, x1, y1, brush, filled)?;
        self.ensure_paintable()?;
        self.rasterize_shape(geometry, BrushMask::new(geometry.brush));
        self.commit_edit();
        Ok(())
    }

    /// Start or replace a live raster shape in one owned transaction. The
    /// displayed pixels already include the final endpoint stamp; committing
    /// them will not change their thickness, alpha, texture or mirrored copy.
    ///
    /// Returns false for identical sanitized input. A new preview refuses an
    /// existing non-preview transaction, and failed updates leave it untouched.
    /// Call commit_shape_preview or cancel_shape_preview before another edit.
    pub fn preview_shape(
        &mut self,
        kind: ShapeKind,
        x0: f32,
        y0: f32,
        x1: f32,
        y1: f32,
        brush: Brush,
    ) -> Result<bool, String> {
        self.preview_shape_with_fill(kind, x0, y0, x1, y1, brush, false)
    }

    /// Like preview_shape, including a closed shape's antialiased interior in
    /// the same owned transaction. Changing filled replaces the previous
    /// preview; repeating identical input does no raster work.
    pub fn preview_shape_with_fill(
        &mut self,
        kind: ShapeKind,
        x0: f32,
        y0: f32,
        x1: f32,
        y1: f32,
        brush: Brush,
        filled: bool,
    ) -> Result<bool, String> {
        let geometry = ShapeGeometry::new(kind, x0, y0, x1, y1, brush, filled)?;
        self.ensure_paintable()?;
        if let Some(previous) = &self.shape_preview {
            if previous.width != self.width
                || previous.height != self.height
                || previous.layer_id != self.active_layer().id
                || self.stroke.is_some()
                || !matches!(&self.pending, Some(PendingEdit::Raster(raster)) if raster.layer_id == previous.layer_id)
            {
                return Err("Сначала завершите текущую правку.".into());
            }
            if geometry == previous.geometry {
                return Ok(false);
            }
        } else if self.pending.is_some() || self.stroke.is_some() {
            return Err("Сначала завершите текущую правку.".into());
        }

        let mask = if let Some(previous) = self.shape_preview.take() {
            if let Some(PendingEdit::Raster(raster)) = &mut self.pending {
                let damage = raster.reset_preview(&mut self.layers, self.width as usize);
                if let Some((x, y, width, height)) = damage {
                    self.add_damage(x, y, width, height);
                }
            }
            let before = previous.geometry.brush;
            let after = geometry.brush;
            if before.kind == after.kind
                && before.size == after.size
                && before.hardness == after.hardness
                && before.angle == after.angle
            {
                previous.mask
            } else {
                BrushMask::new(after)
            }
        } else {
            BrushMask::new(geometry.brush)
        };
        let mask = self.rasterize_shape(geometry, mask);
        self.shape_preview = Some(ShapePreview {
            geometry,
            layer_id: self.active_layer().id,
            width: self.width,
            height: self.height,
            mask,
        });
        Ok(true)
    }

    /// Commit exactly the last displayed shape, with no extra raster work.
    pub fn commit_shape_preview(&mut self) -> bool {
        if self.shape_preview.take().is_none() {
            return false;
        }
        self.commit_edit()
    }

    /// Restore the preview's baseline and damage without touching any edit
    /// which has since taken ownership of the pending transaction.
    pub fn cancel_shape_preview(&mut self) -> bool {
        if self.shape_preview.take().is_none() {
            return false;
        }
        self.cancel_edit();
        true
    }

    /// Fill once per covered pixel, independently of brush spacing/texture.
    /// Rectangle coverage is exact pixel area. Ellipses integrate four
    /// horizontal intersections over each clipped pixel row; their solid
    /// interior needs no per-pixel square roots or repeated brush stamps.
    fn fill_shape_interior(&mut self, geometry: ShapeGeometry) {
        let ShapeGeometry {
            kind,
            x0,
            y0,
            x1,
            y1,
            brush,
            ..
        } = geometry;
        if kind == ShapeKind::Line
            || (brush.opacity * brush.flow * 255.0 + 0.5) as u8 == 0
            || (!brush.erase && brush.color.a == 0)
        {
            return;
        }
        let (left, right) = ((x0 as f64).min(x1 as f64), (x0 as f64).max(x1 as f64));
        let (top, bottom) = ((y0 as f64).min(y1 as f64), (y0 as f64).max(y1 as f64));
        if left >= right || top >= bottom {
            return;
        }
        let width = self.width as usize;
        let height = self.height as usize;
        let min_x = left.floor().clamp(0.0, width as f64) as usize;
        let max_x = right.ceil().clamp(0.0, width as f64) as usize;
        let min_y = top.floor().clamp(0.0, height as f64) as usize;
        let max_y = bottom.ceil().clamp(0.0, height as f64) as usize;
        if min_x >= max_x || min_y >= max_y {
            return;
        }
        let cx = (left + right) * 0.5;
        let cy = (top + bottom) * 0.5;
        let rx = (right - left) * 0.5;
        let ry = (bottom - top) * 0.5;
        let mut changed = false;
        for y in min_y..max_y {
            let row_top = (y as f64).max(top);
            let row_bottom = ((y + 1) as f64).min(bottom);
            let row_height = row_bottom - row_top;
            let mut spans = [(left, right); 4];
            if kind == ShapeKind::Ellipse {
                for (sample, span) in spans.iter_mut().enumerate() {
                    let sample_y = row_top + row_height * (sample as f64 + 0.5) * 0.25;
                    let unit_y = (sample_y - cy) / ry;
                    let extent = rx * (1.0 - unit_y * unit_y).max(0.0).sqrt();
                    *span = (cx - extent, cx + extent);
                }
            }
            let outer_left = spans.iter().map(|s| s.0).fold(f64::INFINITY, f64::min);
            let outer_right = spans.iter().map(|s| s.1).fold(f64::NEG_INFINITY, f64::max);
            let inner_left = spans.iter().map(|s| s.0).fold(f64::NEG_INFINITY, f64::max);
            let inner_right = spans.iter().map(|s| s.1).fold(f64::INFINITY, f64::min);
            let row_min_x = outer_left.floor().clamp(min_x as f64, max_x as f64) as usize;
            let row_max_x = outer_right.ceil().clamp(min_x as f64, max_x as f64) as usize;
            let solid_min_x = inner_left.ceil().clamp(min_x as f64, max_x as f64) as usize;
            let solid_max_x = inner_right.floor().clamp(min_x as f64, max_x as f64) as usize;
            let mut row_changed = false;
            for x in row_min_x..row_max_x {
                let coverage = if x >= solid_min_x && x < solid_max_x {
                    row_height
                } else if kind == ShapeKind::Rectangle {
                    (right.min((x + 1) as f64) - left.max(x as f64)).clamp(0.0, 1.0) * row_height
                } else {
                    spans
                        .iter()
                        .map(|&(a, b)| (b.min((x + 1) as f64) - a.max(x as f64)).clamp(0.0, 1.0))
                        .sum::<f64>()
                        * 0.25
                        * row_height
                };
                let alpha = (coverage as f32 * brush.opacity * brush.flow * 255.0 + 0.5)
                    .clamp(0.0, 255.0) as u8;
                if alpha == 0 {
                    continue;
                }
                row_changed |= self.paint_fill_pixel(x, y, brush, alpha);
                if brush.mirror && width - 1 - x != x {
                    row_changed |= self.paint_fill_pixel(width - 1 - x, y, brush, alpha);
                }
            }
            if row_changed {
                let damage_left = if brush.mirror {
                    row_min_x.min(width - row_max_x)
                } else {
                    row_min_x
                };
                let damage_right = if brush.mirror {
                    row_max_x.max(width - row_min_x)
                } else {
                    row_max_x
                };
                self.add_damage(
                    damage_left as u32,
                    y as u32,
                    (damage_right - damage_left) as u32,
                    1,
                );
                changed = true;
            }
        }
        if changed {
            if let Some(PendingEdit::Raster(raster)) = &mut self.pending {
                raster.changed = true;
            }
        }
    }

    fn paint_fill_pixel(&mut self, x: usize, y: usize, brush: Brush, alpha: u8) -> bool {
        let index = y * self.width as usize + x;
        let old = self.layers[self.active].pixels[index];
        let new = if brush.erase {
            if old == Color::TRANSPARENT {
                return false;
            }
            let amount = (alpha as u32 * brush.color.a as u32 + 127) / 255;
            let a = ((old.a as u32 * (255 - amount) + 127) / 255) as u8;
            if a == 0 {
                Color::TRANSPARENT
            } else {
                Color { a, ..old }
            }
        } else {
            if old
                == (Color {
                    a: 255,
                    ..brush.color
                })
            {
                return false;
            }
            blend(old, brush.color, alpha)
        };
        if new == old {
            return false;
        }
        if let Some(PendingEdit::Raster(raster)) = &mut self.pending {
            raster.capture(
                &self.layers[self.active],
                self.width as usize,
                self.height as usize,
                x,
                y,
            );
        }
        self.layers[self.active].pixels[index] = new;
        true
    }

    fn rasterize_shape(&mut self, geometry: ShapeGeometry, mask: BrushMask) -> BrushMask {
        if geometry.filled {
            if self.stroke.is_some() {
                self.end_stroke();
            }
            self.begin_raster_edit();
            self.fill_shape_interior(geometry);
        }
        let ShapeGeometry {
            kind,
            x0,
            y0,
            x1,
            y1,
            brush,
            ..
        } = geometry;
        match kind {
            ShapeKind::Line | ShapeKind::Rectangle => {
                self.begin_stroke_with_mask(x0, y0, brush, mask);
                if kind == ShapeKind::Line {
                    self.continue_stroke(x1, y1);
                } else {
                    self.continue_stroke(x1, y0);
                    self.continue_stroke(x1, y1);
                    self.continue_stroke(x0, y1);
                    self.continue_stroke(x0, y0);
                }
            }
            ShapeKind::Ellipse => {
                // Finite endpoints must not overflow during midpoint/radius
                // arithmetic, even when a pointer is far outside the canvas.
                let cx = (x0 as f64 + x1 as f64) * 0.5;
                let cy = (y0 as f64 + y1 as f64) * 0.5;
                let rx = (x1 as f64 - x0 as f64).abs() * 0.5;
                let ry = (y1 as f64 - y0 as f64).abs() * 0.5;
                let steps = ((rx + ry) * std::f64::consts::PI / 2.0)
                    .ceil()
                    .clamp(24.0, 8192.0) as usize;
                self.begin_stroke_with_mask((cx + rx) as f32, cy as f32, brush, mask);
                for i in 1..=steps {
                    let angle = std::f64::consts::TAU * i as f64 / steps as f64;
                    self.continue_stroke(
                        (cx + rx * angle.cos()) as f32,
                        (cy + ry * angle.sin()) as f32,
                    );
                }
            }
        }
        self.finish_stroke_pixels()
            .expect("shape rasterization starts one stroke")
    }

    /// Four-connected fill on the active layer. Tolerance includes alpha.
    pub fn flood_fill(
        &mut self,
        x: u32,
        y: u32,
        color: Color,
        tolerance: u8,
    ) -> Result<bool, String> {
        self.ensure_paintable()?;
        if x >= self.width || y >= self.height {
            return Ok(false);
        }
        let w = self.width as usize;
        let h = self.height as usize;
        let index = y as usize * w + x as usize;
        let original = self.layers[self.active].pixels[index];
        if original == color {
            return Ok(false);
        }
        let owns = self.begin_raster_edit();
        let mut visited = vec![0u64; (w * h).div_ceil(64)];
        let mut seeds = vec![index];
        let seen = |visited: &[u64], at: usize| visited[at / 64] & (1 << (at % 64)) != 0;
        let matches = |p: Color| {
            p.r.abs_diff(original.r) <= tolerance
                && p.g.abs_diff(original.g) <= tolerance
                && p.b.abs_diff(original.b) <= tolerance
                && p.a.abs_diff(original.a) <= tolerance
        };
        let (mut left_bound, mut top_bound, mut right_bound, mut bottom_bound) = (w, h, 0, 0);
        while let Some(at) = seeds.pop() {
            let pixels = &self.layers[self.active].pixels;
            if seen(&visited, at) || !matches(pixels[at]) {
                continue;
            }
            let cy = at / w;
            let row = cy * w;
            let mut left = at % w;
            let mut right = left + 1;
            while left > 0 && !seen(&visited, row + left - 1) && matches(pixels[row + left - 1]) {
                left -= 1;
            }
            while right < w && !seen(&visited, row + right) && matches(pixels[row + right]) {
                right += 1;
            }
            // Save each intersecting tile before its first write. A small fill
            // therefore has the same sparse Undo cost as a small brush stroke.
            if let Some(PendingEdit::Raster(raster)) = &mut self.pending {
                for tile_x in left / TILE_SIZE..=(right - 1) / TILE_SIZE {
                    raster.capture(&self.layers[self.active], w, h, tile_x * TILE_SIZE, cy);
                }
                raster.changed = true;
            }
            self.layers[self.active].pixels[row + left..row + right].fill(color);
            let first = (row + left) / 64;
            let last = (row + right - 1) / 64;
            let first_mask = u64::MAX << ((row + left) % 64);
            let last_mask = u64::MAX >> (63 - ((row + right - 1) % 64));
            if first == last {
                visited[first] |= first_mask & last_mask;
            } else {
                visited[first] |= first_mask;
                visited[first + 1..last].fill(u64::MAX);
                visited[last] |= last_mask;
            }
            left_bound = left_bound.min(left);
            top_bound = top_bound.min(cy);
            right_bound = right_bound.max(right);
            bottom_bound = bottom_bound.max(cy + 1);
            // One seed per matching run above/below gives four-connectivity,
            // without putting every pixel into a queue. The separate visited
            // bitmap also handles replacement colors inside the tolerance.
            for adjacent_y in [(cy > 0).then(|| cy - 1), (cy + 1 < h).then_some(cy + 1)]
                .into_iter()
                .flatten()
            {
                let pixels = &self.layers[self.active].pixels;
                let adjacent = adjacent_y * w;
                let mut x = left;
                while x < right {
                    if seen(&visited, adjacent + x) || !matches(pixels[adjacent + x]) {
                        x += 1;
                        continue;
                    }
                    seeds.push(adjacent + x);
                    x += 1;
                    while x < right
                        && !seen(&visited, adjacent + x)
                        && matches(pixels[adjacent + x])
                    {
                        x += 1;
                    }
                }
            }
        }
        self.add_damage(
            left_bound as u32,
            top_bound as u32,
            (right_bound - left_bound) as u32,
            (bottom_bound - top_bound) as u32,
        );
        if owns {
            self.commit_edit();
        }
        Ok(true)
    }

    fn add_damage(&mut self, x: u32, y: u32, width: u32, height: u32) {
        let right = x.saturating_add(width).min(self.width);
        let bottom = y.saturating_add(height).min(self.height);
        let x = x.min(self.width);
        let y = y.min(self.height);
        if right <= x || bottom <= y {
            return;
        }
        self.paint_damage = Some(match self.paint_damage {
            Some((old_x, old_y, old_w, old_h)) => {
                let left = old_x.min(x);
                let top = old_y.min(y);
                (
                    left,
                    top,
                    old_x.saturating_add(old_w).max(right) - left,
                    old_y.saturating_add(old_h).max(bottom) - top,
                )
            }
            None => (x, y, right - x, bottom - y),
        });
    }

    /// Returns and clears paint damage in x, y, width, height document pixels.
    pub fn take_paint_damage(&mut self) -> Option<(u32, u32, u32, u32)> {
        self.paint_damage.take()
    }

    /// Update part of a retained full-size compositor buffer. Invalid buffers
    /// return false without any partial write.
    pub fn composite_region_into(
        &self,
        output: &mut [Color],
        bounds: (u32, u32, u32, u32),
    ) -> bool {
        let Ok(count) = pixel_count(self.width, self.height) else {
            return false;
        };
        if output.len() != count || self.layers.iter().any(|l| l.pixels.len() != count) {
            return false;
        }
        let (x, y, width, height) = bounds;
        let left = x.min(self.width);
        let top = y.min(self.height);
        let right = x.saturating_add(width).min(self.width);
        let bottom = y.saturating_add(height).min(self.height);
        for row in top..bottom {
            let from = (row * self.width + left) as usize;
            let to = (row * self.width + right) as usize;
            output[from..to].fill(Color::TRANSPARENT);
            for layer in &self.layers {
                if !layer.visible || layer.opacity == 0 {
                    continue;
                }
                for (dst, &src) in output[from..to].iter_mut().zip(&layer.pixels[from..to]) {
                    *dst = blend(*dst, src, layer.opacity);
                }
            }
        }
        true
    }

    /// Independent worker copy; excludes history and in-progress transaction.
    /// Pixel allocation errors are returned before the writer thread is started.
    pub fn snapshot_for_io(&self) -> Result<Self, String> {
        self.validate()?;
        let mut layers = Vec::new();
        layers
            .try_reserve_exact(self.layers.len())
            .map_err(|_| "Недостаточно памяти для копии документа.")?;
        for layer in &self.layers {
            let mut pixels = Vec::new();
            pixels
                .try_reserve_exact(layer.pixels.len())
                .map_err(|_| "Недостаточно памяти для копии документа.")?;
            pixels.extend_from_slice(&layer.pixels);
            layers.push(Layer {
                text: layer.text.clone(),
                id: layer.id,
                name: layer.name.clone(),
                visible: layer.visible,
                locked: layer.locked,
                opacity: layer.opacity,
                pixels,
            });
        }
        Ok(Self {
            width: self.width,
            height: self.height,
            layers,
            active: self.active,
            title: self.title.clone(),
            undo: VecDeque::new(),
            redo: VecDeque::new(),
            pending: None,
            stroke: None,
            shape_preview: None,
            revision: self.revision,
            next_revision: self.next_revision,
            saved_revision: self.saved_revision,
            next_layer_id: self.next_layer_id,
            paint_damage: None,
            #[cfg(test)]
            last_sample_count: 0,
        })
    }

    pub fn composite(&self) -> Vec<Color> {
        let mut output = vec![Color::TRANSPARENT; self.width as usize * self.height as usize];
        self.composite_into(&mut output);
        output
    }

    pub fn composite_into(&self, output: &mut [Color]) {
        output.fill(Color::TRANSPARENT);
        for layer in &self.layers {
            if !layer.visible || layer.opacity == 0 {
                continue;
            }
            for (dst, &src) in output.iter_mut().zip(layer.pixels.iter()) {
                *dst = blend(*dst, src, layer.opacity);
            }
        }
    }

    pub fn sample(&self, x: u32, y: u32) -> Color {
        if x >= self.width || y >= self.height {
            return Color::TRANSPARENT;
        }
        let index = (y * self.width + x) as usize;
        let mut color = Color::TRANSPARENT;
        for layer in &self.layers {
            if layer.visible {
                if let Some(&pixel) = layer.pixels.get(index) {
                    color = blend(color, pixel, layer.opacity);
                }
            }
        }
        color
    }

    pub fn to_bytes(&self) -> Result<Vec<u8>, String> {
        self.validate()?;
        let mut bytes = Vec::with_capacity(
            self.layers.len() * self.width as usize * self.height as usize * 4 + 2048,
        );
        bytes.extend_from_slice(if self.layers.iter().any(|l| l.text.is_some()) {
            TEXT_MAGIC
        } else {
            MAGIC
        });
        push_u32(&mut bytes, self.width);
        push_u32(&mut bytes, self.height);
        push_u32(&mut bytes, self.layers.len() as u32);
        push_u32(&mut bytes, self.active as u32);
        push_string(&mut bytes, &self.title);
        for layer in &self.layers {
            bytes.extend_from_slice(&layer.id.to_le_bytes());
            bytes.extend_from_slice(&[
                layer.visible as u8,
                layer.locked as u8,
                layer.opacity,
                layer.text.is_some() as u8,
            ]);
            push_string(&mut bytes, &layer.name);
            if let Some(text) = &layer.text {
                text.write(&mut bytes);
            }
            for p in &layer.pixels {
                bytes.extend_from_slice(&[p.r, p.g, p.b, p.a]);
            }
        }
        let checksum = crc32(&bytes);
        push_u32(&mut bytes, checksum);
        Ok(bytes)
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() < 32 || bytes.len() as u64 > MAX_FILE_BYTES {
            return Err("Файл Portfolio имеет недопустимый размер.".into());
        }
        let checksum_at = bytes.len() - 4;
        let stored = u32::from_le_bytes(
            bytes[checksum_at..]
                .try_into()
                .map_err(|_| "Повреждён файл.")?,
        );
        if crc32(&bytes[..checksum_at]) != stored {
            return Err("Файл повреждён: контрольная сумма не совпадает.".into());
        }
        let mut reader = Reader::new(&bytes[..checksum_at]);
        let version = reader.take(8)?;
        if version != MAGIC && version != TEXT_MAGIC {
            return Err("Неизвестная версия файла Portfolio.".into());
        }
        let width = reader.u32()?;
        let height = reader.u32()?;
        let count = pixel_count(width, height)?;
        let layer_count = reader.u32()? as usize;
        let active = reader.u32()? as usize;
        if layer_count == 0
            || layer_count > MAX_LAYERS
            || count.saturating_mul(layer_count) > MAX_LAYER_PIXELS
            || active >= layer_count
        {
            return Err("В файле недопустимое количество слоёв.".into());
        }
        let title = reader.string(1024)?;
        let mut layers: Vec<Layer> = Vec::with_capacity(layer_count);
        let mut max_id = 0u64;
        for _ in 0..layer_count {
            let id = reader.u64()?;
            if id == 0 || id == u64::MAX || layers.iter().any(|l| l.id == id) {
                return Err("Повреждены идентификаторы слоёв.".into());
            }
            max_id = max_id.max(id);
            let flags = reader.take(4)?;
            if flags[0] > 1 || flags[1] > 1 || flags[3] > u8::from(version == TEXT_MAGIC) {
                return Err("Повреждены параметры слоя.".into());
            }
            let (visible, locked, opacity) = (flags[0] != 0, flags[1] != 0, flags[2]);
            let name = reader.string(512)?;
            let text = if flags[3] == 1 {
                Some(TextLayer::read(&mut reader, width, height)?)
            } else {
                None
            };
            let data = reader.take(count * 4)?;
            let mut pixels = Vec::new();
            pixels
                .try_reserve_exact(count)
                .map_err(|_| "Недостаточно памяти для изображения.")?;
            for rgba in data.chunks_exact(4) {
                pixels.push(Color::rgba(rgba[0], rgba[1], rgba[2], rgba[3]));
            }
            layers.push(Layer {
                text,
                id,
                name,
                visible,
                locked,
                opacity,
                pixels,
            });
        }
        if !reader.remaining().is_empty() {
            return Err("В конце файла обнаружены неподдерживаемые данные.".into());
        }
        let mut doc = Self::new(1, 1)?;
        doc.width = width;
        doc.height = height;
        doc.layers = layers;
        doc.active = active;
        doc.title = title;
        doc.next_layer_id = max_id + 1;
        Ok(doc)
    }

    fn validate(&self) -> Result<(), String> {
        let count = pixel_count(self.width, self.height)?;
        if self.layers.is_empty()
            || self.layers.len() > MAX_LAYERS
            || self.active >= self.layers.len()
            || count.saturating_mul(self.layers.len()) > MAX_LAYER_PIXELS
        {
            return Err("Недопустимая структура документа.".into());
        }
        if self.title.len() > 1024 {
            return Err("Слишком длинное название документа.".into());
        }
        for layer in &self.layers {
            if let Some(text) = &layer.text {
                text.validate(self.width, self.height)?;
            }
            if layer.pixels.len() != count
                || layer.name.len() > 512
                || layer.id == 0
                || layer.id == u64::MAX
            {
                return Err("Недопустимая структура слоя.".into());
            }
        }
        Ok(())
    }

    pub fn save(&mut self, path: &Path) -> Result<(), String> {
        if self.pending.is_some() {
            self.commit_edit();
        }
        let data = self.to_bytes()?;
        atomic_write(path, &data).map_err(|e| format!("Не удалось сохранить проект: {e}"))?;
        self.mark_saved();
        Ok(())
    }

    pub fn load(path: &Path) -> Result<Self, String> {
        let bytes = read_bounded(path, MAX_FILE_BYTES)?;
        Self::from_bytes(&bytes)
    }

    pub fn export_png(&self, path: &Path) -> Result<(), String> {
        let data = encode_png(self.width, self.height, &self.composite())?;
        atomic_write(path, &data).map_err(|e| format!("Не удалось экспортировать PNG: {e}"))
    }

    pub fn import_bmp(path: &Path) -> Result<Self, String> {
        let data = read_bounded(path, MAX_FILE_BYTES)?;
        let (width, height, pixels) = decode_bmp(&data)?;
        let mut doc = Self::from_pixels(width, height, pixels)?;
        doc.title = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Изображение")
            .chars()
            .take(200)
            .collect();
        Ok(doc)
    }
}

fn pixel_count(width: u32, height: u32) -> Result<usize, String> {
    if width == 0 || height == 0 || width > MAX_DIMENSION || height > MAX_DIMENSION {
        return Err(format!(
            "Размер холста должен быть от 1 до {MAX_DIMENSION} пикселей по каждой стороне."
        ));
    }
    Ok(width as usize * height as usize)
}

fn blank_pixels(count: usize) -> Result<Vec<Color>, String> {
    let mut pixels = Vec::new();
    pixels
        .try_reserve_exact(count)
        .map_err(|_| "Недостаточно памяти для нового слоя.")?;
    pixels.resize(count, Color::TRANSPARENT);
    Ok(pixels)
}

fn validated_name(name: &str) -> Result<String, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("Введите название слоя.".into());
    }
    if name.len() > 512 || name.chars().any(|c| c.is_control()) {
        return Err("Название слоя слишком длинное или содержит управляющие символы.".into());
    }
    Ok(name.to_owned())
}

fn bilinear(pixels: &[Color], width: u32, height: u32, x: f32, y: f32) -> Color {
    let x0 = (x.floor() as u32).min(width - 1);
    let y0 = (y.floor() as u32).min(height - 1);
    let x1 = (x0 + 1).min(width - 1);
    let y1 = (y0 + 1).min(height - 1);
    let fx = (x - x0 as f32).clamp(0.0, 1.0);
    let fy = (y - y0 as f32).clamp(0.0, 1.0);
    let samples = [
        (pixels[(y0 * width + x0) as usize], (1.0 - fx) * (1.0 - fy)),
        (pixels[(y0 * width + x1) as usize], fx * (1.0 - fy)),
        (pixels[(y1 * width + x0) as usize], (1.0 - fx) * fy),
        (pixels[(y1 * width + x1) as usize], fx * fy),
    ];
    let mut a = 0.0;
    let (mut r, mut g, mut b) = (0.0, 0.0, 0.0);
    for (p, weight) in samples {
        let wa = weight * p.a as f32;
        a += wa;
        r += p.r as f32 * wa;
        g += p.g as f32 * wa;
        b += p.b as f32 * wa;
    }
    if a < 0.0001 {
        Color::TRANSPARENT
    } else {
        Color::rgba(
            (r / a + 0.5) as u8,
            (g / a + 0.5) as u8,
            (b / a + 0.5) as u8,
            (a + 0.5) as u8,
        )
    }
}

fn push_u32(bytes: &mut Vec<u8>, value: u32) {
    bytes.extend_from_slice(&value.to_le_bytes());
}
fn push_string(bytes: &mut Vec<u8>, value: &str) {
    push_u32(bytes, value.len() as u32);
    bytes.extend_from_slice(value.as_bytes());
}

struct Reader<'a> {
    data: &'a [u8],
    at: usize,
}
impl<'a> Reader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, at: 0 }
    }
    fn take(&mut self, size: usize) -> Result<&'a [u8], String> {
        let end = self.at.checked_add(size).ok_or("Повреждён размер блока.")?;
        let out = self
            .data
            .get(self.at..end)
            .ok_or("Файл обрывается раньше конца данных.")?;
        self.at = end;
        Ok(out)
    }
    fn u32(&mut self) -> Result<u32, String> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn u64(&mut self) -> Result<u64, String> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }
    fn utf8(&mut self, max_bytes: usize) -> Result<String, String> {
        let size = self.u32()? as usize;
        if size > max_bytes {
            return Err("Недопустимая длина текста в файле.".into());
        }
        let bytes = self.take(size)?;
        let value = std::str::from_utf8(bytes).map_err(|_| "В файле повреждён текст UTF-8.")?;
        Ok(value.to_owned())
    }
    fn string(&mut self, max_bytes: usize) -> Result<String, String> {
        let value = self.utf8(max_bytes)?;
        if value.chars().any(|c| c.is_control()) {
            return Err("В названии обнаружены управляющие символы.".into());
        }
        Ok(value)
    }
    fn remaining(&self) -> &'a [u8] {
        &self.data[self.at..]
    }
}

fn read_bounded(path: &Path, limit: u64) -> Result<Vec<u8>, String> {
    let file = File::open(path).map_err(|e| format!("Не удалось открыть файл: {e}"))?;
    let size = file
        .metadata()
        .map_err(|e| format!("Не удалось прочитать параметры файла: {e}"))?
        .len();
    if size > limit {
        return Err("Файл превышает допустимый размер.".into());
    }
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(size as usize)
        .map_err(|_| "Недостаточно памяти для файла.")?;
    file.take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("Не удалось прочитать файл: {e}"))?;
    if bytes.len() as u64 > limit {
        return Err("Файл превышает допустимый размер.".into());
    }
    Ok(bytes)
}

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Writes in the destination directory, flushes, then atomically replaces the file.
/// A failed replacement preserves the previous file and removes the temporary file.
pub fn atomic_write(path: &Path, data: &[u8]) -> std::io::Result<()> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let name = path
        .file_name()
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidInput, "No filename"))?;
    let mut temporary = PathBuf::from(parent);
    let mut temp_name = name.to_os_string();
    temp_name.push(format!(
        ".{}.{}.tmp",
        std::process::id(),
        TEMP_COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    temporary.push(temp_name);
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        file.write_all(data)?;
        file.sync_all()?;
        drop(file);
        replace_file(&temporary, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

#[cfg(not(windows))]
fn replace_file(source: &Path, target: &Path) -> std::io::Result<()> {
    fs::rename(source, target)
}

#[cfg(windows)]
fn replace_file(source: &Path, target: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    #[link(name = "kernel32")]
    extern "system" {
        fn MoveFileExW(existing: *const u16, new_name: *const u16, flags: u32) -> i32;
    }
    let source: Vec<u16> = source.as_os_str().encode_wide().chain(Some(0)).collect();
    let target: Vec<u16> = target.as_os_str().encode_wide().chain(Some(0)).collect();
    // MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH. Both are on the same volume.
    if unsafe { MoveFileExW(source.as_ptr(), target.as_ptr(), 0x1 | 0x8) } == 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

/// Encodes an RGBA PNG with lossless, uncompressed DEFLATE blocks and no dependencies.
/// Export deliberately favors a small implementation and predictable decoding over size.
pub fn encode_png(width: u32, height: u32, pixels: &[Color]) -> Result<Vec<u8>, String> {
    let count = pixel_count(width, height)?;
    if pixels.len() != count {
        return Err("Некорректный размер буфера PNG.".into());
    }
    let mut raw = Vec::with_capacity(count * 4 + height as usize);
    for row in pixels.chunks_exact(width as usize) {
        raw.push(0); // PNG filter None.
        for p in row {
            raw.extend_from_slice(&[p.r, p.g, p.b, p.a]);
        }
    }
    let mut zlib = Vec::with_capacity(raw.len() + raw.len() / 65535 * 5 + 16);
    zlib.extend_from_slice(&[0x78, 0x01]);
    let chunks = raw.chunks(65535);
    let total = chunks.len();
    for (i, chunk) in chunks.enumerate() {
        zlib.push(if i + 1 == total { 1 } else { 0 });
        let len = chunk.len() as u16;
        zlib.extend_from_slice(&len.to_le_bytes());
        zlib.extend_from_slice(&(!len).to_le_bytes());
        zlib.extend_from_slice(chunk);
    }
    zlib.extend_from_slice(&adler32(&raw).to_be_bytes());
    let mut result = Vec::with_capacity(zlib.len() + 64);
    result.extend_from_slice(b"\x89PNG\r\n\x1a\n");
    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
    png_chunk(&mut result, b"IHDR", &ihdr);
    png_chunk(&mut result, b"IDAT", &zlib);
    png_chunk(&mut result, b"IEND", &[]);
    Ok(result)
}

fn png_chunk(output: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    output.extend_from_slice(&(data.len() as u32).to_be_bytes());
    let start = output.len();
    output.extend_from_slice(kind);
    output.extend_from_slice(data);
    let checksum = crc32(&output[start..]);
    output.extend_from_slice(&checksum.to_be_bytes());
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &byte in bytes {
        crc ^= byte as u32;
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xedb88320u32 & (0u32.wrapping_sub(crc & 1)));
        }
    }
    !crc
}

fn adler32(bytes: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for chunk in bytes.chunks(5552) {
        for &byte in chunk {
            a += byte as u32;
            b += a;
        }
        a %= 65521;
        b %= 65521;
    }
    (b << 16) | a
}

/// Reads ordinary 24/32-bit uncompressed Windows BMP, including top-down images.
pub fn decode_bmp(bytes: &[u8]) -> Result<(u32, u32, Vec<Color>), String> {
    if bytes.len() < 54 || &bytes[..2] != b"BM" {
        return Err("Ожидается несжатый BMP 24 или 32 бит.".into());
    }
    let u16at = |p: usize| u16::from_le_bytes([bytes[p], bytes[p + 1]]);
    let u32at = |p: usize| u32::from_le_bytes(bytes[p..p + 4].try_into().unwrap());
    let offset = u32at(10) as usize;
    let dib_size = u32at(14);
    let width_signed = u32at(18) as i32;
    let height_signed = u32at(22) as i32;
    let bits = u16at(28);
    if dib_size < 40
        || width_signed <= 0
        || height_signed == 0
        || height_signed == i32::MIN
        || u16at(26) != 1
        || ![24, 32].contains(&bits)
        || u32at(30) != 0
    {
        return Err("Поддерживаются несжатые BMP 24/32 бит с обычной RGB-разметкой.".into());
    }
    let width = width_signed as u32;
    let height = height_signed.unsigned_abs();
    let count = pixel_count(width, height)?;
    let stride = ((width as usize * bits as usize + 31) / 32) * 4;
    let length = stride
        .checked_mul(height as usize)
        .ok_or("Повреждён BMP.")?;
    if offset < 14usize.saturating_add(dib_size as usize)
        || offset
            .checked_add(length)
            .is_none_or(|end| end > bytes.len())
    {
        return Err("BMP обрывается раньше конца данных.".into());
    }
    let mut pixels = blank_pixels(count)?;
    // BI_RGB's fourth byte is normally reserved, so import it as opaque.
    for y in 0..height as usize {
        let source_y = if height_signed < 0 {
            y
        } else {
            height as usize - 1 - y
        };
        for x in 0..width as usize {
            let p = offset + source_y * stride + x * (bits as usize / 8);
            pixels[y * width as usize + x] = Color::rgb(bytes[p + 2], bytes[p + 1], bytes[p]);
        }
    }
    Ok((width, height, pixels))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn red_brush() -> Brush {
        Brush {
            color: Color::rgb(230, 40, 30),
            size: 6.0,
            ..Brush::default()
        }
    }

    #[test]
    fn cached_stamp_alpha_and_reflected_bounds_match_full_mask_reference() {
        // Deliberately scan the complete mask with the original scalar alpha
        // calculation. This oracle does not use cached alpha or row bounds.
        fn reference_stamp(
            pixels: &mut [Color],
            width: usize,
            height: usize,
            mask: &BrushMask,
            brush: Brush,
            x: f32,
            y: f32,
            index: u32,
            compensation: f32,
        ) {
            let scatter = brush.scatter * brush.size;
            let x = x + (grain(index as i32, 17, 0x91) - 0.5) * scatter;
            let y = y + (grain(index as i32, 31, 0xb7) - 0.5) * scatter;
            let left = (x as f64 - mask.side as f64 * 0.5).round() as i32;
            let top = (y as f64 - mask.side as f64 * 0.5).round() as i32;
            for reflect in [false, true] {
                if reflect && (!brush.mirror || (width as f32 - x - x).abs() <= 0.01) {
                    continue;
                }
                for my in 0..mask.side {
                    for mx in 0..mask.side {
                        let px = if reflect {
                            width as i32 - 1 - (left + mx as i32)
                        } else {
                            left + mx as i32
                        };
                        let py = top + my as i32;
                        if px < 0 || py < 0 || px >= width as i32 || py >= height as i32 {
                            continue;
                        }
                        let n = mask.coverage[my * mask.side + mx];
                        let deposit = n as f32 / 255.0 * brush.opacity * brush.flow;
                        let deposit = if compensation > 1.0 {
                            1.0 - (1.0 - deposit).powf(compensation.min(64.0))
                        } else {
                            deposit
                        };
                        let a = (deposit * 255.0 + 0.5).clamp(0.0, 255.0) as u8;
                        if a == 0 {
                            continue;
                        }
                        let old = pixels[py as usize * width + px as usize];
                        pixels[py as usize * width + px as usize] = if brush.erase {
                            let amount = (a as u32 * brush.color.a as u32 + 127) / 255;
                            let a = ((old.a as u32 * (255 - amount) + 127) / 255) as u8;
                            if a == 0 {
                                Color::TRANSPARENT
                            } else {
                                Color { a, ..old }
                            }
                        } else {
                            blend(old, brush.color, a)
                        };
                    }
                }
            }
        }

        let original: Vec<Color> = (0..151 * 109)
            .map(|i| {
                Color::rgba(
                    (i * 13 % 256) as u8,
                    (i * 31 % 256) as u8,
                    97,
                    (i % 256) as u8,
                )
            })
            .collect();
        for kind in BrushKind::ALL {
            for erase in [false, true] {
                for mirror in [false, true] {
                    let mut brush = Brush {
                        kind,
                        erase,
                        mirror,
                        size: 37.0,
                        angle: 31.0,
                        hardness: 0.58,
                        flow: 0.61,
                        scatter: 0.23,
                        ..red_brush()
                    };
                    let mut mask = BrushMask::new(brush);
                    let mut expected = original.clone();
                    let mut doc = Document::from_pixels(151, 109, original.clone()).unwrap();
                    doc.begin_raster_edit();
                    for (index, (x, y, opacity, compensation, source_alpha)) in [
                        (3.2, 8.4, 0.37, 1.0, 171),
                        (75.5, 54.5, 0.003, 9.7, 171),
                        (151.2, 102.6, 0.8, 1.0, 255),
                        (5.8, 102.3, 0.8, 1.0, 0),
                    ]
                    .into_iter()
                    .enumerate()
                    {
                        brush.opacity = opacity;
                        brush.color.a = source_alpha;
                        mask.prepare_alpha(brush, compensation);
                        doc.stamp(x, y, brush, &mask, index as u32);
                        reference_stamp(
                            &mut expected,
                            151,
                            109,
                            &mask,
                            brush,
                            x,
                            y,
                            index as u32,
                            compensation,
                        );
                        assert_eq!(
                            doc.layers[0].pixels, expected,
                            "{kind:?}, erase={erase}, mirror={mirror}, stamp={index}"
                        );
                    }
                    doc.cancel_edit();
                    assert_eq!(doc.layers[0].pixels, original);
                    assert!(!doc.is_dirty());
                }
            }
        }
    }

    #[test]
    fn a_stroke_is_one_undo_step_and_saved_revision_tracks_undo() {
        let mut doc = Document::new(64, 40).unwrap();
        doc.begin_stroke(4.0, 8.0, red_brush()).unwrap();
        for x in 5..50 {
            doc.continue_stroke(x as f32, 8.0);
        }
        assert!(doc.end_stroke());
        assert_eq!(doc.undo_count(), 1);
        assert!(doc.is_dirty());
        assert_eq!(doc.sample(30, 8).r, 230);
        doc.mark_saved();
        assert!(!doc.is_dirty());
        assert!(doc.undo());
        assert_eq!(doc.sample(30, 8), Color::TRANSPARENT);
        assert!(doc.is_dirty());
        assert!(doc.redo());
        assert!(!doc.is_dirty());
    }

    #[test]
    fn cancel_restores_layer_pixels_and_noop_creates_no_history() {
        let mut doc = Document::new(32, 32).unwrap();
        doc.begin_stroke(12.0, 12.0, red_brush()).unwrap();
        doc.cancel_stroke();
        assert!(doc.layers[0]
            .pixels
            .iter()
            .all(|&p| p == Color::TRANSPARENT));
        assert!(!doc.is_dirty());
        assert!(!doc.can_undo());
        doc.begin_edit();
        assert!(!doc.commit_edit());
        doc.draw_line(-40.0, -40.0, -20.0, -20.0, red_brush())
            .unwrap();
        assert!(!doc.is_dirty());
        assert!(!doc.can_undo());
    }

    #[test]
    fn live_shapes_match_committed_raster_at_large_sizes_alpha_texture_and_mirror() {
        let baseline = vec![Color::rgba(28, 71, 109, 143); 257 * 193];
        for kind in [ShapeKind::Line, ShapeKind::Rectangle, ShapeKind::Ellipse] {
            for brush in [
                Brush {
                    size: 96.0,
                    opacity: 0.37,
                    flow: 0.71,
                    mirror: true,
                    ..red_brush()
                },
                Brush {
                    size: 512.0,
                    kind: BrushKind::Soft,
                    hardness: 0.4,
                    opacity: 0.18,
                    flow: 0.6,
                    spacing: 0.25,
                    mirror: true,
                    ..red_brush()
                },
                Brush {
                    size: 57.0,
                    kind: BrushKind::Calligraphy,
                    angle: 57.0,
                    scatter: 0.23,
                    opacity: 0.46,
                    flow: 0.83,
                    mirror: true,
                    erase: true,
                    ..red_brush()
                },
            ] {
                let mut expected = Document::from_pixels(257, 193, baseline.clone()).unwrap();
                match kind {
                    ShapeKind::Line => expected.draw_line(28.25, 32.5, 137.5, 128.75, brush),
                    ShapeKind::Rectangle => {
                        expected.draw_rectangle(28.25, 32.5, 137.5, 128.75, brush)
                    }
                    ShapeKind::Ellipse => expected.draw_ellipse(28.25, 32.5, 137.5, 128.75, brush),
                }
                .unwrap();
                let mut live = Document::from_pixels(257, 193, baseline.clone()).unwrap();
                assert!(live
                    .preview_shape(kind, 28.25, 32.5, 88.0, 84.0, brush)
                    .unwrap());
                assert!(live
                    .preview_shape(kind, 28.25, 32.5, 137.5, 128.75, brush)
                    .unwrap());
                assert_eq!(live.undo_count(), 0, "preview created history for {kind:?}");
                assert_eq!(live.layers, expected.layers, "preview differs for {kind:?}");
                assert_eq!(live.composite(), expected.composite());
                let shown = live.layers.clone();
                live.take_paint_damage();
                assert!(!live
                    .preview_shape(kind, 28.25, 32.5, 137.5, 128.75, brush)
                    .unwrap());
                assert!(live.take_paint_damage().is_none());
                assert!(live.commit_shape_preview());
                assert_eq!(live.layers, shown, "commit changed displayed {kind:?}");
                assert!(live.take_paint_damage().is_none());
                assert_eq!(live.undo_count(), 1);
                assert!(!live.commit_shape_preview());
                assert!(live.undo());
                assert_eq!(live.layers[0].pixels, baseline);
                assert!(!live.is_dirty());
                assert!(live.redo());
                assert_eq!(live.layers, shown);
            }
        }
    }

    #[test]
    fn live_shape_damage_restores_old_geometry_and_cancel_preserves_redo() {
        let mut doc = Document::with_background(513, 257, Color::WHITE).unwrap();
        doc.draw_line(20.0, 20.0, 25.0, 22.0, red_brush()).unwrap();
        doc.undo();
        let original = doc.layers.clone();
        let mut composite = doc.composite();
        doc.take_paint_damage();
        let brush = Brush {
            size: 8.0,
            opacity: 0.43,
            ..red_brush()
        };
        for (kind, x0, y0, x1, y1) in [
            (ShapeKind::Line, 5.0, 5.0, 22.0, 21.0),
            (ShapeKind::Rectangle, 470.0, 230.0, 509.0, 254.0),
            (ShapeKind::Ellipse, 74.0, 88.0, 112.0, 115.0),
        ] {
            assert!(doc.preview_shape(kind, x0, y0, x1, y1, brush).unwrap());
            let damage = doc.take_paint_damage().unwrap();
            assert!(doc.composite_region_into(&mut composite, damage));
            assert_eq!(composite, doc.composite());
            assert_eq!(doc.undo_count(), 0);
            assert_eq!(doc.redo_count(), 1);
        }
        assert!(doc.cancel_shape_preview());
        let damage = doc.take_paint_damage().unwrap();
        assert!(doc.composite_region_into(&mut composite, damage));
        assert_eq!(composite, doc.composite());
        assert_eq!(doc.layers, original);
        assert!(!doc.is_dirty());
        assert_eq!(doc.redo_count(), 1);
        assert!(!doc.cancel_shape_preview());
        assert!(doc.redo());
    }

    #[test]
    fn live_shape_reuses_mask_and_tile_storage_without_accumulating_visited_regions() {
        let mut doc = Document::new(512, 512).unwrap();
        let brush = Brush {
            size: 8.0,
            ..red_brush()
        };
        doc.preview_shape(ShapeKind::Line, 20.0, 20.0, 28.0, 24.0, brush)
            .unwrap();
        let mask = doc.shape_preview.as_ref().unwrap().mask.coverage.as_ptr();
        let tile_buffer = match &doc.pending {
            Some(PendingEdit::Raster(raster)) => {
                assert_eq!(raster.tiles.iter().flatten().count(), 1);
                raster
                    .tiles
                    .iter()
                    .flatten()
                    .next()
                    .unwrap()
                    .pixels
                    .as_ptr()
            }
            _ => panic!("shape must own a sparse raster transaction"),
        };
        for (x, y) in [(148.0, 148.0), (276.0, 276.0), (404.0, 404.0)] {
            doc.preview_shape(
                ShapeKind::Line,
                x,
                y,
                x + 8.0,
                y + 4.0,
                Brush {
                    opacity: 0.4,
                    ..brush
                },
            )
            .unwrap();
            assert_eq!(
                doc.shape_preview.as_ref().unwrap().mask.coverage.as_ptr(),
                mask
            );
            match &doc.pending {
                Some(PendingEdit::Raster(raster)) => {
                    assert_eq!(raster.tiles.iter().flatten().count(), 1);
                    assert!(raster.recycled_tiles.is_empty());
                    assert_eq!(
                        raster
                            .tiles
                            .iter()
                            .flatten()
                            .next()
                            .unwrap()
                            .pixels
                            .as_ptr(),
                        tile_buffer
                    );
                }
                _ => panic!("shape preview copied a full snapshot"),
            }
        }
        doc.cancel_shape_preview();
        assert!(doc.layers[0]
            .pixels
            .iter()
            .all(|p| *p == Color::TRANSPARENT));
    }

    #[test]
    fn live_shape_never_takes_or_cancels_an_unrelated_pending_edit() {
        let mut doc = Document::new(96, 96).unwrap();
        doc.begin_layer_metadata_edit();
        doc.set_layer_opacity(0, 122);
        assert!(doc
            .preview_shape(ShapeKind::Line, 10.0, 10.0, 50.0, 50.0, red_brush())
            .is_err());
        assert!(!doc.cancel_shape_preview());
        assert!(!doc.commit_shape_preview());
        assert_eq!(doc.layers[0].opacity, 122);
        assert!(matches!(doc.pending, Some(PendingEdit::Metadata(_))));
        doc.cancel_edit();
        doc.begin_stroke(10.0, 10.0, red_brush()).unwrap();
        let stroke_pixels = doc.layers[0].pixels.clone();
        assert!(doc
            .preview_shape(ShapeKind::Rectangle, 10.0, 10.0, 50.0, 50.0, red_brush())
            .is_err());
        assert!(!doc.cancel_shape_preview());
        assert!(!doc.commit_shape_preview());
        assert_eq!(doc.layers[0].pixels, stroke_pixels);
        assert!(doc.stroke.is_some());
        doc.cancel_stroke();

        doc.preview_shape(ShapeKind::Line, 10.0, 10.0, 50.0, 50.0, red_brush())
            .unwrap();
        doc.begin_edit();
        doc.layers[0].pixels[0] = Color::WHITE;
        let combined_pixels = doc.layers[0].pixels.clone();
        assert!(!doc.cancel_shape_preview());
        assert!(!doc.commit_shape_preview());
        assert_eq!(doc.layers[0].pixels, combined_pixels);
        assert!(matches!(doc.pending, Some(PendingEdit::Snapshot(_))));
        doc.cancel_edit();
        assert!(doc.layers[0]
            .pixels
            .iter()
            .all(|p| *p == Color::TRANSPARENT));
        assert!(!doc.is_dirty());
    }

    #[test]
    fn live_shape_invalid_update_is_atomic_and_empty_preview_is_not_history() {
        let mut doc = Document::new(96, 96).unwrap();
        let brush = red_brush();
        doc.preview_shape(ShapeKind::Ellipse, 10.0, 10.0, 50.0, 50.0, brush)
            .unwrap();
        let shown = doc.layers.clone();
        doc.take_paint_damage();
        assert!(doc
            .preview_shape(ShapeKind::Ellipse, 10.0, 10.0, f32::NAN, 50.0, brush)
            .is_err());
        assert_eq!(doc.layers, shown);
        assert!(doc.take_paint_damage().is_none());
        assert!(doc.commit_shape_preview());
        assert_eq!(doc.layers, shown);
        doc.undo();
        doc.take_paint_damage();
        let invisible = Brush {
            opacity: 0.0,
            ..brush
        };
        assert!(doc
            .preview_shape(ShapeKind::Rectangle, 10.0, 10.0, 50.0, 50.0, invisible)
            .unwrap());
        assert!(!doc.is_dirty());
        assert!(doc.take_paint_damage().is_none());
        assert!(!doc.commit_shape_preview());
        assert_eq!(doc.undo_count(), 0);
        assert_eq!(doc.redo_count(), 1);
    }

    #[test]
    fn closed_shapes_fill_the_center_with_opacity_flow_and_source_alpha() {
        let brush = Brush {
            color: Color::rgba(180, 30, 70, 128),
            size: 3.0,
            opacity: 0.5,
            flow: 0.5,
            ..red_brush()
        };
        for kind in [ShapeKind::Rectangle, ShapeKind::Ellipse] {
            let mut hollow = Document::new(96, 80).unwrap();
            match kind {
                ShapeKind::Rectangle => hollow.draw_rectangle(12.25, 10.5, 83.75, 69.5, brush),
                _ => hollow.draw_ellipse(12.25, 10.5, 83.75, 69.5, brush),
            }
            .unwrap();
            assert_eq!(hollow.sample(48, 40), Color::TRANSPARENT);
            let mut filled = Document::new(96, 80).unwrap();
            filled
                .draw_shape_with_fill(kind, 12.25, 10.5, 83.75, 69.5, brush, true)
                .unwrap();
            assert_eq!(
                filled.sample(48, 40),
                blend(Color::TRANSPARENT, brush.color, 64)
            );
            assert_eq!(filled.sample(0, 0), Color::TRANSPARENT);
            assert_eq!(filled.undo_count(), 1);
            let mut compatible = Document::new(96, 80).unwrap();
            compatible
                .draw_shape_with_fill(kind, 12.25, 10.5, 83.75, 69.5, brush, false)
                .unwrap();
            assert_eq!(compatible.layers, hollow.layers);

            let mut erase =
                Document::from_pixels(96, 80, vec![Color::rgba(20, 60, 90, 211); 96 * 80]).unwrap();
            erase
                .draw_shape_with_fill(
                    kind,
                    12.25,
                    10.5,
                    83.75,
                    69.5,
                    Brush {
                        erase: true,
                        ..brush
                    },
                    true,
                )
                .unwrap();
            assert_eq!(
                erase.sample(48, 40),
                Color::rgba(20, 60, 90, ((211u32 * (255 - 32) + 127) / 255) as u8)
            );
        }
    }

    #[test]
    fn filled_preview_replaces_outline_and_commits_exactly_one_reversible_edit() {
        for kind in [ShapeKind::Rectangle, ShapeKind::Ellipse] {
            let baseline = vec![Color::rgba(20, 70, 115, 93); 130 * 98];
            let mut doc = Document::from_pixels(130, 98, baseline.clone()).unwrap();
            let mut cached_composite = doc.composite();
            let brush = Brush {
                size: 9.0,
                opacity: 0.4,
                flow: 0.8,
                mirror: true,
                ..red_brush()
            };
            for filled in [true, false, true] {
                assert!(doc
                    .preview_shape_with_fill(kind, 99.75, 77.5, 12.25, 15.5, brush, filled)
                    .unwrap());
                let mut expected = Document::from_pixels(130, 98, baseline.clone()).unwrap();
                expected
                    .draw_shape_with_fill(kind, 99.75, 77.5, 12.25, 15.5, brush, filled)
                    .unwrap();
                assert_eq!(doc.layers, expected.layers);
                assert_eq!(doc.undo_count(), 0);
                let damage = doc.take_paint_damage().unwrap();
                assert!(doc.composite_region_into(&mut cached_composite, damage));
                assert_eq!(cached_composite, doc.composite());
            }
            let shown = doc.layers.clone();
            assert!(!doc
                .preview_shape_with_fill(kind, 99.75, 77.5, 12.25, 15.5, brush, true)
                .unwrap());
            assert!(doc.take_paint_damage().is_none());
            assert!(doc.commit_shape_preview());
            assert_eq!(doc.layers, shown);
            assert!(doc.take_paint_damage().is_none());
            assert_eq!(doc.undo_count(), 1);
            assert!(doc.undo());
            assert_eq!(doc.layers[0].pixels, baseline);
            assert!(doc.redo());
            assert_eq!(doc.layers, shown);
            doc.take_paint_damage();
            doc.preview_shape_with_fill(kind, -15.0, -7.0, 40.25, 45.5, brush, true)
                .unwrap();
            let damage = doc.take_paint_damage().unwrap();
            assert!(doc.composite_region_into(&mut cached_composite, damage));
            assert!(doc.cancel_shape_preview());
            assert_eq!(doc.layers, shown);
            let damage = doc.take_paint_damage().unwrap();
            assert!(doc.composite_region_into(&mut cached_composite, damage));
            assert_eq!(cached_composite, doc.composite());
            assert_eq!(doc.undo_count(), 1);
        }
    }

    #[test]
    fn fill_scanlines_antialias_clip_and_accept_reversed_or_extreme_endpoints() {
        let brush = Brush {
            color: Color::BLACK,
            size: 1.0,
            ..red_brush()
        };
        for kind in [ShapeKind::Rectangle, ShapeKind::Ellipse] {
            let mut a = Document::new(64, 48).unwrap();
            let mut b = Document::new(64, 48).unwrap();
            a.begin_raster_edit();
            b.begin_raster_edit();
            a.fill_shape_interior(
                ShapeGeometry::new(kind, -12.25, 10.5, 35.75, 57.8, brush, true).unwrap(),
            );
            b.fill_shape_interior(
                ShapeGeometry::new(kind, 35.75, 57.8, -12.25, 10.5, brush, true).unwrap(),
            );
            assert_eq!(a.layers, b.layers);
            assert!(a.layers[0].pixels.iter().any(|p| p.a > 0 && p.a < 255));
            assert_eq!(a.sample(63, 0), Color::TRANSPARENT);
            assert!(a.commit_edit());
            assert!(a.undo());
            assert!(a.layers[0].pixels.iter().all(|p| *p == Color::TRANSPARENT));
        }
        let mut rect = Document::new(64, 48).unwrap();
        rect.begin_raster_edit();
        rect.fill_shape_interior(
            ShapeGeometry::new(ShapeKind::Rectangle, 10.25, 10.5, 35.75, 30.8, brush, true)
                .unwrap(),
        );
        assert_eq!(rect.sample(10, 10).a, 96); // 3/4 horizontal × 1/2 vertical coverage.
        assert_eq!(rect.sample(11, 11).a, 255);
        assert_eq!(rect.sample(9, 10).a, 0);
        rect.cancel_edit();
        for kind in [ShapeKind::Rectangle, ShapeKind::Ellipse] {
            rect.begin_raster_edit();
            rect.fill_shape_interior(
                ShapeGeometry::new(kind, -f32::MAX, -f32::MAX, f32::MAX, f32::MAX, brush, true)
                    .unwrap(),
            );
            assert!(rect.layers[0].pixels.iter().all(|p| *p == Color::BLACK));
            rect.cancel_edit();
        }
        rect.begin_raster_edit();
        rect.fill_shape_interior(
            ShapeGeometry::new(ShapeKind::Ellipse, 10.0, 10.0, 40.0, 10.1, brush, true).unwrap(),
        );
        assert!(rect.layers[0].pixels.iter().any(|p| p.a != 0));
        assert!(!rect.layers[0].pixels.iter().any(|p| p.a == 255));
    }

    #[test]
    fn filled_shape_mirror_reflects_the_same_fractional_coverage_and_contour() {
        let width = 128usize;
        let height = 80usize;
        let brush = Brush {
            size: 3.0,
            opacity: 0.43,
            color: Color::rgba(210, 40, 90, 173),
            ..red_brush()
        };
        for kind in [ShapeKind::Rectangle, ShapeKind::Ellipse] {
            let mut single = Document::new(width as u32, height as u32).unwrap();
            let mut mirrored = Document::new(width as u32, height as u32).unwrap();
            single
                .draw_shape_with_fill(kind, 7.25, 12.5, 31.75, 67.2, brush, true)
                .unwrap();
            mirrored
                .preview_shape_with_fill(
                    kind,
                    7.25,
                    12.5,
                    31.75,
                    67.2,
                    Brush {
                        mirror: true,
                        ..brush
                    },
                    true,
                )
                .unwrap();
            for y in 0..height {
                for x in 0..width {
                    let expected = blend(
                        single.layers[0].pixels[y * width + x],
                        single.layers[0].pixels[y * width + width - 1 - x],
                        255,
                    );
                    assert_eq!(
                        mirrored.layers[0].pixels[y * width + x],
                        expected,
                        "{kind:?} at {x},{y}"
                    );
                }
            }
            assert!(mirrored.commit_shape_preview());
            assert_eq!(mirrored.undo_count(), 1);
            assert!(mirrored.undo());
            assert!(mirrored.layers[0]
                .pixels
                .iter()
                .all(|p| *p == Color::TRANSPARENT));
        }
    }

    #[test]
    fn filled_shape_guards_noop_alpha_and_line_fill_flag() {
        let mut doc = Document::new(80, 64).unwrap();
        doc.layers[0].locked = true;
        assert!(doc
            .preview_shape_with_fill(
                ShapeKind::Rectangle,
                5.0,
                5.0,
                70.0,
                55.0,
                red_brush(),
                true
            )
            .is_err());
        assert!(doc
            .draw_shape_with_fill(ShapeKind::Ellipse, 5.0, 5.0, 70.0, 55.0, red_brush(), true)
            .is_err());
        assert!(doc.pending.is_none());
        assert!(!doc.is_dirty());
        doc.layers[0].locked = false;
        for brush in [
            Brush {
                opacity: 0.0,
                ..red_brush()
            },
            Brush {
                flow: 0.0,
                ..red_brush()
            },
            Brush {
                color: Color::rgba(200, 30, 70, 0),
                ..red_brush()
            },
        ] {
            assert!(doc
                .preview_shape_with_fill(ShapeKind::Rectangle, 5.0, 5.0, 70.0, 55.0, brush, true)
                .unwrap());
            assert!(!doc.is_dirty());
            assert!(doc.take_paint_damage().is_none());
            assert!(!doc.commit_shape_preview());
            assert_eq!(doc.undo_count(), 0);
        }
        assert!(doc
            .preview_shape_with_fill(ShapeKind::Line, 5.0, 5.0, 70.0, 55.0, red_brush(), true)
            .unwrap());
        doc.take_paint_damage();
        assert!(!doc
            .preview_shape(ShapeKind::Line, 5.0, 5.0, 70.0, 55.0, red_brush())
            .unwrap());
        assert!(doc.take_paint_damage().is_none());
        let mut expected = Document::new(80, 64).unwrap();
        expected
            .draw_line(5.0, 5.0, 70.0, 55.0, red_brush())
            .unwrap();
        assert_eq!(doc.layers, expected.layers);
        doc.cancel_shape_preview();
        doc.begin_layer_metadata_edit();
        doc.set_layer_opacity(0, 123);
        assert!(doc
            .preview_shape_with_fill(ShapeKind::Ellipse, 5.0, 5.0, 70.0, 55.0, red_brush(), true)
            .is_err());
        assert!(!doc.cancel_shape_preview());
        assert_eq!(doc.layers[0].opacity, 123);
        doc.cancel_edit();
    }

    #[test]
    fn straight_alpha_and_layer_opacity_compose_correctly() {
        assert_eq!(
            blend(Color::TRANSPARENT, Color::rgba(80, 160, 240, 128), 255),
            Color::rgba(80, 160, 240, 128)
        );
        let result = blend(Color::WHITE, Color::BLACK, 128);
        assert_eq!(result, Color::rgb(127, 127, 127));
        let mut doc = Document::new(2, 2).unwrap();
        doc.layers[0].pixels.fill(Color::WHITE);
        let top = doc.add_layer("Top").unwrap();
        doc.layers[top].pixels.fill(Color::BLACK);
        doc.set_layer_opacity(top, 128);
        assert_eq!(doc.sample(0, 0), Color::rgb(127, 127, 127));
        doc.set_layer_visibility(top, false);
        assert_eq!(doc.sample(0, 0), Color::WHITE);
    }

    #[test]
    fn symmetry_and_erase_are_one_reversible_edit() {
        let mut doc = Document::new(80, 48).unwrap();
        let brush = Brush {
            mirror: true,
            size: 4.0,
            ..red_brush()
        };
        doc.draw_line(10.0, 10.0, 10.0, 35.0, brush).unwrap();
        assert!(doc.sample(10, 20).a > 0);
        assert!(doc.sample(69, 20).a > 0);
        assert_eq!(doc.undo_count(), 1);
        doc.draw_line(
            10.0,
            10.0,
            10.0,
            35.0,
            Brush {
                erase: true,
                ..brush
            },
        )
        .unwrap();
        assert_eq!(doc.sample(10, 20).a, 0);
        doc.undo();
        assert!(doc.sample(10, 20).a > 0);
    }

    #[test]
    fn fill_respects_a_boundary_and_locked_layer() {
        let mut doc = Document::new(12, 10).unwrap();
        for y in 0..10 {
            doc.layers[0].pixels[y * 12 + 6] = Color::BLACK;
        }
        assert!(doc.flood_fill(0, 0, Color::WHITE, 0).unwrap());
        assert_eq!(doc.sample(5, 5), Color::WHITE);
        assert_eq!(doc.sample(6, 5), Color::BLACK);
        assert_eq!(doc.sample(7, 5), Color::TRANSPARENT);
        doc.undo();
        assert_eq!(doc.sample(5, 5), Color::TRANSPARENT);
        doc.set_layer_locked(0, true);
        assert!(doc.begin_stroke(2.0, 2.0, red_brush()).is_err());
        assert!(doc.flood_fill(0, 0, Color::WHITE, 0).is_err());
    }

    #[test]
    fn project_roundtrip_preserves_layers_and_unicode() {
        let mut doc = Document::with_background(18, 12, Color::from_hex(0xf2ead3)).unwrap();
        doc.title = "Этюд — 雨".into();
        doc.rename_layer(1, "Кисть и цвет").unwrap();
        doc.draw_line(2.0, 3.0, 14.0, 9.0, red_brush()).unwrap();
        doc.add_layer("Скрытый").unwrap();
        doc.set_layer_visibility(2, false);
        doc.set_layer_opacity(1, 130);
        let reopened = Document::from_bytes(&doc.to_bytes().unwrap()).unwrap();
        assert_eq!(reopened.title, doc.title);
        assert_eq!(reopened.layers, doc.layers);
        assert_eq!(reopened.active, doc.active);
        assert_eq!(reopened.composite(), doc.composite());
        assert!(!reopened.is_dirty());
        assert!(!reopened.can_undo());
    }

    #[test]
    fn malformed_project_is_rejected_without_large_allocation() {
        let doc = Document::new(2, 2).unwrap();
        let bytes = doc.to_bytes().unwrap();
        for end in 0..bytes.len() {
            assert!(Document::from_bytes(&bytes[..end]).is_err());
        }
        let mut broken = bytes.clone();
        broken[8..12].copy_from_slice(&u32::MAX.to_le_bytes());
        let at = broken.len() - 4;
        let checksum = crc32(&broken[..at]);
        broken[at..].copy_from_slice(&checksum.to_le_bytes());
        assert!(Document::from_bytes(&broken).is_err());
        let mut wrong_crc = bytes;
        wrong_crc[31] ^= 0x40;
        assert!(Document::from_bytes(&wrong_crc).is_err());
    }

    #[test]
    fn png_has_valid_chunks_checksums_and_deflate_data() {
        let pixels = [Color::rgba(12, 34, 56, 78), Color::rgb(90, 100, 110)];
        let png = encode_png(2, 1, &pixels).unwrap();
        assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
        let mut at = 8;
        let mut seen_idat = false;
        while at < png.len() {
            let len = u32::from_be_bytes(png[at..at + 4].try_into().unwrap()) as usize;
            let kind = &png[at + 4..at + 8];
            let body = &png[at + 8..at + 8 + len];
            let check = u32::from_be_bytes(png[at + 8 + len..at + 12 + len].try_into().unwrap());
            assert_eq!(crc32(&png[at + 4..at + 8 + len]), check);
            if kind == b"IDAT" {
                seen_idat = true;
                assert_eq!(&body[..3], &[0x78, 0x01, 0x01]);
                let size = u16::from_le_bytes([body[3], body[4]]) as usize;
                assert_eq!(size, 9);
                assert_eq!(u16::from_le_bytes([body[5], body[6]]), !(size as u16));
                assert_eq!(&body[7..7 + size], &[0, 12, 34, 56, 78, 90, 100, 110, 255]);
                assert_eq!(
                    u32::from_be_bytes(body[7 + size..].try_into().unwrap()),
                    adler32(&body[7..7 + size])
                );
            }
            at += len + 12;
        }
        assert!(seen_idat);
    }

    #[test]
    fn bmp_topdown_and_bottomup_decode_to_identical_pixels() {
        let mut bmp = vec![0u8; 54 + 16];
        bmp[..2].copy_from_slice(b"BM");
        bmp[10..14].copy_from_slice(&54u32.to_le_bytes());
        bmp[14..18].copy_from_slice(&40u32.to_le_bytes());
        bmp[18..22].copy_from_slice(&2i32.to_le_bytes());
        bmp[22..26].copy_from_slice(&2i32.to_le_bytes());
        bmp[26..28].copy_from_slice(&1u16.to_le_bytes());
        bmp[28..30].copy_from_slice(&24u16.to_le_bytes());
        bmp[54..62].copy_from_slice(&[0, 0, 255, 0, 255, 0, 0, 0]);
        bmp[62..70].copy_from_slice(&[255, 0, 0, 255, 255, 255, 0, 0]);
        let (_, _, pixels) = decode_bmp(&bmp).unwrap();
        assert_eq!(
            pixels,
            vec![
                Color::rgb(0, 0, 255),
                Color::WHITE,
                Color::rgb(255, 0, 0),
                Color::rgb(0, 255, 0)
            ]
        );
        bmp[22..26].copy_from_slice(&(-2i32).to_le_bytes());
        let bottom = bmp[54..62].to_vec();
        bmp.copy_within(62..70, 54);
        bmp[62..70].copy_from_slice(&bottom);
        assert_eq!(decode_bmp(&bmp).unwrap().2, pixels);
    }

    #[test]
    fn atomic_write_can_replace_existing_file() {
        let dir = std::env::current_dir().unwrap();
        let path = dir.join(format!(
            "portfolio-core-test-{}-{}.tmp",
            std::process::id(),
            TEMP_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        atomic_write(&path, b"first").unwrap();
        atomic_write(&path, b"second").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"second");
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn delete_reorder_and_duplicate_restore_correct_active_layer() {
        let mut doc = Document::new(8, 8).unwrap();
        doc.add_layer("A").unwrap();
        doc.add_layer("B").unwrap();
        doc.move_layer(-1);
        assert_eq!(doc.layers[1].name, "B");
        doc.undo();
        assert_eq!(doc.layers[2].name, "B");
        assert_eq!(doc.active, 2);
        doc.duplicate_layer().unwrap();
        assert_eq!(doc.layers.len(), 4);
        doc.delete_layer().unwrap();
        assert_eq!(doc.layers.len(), 3);
        doc.undo();
        assert_eq!(doc.layers.len(), 4);
        assert_eq!(doc.active, 3);
    }

    #[test]
    fn tile_history_keeps_forty_strokes_without_copying_every_layer() {
        let mut doc = Document::with_background(512, 384, Color::WHITE).unwrap();
        doc.add_layer("Details").unwrap();
        doc.add_layer("Ink").unwrap();
        doc.clear_history();
        let initial = doc.layers.clone();
        for i in 0..40 {
            let x = 8.0 + (i % 10) as f32 * 48.0;
            let y = 16.0 + (i / 10) as f32 * 80.0;
            doc.draw_line(x, y, x + 20.0, y + 24.0, red_brush())
                .unwrap();
        }
        assert_eq!(doc.undo_count(), 40);
        assert!(doc.history_bytes() < 8 * 1024 * 1024);
        let finished = doc.layers.clone();
        for _ in 0..40 {
            assert!(doc.undo());
        }
        assert_eq!(doc.layers, initial);
        for _ in 0..40 {
            assert!(doc.redo());
        }
        assert_eq!(doc.layers, finished);
    }

    #[test]
    fn edge_tiles_and_combined_layer_edits_roundtrip() {
        let mut doc = Document::new(130, 70).unwrap();
        let start = doc.layers.clone();
        doc.begin_edit();
        doc.add_layer("Temporary").unwrap();
        doc.layers[1].pixels[130 * 69 + 129] = Color::rgba(1, 2, 3, 4);
        doc.rename_layer(0, "Original").unwrap();
        doc.layers[0].pixels[130 * 68 + 128] = Color::rgba(5, 6, 7, 8);
        doc.commit_edit();
        let changed = doc.layers.clone();
        assert_eq!(doc.undo_count(), 1);
        doc.undo();
        assert_eq!(doc.layers, start);
        doc.redo();
        assert_eq!(doc.layers, changed);
        doc.delete_layer().unwrap();
        doc.undo();
        assert_eq!(doc.layers, changed);
        doc.undo();
        assert_eq!(doc.layers, start);
    }

    fn paint_sample(brush: Brush) -> Document {
        let mut doc = Document::new(96, 80).unwrap();
        doc.begin_stroke(20.0, 30.0, brush).unwrap();
        doc.continue_stroke(46.0, 46.0);
        doc.continue_stroke(76.0, 26.0);
        doc.end_stroke();
        doc
    }

    #[test]
    fn all_ten_brushes_are_distinct_deterministic_and_eraser_compatible() {
        let mut results: Vec<Vec<Color>> = Vec::new();
        for kind in BrushKind::ALL {
            assert_eq!(BrushKind::from_index(kind.index()), kind);
            let brush = Brush {
                kind,
                size: 30.0,
                opacity: 0.7,
                flow: 0.8,
                spacing: 0.26,
                angle: 31.0,
                scatter: 0.12,
                ..red_brush()
            };
            let mut a = paint_sample(brush);
            let b = paint_sample(brush);
            let pixels = a.layers[0].pixels.clone();
            assert_eq!(pixels, b.layers[0].pixels);
            assert!(pixels.iter().any(|p| p.a != 0), "empty brush {kind:?}");
            assert!(
                results.iter().all(|previous| previous != &pixels),
                "duplicate {kind:?}"
            );
            results.push(pixels.clone());
            assert_eq!(a.undo_count(), 1);
            a.undo();
            assert!(a.layers[0].pixels.iter().all(|p| *p == Color::TRANSPARENT));
            a.redo();
            assert_eq!(a.layers[0].pixels, pixels);
            let mut erase = Document::from_pixels(96, 80, vec![Color::WHITE; 96 * 80]).unwrap();
            erase
                .draw_line(
                    20.0,
                    30.0,
                    76.0,
                    26.0,
                    Brush {
                        erase: true,
                        ..brush
                    },
                )
                .unwrap();
            assert!(
                erase.layers[0].pixels.iter().any(|p| p.a < 255),
                "eraser {kind:?}"
            );
            erase.undo();
            assert!(erase.layers[0].pixels.iter().all(|p| *p == Color::WHITE));
        }
        assert_eq!(BrushKind::from_index(255), BrushKind::Ink);
    }

    #[test]
    fn brush_parameters_change_deposition_geometry_and_sanitize_nonfinite_values() {
        let base = Brush {
            size: 28.0,
            opacity: 0.8,
            ..red_brush()
        };
        let plain = paint_sample(base).layers.remove(0).pixels;
        for brush in [
            Brush {
                hardness: 0.2,
                ..base
            },
            Brush { flow: 0.2, ..base },
            Brush {
                spacing: 1.2,
                ..base
            },
            Brush {
                scatter: 0.8,
                ..base
            },
        ] {
            assert_ne!(paint_sample(brush).layers[0].pixels, plain);
        }
        let a = Brush {
            kind: BrushKind::Calligraphy,
            ..base
        };
        assert_ne!(
            paint_sample(a).layers[0].pixels,
            paint_sample(Brush { angle: 90.0, ..a }).layers[0].pixels
        );
        let dirty = Brush {
            size: f32::NAN,
            opacity: f32::INFINITY,
            hardness: f32::NAN,
            flow: -3.0,
            spacing: f32::NEG_INFINITY,
            angle: f32::NAN,
            scatter: 6.0,
            ..base
        }
        .sanitized();
        assert_eq!(dirty.size, 20.0);
        assert_eq!(dirty.flow, 0.0);
        assert_eq!(dirty.scatter, 1.0);
        assert_eq!(dirty.spacing, 0.12);
        assert_eq!(dirty.angle, 0.0);
        assert_eq!(
            Brush {
                angle: -45.0,
                ..base
            }
            .sanitized()
            .angle,
            315.0
        );
        let mut doc = Document::new(32, 32).unwrap();
        assert!(doc.begin_stroke(f32::NAN, 4.0, base).is_err());
        assert!(doc.draw_rectangle(1.0, 2.0, f32::NAN, 6.0, base).is_err());
        assert!(!doc.is_dirty());
        assert!(doc.pending.is_none());
    }

    #[test]
    fn raster_edit_captures_only_touched_tiles_and_promotes_for_general_operations() {
        let mut doc = Document::with_background(256, 256, Color::WHITE).unwrap();
        doc.add_layer("detail").unwrap();
        doc.clear_history();
        doc.mark_saved();
        let original = doc.layers.clone();
        doc.begin_stroke(
            80.0,
            80.0,
            Brush {
                size: 8.0,
                ..red_brush()
            },
        )
        .unwrap();
        if let Some(PendingEdit::Raster(raster)) = &doc.pending {
            assert_eq!(raster.tiles.iter().flatten().count(), 1);
            assert_eq!(
                raster
                    .tiles
                    .iter()
                    .flatten()
                    .map(|t| t.pixels.len())
                    .sum::<usize>(),
                4096
            );
        } else {
            panic!("ordinary stroke copied a full snapshot");
        }
        assert!(doc.is_dirty());
        doc.add_layer("inside gesture").unwrap();
        assert!(matches!(doc.pending, Some(PendingEdit::Snapshot(_))));
        doc.layers[doc.active].pixels[1] = Color::BLACK;
        doc.end_stroke();
        assert_eq!(doc.undo_count(), 1);
        let result = doc.layers.clone();
        doc.undo();
        assert_eq!(doc.layers, original);
        doc.redo();
        assert_eq!(doc.layers, result);
        doc.begin_stroke(12.0, 12.0, red_brush()).unwrap();
        doc.begin_edit();
        doc.layers[doc.active].pixels[128] = Color::BLACK;
        doc.cancel_edit();
        assert_eq!(doc.layers, result);
    }

    #[test]
    fn paint_damage_recomposes_exactly_and_cancel_reports_restored_tiles() {
        let mut doc = Document::with_background(130, 70, Color::WHITE).unwrap();
        let mut output = doc.composite();
        doc.take_paint_damage();
        doc.begin_stroke(124.0, 66.0, red_brush()).unwrap();
        let bounds = doc.take_paint_damage().unwrap();
        assert!(bounds.2 < doc.width);
        assert!(doc.composite_region_into(&mut output, bounds));
        assert_eq!(output, doc.composite());
        assert!(doc.take_paint_damage().is_none());
        doc.continue_stroke(68.0, 35.0);
        let bounds = doc.take_paint_damage().unwrap();
        assert!(doc.composite_region_into(&mut output, bounds));
        assert_eq!(output, doc.composite());
        doc.cancel_stroke();
        let bounds = doc.take_paint_damage().unwrap();
        assert!(doc.composite_region_into(&mut output, bounds));
        assert_eq!(output, doc.composite());
        assert!(output.iter().all(|p| *p == Color::WHITE));
        let mut short = [Color::BLACK; 4];
        assert!(!doc.composite_region_into(&mut short, (0, 0, 2, 2)));
        assert_eq!(short, [Color::BLACK; 4]);
        assert!(doc.composite_region_into(&mut output, (u32::MAX, u32::MAX, u32::MAX, u32::MAX)));
    }

    #[test]
    fn extreme_pointer_segments_are_visible_clipped_and_have_a_fixed_work_budget() {
        let mut doc = Document::new(128, 128).unwrap();
        let brush = Brush {
            size: 48.0,
            kind: BrushKind::Soft,
            ..red_brush()
        };
        doc.begin_stroke(-f32::MAX, 64.0, brush).unwrap();
        assert_eq!(doc.last_sample_count, 0);
        doc.continue_stroke(f32::MAX, 64.0);
        assert!(doc.last_sample_count <= MAX_STROKE_SAMPLES);
        assert!(doc.layers[0].pixels[64 * 128 + 64].a > 0);
        doc.cancel_stroke();
        doc.begin_stroke(-f32::MAX, -f32::MAX, brush).unwrap();
        doc.continue_stroke(-f32::MAX * 0.5, -f32::MAX * 0.5);
        assert_eq!(doc.last_sample_count, 0);
        assert!(!doc.end_stroke());
        doc.begin_stroke(-f32::MAX, -f32::MAX, brush).unwrap();
        doc.continue_stroke(f32::MAX, f32::MAX);
        assert!(doc.last_sample_count <= MAX_STROKE_SAMPLES);
        assert!(doc.layers[0].pixels[64 * 128 + 64].a > 0);
        doc.cancel_stroke();
        let brush = Brush {
            size: 512.0,
            mirror: true,
            ..brush
        };
        doc.begin_stroke(-1e30, 64.0, brush).unwrap();
        doc.continue_stroke(1e30, 64.0);
        assert!(doc.last_sample_count <= MAX_STROKE_SAMPLES);
        assert!(doc.layers[0].pixels.iter().any(|p| p.a > 0));
        doc.cancel_stroke();
    }

    #[test]
    fn writer_snapshots_are_independent_and_exclude_history_and_pending_edits() {
        let mut doc = paint_sample(red_brush());
        doc.begin_stroke(20.0, 20.0, red_brush()).unwrap();
        let copy = doc.snapshot_for_io().unwrap();
        assert_eq!(copy.layers, doc.layers);
        assert_eq!(copy.width, doc.width);
        assert!(copy.undo.is_empty() && copy.redo.is_empty());
        assert!(copy.pending.is_none() && copy.stroke.is_none());
        let current = copy.layers.clone();
        doc.cancel_stroke();
        doc.layers[0].pixels[0] = Color::BLACK;
        assert_eq!(copy.layers, current);
        let loaded = Document::from_bytes(&copy.to_bytes().unwrap()).unwrap();
        assert_eq!(loaded.layers, current);
    }

    #[test]
    fn malformed_import_and_layer_buffers_do_not_partially_change_the_document() {
        let mut doc = Document::new(32, 32).unwrap();
        let original = doc.layers.clone();
        assert!(doc
            .add_image_layer("bad", 8, 8, &[Color::BLACK; 2])
            .is_err());
        assert_eq!(doc.layers, original);
        assert_eq!(doc.undo_count(), 0);
        doc.layers[0].pixels.pop();
        let broken = doc.layers.clone();
        assert!(doc.begin_stroke(2.0, 2.0, red_brush()).is_err());
        assert!(doc
            .add_image_layer("valid source", 1, 1, &[Color::BLACK])
            .is_err());
        assert!(doc.snapshot_for_io().is_err());
        assert_eq!(doc.layers, broken);
        assert!(doc.pending.is_none());
        let mut output = vec![Color::WHITE; 32 * 32];
        assert!(!doc.composite_region_into(&mut output, (0, 0, 32, 32)));
        assert!(output.iter().all(|p| *p == Color::WHITE));
    }

    #[test]
    fn optimized_blending_matches_straight_alpha_reference() {
        fn reference(dst: Color, src: Color, coverage: u8) -> Color {
            let sa = (src.a as u32 * coverage as u32 + 127) / 255;
            if sa == 0 {
                return dst;
            }
            let da = dst.a as u32;
            let a255 = sa * 255 + da * (255 - sa);
            let channel = |s: u8, d: u8| {
                ((s as u32 * sa * 255 + d as u32 * da * (255 - sa) + a255 / 2) / a255).min(255)
                    as u8
            };
            Color::rgba(
                channel(src.r, dst.r),
                channel(src.g, dst.g),
                channel(src.b, dst.b),
                ((a255 + 127) / 255).min(255) as u8,
            )
        }
        for a in 0..=255u8 {
            for b in 0..=255u8 {
                for coverage in [0, 1, 31, 127, 254, 255] {
                    for (dst, src) in [
                        (Color::rgba(17, 153, 238, a), Color::rgba(17, 153, 238, b)),
                        (Color::rgba(235, 18, 47, a), Color::rgba(83, 241, 14, b)),
                    ] {
                        assert_eq!(blend(dst, src, coverage), reference(dst, src, coverage));
                    }
                }
            }
        }
    }

    #[test]
    fn directional_and_grained_brushes_mirror_their_actual_nib() {
        for kind in [
            BrushKind::Calligraphy,
            BrushKind::Charcoal,
            BrushKind::Flat,
            BrushKind::Spray,
        ] {
            let mut doc = Document::new(160, 80).unwrap();
            let brush = Brush {
                kind,
                size: 28.0,
                angle: 37.0,
                mirror: true,
                ..red_brush()
            };
            doc.begin_stroke(32.0, 30.0, brush).unwrap();
            doc.end_stroke();
            let pixels = &doc.layers[0].pixels;
            for y in 0..80 {
                for x in 0..80 {
                    assert_eq!(pixels[y * 160 + x], pixels[y * 160 + 159 - x], "{kind:?}");
                }
            }
        }
    }

    #[test]
    fn layer_metadata_gestures_reuse_pixels_and_cancel_or_commit_once() {
        let mut doc = Document::with_background(129, 130, Color::WHITE).unwrap();
        doc.layers[1].pixels[129 + 17] = Color::rgb(14, 28, 56);
        doc.add_layer("Top").unwrap();
        doc.clear_history();
        doc.mark_saved();
        let original = doc.layers.clone();
        let original_active = doc.active;
        let allocations: Vec<_> = doc
            .layers
            .iter()
            .map(|l| (l.id, l.pixels.as_ptr()))
            .collect();
        for commit in [false, true] {
            doc.begin_layer_metadata_edit();
            assert!(matches!(doc.pending, Some(PendingEdit::Metadata(_))));
            for opacity in [220, 180, 110, 73] {
                doc.set_layer_opacity(doc.active, opacity);
                assert!(doc.is_dirty());
            }
            doc.rename_layer(doc.active, "Gesture").unwrap();
            doc.move_layer(-1);
            doc.set_layer_visibility(doc.active, false);
            doc.set_layer_locked(doc.active, true);
            for layer in &doc.layers {
                assert_eq!(
                    layer.pixels.as_ptr(),
                    allocations.iter().find(|a| a.0 == layer.id).unwrap().1
                );
            }
            assert!(matches!(doc.pending, Some(PendingEdit::Metadata(_))));
            if commit {
                let changed = doc.layers.clone();
                assert!(doc.commit_edit());
                assert_eq!(doc.undo_count(), 1);
                assert!(doc.history_bytes() < 4096);
                assert!(doc.undo());
                assert_eq!(doc.layers, original);
                assert_eq!(doc.active, original_active);
                assert!(!doc.is_dirty());
                assert!(doc.redo());
                assert_eq!(doc.layers, changed);
                for layer in &doc.layers {
                    assert_eq!(
                        layer.pixels.as_ptr(),
                        allocations.iter().find(|a| a.0 == layer.id).unwrap().1
                    );
                }
            } else {
                doc.cancel_edit();
                assert_eq!(doc.layers, original);
                assert_eq!(doc.active, original_active);
                assert!(!doc.is_dirty());
                assert_eq!(doc.undo_count(), 0);
            }
        }
        doc.clear_history();
        doc.mark_saved();
        doc.begin_layer_metadata_edit();
        let opacity = doc.active_layer().opacity;
        doc.set_layer_opacity(doc.active, 255);
        doc.set_layer_opacity(doc.active, opacity);
        assert!(!doc.is_dirty());
        assert!(!doc.commit_edit());
        assert_eq!(doc.undo_count(), 0);
    }

    #[test]
    fn metadata_transactions_promote_without_losing_order_pixels_or_original_state() {
        for structural in [false, true] {
            for commit in [false, true] {
                let mut doc = Document::new(129, 130).unwrap();
                doc.layers[0].pixels[0] = Color::BLACK;
                doc.add_layer("Top").unwrap();
                doc.clear_history();
                doc.mark_saved();
                let before = doc.layers.clone();
                let active = doc.active;
                doc.begin_layer_metadata_edit();
                doc.move_layer(-1);
                doc.set_layer_opacity(doc.active, 90);
                doc.begin_stroke(65.0, 70.0, red_brush()).unwrap();
                assert!(matches!(doc.pending, Some(PendingEdit::Raster(_))));
                doc.continue_stroke(80.0, 85.0);
                if structural {
                    doc.add_layer("Inside gesture").unwrap();
                    assert!(matches!(doc.pending, Some(PendingEdit::Snapshot(_))));
                    doc.layers[doc.active].pixels[128] = Color::BLACK;
                }
                if commit {
                    doc.end_stroke();
                    let after = doc.layers.clone();
                    assert_eq!(doc.undo_count(), 1);
                    doc.undo();
                    assert_eq!(doc.layers, before);
                    assert_eq!(doc.active, active);
                    doc.redo();
                    assert_eq!(doc.layers, after);
                } else {
                    doc.cancel_edit();
                    assert_eq!(doc.layers, before);
                    assert_eq!(doc.active, active);
                    assert!(!doc.is_dirty());
                    assert_eq!(doc.undo_count(), 0);
                }
            }
        }
        let mut doc = Document::new(17, 19).unwrap();
        doc.add_layer("Top").unwrap();
        doc.clear_history();
        let before = doc.layers.clone();
        doc.begin_layer_metadata_edit();
        doc.move_layer(-1);
        doc.begin_edit();
        assert!(matches!(doc.pending, Some(PendingEdit::Snapshot(_))));
        doc.layers[doc.active].pixels[1] = Color::BLACK;
        doc.cancel_edit();
        assert_eq!(doc.layers, before);
    }

    #[test]
    fn structural_layer_commands_preserve_unrelated_buffers_and_exact_history() {
        let mut doc = Document::new(129, 130).unwrap();
        for (i, pixel) in doc.layers[0].pixels.iter_mut().enumerate() {
            *pixel = Color::rgba(i as u8, (i / 3) as u8, (i / 7) as u8, (i / 11) as u8);
        }
        let pixels_ptr = doc.layers[0].pixels.as_ptr();
        let base = doc.layers.clone();
        doc.add_layer("Empty").unwrap();
        assert!(
            doc.history_bytes() < 2048,
            "blank layer history should contain only metadata"
        );
        assert_eq!(doc.layers[0].pixels.as_ptr(), pixels_ptr);
        let added = doc.layers.clone();
        doc.undo();
        assert_eq!(doc.layers, base);
        doc.redo();
        assert_eq!(doc.layers, added);
        doc.active = 0;
        doc.duplicate_layer().unwrap();
        assert_eq!(doc.layers[0].pixels.as_ptr(), pixels_ptr);
        assert_eq!(doc.layers[1].pixels, doc.layers[0].pixels);
        let duplicated = doc.layers.clone();
        doc.undo();
        assert_eq!(doc.layers, added);
        doc.redo();
        assert_eq!(doc.layers, duplicated);
        doc.clear_history();
        let removed_ptr = doc.active_layer().pixels.as_ptr();
        doc.delete_layer().unwrap();
        let deleted = doc.layers.clone();
        assert_eq!(doc.layers[0].pixels.as_ptr(), pixels_ptr);
        match &doc.undo.back().unwrap().layers[0].pixels {
            PixelDelta::Whole(pixels) => assert_eq!(pixels.as_ptr(), removed_ptr),
            _ => panic!("delete should transfer its buffer directly to Undo"),
        }
        doc.undo();
        assert_eq!(doc.layers, duplicated);
        doc.redo();
        assert_eq!(doc.layers, deleted);
    }

    #[test]
    fn span_fill_matches_reference_for_tolerance_alpha_edges_and_undo() {
        fn reference(
            pixels: &mut [Color],
            w: usize,
            h: usize,
            seed: usize,
            color: Color,
            tolerance: u8,
        ) -> bool {
            let original = pixels[seed];
            if original == color {
                return false;
            }
            let matches = |p: Color| {
                p.r.abs_diff(original.r) <= tolerance
                    && p.g.abs_diff(original.g) <= tolerance
                    && p.b.abs_diff(original.b) <= tolerance
                    && p.a.abs_diff(original.a) <= tolerance
            };
            let mut visited = vec![false; pixels.len()];
            let mut queue = VecDeque::from([seed]);
            visited[seed] = true;
            while let Some(at) = queue.pop_front() {
                if !matches(pixels[at]) {
                    continue;
                }
                pixels[at] = color;
                let (x, y) = (at % w, at / w);
                for next in [
                    (x > 0).then(|| at - 1),
                    (x + 1 < w).then_some(at + 1),
                    (y > 0).then(|| at - w),
                    (y + 1 < h).then_some(at + w),
                ]
                .into_iter()
                .flatten()
                {
                    if !visited[next] {
                        visited[next] = true;
                        queue.push_back(next);
                    }
                }
            }
            true
        }
        for (w, h) in [(1, 71), (71, 1), (17, 19), (69, 71)] {
            for pattern in 0..8 {
                let mut random = 0x156731u32 + pattern;
                let mut original = Vec::new();
                for y in 0..h {
                    for x in 0..w {
                        random = random.wrapping_mul(1664525).wrapping_add(1013904223);
                        let a = ((random >> 28) as u8 % 4) * 3;
                        let barrier = match pattern % 4 {
                            0 => x % 7 == 0 && y % 5 != 0,
                            1 => (x + y) % 2 == 0,
                            2 => random >> 28 == 0,
                            _ => false,
                        };
                        original.push(if barrier {
                            Color::BLACK
                        } else {
                            Color::rgba(10 + a, 20 + a, 30 + a, 120 + a)
                        });
                    }
                }
                for tolerance in [0, 3, 12, 255] {
                    let seed = (pattern as usize * 109 + w * h / 2) % (w * h);
                    // Include a replacement still matching the original, and
                    // zero-alpha pixels whose RGB channels remain significant.
                    for color in [
                        Color::rgba(13, 23, 33, 123),
                        Color::rgba(250, 40, 90, 0),
                        original[seed],
                    ] {
                        let mut expected = original.clone();
                        let changed = reference(&mut expected, w, h, seed, color, tolerance);
                        let mut doc =
                            Document::from_pixels(w as u32, h as u32, original.clone()).unwrap();
                        assert_eq!(
                            doc.flood_fill((seed % w) as u32, (seed / w) as u32, color, tolerance)
                                .unwrap(),
                            changed
                        );
                        assert_eq!(
                            doc.layers[0].pixels, expected,
                            "{w}x{h}, pattern {pattern}, tolerance {tolerance}"
                        );
                        if changed {
                            assert_eq!(doc.undo_count(), 1);
                            doc.undo();
                            assert_eq!(doc.layers[0].pixels, original);
                            doc.redo();
                            assert_eq!(doc.layers[0].pixels, expected);
                        } else {
                            assert_eq!(doc.undo_count(), 0);
                            assert!(doc.take_paint_damage().is_none());
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn enclosed_fill_records_only_touched_tiles_and_cancels_inside_metadata_edit() {
        let mut doc = Document::with_background(512, 512, Color::WHITE).unwrap();
        for x in 64..128 {
            doc.layers[1].pixels[64 * 512 + x] = Color::BLACK;
            doc.layers[1].pixels[127 * 512 + x] = Color::BLACK;
        }
        for y in 64..128 {
            doc.layers[1].pixels[y * 512 + 64] = Color::BLACK;
            doc.layers[1].pixels[y * 512 + 127] = Color::BLACK;
        }
        doc.take_paint_damage();
        let before = doc.layers.clone();
        let mut composite = doc.composite();
        doc.flood_fill(90, 90, Color::rgb(250, 10, 60), 0).unwrap();
        let damage = doc.take_paint_damage().unwrap();
        assert_eq!(damage, (65, 65, 62, 62));
        assert!(doc.history_bytes() < 20 * 1024);
        assert!(doc.composite_region_into(&mut composite, damage));
        assert_eq!(composite, doc.composite());
        doc.undo();
        assert_eq!(doc.layers, before);
        doc.clear_history();
        doc.mark_saved();
        doc.begin_layer_metadata_edit();
        doc.set_layer_opacity(1, 90);
        doc.flood_fill(90, 90, Color::rgb(250, 10, 60), 0).unwrap();
        match &doc.pending {
            Some(PendingEdit::Raster(raster)) => {
                assert_eq!(raster.tiles.iter().flatten().count(), 1)
            }
            _ => panic!("fill should promote metadata to sparse raster history"),
        }
        doc.cancel_edit();
        assert_eq!(doc.layers, before);
        assert!(!doc.is_dirty());
        assert_eq!(doc.undo_count(), 0);
    }
}
