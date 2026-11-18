#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]
use portfolio::{
    app::PortfolioApp,
    appearance::Accent,
    document::BrushKind,
    i18n::Language,
    platform::{Event, Modifiers, MouseButton, NativeApp, UiFont},
    tool_browser::{Category, Family, ItemId},
};
fn key(app: &mut PortfolioApp, code: u32, ctrl: bool) {
    app.event(Event::Key {
        code,
        pressed: true,
        repeat: false,
        modifiers: Modifiers {
            ctrl,
            ..Modifiers::default()
        },
    });
    app.render();
}
fn click(app: &mut PortfolioApp, x: f32, y: f32) {
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
    app.render();
}

fn wheel(app: &mut PortfolioApp, x: f32, y: f32, delta: f32) {
    app.event(Event::Wheel {
        x,
        y,
        delta,
        modifiers: Modifiers::default(),
    });
    app.render();
}

fn drag_panel(app: &mut PortfolioApp, x: f32, y: f32, dx: f32, dy: f32) {
    if dx == 0. && dy == 0. {
        return;
    }
    app.event(Event::MouseDown {
        x,
        y,
        button: MouseButton::Left,
        modifiers: Modifiers::default(),
    });
    app.event(Event::MouseMove {
        x: x + dx,
        y: y + dy,
        modifiers: Modifiers::default(),
    });
    app.event(Event::MouseUp {
        x: x + dx,
        y: y + dy,
        button: MouseButton::Left,
        modifiers: Modifiers::default(),
    });
    app.render();
}

fn chooser_query(app: &mut PortfolioApp, query: &str) {
    key(app, 70, true);
    key(app, 65, true);
    if query.is_empty() {
        key(app, 46, false);
    } else {
        app.event(Event::Text(query.to_owned()));
        app.render();
    }
}

fn chooser_scroll(app: &mut PortfolioApp, x: f32, y: f32, pixels: f32, step: f32) {
    // Reach the left bound through the same clamped wheel route as the UI.
    wheel(app, x, y, 1000.);
    if pixels > 0. {
        wheel(app, x, y, -pixels / step);
    }
}

fn settings_query(app: &mut PortfolioApp, query: &str) {
    key(app, 70, true);
    key(app, 65, true);
    if query.is_empty() {
        key(app, 46, false);
    } else {
        app.event(Event::Text(query.to_owned()));
        app.render();
    }
}

/// Search and keyboard selection exercise the same focus/reveal path as the
/// application, independently of font size or the vertical scroll position.
fn settings_option(app: &mut PortfolioApp, query: &str, index: usize) {
    settings_query(app, query);
    key(app, 13, false); // Focus the matching field.
    key(app, 13, false); // Open its selection-only dropdown.
    key(app, 36, false);
    for _ in 0..index {
        key(app, 40, false);
    }
    key(app, 13, false);
}

fn tool_viewport_point(app: &mut PortfolioApp, language: Language) -> Option<(f32, f32)> {
    let labels = ["Основные", "Отпечаток", "Штрих", "Заливка"].map(|text| language.translate(text));
    app.render()
        .texts
        .iter()
        .rev()
        .filter(|run| labels.contains(&run.text))
        .find_map(|run| {
            let (x, y, w, h) = run.clip?;
            (w > 0. && h > 0.).then_some((x + w / 2., y + h / 2.))
        })
}

fn click_visible_text(app: &mut PortfolioApp, text: &str) -> bool {
    let frame = app.render();
    let point = frame
        .texts
        .iter()
        .rev()
        .filter(|run| run.text == text)
        .find_map(|run| {
            let (mut left, mut top, mut right, mut bottom) =
                (run.x, run.y, run.x + run.width, run.y + run.height);
            if let Some((x, y, w, h)) = run.clip {
                left = left.max(x);
                top = top.max(y);
                right = right.min(x + w);
                bottom = bottom.min(y + h);
            }
            (right > left && bottom > top).then_some(((left + right) / 2., (top + bottom) / 2.))
        });
    if let Some((x, y)) = point {
        click(app, x, y);
        true
    } else {
        false
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("--render-preview") {
        let path = args.get(2).map(String::as_str).unwrap_or("preview");
        let value = |flag: &str| {
            args.iter()
                .position(|a| a == flag)
                .and_then(|i| args.get(i + 1))
        };
        let has = |flag: &str| args.iter().any(|a| a == flag);
        let size = value("--size")
            .and_then(|s| s.split_once('x'))
            .and_then(|(w, h)| Some((w.parse::<u32>().ok()?, h.parse::<u32>().ok()?)))
            .unwrap_or((1440, 940));
        let scale = value("--scale")
            .and_then(|s| s.parse::<f32>().ok())
            .filter(|v| v.is_finite())
            .unwrap_or(1.)
            .clamp(0.5, 3.);
        let physical = (
            (size.0 as f32 * scale) as u32,
            (size.1 as f32 * scale) as u32,
        );
        if portfolio::platform::bounded_frame_size(physical.0, physical.1) != physical {
            eprintln!("Preview exceeds the framebuffer budget.");
            std::process::exit(2);
        }
        let preview_value = |flag: &str, min: f32, max: f32| {
            if !has(flag) {
                return None;
            }
            let parsed = value(flag)
                .and_then(|s| s.parse::<f32>().ok())
                .filter(|v| v.is_finite() && (min..=max).contains(v));
            if parsed.is_none() {
                eprintln!("{flag} requires a finite number between {min} and {max}.");
                std::process::exit(2);
            }
            parsed
        };
        let size_preview = preview_value("--size-preview", 1., 512.);
        let opacity_preview = preview_value("--opacity-preview", 0., 100.);
        let preview_offset = |flag: &str| {
            if !has(flag) {
                return None;
            }
            let parsed = value(flag)
                .and_then(|s| s.split_once(','))
                .and_then(|(dx, dy)| {
                    Some((
                        dx.trim().parse::<f32>().ok()?,
                        dy.trim().parse::<f32>().ok()?,
                    ))
                })
                .filter(|&(dx, dy)| {
                    dx.is_finite()
                        && dy.is_finite()
                        && (-10_000. ..=10_000.).contains(&dx)
                        && (-10_000. ..=10_000.).contains(&dy)
                });
            if parsed.is_none() {
                eprintln!("{flag} requires DX,DY with finite values between -10000 and 10000.");
                std::process::exit(2);
            }
            parsed
        };
        let toolbar_offset = preview_offset("--toolbar-offset");
        let chooser_offset = preview_offset("--chooser-offset");
        if let Some((dx, dy)) = chooser_offset {
            if !has("--pin-panel") {
                eprintln!("--chooser-offset requires --pin-panel.");
                std::process::exit(2);
            }
            if dx.hypot(dy) > 0. && dx.hypot(dy) <= 5. {
                eprintln!("--chooser-offset must exceed the 5 px drag threshold, or be 0,0.");
                std::process::exit(2);
            }
        }
        let brush_search = if has("--brush-search") {
            match value("--brush-search") {
                Some(query) => Some(query.as_str()),
                None => {
                    eprintln!("--brush-search requires a query (an empty string clears it).");
                    std::process::exit(2);
                }
            }
        } else {
            None
        };
        let brush_category = if has("--brush-category") {
            match value("--brush-category").and_then(|key| Category::from_key(key)) {
                Some(category) => Some(category),
                None => {
                    eprintln!("--brush-category requires a supported category key.");
                    std::process::exit(2);
                }
            }
        } else {
            None
        };
        let brush_scroll = preview_value("--brush-scroll", 0., 1_000_000.);
        let settings_scroll = preview_value("--settings-scroll", 0., 1_000_000.);
        let tool_scroll = preview_value("--tool-scroll", 0., 1_000_000.);
        let settings_search = if has("--settings-search") {
            match value("--settings-search") {
                Some(query) => Some(query.as_str()),
                None => {
                    eprintln!("--settings-search requires a query (an empty string clears it).");
                    std::process::exit(2);
                }
            }
        } else {
            None
        };
        let logical_width = physical.0 as f32 / scale;
        let mut app = PortfolioApp::new(false);
        app.resize(physical.0, physical.1, scale);
        if has("--light") {
            app.dark = false;
        }
        app.render();
        if has("--layers") {
            key(&mut app, 118, false);
        }
        let font = value("--font").and_then(|s| UiFont::from_key(s));
        let accent = value("--accent").and_then(|s| Accent::from_key(s));
        let language = value("--language").and_then(|s| Language::from_key(s));
        let fontsize = value("--font-size").and_then(|s| s.parse::<u16>().ok());
        if font.is_some()
            || accent.is_some()
            || language.is_some()
            || fontsize.is_some()
            || has("--settings")
            || has("--fonts")
            || has("--languages")
            || has("--font-menu")
            || has("--language-menu")
            || has("--settings-collapsed")
            || settings_search.is_some()
            || settings_scroll.is_some()
        {
            key(&mut app, 188, true);
            if let Some(accent) = accent {
                settings_query(&mut app, "accent");
                key(&mut app, 13, false);
                let i = Accent::ALL.iter().position(|a| *a == accent).unwrap();
                for _ in 0..i {
                    key(&mut app, 39, false);
                }
            }
            if let Some(font) = font {
                let i = UiFont::ALL.iter().position(|f| *f == font).unwrap();
                settings_option(&mut app, "font", i);
            }
            if let Some(size) = fontsize {
                app.event(Event::Prompted {
                    id: 22,
                    value: Some(size.to_string()),
                });
                app.render();
            }
            if let Some(language) = language {
                let i = Language::ALL.iter().position(|l| *l == language).unwrap();
                settings_option(&mut app, "language", i);
            }
            let show_settings = has("--settings")
                || has("--settings-collapsed")
                || has("--fonts")
                || has("--languages")
                || has("--font-menu")
                || has("--language-menu")
                || settings_search.is_some()
                || settings_scroll.is_some();
            if show_settings {
                let query = if has("--language-menu") {
                    "language"
                } else if has("--font-menu") {
                    "text"
                } else if let Some(query) = settings_search {
                    query
                } else if has("--settings") || has("--settings-collapsed") {
                    ""
                } else if has("--languages") {
                    "language"
                } else if has("--fonts") {
                    "text"
                } else {
                    ""
                };
                settings_query(&mut app, query);
                if let Some(offset) = settings_scroll {
                    let sx = (logical_width - 416.).max(16.);
                    chooser_scroll(&mut app, sx + 180., 176., offset, 44.);
                }
                if has("--font-menu") || has("--language-menu") {
                    key(&mut app, 13, false);
                    key(&mut app, 13, false);
                } else {
                    key(&mut app, 27, false); // Leave the query visible without a caret.
                }
            } else {
                for _ in 0..3 {
                    key(&mut app, 27, false);
                }
            }
        }
        let brush = value("--brush")
            .and_then(|s| s.parse::<usize>().ok())
            .filter(|i| *i < 10);
        let bx = (logical_width
            - (logical_width - 96.)
                .max(0.)
                .min(portfolio::app::TOOLBAR_WIDTH))
            / 2.;
        let shape_fill = has("--shape-fill");
        if shape_fill && (has("--eraser-panel") || has("--brush-panel") || brush.is_some()) {
            eprintln!(
                "--shape-fill requires the shapes family; remove brush/eraser selection flags."
            );
            std::process::exit(2);
        }
        let family = if has("--eraser-panel") {
            Family::Eraser
        } else if has("--shape-panel") || shape_fill {
            Family::Shapes
        } else {
            Family::Brush
        };
        if has("--detail") && family == Family::Shapes {
            eprintln!("--detail requires a brush or eraser; use --shape-fill for shapes.");
            std::process::exit(2);
        }
        if has("--brush-panel")
            || has("--eraser-panel")
            || has("--shape-panel")
            || brush.is_some()
            || brush_search.is_some()
            || brush_category.is_some()
            || brush_scroll.is_some()
            || has("--pin-panel")
            || has("--browser-search-open")
            || shape_fill
        {
            if brush.is_some() && family == Family::Shapes {
                eprintln!(
                    "--brush selects a brush or eraser preset; use --brush-search for shapes."
                );
                std::process::exit(2);
            }
            if brush_category
                .is_some_and(|category| !Category::for_family(family).contains(&category))
            {
                eprintln!("--brush-category does not belong to the selected tool family.");
                std::process::exit(2);
            }
            // Close any requested settings/list before opening a different
            // surface; its outside click would otherwise only dismiss it.
            for _ in 0..3 {
                key(&mut app, 27, false);
            }
            click(&mut app, bx + 26. + family.index() as f32 * 40., 104.);
            let standard_width: f32 = if family == Family::Shapes { 380. } else { 464. };
            let initial_x = (bx + 8. + family.index() as f32 * 40.)
                .clamp(16., (logical_width - standard_width - 16.).max(16.));
            if let Some(i) = brush {
                let kind = BrushKind::from_index(i as u8);
                let item = if family == Family::Eraser {
                    ItemId::Eraser(kind)
                } else {
                    ItemId::Brush(kind)
                };
                chooser_query(&mut app, item.label());
                key(&mut app, 13, false);
                key(&mut app, 27, false); // Close search and restore the unfiltered categories.
            }
            if has("--pin-panel") {
                click(&mut app, initial_x + standard_width - 22., 162.);
            }
            let popup_width = if has("--pin-panel") {
                standard_width.min((logical_width - 640.).max(244.))
            } else {
                standard_width
            };
            let px = if has("--pin-panel") {
                (logical_width - popup_width) / 2.
            } else {
                initial_x
            };
            let items_width = popup_width - 56.;
            if let Some(i) = brush {
                let offset =
                    (i as f32 * 104. - (items_width - 96.) / 2.).clamp(0., 1032. - items_width);
                chooser_scroll(&mut app, px + 56., 170., offset, 72.);
            }
            // Categories are available while search is closed. Apply them
            // first because closing an expanded search deliberately clears it.
            if let Some(category) = brush_category {
                let categories = Category::for_family(family);
                let index = categories
                    .iter()
                    .position(|candidate| *candidate == category)
                    .unwrap();
                if index == 0 {
                    click(&mut app, px + 24., 214.);
                } else {
                    let index = index - 1;
                    let viewport = popup_width - 128.;
                    let maximum = ((categories.len() - 1) as f32 * 36. - 4. - viewport).max(0.);
                    let offset = (index as f32 * 36. + 32. - viewport).clamp(0., maximum);
                    chooser_scroll(&mut app, px + 60., 214., offset, 36.);
                    click(&mut app, px + 60. + index as f32 * 36. - offset, 214.);
                }
            }
            if let Some(query) = brush_search {
                chooser_query(&mut app, query);
            }
            if let Some(offset) = brush_scroll {
                chooser_scroll(&mut app, px + 56., 170., offset, 72.);
            }
            if has("--browser-search-open") {
                key(&mut app, 70, true);
            }
        }
        if has("--tool-settings") || has("--detail") || tool_scroll.is_some() || shape_fill {
            // A tool-only preview may follow an appearance-only setup.
            if !has("--pin-panel")
                && !has("--brush-panel")
                && !has("--eraser-panel")
                && !has("--shape-panel")
                && brush.is_none()
                && brush_search.is_none()
                && brush_category.is_none()
                && brush_scroll.is_none()
                && !has("--browser-search-open")
                && !shape_fill
            {
                for _ in 0..3 {
                    key(&mut app, 27, false);
                }
            }
            key(&mut app, 115, false);
        }
        if shape_fill || tool_scroll.is_some() || has("--detail") {
            let language = language.unwrap_or_default();
            let section = if shape_fill {
                Some("Заливка")
            } else if has("--detail") {
                Some("Отпечаток")
            } else {
                None
            };
            if let Some(section) = section {
                if !click_visible_text(&mut app, &language.translate(section)) {
                    eprintln!("The requested tool settings section is not visible.");
                    std::process::exit(2);
                }
            }
            let Some((x, y)) = tool_viewport_point(&mut app, language) else {
                eprintln!("The tool settings viewport is not visible.");
                std::process::exit(2);
            };
            if shape_fill {
                chooser_scroll(&mut app, x, y, 1_000_000., 44.);
                if !click_visible_text(&mut app, &language.translate("Заливка фигуры"))
                {
                    eprintln!("The shape fill switch is not visible.");
                    std::process::exit(2);
                }
            }
            if let Some(offset) = tool_scroll {
                chooser_scroll(&mut app, x, y, offset, 44.);
            }
        }
        if has("--menu") {
            click(&mut app, 105., 30.);
        }
        if has("--new") {
            key(&mut app, 78, true);
        }
        if has("--help-ui") {
            key(&mut app, 112, false);
        }
        if has("--scroll-panel") {
            app.event(Event::Wheel {
                x: size.0 as f32 - 100.,
                y: 360.,
                delta: -12.,
                modifiers: Modifiers::default(),
            });
        }
        if has("--quick-controls") {
            // Explicitly show the editor after applying the other options.
            // Closing properties opened from a chooser restores that chooser;
            // a settings dropdown also needs its own Escape before Settings.
            for _ in 0..3 {
                key(&mut app, 27, false);
            }
        }
        if let Some(progress) = value("--motion")
            .and_then(|s| s.parse::<f32>().ok())
            .filter(|v| v.is_finite())
        {
            app.set_preview_motion(progress);
        }
        // One held gesture owns the transient circle preview. Opacity takes
        // precedence when both preview flags are supplied; both are validated.
        if let Some((index, value, min, max)) = opacity_preview
            .map(|v| (1., v, 0., 100.))
            .or_else(|| size_preview.map(|v| (0., v, 1., 512.)))
        {
            for _ in 0..3 {
                key(&mut app, 27, false);
            }
            let (group_x, group_width, group_y) = if has("--pin-panel") {
                let chooser_width = if family == Family::Shapes {
                    380_f32
                } else {
                    464_f32
                }
                .min((logical_width - 640.).max(244.));
                ((logical_width - chooser_width) / 2., chooser_width, 244.)
            } else {
                let width = (logical_width - 640.).clamp(244., 544.);
                ((logical_width - width) / 2., width, 136.)
            };
            let card_width = (group_width - 12.) / 2.;
            let track_x = group_x + index * (card_width + 12.) + 12.;
            let track_width = card_width - 76.;
            let x = track_x + track_width * (value - min) / (max - min);
            let y = group_y + 20.;
            app.event(Event::MouseDown {
                x,
                y,
                button: MouseButton::Left,
                modifiers: Modifiers::default(),
            });
            app.event(Event::MouseMove {
                x,
                y,
                modifiers: Modifiers::default(),
            });
            // Keep the real slider drag active through the final render.
        }
        // Finish layout previews with real gestures after the initial controls
        // have been addressed at their default positions. The groups move
        // independently: first the pinned chooser, then the main toolbar.
        if let Some((dx, dy)) = chooser_offset {
            let width = if family == Family::Shapes {
                380_f32
            } else {
                464_f32
            }
            .min((logical_width - 640.).max(244.));
            let x = (logical_width - width) / 2. + width - 22.;
            drag_panel(&mut app, x, 162., dx, dy);
        }
        if let Some((dx, dy)) = toolbar_offset {
            drag_panel(
                &mut app,
                bx + portfolio::app::TOOLBAR_GRIP_OFFSET + 10.,
                104.,
                dx,
                dy,
            );
        }
        let mut frame = app.render();
        for text in &mut frame.texts {
            text.x *= scale;
            text.y *= scale;
            text.width *= scale;
            text.height *= scale;
            text.size *= scale;
            if let Some((x, y, w, h)) = text.clip {
                text.clip = Some((x * scale, y * scale, w * scale, h * scale));
            }
        }
        if let Err(e) = portfolio::render::write_preview(&frame, std::path::Path::new(path)) {
            eprintln!("Preview error: {e}");
            std::process::exit(1);
        }
        return;
    }
    #[cfg(windows)]
    if let Err(error) = portfolio::platform::run(Box::new(PortfolioApp::new(true))) {
        eprintln!("Portfolio: {error}");
    }
    #[cfg(not(windows))]
    println!("Portfolio is a native Windows app. Build: cargo run --release. Checks: cargo test. Design render: cargo run -- --render-preview preview");
}
