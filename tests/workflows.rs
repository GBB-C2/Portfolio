//! Application-level workflows driven through the same event boundary as Win32.
//! The small blank fixture makes pixel assertions independent of the demo poster.

use portfolio::app::{PortfolioApp, Tool};
use portfolio::document::{encode_png, Color, Document};
use portfolio::platform::{Answer, Effect, Event, FileKind, Modifiers, MouseButton, NativeApp};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

fn app() -> PortfolioApp {
    let mut app = PortfolioApp::new(false);
    app.doc = Document::with_background(600, 420, Color::rgb(246, 244, 234)).unwrap();
    app.doc.title = "Workflow test".into();
    app.resize(1440, 940, 1.0);
    app.render();
    app.event(Event::Tick);
    app
}

fn key(app: &mut PortfolioApp, code: u32, ctrl: bool, shift: bool) -> Vec<Effect> {
    app.event(Event::Key {
        code,
        pressed: true,
        repeat: false,
        modifiers: Modifiers {
            ctrl,
            shift,
            alt: false,
        },
    })
}

fn down(app: &mut PortfolioApp, x: f32, y: f32) -> Vec<Effect> {
    app.event(Event::MouseDown {
        x,
        y,
        button: MouseButton::Left,
        modifiers: Modifiers::default(),
    })
}

fn movement(app: &mut PortfolioApp, x: f32, y: f32) {
    app.event(Event::MouseMove {
        x,
        y,
        modifiers: Modifiers::default(),
    });
}

fn up(app: &mut PortfolioApp, x: f32, y: f32) -> Vec<Effect> {
    app.event(Event::MouseUp {
        x,
        y,
        button: MouseButton::Left,
        modifiers: Modifiers::default(),
    })
}

fn click(app: &mut PortfolioApp, x: f32, y: f32) -> Vec<Effect> {
    let mut result = down(app, x, y);
    result.extend(up(app, x, y));
    result
}

fn stroke(app: &mut PortfolioApp) {
    down(app, 400.0, 400.0);
    for i in 1..=10 {
        movement(app, 400.0 + i as f32 * 10.0, 400.0 + i as f32 * 5.0);
    }
    up(app, 500.0, 450.0);
}

fn prompt_at(app: &mut PortfolioApp, x: f32, y: f32) -> (u32, String) {
    app.render();
    click(app, x, y)
        .into_iter()
        .find_map(|effect| match effect {
            Effect::Prompt { id, value, .. } => Some((id, value)),
            _ => None,
        })
        .expect("the selected value must open a real text prompt")
}

fn color_prompt(app: &mut PortfolioApp) -> (u32, String) {
    let frame = app.render();
    let readout = frame
        .texts
        .iter()
        .find(|run| Color::parse_hex(&run.text).is_some())
        .expect("the color inspector exposes its current HEX value");
    prompt_at(
        app,
        readout.x + readout.width * 0.5,
        readout.y + readout.height * 0.5,
    )
}

fn size_prompt(app: &mut PortfolioApp) -> (u32, String) {
    let size_readout = |text: &str| {
        text.strip_suffix(" px")
            .unwrap_or(text)
            .parse::<f32>()
            .is_ok()
    };
    let mut frame = app.render();
    if !frame.texts.iter().any(|run| size_readout(&run.text)) {
        key(app, 115, false, false); // F4 opens the active tool's parameter dropdown.
        frame = app.render();
    }
    let readout = frame
        .texts
        .iter()
        .find(|run| size_readout(&run.text))
        .expect("the visible size control exposes a precise numeric readout");
    prompt_at(
        app,
        readout.x + readout.width * 0.5,
        readout.y + readout.height * 0.5,
    )
}

fn has_error(effects: &[Effect]) -> bool {
    effects
        .iter()
        .any(|effect| matches!(effect, Effect::Message { error: true, .. }))
}

#[test]
fn a_pointer_gesture_is_one_edit_and_shortcuts_undo_and_redo_it() {
    let mut app = app();
    let before = app.doc.layers.clone();
    stroke(&mut app);
    assert_eq!(app.doc.undo_count(), 1);
    assert!(app.doc.is_dirty());
    assert_ne!(app.doc.layers, before);
    let painted = app.doc.layers.clone();
    key(&mut app, 90, true, false); // Ctrl+Z.
    assert_eq!(app.doc.layers, before);
    assert!(!app.doc.is_dirty());
    key(&mut app, 90, true, true); // Ctrl+Shift+Z.
    assert_eq!(app.doc.layers, painted);
    assert_eq!(app.doc.undo_count(), 1);
}

#[test]
fn escape_capture_loss_and_focus_loss_restore_an_unfinished_stroke() {
    for interruption in 0..3 {
        let mut app = app();
        let before = app.doc.layers.clone();
        down(&mut app, 400.0, 400.0);
        movement(&mut app, 500.0, 450.0);
        assert!(app.doc.is_dirty());
        match interruption {
            0 => {
                key(&mut app, 27, false, false);
            }
            1 => {
                app.event(Event::CaptureLost);
            }
            _ => {
                app.event(Event::FocusLost);
            }
        }
        // A late mouse-up from the canceled gesture must not create another edit.
        up(&mut app, 550.0, 470.0);
        assert_eq!(app.doc.layers, before);
        assert!(!app.doc.is_dirty());
        assert_eq!(app.doc.undo_count(), 0);
    }
}

#[test]
fn layer_opacity_drag_is_one_metadata_edit_and_interruptions_restore_it() {
    for interruption in 0..4 {
        let mut app = app();
        let before = app.doc.layers.clone();
        let frame = app.render();
        let label = frame
            .texts
            .iter()
            .find(|run| run.text == "Непрозрачность слоя")
            .expect("the layer inspector exposes its opacity slider");
        let (x, y) = (label.x, label.y + 33.0);
        down(&mut app, x + 180.0, y);
        movement(&mut app, x + 130.0, y);
        movement(&mut app, x + 75.0, y);
        assert!(app.doc.is_dirty());
        assert_eq!(app.doc.undo_count(), 0);
        assert!(app.doc.active_layer().opacity < 128);
        for (old, new) in before.iter().zip(&app.doc.layers) {
            assert_eq!(old.pixels, new.pixels);
        }
        match interruption {
            0 => {
                key(&mut app, 27, false, false);
            }
            1 => {
                app.event(Event::CaptureLost);
            }
            2 => {
                app.event(Event::FocusLost);
            }
            _ => {
                up(&mut app, x + 75.0, y);
            }
        }
        if interruption < 3 {
            up(&mut app, x + 50.0, y);
            assert_eq!(app.doc.layers, before);
            assert!(!app.doc.is_dirty());
            assert_eq!(app.doc.undo_count(), 0);
        } else {
            let after = app.doc.layers.clone();
            assert_eq!(app.doc.undo_count(), 1);
            assert!(app.doc.history_bytes() < 4096);
            key(&mut app, 90, true, false);
            assert_eq!(app.doc.layers, before);
            assert!(!app.doc.is_dirty());
            key(&mut app, 90, true, true);
            assert_eq!(app.doc.layers, after);
        }
    }
}

#[test]
fn the_click_dismissing_a_menu_does_not_paint_under_it() {
    let mut app = app();
    let before = app.doc.layers.clone();
    click(&mut app, 105.0, 32.0); // Portfolio opens the file menu.
    let menu = app.render();
    assert!(menu.texts.iter().any(|run| run.text == "Открыть проект…"));
    click(&mut app, 650.0, 500.0); // Outside menu, on the canvas.
    assert_eq!(app.doc.layers, before);
    assert_eq!(app.doc.undo_count(), 0);
    app.render();
    click(&mut app, 650.0, 500.0); // The next deliberate canvas click paints.
    assert!(app.doc.is_dirty());
    assert_eq!(app.doc.undo_count(), 1);
}

#[test]
fn switching_theme_keeps_the_document_tool_and_view() {
    let mut app = app();
    stroke(&mut app);
    key(&mut app, 69, false, false); // Eraser.
    app.event(Event::Wheel {
        x: 500.0,
        y: 400.0,
        delta: 2.0,
        modifiers: Modifiers::default(),
    });
    movement(&mut app, 20.0, 20.0); // Keep the transient brush cursor outside the artwork.
    let before = app.doc.to_bytes().unwrap();
    let history = app.doc.undo_count();
    let first = app.render();
    assert!(app.dark);
    let effects = click(&mut app, 1241.0, 32.0);
    assert!(effects
        .iter()
        .any(|effect| matches!(effect, Effect::SetDarkMode(false))));
    assert!(!app.dark);
    let second = app.render();
    assert_eq!(app.tool, Tool::Eraser);
    assert_eq!(app.doc.to_bytes().unwrap(), before);
    assert_eq!(app.doc.undo_count(), history);
    assert_ne!(first.pixels[0], second.pixels[0]);
    // Pixels inside the art keep their colors when only the UI palette changes.
    for y in 300..500 {
        assert_eq!(
            &first.pixels[y * 1440 + 300..y * 1440 + 600],
            &second.pixels[y * 1440 + 300..y * 1440 + 600]
        );
    }
    click(&mut app, 1241.0, 32.0);
    assert!(app.dark);
}

#[test]
fn mirror_shortcut_paints_both_sides_in_one_undo_step() {
    let mut app = app();
    key(&mut app, 77, false, false); // Mirror.
    down(&mut app, 400.0, 400.0);
    movement(&mut app, 410.0, 460.0);
    up(&mut app, 410.0, 460.0);
    assert_eq!(app.doc.undo_count(), 1);
    let pixels = &app.doc.active_layer().pixels;
    let width = app.doc.width as usize;
    let mut painted_left = 0;
    let mut painted_right = 0;
    for row in pixels.chunks_exact(width) {
        for x in 0..width / 2 {
            painted_left += usize::from(row[x].a > 0);
            painted_right += usize::from(row[width - 1 - x].a > 0);
            assert!(row[x].a.abs_diff(row[width - 1 - x].a) <= 1);
        }
    }
    assert!(painted_left > 50 && painted_right > 50);
    key(&mut app, 90, true, false);
    assert!(app.doc.active_layer().pixels.iter().all(|p| p.a == 0));
}

#[test]
fn value_view_changes_the_preview_but_preserves_color_export() {
    let mut app = app();
    stroke(&mut app);
    movement(&mut app, 20.0, 20.0);
    let pixels = app.doc.composite();
    let expected_png = encode_png(app.doc.width, app.doc.height, &pixels).unwrap();
    let color_frame = app.render();
    key(&mut app, 86, false, false); // Value preview.
    let gray_frame = app.render();
    assert_eq!(app.doc.composite(), pixels);
    assert_eq!(
        encode_png(app.doc.width, app.doc.height, &app.doc.composite()).unwrap(),
        expected_png
    );
    let mut changed_pixels = 0;
    for y in 350..500 {
        for x in 350..550 {
            let i = y * 1440 + x;
            changed_pixels += usize::from(color_frame.pixels[i] != gray_frame.pixels[i]);
            let color = gray_frame.pixels[i];
            assert_eq!((color >> 16) & 255, (color >> 8) & 255);
            assert_eq!((color >> 8) & 255, color & 255);
        }
    }
    assert!(changed_pixels > 100);
}

#[test]
fn a_dirty_document_is_protected_when_new_is_requested() {
    let mut app = app();
    stroke(&mut app);
    let before = app.doc.to_bytes().unwrap();
    let effects = key(&mut app, 78, true, false); // Ctrl+N.
    let id = effects
        .iter()
        .find_map(|effect| match effect {
            Effect::Confirm { id, .. } => Some(*id),
            _ => None,
        })
        .expect("replacing an unsaved painting requires save/discard/cancel");
    assert_eq!(app.doc.to_bytes().unwrap(), before);
    app.event(Event::Confirmed {
        id,
        answer: Answer::Cancel,
    });
    assert_eq!(app.doc.to_bytes().unwrap(), before);
    assert!(app.doc.is_dirty());
    assert_eq!(app.doc.undo_count(), 1);
}

#[test]
fn canceled_and_invalid_color_and_numeric_prompts_keep_existing_values() {
    let mut app = app();
    let before = app.doc.to_bytes().unwrap();
    let (color_id, color) = color_prompt(&mut app);
    app.event(Event::Prompted {
        id: color_id,
        value: None,
    });
    assert_eq!(color_prompt(&mut app).1, color);
    let invalid = app.event(Event::Prompted {
        id: color_id,
        value: Some("#NOPE00".into()),
    });
    assert!(has_error(&invalid));
    assert_eq!(color_prompt(&mut app).1, color);
    app.event(Event::Prompted {
        id: color_id,
        value: None,
    });

    let (size_id, size) = size_prompt(&mut app);
    for invalid in ["NaN", "513", "-1", "not a number"] {
        assert!(has_error(&app.event(Event::Prompted {
            id: size_id,
            value: Some(invalid.into())
        })));
        assert_eq!(size_prompt(&mut app).1, size);
    }
    app.event(Event::Prompted {
        id: size_id,
        value: None,
    });
    assert_eq!(app.doc.to_bytes().unwrap(), before);
    assert!(!app.doc.is_dirty());
    assert_eq!(app.doc.undo_count(), 0);
}

#[test]
fn creating_a_canvas_provides_paper_and_one_empty_painting_layer() {
    let mut app = app();
    key(&mut app, 78, true, false);
    let dialog = app.render();
    assert!(dialog
        .texts
        .iter()
        .any(|run| run.text == "Место для новой идеи."));
    click(&mut app, 880.0, 623.0);
    assert_eq!((app.doc.width, app.doc.height), (1200, 840));
    assert_eq!(app.doc.layers.len(), 2);
    assert!(app.doc.layers[0].locked);
    assert!(!app.doc.active_layer().locked);
    assert!(app.doc.active_layer().pixels.iter().all(|p| p.a == 0));
    assert!(!app.doc.can_undo());
}

static FILE_COUNTER: AtomicU64 = AtomicU64::new(0);
struct TestFile(PathBuf);
impl TestFile {
    fn new() -> Self {
        let directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("target/workflow-tests");
        std::fs::create_dir_all(&directory).unwrap();
        Self(directory.join(format!(
            "{}-{}.portfolio",
            std::process::id(),
            FILE_COUNTER.fetch_add(1, Ordering::Relaxed)
        )))
    }
}
impl Drop for TestFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

#[test]
fn saving_then_opening_uses_the_correct_file_dialog_intention() {
    let mut app = app();
    stroke(&mut app);
    let painted = app.doc.layers.clone();
    let target = TestFile::new();
    let effects = key(&mut app, 83, true, false); // Ctrl+S.
    assert!(effects.iter().any(|effect| matches!(
        effect,
        Effect::SaveFile {
            kind: FileKind::Project,
            ..
        }
    )));
    app.event(Event::FileSelected {
        kind: FileKind::Project,
        path: Some(target.0.clone()),
    });
    let start = Instant::now();
    while app.doc.is_dirty() && start.elapsed() < Duration::from_secs(5) {
        std::thread::sleep(Duration::from_millis(5));
        app.event(Event::Tick);
    }
    assert!(
        !app.doc.is_dirty(),
        "saving must eventually acknowledge the saved revision"
    );
    assert_eq!(Document::load(&target.0).unwrap().layers, painted);

    let other = TestFile::new();
    let mut reopened = Document::with_background(64, 48, Color::rgb(30, 60, 90)).unwrap();
    reopened.title = "Reopened study".into();
    reopened.save(&other.0).unwrap();
    let original_file = std::fs::read(&other.0).unwrap();
    let effects = key(&mut app, 79, true, false); // Ctrl+O, after a previous Save dialog.
    assert!(effects
        .iter()
        .any(|effect| matches!(effect, Effect::OpenFile(FileKind::Project))));
    app.event(Event::FileSelected {
        kind: FileKind::Project,
        path: Some(other.0.clone()),
    });
    // The document changes only when the worker result is delivered by Tick.
    let start = Instant::now();
    while (app.doc.width, app.doc.height) != (64, 48) && start.elapsed() < Duration::from_secs(5) {
        std::thread::sleep(Duration::from_millis(5));
        app.event(Event::Tick);
    }
    assert_eq!((app.doc.width, app.doc.height), (64, 48));
    assert_eq!(app.doc.title, "Reopened study");
    assert_eq!(app.doc.layers, reopened.layers);
    assert!(!app.doc.is_dirty());
    assert_eq!(
        std::fs::read(&other.0).unwrap(),
        original_file,
        "Open must never overwrite its selected input"
    );
}

#[test]
fn quick_size_readout_edits_the_tool_without_opening_properties_or_changing_art() {
    let mut app = app();
    let before = app.doc.to_bytes().unwrap();
    let frame = app.render();
    let readout = frame
        .texts
        .iter()
        .find(|run| run.text.parse::<u32>().is_ok())
        .expect("the quick bar shows a size readout on the canvas without F4");
    let position = (readout.x, readout.y);
    let effects = click(
        &mut app,
        readout.x + readout.width * 0.5,
        readout.y + readout.height * 0.5,
    );
    let id = effects
        .into_iter()
        .find_map(|effect| match effect {
            Effect::Prompt { id, .. } => Some(id),
            _ => None,
        })
        .expect("the quick size value opens its exact-value prompt");
    let effects = app.event(Event::Prompted {
        id,
        value: Some("64".into()),
    });
    assert!(!has_error(&effects));
    let frame = app.render();
    let changed = frame.texts.iter().find(|run| run.text == "64").unwrap();
    assert_eq!((changed.x, changed.y), position);
    assert_eq!(app.doc.to_bytes().unwrap(), before);
    assert!(!app.doc.is_dirty());
    assert_eq!((app.doc.undo_count(), app.doc.redo_count()), (0, 0));
}
