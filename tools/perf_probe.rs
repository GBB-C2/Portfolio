//! Release-mode CPU benchmarks. No native UI, file IO, or timing assertions.
//! Baseline source is preserved before this performance revision.
#![allow(dead_code)]
#[cfg(not(portfolio_baseline))]
#[path = "../src/document.rs"]
mod document;
#[cfg(portfolio_baseline)]
#[path = "../target/performance_revision_before/document.rs"]
mod document;

use document::{Brush, BrushKind, Color, Document};
use std::hint::black_box;
use std::time::Instant;
type Metrics = Vec<(&'static str, u128)>;

fn timed<T>(metrics: &mut Metrics, name: &'static str, operation: impl FnOnce() -> T) -> T {
    let start = Instant::now();
    let result = operation();
    metrics.push((name, start.elapsed().as_micros()));
    result
}

fn hash_pixels(pixels: &[Color]) -> u64 {
    pixels.iter().fold(0xcbf29ce484222325, |hash, p| {
        (hash ^ u32::from_le_bytes([p.r, p.g, p.b, p.a]) as u64).wrapping_mul(0x100000001b3)
    })
}

fn document_hash(doc: &Document) -> u64 {
    let mut hash = doc.width as u64 ^ ((doc.height as u64) << 32) ^ doc.active as u64;
    for layer in &doc.layers {
        hash = hash.rotate_left(17)
            ^ hash_pixels(&layer.pixels)
            ^ layer.id
            ^ (layer.opacity as u64) << 8
            ^ (layer.visible as u64) << 16
            ^ (layer.locked as u64) << 17;
        for byte in layer.name.as_bytes() {
            hash = (hash ^ *byte as u64).wrapping_mul(0x100000001b3);
        }
    }
    hash
}

fn fixture(size: u32) -> Document {
    let mut doc = Document::with_background(size, size, Color::WHITE).unwrap();
    let layers = if size >= 4096 { 2 } else { 4 };
    while doc.layers.len() < layers {
        doc.add_layer("Paint").unwrap();
        doc.clear_history();
    }
    doc.active = doc.layers.len() - 1;
    doc.clear_history();
    doc.mark_saved();
    doc.take_paint_damage();
    doc
}

fn strokes(run: usize, size: u32, metrics: &mut Metrics) {
    let mut doc = fixture(size);
    let mut composite = doc.composite();
    let at = size as f32 * 0.5;
    let brush = Brush {
        size: 32.0,
        color: Color::from_hex(0xda503b),
        ..Brush::default()
    };
    timed(metrics, "stroke_begin", || {
        doc.begin_stroke(at, at, brush).unwrap()
    });
    timed(metrics, "stroke_dirty60", || {
        for _ in 0..60 {
            black_box(black_box(&doc).is_dirty());
        }
    });
    timed(metrics, "stroke_move", || {
        doc.continue_stroke(at + 80.0, at + 50.0)
    });
    timed(metrics, "stroke_commit", || doc.end_stroke());
    println!("check,{run},stroke,{:016x}", document_hash(&doc));
    let damage = doc.take_paint_damage().unwrap();
    timed(metrics, "region_composite", || {
        assert!(doc.composite_region_into(&mut composite, damage))
    });
    timed(metrics, "stroke_undo", || assert!(doc.undo()));
    println!("check,{run},stroke_undo,{:016x}", document_hash(&doc));
    timed(metrics, "stroke_redo", || assert!(doc.redo()));
    println!("check,{run},stroke_redo,{:016x}", document_hash(&doc));
    let copy = timed(metrics, "io_snapshot", || doc.snapshot_for_io().unwrap());
    black_box(copy);

    let mut soft = Document::new(size, size).unwrap();
    let brush = Brush {
        kind: BrushKind::Soft,
        size: 512.0,
        ..brush
    };
    timed(metrics, "soft512_begin", || {
        soft.begin_stroke(64.0, 64.0, brush).unwrap()
    });
    timed(metrics, "soft512_move", || {
        soft.continue_stroke(size as f32 - 64.0, size as f32 - 64.0)
    });
    timed(metrics, "soft512_commit", || soft.end_stroke());
    println!("check,{run},soft512,{:016x}", document_hash(&soft));
}

fn composition(run: usize, size: u32, metrics: &mut Metrics) {
    let mut doc = fixture(size);
    let mut output = vec![Color::TRANSPARENT; size as usize * size as usize];
    timed(metrics, "composite_sparse", || {
        doc.composite_into(black_box(&mut output))
    });
    println!("check,{run},composite_sparse,{:016x}", hash_pixels(&output));
    for (i, layer) in doc.layers.iter_mut().enumerate() {
        for (j, pixel) in layer.pixels.iter_mut().enumerate() {
            *pixel = Color::rgba(
                (j * 13 + i * 97) as u8,
                (j * 19 + i * 73) as u8,
                (j * 7 + i * 31) as u8,
                [255, 128, 64, 220][i % 4],
            );
        }
    }
    timed(metrics, "composite_dense", || {
        doc.composite_into(black_box(&mut output))
    });
    println!("check,{run},composite_dense,{:016x}", hash_pixels(&output));
}

fn layers(run: usize, size: u32, metrics: &mut Metrics) {
    let mut doc = fixture(size);
    doc.layers[doc.active].pixels.fill(Color::rgb(90, 110, 210));
    let index = doc.active;
    timed(metrics, "layer_visibility", || {
        doc.set_layer_visibility(index, false)
    });
    timed(metrics, "layer_visibility_undo", || doc.undo());
    println!(
        "check,{run},layer_visibility_undo,{:016x}",
        document_hash(&doc)
    );
    timed(metrics, "layer_rename", || {
        doc.rename_layer(index, "Renamed").unwrap()
    });
    timed(metrics, "layer_reorder", || doc.move_layer(-1));
    println!("check,{run},layer_reorder,{:016x}", document_hash(&doc));
    timed(metrics, "layer_opacity_begin", || {
        #[cfg(portfolio_baseline)]
        doc.begin_edit();
        #[cfg(not(portfolio_baseline))]
        doc.begin_layer_metadata_edit();
    });
    let active = doc.active;
    timed(metrics, "layer_opacity_updates20", || {
        for opacity in 100..120 {
            doc.set_layer_opacity(active, opacity);
        }
    });
    timed(metrics, "layer_opacity_commit", || doc.commit_edit());
    println!("check,{run},layer_opacity,{:016x}", document_hash(&doc));
    timed(metrics, "layer_add", || doc.add_layer("New").unwrap());
    timed(metrics, "layer_duplicate", || {
        doc.duplicate_layer().unwrap()
    });
    println!("check,{run},layer_duplicate,{:016x}", document_hash(&doc));
    timed(metrics, "layer_delete", || doc.delete_layer().unwrap());
    timed(metrics, "layer_delete_undo", || doc.undo());
    println!("check,{run},layer_delete_undo,{:016x}", document_hash(&doc));
}

fn fills(run: usize, size: u32, metrics: &mut Metrics) {
    let mut doc = fixture(size);
    let active = doc.active;
    let w = size as usize;
    // An enclosed 126 x 126 pocket inside a much larger transparent layer.
    for x in 64..192 {
        doc.layers[active].pixels[64 * w + x] = Color::BLACK;
        doc.layers[active].pixels[191 * w + x] = Color::BLACK;
    }
    for y in 64..192 {
        doc.layers[active].pixels[y * w + 64] = Color::BLACK;
        doc.layers[active].pixels[y * w + 191] = Color::BLACK;
    }
    timed(metrics, "fill_small_pocket", || {
        doc.flood_fill(100, 100, Color::rgb(130, 40, 80), 0)
            .unwrap()
    });
    println!("check,{run},fill_small_pocket,{:016x}", document_hash(&doc));
    timed(metrics, "fill_small_undo", || doc.undo());
    println!("check,{run},fill_small_undo,{:016x}", document_hash(&doc));
    doc.layers[active].pixels.fill(Color::TRANSPARENT);
    doc.clear_history();
    timed(metrics, "fill_full_canvas", || {
        doc.flood_fill(0, 0, Color::rgb(20, 140, 210), 0).unwrap()
    });
    println!("check,{run},fill_full_canvas,{:016x}", document_hash(&doc));
    timed(metrics, "fill_full_undo", || doc.undo());
    println!("check,{run},fill_full_undo,{:016x}", document_hash(&doc));
    timed(metrics, "fill_full_redo", || doc.redo());
    println!("check,{run},fill_full_redo,{:016x}", document_hash(&doc));
}

fn main() {
    let mut args = std::env::args().skip(1);
    let repeats = args
        .next()
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(3)
        .clamp(1, 9);
    let size = args
        .next()
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(2048)
        .clamp(256, 4096);
    println!("kind,run,metric,microseconds_or_hash");
    println!("fixture,0,side,{size}");
    let mut all = std::collections::BTreeMap::<&'static str, Vec<u128>>::new();
    for run in 1..=repeats {
        let mut metrics = Vec::new();
        strokes(run, size, &mut metrics);
        composition(run, size, &mut metrics);
        layers(run, size, &mut metrics);
        fills(run, size, &mut metrics);
        for (name, value) in metrics {
            println!("run,{run},{name},{value}");
            all.entry(name).or_default().push(value);
        }
    }
    for (name, mut values) in all {
        values.sort_unstable();
        println!("median,0,{name},{}", values[values.len() / 2]);
    }
}
