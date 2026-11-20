//! Regression cases for delayed writer completions and document protection.
//! Channels stand in for workers, making the event order deterministic.

use super::*;
use std::sync::atomic::{AtomicU64, Ordering};

static FIXTURE_COUNTER: AtomicU64 = AtomicU64::new(0);

struct FixtureDirectory(PathBuf);
impl FixtureDirectory {
    fn new() -> Self {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target/app-regression-tests")
            .join(format!(
                "{}-{}",
                std::process::id(),
                FIXTURE_COUNTER.fetch_add(1, Ordering::Relaxed)
            ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn file(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for FixtureDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn app_fixture() -> PortfolioApp {
    let mut app = PortfolioApp::new(false);
    app.doc = Document::with_background(32, 24, Color::rgb(246, 244, 234)).unwrap();
    app.first_tick = false;
    app
}

fn edit(app: &mut PortfolioApp, y: f32) {
    let brush = Brush {
        color: Color::rgb(60, 90, 210),
        size: 3.0,
        ..Brush::default()
    };
    app.doc.draw_line(4.0, y, 24.0, y, brush).unwrap();
    app.changed();
}

#[test]
fn an_older_save_completion_never_quits_or_marks_newer_edits_saved() {
    let directory = FixtureDirectory::new();
    let recovery = directory.file("recovery.portfolio");
    let mut app = app_fixture();
    app.recovery = Some(recovery.clone());
    edit(&mut app, 6.0);
    let saved_serial = app.serial;
    let saved_bytes = app.doc.to_bytes().unwrap();
    fs::write(&recovery, &saved_bytes).unwrap();
    edit(&mut app, 17.0);
    let current = app.doc.to_bytes().unwrap();
    let (tx, rx) = mpsc::channel();
    app.io = Some(rx);
    app.busy = true;
    app.pending = Pending::Quit;
    assert!(tx
        .send(IoMessage::Saved {
            path: directory.file("manual.portfolio"),
            serial: saved_serial,
            result: Ok(()),
        })
        .is_ok());

    let effects = app.event(Event::Tick);
    assert!(!effects.iter().any(|effect| matches!(effect, Effect::Quit)));
    assert!(matches!(app.pending, Pending::None));
    assert!(app.doc.is_dirty());
    assert_eq!(app.doc.to_bytes().unwrap(), current);
    assert_eq!(fs::read(&recovery).unwrap(), saved_bytes);
    assert!(app.io.is_none());
    assert!(!app.busy);
}

#[test]
fn stale_autosave_is_removed_without_consuming_the_manual_writer_or_current_art() {
    let directory = FixtureDirectory::new();
    let recovery = directory.file("recovery.portfolio");
    let mut app = app_fixture();
    app.recovery = Some(recovery.clone());
    edit(&mut app, 5.0);
    let old_document_id = app.document_id;
    let old_snapshot = app.doc.to_bytes().unwrap();
    let (auto_tx, auto_rx) = mpsc::channel();
    app.auto_io = Some(auto_rx);

    app.reset_document(
        Document::with_background(40, 30, Color::rgb(220, 230, 240)).unwrap(),
        None,
        false,
    );
    edit(&mut app, 19.0);
    let current = app.doc.to_bytes().unwrap();
    // The old worker finishes after the user has already switched documents.
    fs::write(&recovery, old_snapshot).unwrap();
    assert!(auto_tx.send((old_document_id, Ok(()))).is_ok());
    let (manual_tx, manual_rx) = mpsc::channel();
    app.io = Some(manual_rx);
    app.busy = true;

    app.event(Event::Tick);
    assert!(!recovery.exists());
    assert!(app.auto_io.is_none());
    assert!(
        app.io.is_some(),
        "autosave completion must not replace the manual receiver"
    );
    assert!(app.busy, "the unrelated manual write is still running");
    assert!(app.doc.is_dirty());
    assert_eq!(app.doc.to_bytes().unwrap(), current);

    assert!(manual_tx.send(IoMessage::Exported(Ok(()))).is_ok());
    app.event(Event::Tick);
    assert!(app.io.is_none());
    assert!(!app.busy);
    assert!(app.doc.is_dirty());
    assert_eq!(app.doc.to_bytes().unwrap(), current);
}

#[test]
fn postponing_recovery_preserves_the_old_file_through_cleanup_and_a_new_manual_save() {
    let directory = FixtureDirectory::new();
    let recovery = directory.file("recovery.portfolio");
    let mut old = Document::with_background(8, 8, Color::rgb(155, 45, 60)).unwrap();
    old.title = "Earlier work".into();
    let old_bytes = old.to_bytes().unwrap();
    fs::write(&recovery, &old_bytes).unwrap();
    let mut app = app_fixture();
    app.recovery = Some(recovery.clone());
    app.recover_available = true;
    edit(&mut app, 12.0);

    app.event(Event::Confirmed {
        id: 2,
        answer: Answer::Cancel,
    });
    assert!(app.preserve_recovery);
    app.clear_recovery();
    assert_eq!(fs::read(&recovery).unwrap(), old_bytes);
    // Make the current sketch old enough to autosave; Cancel must suppress it.
    app.last_auto = Instant::now() - Duration::from_secs(60);
    app.last_edit = Instant::now() - Duration::from_secs(60);
    app.event(Event::Tick);
    assert!(app.auto_io.is_none());
    assert_eq!(fs::read(&recovery).unwrap(), old_bytes);

    let manual = directory.file("current.portfolio");
    fs::write(&manual, app.doc.to_bytes().unwrap()).unwrap();
    let (tx, rx) = mpsc::channel();
    app.io = Some(rx);
    app.busy = true;
    assert!(tx
        .send(IoMessage::Saved {
            path: manual.clone(),
            serial: app.serial,
            result: Ok(())
        })
        .is_ok());
    app.event(Event::Tick);
    assert!(!app.doc.is_dirty());
    assert_eq!(app.path.as_ref(), Some(&manual));
    assert_eq!(fs::read(&recovery).unwrap(), old_bytes);
    assert!(app.preserve_recovery);
    assert!(app.auto_io.is_none());
}

#[test]
fn a_focused_layer_opacity_control_cannot_mutate_a_document_during_a_write() {
    let mut app = app_fixture();
    app.doc.layers[app.doc.active].opacity = 180;
    app.resize(1440, 940, 1.0);
    app.render();
    let index = app
        .controls
        .iter()
        .position(|control| control.action == Action::LayerOpacity)
        .expect("the rendered layer opacity control is keyboard reachable");
    app.focus = Some(index);
    app.busy = true;
    let before = app.doc.to_bytes().unwrap();
    let arrow = || Event::Key {
        code: 37,
        pressed: true,
        repeat: false,
        modifiers: Modifiers::default(),
    };
    app.event(arrow());
    assert_eq!(app.doc.to_bytes().unwrap(), before);
    assert!(!app.doc.is_dirty());
    assert_eq!(app.doc.undo_count(), 0);

    // The same key remains useful when the write has finished.
    app.busy = false;
    app.event(arrow());
    assert!(app.doc.active_layer().opacity < 180);
    assert!(app.doc.is_dirty());
    assert_eq!(app.doc.undo_count(), 1);
}

fn floating_panel_fixture() -> PortfolioApp {
    let mut app = app_fixture();
    app.doc = Document::with_background(640, 400, Color::WHITE).unwrap();
    app.resize(1440, 940, 1.0);
    // Cover the whole workspace, including the pixels underneath floating surfaces.
    // A passing interception test must not rely on the artwork ending earlier.
    app.zoom = 2.4;
    app.view_x = -48.0;
    app.view_y = 64.0;
    app.render();
    app
}

#[test]
fn toolbar_and_inspector_absorb_input_and_tab_preserves_the_full_viewport() {
    let mut app = floating_panel_fixture();
    let stage = app.stage();
    assert_eq!(
        (stage.x, stage.y, stage.w, stage.h),
        (0.0, 64.0, 1440.0, 876.0)
    );
    let viewport = (app.view_x, app.view_y, app.zoom);
    let before = app.doc.to_bytes().unwrap();
    let brush_size = app.brush.size;
    let toolbar = app.top_tools_rect();
    let right = app.right_panel_rect();
    let quick = app.quick_controls_rect();
    let blank_points = [
        (toolbar.x + 2.0, toolbar.y + toolbar.h * 0.5),
        (right.x + 2.0, right.y + 30.0),
        (quick.x + 2.0, quick.y + 2.0),
    ];
    for (x, y) in blank_points {
        assert!(app.canvas().contains(x, y));
        for button in [MouseButton::Left, MouseButton::Middle, MouseButton::Right] {
            app.event(Event::MouseDown {
                x,
                y,
                button,
                modifiers: Modifiers::default(),
            });
            app.event(Event::MouseUp {
                x,
                y,
                button,
                modifiers: Modifiers::default(),
            });
        }
        for ctrl in [false, true] {
            app.event(Event::Wheel {
                x,
                y,
                delta: -1.0,
                modifiers: Modifiers {
                    ctrl,
                    ..Modifiers::default()
                },
            });
        }
        assert_eq!(app.doc.to_bytes().unwrap(), before);
        assert_eq!((app.view_x, app.view_y, app.zoom), viewport);
        assert_eq!(app.brush.size, brush_size);
    }
    assert_eq!(app.doc.undo_count(), 0);

    let tab = || Event::Key {
        code: 9,
        pressed: true,
        repeat: false,
        modifiers: Modifiers::default(),
    };
    app.event(tab());
    assert!(app.focused_mode);
    let focused_stage = app.stage();
    assert_eq!(
        (
            focused_stage.x,
            focused_stage.y,
            focused_stage.w,
            focused_stage.h
        ),
        (stage.x, stage.y, stage.w, stage.h)
    );
    assert_eq!((app.view_x, app.view_y, app.zoom), viewport);
    // The last presented toolbar still owns its old area until the hidden frame is drawn.
    let (x, y) = blank_points[0];
    app.event(Event::MouseDown {
        x,
        y,
        button: MouseButton::Left,
        modifiers: Modifiers::default(),
    });
    assert!(matches!(app.drag, Drag::None));
    assert_eq!(app.doc.to_bytes().unwrap(), before);
    left_up(&mut app, x, y);
    app.render();
    app.event(Event::MouseDown {
        x,
        y,
        button: MouseButton::Left,
        modifiers: Modifiers::default(),
    });
    assert!(
        app.doc.is_dirty(),
        "the exposed canvas accepts a deliberate new stroke"
    );
    app.event(Event::CaptureLost);
    app.event(tab());
    assert!(!app.focused_mode);
    assert_eq!((app.view_x, app.view_y, app.zoom), viewport);
    // Geometry must also block a newly restored panel before its next redraw.
    app.event(Event::MouseDown {
        x,
        y,
        button: MouseButton::Left,
        modifiers: Modifiers::default(),
    });
    app.event(Event::MouseUp {
        x,
        y,
        button: MouseButton::Left,
        modifiers: Modifiers::default(),
    });
    assert_eq!(app.doc.to_bytes().unwrap(), before);
    assert_eq!(app.doc.undo_count(), 0);
}

#[test]
fn a_stroke_stops_at_a_floating_panel_and_never_bridges_when_the_pointer_returns() {
    for release_enters_panel in [false, true] {
        let mut app = floating_panel_fixture();
        let right = app.right_panel_rect();
        app.event(Event::MouseDown {
            x: right.x - 80.0,
            y: 400.0,
            button: MouseButton::Left,
            modifiers: Modifiers::default(),
        });
        app.event(Event::MouseMove {
            x: right.x - 40.0,
            y: 420.0,
            modifiers: Modifiers::default(),
        });
        // Compare to a normally completed gesture ending at the last exposed
        // point, including the brush's final endpoint stamp.
        let mut completed = floating_panel_fixture();
        completed.event(Event::MouseDown {
            x: right.x - 80.0,
            y: 400.0,
            button: MouseButton::Left,
            modifiers: Modifiers::default(),
        });
        completed.event(Event::MouseMove {
            x: right.x - 40.0,
            y: 420.0,
            modifiers: Modifiers::default(),
        });
        completed.event(Event::MouseUp {
            x: right.x - 40.0,
            y: 420.0,
            button: MouseButton::Left,
            modifiers: Modifiers::default(),
        });
        let last_exposed_pixels = completed.doc.active_layer().pixels.clone();
        assert!(last_exposed_pixels.iter().any(|pixel| pixel.a > 0));
        let (x, y) = (right.x + 2.0, 430.0);
        if release_enters_panel {
            app.event(Event::MouseUp {
                x,
                y,
                button: MouseButton::Left,
                modifiers: Modifiers::default(),
            });
        } else {
            app.event(Event::MouseMove {
                x,
                y,
                modifiers: Modifiers::default(),
            });
        }
        assert!(matches!(app.drag, Drag::None));
        assert_eq!(app.doc.active_layer().pixels, last_exposed_pixels);
        assert_eq!(app.doc.undo_count(), 1);

        // Leaving the panel while the same button sequence is in flight must not
        // resume the old stroke or connect a line across hidden document pixels.
        app.event(Event::MouseMove {
            x: 800.0,
            y: 550.0,
            modifiers: Modifiers::default(),
        });
        app.event(Event::MouseUp {
            x: 800.0,
            y: 550.0,
            button: MouseButton::Left,
            modifiers: Modifiers::default(),
        });
        assert_eq!(app.doc.active_layer().pixels, last_exposed_pixels);
        assert_eq!(app.doc.undo_count(), 1);
        app.event(Event::Key {
            code: 90,
            pressed: true,
            repeat: false,
            modifiers: Modifiers {
                ctrl: true,
                ..Modifiers::default()
            },
        });
        assert!(app
            .doc
            .active_layer()
            .pixels
            .iter()
            .all(|pixel| pixel.a == 0));
        assert!(!app.doc.is_dirty());
    }
}

fn click_action(app: &mut PortfolioApp, action: Action) -> Vec<Effect> {
    app.render();
    let hit = app
        .controls
        .iter()
        .rev()
        .find(|control| control.enabled && control.action == action)
        .unwrap_or_else(|| panic!("visible action {action:?} is reachable"))
        .hit;
    let (x, y) = (hit.x + hit.w * 0.5, hit.y + hit.h * 0.5);
    let mut effects = app.event(Event::MouseDown {
        x,
        y,
        button: MouseButton::Left,
        modifiers: Modifiers::default(),
    });
    effects.extend(app.event(Event::MouseUp {
        x,
        y,
        button: MouseButton::Left,
        modifiers: Modifiers::default(),
    }));
    effects
}

fn activate_jump(app: &mut PortfolioApp, action: Action) -> Vec<Effect> {
    // These retained commands reveal a vertical section; they are no longer
    // interactive tabs or accordion headers on either properties surface.
    assert!(matches!(
        action,
        Action::SettingsSection(_) | Action::BrushDetail(_)
    ));
    let mut effects = Vec::new();
    app.activate(action, &mut effects);
    app.render();
    effects
}

fn browser_wheel(
    app: &mut PortfolioApp,
    family: ToolPanel,
    row: BrowserRow,
    delta: f32,
    ctrl: bool,
) {
    app.render();
    let area = match row {
        BrowserRow::Items => app.browser_items_rect(family),
        BrowserRow::Categories => app.browser_categories_rect(family),
    };
    app.event(Event::Wheel {
        x: area.x + area.w * 0.5,
        y: area.y + area.h * 0.5,
        delta,
        modifiers: Modifiers {
            ctrl,
            ..Modifiers::default()
        },
    });
}

fn browser_query(app: &mut PortfolioApp, query: &str) {
    click_action(app, Action::BrowserSearch);
    press_key(app, 65, true);
    if query.is_empty() {
        press_key(app, 8, false);
    } else {
        app.event(Event::Text(query.into()));
    }
    press_key(app, 9, false); // Blur without collapsing the field or clearing the query.
    app.render();
}

fn control_center(app: &mut PortfolioApp, action: Action) -> (f32, f32) {
    app.render();
    let hit = app
        .controls
        .iter()
        .rev()
        .find(|c| c.enabled && c.action == action)
        .unwrap_or_else(|| panic!("visible action {action:?} is reachable"))
        .hit;
    (hit.x + hit.w * 0.5, hit.y + hit.h * 0.5)
}

fn left_down(app: &mut PortfolioApp, x: f32, y: f32) {
    app.event(Event::MouseDown {
        x,
        y,
        button: MouseButton::Left,
        modifiers: Modifiers::default(),
    });
}

fn pointer_move(app: &mut PortfolioApp, x: f32, y: f32) {
    app.event(Event::MouseMove {
        x,
        y,
        modifiers: Modifiers::default(),
    });
}

fn left_up(app: &mut PortfolioApp, x: f32, y: f32) {
    app.event(Event::MouseUp {
        x,
        y,
        button: MouseButton::Left,
        modifiers: Modifiers::default(),
    });
}

#[test]
fn browser_wheel_scrolls_two_clipped_rows_continuously_without_changing_art_or_view() {
    let mut app = floating_panel_fixture();
    app.resize(884, 581, 1.);
    let before = app.doc.to_bytes().unwrap();
    let serial = app.serial;
    click_action(&mut app, Action::ToolPanel(ToolPanel::Brush));
    click_action(&mut app, Action::PinToolPanel);
    for family in ToolPanel::ALL {
        click_action(&mut app, Action::ToolPanel(family));
        app.render();
        let view = (app.view_x, app.view_y, app.zoom);
        let settings = ToolSettings::from_brush(app.brush);
        let selected = app.selected_tool_option();
        let categories_before = app.browser.view(family).categories;
        browser_wheel(&mut app, family, BrowserRow::Items, -0.25, false);
        assert_eq!(app.browser.view(family).items.offset, 18.);
        assert_eq!(app.browser.view(family).categories, categories_before);
        browser_wheel(&mut app, family, BrowserRow::Items, -0.25, true);
        assert_eq!(
            app.browser.view(family).items.offset,
            36_f32.min(app.browser.view(family).items.max())
        );

        let items_before = app.browser.view(family).items;
        browser_wheel(&mut app, family, BrowserRow::Categories, -0.25, true);
        assert_eq!(
            app.browser.view(family).categories.offset,
            9_f32.min(app.browser.view(family).categories.max())
        );
        assert_eq!(app.browser.view(family).items, items_before);
        for row in [BrowserRow::Items, BrowserRow::Categories] {
            browser_wheel(&mut app, family, row, -100., false);
            let offset = match row {
                BrowserRow::Items => app.browser.view(family).items,
                BrowserRow::Categories => app.browser.view(family).categories,
            };
            assert_eq!(offset.offset, offset.max());
            app.render();
            let items = app.browser_items_rect(family);
            let categories = app.browser_categories_rect(family);
            for control in app.controls.iter().filter(|c| c.enabled) {
                let area = match control.action {
                    Action::Brush(_) | Action::Shape(_) | Action::TextPreset(_) => items,
                    Action::BrowserCategory(Category::All) => app.browser_all_category_rect(family),
                    Action::BrowserCategory(_) => categories,
                    _ => continue,
                };
                assert!(control.hit.x >= area.x && control.hit.y >= area.y);
                assert!(control.hit.x + control.hit.w <= area.x + area.w + 0.01);
                assert!(control.hit.y + control.hit.h <= area.y + area.h + 0.01);
            }
            browser_wheel(&mut app, family, row, 100., true);
            assert_eq!(
                match row {
                    BrowserRow::Items => app.browser.view(family).items.offset,
                    BrowserRow::Categories => app.browser.view(family).categories.offset,
                },
                0.
            );
        }
        assert_eq!(app.selected_tool_option(), selected);
        assert_eq!(ToolSettings::from_brush(app.brush), settings);
        assert_eq!((app.view_x, app.view_y, app.zoom), view);
        assert_eq!(app.doc.to_bytes().unwrap(), before);
        assert_eq!(app.serial, serial);
        assert_eq!((app.doc.undo_count(), app.doc.redo_count()), (0, 0));
    }
}

#[test]
fn browser_clicks_select_on_release_but_dragging_either_row_never_selects_or_paints() {
    for family in ToolPanel::ALL {
        for row in [BrowserRow::Items, BrowserRow::Categories] {
            let mut app = floating_panel_fixture();
            let before = app.doc.to_bytes().unwrap();
            click_action(&mut app, Action::ToolPanel(family));
            let initial_option = app.selected_tool_option();
            let action = match row {
                BrowserRow::Items if family == ToolPanel::Text => {
                    Action::TextPreset(TextPreset::Caption)
                }
                BrowserRow::Categories if family == ToolPanel::Text => {
                    Action::BrowserCategory(Category::Titles)
                }
                BrowserRow::Items if family == ToolPanel::Shapes => Action::Shape(Tool::Line),
                BrowserRow::Items => Action::Brush(1),
                BrowserRow::Categories if family == ToolPanel::Shapes => {
                    Action::BrowserCategory(Category::Curves)
                }
                BrowserRow::Categories => Action::BrowserCategory(Category::Sketch),
            };
            let (x, y) = control_center(&mut app, action);
            left_down(&mut app, x, y);
            assert_eq!(app.selected_tool_option(), initial_option);
            assert_eq!(app.browser.view(family).category, Category::All);
            assert!(matches!(app.drag, Drag::Browser { moved: false, .. }));
            pointer_move(&mut app, x + 4., y);
            assert!(matches!(app.drag, Drag::Browser { moved: false, .. }));
            assert_eq!(app.browser.view(family).items.offset, 0.);
            left_up(&mut app, x + 4., y);
            match action {
                Action::BrowserCategory(category) => {
                    assert_eq!(app.browser.view(family).category, category)
                }
                _ => assert_eq!(app.selected_tool_option(), action),
            }
            assert!(matches!(app.drag, Drag::None));

            if row == BrowserRow::Categories {
                click_action(&mut app, Action::BrowserCategory(Category::All));
            }
            let selected = app.selected_tool_option();
            let drag_action = match row {
                BrowserRow::Items if family == ToolPanel::Text => {
                    Action::TextPreset(TextPreset::Heading)
                }
                BrowserRow::Items if family == ToolPanel::Shapes => Action::Shape(Tool::Rectangle),
                BrowserRow::Items => Action::Brush(2),
                BrowserRow::Categories => action,
            };
            let (x, y) = control_center(&mut app, drag_action);
            left_down(&mut app, x, y);
            pointer_move(&mut app, x - 26., y);
            assert!(matches!(app.drag, Drag::Browser { moved: true, .. }));
            let offset = match row {
                BrowserRow::Items => app.browser.view(family).items,
                BrowserRow::Categories => app.browser.view(family).categories,
            };
            assert_eq!(offset.offset, 26_f32.min(offset.max()));
            // Returning to the press point must not turn an already-started drag into a click.
            pointer_move(&mut app, x, y);
            left_up(&mut app, x, y);
            assert_eq!(app.selected_tool_option(), selected);
            assert_eq!(app.browser.view(family).category, Category::All);

            // MouseUp can be the first event beyond the threshold and can be over the artwork.
            let (x, y) = control_center(&mut app, drag_action);
            left_down(&mut app, x, y);
            left_up(&mut app, 850., 700.);
            assert!(matches!(app.drag, Drag::None));
            assert_eq!(app.tool_popup, Some(family));
            assert_eq!(app.selected_tool_option(), selected);
            assert_eq!(app.browser.view(family).category, Category::All);
            assert_eq!(app.doc.to_bytes().unwrap(), before);
            assert_eq!((app.doc.undo_count(), app.doc.redo_count()), (0, 0));
        }
    }
}

#[test]
fn browser_cancel_restores_only_the_dragged_scroll_and_ignores_late_release() {
    for row in [BrowserRow::Items, BrowserRow::Categories] {
        for interruption in 0..3 {
            let mut app = floating_panel_fixture();
            app.resize(884, 581, 1.);
            let before = app.doc.to_bytes().unwrap();
            click_action(&mut app, Action::ToolPanel(ToolPanel::Brush));
            click_action(&mut app, Action::PinToolPanel);
            browser_wheel(&mut app, ToolPanel::Brush, BrowserRow::Items, -1., false);
            browser_wheel(
                &mut app,
                ToolPanel::Brush,
                BrowserRow::Categories,
                -1.,
                false,
            );
            let viewed = app.browser.view(ToolPanel::Brush).clone();
            let selected = app.selected_tool_option();
            app.render();
            let area = match row {
                BrowserRow::Items => app.browser_items_rect(ToolPanel::Brush),
                BrowserRow::Categories => app.browser_categories_rect(ToolPanel::Brush),
            };
            let (x, y) = (area.x + area.w * 0.5, area.y + area.h * 0.5);
            left_down(&mut app, x, y);
            pointer_move(&mut app, x + 22., y);
            assert_ne!(app.browser.view(ToolPanel::Brush), &viewed);
            match interruption {
                0 => {
                    press_key(&mut app, 27, false);
                }
                1 => {
                    app.event(Event::CaptureLost);
                }
                _ => {
                    app.event(Event::FocusLost);
                }
            }
            assert!(matches!(app.drag, Drag::None));
            assert_eq!(app.tool_popup, Some(ToolPanel::Brush));
            assert_eq!(app.browser.view(ToolPanel::Brush), &viewed);
            left_up(&mut app, x + 22., y);
            app.render();
            assert_eq!(app.browser.view(ToolPanel::Brush), &viewed);
            assert_eq!(app.selected_tool_option(), selected);
            assert_eq!(app.doc.to_bytes().unwrap(), before);
            assert_eq!((app.doc.undo_count(), app.doc.redo_count()), (0, 0));
        }
    }
}

#[test]
fn browser_search_edits_unicode_and_clipboard_without_leaking_tool_shortcuts() {
    let mut app = floating_panel_fixture();
    let before = app.doc.to_bytes().unwrap();
    click_action(&mut app, Action::ToolPanel(ToolPanel::Brush));
    press_key(&mut app, 70, true); // Ctrl+F focuses search only inside the chooser.
    app.render();
    assert!(app.browser_search_focus);
    let tool = app.tool;
    let settings = ToolSettings::from_brush(app.brush);
    for code in [66, 69, 32] {
        press_key(&mut app, code, false);
        app.event(Event::Key {
            code,
            pressed: false,
            repeat: false,
            modifiers: Modifiers::default(),
        });
    }
    assert_eq!(app.tool, tool);
    assert!(!app.space);
    assert_eq!(ToolSettings::from_brush(app.brush), settings);
    app.event(Event::Text("тушь".into()));
    app.render();
    assert_eq!(
        app.browser.filtered_ids(ToolPanel::Brush, app.language),
        vec![ItemId::Brush(BrushKind::Ink)]
    );
    press_key(&mut app, 65, true);
    assert!(press_key(&mut app, 86, true)
        .iter()
        .any(|effect| matches!(effect, Effect::PasteText)));
    app.event(Event::Paste("筆🖌한🙂".into()));
    app.render();
    assert_eq!(app.browser.view(ToolPanel::Brush).query.text(), "筆🖌한🙂");
    press_key(&mut app, 8, false); // Remove a four-byte scalar.
    press_key(&mut app, 37, false);
    press_key(&mut app, 46, false); // Delete the Korean scalar after the caret.
    app.render();
    assert_eq!(app.browser.view(ToolPanel::Brush).query.text(), "筆🖌");
    press_key(&mut app, 36, false);
    press_key(&mut app, 46, false);
    press_key(&mut app, 35, false);
    press_key(&mut app, 8, false);
    assert_eq!(app.browser.view(ToolPanel::Brush).query.text(), "");

    app.event(Event::Text("КАЛЛИГРАФИЯ".into()));
    app.render();
    assert_eq!(
        app.browser.filtered_ids(ToolPanel::Brush, app.language),
        vec![ItemId::Brush(BrushKind::Calligraphy)]
    );
    assert!(app
        .controls
        .iter()
        .any(|c| c.enabled && c.action == Action::Brush(7)));
    press_key(&mut app, 65, true);
    for code in [67, 88] {
        assert!(press_key(&mut app, code, true)
            .iter()
            .any(|effect| matches!(effect, Effect::CopyText(value) if value == "КАЛЛИГРАФИЯ")));
    }
    assert_eq!(app.browser.view(ToolPanel::Brush).query.text(), "");
    app.event(Event::Paste("char".into()));
    app.render();
    press_key(&mut app, 13, false);
    assert_eq!(app.brush.kind, BrushKind::Charcoal);
    assert_eq!(app.browser.view(ToolPanel::Brush).query.text(), "char");
    assert_eq!(app.tool_popup, Some(ToolPanel::Brush));

    for code in [9, 117] {
        for shift in [false, true] {
            press_key(&mut app, 70, true);
            app.render();
            assert!(app.browser_search_focus);
            let previous_item = app
                .controls
                .iter()
                .rev()
                .find(|c| c.enabled && matches!(c.action, Action::Brush(_) | Action::Shape(_)))
                .unwrap()
                .action;
            // A native text message can be immediately followed by Tab/F6
            // before the invalidated result controls have been redrawn.
            app.event(Event::Text(" ".into()));
            app.event(Event::Key {
                code,
                pressed: true,
                repeat: false,
                modifiers: Modifiers {
                    shift,
                    ..Modifiers::default()
                },
            });
            app.render();
            assert!(!app.browser_search_focus);
            assert_eq!(
                focused_action(&app),
                Some(if shift {
                    previous_item
                } else {
                    Action::ClearBrowserSearch
                })
            );
            assert!(!app.focused_mode);
            assert_eq!(app.tool_popup, Some(ToolPanel::Brush));
            assert_eq!(app.brush.kind, BrushKind::Charcoal);
        }
    }
    press_key(&mut app, 70, true);
    press_key(&mut app, 27, false);
    assert!(!app.browser_search_focus);
    assert!(!app.browser.view(ToolPanel::Brush).search_open);
    assert_eq!(app.tool_popup, Some(ToolPanel::Brush));
    assert_eq!(app.browser.view(ToolPanel::Brush).query.text(), "");
    press_key(&mut app, 27, false);
    assert_eq!(app.tool_popup, None);
    assert_eq!(app.doc.to_bytes().unwrap(), before);
    assert_eq!((app.doc.undo_count(), app.doc.redo_count()), (0, 0));
}

#[test]
fn browser_search_toggle_replaces_categories_and_fixed_all_survives_scrolling() {
    fn all_hit(app: &PortfolioApp) -> (f32, f32, f32, f32) {
        let hit = app
            .controls
            .iter()
            .find(|c| c.enabled && c.action == Action::BrowserCategory(Category::All))
            .expect("All remains a visible control outside the scrolling row")
            .hit;
        (hit.x, hit.y, hit.w, hit.h)
    }

    let mut app = floating_panel_fixture();
    app.resize(884, 581, 1.);
    let before = app.doc.to_bytes().unwrap();
    let viewport = (app.view_x, app.view_y, app.zoom);
    let family = ToolPanel::Brush;
    click_action(&mut app, Action::ToolPanel(family));
    click_action(&mut app, Action::PinToolPanel);
    click_action(&mut app, Action::BrowserCategory(Category::Sketch));
    app.render();
    let fixed_all = all_hit(&app);
    browser_wheel(&mut app, family, BrowserRow::Categories, -1., false);
    app.render();
    assert!(app.browser.view(family).categories.offset > 0.);
    assert_eq!(all_hit(&app), fixed_all);
    let row = app.browser_categories_rect(family);
    let (x, y) = (row.x + row.w * 0.5, row.y + row.h * 0.5);
    let initial_scroll = app.browser.view(family).categories.offset;
    left_down(&mut app, x, y);
    pointer_move(&mut app, x - 19., y);
    left_up(&mut app, x - 19., y);
    app.render();
    assert!(app.browser.view(family).categories.offset > initial_scroll);
    assert_eq!(all_hit(&app), fixed_all);
    assert_eq!(app.browser.view(family).category, Category::Sketch);

    for _ in 0..=app.controls.len() {
        if matches!(focused_action(&app), Some(Action::BrowserCategory(c)) if c != Category::All) {
            break;
        }
        press_key(&mut app, 9, false);
        app.render();
    }
    assert!(matches!(focused_action(&app), Some(Action::BrowserCategory(c)) if c != Category::All));
    let scrolled = app.browser.view(family).categories.offset;
    press_key(&mut app, 36, false); // Home reaches fixed All without moving the strip.
    app.render();
    assert_eq!(
        focused_action(&app),
        Some(Action::BrowserCategory(Category::All))
    );
    assert_eq!(app.browser.view(family).categories.offset, scrolled);
    assert_eq!(app.browser.view(family).category, Category::Sketch);
    press_key(&mut app, 35, false);
    app.render();
    assert_eq!(
        focused_action(&app),
        Some(Action::BrowserCategory(Category::Flat))
    );
    let category_scroll = app.browser.view(family).categories.offset;
    assert!(category_scroll > scrolled);
    assert_eq!(all_hit(&app), fixed_all);

    for _ in 0..=app.controls.len() {
        assert!(!app.browser.view(family).search_open);
        if focused_action(&app) == Some(Action::BrowserSearch) {
            break;
        }
        press_key(&mut app, 9, false);
        app.render();
    }
    assert_eq!(focused_action(&app), Some(Action::BrowserSearch));
    assert!(
        !app.browser_search_focus,
        "Tab focuses the collapsed magnifier without opening it"
    );
    press_key(&mut app, 13, false);
    app.render();
    assert!(app.browser.view(family).search_open && app.browser_search_focus);
    assert!(!app
        .controls
        .iter()
        .any(|c| matches!(c.action, Action::BrowserCategory(_))));
    assert!(app
        .controls
        .iter()
        .any(|c| c.enabled && c.action == Action::CloseBrowserSearch));
    assert!(!app
        .controls
        .iter()
        .any(|c| c.action == Action::ClearBrowserSearch));
    let search_frame = app.render();
    let field = app.browser_search_rect(family);
    let field_fill = frame_pixel(&search_frame, field.x + 24., field.y + 12.);
    for (x, y) in [
        (field.x + 24., field.y),
        (field.x, field.y + 12.),
        (field.x + 24., field.y + field.h - 1.),
    ] {
        assert_eq!(
            frame_pixel(&search_frame, x, y),
            field_fill,
            "the active search field has no accent outline"
        );
    }
    let magnifier = app
        .controls
        .iter()
        .find(|c| c.action == Action::CloseBrowserSearch)
        .unwrap()
        .hit;
    assert_eq!(
        frame_pixel(&search_frame, magnifier.x, magnifier.y + magnifier.h * 0.5),
        frame_pixel(
            &search_frame,
            magnifier.x + 3.,
            magnifier.y + magnifier.h * 0.5
        ),
        "the active magnifier keeps its fill without a selected outline"
    );

    app.event(Event::Text("ch".into()));
    browser_wheel(&mut app, family, BrowserRow::Items, -100., false);
    assert!(app.browser.view(family).items.offset > 0.);
    click_action(&mut app, Action::ClearBrowserSearch);
    app.render();
    assert!(app.browser.view(family).search_open && app.browser_search_focus);
    assert_eq!(app.browser.view(family).query.text(), "");
    assert_eq!(app.browser.view(family).items.offset, 0.);
    assert_eq!(app.browser.view(family).category, Category::Sketch);
    assert_eq!(app.browser.view(family).categories.offset, category_scroll);
    app.event(Event::Text("ch".into()));
    browser_wheel(&mut app, family, BrowserRow::Items, -100., false);
    assert!(app.browser.view(family).items.offset > 0.);
    click_action(&mut app, Action::CloseBrowserSearch);
    app.render();
    assert!(!app.browser.view(family).search_open && !app.browser_search_focus);
    assert_eq!(app.browser.view(family).query.text(), "");
    assert_eq!(app.browser.view(family).items.offset, 0.);
    assert_eq!(app.browser.view(family).category, Category::Sketch);
    assert_eq!(app.browser.view(family).categories.offset, category_scroll);
    assert_eq!(focused_action(&app), Some(Action::BrowserSearch));
    assert_eq!(all_hit(&app), fixed_all);

    press_key(&mut app, 70, true);
    app.event(Event::Text("no matching brush".into()));
    app.event(Event::Key {
        code: 9,
        pressed: true,
        repeat: false,
        modifiers: Modifiers {
            shift: true,
            ..Modifiers::default()
        },
    });
    app.render();
    let fallback = app.controls[app.focus.expect("empty search retains a reachable focus")].clone();
    assert!(fallback.enabled);
    assert!(matches!(
        fallback.action,
        Action::ToolProperties | Action::PinToolPanel
    ));
    assert!(app.browser.view(family).search_open && !app.browser_search_focus);
    click_action(&mut app, Action::BrowserSearch);
    press_key(&mut app, 65, true);
    app.event(Event::Text("char".into()));
    press_key(&mut app, 13, false);
    app.render();
    assert_eq!(app.brush.kind, BrushKind::Charcoal);
    assert!(app.browser.view(family).search_open && !app.browser_search_focus);
    assert_eq!(app.browser.view(family).query.text(), "char");
    press_key(&mut app, 115, false);
    app.render();
    assert!(app.tool_settings_visible());
    press_key(&mut app, 27, false);
    app.render();
    assert!(
        app.tool_settings_visible(),
        "Escape collapses search before closing Properties"
    );
    assert!(!app.browser.view(family).search_open);
    assert_eq!(app.browser.view(family).query.text(), "");
    assert_eq!(app.browser.view(family).category, Category::Sketch);
    assert_eq!(app.browser.view(family).categories.offset, category_scroll);
    assert_eq!(focused_action(&app), Some(Action::BrowserSearch));
    press_key(&mut app, 27, false);
    assert!(!app.tool_settings_visible());
    assert_eq!(app.tool_popup, Some(family));
    assert_eq!((app.view_x, app.view_y, app.zoom), viewport);
    assert_eq!(app.doc.to_bytes().unwrap(), before);
    assert_eq!((app.doc.undo_count(), app.doc.redo_count()), (0, 0));
}

#[test]
fn browser_filter_intersects_categories_and_invalidates_stale_card_hits() {
    let mut app = floating_panel_fixture();
    let before = app.doc.to_bytes().unwrap();
    click_action(&mut app, Action::ToolPanel(ToolPanel::Brush));
    click_action(&mut app, Action::BrowserCategory(Category::Sketch));
    browser_query(&mut app, "char");
    assert_eq!(
        app.browser.filtered_ids(ToolPanel::Brush, app.language),
        vec![ItemId::Brush(BrushKind::Charcoal)]
    );
    browser_query(&mut app, "ink");
    assert!(app
        .browser
        .filtered_ids(ToolPanel::Brush, app.language)
        .is_empty());
    assert!(!app
        .controls
        .iter()
        .any(|c| c.enabled && matches!(c.action, Action::Brush(_))));
    assert!(app
        .render()
        .texts
        .iter()
        .any(|run| run.text == "Ничего не найдено"));
    assert_eq!(app.brush.kind, BrushKind::Ink);
    click_action(&mut app, Action::ClearBrowserSearch);
    app.render();
    assert_eq!(
        app.browser.view(ToolPanel::Brush).category,
        Category::Sketch
    );
    assert_eq!(
        app.browser.filtered_ids(ToolPanel::Brush, app.language),
        vec![
            ItemId::Brush(BrushKind::Pencil),
            ItemId::Brush(BrushKind::Charcoal),
            ItemId::Brush(BrushKind::Chalk),
        ]
    );
    let stale_card = control_center(&mut app, Action::Brush(4));
    click_action(&mut app, Action::BrowserSearch);
    app.event(Event::Text("no such brush".into()));
    // No render between changing the result set and clicking a card from the old frame.
    click_point(&mut app, stale_card.0, stale_card.1);
    assert_eq!(app.brush.kind, BrushKind::Ink);
    assert_eq!(
        app.browser.view(ToolPanel::Brush).query.text(),
        "no such brush"
    );
    assert_eq!(app.tool_popup, Some(ToolPanel::Brush));
    app.render();
    let brush_view = app.browser.view(ToolPanel::Brush).clone();
    click_action(&mut app, Action::ToolPanel(ToolPanel::Eraser));
    assert_eq!(app.browser.view(ToolPanel::Eraser).query.text(), "");
    assert_eq!(app.browser.view(ToolPanel::Eraser).category, Category::All);
    browser_query(&mut app, "flat");
    click_action(&mut app, Action::Brush(9));
    click_action(&mut app, Action::ToolPanel(ToolPanel::Brush));
    app.render();
    assert_eq!(app.browser.view(ToolPanel::Brush), &brush_view);
    assert_eq!(app.brush.kind, BrushKind::Ink);
    assert_eq!(app.doc.to_bytes().unwrap(), before);
    assert_eq!((app.doc.undo_count(), app.doc.redo_count()), (0, 0));
}

fn shape_raster_fixture(tool: Tool) -> PortfolioApp {
    let mut app = floating_panel_fixture();
    let underlay = app.doc.active;
    app.doc.layers[underlay].pixels.fill(Color {
        a: 93,
        ..Color::rgb(55, 115, 185)
    });
    app.doc.layers[underlay].opacity = 170;
    let top = app.doc.add_layer("Shape preview").unwrap();
    app.doc.layers[top].opacity = 180;
    for y in 165..185 {
        for x in 205..237 {
            app.doc.layers[top].pixels[y * app.doc.width as usize + x] = Color {
                a: 107,
                ..Color::rgb(185, 65, 105)
            };
        }
    }
    app.doc.clear_history();
    app.doc.mark_saved();
    app.switch_tool(tool);
    app.brush = Brush {
        size: 84.,
        opacity: 0.35,
        color: Color::rgb(205, 75, 45),
        kind: BrushKind::Ink,
        mirror: true,
        ..Brush::default()
    };
    app.zoom = 2.;
    app.view_x = 0.;
    app.view_y = 64.;
    app.composite_dirty = true;
    app.canvas_raster = None;
    app.render();
    app
}

fn shape_canvas_pixels(app: &mut PortfolioApp) -> Vec<u32> {
    let frame = app.render();
    // This rectangle covers both the original and mirrored thick shape, while
    // avoiding the floating toolbar, inspector, quick controls and status text.
    (240..760)
        .flat_map(|y| {
            frame.pixels[y * frame.width as usize + 120..y * frame.width as usize + 1110]
                .iter()
                .copied()
        })
        .collect()
}

fn assert_same_shape_canvas(expected: &[u32], actual: &[u32], tool: Tool, phase: &str) {
    assert_eq!(expected.len(), actual.len());
    let differences = expected.iter().zip(actual).filter(|(a, b)| a != b).count();
    assert_eq!(
        differences, 0,
        "{tool:?}: canvas pixels changed during {phase}"
    );
}

#[test]
fn live_thick_translucent_mirrored_shapes_match_release_pixels_and_are_one_undo() {
    for tool in [Tool::Line, Tool::Rectangle, Tool::Ellipse] {
        let mut app = shape_raster_fixture(tool);
        let before = app.doc.layers.clone();
        let initial_canvas = shape_canvas_pixels(&mut app);
        let serial = app.serial;
        left_down(&mut app, 280., 340.);
        pointer_move(&mut app, 530., 490.);
        let intermediate = shape_canvas_pixels(&mut app);
        assert!(
            intermediate != initial_canvas,
            "{tool:?}: a live shape must be visible"
        );
        pointer_move(&mut app, 800., 640.);
        assert!(matches!(app.drag, Drag::Shape { .. }));
        let live_canvas = shape_canvas_pixels(&mut app);
        let live_layers = app.doc.layers.clone();
        assert!(
            live_layers != before,
            "{tool:?}: preview must already be real document pixels"
        );
        assert!(
            live_canvas != intermediate,
            "{tool:?}: moving the endpoint updates the frame"
        );
        assert_eq!((app.doc.undo_count(), app.doc.redo_count()), (0, 0));
        assert_eq!(app.serial, serial);
        assert!(app
            .doc
            .active_layer()
            .pixels
            .iter()
            .any(|p| p.a > 0 && p.a < 255));
        assert!(
            app.doc
                .active_layer()
                .pixels
                .chunks(app.doc.width as usize)
                .any(|row| row[480..].iter().any(|p| p.a > 0)),
            "the mirrored copy is present in live pixels"
        );

        left_up(&mut app, 800., 640.);
        assert!(matches!(app.drag, Drag::None));
        assert!(
            app.doc.layers == live_layers,
            "{tool:?}: release must not rerasterize a wider or darker shape"
        );
        assert_same_shape_canvas(
            &live_canvas,
            &shape_canvas_pixels(&mut app),
            tool,
            "release",
        );
        assert_eq!((app.doc.undo_count(), app.doc.redo_count()), (1, 0));
        assert_eq!(app.serial, serial + 1);

        press_key(&mut app, 90, true);
        assert!(
            app.doc.layers == before,
            "{tool:?}: undo restores every layer"
        );
        assert_same_shape_canvas(
            &initial_canvas,
            &shape_canvas_pixels(&mut app),
            tool,
            "undo",
        );
        assert_eq!((app.doc.undo_count(), app.doc.redo_count()), (0, 1));
        press_key(&mut app, 89, true);
        assert!(
            app.doc.layers == live_layers,
            "{tool:?}: redo restores exactly the live preview"
        );
        assert_same_shape_canvas(&live_canvas, &shape_canvas_pixels(&mut app), tool, "redo");
        assert_eq!((app.doc.undo_count(), app.doc.redo_count()), (1, 0));

        // The final pointer coordinates can arrive before another paint event.
        // Committing must flush that endpoint, even though the last rendered
        // preview still showed the intermediate geometry.
        press_key(&mut app, 90, true);
        left_down(&mut app, 280., 340.);
        pointer_move(&mut app, 530., 490.);
        app.render();
        left_up(&mut app, 800., 640.);
        assert!(
            app.doc.layers == live_layers,
            "{tool:?}: release flushes the final pointer position before commit"
        );
        assert_same_shape_canvas(
            &live_canvas,
            &shape_canvas_pixels(&mut app),
            tool,
            "release before redraw",
        );
        assert_eq!((app.doc.undo_count(), app.doc.redo_count()), (1, 0));
    }
}

#[test]
fn cancelling_live_shapes_restores_art_and_cached_canvas_without_creating_history() {
    for tool in [Tool::Line, Tool::Rectangle, Tool::Ellipse] {
        for interruption in 0..3 {
            let mut app = shape_raster_fixture(tool);
            let before = app.doc.layers.clone();
            let initial_canvas = shape_canvas_pixels(&mut app);
            let serial = app.serial;
            left_down(&mut app, 280., 340.);
            pointer_move(&mut app, 530., 490.);
            app.render();
            pointer_move(&mut app, 800., 640.);
            let preview = shape_canvas_pixels(&mut app);
            assert!(
                preview != initial_canvas,
                "{tool:?}: a live shape must be visible before cancellation"
            );
            assert!(app.doc.layers != before);
            match interruption {
                0 => {
                    press_key(&mut app, 27, false);
                }
                1 => {
                    app.event(Event::CaptureLost);
                }
                _ => {
                    app.event(Event::FocusLost);
                }
            }
            assert!(matches!(app.drag, Drag::None));
            assert!(
                app.doc.layers == before,
                "{tool:?}: cancelled preview restores the original layers"
            );
            assert_same_shape_canvas(
                &initial_canvas,
                &shape_canvas_pixels(&mut app),
                tool,
                "cancel",
            );
            left_up(&mut app, 800., 640.);
            assert!(
                app.doc.layers == before,
                "{tool:?}: a late release must not resurrect the shape"
            );
            assert_same_shape_canvas(
                &initial_canvas,
                &shape_canvas_pixels(&mut app),
                tool,
                "late release",
            );
            assert_eq!((app.doc.undo_count(), app.doc.redo_count()), (0, 0));
            assert_eq!(app.serial, serial);
            assert!(!app.doc.is_dirty());
        }
    }
}

#[test]
fn tool_properties_sections_reveal_only_their_parameters_and_preserve_independent_scroll() {
    fn visible(app: &PortfolioApp, action: Action) -> bool {
        let panel = app.tool_settings_rect();
        app.controls.iter().any(|c| {
            c.enabled
                && c.action == action
                && panel.contains(c.hit.x + c.hit.w * 0.5, c.hit.y + c.hit.h * 0.5)
        })
    }
    const BASIC: [Action; 3] = [Action::Size, Action::Opacity, Action::Hardness];
    const STAMP: [Action; 5] = [
        Action::Spacing,
        Action::Angle,
        Action::Scatter,
        Action::Roundness,
        Action::Grain,
    ];
    for (width, height, font_size, dark) in [(884, 581, 17, true), (1440, 940, 11, false)] {
        for tool in [Tool::Brush, Tool::Eraser] {
            let mut app = floating_panel_fixture();
            app.resize(width, height, 1.);
            app.font_size = font_size;
            app.dark = dark;
            let family = ToolPanel::for_tool(tool).unwrap();
            click_action(&mut app, Action::ToolPanel(family));
            click_action(&mut app, Action::PinToolPanel);
            let before = app.doc.to_bytes().unwrap();
            let settings = ToolSettings::from_brush(app.brush);
            let view = (app.view_x, app.view_y, app.zoom);
            let serial = app.serial;
            press_key(&mut app, 115, false);
            assert_eq!(app.pending_focus, Some(Action::Size));
            let unpainted_body = app.tool_settings_viewport();
            app.event(Event::Wheel {
                x: unpainted_body.x + unpainted_body.w * 0.5,
                y: unpainted_body.y + unpainted_body.h * 0.5,
                delta: -5.,
                modifiers: Modifiers::default(),
            });
            let user_offset = app.tool_settings_scroll().offset;
            app.render();
            if width == 884 {
                assert!(user_offset > 0.);
                assert_eq!(
                    app.tool_settings_scroll().offset,
                    user_offset,
                    "wheel before the first frame overrides the stale F4 Size reveal"
                );
                assert_ne!(app.pending_focus, Some(Action::Size));
                assert_ne!(focused_action(&app), Some(Action::Size));
            }
            press_key(&mut app, 115, false);
            press_key(&mut app, 115, false);
            app.render();
            assert_eq!(app.tool_settings_section(), 0);
            let panel = app.tool_settings_rect();
            assert_eq!(panel.w, 448.);
            assert!(panel.x >= 0. && panel.x + panel.w <= app.w());
            assert!(panel.y >= 72. && panel.y + panel.h <= app.h());
            assert!(!panel.intersects(app.tool_popup_rect().unwrap()));
            assert!(!panel.intersects(app.quick_controls_rect()));
            assert!(!app
                .controls
                .iter()
                .any(|c| matches!(c.action, Action::BrushDetail(_) | Action::ShapeFill)));
            let nav: Vec<_> = (0..3)
                .map(|section| {
                    app.controls
                        .iter()
                        .find(|c| c.enabled && c.action == Action::ToolSection(section))
                        .expect("each section is a visible navigation control")
                        .hit
                })
                .collect();
            assert!(nav
                .windows(2)
                .all(|pair| pair[0].y + pair[0].h <= pair[1].y));
            assert!(STAMP.iter().all(|&a| !visible(&app, a)));
            let reset = app
                .controls
                .iter()
                .find(|c| c.action == Action::ResetBrush)
                .unwrap()
                .hit;
            for action in BASIC {
                assert!(app.tool_settings_reveal_action(action));
                app.render();
                assert!(visible(&app, action));
                let field = app
                    .controls
                    .iter()
                    .find(|c| {
                        c.enabled
                            && c.action == action
                            && c.kind == ControlKind::Value
                            && panel.contains(c.hit.x + c.hit.w * 0.5, c.hit.y + c.hit.h * 0.5)
                    })
                    .unwrap();
                assert!(
                    field.hit.x > nav[0].x + nav[0].w,
                    "parameters stay to the right of the section list"
                );
            }
            let basic_scroll = app.tool_settings_scroll().offset;
            click_action(&mut app, Action::ToolSection(1));
            app.render();
            assert_eq!(app.tool_settings_section(), 1);
            assert!(!visible(&app, Action::Size) && !visible(&app, Action::Opacity));
            for action in STAMP {
                assert!(app.tool_settings_reveal_action(action));
                app.render();
                assert!(visible(&app, action));
            }
            let body = app.tool_settings_viewport();
            app.event(Event::Wheel {
                x: body.x + body.w * 0.5,
                y: body.y + body.h * 0.5,
                delta: -100.,
                modifiers: Modifiers {
                    ctrl: true,
                    ..Modifiers::default()
                },
            });
            app.render();
            let stamp_scroll = app.tool_settings_scroll().offset;
            assert_eq!(stamp_scroll, app.tool_settings_scroll().max());
            let after_reset = app
                .controls
                .iter()
                .find(|c| c.action == Action::ResetBrush)
                .unwrap()
                .hit;
            assert_eq!(
                (reset.x, reset.y, reset.w, reset.h),
                (after_reset.x, after_reset.y, after_reset.w, after_reset.h)
            );
            click_action(&mut app, Action::ToolSection(0));
            assert_eq!(app.tool_settings_scroll().offset, basic_scroll);
            click_action(&mut app, Action::ToolSection(1));
            assert_eq!(app.tool_settings_scroll().offset, stamp_scroll);

            if app.tool_settings_scroll().max() > 0. {
                for interruption in 0..3 {
                    app.event(Event::Wheel {
                        x: body.x + body.w * 0.5,
                        y: body.y + body.h * 0.5,
                        delta: 100.,
                        modifiers: Modifiers::default(),
                    });
                    app.render();
                    let initial = app.tool_settings_scroll().offset;
                    let thumb = app.tool_settings_scrollbar_thumb();
                    let (x, y) = (thumb.x + thumb.w * 0.5, thumb.y + thumb.h * 0.5);
                    left_down(&mut app, x, y);
                    pointer_move(&mut app, x, y + 45.);
                    assert!(matches!(app.drag, Drag::VerticalScroll { .. }));
                    assert_ne!(app.tool_settings_scroll().offset, initial);
                    match interruption {
                        0 => {
                            press_key(&mut app, 27, false);
                        }
                        1 => {
                            app.event(Event::CaptureLost);
                        }
                        _ => {
                            app.event(Event::FocusLost);
                        }
                    }
                    assert!(matches!(app.drag, Drag::None));
                    assert!(app.tool_settings_visible());
                    assert_eq!(app.tool_settings_section(), 1);
                    assert_eq!(app.tool_settings_scroll().offset, initial);
                    left_up(&mut app, x, y + 45.);
                    assert_eq!(app.tool_settings_scroll().offset, initial);
                }
            }
            click_action(&mut app, Action::ToolSection(2));
            app.render();
            assert_eq!(app.tool_settings_section(), 2);
            assert!(visible(&app, Action::Opacity));
            assert!(!visible(&app, Action::Roundness));
            assert!(
                app.brush_preview.is_some(),
                "the fixed preview renders the current tool"
            );
            press_key(&mut app, 115, false);
            press_key(&mut app, 115, false);
            app.render();
            assert_eq!(app.tool_settings_section(), 0);
            assert_eq!(focused_action(&app), Some(Action::Size));
            for action in [
                Action::ResetSlider(Slider::Size),
                Action::Opacity,
                Action::ResetSlider(Slider::Opacity),
                Action::Hardness,
            ] {
                press_key(&mut app, 9, false);
                app.render();
                assert_eq!(focused_action(&app), Some(action));
                let hit = app.controls[app.focus.unwrap()].hit;
                assert!(panel.contains(hit.x + hit.w * 0.5, hit.y + hit.h * 0.5));
                assert_eq!(app.tool_settings_section(), 0);
            }
            for _ in 0..5 {
                app.event(Event::Key {
                    code: 9,
                    pressed: true,
                    repeat: false,
                    modifiers: Modifiers {
                        shift: true,
                        ..Modifiers::default()
                    },
                });
                app.render();
            }
            assert_eq!(focused_action(&app), Some(Action::ToolSection(2)));
            assert_eq!(app.tool_settings_section(), 0);
            press_key(&mut app, 35, false); // End selects Stroke.
            assert_eq!(app.tool_settings_section(), 2);
            press_key(&mut app, 36, false); // Home and Right can arrive before a redraw.
            press_key(&mut app, 39, false);
            app.render();
            assert_eq!(focused_action(&app), Some(Action::ToolSection(1)));
            assert_eq!(app.tool_settings_section(), 1);
            assert!(!visible(&app, Action::Size) && !visible(&app, Action::Opacity));
            assert_eq!(ToolSettings::from_brush(app.brush), settings);
            assert_eq!((app.view_x, app.view_y, app.zoom), view);
            assert_eq!(app.doc.to_bytes().unwrap(), before);
            assert_eq!((app.doc.undo_count(), app.doc.redo_count()), (0, 0));
            assert_eq!(app.serial, serial);
        }
    }
}

#[test]
fn shape_fill_checkbox_drives_exact_live_raster_and_line_remains_outline_only() {
    for tool in [Tool::Rectangle, Tool::Ellipse] {
        let mut app = shape_raster_fixture(tool);
        let before = app.doc.layers.clone();
        let initial_canvas = shape_canvas_pixels(&mut app);
        assert!(!app.shape_filled);
        press_key(&mut app, 115, false);
        click_action(&mut app, Action::ToolSection(1));
        app.render();
        let checkbox = app
            .controls
            .iter()
            .find(|c| c.action == Action::ShapeFill)
            .unwrap();
        assert!(checkbox.enabled);
        assert_eq!(app.tool_settings_scroll().offset, 0.);
        assert!(app.tool_settings_viewport().contains(
            checkbox.hit.x + checkbox.hit.w * 0.5,
            checkbox.hit.y + checkbox.hit.h * 0.5
        ));
        click_action(&mut app, Action::ShapeFill);
        assert!(app.shape_filled);
        assert!(app.doc.layers == before);
        assert_eq!((app.doc.undo_count(), app.doc.redo_count()), (0, 0));
        press_key(&mut app, 27, false);
        app.render(); // Present the frame without the left parameter panel before painting there.
        left_down(&mut app, 280., 340.);
        assert!(matches!(app.drag, Drag::Shape { .. }));
        pointer_move(&mut app, 800., 640.);
        let live_canvas = shape_canvas_pixels(&mut app);
        let live = app.doc.layers.clone();
        assert!(
            app.doc.active_layer().pixels[213 * app.doc.width as usize + 270].a > 0,
            "{tool:?}: the interior is filled before release"
        );
        left_up(&mut app, 800., 640.);
        assert!(app.doc.layers == live);
        assert_same_shape_canvas(
            &live_canvas,
            &shape_canvas_pixels(&mut app),
            tool,
            "filled release",
        );
        assert_eq!((app.doc.undo_count(), app.doc.redo_count()), (1, 0));
        press_key(&mut app, 90, true);
        assert!(app.doc.layers == before);
        assert_same_shape_canvas(
            &initial_canvas,
            &shape_canvas_pixels(&mut app),
            tool,
            "filled undo",
        );
        press_key(&mut app, 89, true);
        assert!(app.doc.layers == live);
        assert_same_shape_canvas(
            &live_canvas,
            &shape_canvas_pixels(&mut app),
            tool,
            "filled redo",
        );
        press_key(&mut app, 90, true);
        left_down(&mut app, 280., 340.);
        pointer_move(&mut app, 800., 640.);
        app.render();
        press_key(&mut app, 27, false);
        left_up(&mut app, 800., 640.);
        assert!(app.doc.layers == before);
        assert_eq!((app.doc.undo_count(), app.doc.redo_count()), (0, 1));
        assert_same_shape_canvas(
            &initial_canvas,
            &shape_canvas_pixels(&mut app),
            tool,
            "filled cancel",
        );

        press_key(&mut app, 76, false); // Line shares the remembered shape settings.
        press_key(&mut app, 115, false);
        click_action(&mut app, Action::ToolSection(1));
        app.render();
        let disabled = app
            .controls
            .iter()
            .find(|c| c.action == Action::ShapeFill)
            .unwrap();
        assert!(!disabled.enabled);
        let hit = disabled.hit;
        click_point(&mut app, hit.x + hit.w * 0.5, hit.y + hit.h * 0.5);
        assert!(
            app.shape_filled,
            "a disabled line checkbox preserves the remembered shape choice"
        );
        assert!(app.doc.layers == before);
        assert_eq!((app.doc.undo_count(), app.doc.redo_count()), (0, 1));
    }
}

#[test]
fn pinned_chooser_is_centered_above_quick_cards_follows_tools_and_allows_canvas_painting() {
    for (width, height, font_size) in [(1440, 940, 13), (884, 581, 17)] {
        let mut app = floating_panel_fixture();
        app.resize(width, height, 1.);
        app.font_size = font_size;
        let before = app.doc.to_bytes().unwrap();
        click_action(&mut app, Action::ToolPanel(ToolPanel::Brush));
        let old_pin = control_center(&mut app, Action::PinToolPanel);
        click_action(&mut app, Action::PinToolPanel);
        assert!(app.tool_panel_pinned);
        // The second click of a double click can still target the old frame.
        // Pinning relocates the row before that frame has been repainted.
        left_down(&mut app, old_pin.0, old_pin.1);
        assert!(matches!(app.drag, Drag::None));
        left_up(&mut app, old_pin.0, old_pin.1);
        assert_eq!(app.doc.to_bytes().unwrap(), before);
        assert_eq!((app.doc.undo_count(), app.doc.redo_count()), (0, 0));
        assert!(app.tool_panel_pinned);
        for (code, tool, family) in [
            (66, Tool::Brush, ToolPanel::Brush),
            (69, Tool::Eraser, ToolPanel::Eraser),
            (76, Tool::Line, ToolPanel::Shapes),
            (82, Tool::Rectangle, ToolPanel::Shapes),
            (79, Tool::Ellipse, ToolPanel::Shapes),
            (66, Tool::Brush, ToolPanel::Brush),
        ] {
            press_key(&mut app, code, false);
            app.render();
            assert_eq!(app.tool, tool);
            assert_eq!(app.tool_popup, Some(family));
            assert!(app.quick_controls_visible());
            let chooser = app.tool_popup_rect().unwrap();
            let quick = app.quick_controls_rect();
            assert_eq!((chooser.y, chooser.h), (136., 96.));
            assert_eq!(chooser.x + chooser.w * 0.5, width as f32 * 0.5);
            assert_eq!(quick.x, chooser.x);
            assert_eq!(quick.w, chooser.w);
            assert_eq!(quick.y, chooser.y + chooser.h + 12.);
            assert!(quick.x + quick.w + 12. <= app.right_panel_rect().x);
            for action in [Action::Size, Action::Opacity] {
                assert!(app
                    .controls
                    .iter()
                    .any(|c| c.enabled && c.action == action && c.kind == ControlKind::Slider));
            }
        }
        let size = app.brush.size;
        begin_quick_drag(&mut app, Action::Size, 0.5);
        assert_eq!(app.tool_popup, Some(ToolPanel::Brush));
        press_key(&mut app, 27, false);
        assert_eq!(app.brush.size, size);
        assert!(app.tool_panel_pinned);
        press_key(&mut app, 27, false);
        assert_eq!(
            app.tool_popup,
            Some(ToolPanel::Brush),
            "Escape preserves a pinned chooser"
        );
        for overlay in [Overlay::Settings, Overlay::ToolProperties, Overlay::File] {
            match overlay {
                Overlay::Settings => {
                    press_key(&mut app, 188, true);
                }
                Overlay::ToolProperties => {
                    press_key(&mut app, 115, false);
                }
                _ => {
                    click_action(&mut app, Action::File);
                }
            }
            app.render();
            assert_eq!(app.overlay, overlay);
            assert_eq!(
                app.tool_popup,
                if overlay == Overlay::ToolProperties {
                    Some(ToolPanel::Brush)
                } else {
                    None
                }
            );
            assert!(app.tool_panel_pinned);
            match overlay {
                Overlay::Settings => {
                    click_action(&mut app, Action::CloseSettings);
                }
                Overlay::ToolProperties => {
                    click_action(&mut app, Action::CloseToolProperties);
                }
                _ => {
                    press_key(&mut app, 27, false);
                }
            }
            app.render();
            assert_eq!(app.overlay, Overlay::None);
            assert_eq!(app.tool_popup, Some(ToolPanel::Brush));
        }
        assert_eq!(app.doc.to_bytes().unwrap(), before);
        left_down(&mut app, 420., 430.);
        assert!(
            matches!(app.drag, Drag::Stroke),
            "the first canvas press paints while the chooser is pinned"
        );
        pointer_move(&mut app, 460., 470.);
        left_up(&mut app, 460., 470.);
        assert_eq!(app.doc.undo_count(), 1);
        assert_ne!(app.doc.to_bytes().unwrap(), before);
        assert!(app.tool_panel_pinned);
        assert_eq!(app.tool_popup, Some(ToolPanel::Brush));
        press_key(&mut app, 90, true);
        assert_eq!(app.doc.to_bytes().unwrap(), before);

        app.render();
        let old_card = app.quick_control_rect(Action::Opacity);
        let old_card_point = (old_card.x + old_card.w * 0.5, old_card.y + old_card.h * 0.5);
        let selected = app.selected_tool_option();
        click_action(&mut app, Action::PinToolPanel);
        left_down(&mut app, old_card_point.0, old_card_point.1);
        assert!(matches!(app.drag, Drag::None));
        left_up(&mut app, old_card_point.0, old_card_point.1);
        assert_eq!(app.overlay, Overlay::None);
        assert_eq!(app.selected_tool_option(), selected);
        assert_eq!(app.doc.to_bytes().unwrap(), before);
        app.render();
        assert!(!app.tool_panel_pinned);
        assert_eq!(app.tool_popup, Some(ToolPanel::Brush));
        assert!(!app.quick_controls_visible());
        assert!(app.tool_popup_rect().unwrap().x > 16.);
        click_point(&mut app, 420., 430.);
        assert_eq!(app.tool_popup, None);
        assert_eq!(
            app.doc.to_bytes().unwrap(),
            before,
            "the unpinned dismissal click is consumed"
        );
        assert_eq!((app.doc.undo_count(), app.doc.redo_count()), (0, 1));
    }
}

fn rect_coordinates(rect: Rect) -> (f32, f32, f32, f32) {
    (rect.x, rect.y, rect.w, rect.h)
}

fn assert_popup_triangle_absorbs_canvas_input(app: &mut PortfolioApp) {
    app.render();
    assert!(app.tool_settings_visible());
    let popup = app.tool_popup_rect().unwrap();
    let toolbar = app.top_tools_rect();
    let anchor = app.tool_family_rect(app.tool_popup.unwrap());
    let x = (anchor.x + anchor.w * 0.5).clamp(popup.x + 12., popup.x + popup.w - 12.);
    let y = if popup.y > toolbar.y {
        popup.y - 4.
    } else {
        popup.y + popup.h + 4.
    };
    assert!(!popup.contains(x, y) && !toolbar.contains(x, y));
    assert!(!app.tool_settings_rect().contains(x, y));
    assert!(app.canvas().contains(x, y));
    let before = app.doc.to_bytes().unwrap();
    let view = (app.view_x, app.view_y, app.zoom);
    let settings = ToolSettings::from_brush(app.brush);
    click_point(app, x, y);
    assert!(matches!(app.drag, Drag::None));
    for ctrl in [false, true] {
        app.event(Event::Wheel {
            x,
            y,
            delta: 1.,
            modifiers: Modifiers {
                ctrl,
                ..Modifiers::default()
            },
        });
    }
    assert!(app.tool_settings_visible());
    assert!(app.tool_popup.is_some());
    assert_eq!(ToolSettings::from_brush(app.brush), settings);
    assert_eq!((app.view_x, app.view_y, app.zoom), view);
    assert_eq!(app.doc.to_bytes().unwrap(), before);
    assert_eq!((app.doc.undo_count(), app.doc.redo_count()), (0, 0));
}

#[test]
fn moving_toolbar_translates_its_controls_and_anchor_while_pinned_chooser_moves_independently() {
    let mut app = floating_panel_fixture();
    let before = app.doc.to_bytes().unwrap();
    let view = (app.view_x, app.view_y, app.zoom);
    let settings = ToolSettings::from_brush(app.brush);
    let quick_before = app.quick_controls_rect();
    click_action(&mut app, Action::ToolPanel(ToolPanel::Brush));
    let bar = app.top_tools_rect();
    let popup = app.tool_popup_rect().unwrap();
    let family_button = app.tool_family_rect(ToolPanel::Brush);
    let (x, y) = control_center(&mut app, Action::MoveToolbar);
    left_down(&mut app, x, y);
    assert!(matches!(
        app.drag,
        Drag::Panel {
            kind: PanelKind::Toolbar,
            moved: false,
            ..
        }
    ));
    pointer_move(&mut app, x + 3., y);
    assert_eq!(
        rect_coordinates(app.top_tools_rect()),
        rect_coordinates(bar)
    );
    pointer_move(&mut app, x + 80., y + 100.);
    assert!(matches!(
        app.drag,
        Drag::Panel {
            kind: PanelKind::Toolbar,
            moved: true,
            ..
        }
    ));
    app.render();
    assert_eq!(
        (app.top_tools_rect().x, app.top_tools_rect().y),
        (bar.x + 80., bar.y + 100.)
    );
    assert_eq!(
        (
            app.tool_popup_rect().unwrap().x,
            app.tool_popup_rect().unwrap().y
        ),
        (popup.x + 80., popup.y + 100.)
    );
    // The release may contain the last coordinate without an intervening MouseMove.
    left_up(&mut app, x + 95., y + 215.);
    app.render();
    assert!(matches!(app.drag, Drag::None));
    assert_eq!(app.toolbar_position, Some((bar.x + 95., bar.y + 215.)));
    assert_eq!(app.tool_popup, Some(ToolPanel::Brush));
    assert_eq!(
        (
            app.tool_family_rect(ToolPanel::Brush).x,
            app.tool_family_rect(ToolPanel::Brush).y
        ),
        (family_button.x + 95., family_button.y + 215.)
    );
    for action in [
        Action::ToolPanel(ToolPanel::Brush),
        Action::ToolPanel(ToolPanel::Eraser),
        Action::ToolPanel(ToolPanel::Shapes),
        Action::Tool(Tool::Fill),
        Action::Tool(Tool::Picker),
        Action::Tool(Tool::Hand),
        Action::Undo,
        Action::Redo,
    ] {
        let hit = app
            .controls
            .iter()
            .find(|c| c.action == action)
            .unwrap()
            .hit;
        assert!(
            app.top_tools_rect()
                .contains(hit.x + hit.w * 0.5, hit.y + hit.h * 0.5),
            "{action:?} follows the moved toolbar's actual hit bounds"
        );
    }
    press_key(&mut app, 115, false);
    assert_popup_triangle_absorbs_canvas_input(&mut app);
    click_action(&mut app, Action::CloseToolProperties);
    press_key(&mut app, 27, false);
    app.render();
    assert!(app.quick_controls_visible());
    assert_eq!(
        (app.quick_controls_rect().x, app.quick_controls_rect().y),
        (quick_before.x + 95., quick_before.y + 215.)
    );

    click_action(&mut app, Action::ToolPanel(ToolPanel::Brush));
    click_action(&mut app, Action::PinToolPanel);
    app.render();
    let pinned = app.tool_popup_rect().unwrap();
    let pinned_quick = app.quick_controls_rect();
    assert_eq!(pinned.x + pinned.w * 0.5, app.w() * 0.5);
    assert_eq!(pinned.y, 136.);
    let (x, y) = control_center(&mut app, Action::MoveToolbar);
    left_down(&mut app, x, y);
    pointer_move(&mut app, x - 90., y + 180.);
    left_up(&mut app, x - 90., y + 180.);
    app.render();
    assert_eq!(
        rect_coordinates(app.tool_popup_rect().unwrap()),
        rect_coordinates(pinned)
    );
    assert_eq!(
        rect_coordinates(app.quick_controls_rect()),
        rect_coordinates(pinned_quick)
    );
    let moved_bar = app.top_tools_rect();
    let (x, y) = control_center(&mut app, Action::PinToolPanel);
    left_down(&mut app, x, y);
    pointer_move(&mut app, x + 120., y + 28.);
    app.render();
    assert!(matches!(
        app.drag,
        Drag::Panel {
            kind: PanelKind::Chooser,
            moved: true,
            ..
        }
    ));
    assert!(app.tool_panel_pinned);
    assert_eq!(
        (
            app.tool_popup_rect().unwrap().x,
            app.tool_popup_rect().unwrap().y
        ),
        (pinned.x + 120., pinned.y + 28.)
    );
    assert_eq!(
        (app.quick_controls_rect().x, app.quick_controls_rect().y),
        (pinned_quick.x + 120., pinned_quick.y + 28.)
    );
    left_up(&mut app, x + 120., y + 28.);
    app.render();
    assert!(
        app.tool_panel_pinned,
        "dragging the pin moves the group instead of unpinning it"
    );
    assert_eq!(
        rect_coordinates(app.top_tools_rect()),
        rect_coordinates(moved_bar)
    );
    assert_eq!(ToolSettings::from_brush(app.brush), settings);
    assert_eq!((app.view_x, app.view_y, app.zoom), view);
    assert_eq!(app.doc.to_bytes().unwrap(), before);
    assert_eq!(
        (
            app.doc.undo_count(),
            app.doc.redo_count(),
            app.doc.is_dirty()
        ),
        (0, 0, false)
    );
}

#[test]
fn panel_drag_interruptions_restore_default_or_custom_positions_without_canvas_leaks() {
    for kind in [PanelKind::Toolbar, PanelKind::Chooser] {
        for interruption in 0..3 {
            let mut app = floating_panel_fixture();
            let before = app.doc.to_bytes().unwrap();
            let view = (app.view_x, app.view_y, app.zoom);
            let settings = ToolSettings::from_brush(app.brush);
            let action = match kind {
                PanelKind::Toolbar => Action::MoveToolbar,
                PanelKind::Chooser => {
                    click_action(&mut app, Action::ToolPanel(ToolPanel::Brush));
                    click_action(&mut app, Action::PinToolPanel);
                    Action::PinToolPanel
                }
            };
            if interruption > 0 {
                let (x, y) = control_center(&mut app, action);
                left_down(&mut app, x, y);
                pointer_move(&mut app, x + 40., y + 30.);
                left_up(&mut app, x + 40., y + 30.);
                app.render();
            }
            let initial = match kind {
                PanelKind::Toolbar => app.toolbar_position,
                PanelKind::Chooser => app.chooser_position,
            };
            assert_eq!(initial.is_some(), interruption > 0);
            let initial_bounds = match kind {
                PanelKind::Toolbar => app.top_tools_rect(),
                PanelKind::Chooser => app.tool_popup_rect().unwrap(),
            };
            let (x, y) = control_center(&mut app, action);
            left_down(&mut app, x, y);
            pointer_move(&mut app, x + 100., y + 180.);
            app.render();
            assert!(matches!(app.drag, Drag::Panel { moved: true, .. }));
            let moved = match kind {
                PanelKind::Toolbar => app.toolbar_position,
                PanelKind::Chooser => app.chooser_position,
            };
            assert_ne!(moved, initial);
            for button in [MouseButton::Right, MouseButton::Middle] {
                app.event(Event::MouseDown {
                    x: x + 100.,
                    y: y + 180.,
                    button,
                    modifiers: Modifiers::default(),
                });
                app.event(Event::MouseUp {
                    x: x + 100.,
                    y: y + 180.,
                    button,
                    modifiers: Modifiers::default(),
                });
                assert!(matches!(app.drag, Drag::Panel { moved: true, .. }));
                assert_eq!(
                    match kind {
                        PanelKind::Toolbar => app.toolbar_position,
                        PanelKind::Chooser => app.chooser_position,
                    },
                    moved
                );
            }
            match interruption {
                0 => {
                    press_key(&mut app, 27, false);
                }
                1 => {
                    app.event(Event::FocusLost);
                }
                _ => {
                    app.event(Event::CaptureLost);
                }
            }
            assert!(matches!(app.drag, Drag::None));
            assert_eq!(
                match kind {
                    PanelKind::Toolbar => app.toolbar_position,
                    PanelKind::Chooser => app.chooser_position,
                },
                initial
            );
            assert_eq!(
                rect_coordinates(match kind {
                    PanelKind::Toolbar => app.top_tools_rect(),
                    PanelKind::Chooser => app.tool_popup_rect().unwrap(),
                }),
                rect_coordinates(initial_bounds)
            );
            left_up(&mut app, x + 100., y + 180.); // A late release cannot commit the cancelled position.
            for ctrl in [false, true] {
                app.event(Event::Wheel {
                    x: x + 100.,
                    y: y + 180.,
                    delta: 1.,
                    modifiers: Modifiers {
                        ctrl,
                        ..Modifiers::default()
                    },
                });
            }
            left_down(&mut app, x + 100., y + 180.); // The previous presented panel still owns this point.
            assert!(matches!(app.drag, Drag::None));
            left_up(&mut app, x + 100., y + 180.);
            app.render();
            if kind == PanelKind::Chooser {
                assert!(app.tool_panel_pinned);
                assert_eq!(app.tool_popup, Some(ToolPanel::Brush));
            }
            assert_eq!((app.view_x, app.view_y, app.zoom), view);
            assert_eq!(ToolSettings::from_brush(app.brush), settings);
            assert_eq!(app.doc.to_bytes().unwrap(), before);
            assert_eq!(
                (
                    app.doc.undo_count(),
                    app.doc.redo_count(),
                    app.doc.is_dirty()
                ),
                (0, 0, false)
            );
        }
    }
}

#[test]
fn movable_panels_clamp_with_resize_and_support_keyboard_and_independent_resets() {
    let mut app = floating_panel_fixture();
    let before = app.doc.to_bytes().unwrap();
    click_action(&mut app, Action::ToolPanel(ToolPanel::Brush));
    let (x, y) = control_center(&mut app, Action::MoveToolbar);
    left_down(&mut app, x, y);
    left_up(&mut app, x + 10000., y + 10000.);
    app.render();
    let bar = app.top_tools_rect();
    let popup = app.tool_popup_rect().unwrap();
    assert!(bar.x >= 16. && bar.y >= 72.);
    assert!(bar.x + bar.w <= app.w() - 16. && bar.y + bar.h <= app.h() - 16.);
    assert!(popup.x >= 16. && popup.x + popup.w <= app.w() - 16.);
    assert!(
        popup.y + popup.h <= bar.y,
        "the chooser flips above a toolbar at the bottom edge"
    );
    press_key(&mut app, 115, false);
    assert_popup_triangle_absorbs_canvas_input(&mut app);
    click_action(&mut app, Action::CloseToolProperties);
    click_action(&mut app, Action::PinToolPanel);
    let (x, y) = control_center(&mut app, Action::PinToolPanel);
    left_down(&mut app, x, y);
    left_up(&mut app, x + 10000., y + 10000.);
    app.resize(884, 581, 1.);
    app.render();
    let bar = app.top_tools_rect();
    let chooser = app.tool_popup_rect().unwrap();
    let quick = app.quick_controls_rect();
    for rect in [bar, chooser, quick] {
        assert!(rect.x >= 16. && rect.y >= 72.);
        assert!(rect.x + rect.w <= app.w() - 16. && rect.y + rect.h <= app.h() - 16.);
    }
    assert_eq!(quick.x, chooser.x);
    assert_eq!(quick.y, chooser.y + chooser.h + 12.);
    let chooser_position = app.chooser_position;
    for _ in 0..=app.controls.len() {
        if focused_action(&app) == Some(Action::MoveToolbar) {
            break;
        }
        press_key(&mut app, 117, false);
    }
    assert_eq!(focused_action(&app), Some(Action::MoveToolbar));
    press_key(&mut app, 36, false);
    app.render();
    assert_eq!(app.toolbar_position, None);
    assert_eq!(app.chooser_position, chooser_position);
    let centered_bar = app.top_tools_rect();
    press_key(&mut app, 39, false);
    app.render();
    app.event(Event::Key {
        code: 40,
        pressed: true,
        repeat: false,
        modifiers: Modifiers {
            shift: true,
            ..Modifiers::default()
        },
    });
    app.render();
    assert_eq!(
        (app.top_tools_rect().x, app.top_tools_rect().y),
        (centered_bar.x + 8., centered_bar.y + 24.)
    );
    let toolbar_position = app.toolbar_position;
    let (x, y) = control_center(&mut app, Action::PinToolPanel);
    app.event(Event::MouseDown {
        x,
        y,
        button: MouseButton::Right,
        modifiers: Modifiers::default(),
    });
    app.event(Event::MouseUp {
        x,
        y,
        button: MouseButton::Right,
        modifiers: Modifiers::default(),
    });
    app.render();
    assert!(app.tool_panel_pinned);
    assert_eq!(app.chooser_position, None);
    assert_eq!(app.toolbar_position, toolbar_position);
    let (x, y) = control_center(&mut app, Action::PinToolPanel);
    left_down(&mut app, x, y);
    pointer_move(&mut app, x + 40., y + 40.);
    left_up(&mut app, x + 40., y + 40.);
    app.render();
    let chooser_position = app.chooser_position;
    let (x, y) = control_center(&mut app, Action::MoveToolbar);
    app.event(Event::MouseDown {
        x,
        y,
        button: MouseButton::Right,
        modifiers: Modifiers::default(),
    });
    app.event(Event::MouseUp {
        x,
        y,
        button: MouseButton::Right,
        modifiers: Modifiers::default(),
    });
    app.render();
    assert_eq!(app.toolbar_position, None);
    assert_eq!(app.chooser_position, chooser_position);
    app.event(Event::Key {
        code: 48,
        pressed: true,
        repeat: false,
        modifiers: Modifiers {
            ctrl: true,
            alt: true,
            ..Modifiers::default()
        },
    });
    app.render();
    assert_eq!(app.toolbar_position, None);
    assert_eq!(app.chooser_position, None);
    let chooser = app.tool_popup_rect().unwrap();
    assert_eq!(chooser.x + chooser.w * 0.5, app.w() * 0.5);
    assert_eq!(chooser.y, 136.);
    assert!(app.tool_panel_pinned);
    app.chooser_position = Some((16., 220.));
    app.toolbar_position = Some((app.top_tools_rect().x, 384.));
    press_key(&mut app, 115, false);
    app.render();
    assert_eq!(
        rect_coordinates(app.tool_settings_rect()),
        (16., 80., 448., 180.)
    );
    assert!(app
        .browser_items_rect(ToolPanel::Brush)
        .contains(100., 232.));
    left_down(&mut app, 100., 232.);
    assert!(!matches!(app.drag, Drag::Browser { .. }));
    assert_eq!(
        app.tool_settings_section(),
        2,
        "the front Properties navigation owns a point overlapping the chooser row"
    );
    left_up(&mut app, 100., 232.);
    assert_eq!(app.doc.to_bytes().unwrap(), before);
    assert_eq!(
        (
            app.doc.undo_count(),
            app.doc.redo_count(),
            app.doc.is_dirty()
        ),
        (0, 0, false)
    );
}

fn set_tool_number(app: &mut PortfolioApp, action: Action, value: &str) {
    if !app.tool_settings_visible() {
        app.event(Event::Key {
            code: 115,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::default(),
        });
    }
    assert!(app.tool_settings_visible());
    assert!(app.tool_settings_reveal_action(action));
    let id = click_action(app, action)
        .into_iter()
        .find_map(|effect| match effect {
            Effect::Prompt { id, .. } => Some(id),
            _ => None,
        })
        .expect("the exact numeric readout opens a native prompt");
    let effects = app.event(Event::Prompted {
        id,
        value: Some(value.into()),
    });
    assert!(!effects
        .iter()
        .any(|effect| matches!(effect, Effect::Message { error: true, .. })));
}

#[test]
fn tool_family_popovers_toggle_switch_and_consume_dismissal_before_painting() {
    let mut app = floating_panel_fixture();
    let before = app.doc.to_bytes().unwrap();
    let viewport = (app.view_x, app.view_y, app.zoom);
    click_action(&mut app, Action::ToolPanel(ToolPanel::Brush));
    assert_eq!(app.tool_popup, Some(ToolPanel::Brush));
    for (width, height) in [(1024, 720), (1440, 940)] {
        app.resize(width, height, 1.0);
        app.render();
        let toolbar = app.top_tools_rect();
        let popup = app.tool_popup_rect().unwrap();
        let anchor = app.tool_family_rect(ToolPanel::Brush);
        assert_eq!(toolbar.x + toolbar.w * 0.5, width as f32 * 0.5);
        assert!(toolbar.w < width as f32 - 32.0);
        assert!(toolbar.contains(anchor.x, anchor.y));
        assert!(toolbar.contains(anchor.x + anchor.w - 1.0, anchor.y + anchor.h - 1.0));
        assert_eq!(popup.x, anchor.x);
        assert!(
            popup.y >= anchor.y + anchor.h,
            "the chooser stays below its family button after a resize"
        );
        assert!(popup.x >= 0.0 && popup.x + popup.w <= width as f32);
        assert!(popup.h < popup.w / 3.0, "the chooser stays horizontal");
        assert!(
            app.controls
                .iter()
                .filter(|c| { c.enabled && matches!(c.action, Action::Brush(_)) })
                .count()
                == 4
        );
        assert!(!app.tool_settings_visible());
        assert!(!app
            .controls
            .iter()
            .any(|c| matches!(c.action, Action::Size | Action::Opacity)));
    }
    click_action(&mut app, Action::ToolPanel(ToolPanel::Brush));
    assert_eq!(app.tool_popup, None);
    click_action(&mut app, Action::ToolPanel(ToolPanel::Brush));
    click_action(&mut app, Action::ToolPanel(ToolPanel::Eraser));
    assert_eq!(app.tool_popup, Some(ToolPanel::Eraser));
    assert_eq!(app.tool, Tool::Eraser);
    click_action(&mut app, Action::ToolPanel(ToolPanel::Brush));
    assert_eq!(app.tool_popup, Some(ToolPanel::Brush));
    app.event(Event::Key {
        code: 27,
        pressed: true,
        repeat: false,
        modifiers: Modifiers::default(),
    });
    assert_eq!(app.tool_popup, None);
    assert_eq!(app.doc.to_bytes().unwrap(), before);

    click_action(&mut app, Action::ToolPanel(ToolPanel::Brush));
    app.render();
    let popup = app.tool_popup_rect().unwrap();
    let (x, y) = (popup.x + 2.0, popup.y + 40.0);
    app.event(Event::Wheel {
        x,
        y,
        delta: 2.0,
        modifiers: Modifiers::default(),
    });
    assert_eq!((app.view_x, app.view_y, app.zoom), viewport);
    // Clicking the artwork dismisses the chooser; the same click cannot paint.
    let (x, y) = (650.0, 500.0);
    app.event(Event::MouseDown {
        x,
        y,
        button: MouseButton::Left,
        modifiers: Modifiers::default(),
    });
    app.event(Event::MouseUp {
        x,
        y,
        button: MouseButton::Left,
        modifiers: Modifiers::default(),
    });
    assert_eq!(app.tool_popup, None);
    assert_eq!(app.doc.to_bytes().unwrap(), before);
    assert_eq!(app.doc.undo_count(), 0);
    app.render();
    app.event(Event::MouseDown {
        x,
        y,
        button: MouseButton::Left,
        modifiers: Modifiers::default(),
    });
    app.event(Event::MouseUp {
        x,
        y,
        button: MouseButton::Left,
        modifiers: Modifiers::default(),
    });
    assert!(app.doc.is_dirty());
    assert_eq!(app.doc.undo_count(), 1);
}

#[test]
fn brush_eraser_and_shape_choices_keep_their_own_settings_without_editing_art() {
    let mut app = floating_panel_fixture();
    let before = app.doc.to_bytes().unwrap();
    click_action(&mut app, Action::ToolPanel(ToolPanel::Brush));
    click_action(&mut app, Action::Brush(1));
    assert_eq!(app.tool_popup, Some(ToolPanel::Brush));
    set_tool_number(&mut app, Action::Size, "37");
    set_tool_number(&mut app, Action::Opacity, "62");
    let painting = (app.brush.kind, app.brush.size, app.brush.opacity);
    assert_eq!(painting, (BrushKind::Soft, 37.0, 0.62));
    click_action(&mut app, Action::CloseToolProperties);

    click_action(&mut app, Action::ToolPanel(ToolPanel::Eraser));
    click_action(&mut app, Action::Brush(2));
    assert_eq!(
        app.tool,
        Tool::Eraser,
        "choosing an eraser preset must not select the paint brush"
    );
    assert_eq!(app.tool_popup, Some(ToolPanel::Eraser));
    set_tool_number(&mut app, Action::Size, "86");
    set_tool_number(&mut app, Action::Opacity, "41");
    let erasing = (app.brush.kind, app.brush.size, app.brush.opacity);
    assert_eq!(erasing, (BrushKind::Marker, 86.0, 0.41));
    click_action(&mut app, Action::CloseToolProperties);
    click_action(&mut app, Action::ToolPanel(ToolPanel::Brush));
    assert_eq!(
        (app.brush.kind, app.brush.size, app.brush.opacity),
        painting
    );
    click_action(&mut app, Action::ToolPanel(ToolPanel::Eraser));
    assert_eq!((app.brush.kind, app.brush.size, app.brush.opacity), erasing);

    app.event(Event::Key {
        code: 66,
        pressed: true,
        repeat: false,
        modifiers: Modifiers::default(),
    });
    assert_eq!(app.tool, Tool::Brush);
    assert_eq!(
        app.tool_popup, None,
        "B selects the brush without leaving a popup open"
    );
    assert_eq!(
        (app.brush.kind, app.brush.size, app.brush.opacity),
        painting
    );
    click_action(&mut app, Action::ToolPanel(ToolPanel::Shapes));
    for shape in [Tool::Line, Tool::Rectangle, Tool::Ellipse] {
        click_action(&mut app, Action::Shape(shape));
        assert_eq!(app.tool, shape);
        assert_eq!(app.last_shape, shape);
        assert_eq!(app.tool_popup, Some(ToolPanel::Shapes));
    }
    set_tool_number(&mut app, Action::Size, "7");
    set_tool_number(&mut app, Action::Opacity, "55");
    let shapes = (app.brush.kind, app.brush.size, app.brush.opacity);
    click_action(&mut app, Action::CloseToolProperties);
    click_action(&mut app, Action::ToolPanel(ToolPanel::Brush));
    assert_eq!(
        (app.brush.kind, app.brush.size, app.brush.opacity),
        painting
    );
    click_action(&mut app, Action::ToolPanel(ToolPanel::Eraser));
    assert_eq!((app.brush.kind, app.brush.size, app.brush.opacity), erasing);
    click_action(&mut app, Action::ToolPanel(ToolPanel::Shapes));
    assert_eq!(app.tool, Tool::Ellipse);
    assert_eq!((app.brush.kind, app.brush.size, app.brush.opacity), shapes);
    assert_eq!(app.doc.to_bytes().unwrap(), before);
    assert_eq!(app.doc.undo_count(), 0);
}

#[test]
fn keyboard_popover_focus_and_space_release_leave_the_next_canvas_gesture_painting() {
    fn key(app: &mut PortfolioApp, code: u32, pressed: bool) {
        app.event(Event::Key {
            code,
            pressed,
            repeat: false,
            modifiers: Modifiers::default(),
        });
    }
    fn press(app: &mut PortfolioApp, code: u32) {
        key(app, code, true);
        key(app, code, false);
    }
    fn focused_action(app: &PortfolioApp) -> Option<Action> {
        app.focus
            .and_then(|index| app.controls.get(index))
            .map(|control| control.action)
    }
    fn traverse_to(app: &mut PortfolioApp, action: Action) {
        for _ in 0..=app.controls.len() {
            if focused_action(app) == Some(action) {
                return;
            }
            press(app, 117); // F6
        }
        panic!("F6 must reach the visible action {action:?}");
    }

    let mut app = floating_panel_fixture();
    let original = app.doc.to_bytes().unwrap();
    let viewport = (app.view_x, app.view_y, app.zoom);
    let family = Action::ToolPanel(ToolPanel::Brush);
    traverse_to(&mut app, family);
    press(&mut app, 13);
    app.render();
    assert_eq!(app.tool_popup, Some(ToolPanel::Brush));
    assert_eq!(focused_action(&app), Some(Action::Brush(0)));

    traverse_to(&mut app, Action::Brush(1));
    press(&mut app, 13);
    app.render();
    assert_eq!(app.brush.kind, BrushKind::Soft);
    assert_eq!(focused_action(&app), Some(Action::Brush(1)));
    press(&mut app, 27);
    app.render();
    assert_eq!(app.tool_popup, None);
    assert_eq!(focused_action(&app), Some(family));

    // Reopening through the returned focus must enter the selected option.
    press(&mut app, 13);
    app.render();
    assert_eq!(focused_action(&app), Some(Action::Brush(1)));
    press(&mut app, 27);
    app.render();
    assert_eq!(app.tool_popup, None);
    assert_eq!(focused_action(&app), Some(family));

    press(&mut app, 115);
    app.render();
    assert!(matches!(app.overlay, Overlay::ToolProperties));
    assert_eq!(focused_action(&app), Some(Action::Size));
    let initial_size = app.brush.size;
    let slider = app
        .controls
        .iter()
        .find(|control| {
            control.enabled
                && control.action == Action::Size
                && control.kind == ControlKind::Slider
                && app.tool_settings_rect().contains(
                    control.hit.x + control.hit.w * 0.5,
                    control.hit.y + control.hit.h * 0.5,
                )
        })
        .expect("the parameter dropdown exposes its size slider")
        .hit;
    app.event(Event::MouseDown {
        x: slider.x + slider.w * 0.85,
        y: slider.y + slider.h * 0.5,
        button: MouseButton::Left,
        modifiers: Modifiers::default(),
    });
    assert_ne!(app.brush.size, initial_size);
    assert!(matches!(app.drag, Drag::Slider { .. }));
    press(&mut app, 27);
    app.render();
    assert_eq!(app.brush.size, initial_size);
    assert!(matches!(app.drag, Drag::None));
    assert!(
        matches!(app.overlay, Overlay::ToolProperties),
        "Escape first cancels an active slider without dismissing its dropdown"
    );
    press(&mut app, 27);
    app.render();
    assert!(!app.tool_settings_visible());
    assert_eq!(
        focused_action(&app),
        Some(app.tool_properties_return_action())
    );
    assert_eq!(app.doc.to_bytes().unwrap(), original);

    // Keyboard traversal while Space is held must not strand the pan modifier.
    press(&mut app, 27);
    assert!(app.focus.is_none());
    app.render();
    key(&mut app, 32, true);
    assert!(app.space);
    press(&mut app, 117);
    assert!(app.focus.is_some());
    key(&mut app, 32, false);
    assert!(!app.space);
    app.event(Event::MouseDown {
        x: 650.0,
        y: 500.0,
        button: MouseButton::Left,
        modifiers: Modifiers::default(),
    });
    assert!(matches!(app.drag, Drag::Stroke));
    app.event(Event::MouseMove {
        x: 700.0,
        y: 530.0,
        modifiers: Modifiers::default(),
    });
    app.event(Event::MouseUp {
        x: 700.0,
        y: 530.0,
        button: MouseButton::Left,
        modifiers: Modifiers::default(),
    });
    assert!(app.doc.is_dirty());
    assert_eq!(app.doc.undo_count(), 1);
    assert_ne!(app.doc.to_bytes().unwrap(), original);
    assert_eq!((app.view_x, app.view_y, app.zoom), viewport);
}

#[test]
fn appearance_preferences_roundtrip_and_migrate_legacy_theme_files() {
    let directory = FixtureDirectory::new();
    let preferences = directory.file("settings.txt");
    let mut app = app_fixture();
    app.prefs = Some(preferences.clone());

    for (legacy, dark) in [("paper\n", false), ("graphite\n", true)] {
        fs::write(&preferences, legacy).unwrap();
        let migrated = Appearance::decode(&fs::read_to_string(&preferences).unwrap());
        assert_eq!(
            migrated,
            Appearance {
                dark,
                ..Appearance::default()
            }
        );
        app.dark = migrated.dark;
        app.ui_font = migrated.font;
        app.accent = migrated.accent;
        let mut effects = Vec::new();
        app.persist_appearance(&mut effects);
        assert!(!effects
            .iter()
            .any(|effect| matches!(effect, Effect::Message { error: true, .. })));
        let encoded = fs::read_to_string(&preferences).unwrap();
        assert!(encoded.starts_with("version=3\n"));
        assert_eq!(Appearance::decode(&encoded), migrated);
    }

    let assert_persisted = |app: &mut PortfolioApp, expected: Appearance| {
        app.dark = expected.dark;
        app.ui_font = expected.font;
        app.accent = expected.accent;
        app.language = expected.language;
        app.font_size = expected.font_size;
        app.animations = expected.animations;
        let mut effects = Vec::new();
        app.persist_appearance(&mut effects);
        assert!(!effects
            .iter()
            .any(|effect| matches!(effect, Effect::Message { error: true, .. })));
        let encoded = fs::read_to_string(&preferences).unwrap();
        assert_eq!(Appearance::decode(&encoded), expected);
    };
    // The font and accent catalogs have different lengths. Traverse each one
    // independently and retain a non-default value from the other catalog.
    for font in UiFont::ALL {
        assert_persisted(
            &mut app,
            Appearance {
                dark: false,
                font,
                accent: Accent::Red,
                ..Appearance::default()
            },
        );
    }
    for accent in Accent::ALL {
        assert_persisted(
            &mut app,
            Appearance {
                dark: true,
                font: UiFont::Gabriola,
                accent,
                ..Appearance::default()
            },
        );
    }
}

#[test]
fn live_appearance_changes_preserve_art_and_outside_dismissal_does_not_paint() {
    let directory = FixtureDirectory::new();
    let preferences = directory.file("settings.txt");
    let mut app = floating_panel_fixture();
    app.prefs = Some(preferences.clone());
    edit(&mut app, 20.0);
    edit(&mut app, 32.0);
    assert!(app.doc.undo());
    app.changed();
    app.brush.kind = BrushKind::Soft;
    app.brush.size = 37.0;
    app.brush.opacity = 0.62;
    let paint_state = |app: &PortfolioApp| {
        (
            app.doc.to_bytes().unwrap(),
            (
                app.doc.undo_count(),
                app.doc.redo_count(),
                app.doc.is_dirty(),
            ),
            (app.view_x, app.view_y, app.zoom),
            (
                app.tool,
                app.brush.kind,
                app.brush.size,
                app.brush.opacity,
                app.brush.color,
                app.brush.mirror,
            ),
        )
    };
    let before = paint_state(&app);
    click_action(&mut app, Action::ToolPanel(ToolPanel::Brush));
    assert_eq!(app.tool_popup, Some(ToolPanel::Brush));
    app.event(Event::Key {
        code: 188, // Ctrl+, opens appearance settings from the canvas or chooser.
        pressed: true,
        repeat: false,
        modifiers: Modifiers {
            ctrl: true,
            ..Modifiers::default()
        },
    });
    app.render();
    assert!(matches!(app.overlay, Overlay::Settings));
    assert_eq!(app.tool_popup, None);
    assert_eq!(
        app.focus.map(|index| app.controls[index].action),
        Some(Action::SettingsSearch)
    );
    let effects = click_action(&mut app, Action::SetTheme(false));
    assert!(effects
        .iter()
        .any(|effect| matches!(effect, Effect::SetDarkMode(false))));
    assert!(!effects
        .iter()
        .any(|effect| matches!(effect, Effect::Message { error: true, .. })));
    assert_eq!(paint_state(&app), before);
    for action in [
        Action::SetFont(UiFont::Gabriola),
        Action::SetAccent(Accent::Red),
    ] {
        activate_jump(
            &mut app,
            Action::SettingsSection(if matches!(action, Action::SetFont(_)) {
                1
            } else {
                0
            }),
        );
        if matches!(action, Action::SetFont(_)) {
            click_action(&mut app, Action::FontMenu);
        }
        let effects = click_action(&mut app, action);
        assert!(!effects
            .iter()
            .any(|effect| matches!(effect, Effect::Message { error: true, .. })));
        assert_eq!(paint_state(&app), before);
    }
    let appearance = Appearance {
        dark: false,
        font: UiFont::Gabriola,
        accent: Accent::Red,
        ..Appearance::default()
    };
    assert_eq!(
        (app.dark, app.ui_font, app.accent),
        (appearance.dark, appearance.font, appearance.accent)
    );
    assert_eq!(
        Appearance::decode(&fs::read_to_string(&preferences).unwrap()),
        appearance
    );
    let frame = app.render();
    let brand = frame
        .texts
        .iter()
        .find(|run| run.text == "Portfolio")
        .expect("the header retains the application name");
    assert_eq!(brand.font, UiFont::Gabriola);
    let body = frame
        .texts
        .iter()
        .find(|run| run.text == "Учитываются настройки Windows")
        .expect("the appearance panel retains its small explanatory text");
    assert_eq!(body.font, UiFont::Gabriola);
    for run in frame
        .texts
        .iter()
        .filter(|run| !run.serif && run.text != "Aa · 0123")
    {
        assert_eq!(
            run.font,
            UiFont::Gabriola,
            "ordinary UI text {:?} follows the selected font",
            run.text
        );
    }

    let (x, y) = (650.0, 500.0);
    assert!(app.canvas().contains(x, y));
    assert!(!app.settings_rect().contains(x, y));
    for event in [
        Event::MouseDown {
            x,
            y,
            button: MouseButton::Left,
            modifiers: Modifiers::default(),
        },
        Event::MouseUp {
            x,
            y,
            button: MouseButton::Left,
            modifiers: Modifiers::default(),
        },
    ] {
        app.event(event);
    }
    assert!(matches!(app.overlay, Overlay::None));
    assert_eq!(paint_state(&app), before);
    app.render();
    app.event(Event::MouseDown {
        x,
        y,
        button: MouseButton::Left,
        modifiers: Modifiers::default(),
    });
    app.event(Event::MouseUp {
        x,
        y,
        button: MouseButton::Left,
        modifiers: Modifiers::default(),
    });
    assert_ne!(app.doc.to_bytes().unwrap(), before.0);
    assert_eq!(app.doc.undo_count(), before.1 .0 + 1);
    assert_eq!(app.doc.redo_count(), 0);
    assert_eq!((app.view_x, app.view_y, app.zoom), before.2);
}

#[test]
fn modeless_tool_properties_share_input_with_chooser_quick_controls_inspector_and_canvas() {
    for (width, height) in [(1440, 940), (884, 581)] {
        for pinned in [false, true] {
            let mut app = floating_panel_fixture();
            app.resize(width, height, 1.);
            let before = app.doc.to_bytes().unwrap();
            click_action(&mut app, Action::ToolPanel(ToolPanel::Brush));
            if pinned {
                click_action(&mut app, Action::PinToolPanel);
            }
            press_key(&mut app, 115, false);
            app.render();
            assert!(app.tool_settings_visible());
            assert_eq!(app.tool_popup, Some(ToolPanel::Brush));
            assert!(app.quick_controls_visible());
            assert_eq!(focused_action(&app), Some(Action::Size));
            let focused = app.controls[app.focus.unwrap()].hit;
            assert!(
                app.tool_settings_rect()
                    .contains(focused.x + focused.w * 0.5, focused.y + focused.h * 0.5),
                "F4 focuses the parameter panel, not the duplicate quick Size control"
            );
            click_action(&mut app, Action::CloseToolProperties);
            click_action(&mut app, Action::ToolProperties); // Pointer opening has no F4 focus source.
            app.render();
            assert!(!app.pending_properties_focus);
            for _ in 0..=app.controls.len() {
                if focused_action(&app) == Some(Action::CloseToolProperties) {
                    break;
                }
                press_key(&mut app, 117, false);
                app.render();
            }
            assert_eq!(focused_action(&app), Some(Action::CloseToolProperties));
            for action in [
                Action::RenameBrush,
                Action::AddBrush,
                Action::ToolSection(0),
                Action::ToolSection(1),
                Action::ToolSection(2),
                Action::Size,
            ] {
                press_key(&mut app, 9, false);
                assert_eq!(app.pending_focus, Some(action));
                assert!(app.pending_properties_focus);
                app.render();
                assert_eq!(focused_action(&app), Some(action));
                assert_eq!(
                    app.tool_settings_section(),
                    0,
                    "focus alone does not select another section"
                );
            }
            assert_eq!(focused_action(&app), Some(Action::Size));
            let focused = app.controls[app.focus.unwrap()].hit;
            assert!(
                app.tool_settings_rect()
                    .contains(focused.x + focused.w * 0.5, focused.y + focused.h * 0.5),
                "Tab from the pointer-opened panel keeps Size focus inside Properties"
            );
            click_action(&mut app, Action::ToolSection(2));
            app.render();
            assert_eq!(app.tool_settings_section(), 2);
            click_action(&mut app, Action::CloseToolProperties);
            press_key(&mut app, 115, false);
            app.render(); // One frame must both reveal Size and retain every surrounding surface.
            assert_eq!(app.tool_settings_section(), 0);
            assert_eq!(focused_action(&app), Some(Action::Size));
            let focused = app.controls[app.focus.unwrap()].hit;
            assert!(app
                .tool_settings_rect()
                .contains(focused.x + focused.w * 0.5, focused.y + focused.h * 0.5));
            assert!(app
                .controls
                .iter()
                .any(|c| c.enabled && c.action == Action::Brush(1)));
            for action in [Action::Size, Action::Opacity] {
                let quick = app.quick_control_rect(action);
                assert!(app.controls.iter().any(|c| c.enabled
                    && c.action == action
                    && c.kind == ControlKind::Slider
                    && quick.contains(c.hit.x + c.hit.w * 0.5, c.hit.y + c.hit.h * 0.5)));
            }
            assert!(app
                .controls
                .iter()
                .any(|c| c.enabled && c.action == Action::Hex));
            click_action(&mut app, Action::Brush(1));
            assert_eq!(app.brush.kind, BrushKind::Soft);
            assert!(app.tool_settings_visible());
            click_action(&mut app, Action::Swatch(2));
            assert_eq!(app.brush.color, Color::from_hex(PALETTE[2]));
            assert!(app.tool_settings_visible());
            assert_eq!(app.tool_popup, Some(ToolPanel::Brush));

            for family in [ToolPanel::Eraser, ToolPanel::Shapes, ToolPanel::Brush] {
                click_action(&mut app, Action::ToolPanel(family));
                app.render();
                assert_eq!(app.tool_popup, Some(family));
                assert!(app.tool_settings_visible());
                assert_eq!(ToolPanel::for_tool(app.tool), Some(family));
            }
            press_key(&mut app, 118, false); // The Layers inspector remains interactive.
            click_action(&mut app, Action::Layer(0));
            assert_eq!(app.doc.active, 0);
            assert!(app.tool_settings_visible());
            click_action(&mut app, Action::Layer(1));
            assert_eq!(app.doc.active, 1);
            assert!(app.tool_settings_visible());
            assert_eq!(app.doc.to_bytes().unwrap(), before);
            assert_eq!((app.doc.undo_count(), app.doc.redo_count()), (0, 0));

            assert!(app.quick_controls_visible());
            let initial_size = app.brush.size;
            begin_quick_drag(&mut app, Action::Size, 0.5);
            press_key(&mut app, 27, false);
            assert_eq!(app.brush.size, initial_size);
            assert!(
                app.tool_settings_visible(),
                "Escape first rolls back an active quick slider"
            );
            let (x, y) = begin_quick_drag(&mut app, Action::Size, 0.4);
            left_up(&mut app, x, y);
            app.render();
            let quick = app.quick_control_rect(Action::Size);
            let focused = app.controls[app
                .focus
                .expect("a clicked quick slider retains keyboard focus")]
            .hit;
            assert!(quick.contains(focused.x + focused.w * 0.5, focused.y + focused.h * 0.5));
            let size = app.brush.size;
            press_key(&mut app, 39, false);
            assert_eq!(app.brush.size, size + 1.);
            assert!(app.tool_settings_visible());
            app.render();
            assert!(app.controls.iter().any(|c| c.action == Action::Size
                && c.kind == ControlKind::Value
                && app
                    .tool_settings_rect()
                    .contains(c.hit.x + c.hit.w * 0.5, c.hit.y + c.hit.h * 0.5)));

            let panel = app.tool_settings_rect();
            click_point(&mut app, panel.x + 3., panel.y + panel.h * 0.5);
            assert!(app.tool_settings_visible());
            assert_eq!(app.doc.to_bytes().unwrap(), before);
            let (x, y) = [(500., 480.), (850., 700.), (420., 420.)]
                .into_iter()
                .find(|&(x, y)| {
                    app.stage().contains(x, y)
                        && app.canvas().contains(x, y)
                        && !app.canvas_obscured(x, y)
                })
                .expect("there is an unobscured canvas point beside the independent panels");
            let zoom = app.zoom;
            app.event(Event::Wheel {
                x,
                y,
                delta: 0.25,
                modifiers: Modifiers::default(),
            });
            assert_ne!(app.zoom, zoom);
            assert!(app.tool_settings_visible());
            left_down(&mut app, x, y);
            assert!(matches!(app.drag, Drag::Stroke));
            pointer_move(&mut app, x + 15., y + 15.);
            left_up(&mut app, x + 15., y + 15.);
            assert_eq!(app.doc.undo_count(), 1);
            assert!(
                app.tool_settings_visible(),
                "painting outside the parameter panel does not dismiss it"
            );
            press_key(&mut app, 90, true);
            assert_eq!(app.doc.to_bytes().unwrap(), before);
            assert!(app.tool_settings_visible());
            press_key(&mut app, 72, false);
            assert_eq!(app.tool, Tool::Hand);
            assert!(!app.tool_settings_visible());
            press_key(&mut app, 115, false);
            assert!(!app.tool_settings_visible());
            assert_eq!(app.doc.to_bytes().unwrap(), before);
        }
    }
}

fn press_key(app: &mut PortfolioApp, code: u32, ctrl: bool) -> Vec<Effect> {
    app.event(Event::Key {
        code,
        pressed: true,
        repeat: false,
        modifiers: Modifiers {
            ctrl,
            ..Modifiers::default()
        },
    })
}

fn focused_action(app: &PortfolioApp) -> Option<Action> {
    app.focus
        .and_then(|i| app.controls.get(i))
        .map(|c| c.action)
}

fn click_point(app: &mut PortfolioApp, x: f32, y: f32) -> Vec<Effect> {
    let mut effects = app.event(Event::MouseDown {
        x,
        y,
        button: MouseButton::Left,
        modifiers: Modifiers::default(),
    });
    effects.extend(app.event(Event::MouseUp {
        x,
        y,
        button: MouseButton::Left,
        modifiers: Modifiers::default(),
    }));
    effects
}

#[test]
fn font_size_is_live_persisted_and_cancellable_without_editing_the_art() {
    let directory = FixtureDirectory::new();
    let path = directory.file("settings.txt");
    let mut app = floating_panel_fixture();
    app.resize(884, 581, 1.0);
    app.prefs = Some(path.clone());
    let before = app.doc.to_bytes().unwrap();
    let view = (app.view_x, app.view_y, app.zoom);
    let brush = (
        app.brush.kind,
        app.brush.size,
        app.brush.opacity,
        app.brush.color,
    );
    press_key(&mut app, 188, true);
    activate_jump(&mut app, Action::SettingsSection(1));
    let baseline = app
        .render()
        .texts
        .iter()
        .find(|t| t.text == "Portfolio")
        .unwrap()
        .size;
    for size in 11..=17 {
        let effects = click_action(&mut app, Action::FontSize);
        assert!(effects
            .iter()
            .any(|e| matches!(e, Effect::Prompt { id: 22, .. })));
        let effects = app.event(Event::Prompted {
            id: 22,
            value: Some(size.to_string()),
        });
        assert!(!effects
            .iter()
            .any(|e| matches!(e, Effect::Message { error: true, .. })));
        assert_eq!(app.font_size, size);
        assert_eq!(
            Appearance::decode(&fs::read_to_string(&path).unwrap()).font_size,
            size
        );
        assert!((app.ui_font_scale() - size as f32 / 13.0).abs() < 0.0001);
        let frame = app.render();
        let brand = frame.texts.iter().find(|t| t.text == "Portfolio").unwrap();
        assert!((brand.size - baseline * size as f32 / 13.0).abs() < 0.001);
        let bounds = app.settings_rect();
        assert!(app
            .controls
            .iter()
            .filter(|c| c.enabled)
            .all(|c| c.hit.x >= bounds.x
                && c.hit.y >= bounds.y
                && c.hit.x + c.hit.w <= bounds.x + bounds.w
                && c.hit.y + c.hit.h <= bounds.y + bounds.h));
        assert_eq!(app.doc.to_bytes().unwrap(), before);
    }
    for input in ["NaN", "10", "18", "Infinity", "not a number"] {
        let effects = app.event(Event::Prompted {
            id: 22,
            value: Some(input.into()),
        });
        assert!(effects
            .iter()
            .any(|e| matches!(e, Effect::Message { error: true, .. })));
        assert_eq!(app.font_size, 17);
    }
    app.event(Event::Prompted {
        id: 22,
        value: None,
    });
    assert_eq!(app.font_size, 17);
    app.event(Event::Prompted {
        id: 22,
        value: Some("13".into()),
    });
    app.render();
    let track = app
        .controls
        .iter()
        .find(|c| c.action == Action::FontSize && c.kind == ControlKind::Slider)
        .unwrap()
        .rect;
    let (x, y) = (track.x + track.w - 5.0, track.y + track.h * 0.5);
    app.event(Event::MouseDown {
        x,
        y,
        button: MouseButton::Left,
        modifiers: Modifiers::default(),
    });
    assert_eq!(app.font_size, 17);
    assert!(matches!(
        app.drag,
        Drag::Slider {
            action: Action::FontSize,
            ..
        }
    ));
    assert_eq!(
        Appearance::decode(&fs::read_to_string(&path).unwrap()).font_size,
        13
    );
    press_key(&mut app, 27, false);
    assert_eq!(app.font_size, 13);
    assert!(matches!(app.overlay, Overlay::Settings));
    assert_eq!(
        Appearance::decode(&fs::read_to_string(&path).unwrap()).font_size,
        13
    );
    app.event(Event::MouseUp {
        x,
        y,
        button: MouseButton::Left,
        modifiers: Modifiers::default(),
    });
    app.render();
    click_point(&mut app, x, y);
    assert_eq!(app.font_size, 17);
    assert_eq!(
        Appearance::decode(&fs::read_to_string(&path).unwrap()).font_size,
        17
    );
    assert_eq!(app.doc.to_bytes().unwrap(), before);
    assert_eq!((app.view_x, app.view_y, app.zoom), view);
    assert_eq!(
        (
            app.brush.kind,
            app.brush.size,
            app.brush.opacity,
            app.brush.color
        ),
        brush
    );
    assert_eq!((app.doc.undo_count(), app.doc.redo_count()), (0, 0));
}

#[test]
fn ten_fonts_and_six_languages_are_reachable_and_preserve_user_names() {
    let directory = FixtureDirectory::new();
    let path = directory.file("appearance.txt");
    let mut app = app_fixture();
    app.resize(884, 581, 1.0);
    app.prefs = Some(path.clone());
    app.doc.title = "Настройки".into();
    app.doc.layers[app.doc.active].name = "Слой {1} — копия".into();
    app.doc.mark_saved();
    let before = app.doc.to_bytes().unwrap();
    press_key(&mut app, 188, true);
    activate_jump(&mut app, Action::SettingsSection(1));
    assert_eq!(UiFont::ALL.len(), 10);
    for font in UiFont::ALL {
        click_action(&mut app, Action::FontMenu);
        click_action(&mut app, Action::SetFont(font));
        let frame = app.render();
        assert_eq!(app.ui_font(), font);
        assert_eq!(
            Appearance::decode(&fs::read_to_string(&path).unwrap()).font,
            font
        );
        assert!(frame
            .texts
            .iter()
            .filter(|t| t.text != "Aa · 0123")
            .all(|t| t.font == font));
    }
    activate_jump(&mut app, Action::SettingsSection(2));
    for language in Language::ALL {
        click_action(&mut app, Action::LanguageMenu);
        click_action(&mut app, Action::SetLanguage(language));
        let frame = app.render();
        assert_eq!(app.preferred_language(), language.key());
        assert_eq!(
            Appearance::decode(&fs::read_to_string(&path).unwrap()).language,
            language
        );
        assert!(
            frame.texts.iter().any(|t| t.text == "Настройки"),
            "user title remains raw"
        );
        assert!(frame
            .texts
            .iter()
            .any(|t| t.text == language.translate("Язык интерфейса")));
        assert_eq!(app.doc.to_bytes().unwrap(), before);
    }
    let migrated = Appearance::decode("version=2\ntheme=paper\nfont=segoe_script\nlanguage=ja\n");
    assert!(!migrated.dark);
    assert_eq!(migrated.font, UiFont::SegoeScript);
    assert_eq!(migrated.language, Language::Japanese);
    assert_eq!(migrated.font_size, 13);
    assert!(migrated.animations);
    let invalid = Appearance::decode("font_size=999\nanimations=invalid\n");
    assert_eq!(invalid, Appearance::default());
    activate_jump(&mut app, Action::SettingsSection(0));
    click_action(&mut app, Action::SetAnimations(false));
    assert!(!Appearance::decode(&fs::read_to_string(&path).unwrap()).animations);
    click_action(&mut app, Action::ResetAppearance);
    assert_eq!(app.language, Language::Japanese);
    assert!(app.animations);
    assert_eq!(app.font_size, 13);
    assert_eq!(app.doc.to_bytes().unwrap(), before);
}

#[test]
fn animated_panel_focus_survives_clipping_and_transitions_do_not_paint_through() {
    let mut app = floating_panel_fixture();
    app.motion_runtime = true;
    app.motion_preview = Some(0.0);
    let before = app.doc.to_bytes().unwrap();
    press_key(&mut app, 115, false);
    app.render();
    assert_eq!(app.pending_focus, Some(Action::Size));
    assert!(app.pending_properties_focus);
    let panel = app.tool_settings_rect();
    assert!(!app.controls.iter().any(|c| c.action == Action::Size
        && panel.contains(c.hit.x + c.hit.w * 0.5, c.hit.y + c.hit.h * 0.5)));
    app.motion_preview = Some(1.0);
    app.render();
    assert_eq!(app.pending_focus, None);
    assert_eq!(
        app.focus.map(|i| app.controls[i].action),
        Some(Action::Size)
    );
    let focused = app.controls[app.focus.unwrap()].hit;
    assert!(panel.contains(focused.x + focused.w * 0.5, focused.y + focused.h * 0.5));
    press_key(&mut app, 188, true);
    app.motion_preview = Some(0.0);
    app.render();
    assert_eq!(app.pending_focus, Some(Action::SettingsSearch));
    app.motion_preview = Some(1.0);
    app.render();
    assert_eq!(
        app.focus.map(|i| app.controls[i].action),
        Some(Action::SettingsSearch)
    );
    app.overlay_motion.snap(true);
    let (x, y) = (850.0, 700.0);
    assert!(!app.settings_rect().contains(x, y));
    click_point(&mut app, x, y);
    assert_eq!(app.overlay, Overlay::None);
    assert!(app.closing_overlay.is_some());
    assert_eq!(app.doc.to_bytes().unwrap(), before);
    // A second click while the closing surface is still visible consumes it.
    click_point(&mut app, x, y);
    assert!(app.closing_overlay.is_none());
    assert_eq!(app.doc.to_bytes().unwrap(), before);
    app.render();
    click_point(&mut app, x, y);
    assert_eq!(app.doc.undo_count(), 1);
    assert_ne!(app.doc.to_bytes().unwrap(), before);
}

#[test]
fn switching_from_a_closing_chooser_keeps_settings_visible_and_reduced_motion_snaps() {
    let mut app = floating_panel_fixture();
    app.motion_runtime = true;
    app.motion_preview = Some(1.0);
    let before = app.doc.to_bytes().unwrap();
    click_action(&mut app, Action::ToolPanel(ToolPanel::Brush));
    app.popup_motion.snap(true);
    press_key(&mut app, 188, true);
    assert_eq!(app.overlay, Overlay::Settings);
    assert!(app.closing_popup.is_some());
    app.render();
    let search = app
        .controls
        .iter()
        .find(|c| c.action == Action::SettingsSearch)
        .unwrap()
        .hit;
    click_point(
        &mut app,
        search.x + search.w * 0.5,
        search.y + search.h * 0.5,
    );
    assert_eq!(app.overlay, Overlay::Settings);
    assert_eq!(app.overlay_motion.target(), 1.0);
    assert_eq!(app.popup_motion.target(), 0.0);
    assert!(app.closing_popup.is_none());
    assert!(app.settings_ui.focused);
    app.motion_preview = None;
    app.event(Event::ReducedMotion(true));
    assert!(app.system_reduced_motion);
    assert!(
        app.animations,
        "the OS preference does not overwrite the user preference"
    );
    assert!(!app.wants_animation());
    assert_eq!(app.overlay_motion.value(), 1.0);
    press_key(&mut app, 27, false); // Blur search before closing the settings surface.
    press_key(&mut app, 27, false);
    assert!(app.closing_overlay.is_none());
    assert_eq!(app.overlay_motion.value(), 0.0);
    assert_eq!(app.doc.to_bytes().unwrap(), before);
}

#[test]
fn brand_menu_toolbar_history_and_layers_are_reachable_in_a_compact_window() {
    let mut app = floating_panel_fixture();
    app.resize(884, 581, 1.0);
    let original = app.doc.to_bytes().unwrap();
    edit(&mut app, 24.0);
    let painted = app.doc.to_bytes().unwrap();
    let frame = app.render();
    assert!(!frame.texts.iter().any(|t| t.text == "Файл"));
    let bar = app.top_tools_rect();
    for action in [Action::Undo, Action::Redo] {
        let control = app.controls.iter().find(|c| c.action == action).unwrap();
        assert!(bar.contains(
            control.rect.x + control.rect.w * 0.5,
            control.rect.y + control.rect.h * 0.5
        ));
    }
    click_action(&mut app, Action::Undo);
    assert_eq!(app.doc.to_bytes().unwrap(), original);
    click_action(&mut app, Action::Redo);
    assert_eq!(app.doc.to_bytes().unwrap(), painted);
    click_action(&mut app, Action::File);
    assert_eq!(app.overlay, Overlay::File);
    assert!(app
        .render()
        .texts
        .iter()
        .any(|t| t.text == "Открыть проект…"));
    press_key(&mut app, 27, false);
    press_key(&mut app, 118, false);
    assert!(app.inspector_layers);
    app.render();
    assert!(app
        .controls
        .iter()
        .any(|c| c.action == Action::AddLayer && c.enabled));
    assert!(app
        .controls
        .iter()
        .any(|c| c.action == Action::Layer(app.doc.active) && c.enabled));
    assert_eq!(app.doc.to_bytes().unwrap(), painted);
}

fn deliver_io(app: &mut PortfolioApp, message: IoMessage) -> Vec<Effect> {
    let (tx, rx) = mpsc::channel();
    app.io = Some(rx);
    app.busy = true;
    assert!(tx.send(message).is_ok());
    app.event(Event::Tick)
}

fn image_result(color: Color) -> Result<platform::ImportedImage, String> {
    Ok(platform::ImportedImage {
        width: 2,
        height: 2,
        pixels: vec![color; 4],
    })
}

#[test]
fn image_import_reports_one_summary_preserves_order_and_resets_the_next_batch() {
    let mut app = app_fixture();
    app.language = Language::English;
    let original = app.doc.to_bytes().unwrap();
    let too_many = app.event(Event::FilesDropped {
        paths: (0..33).map(|n| PathBuf::from(format!("{n}.png"))).collect(),
    });
    assert!(too_many
        .iter()
        .any(|e| matches!(e, Effect::Message { error: true, .. })));
    assert!(app.import_queue.is_empty() && app.io.is_none());
    assert_eq!(app.doc.to_bytes().unwrap(), original);
    let bad_path = PathBuf::from("Слой {1} — копия.png");
    app.importing = true;
    app.import_queue = VecDeque::from([bad_path.clone(), PathBuf::from("last.png")]);
    let first = deliver_io(
        &mut app,
        IoMessage::Imported {
            path: PathBuf::from("first.png"),
            result: image_result(Color::BLACK),
        },
    );
    assert!(!first.iter().any(|e| matches!(e, Effect::Message { .. })));
    assert_eq!(app.doc.layers.len(), 3);
    assert_eq!(app.doc.active_layer().name, "first");
    assert!(app.inspector_layers);
    let middle = deliver_io(
        &mut app,
        IoMessage::Imported {
            path: bad_path.clone(),
            result: Err("Повреждён буфер изображения.".into()),
        },
    );
    assert!(!middle.iter().any(|e| matches!(e, Effect::Message { .. })));
    assert_eq!(app.doc.layers.len(), 3);
    let final_effects = deliver_io(
        &mut app,
        IoMessage::Imported {
            path: PathBuf::from("last.png"),
            result: image_result(Color::WHITE),
        },
    );
    let errors: Vec<_> = final_effects
        .iter()
        .filter_map(|e| match e {
            Effect::Message {
                message,
                error: true,
                ..
            } => Some(message),
            _ => None,
        })
        .collect();
    assert_eq!(errors.len(), 1);
    assert!(errors[0].contains("Some images could not be imported."));
    assert!(errors[0].contains("Слой {1} — копия.png"));
    assert_eq!(app.doc.layers.len(), 4);
    assert_eq!(app.doc.active_layer().name, "last");
    assert_eq!(app.doc.undo_count(), 2);
    assert!(!app.importing && app.import_queue.is_empty() && app.io.is_none() && !app.busy);
    assert!(app.import_failures.is_empty());
    assert_eq!(app.import_count, 0);
    app.importing = true;
    let next = deliver_io(
        &mut app,
        IoMessage::Imported {
            path: PathBuf::from("next.png"),
            result: image_result(Color::BLACK),
        },
    );
    assert!(!next
        .iter()
        .any(|e| matches!(e, Effect::Message { error: true, .. })));
    assert_eq!(app.language.translate(&app.status), "Images added: 1");
    assert_eq!(app.doc.undo_count(), 3);
    app.doc.undo();
    assert_eq!(app.doc.active_layer().name, "last");
}

fn wait_for_io(app: &mut PortfolioApp) -> Vec<Effect> {
    let start = Instant::now();
    let mut effects = Vec::new();
    while app.io.is_some() && start.elapsed() < Duration::from_secs(5) {
        effects.extend(app.event(Event::Tick));
        if app.io.is_some() {
            std::thread::sleep(Duration::from_millis(2));
        }
    }
    assert!(app.io.is_none(), "background operation must complete");
    effects
}

#[test]
fn recovery_waits_for_idle_and_only_a_successful_background_decode_replaces_art() {
    let directory = FixtureDirectory::new();
    let recovery = directory.file("recover.portfolio");
    let mut saved = Document::with_background(20, 12, Color::rgb(20, 90, 110)).unwrap();
    saved.title = "Recovered original".into();
    let saved_bytes = saved.to_bytes().unwrap();
    fs::write(&recovery, &saved_bytes).unwrap();
    let mut app = app_fixture();
    app.recovery = Some(recovery.clone());
    app.recover_available = true;
    app.preserve_recovery = true;
    app.first_tick = true;
    let before = app.doc.to_bytes().unwrap();
    let (manual_tx, manual_rx) = mpsc::channel();
    app.io = Some(manual_rx);
    app.busy = true;
    let no_prompt = |effects: Vec<Effect>| {
        assert!(!effects
            .iter()
            .any(|e| matches!(e, Effect::Confirm { id: 2, .. })))
    };
    no_prompt(app.event(Event::Tick));
    assert!(app.first_tick);
    app.busy = false;
    no_prompt(app.event(Event::Tick));
    app.io = None;
    drop(manual_tx);
    let (auto_tx, auto_rx) = mpsc::channel();
    app.auto_io = Some(auto_rx);
    no_prompt(app.event(Event::Tick));
    assert!(app.first_tick);
    assert!(auto_tx.send((app.document_id, Ok(()))).is_ok());
    no_prompt(app.event(Event::Tick));
    app.drag = Drag::Stroke;
    no_prompt(app.event(Event::Tick));
    app.drag = Drag::None;
    let prompt = app.event(Event::Tick);
    assert!(prompt
        .iter()
        .any(|e| matches!(e, Effect::Confirm { id: 2, .. })));
    assert_eq!(fs::read(&recovery).unwrap(), saved_bytes);
    app.event(Event::Confirmed {
        id: 2,
        answer: Answer::Yes,
    });
    assert!(app.io.is_some() && app.busy);
    assert_eq!(
        app.doc.to_bytes().unwrap(),
        before,
        "no inline read or document replacement"
    );
    let effects = wait_for_io(&mut app);
    assert!(!effects
        .iter()
        .any(|e| matches!(e, Effect::Message { error: true, .. })));
    assert_eq!(app.doc.layers, saved.layers);
    assert_eq!(app.doc.title, saved.title);
    assert!(app.path.is_none());
    assert!(app.dirty());
    assert!(!app.preserve_recovery);

    // A failed recovery leaves current art, history, path and original backup intact.
    fs::write(&recovery, b"broken backup").unwrap();
    edit(&mut app, 5.0);
    app.path = Some(directory.file("current.portfolio"));
    let old_path = app.path.clone();
    let before = app.doc.to_bytes().unwrap();
    let history = (app.doc.undo_count(), app.doc.redo_count());
    app.preserve_recovery = true;
    let (tx, rx) = mpsc::channel();
    app.io = Some(rx);
    app.busy = true;
    app.event(Event::Confirmed {
        id: 2,
        answer: Answer::Yes,
    });
    assert!(app.recovery_requested);
    assert!(tx.send(IoMessage::Exported(Ok(()))).is_ok());
    app.event(Event::Tick);
    assert!(!app.recovery_requested);
    let effects = wait_for_io(&mut app);
    assert!(effects
        .iter()
        .any(|e| matches!(e, Effect::Message { error: true, .. })));
    assert_eq!(app.doc.to_bytes().unwrap(), before);
    assert_eq!(app.path, old_path);
    assert_eq!((app.doc.undo_count(), app.doc.redo_count()), history);
    assert!(app.preserve_recovery);
    assert_eq!(fs::read(&recovery).unwrap(), b"broken backup");
}

#[test]
fn disconnected_workers_release_busy_state_and_unchanged_recovery_is_not_rewritten() {
    let directory = FixtureDirectory::new();
    let mut app = app_fixture();
    edit(&mut app, 7.0);
    let before = app.doc.to_bytes().unwrap();
    let (tx, rx) = mpsc::channel::<IoMessage>();
    app.io = Some(rx);
    app.busy = true;
    app.importing = true;
    app.import_queue.push_back(PathBuf::from("queued.png"));
    drop(tx);
    let effects = app.event(Event::Tick);
    assert!(effects
        .iter()
        .any(|e| matches!(e, Effect::Message { error: true, .. })));
    assert!(!app.busy && app.io.is_none() && !app.importing && app.import_queue.is_empty());
    assert_eq!(app.doc.to_bytes().unwrap(), before);
    app.recovery = Some(directory.file("recovery.portfolio"));
    app.last_auto_saved = Some((app.document_id, app.serial));
    app.last_auto = Instant::now() - Duration::from_secs(60);
    app.last_edit = Instant::now() - Duration::from_secs(60);
    app.event(Event::Tick);
    assert!(app.auto_io.is_none());
    assert!(!app.recovery.as_ref().unwrap().exists());
    let (tx, rx) = mpsc::channel::<(u64, Result<(), String>)>();
    app.auto_io = Some(rx);
    drop(tx);
    app.event(Event::Tick);
    assert!(app.auto_io.is_none());
    assert_eq!(app.doc.to_bytes().unwrap(), before);
}

#[test]
fn extended_brush_scrolling_and_stamp_settings_keep_separate_paint_and_eraser_presets() {
    let mut app = floating_panel_fixture();
    let before = app.doc.to_bytes().unwrap();
    click_action(&mut app, Action::ToolPanel(ToolPanel::Brush));
    browser_wheel(&mut app, ToolPanel::Brush, BrowserRow::Items, -100., false);
    app.render();
    let last_offset = app.browser.view(ToolPanel::Brush).items.offset;
    assert!(last_offset > 0.);
    assert_eq!(last_offset, app.browser.view(ToolPanel::Brush).items.max());
    assert!(!app.controls.iter().any(|c| c.action == Action::Brush(0)));
    click_action(&mut app, Action::Brush(BrushKind::Twin.index() as usize));
    assert_eq!(app.brush.kind, BrushKind::Twin);
    set_tool_number(&mut app, Action::Size, "117");
    set_tool_number(&mut app, Action::Hardness, "45");
    set_tool_number(&mut app, Action::Flow, "35");
    activate_jump(&mut app, Action::BrushDetail(true));
    set_tool_number(&mut app, Action::Spacing, "37");
    set_tool_number(&mut app, Action::Angle, "72");
    set_tool_number(&mut app, Action::Scatter, "28");
    let paint = ToolSettings::from_brush(app.brush);
    app.render();
    let track = app
        .controls
        .iter()
        .find(|c| c.action == Action::Angle && c.kind == ControlKind::Slider)
        .unwrap()
        .hit;
    app.event(Event::MouseDown {
        x: track.x + track.w * 0.85,
        y: track.y + track.h * 0.5,
        button: MouseButton::Left,
        modifiers: Modifiers::default(),
    });
    assert!(matches!(
        app.drag,
        Drag::Slider {
            action: Action::Angle,
            ..
        }
    ));
    assert_ne!(app.brush.angle, paint.angle);
    press_key(&mut app, 27, false);
    assert_eq!(ToolSettings::from_brush(app.brush), paint);
    assert!(app.tool_settings_visible());
    press_key(&mut app, 27, false);
    assert_eq!(app.tool_popup, Some(ToolPanel::Brush));
    assert_eq!(app.browser.view(ToolPanel::Brush).items.offset, last_offset);
    browser_wheel(&mut app, ToolPanel::Brush, BrowserRow::Items, 100., false);
    click_action(&mut app, Action::Brush(0));
    browser_wheel(&mut app, ToolPanel::Brush, BrowserRow::Items, -100., false);
    click_action(&mut app, Action::Brush(BrushKind::Twin.index() as usize));
    assert_eq!(ToolSettings::from_brush(app.brush), paint);
    click_action(&mut app, Action::ToolPanel(ToolPanel::Eraser));
    browser_wheel(&mut app, ToolPanel::Eraser, BrowserRow::Items, -100., false);
    click_action(&mut app, Action::Brush(BrushKind::Twin.index() as usize));
    assert_eq!(app.tool, Tool::Eraser);
    assert_ne!(ToolSettings::from_brush(app.brush), paint);
    set_tool_number(&mut app, Action::Flow, "81");
    let erase = ToolSettings::from_brush(app.brush);
    press_key(&mut app, 27, false);
    click_action(&mut app, Action::ToolPanel(ToolPanel::Brush));
    assert_eq!(ToolSettings::from_brush(app.brush), paint);
    click_action(&mut app, Action::ToolPanel(ToolPanel::Eraser));
    assert_eq!(ToolSettings::from_brush(app.brush), erase);
    press_key(&mut app, 115, false);
    click_action(&mut app, Action::ResetBrush);
    assert_eq!(
        ToolSettings::from_brush(app.brush),
        ToolSettings::preset(BrushKind::Twin, true)
    );
    press_key(&mut app, 27, false);
    click_action(&mut app, Action::ToolPanel(ToolPanel::Brush));
    assert_eq!(ToolSettings::from_brush(app.brush), paint);
    assert_eq!(app.doc.to_bytes().unwrap(), before);
    assert_eq!((app.doc.undo_count(), app.doc.redo_count()), (0, 0));
}

#[test]
fn eraser_has_its_own_catalogue_presets_and_true_erasing_samples() {
    let mut app = floating_panel_fixture();
    let before = app.doc.to_bytes().unwrap();
    let paint_presets = app.presets.entries[0].clone();
    let paint_settings = ToolSettings::from_brush(app.brush);
    let eraser_labels = [
        "Твёрдый",
        "Мягкий",
        "Плоский",
        "Точный",
        "Фактурный",
        "Зернистый",
        "Воздушный",
        "Скошенный",
        "Крапчатый",
        "Широкий",
        "Щетинный",
        "Точечный",
        "Сухой",
        "Двойной",
    ];
    for dark in [true, false] {
        app.dark = dark;
        for family in [ToolPanel::Brush, ToolPanel::Eraser] {
            click_action(&mut app, Action::ToolPanel(family));
            let ids = app.browser.filtered_ids(family, app.language);
            assert_eq!(ids.len(), BrushKind::ALL.len());
            for (index, id) in ids.into_iter().enumerate() {
                assert_eq!(
                    id,
                    if family == ToolPanel::Eraser {
                        ItemId::Eraser(BrushKind::from_index(index as u8))
                    } else {
                        ItemId::Brush(BrushKind::from_index(index as u8))
                    }
                );
                if family == ToolPanel::Eraser {
                    assert_eq!(id.label(), eraser_labels[index]);
                }
            }
            for delta in [100., -4., -4., -4., -100.] {
                browser_wheel(&mut app, family, BrowserRow::Items, delta, false);
                app.render();
                for c in app.controls.iter().filter(|c| c.enabled) {
                    if let Action::Brush(index) = c.action {
                        let item = if family == ToolPanel::Eraser {
                            ItemId::Eraser(BrushKind::from_index(index as u8))
                        } else {
                            ItemId::Brush(BrushKind::from_index(index as u8))
                        };
                        assert_eq!(c.label, item.label());
                    }
                }
            }
            for index in 0..BrushKind::ALL.len() {
                let key = (family == ToolPanel::Eraser, index, dark);
                let sample = &app
                    .brush_samples
                    .get(&key)
                    .expect("every scrolled catalogue sample was drawn")
                    .1;
                assert_eq!(sample.len(), 112 * 40);
                if family == ToolPanel::Eraser {
                    assert_eq!(
                        sample[0].a, 255,
                        "an eraser sample starts on an opaque painted patch"
                    );
                    assert!(
                        sample.iter().any(|p| p.a < 255),
                        "the sample actually removes alpha"
                    );
                } else {
                    assert_eq!(sample[0].a, 0, "paint samples start on a transparent layer");
                    assert!(sample.iter().any(|p| p.a > 0));
                }
            }
        }
    }
    browser_query(&mut app, "точный");
    click_action(&mut app, Action::Brush(3));
    assert_eq!(app.tool, Tool::Eraser);
    set_tool_number(&mut app, Action::Size, "19");
    set_tool_number(&mut app, Action::Opacity, "23");
    set_tool_number(&mut app, Action::Flow, "41");
    let erase_settings = ToolSettings::from_brush(app.brush);
    click_action(&mut app, Action::CloseToolProperties);
    click_action(&mut app, Action::ToolPanel(ToolPanel::Brush));
    assert_eq!(app.presets.entries[0], paint_presets);
    assert_eq!(ToolSettings::from_brush(app.brush), paint_settings);
    click_action(&mut app, Action::ToolPanel(ToolPanel::Eraser));
    assert_eq!(ToolSettings::from_brush(app.brush), erase_settings);
    press_key(&mut app, 115, false);
    click_action(&mut app, Action::ResetBrush);
    assert_eq!(
        ToolSettings::from_brush(app.brush),
        ToolSettings::preset(BrushKind::Pencil, true)
    );
    assert_eq!(app.presets.entries[0], paint_presets);

    // Equal settings and theme still need distinct preview cache entries for paint and erase.
    let common = ToolSettings {
        kind: BrushKind::Ink,
        texture_scale: 1.,
        opacity_jitter: 0.,
        size: 14.,
        roundness: 1.,
        grain: 0.,
        opacity: 0.45,
        hardness: 0.8,
        flow: 0.7,
        spacing: 0.12,
        angle: 0.,
        scatter: 0.,
    };
    click_action(&mut app, Action::ToolPanel(ToolPanel::Brush));
    common.apply(&mut app.brush);
    click_action(&mut app, Action::ToolSection(2));
    app.render();
    let painted = app.brush_preview.as_ref().unwrap().clone();
    assert_eq!(painted.0, common);
    assert!(!painted.2);
    assert_eq!(painted.3[0].a, 0);
    click_action(&mut app, Action::ToolPanel(ToolPanel::Eraser));
    common.apply(&mut app.brush);
    click_action(&mut app, Action::ToolSection(2));
    app.render();
    let erased = app.brush_preview.as_ref().unwrap();
    assert_eq!(erased.0, common);
    assert_eq!(painted.1, rgb(app.brush.color));
    assert!(erased.2);
    assert_eq!(erased.3[0].a, 255);
    assert!(erased.3.iter().any(|p| p.a < 255));
    assert_ne!(erased.3, painted.3);
    let mut zero = common;
    zero.opacity = 0.;
    assert!(PortfolioApp::tool_sample(zero, painted.1, true)
        .iter()
        .all(|p| p.a == 255));
    assert!(PortfolioApp::tool_sample(zero, painted.1, false)
        .iter()
        .all(|p| p.a == 0));
    assert_eq!(app.doc.to_bytes().unwrap(), before);
    assert_eq!((app.doc.undo_count(), app.doc.redo_count()), (0, 0));
}

#[test]
fn contextual_gears_live_below_chooser_pin_and_keep_the_active_family() {
    let mut app = floating_panel_fixture();
    app.resize(884, 581, 1.0);
    let before = app.doc.to_bytes().unwrap();
    for family in [ToolPanel::Brush, ToolPanel::Eraser, ToolPanel::Shapes] {
        click_action(&mut app, Action::ToolPanel(family));
        app.render();
        let popup = app.tool_popup_rect().unwrap();
        let close = app
            .controls
            .iter()
            .find(|c| c.action == Action::PinToolPanel)
            .unwrap()
            .hit;
        let gear = app
            .controls
            .iter()
            .find(|c| c.action == Action::ToolProperties)
            .unwrap()
            .hit;
        assert!(gear.y >= close.y + close.h);
        assert_eq!(gear.x, close.x);
        let search = app.browser_search_button_rect(family);
        assert_eq!(gear.y + gear.h / 2., search.y + search.h / 2.);
        assert!(search.x + search.w <= gear.x);
        assert!(popup.contains(gear.x + gear.w * 0.5, gear.y + gear.h * 0.5));
        assert!(!app.top_tools_rect().contains(gear.x, gear.y));
        click_action(&mut app, Action::ToolProperties);
        assert!(app.tool_settings_visible());
        assert_eq!(ToolPanel::for_tool(app.tool), Some(family));
        press_key(&mut app, 27, false);
        app.render();
        assert_eq!(app.tool_popup, Some(family));
        assert_eq!(focused_action(&app), Some(Action::ToolProperties));
        assert_eq!(app.doc.to_bytes().unwrap(), before);
    }
}

#[test]
fn selection_dropdowns_commit_only_on_activation_and_consume_outside_clicks() {
    let directory = FixtureDirectory::new();
    let mut app = floating_panel_fixture();
    app.resize(884, 581, 1.0);
    app.font_size = 17;
    app.prefs = Some(directory.file("dropdowns.txt"));
    let before = app.doc.to_bytes().unwrap();
    press_key(&mut app, 188, true);
    for (choice, tab) in [(SettingsChoice::Font, 1), (SettingsChoice::Language, 2)] {
        activate_jump(&mut app, Action::SettingsSection(tab));
        let original = (app.ui_font, app.language);
        click_action(&mut app, choice.trigger());
        assert_eq!(app.settings_choice, Some(choice));
        app.render();
        let bounds = app.choice_menu_rect(choice);
        assert!(bounds.y >= 0. && bounds.y + bounds.h <= app.h());
        assert_eq!(
            app.controls.iter().filter(|c| c.enabled).count(),
            choice.len()
        );
        assert!(app
            .controls
            .iter()
            .filter(|c| c.enabled)
            .all(|c| c.hit.h >= 28.));
        let row_height = match choice {
            SettingsChoice::Font => 28.0,
            SettingsChoice::Language => 32.0,
        };
        assert!(app
            .controls
            .iter()
            .filter(|c| c.enabled)
            .all(|c| c.rect.h == row_height));
        press_key(&mut app, 35, false); // End highlights the last item without a commit.
        app.render();
        assert_eq!((app.ui_font, app.language), original);
        assert_eq!(focused_action(&app), Some(choice.action(choice.len() - 1)));
        press_key(&mut app, 27, false);
        app.render();
        assert_eq!(app.settings_choice, None);
        assert_eq!(app.overlay, Overlay::Settings);
        assert_eq!(focused_action(&app), Some(choice.trigger()));
        assert_eq!((app.ui_font, app.language), original);
        press_key(&mut app, 13, false); // Reopen with keyboard.
        app.render();
        assert_eq!(app.settings_choice, Some(choice));
        press_key(&mut app, 35, false);
        press_key(&mut app, 13, false);
        app.render();
        assert_eq!(app.settings_choice, None);
        assert_eq!(focused_action(&app), Some(choice.trigger()));
        match choice {
            SettingsChoice::Font => assert_eq!(app.ui_font, *UiFont::ALL.last().unwrap()),
            SettingsChoice::Language => assert_eq!(app.language, *Language::ALL.last().unwrap()),
        }
        let committed = (app.ui_font, app.language);
        click_action(&mut app, choice.trigger());
        press_key(&mut app, 36, false);
        click_point(&mut app, 250., 420.);
        assert_eq!(app.settings_choice, None);
        assert_eq!(
            app.overlay,
            Overlay::Settings,
            "outside click closes only the list"
        );
        assert_eq!((app.ui_font, app.language), committed);
        assert_eq!(app.doc.to_bytes().unwrap(), before);
        app.render();
        click_action(&mut app, choice.trigger());
        app.event(Event::FocusLost);
        assert_eq!(app.settings_choice, None);
        assert_eq!((app.ui_font, app.language), committed);
        assert_eq!(app.doc.to_bytes().unwrap(), before);
    }
    assert_eq!((app.doc.undo_count(), app.doc.redo_count()), (0, 0));
    let persisted = Appearance::decode(&fs::read_to_string(app.prefs.as_ref().unwrap()).unwrap());
    assert_eq!(persisted.font, app.ui_font);
    assert_eq!(persisted.language, app.language);
}

#[test]
fn cached_canvas_matches_fresh_pixels_after_edits_and_view_changes() {
    fn same_as_fresh(app: &mut PortfolioApp) {
        let cached = app.render().pixels;
        app.canvas_raster = None;
        let fresh = app.render().pixels;
        assert_eq!(cached, fresh);
    }
    let mut app = app_fixture();
    app.resize(884, 581, 1.0);
    same_as_fresh(&mut app);
    app.event(Event::MouseMove {
        x: 180.,
        y: 340.,
        modifiers: Modifiers::default(),
    });
    same_as_fresh(&mut app);
    edit(&mut app, 10.);
    same_as_fresh(&mut app);
    let mut out = Vec::new();
    for action in [
        Action::Undo,
        Action::Redo,
        Action::ZoomIn,
        Action::Theme,
        Action::Value,
        Action::Grid,
        Action::Mirror,
    ] {
        app.activate(action, &mut out);
        same_as_fresh(&mut app);
    }
    app.view_x += 3.25;
    app.view_y -= 4.5;
    same_as_fresh(&mut app);
    app.resize(1105, 727, 1.25);
    same_as_fresh(&mut app);
    let (x, y) = (10., 10.);
    app.doc
        .begin_stroke(
            x,
            y,
            Brush {
                size: 3.,
                ..Brush::default()
            },
        )
        .unwrap();
    app.doc.continue_stroke(20., 16.);
    same_as_fresh(&mut app);
    app.doc.end_stroke();
    same_as_fresh(&mut app);
    app.activate(Action::Tool(Tool::Fill), &mut out);
    app.render();
    let canvas = app.canvas();
    let (x, y) = (canvas.x + canvas.w * 0.5, canvas.y + canvas.h * 0.5);
    assert!(!app.canvas_obscured(x, y));
    let before_fill = app.doc.to_bytes().unwrap();
    click_point(&mut app, x, y);
    assert_ne!(app.doc.to_bytes().unwrap(), before_fill);
    assert!(!app.composite_dirty, "fill uses its exact paint damage");
    let regional = app.render().pixels;
    app.composite_dirty = true;
    let full = app.render().pixels;
    assert_eq!(
        regional, full,
        "regional fill display matches a full composition"
    );
}

#[test]
fn closing_font_list_blocks_double_clicks_but_restores_keyboard_focus_immediately() {
    let mut app = floating_panel_fixture();
    app.motion_runtime = true;
    app.system_reduced_motion = false;
    app.animations = true;
    press_key(&mut app, 188, true);
    app.overlay_motion.snap(true);
    activate_jump(&mut app, Action::SettingsSection(1));
    click_action(&mut app, Action::FontMenu);
    app.choice_motion.snap(true);
    app.render();
    let row = app
        .controls
        .iter()
        .find(|c| c.action == Action::SetFont(UiFont::ALL[1]))
        .unwrap()
        .hit;
    let (x, y) = (row.x + row.w * 0.5, row.y + row.h * 0.5);
    let initial_size = app.font_size;
    click_point(&mut app, x, y);
    assert_eq!(app.settings_choice, None);
    assert_eq!(app.closing_choice, Some(SettingsChoice::Font));
    app.motion_preview = Some(0.9);
    app.render();
    assert_eq!(focused_action(&app), Some(Action::FontMenu));
    click_point(&mut app, x, y);
    assert_eq!(
        app.font_size, initial_size,
        "the fading list must block the slider below it"
    );
    assert!(matches!(app.drag, Drag::None));
    app.render();
    press_key(&mut app, 115, false);
    assert_eq!(app.overlay, Overlay::Settings);
    assert_eq!(app.settings_choice, Some(SettingsChoice::Font));
    app.choice_motion.snap(true);
    app.motion_preview = None;
    app.render();
    press_key(&mut app, 13, false);
    app.render();
    assert_eq!(focused_action(&app), Some(Action::FontMenu));
    press_key(&mut app, 115, false);
    assert_eq!(app.overlay, Overlay::Settings);
    assert_eq!(app.settings_choice, Some(SettingsChoice::Font));
    assert_eq!(app.font_size, initial_size);
}

#[test]
fn quick_sliders_are_centered_live_cancellable_and_preserve_the_document() {
    let mut app = floating_panel_fixture();
    edit(&mut app, 10.0);
    edit(&mut app, 20.0);
    app.doc.undo();
    app.changed();
    let before = app.doc.to_bytes().unwrap();
    let history = (
        app.doc.undo_count(),
        app.doc.redo_count(),
        app.doc.is_dirty(),
    );
    let serial = app.serial;
    let mut effects = Vec::new();
    for tool in [
        Tool::Brush,
        Tool::Eraser,
        Tool::Line,
        Tool::Rectangle,
        Tool::Ellipse,
        Tool::Fill,
    ] {
        app.activate(Action::Tool(tool), &mut effects);
        for (width, height) in [(1440, 940), (884, 581)] {
            app.resize(width, height, 1.0);
            app.render();
            assert!(app.quick_controls_visible(), "{tool:?}");
            let bar = app.quick_controls_rect();
            let toolbar = app.top_tools_rect();
            assert!((bar.x + bar.w * 0.5 - width as f32 * 0.5).abs() < 0.01);
            assert!(bar.y >= toolbar.y + toolbar.h);
            assert!(bar.w < width as f32 - 32.0);
            assert!(bar.x >= 0.0 && bar.x + bar.w <= width as f32);
            for action in [Action::Size, Action::Opacity] {
                let slider = app
                    .controls
                    .iter()
                    .find(|c| c.action == action && c.kind == ControlKind::Slider)
                    .expect("the quick bar exposes both slider positions");
                assert_eq!(
                    slider.enabled,
                    tool != Tool::Fill || action == Action::Opacity
                );
                assert!(bar.contains(
                    slider.hit.x + slider.hit.w * 0.5,
                    slider.hit.y + slider.hit.h * 0.5
                ));
            }
        }
        for action in [Action::Size, Action::Opacity] {
            app.render();
            let slider = app
                .controls
                .iter()
                .find(|c| c.action == action && c.kind == ControlKind::Slider)
                .unwrap()
                .clone();
            let initial = app.parameter_value(action);
            let above_middle = initial > if action == Action::Size { 256.0 } else { 50.0 };
            let (first_fraction, next_fraction) = if above_middle {
                (0.24, 0.39)
            } else {
                (0.76, 0.61)
            };
            let (x, y) = (
                slider.rect.x + slider.rect.w * first_fraction,
                slider.rect.y + slider.rect.h * 0.5,
            );
            app.event(Event::MouseDown {
                x,
                y,
                button: MouseButton::Left,
                modifiers: Modifiers::default(),
            });
            if !slider.enabled {
                assert_eq!(app.parameter_value(action), initial);
                assert!(matches!(app.drag, Drag::None));
                app.event(Event::MouseUp {
                    x,
                    y,
                    button: MouseButton::Left,
                    modifiers: Modifiers::default(),
                });
                continue;
            }
            assert_ne!(app.parameter_value(action), initial);
            assert!(matches!(app.drag, Drag::Slider { action: active, .. } if active == action));
            let first = app.parameter_value(action);
            let moved_x = slider.rect.x + slider.rect.w * next_fraction;
            app.event(Event::MouseMove {
                x: moved_x,
                y,
                modifiers: Modifiers::default(),
            });
            assert_ne!(
                app.parameter_value(action),
                first,
                "the value changes before mouse-up"
            );
            assert_eq!(app.doc.to_bytes().unwrap(), before);
            press_key(&mut app, 27, false);
            assert_eq!(app.parameter_value(action), initial);
            assert!(matches!(app.drag, Drag::None));
            assert_eq!(app.overlay, Overlay::None);
            app.event(Event::MouseUp {
                x: moved_x,
                y,
                button: MouseButton::Left,
                modifiers: Modifiers::default(),
            });
            assert_eq!(
                app.parameter_value(action),
                initial,
                "late release does not restore a cancelled value"
            );
            app.render();
            click_point(&mut app, moved_x, y);
            assert_ne!(app.parameter_value(action), initial);
            assert!(matches!(app.drag, Drag::None));
        }
        assert_eq!(app.doc.to_bytes().unwrap(), before);
        assert_eq!(
            (
                app.doc.undo_count(),
                app.doc.redo_count(),
                app.doc.is_dirty()
            ),
            history
        );
        assert_eq!(app.serial, serial);
    }
    for tool in [Tool::Hand, Tool::Picker] {
        app.activate(Action::Tool(tool), &mut effects);
        app.render();
        assert!(!app.quick_controls_visible());
        assert!(!app
            .controls
            .iter()
            .any(|c| matches!(c.action, Action::Size | Action::Opacity)));
    }
}

#[test]
fn quick_sliders_hide_for_unpinned_chooser_but_coexist_with_properties_and_ghosts_block_wheel() {
    fn wheel(app: &mut PortfolioApp, x: f32, y: f32, ctrl: bool) {
        app.event(Event::Wheel {
            x,
            y,
            delta: 1.0,
            modifiers: Modifiers {
                ctrl,
                ..Modifiers::default()
            },
        });
    }
    let mut app = floating_panel_fixture();
    app.motion_runtime = true;
    app.animations = true;
    app.motion_preview = Some(1.0);
    let before = app.doc.to_bytes().unwrap();
    let view = (app.view_x, app.view_y, app.zoom);
    let size = app.brush.size;
    assert!(app.quick_controls_visible());
    click_action(&mut app, Action::ToolPanel(ToolPanel::Brush));
    app.popup_motion.snap(true);
    app.render();
    let chooser = app.tool_popup_rect().unwrap();
    assert!(!app.quick_controls_visible());
    assert!(!app
        .controls
        .iter()
        .any(|c| matches!(c.action, Action::Size | Action::Opacity)));
    press_key(&mut app, 27, false);
    assert_eq!(app.tool_popup, None);
    assert_eq!(app.closing_popup, Some(ToolPanel::Brush));
    app.motion_preview = Some(0.8);
    app.render();
    assert!(!app.quick_controls_visible());
    assert!(!app
        .controls
        .iter()
        .any(|c| matches!(c.action, Action::Size | Action::Opacity)));
    for ctrl in [false, true] {
        // Keep the real animation clock in its closing phase while preview
        // fixes the visible region, independent of test-runner scheduling.
        app.popup_motion.snap(true);
        app.popup_motion.set(false);
        wheel(&mut app, chooser.x + 2.0, chooser.y + 2.0, ctrl);
        assert_eq!((app.view_x, app.view_y, app.zoom), view);
        assert_eq!(app.brush.size, size);
    }
    app.event(Event::ReducedMotion(true));
    app.render();
    assert!(app.closing_popup.is_none());
    assert!(app.quick_controls_visible());
    press_key(&mut app, 115, false);
    app.render();
    assert_eq!(app.overlay, Overlay::ToolProperties);
    assert!(app.quick_controls_visible());
    let properties = app.tool_settings_rect();
    for action in [Action::Size, Action::Opacity] {
        let quick = app.quick_control_rect(action);
        assert!(app.controls.iter().any(|c| c.action == action
            && c.kind == ControlKind::Slider
            && properties.contains(c.hit.x + c.hit.w * 0.5, c.hit.y + c.hit.h * 0.5)));
        assert!(app.controls.iter().any(|c| c.action == action
            && c.kind == ControlKind::Slider
            && quick.contains(c.hit.x + c.hit.w * 0.5, c.hit.y + c.hit.h * 0.5)));
    }
    app.event(Event::ReducedMotion(false));
    app.overlay_motion.snap(true);
    press_key(&mut app, 27, false);
    app.render();
    assert_eq!(
        app.tool_popup, None,
        "properties opened from the canvas do not invent a chooser"
    );
    assert_eq!(app.closing_overlay, Some(Overlay::ToolProperties));
    assert!(app.quick_controls_visible());
    for ctrl in [false, true] {
        app.overlay_motion.snap(true);
        app.overlay_motion.set(false);
        wheel(&mut app, properties.x + 2.0, properties.y + 2.0, ctrl);
        assert_eq!((app.view_x, app.view_y, app.zoom), view);
        assert_eq!(app.brush.size, size);
    }
    app.event(Event::ReducedMotion(true));
    app.render();
    assert!(app.closing_overlay.is_none());
    assert!(app.quick_controls_visible());
    assert!(!app.canvas_obscured(850.0, 700.0));
    wheel(&mut app, 850.0, 700.0, false);
    assert_ne!(
        app.zoom, view.2,
        "after settling, the free canvas accepts wheel zoom"
    );
    wheel(&mut app, 850.0, 700.0, true);
    assert_ne!(
        app.brush.size, size,
        "after settling, Ctrl+wheel changes brush size"
    );
    assert_eq!(app.doc.to_bytes().unwrap(), before);
    assert_eq!((app.doc.undo_count(), app.doc.redo_count()), (0, 0));
}

#[test]
fn properties_coexist_with_the_chooser_and_preserve_its_view_when_explicitly_closed() {
    for family in [ToolPanel::Brush, ToolPanel::Eraser, ToolPanel::Shapes] {
        for close in 0..3 {
            let mut app = floating_panel_fixture();
            let before = app.doc.to_bytes().unwrap();
            click_action(&mut app, Action::ToolPanel(family));
            browser_wheel(&mut app, family, BrowserRow::Categories, -100., false);
            if close == 0 {
                // Search without selecting an option, then view the far end.
                // The selected Ink/shape may be outside the visible viewport.
                browser_query(
                    &mut app,
                    if family == ToolPanel::Shapes {
                        "l"
                    } else {
                        "a"
                    },
                );
                browser_wheel(&mut app, family, BrowserRow::Items, -100., false);
                assert!(app.browser.view(family).items.offset > 0.);
            } else if close == 1 {
                let category = if family == ToolPanel::Shapes {
                    Category::Angular
                } else {
                    Category::Sketch
                };
                click_action(&mut app, Action::BrowserCategory(category));
                browser_query(
                    &mut app,
                    if family == ToolPanel::Shapes {
                        "rect"
                    } else {
                        "ch"
                    },
                );
                assert_eq!(app.browser.view(family).category, category);
            } else {
                browser_wheel(&mut app, family, BrowserRow::Items, -100., false);
            }
            app.render();
            let viewed = app.browser.view(family).clone();
            let visible_options: Vec<_> = app
                .controls
                .iter()
                .filter(|c| c.enabled && matches!(c.action, Action::Brush(_) | Action::Shape(_)))
                .map(|c| c.action)
                .collect();
            if close == 1 {
                press_key(&mut app, 115, false);
            } else {
                click_action(&mut app, Action::ToolProperties);
            }
            assert_eq!(app.overlay, Overlay::ToolProperties);
            assert_eq!(app.tool_popup, Some(family));
            assert_eq!(app.browser.view(family), &viewed);
            let settings = ToolSettings::from_brush(app.brush);
            match close {
                0 => {
                    click_action(&mut app, Action::CloseToolProperties);
                }
                1 => {
                    press_key(&mut app, 115, false);
                }
                _ => {
                    press_key(&mut app, 27, false);
                }
            }
            app.render();
            assert_eq!(app.overlay, Overlay::None);
            assert_eq!(app.tool_popup, Some(family));
            assert_eq!(app.browser.view(family), &viewed);
            assert_eq!(focused_action(&app), Some(Action::ToolProperties));
            assert_eq!(ToolSettings::from_brush(app.brush), settings);
            assert_eq!(app.doc.to_bytes().unwrap(), before);
            assert_eq!((app.doc.undo_count(), app.doc.redo_count()), (0, 0));
            assert_eq!(app.controls.iter().filter(|c| c.enabled && matches!(c.action, Action::Brush(_) | Action::Shape(_))).map(|c| c.action).collect::<Vec<_>>(), visible_options);
            if viewed.search_open {
                press_key(&mut app, 27, false);
                assert_eq!(app.tool_popup, Some(family));
                assert!(!app.browser.view(family).search_open);
            }
            press_key(&mut app, 27, false);
            app.render();
            assert_eq!(app.tool_popup, None);
            assert_eq!(focused_action(&app), Some(Action::ToolPanel(family)));
            assert!(app.quick_controls_visible());
            assert_eq!(app.doc.to_bytes().unwrap(), before);
        }
    }
}

#[test]
fn selecting_a_nonproperty_tool_or_opening_settings_does_not_resurrect_an_old_chooser() {
    for open_settings in [false, true] {
        let mut app = floating_panel_fixture();
        let before = app.doc.to_bytes().unwrap();
        click_action(&mut app, Action::ToolPanel(ToolPanel::Brush));
        browser_wheel(&mut app, ToolPanel::Brush, BrowserRow::Items, -100., false);
        click_action(&mut app, Action::ToolProperties);
        if open_settings {
            press_key(&mut app, 188, true);
            assert_eq!(app.overlay, Overlay::Settings);
            press_key(&mut app, 27, false);
        } else {
            press_key(&mut app, 72, false); // Hand has no tool properties.
            assert_eq!(app.tool, Tool::Hand);
            assert!(!app.tool_settings_visible());
            press_key(&mut app, 66, false);
        }
        app.render();
        assert_eq!(app.overlay, Overlay::None);
        assert_eq!(app.tool_popup, None);
        assert!(app.quick_controls_visible());
        press_key(&mut app, 115, false);
        assert_eq!(app.overlay, Overlay::ToolProperties);
        press_key(&mut app, 27, false);
        app.render();
        assert_eq!(app.overlay, Overlay::None);
        assert_eq!(
            app.tool_popup, None,
            "the old brush chooser must not reappear"
        );
        assert!(app.quick_controls_visible());
        assert_eq!(app.doc.to_bytes().unwrap(), before);
        assert_eq!((app.doc.undo_count(), app.doc.redo_count()), (0, 0));
    }
}

#[test]
fn settings_sections_stay_expanded_and_search_filters_a_scrollable_compact_panel() {
    for (width, height, font_size) in [(1440, 940, 13), (884, 581, 17)] {
        let mut app = floating_panel_fixture();
        app.resize(width, height, 1.0);
        app.font_size = font_size;
        let before = app.doc.to_bytes().unwrap();
        let view = (app.view_x, app.view_y, app.zoom);
        let size = app.brush.size;
        let history = (
            app.doc.undo_count(),
            app.doc.redo_count(),
            app.doc.is_dirty(),
        );
        press_key(&mut app, 188, true);
        app.render();
        let panel = app.settings_rect();
        assert_eq!(panel.w, 400.);
        assert!(panel.y >= 64. && panel.y + panel.h <= app.h());
        assert_eq!(app.settings_ui.query.text(), "");
        assert!(!app
            .controls
            .iter()
            .any(|c| matches!(c.action, Action::SettingsSection(_))));
        for section in 0..3 {
            let header = app.settings_section_rect(section);
            assert!(
                header.h > 0.,
                "every section belongs to the unfiltered vertical layout"
            );
            if section > 0 {
                let previous = app.settings_section_rect(section - 1);
                assert!(header.y > previous.y + previous.h);
            }
        }
        let reset = app
            .controls
            .iter()
            .find(|c| c.action == Action::ResetAppearance)
            .unwrap()
            .hit;
        for (section, action) in [
            (0, Action::SetTheme(true)),
            (1, Action::FontMenu),
            (2, Action::LanguageMenu),
        ] {
            activate_jump(&mut app, Action::SettingsSection(section));
            assert!(app.controls.iter().any(|c| c.enabled && c.action == action));
            let offset = app.settings_ui.scroll.offset;
            activate_jump(&mut app, Action::SettingsSection(section));
            assert_eq!(app.settings_ui.scroll.offset, offset);
            assert_eq!(
                app.settings_rect().h,
                panel.h,
                "revealing a section never collapses another"
            );
            assert!(app
                .controls
                .iter()
                .filter(|c| c.enabled)
                .all(|c| c.hit.x >= panel.x
                    && c.hit.x + c.hit.w <= panel.x + panel.w
                    && c.hit.y >= panel.y
                    && c.hit.y + c.hit.h <= panel.y + panel.h));
        }
        let body = app.settings_viewport();
        app.event(Event::Wheel {
            x: body.x + body.w * 0.5,
            y: body.y + body.h * 0.5,
            delta: -100.,
            modifiers: Modifiers {
                ctrl: true,
                ..Modifiers::default()
            },
        });
        app.render();
        assert_eq!(app.settings_ui.scroll.offset, app.settings_ui.scroll.max());
        let fixed_reset = app
            .controls
            .iter()
            .find(|c| c.action == Action::ResetAppearance)
            .unwrap()
            .hit;
        assert_eq!(
            (reset.x, reset.y, reset.w, reset.h),
            (fixed_reset.x, fixed_reset.y, fixed_reset.w, fixed_reset.h)
        );
        assert_eq!((app.view_x, app.view_y, app.zoom), view);
        assert_eq!(app.brush.size, size);

        click_action(&mut app, Action::SettingsSearch);
        app.event(Event::Text("font size".into()));
        app.render();
        assert!(app
            .controls
            .iter()
            .any(|c| c.enabled && c.action == Action::FontSize));
        assert!(!app.controls.iter().any(|c| matches!(
            c.action,
            Action::FontMenu | Action::LanguageMenu | Action::SetTheme(_)
        )));
        click_action(&mut app, Action::ClearSettingsSearch);
        app.render();
        assert_eq!(app.settings_ui.query.text(), "");
        assert_eq!(app.settings_ui.scroll.offset, 0.);
        assert!(app.settings_field_visible(SettingsChoice::Font));
        assert!(app.settings_field_visible(SettingsChoice::Language));
        assert_eq!(app.doc.to_bytes().unwrap(), before);
        assert_eq!(
            (
                app.doc.undo_count(),
                app.doc.redo_count(),
                app.doc.is_dirty()
            ),
            history
        );
    }
}

fn begin_quick_drag(app: &mut PortfolioApp, action: Action, fraction: f32) -> (f32, f32) {
    app.render();
    let track = app.quick_slider_rect(action);
    let (x, y) = (track.x + track.w * fraction, track.y + track.h * 0.5);
    app.event(Event::MouseDown {
        x,
        y,
        button: MouseButton::Left,
        modifiers: Modifiers::default(),
    });
    (x, y)
}

fn frame_pixel(frame: &Frame, x: f32, y: f32) -> u32 {
    frame.pixels[y.floor() as usize * frame.width as usize + x.floor() as usize]
}

#[test]
fn quick_cards_have_separate_surfaces_and_unlabelled_values_to_the_right_of_tracks() {
    for (width, height, font_size) in [
        (1440, 940, 13),
        (1440, 940, 17),
        (884, 581, 13),
        (884, 581, 17),
    ] {
        let mut app = floating_panel_fixture();
        app.resize(width, height, 1.0);
        app.font_size = font_size;
        let before = app.doc.to_bytes().unwrap();
        let frame = app.render();
        let size_card = app.quick_control_rect(Action::Size);
        let opacity_card = app.quick_control_rect(Action::Opacity);
        assert_eq!(size_card.y, opacity_card.y);
        assert_eq!(size_card.h, opacity_card.h);
        assert!(opacity_card.x > size_card.x + size_card.w);
        assert_eq!(opacity_card.x - size_card.x - size_card.w, 12.0);
        assert!(!size_card.intersects(opacity_card));
        let gap = (
            size_card.x + size_card.w + 6.0,
            size_card.y + size_card.h * 0.5,
        );
        assert!(
            !app.canvas_obscured(gap.0, gap.1),
            "the gap between independent cards remains canvas"
        );
        assert_ne!(
            frame_pixel(&frame, gap.0, gap.1),
            Theme::with_accent(app.dark, app.accent).surface
        );
        for (action, card) in [(Action::Size, size_card), (Action::Opacity, opacity_card)] {
            let surface = Theme::with_accent(app.dark, app.accent).surface;
            // These pixels were inside the old 4 px corner. The larger radius
            // must reveal the artwork/shadow there, while the straight top is filled.
            for (x, y) in [
                (card.x + 2.0, card.y + 2.0),
                (card.x + card.w - 3.0, card.y + 2.0),
                (card.x + 2.0, card.y + card.h - 3.0),
                (card.x + card.w - 3.0, card.y + card.h - 3.0),
            ] {
                assert_ne!(frame_pixel(&frame, x, y), surface);
            }
            assert_eq!(
                frame_pixel(&frame, card.x + card.w * 0.5, card.y + 2.0),
                surface
            );
            let slider = app
                .controls
                .iter()
                .find(|c| c.action == action && c.kind == ControlKind::Slider)
                .unwrap();
            let value = app
                .controls
                .iter()
                .find(|c| c.action == action && c.kind == ControlKind::Value)
                .unwrap();
            assert!(slider.enabled && value.enabled);
            assert!(value.hit.x >= slider.hit.x + slider.hit.w);
            assert!(!value.hit.intersects(slider.hit));
            for control in [slider, value] {
                assert!(
                    control.hit.x >= card.x && control.hit.x + control.hit.w <= card.x + card.w
                );
                assert!(
                    control.hit.y >= card.y && control.hit.y + control.hit.h <= card.y + card.h
                );
            }
            let track = app.quick_slider_rect(action);
            assert!((value.hit.y + value.hit.h * 0.5 - track.y - track.h * 0.5).abs() < 0.01);
            let expected = if action == Action::Size {
                format!("{:.0}", app.brush.size)
            } else {
                format!("{:.0}%", app.brush.opacity * 100.0)
            };
            let texts: Vec<_> = frame
                .texts
                .iter()
                .filter(|run| card.contains(run.x + run.width * 0.5, run.y + run.height * 0.5))
                .collect();
            assert_eq!(texts.len(), 1, "the card contains only its numeric readout");
            assert_eq!(texts[0].text, expected);
            assert!(texts[0].x >= track.x + track.w);
            let effects = click_action(&mut app, action);
            let prompt_id = effects
                .into_iter()
                .find_map(|effect| match effect {
                    Effect::Prompt { id, .. } => Some(id),
                    _ => None,
                })
                .expect("the tall numeric control opens a prompt, not a drag");
            assert!(matches!(app.drag, Drag::None));
            assert_eq!(app.quick_preview_action(), None);
            app.event(Event::Prompted {
                id: prompt_id,
                value: None,
            });
            assert_eq!(app.doc.to_bytes().unwrap(), before);
        }
        assert_eq!((app.doc.undo_count(), app.doc.redo_count()), (0, 0));
    }
}

#[test]
fn quick_opacity_preview_renders_live_color_and_disappears_on_release() {
    let mut app = floating_panel_fixture();
    app.brush.color = Color::rgb(208, 42, 73);
    let before = app.doc.to_bytes().unwrap();
    let serial = app.serial;
    let baseline = app.render();
    let theme = Theme::with_accent(app.dark, app.accent);
    let (_, y) = begin_quick_drag(&mut app, Action::Opacity, 0.0);
    assert_eq!(app.brush.opacity, 0.0);
    assert_eq!(app.quick_preview_action(), Some(Action::Opacity));
    let opacity_rect = app.quick_preview_rect(Action::Opacity);
    let (cx, cy) = (
        opacity_rect.x + opacity_rect.w * 0.5,
        opacity_rect.y + opacity_rect.h * 0.5,
    );
    let radius = app.quick_preview_radius(Action::Opacity);
    let zero = app.render();
    let track = app.quick_slider_rect(Action::Opacity);
    app.event(Event::MouseMove {
        x: track.x + track.w * 0.5,
        y,
        modifiers: Modifiers::default(),
    });
    assert_eq!(app.brush.opacity, 0.5);
    let half = app.render();
    app.event(Event::MouseMove {
        x: track.x + track.w,
        y,
        modifiers: Modifiers::default(),
    });
    assert_eq!(app.brush.opacity, 1.0);
    assert_eq!(app.quick_preview_radius(Action::Opacity), radius);
    let full = app.render();
    let (zero_center, half_center, full_center) = (
        frame_pixel(&zero, cx, cy),
        frame_pixel(&half, cx, cy),
        frame_pixel(&full, cx, cy),
    );
    assert_eq!(full_center, 0xd02a49);
    assert_ne!(zero_center, full_center);
    for shift in [0, 8, 16] {
        let expected = (((zero_center >> shift) & 255) + ((full_center >> shift) & 255)) / 2;
        assert!(((half_center >> shift) & 255).abs_diff(expected) <= 1);
    }
    assert_eq!(
        frame_pixel(&zero, cx + radius + 3.0, cy),
        frame_pixel(&full, cx + radius + 3.0, cy)
    );
    app.event(Event::MouseUp {
        x: track.x + track.w,
        y,
        button: MouseButton::Left,
        modifiers: Modifiers::default(),
    });
    assert_eq!(app.quick_preview_action(), None);
    assert_eq!(app.brush.opacity, 1.0);
    let released = app.render();
    assert_eq!(
        frame_pixel(&released, cx, cy),
        frame_pixel(&baseline, cx, cy)
    );
    // The opacity disc is cached, so changing color or entering eraser mode
    // must replace the old tint rather than reusing its raster.
    app.brush.color = Color::rgb(12, 180, 230);
    let (x, y) = begin_quick_drag(&mut app, Action::Opacity, 1.0);
    assert_eq!(frame_pixel(&app.render(), cx, cy), 0x0cb4e6);
    app.event(Event::MouseUp {
        x,
        y,
        button: MouseButton::Left,
        modifiers: Modifiers::default(),
    });
    press_key(&mut app, 69, false);
    let (x, y) = begin_quick_drag(&mut app, Action::Opacity, 1.0);
    assert_eq!(frame_pixel(&app.render(), cx, cy), theme.text);
    app.event(Event::MouseUp {
        x,
        y,
        button: MouseButton::Left,
        modifiers: Modifiers::default(),
    });
    assert_eq!(app.doc.to_bytes().unwrap(), before);
    assert_eq!((app.doc.undo_count(), app.doc.redo_count()), (0, 0));
    assert_eq!(app.serial, serial);
}

#[test]
fn quick_preview_gestures_end_or_rollback_without_leaking_pointer_events_to_art() {
    for action in [Action::Size, Action::Opacity] {
        for ending in 0..5 {
            let mut app = floating_panel_fixture();
            let before = app.doc.to_bytes().unwrap();
            let initial = app.parameter_value(action);
            begin_quick_drag(&mut app, action, 0.2);
            assert_eq!(app.quick_preview_action(), Some(action));
            let first = app.parameter_value(action);
            for button in [MouseButton::Middle, MouseButton::Right] {
                app.event(Event::MouseDown {
                    x: 800.0,
                    y: 600.0,
                    button,
                    modifiers: Modifiers::default(),
                });
                app.event(Event::MouseUp {
                    x: 800.0,
                    y: 600.0,
                    button,
                    modifiers: Modifiers::default(),
                });
                assert!(
                    matches!(app.drag, Drag::Slider { action: current, .. } if current == action)
                );
                assert_eq!(app.parameter_value(action), first);
                assert_eq!(app.quick_preview_action(), Some(action));
            }
            // Keeping the original left gesture over exposed art remains a
            // parameter drag; it must never begin a stroke or pan.
            app.event(Event::MouseMove {
                x: 850.0,
                y: 700.0,
                modifiers: Modifiers::default(),
            });
            let changed = app.parameter_value(action);
            assert_ne!(changed, initial);
            assert_eq!(app.doc.to_bytes().unwrap(), before);
            match ending {
                0 => {
                    app.event(Event::MouseUp {
                        x: 850.0,
                        y: 700.0,
                        button: MouseButton::Left,
                        modifiers: Modifiers::default(),
                    });
                }
                1 => {
                    press_key(&mut app, 27, false);
                }
                2 => {
                    app.event(Event::FocusLost);
                }
                3 => {
                    app.event(Event::CaptureLost);
                }
                _ => {
                    press_key(&mut app, 69, false);
                    assert_eq!(app.tool, Tool::Eraser);
                    assert_eq!(app.quick_preview_action(), None);
                    press_key(&mut app, 66, false);
                    assert_eq!(app.tool, Tool::Brush);
                }
            }
            assert!(matches!(app.drag, Drag::None));
            assert_eq!(app.quick_preview_action(), None);
            assert!(
                app.quick_preview.is_none(),
                "rollback must not start another transient preview"
            );
            assert_eq!(
                app.parameter_value(action),
                if ending == 0 || ending == 4 {
                    changed
                } else {
                    initial
                }
            );
            app.event(Event::MouseMove {
                x: 860.0,
                y: 710.0,
                modifiers: Modifiers::default(),
            });
            app.event(Event::MouseUp {
                x: 860.0,
                y: 710.0,
                button: MouseButton::Left,
                modifiers: Modifiers::default(),
            });
            assert_eq!(app.doc.to_bytes().unwrap(), before);
            assert_eq!((app.doc.undo_count(), app.doc.redo_count()), (0, 0));
            assert_eq!(app.serial, 0);
            app.render();
            click_point(&mut app, 850.0, 700.0);
            assert_eq!(
                app.doc.undo_count(),
                1,
                "a fresh deliberate canvas gesture still paints"
            );
        }
    }
}

#[test]
fn transient_quick_preview_handles_exact_input_keyboard_expiry_and_other_gestures() {
    let mut app = floating_panel_fixture();
    let before = app.doc.to_bytes().unwrap();
    let view = (app.view_x, app.view_y, app.zoom);
    let prompt_id = click_action(&mut app, Action::Size)
        .into_iter()
        .find_map(|effect| match effect {
            Effect::Prompt { id, .. } => Some(id),
            _ => None,
        })
        .unwrap();
    app.event(Event::Prompted {
        id: prompt_id,
        value: Some("64".into()),
    });
    assert_eq!(app.brush.size, 64.0);
    assert_eq!(app.quick_preview_action(), Some(Action::Size));
    assert!(matches!(app.drag, Drag::None));
    app.quick_preview.as_mut().unwrap().1 = Instant::now() - Duration::from_millis(1);
    app.event(Event::Tick);
    app.render();
    assert_eq!(app.quick_preview_action(), None);
    app.focus = app
        .controls
        .iter()
        .position(|c| c.action == Action::Size && c.kind == ControlKind::Slider);
    press_key(&mut app, 39, false);
    assert!(app.brush.size > 64.0);
    assert_eq!(app.quick_preview_action(), Some(Action::Size));
    let size = app.brush.size;
    let preview = app.quick_preview_rect(Action::Size);
    let (x, y) = (preview.x + preview.w * 0.5, preview.y + preview.h * 0.5);
    app.render();
    for ctrl in [false, true] {
        app.quick_preview = Some((Action::Size, Instant::now() + Duration::from_millis(600)));
        app.event(Event::Wheel {
            x,
            y,
            delta: 1.0,
            modifiers: Modifiers {
                ctrl,
                ..Modifiers::default()
            },
        });
        assert_eq!((app.view_x, app.view_y, app.zoom), view);
        assert_eq!(app.brush.size, size);
    }
    click_point(&mut app, x, y);
    assert_eq!(app.quick_preview_action(), None);
    assert_eq!(app.doc.to_bytes().unwrap(), before);
    app.render();

    for gesture in 0..4 {
        if gesture == 1 {
            press_key(&mut app, 76, false);
        } else {
            press_key(&mut app, 66, false);
        }
        app.render();
        press_key(&mut app, 221, false);
        assert_eq!(app.quick_preview_action(), Some(Action::Size));
        let (x, y, button) = if gesture == 3 {
            app.render();
            let sv = app
                .controls
                .iter()
                .find(|c| c.action == Action::SV)
                .unwrap()
                .hit;
            (sv.x + sv.w * 0.5, sv.y + sv.h * 0.5, MouseButton::Left)
        } else {
            (
                850.0,
                700.0,
                if gesture == 2 {
                    MouseButton::Middle
                } else {
                    MouseButton::Left
                },
            )
        };
        app.event(Event::MouseDown {
            x,
            y,
            button,
            modifiers: Modifiers::default(),
        });
        assert!(match gesture {
            0 => matches!(app.drag, Drag::Stroke),
            1 => matches!(app.drag, Drag::Shape { .. }),
            2 => matches!(app.drag, Drag::Pan { .. }),
            _ => matches!(app.drag, Drag::Color { .. }),
        });
        assert_eq!(app.quick_preview_action(), None);
        app.event(Event::CaptureLost);
        assert_eq!(app.quick_preview_action(), None);
        assert_eq!(app.doc.to_bytes().unwrap(), before);
        assert_eq!((app.doc.undo_count(), app.doc.redo_count()), (0, 0));
    }
    assert_eq!((app.view_x, app.view_y, app.zoom), view);
}

#[test]
fn last_rendered_quick_preview_blocks_input_until_its_hidden_frame_is_drawn() {
    for action in [Action::Size, Action::Opacity] {
        for expired in [false, true] {
            let mut app = floating_panel_fixture();
            let before = app.doc.to_bytes().unwrap();
            let view = (app.view_x, app.view_y, app.zoom);
            if expired {
                app.focus = app
                    .controls
                    .iter()
                    .position(|c| c.action == action && c.kind == ControlKind::Slider);
                press_key(&mut app, 37, false);
                assert_eq!(app.quick_preview_action(), Some(action));
            } else {
                begin_quick_drag(&mut app, action, 0.3);
            }
            app.render();
            let preview = app.quick_preview_rect(action);
            let (x, y) = (preview.x + preview.w * 0.5, preview.y + preview.h * 0.5);
            if expired {
                app.quick_preview.as_mut().unwrap().1 = Instant::now() - Duration::from_millis(1);
                app.event(Event::Tick);
            } else {
                let track = app.quick_slider_rect(action);
                app.event(Event::MouseUp {
                    x: track.x + track.w * 0.3,
                    y: track.y + 2.0,
                    button: MouseButton::Left,
                    modifiers: Modifiers::default(),
                });
            }
            assert_eq!(app.quick_preview_action(), None);
            let size = app.brush.size;
            // Both the circle interior and the opacity panel stay protected
            // until a fresh frame removes the preview that is still visible.
            for ctrl in [false, true] {
                app.event(Event::Wheel {
                    x,
                    y,
                    delta: 1.0,
                    modifiers: Modifiers {
                        ctrl,
                        ..Modifiers::default()
                    },
                });
                assert_eq!((app.view_x, app.view_y, app.zoom), view);
                assert_eq!(app.brush.size, size);
            }
            click_point(&mut app, x, y);
            assert_eq!(app.doc.to_bytes().unwrap(), before);
            app.render();
            assert!(!app.canvas_obscured(x, y));
            app.event(Event::Wheel {
                x,
                y,
                delta: 1.0,
                modifiers: Modifiers::default(),
            });
            assert_ne!(app.zoom, view.2);
            assert_eq!(app.doc.to_bytes().unwrap(), before);
            assert_eq!((app.doc.undo_count(), app.doc.redo_count()), (0, 0));
        }
    }
}

#[test]
fn size_previews_are_only_outlines_for_brush_eraser_and_all_shapes() {
    fn assert_outline(frame: &Frame, baseline: &Frame, area: Rect, radius: f32) {
        let (cx, cy) = (area.x + area.w * 0.5, area.y + area.h * 0.5);
        assert_eq!(
            frame_pixel(frame, cx, cy),
            frame_pixel(baseline, cx, cy),
            "the center is unfilled"
        );
        let mut changed = 0;
        let (mut dark, mut light) = (false, false);
        for y in area.y as usize..(area.y + area.h) as usize {
            for x in area.x as usize..(area.x + area.w) as usize {
                let at = y * frame.width as usize + x;
                if frame.pixels[at] == baseline.pixels[at] {
                    continue;
                }
                let distance = (x as f32 + 0.5 - cx).hypot(y as f32 + 0.5 - cy);
                assert!(
                    (radius - 2.0..=radius + 3.0).contains(&distance),
                    "size preview adds no background, shadow or filled disk at ({x}, {y})"
                );
                changed += 1;
                // Antialiasing blends the adjacent rings. The outer edge must
                // still have dark pixels, while the inner light ring changes
                // pixels that the outer ring cannot reach.
                dark |= distance > radius + 0.2 && (frame.pixels[at] & 255) < 128;
                light |= distance < radius - 0.2 && (frame.pixels[at] & 255) > 200;
            }
        }
        assert!(
            changed as f32 > radius * 3.0,
            "a visible outline is actually painted"
        );
        assert!(dark && light, "both contrasting outline colors are present");
    }
    for tool in [
        Tool::Brush,
        Tool::Eraser,
        Tool::Line,
        Tool::Rectangle,
        Tool::Ellipse,
    ] {
        let mut app = floating_panel_fixture();
        app.activate(Action::Tool(tool), &mut Vec::new());
        let before = app.doc.to_bytes().unwrap();
        let baseline = app.render();
        let (_, y) = begin_quick_drag(&mut app, Action::Size, 0.08);
        let area = app.quick_preview_rect(Action::Size);
        let (cx, cy) = (area.x + area.w * 0.5, area.y + area.h * 0.5);
        let small_radius = app.quick_preview_radius(Action::Size);
        let small = app.render();
        assert_outline(&small, &baseline, area, small_radius);
        assert!(matches!(
            app.quick_preview_painted,
            Some(PreviewHit::Circle { .. })
        ));
        assert!(
            app.quick_preview_contains(cx, cy),
            "the visible circle still absorbs center input"
        );
        let track = app.quick_slider_rect(Action::Size);
        let x = track.x + track.w * 0.78;
        app.event(Event::MouseMove {
            x,
            y,
            modifiers: Modifiers::default(),
        });
        let large_radius = app.quick_preview_radius(Action::Size);
        assert!(large_radius > small_radius + 10.0);
        let large = app.render();
        assert_outline(&large, &baseline, area, large_radius);
        assert_ne!(small.pixels, large.pixels);
        let corner = (area.x + 3.0, area.y + 3.0);
        assert!(!app.quick_preview_contains(corner.0, corner.1));
        assert!(!app.canvas_obscured(corner.0, corner.1));
        assert!(
            !app.controls
                .iter()
                .any(|c| c.action == Action::NoOp && c.hit.contains(corner.0, corner.1)),
            "the old rectangular preview control was removed"
        );
        app.event(Event::MouseUp {
            x,
            y,
            button: MouseButton::Left,
            modifiers: Modifiers::default(),
        });
        // Before the hidden frame is drawn, only the last painted disk blocks
        // input. An empty corner of its former square reaches the canvas.
        app.event(Event::MouseDown {
            x: corner.0,
            y: corner.1,
            button: MouseButton::Left,
            modifiers: Modifiers::default(),
        });
        assert!(match tool {
            Tool::Brush | Tool::Eraser => matches!(app.drag, Drag::Stroke),
            _ => matches!(app.drag, Drag::Shape { .. }),
        });
        app.event(Event::CaptureLost);
        app.event(Event::MouseMove {
            x,
            y,
            modifiers: Modifiers::default(),
        });
        let released = app.render();
        assert_eq!(app.quick_preview_action(), None);
        for py in area.y as usize..(area.y + area.h) as usize {
            let at = py * released.width as usize + area.x as usize;
            assert_eq!(
                &released.pixels[at..at + area.w as usize],
                &baseline.pixels[at..at + area.w as usize]
            );
        }

        // Opacity keeps its rectangular panel for the very same tool.
        let (opacity_x, opacity_y) = begin_quick_drag(&mut app, Action::Opacity, 0.5);
        app.render();
        assert!(matches!(
            app.quick_preview_painted,
            Some(PreviewHit::Panel(_))
        ));
        let opacity_area = app.quick_preview_rect(Action::Opacity);
        let opacity_corner = (opacity_area.x + 3.0, opacity_area.y + 3.0);
        assert!(app.quick_preview_contains(opacity_corner.0, opacity_corner.1));
        app.event(Event::MouseUp {
            x: opacity_x,
            y: opacity_y,
            button: MouseButton::Left,
            modifiers: Modifiers::default(),
        });
        press_key(&mut app, 221, false);
        assert_eq!(app.quick_preview_action(), Some(Action::Size));
        assert!(
            app.quick_preview_contains(opacity_corner.0, opacity_corner.1),
            "the old opacity frame remains protected until redraw"
        );
        app.render();
        assert!(matches!(
            app.quick_preview_painted,
            Some(PreviewHit::Circle { .. })
        ));
        assert!(!app.quick_preview_contains(opacity_corner.0, opacity_corner.1));
        assert!(!app.controls.iter().any(
            |c| c.action == Action::NoOp && c.hit.contains(opacity_corner.0, opacity_corner.1)
        ));
        assert_eq!(app.doc.to_bytes().unwrap(), before);
        assert_eq!((app.doc.undo_count(), app.doc.redo_count()), (0, 0));
    }
    let mut app = floating_panel_fixture();
    app.resize(884, 581, 1.);
    let before = app.doc.to_bytes().unwrap();
    click_action(&mut app, Action::ToolPanel(ToolPanel::Brush));
    click_action(&mut app, Action::PinToolPanel);
    press_key(&mut app, 115, false);
    app.render();
    let properties = app.tool_settings_rect();
    click_point(&mut app, properties.x + 3., properties.y + 6.);
    app.brush.size = 257.; // The half-track gesture rounds to 257, including the live header sample.
    let baseline = app.render();
    begin_quick_drag(&mut app, Action::Size, 0.5);
    let area = app.quick_preview_rect(Action::Size);
    let (cx, cy) = (area.x + area.w * 0.5, area.y + area.h * 0.5);
    assert_eq!((cx, cy), (378., 352.));
    assert!(properties.contains(cx, cy));
    let radius = app.quick_preview_radius(Action::Size);
    let visible = app.render();
    assert_outline(&visible, &baseline, area, radius);
    assert!(app.quick_preview_contains(cx, cy));
    press_key(&mut app, 27, false);
    assert!(app.tool_settings_visible());
    assert_eq!(app.doc.to_bytes().unwrap(), before);
    assert_eq!((app.doc.undo_count(), app.doc.redo_count()), (0, 0));
}

#[test]
fn preset_header_renames_copies_and_restores_independent_brushes() {
    let directory = FixtureDirectory::new();
    let mut app = floating_panel_fixture();
    app.prefs = Some(directory.file("settings.txt"));
    click_action(&mut app, Action::ToolPanel(ToolPanel::Brush));
    press_key(&mut app, 115, false);
    app.render();
    let initial = ToolSettings::from_brush(app.brush);
    let doc = app.doc.to_bytes().unwrap();
    for s in 0..3 {
        click_action(&mut app, Action::ToolSection(s));
        let frame = app.render();
        let caption = frame.texts.iter().find(|r| r.text == "Образец").unwrap();
        assert!(caption.y < app.tool_settings_viewport().y);
        assert!(!app
            .controls
            .iter()
            .any(|c| matches!(c.action, Action::ToolSection(_)) && c.label == "Образец"));
    }
    click_action(&mut app, Action::RenameBrush);
    app.event(Event::Text("Тушь № 1".into()));
    press_key(&mut app, 84, false);
    assert_eq!(app.tool, Tool::Brush);
    press_key(&mut app, 13, false);
    assert_eq!(app.preset_name(0, 0), "Тушь № 1");
    click_action(&mut app, Action::RenameBrush);
    app.event(Event::Text("Черновик".into()));
    press_key(&mut app, 27, false);
    assert_eq!(app.preset_name(0, 0), "Тушь № 1");
    click_action(&mut app, Action::AddBrush);
    app.event(Event::Paste("Перо 雪".into()));
    press_key(&mut app, 13, false);
    assert_eq!(app.presets.selected[0], BrushKind::ALL.len());
    assert_eq!(ToolSettings::from_brush(app.brush), initial);
    set_tool_number(&mut app, Action::Roundness, "30");
    set_tool_number(&mut app, Action::Grain, "80");
    let copied = ToolSettings::from_brush(app.brush);
    assert_ne!(
        PortfolioApp::tool_sample(initial, 0x334455, false),
        PortfolioApp::tool_sample(copied, 0x334455, false)
    );
    assert_eq!(app.presets.entries[0][0].settings, initial);
    browser_query(&mut app, "雪");
    assert_eq!(app.picker_items(ToolPanel::Brush).len(), 1);
    click_action(&mut app, Action::Brush(BrushKind::ALL.len()));
    assert_eq!(ToolSettings::from_brush(app.brush), copied);
    let saved =
        Presets::decode(&fs::read_to_string(directory.file("brushes.txt")).unwrap()).unwrap();
    assert_eq!(saved.entries, app.presets.entries);
    assert_eq!(saved.selected, app.presets.selected);
    assert_eq!(saved.entries[1].len(), BrushKind::ALL.len());
    assert_eq!(app.doc.to_bytes().unwrap(), doc);
}

fn anchor_raster(text: &TextLayer, w: u32, h: u32, _: Language) -> Result<Vec<Color>, String> {
    let mut pixels = vec![Color::TRANSPARENT; (w * h) as usize];
    pixels[(text.y * w + text.x) as usize] = text.color;
    Ok(pixels)
}

#[test]
fn canvas_text_creation_and_reediting_use_document_coordinates_and_one_undo_step() {
    let mut app = floating_panel_fixture();
    app.text_renderer = anchor_raster;
    click_action(&mut app, Action::ToolPanel(ToolPanel::Text));
    assert_eq!(app.tool_popup, Some(ToolPanel::Text));
    assert!(app.quick_controls_visible());
    click_action(&mut app, Action::ToolProperties);
    assert!(app.tool_settings_visible());
    let (x, y) = (810., 650.);
    let (dx, dy) = app.doc_point(x, y);
    let place = |app: &mut PortfolioApp| {
        app.event(Event::MouseDown {
            x,
            y,
            button: MouseButton::Left,
            modifiers: Modifiers::default(),
        });
    };
    place(&mut app);
    let view = app.canvas_editor().unwrap();
    assert_eq!((view.text.x, view.text.y), (dx as u32, dy as u32));
    assert!((view.anchor.0 as f32 - x * app.scale).abs() < 2.);
    assert_eq!(app.doc.undo_count(), 0);
    app.event(Event::CanvasTextChanged {
        session: view.session,
        body: "Абзац & 字🌿\nСтрока 2".into(),
    });
    click_action(&mut app, Action::TextBold);
    click_action(&mut app, Action::TextAlign(ParagraphAlign::Center));
    click_action(&mut app, Action::TextFont(true));
    app.event(Event::Prompted {
        id: 25,
        value: Some("65".into()),
    });
    app.event(Event::Prompted {
        id: 10,
        value: Some("#6A3281".into()),
    });
    let text = app.canvas_editor().unwrap().text;
    assert_eq!(text.size, 65);
    assert_eq!(text.color.to_hex(), "#6A3281");
    assert!(text.bold && text.body.contains('🌿'));
    assert_eq!(text.align, ParagraphAlign::Center);
    press_key(&mut app, 13, true);
    assert!(app.canvas_editor().is_none());
    let id = app.doc.active_layer().id;
    assert_eq!(app.doc.undo_count(), 1);
    assert_eq!(app.doc.active_layer().text.as_ref(), Some(&text));
    let saved = app.doc.snapshot_for_io().unwrap().to_bytes().unwrap();
    app.doc = Document::from_bytes(&saved).unwrap();
    click_action(&mut app, Action::EditTextLayer(id));
    let edit = app.canvas_editor().unwrap();
    assert_eq!(edit.text, text);
    app.event(Event::CanvasTextChanged {
        session: view.session,
        body: "stale".into(),
    });
    assert_eq!(app.current_text(), &text);
    app.event(Event::CanvasTextChanged {
        session: edit.session,
        body: "отменить".into(),
    });
    app.render();
    let at = (text.y * app.doc.width + text.x) as usize;
    assert_ne!(app.composite[at], app.doc.composite()[at]);
    press_key(&mut app, 27, false);
    assert!(app.canvas_editor().is_none());
    assert_eq!(app.doc.to_bytes().unwrap(), saved);
    place(&mut app);
    assert_eq!(app.draft.as_ref().unwrap().target, Some(id));
    let session = app.canvas_editor().unwrap().session;
    app.event(Event::CanvasTextChanged {
        session,
        body: "Изменено".into(),
    });
    click_action(&mut app, Action::TextItalic);
    let edited = app.canvas_editor().unwrap().text;
    press_key(&mut app, 13, true);
    assert_eq!(app.doc.undo_count(), 1);
    assert!(app.doc.undo());
    assert_eq!(app.doc.to_bytes().unwrap(), saved);
    assert!(app.doc.redo());
    assert_eq!(app.doc.active_layer().text.as_ref(), Some(&edited));
    app.doc.layers[app.doc.active].locked = true;
    place(&mut app);
    assert!(app.canvas_editor().is_none());
    click_action(&mut app, Action::NewText);
    place(&mut app);
    assert_eq!(app.draft.as_ref().unwrap().target, None);
    press_key(&mut app, 27, false);
    assert_eq!(app.doc.undo_count(), 1);
}

#[test]
fn canvas_text_survives_failed_apply_and_follows_zoom_and_panel_focus() {
    let mut app = floating_panel_fixture();
    let mut out = Vec::new();
    app.open_canvas_text(None, app.doc.width / 2, app.doc.height / 2, &mut out);
    let first = app.canvas_editor().unwrap();
    app.event(Event::CanvasTextChanged {
        session: first.session,
        body: "текст".into(),
    });
    app.text_renderer = |_, _, _, _| Err("render error".into());
    app.activate(Action::Save, &mut out);
    assert!(out
        .iter()
        .any(|e| matches!(e, Effect::Message { error: true, .. })));
    assert_eq!(app.current_text().body, "текст");
    assert!(!app.doc.can_undo());
    app.event(Event::FocusTextSettings);
    app.render();
    assert_eq!(focused_action(&app), Some(Action::TextSize));
    press_key(&mut app, 39, false);
    assert_eq!(app.current_text().size, 49);
    let before = app.canvas_editor().unwrap();
    app.zoom_at(2., 420., 330.);
    let after = app.canvas_editor().unwrap();
    assert!(after.font_px > before.font_px && after.anchor != before.anchor);
    app.text_renderer = anchor_raster;
    app.activate(Action::Tool(Tool::Brush), &mut out);
    assert!(app.canvas_editor().is_none());
    assert_eq!(app.doc.active_layer().text.as_ref().unwrap().body, "текст");
}

#[test]
fn each_full_size_slider_resets_only_its_parameter() {
    let mut app = floating_panel_fixture();
    click_action(&mut app, Action::ToolPanel(ToolPanel::Eraser));
    click_action(&mut app, Action::PinToolPanel);
    click_action(&mut app, Action::ToolProperties);
    let defaults = ToolSettings::from_brush(app.brush);
    for slider in [
        Slider::Size,
        Slider::Opacity,
        Slider::Hardness,
        Slider::Flow,
        Slider::Spacing,
        Slider::Angle,
        Slider::Roundness,
        Slider::Grain,
        Slider::Scatter,
    ] {
        let action = slider.action();
        let default = app.parameter_value(action);
        let (min, max, _, _, _) = app.parameter_info(action);
        app.set_parameter(action, if default == min { max } else { min });
        let reset = Action::ResetSlider(slider);
        assert!(app.tool_settings_reveal_action(reset));
        app.render();
        let button = app
            .controls
            .iter()
            .find(|c| c.enabled && c.action == reset)
            .unwrap();
        assert!(app
            .tool_settings_rect()
            .contains(button.hit.x + 1., button.hit.y + 1.));
        assert!(!app.quick_controls_rect().intersects(button.hit));
        click_action(&mut app, reset);
        assert!((app.parameter_value(action) - default).abs() < 0.001);
    }
    assert_eq!(ToolSettings::from_brush(app.brush), defaults);
    press_key(&mut app, 115, false);
    app.render();
    assert!(app.quick_controls_visible());
    assert!(!app.controls.iter().any(|c| c.enabled
        && matches!(c.action, Action::ResetSlider(_))
        && c.hit.intersects(app.quick_controls_rect())));
    app.activate(Action::Tool(Tool::Rectangle), &mut Vec::new());
    app.set_parameter(Action::Size, 70.);
    app.set_parameter(Action::Opacity, 42.);
    app.activate(Action::ResetSlider(Slider::Size), &mut Vec::new());
    assert_eq!(app.brush.size, 4.);
    assert!((app.brush.opacity - 0.42).abs() < 0.001);
    app.doc.set_layer_opacity(app.doc.active, 80);
    app.activate(Action::ResetSlider(Slider::LayerOpacity), &mut Vec::new());
    assert_eq!(app.doc.active_layer().opacity, 255);
    assert!(app.doc.undo());
    assert_eq!(app.doc.active_layer().opacity, 80);
    app.color_s = 0.4;
    app.color_v = 0.67;
    app.color_h = 12.;
    app.activate(Action::ResetSlider(Slider::Hue), &mut Vec::new());
    assert_eq!((app.color_s, app.color_v), (0.4, 0.67));
    assert_eq!(app.color_h, to_hsv(Color::from_hex(DEFAULT_INK), 220.).0);
    app.set_parameter(Action::FontSize, 17.);
    app.activate(Action::ResetSlider(Slider::FontSize), &mut Vec::new());
    assert_eq!(app.font_size, Appearance::default().font_size);
    press_key(&mut app, 84, false);
    click_action(&mut app, Action::ToolProperties);
    app.set_parameter(Action::TextSize, 400.);
    click_action(&mut app, Action::ResetSlider(Slider::TextSize));
    assert_eq!(app.current_text().size, 48);
    app.render();
    let bar = app.top_tools_rect();
    let grip = app.toolbar_grip_rect();
    assert!(app
        .controls
        .iter()
        .filter(|c| c.enabled
            && c.action != Action::MoveToolbar
            && c.action != Action::NoOp
            && bar.contains(c.hit.x + c.hit.w / 2., c.hit.y + c.hit.h / 2.))
        .all(|c| c.hit.x + c.hit.w <= grip.x));
}

#[test]
fn text_catalog_basics_and_gear_edit_one_canvas_draft_without_touching_brush_controls() {
    let mut app = floating_panel_fixture();
    let brush = app.brush;
    click_action(&mut app, Action::ToolPanel(ToolPanel::Text));
    assert_eq!(app.tool_popup, Some(ToolPanel::Text));
    assert!(!app.tool_settings_visible());
    assert!(app.quick_controls_visible());
    app.render();
    let bar = app.quick_controls_rect();
    assert!(app.controls.iter().any(|c| c.enabled
        && c.action == Action::TextBold
        && bar.contains(c.hit.x + 1., c.hit.y + 1.)));
    assert!(app
        .controls
        .iter()
        .filter(|c| c.enabled && bar.contains(c.hit.x + 1., c.hit.y + 1.))
        .all(|c| c.kind != ControlKind::Slider));
    click_action(&mut app, Action::BrowserCategory(Category::Volume));
    assert_eq!(app.picker_items(ToolPanel::Text).len(), 2);
    browser_query(&mut app, "relief");
    assert_eq!(
        app.picker_items(ToolPanel::Text)[0].0,
        Action::TextPreset(TextPreset::Relief)
    );
    press_key(&mut app, 70, true); // Return focus to the search field.
    press_key(&mut app, 13, false);
    press_key(&mut app, 27, false);
    assert_eq!(
        app.selected_tool_option(),
        Action::TextPreset(TextPreset::Relief)
    );
    let (x, y) = (810., 650.);
    left_down(&mut app, x, y);
    left_up(&mut app, x, y);
    let v = app
        .canvas_editor()
        .expect("first canvas click opens the editor");
    app.event(Event::CanvasTextChanged {
        session: v.session,
        body: "Слой 3D".into(),
    });
    click_action(&mut app, Action::TextPreset(TextPreset::Block));
    assert_eq!(app.canvas_editor().unwrap().session, v.session);
    assert_eq!(app.current_text().body, "Слой 3D");
    assert_eq!(app.current_text().effects.depth, 12);
    click_action(&mut app, Action::CycleTextAlign);
    assert_eq!(app.current_text().align, ParagraphAlign::Center);
    click_action(&mut app, Action::ToolProperties);
    click_action(&mut app, Action::ToolSection(2));
    app.event(Event::Prompted {
        id: 33,
        value: Some("26".into()),
    });
    app.event(Event::Prompted {
        id: 34,
        value: Some("135".into()),
    });
    app.render();
    let sample = &app.text_samples.get(&None).unwrap().0;
    assert_eq!(sample.body, "Слой 3D");
    assert_eq!(sample.effects.angle, 135);
    assert!(sample.effects.depth > 0);
    assert_eq!(app.draft_effects.as_ref().unwrap().0.effects.depth, 26);
    click_action(
        &mut app,
        Action::ResetSlider(Slider::TextEffect(EffectParam::Depth)),
    );
    assert_eq!(app.current_text().effects.depth, 0);
    click_action(&mut app, Action::ToolSection(1));
    app.event(Event::Prompted {
        id: 30,
        value: Some("3".into()),
    });
    app.event(Event::Prompted {
        id: 32,
        value: Some("70".into()),
    });
    app.event(Event::Prompted {
        id: 29,
        value: Some("#AB3572".into()),
    });
    assert_eq!(app.current_text().effects.color, Color::from_hex(0xab3572));
    app.text_renderer = anchor_raster; // the native renderer is covered independently
    press_key(&mut app, 13, true);
    assert_eq!(app.doc.undo_count(), 1);
    assert_eq!(
        app.doc
            .active_layer()
            .text
            .as_ref()
            .unwrap()
            .effects
            .outline,
        3
    );
    click_action(&mut app, Action::ToolPanel(ToolPanel::Brush));
    assert_eq!(
        ToolSettings::from_brush(app.brush),
        ToolSettings::from_brush(brush)
    );
}

#[test]
fn toolbar_disc_updates_one_text_draft_and_gives_its_caret_back() {
    let mut app = floating_panel_fixture();
    app.text_renderer = anchor_raster;
    click_action(&mut app, Action::ToolPanel(ToolPanel::Text));
    click_action(&mut app, Action::ToolProperties);
    left_down(&mut app, 810., 650.);
    left_up(&mut app, 810., 650.);
    let session = app.canvas_editor().unwrap().session;
    app.event(Event::CanvasTextChanged {
        session,
        body: "Линия цвета".into(),
    });
    click_action(&mut app, Action::ColorDisc);
    assert!(app.color_disc.open && app.tool_settings_visible());
    let p = app.color_popup_rect();
    let r = app.wheel_rect();
    let (cx, cy) = (r.x + r.w / 2., r.y + r.h / 2.);
    let saved = app.current_text().clone();
    left_down(&mut app, cx + r.w * 0.46, cy);
    left_up(&mut app, cx + r.w * 0.46, cy);
    assert!((app.color_h - 90.).abs() < 0.001);
    left_down(&mut app, cx, cy);
    left_up(&mut app, cx, cy);
    assert_eq!(app.current_text().color, hsv(90., 0.5, 0.5));
    assert_eq!(app.brush.color, app.current_text().color);
    assert_eq!(app.canvas_editor().unwrap().session, session);
    assert_eq!(app.current_text().body, saved.body);
    assert!(app
        .canvas_editor()
        .unwrap()
        .panels
        .iter()
        .any(|rect| rect[0] <= p.x as i32 && rect[2] >= (p.x + p.w) as i32));
    let effects = click_action(&mut app, Action::Hex);
    assert!(effects
        .iter()
        .any(|e| matches!(e, Effect::Prompt { id: 10, .. })));
    app.event(Event::Prompted {
        id: 10,
        value: Some("#542577".into()),
    });
    assert_eq!(app.current_text().color, Color::from_hex(0x542577));
    let close = press_key(&mut app, 27, false);
    assert!(!app.color_disc.open && app.tool_settings_visible());
    assert!(close.iter().any(|e| matches!(e, Effect::FocusCanvasText)));
    assert_eq!(app.doc.undo_count(), 0);
    assert_eq!(app.tool_popup, Some(ToolPanel::Text));
    press_key(&mut app, 13, true);
    let layer = app.doc.active_layer();
    assert_eq!(
        layer.text.as_ref().unwrap().color,
        Color::from_hex(0x542577)
    );
    let id = layer.id;
    assert_eq!(app.doc.undo_count(), 1);
    app.update_color(Color::WHITE);
    app.activate(Action::EditTextLayer(id), &mut Vec::new());
    assert_eq!(app.brush.color, Color::from_hex(0x542577));
    assert_eq!(app.doc.undo_count(), 1);
}

#[test]
fn color_disc_owns_its_gesture_dismissal_and_keyboard_in_both_themes() {
    let mut app = floating_panel_fixture();
    app.inspector_layers = true;
    for (dark, scale) in [(true, 1.), (false, 1.5)] {
        app.dark = dark;
        app.resize((884. * scale) as u32, (581. * scale) as u32, scale);
        app.set_panel_position(PanelKind::Toolbar, Some((16., 505.)));
        click_action(&mut app, Action::ColorDisc);
        let p = app.color_popup_rect();
        let trigger = app.color_trigger();
        assert!(p.y >= 8. && p.y + p.h < trigger.y && p.x >= 0. && p.x + p.w <= app.w());
        assert!(app.inspector_layers);
        let wheel = app.wheel_rect();
        let (cx, cy) = (wheel.x + wheel.w * 0.5, wheel.y + wheel.h * 0.5);
        let baseline = (app.brush.color, app.color_h, app.color_s, app.color_v);
        left_down(&mut app, cx + wheel.w * 0.398, cy); // gap between hue and SV
        assert!(matches!(app.drag, Drag::None));
        left_up(&mut app, cx + wheel.w * 0.398, cy);
        left_down(&mut app, cx, cy - wheel.h * 0.46);
        app.event(Event::MouseMove {
            x: cx + wheel.w,
            y: cy,
            modifiers: Modifiers::default(),
        });
        assert_eq!(app.color_h, 90.);
        assert_eq!((app.color_s, app.color_v), (baseline.2, baseline.3));
        app.event(Event::CaptureLost);
        assert_eq!(
            (app.brush.color, app.color_h, app.color_s, app.color_v),
            baseline
        );
        left_down(&mut app, cx, cy);
        press_key(&mut app, 27, false);
        assert!(app.color_disc.open);
        assert_eq!(app.brush.color, baseline.0);
        app.render();
        app.event(Event::Prompted {
            id: 10,
            value: Some("808080".into()),
        });
        let remembered = app.color_h;
        press_key(&mut app, 39, false); // last focus was saturation/value
        assert!(app.color_s > 0. && (app.color_h - remembered).abs() < 0.001);
        click_action(&mut app, Action::Swatch(2));
        assert_eq!(app.brush.color, Color::from_hex(PALETTE[2]));
        let zoom = app.zoom;
        app.event(Event::Wheel {
            x: cx,
            y: cy,
            delta: 1.,
            modifiers: Modifiers::default(),
        });
        assert_eq!(app.zoom, zoom);
        left_down(&mut app, 470., 410.);
        left_up(&mut app, 470., 410.);
        assert!(!app.color_disc.open);
        assert_eq!(app.doc.undo_count(), 0);
        click_action(&mut app, Action::ColorDisc);
        press_key(&mut app, 9, false);
        press_key(&mut app, 9, false);
        let edit = press_key(&mut app, 13, false);
        assert!(edit
            .iter()
            .any(|e| matches!(e, Effect::Prompt { id: 10, .. })));
        left_down(&mut app, trigger.x + 18., trigger.y + 16.);
        left_up(&mut app, trigger.x + 18., trigger.y + 16.);
        assert!(!app.color_disc.open);
    }
}

#[test]
fn text_properties_use_the_brush_navigation_and_the_basic_strip_has_six_controls() {
    let mut app = floating_panel_fixture();
    app.resize(884, 581, 1.);
    click_action(&mut app, Action::ToolPanel(ToolPanel::Text));
    click_action(&mut app, Action::PinToolPanel);
    click_action(&mut app, Action::ToolProperties);
    app.render();
    let bar = app.quick_controls_rect();
    let actions: Vec<_> = app
        .controls
        .iter()
        .filter(|c| c.enabled && bar.contains(c.hit.x + c.hit.w / 2., c.hit.y + c.hit.h / 2.))
        .map(|c| c.action)
        .collect();
    assert_eq!(
        actions,
        [
            Action::TextSize,
            Action::TextFonts,
            Action::TextBold,
            Action::TextItalic,
            Action::CycleTextAlign,
            Action::NewText
        ]
    );
    let mut last = None;
    for section in 0..3 {
        click_action(&mut app, Action::ToolSection(section));
        app.render();
        let nav = app
            .controls
            .iter()
            .find(|c| c.enabled && c.action == Action::ToolSection(section))
            .unwrap()
            .rect;
        let body = app.tool_settings_viewport();
        assert!(nav.x + nav.w <= body.x && nav.y >= body.y);
        if let Some((nx, ny)) = last {
            assert_eq!(nav.x, nx);
            assert!(nav.y > ny);
        }
        last = Some((nav.x, nav.y));
        let expected = [
            Action::TextSize,
            Action::TextEffect(EffectParam::Outline),
            Action::TextEffect(EffectParam::Depth),
        ][section as usize];
        assert!(app
            .controls
            .iter()
            .any(|c| c.enabled && c.action == expected && body.intersects(c.hit)));
        assert!(app.text_samples.contains_key(&None));
    }
}

#[test]
fn quick_font_search_selects_a_typeface_without_replacing_the_canvas_draft() {
    let mut app = floating_panel_fixture();
    app.text_renderer = anchor_raster;
    click_action(&mut app, Action::ToolPanel(ToolPanel::Text));
    click_action(&mut app, Action::ToolProperties);
    click_point(&mut app, 810., 650.);
    let original = app.canvas_editor().unwrap();
    let body = "Тень и свет 🌿";
    app.event(Event::CanvasTextChanged {
        session: original.session,
        body: body.into(),
    });
    let appearance = app.ui_font;
    click_action(&mut app, Action::TextFonts);
    assert!(app.font_picker.open);
    app.event(Event::Text("pRoGgY".into()));
    app.render();
    let results: Vec<_> = app
        .controls
        .iter()
        .filter_map(|c| {
            if let Action::DocumentFont(f) = c.action {
                Some(f)
            } else {
                None
            }
        })
        .collect();
    assert_eq!(
        results,
        [
            UiFont::ProggyForever,
            UiFont::ProggyClean,
            UiFont::ProggySquare
        ]
    );
    press_key(&mut app, 65, true);
    app.event(Event::Paste("Шрифт 雪".into()));
    assert!(app
        .render()
        .texts
        .iter()
        .any(|r| r.text == "Шрифты не найдены"));
    click_action(&mut app, Action::ClearFontQuery);
    app.event(Event::Text("segoe script".into()));
    let effects = press_key(&mut app, 13, false);
    assert!(effects.iter().any(|e| matches!(e, Effect::FocusCanvasText)));
    let updated = app.canvas_editor().unwrap();
    assert_eq!(
        (updated.session, updated.text.x, updated.text.y),
        (original.session, original.text.x, original.text.y)
    );
    assert_eq!(updated.text.font, UiFont::SegoeScript.key());
    assert_eq!(updated.text.body, body);
    assert_eq!(app.ui_font, appearance);
    assert!(app.tool_settings_visible());
    assert_eq!(app.tool_popup, Some(ToolPanel::Text));
    assert_eq!(app.doc.undo_count(), 0);
    click_action(&mut app, Action::TextFonts);
    app.event(Event::Text("verdana".into()));
    press_key(&mut app, 27, false);
    assert!(!app.font_picker.open);
    assert_eq!(app.canvas_editor().unwrap().text, updated.text);
    press_key(&mut app, 13, true);
    assert_eq!(app.doc.active_layer().text.as_ref(), Some(&updated.text));
}

#[test]
fn quick_font_list_scrolls_with_wheel_thumb_and_keyboard_and_consumes_outside_clicks() {
    for (dark, scale) in [(true, 1.), (false, 1.5)] {
        let mut app = floating_panel_fixture();
        app.resize((884. * scale) as u32, (581. * scale) as u32, scale);
        app.dark = dark;
        let (art, zoom) = (app.doc.to_bytes().unwrap(), app.zoom);
        click_action(&mut app, Action::ToolPanel(ToolPanel::Text));
        click_action(&mut app, Action::PinToolPanel);
        click_action(&mut app, Action::TextFonts);
        app.render();
        let panel = app.font_menu_rect();
        let track = app.font_scroll_track();
        assert!(track.h > 0. && app.font_picker.scroll.max() > 0.);
        app.event(Event::Wheel {
            x: panel.x + 25.,
            y: track.y + 12.,
            delta: -1.,
            modifiers: Modifiers {
                ctrl: true,
                ..Default::default()
            },
        });
        assert!(app.font_picker.scroll.offset > 0.);
        let thumb = app.font_scroll_thumb();
        left_down(&mut app, thumb.x + 5., thumb.y + 4.);
        left_up(&mut app, thumb.x + 5., track.y + track.h - 1.);
        assert_eq!(app.font_picker.scroll.offset, app.font_picker.scroll.max());
        click_action(&mut app, Action::DocumentFont(UiFont::ProggySquare));
        assert_eq!(app.current_text().font, UiFont::ProggySquare.key());
        click_action(&mut app, Action::TextFonts);
        press_key(&mut app, 38, false);
        press_key(&mut app, 36, false);
        press_key(&mut app, 13, false);
        assert_eq!(app.current_text().font, UiFont::Bahnschrift.key());
        click_action(&mut app, Action::TextFonts);
        click_point(&mut app, 22., 500.);
        assert!(!app.font_picker.open && app.draft.is_none());
        assert_eq!(app.zoom, zoom);
        assert_eq!(app.doc.to_bytes().unwrap(), art);
    }
}

#[test]
fn separate_brush_and_eraser_dynamics_and_shape_contour_use_their_property_controls() {
    let mut app = floating_panel_fixture();
    for (family, scale, jitter) in [
        (ToolPanel::Brush, "190", "60"),
        (ToolPanel::Eraser, "75", "20"),
    ] {
        click_action(&mut app, Action::ToolPanel(family));
        browser_query(&mut app, "bristle");
        click_action(&mut app, Action::Brush(BrushKind::Bristle.index() as usize));
        set_tool_number(&mut app, Action::TextureScale, scale);
        set_tool_number(&mut app, Action::OpacityJitter, jitter);
        assert_eq!(
            (
                app.parameter_value(Action::TextureScale).round(),
                app.parameter_value(Action::OpacityJitter).round()
            ),
            (scale.parse().unwrap(), jitter.parse().unwrap())
        );
    }
    click_action(&mut app, Action::ToolPanel(ToolPanel::Brush));
    assert_eq!(app.parameter_value(Action::OpacityJitter).round(), 60.);
    app.tool_settings_reveal_action(Action::OpacityJitter);
    click_action(&mut app, Action::ResetSlider(Slider::OpacityJitter));
    assert_eq!(
        app.brush.opacity_jitter,
        ToolSettings::preset(BrushKind::Bristle, false).opacity_jitter
    );
    click_action(&mut app, Action::ToolPanel(ToolPanel::Shapes));
    browser_query(&mut app, "star");
    click_action(&mut app, Action::Shape(Tool::Star));
    assert_eq!(app.tool.shape_kind(), Some(ShapeKind::Star));
    set_tool_number(&mut app, Action::FillOpacity, "32");
    set_tool_number(&mut app, Action::Spacing, "17");
    assert_eq!(app.tool_settings_section(), 2);
    assert!((app.fill_opacity - 0.32).abs() < 0.001 && (app.brush.spacing - 0.17).abs() < 0.001);
}
