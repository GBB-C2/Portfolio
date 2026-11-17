//! Reproducible host CPU benchmark of the real PortfolioApp software renderer.
//! This excludes Win32/GDI text rasterization and presentation; it is not UI FPS.
//!
//! cargo build --release --lib
//! rustc --edition=2021 -C opt-level=3 -C lto=fat -C codegen-units=1 \
//!   tools/frame_probe.rs --extern portfolio=target/release/libportfolio.rlib \
//!   -L dependency=target/release/deps -o target/frame_probe
//! target/frame_probe --samples 60 --warmup 8 [--dump target/frame-dumps]
//!
//! --only app|primitives, --size 1440x940, and --scenario idle|move|menu|drawing
//! restrict a run. Dumps are uncompressed little-endian RGB u32 buffers plus
//! Debug TextRun records, suitable for exact comparison between renderer builds.

use portfolio::app::PortfolioApp;
use portfolio::demo;
use portfolio::platform::{Event, Frame, Modifiers, MouseButton, NativeApp};
use portfolio::render::{Painter, Rect};
use std::hint::black_box;
use std::path::{Path, PathBuf};
use std::time::Instant;

struct Options {
    samples: usize,
    warmup: usize,
    sizes: Vec<(u32, u32)>,
    only: String,
    scenario: Option<String>,
    dump: Option<PathBuf>,
}

fn options() -> Options {
    let mut result = Options {
        samples: 60,
        warmup: 8,
        sizes: vec![(1440, 940), (884, 581)],
        only: "all".into(),
        scenario: None,
        dump: None,
    };
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let value = args
            .next()
            .unwrap_or_else(|| panic!("missing value: {arg}"));
        match arg.as_str() {
            "--samples" => result.samples = value.parse::<usize>().unwrap().clamp(1, 2000),
            "--warmup" => result.warmup = value.parse::<usize>().unwrap().min(100),
            "--size" => {
                let (w, h) = value.split_once('x').expect("expected WIDTHxHEIGHT");
                result.sizes = vec![(w.parse().unwrap(), h.parse().unwrap())];
            }
            "--only" => {
                assert!(matches!(value.as_str(), "app" | "primitives" | "all"));
                result.only = value;
            }
            "--scenario" => {
                assert!(matches!(
                    value.as_str(),
                    "idle" | "move" | "menu" | "drawing"
                ));
                result.scenario = Some(value);
            }
            "--dump" => result.dump = Some(value.into()),
            _ => panic!("unknown argument: {arg}"),
        }
    }
    result
}

fn hash(bytes: impl IntoIterator<Item = u8>) -> u64 {
    bytes.into_iter().fold(0xcbf29ce484222325, |h, byte| {
        (h ^ u64::from(byte)).wrapping_mul(0x100000001b3)
    })
}

fn report(
    kind: &str,
    scenario: &str,
    frame: &Frame,
    mut events: Vec<f64>,
    mut renders: Vec<f64>,
    dump: Option<&Path>,
) {
    events.sort_by(f64::total_cmp);
    renders.sort_by(f64::total_cmp);
    let n = renders.len();
    let texts = format!("{:?}\n", frame.texts);
    let pixel_hash = hash(frame.pixels.iter().flat_map(|p| p.to_le_bytes()));
    let text_hash = hash(texts.bytes());
    println!(
        "{kind},{}x{},{scenario},{n},{:.3},{:.3},{:.3},{pixel_hash:016x},{text_hash:016x}",
        frame.width,
        frame.height,
        events[events.len() / 2],
        renders[n / 2],
        renders[((n * 95).div_ceil(100) - 1).min(n - 1)],
    );
    if let Some(path) = dump {
        std::fs::create_dir_all(path).unwrap();
        let name = format!("{kind}_{}x{}_{scenario}", frame.width, frame.height);
        let pixels: Vec<u8> = frame.pixels.iter().flat_map(|p| p.to_le_bytes()).collect();
        std::fs::write(path.join(format!("{name}.rgb32")), pixels).unwrap();
        std::fs::write(path.join(format!("{name}.texts")), texts).unwrap();
    }
}

fn movement(width: u32, height: u32, index: usize) -> (f32, f32) {
    // A short, repeatable path clear of floating controls at both test sizes.
    let angle = index as f32 * 0.075;
    (
        width as f32 * 0.43 + 45.0 * angle.sin(),
        height as f32 * 0.57 + 25.0 * angle.cos(),
    )
}

fn move_pointer(app: &mut PortfolioApp, x: f32, y: f32) {
    black_box(app.event(Event::MouseMove {
        x,
        y,
        modifiers: Modifiers::default(),
    }));
}

fn mouse_button(app: &mut PortfolioApp, x: f32, y: f32, down: bool) {
    let modifiers = Modifiers::default();
    let button = MouseButton::Left;
    black_box(app.event(if down {
        Event::MouseDown {
            x,
            y,
            button,
            modifiers,
        }
    } else {
        Event::MouseUp {
            x,
            y,
            button,
            modifiers,
        }
    }));
}

fn app_case(options: &Options, width: u32, height: u32, scenario: &str) {
    let mut app = PortfolioApp::new(false);
    app.resize(width, height, 1.0);
    let mut frame = app.render();
    if scenario == "menu" {
        mouse_button(&mut app, 105.0, 30.0, true);
        mouse_button(&mut app, 105.0, 30.0, false);
    } else if scenario == "drawing" {
        let (x, y) = movement(width, height, 0);
        mouse_button(&mut app, x, y, true);
    }
    let mut events = Vec::with_capacity(options.samples);
    let mut renders = Vec::with_capacity(options.samples);
    for index in 0..options.warmup + options.samples {
        let start = Instant::now();
        if matches!(scenario, "move" | "drawing") {
            let (x, y) = movement(width, height, index + 1);
            move_pointer(&mut app, x, y);
        }
        let event_time = start.elapsed().as_secs_f64() * 1e6;
        let start = Instant::now();
        let rendered = black_box(app.render());
        let render_time = start.elapsed().as_secs_f64() * 1e6;
        frame = rendered; // Drop the preceding frame outside the measured scope.
        if index >= options.warmup {
            events.push(event_time);
            renders.push(render_time);
        }
    }
    if scenario == "drawing" {
        assert!(app.doc.layers[app.doc.active]
            .pixels
            .iter()
            .any(|c| c.a != 0));
        let (x, y) = movement(width, height, options.warmup + options.samples);
        mouse_button(&mut app, x, y, false);
    }
    report(
        "app",
        scenario,
        &frame,
        events,
        renders,
        options.dump.as_deref(),
    );
}

fn primitive_cases(options: &Options, width: u32, height: u32) {
    let doc = demo::document();
    let composite = doc.composite();
    let zoom = ((width as f32 - 80.0) / doc.width as f32)
        .min((height as f32 - 144.0) / doc.height as f32)
        .clamp(0.05, 16.0);
    let canvas = Rect::new(
        (width as f32 - doc.width as f32 * zoom) * 0.5,
        64.0 + (height as f32 - 64.0 - doc.height as f32 * zoom) * 0.5,
        doc.width as f32 * zoom,
        doc.height as f32 * zoom,
    );
    for scenario in ["allocate_clear", "image", "shadows"] {
        let mut p = Painter::new(width, height, 1.0, 0x191919);
        let mut times = Vec::with_capacity(options.samples);
        for index in 0..options.warmup + options.samples {
            let start = Instant::now();
            match scenario {
                "allocate_clear" => {
                    let next = black_box(Painter::new(width, height, 1.0, 0x191919));
                    let time = start.elapsed().as_secs_f64() * 1e6;
                    p = next;
                    if index >= options.warmup {
                        times.push(time);
                    }
                    continue;
                }
                "image" => p.image(
                    canvas,
                    doc.width,
                    doc.height,
                    black_box(&composite),
                    false,
                    1.0,
                ),
                "shadows" => {
                    p.soft_shadow(
                        Rect::new(width as f32 * 0.5 - 362.0, 80.0, 724.0, 48.0),
                        4.0,
                        0.14,
                    );
                    p.soft_shadow(
                        Rect::new(width as f32 - 304.0, 144.0, 288.0, height as f32 - 160.0),
                        4.0,
                        0.14,
                    );
                }
                _ => unreachable!(),
            }
            let time = start.elapsed().as_secs_f64() * 1e6;
            black_box(&p.frame);
            if index >= options.warmup {
                times.push(time);
            }
        }
        report(
            "primitive",
            scenario,
            &p.frame,
            vec![0.0],
            times,
            options.dump.as_deref(),
        );
    }
}

fn main() {
    let options = options();
    eprintln!(
        "CPU software-render probe; animations/preferences off; 1x DPI; no native window or GDI."
    );
    println!("kind,size,scenario,samples,event_median_us,render_median_us,render_p95_us,pixel_hash,text_hash");
    for &(width, height) in &options.sizes {
        if options.only != "primitives" {
            for scenario in ["idle", "move", "menu", "drawing"] {
                if options
                    .scenario
                    .as_deref()
                    .is_none_or(|selected| selected == scenario)
                {
                    app_case(&options, width, height, scenario);
                }
            }
        }
        if options.only != "app" {
            primitive_cases(&options, width, height);
        }
    }
}
