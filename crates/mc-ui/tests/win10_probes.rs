/* crates/mc-ui/tests/win10_probes.rs
 *
 * Copyright 2026 Mission Center Developers
 *
 * SPDX-License-Identifier: GPL-3.0-or-later
 */

//! Pixel probes of the Win10 look (docs/win10-spec/probes.json): every tab is rendered offscreen
//! at the reference size (1030×705, 1 px per point) on synthetic data, then colours at probe
//! points must match exactly and text must start within ±1 px of the specified position.
//! Renders are written to `target/ui-snapshots/win10-*.png`.

use std::collections::HashMap;
use std::path::PathBuf;

use egui_kittest::{Harness, kittest::Queryable};
use image::RgbaImage;
use mc_core::config::Config;
use mc_ui::{AlwaysOnTopMode, App, AppOptions, DataSource};
use serde_json::Value;

fn spec() -> Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../docs/win10-spec/probes.json");
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn render_with(
    page: &'static str,
    suffix: &str,
    size: (f32, f32),
    extra_wait_ms: u64,
    setup: fn(&mut App),
) -> RgbaImage {
    render_keys(page, suffix, size, extra_wait_ms, setup, &[])
}

/// Like [`render_with`]; `keys` are pressed once readings have arrived.
fn render_keys(
    page: &'static str,
    suffix: &str,
    size: (f32, f32),
    extra_wait_ms: u64,
    setup: fn(&mut App),
    keys: &[egui::Key],
) -> RgbaImage {
    let mut harness = Harness::builder()
        .with_size(egui::vec2(size.0, size.1))
        .with_pixels_per_point(1.)
        .with_max_steps(400)
        .wgpu()
        .build_ui_state(
            |ui, state: &mut Option<App>| {
                if state.is_none() {
                    let ctx = ui.ctx().clone();
                    *state = Some(App::new(
                        &ctx,
                        AppOptions {
                            source: DataSource::Fixture(
                                mc_magpie::synthetic_series(20, 120),
                                HashMap::new(),
                            ),
                            config_path: None,
                            perf_overlay: false,
                            always_on_top_mode: AlwaysOnTopMode::Viewport,
                            fonts: Default::default(),
                            initial_config: Some(Config {
                                window_selected_page: page.into(),
                                ..Default::default()
                            }),
                        },
                    ));
                    setup(state.as_mut().unwrap());
                    ctx.request_repaint();
                    return;
                }
                let app = state.as_mut().unwrap();
                app.logic(&ui.ctx().clone());
                app.ui(ui);
            },
            None,
        );

    harness.run_steps(1);
    for _ in 0..200 {
        harness.run_steps(1);
        if harness.state().as_ref().unwrap().have_readings() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    std::thread::sleep(std::time::Duration::from_millis(300));
    harness.run_steps(8);
    for key in keys {
        harness.key_press(*key);
        harness.run_steps(2);
    }
    for _ in 0..extra_wait_ms / 100 {
        std::thread::sleep(std::time::Duration::from_millis(100));
        harness.run_steps(1);
    }

    let image = harness.render().expect("offscreen wgpu render unavailable");
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/ui-snapshots");
    std::fs::create_dir_all(&dir).unwrap();
    image
        .save(dir.join(format!("win10-{page}{suffix}.png")))
        .unwrap();
    image
}

fn hex(image: &RgbaImage, x: u32, y: u32) -> String {
    let p = image.get_pixel(x, y);
    format!("#{:02x}{:02x}{:02x}", p[0], p[1], p[2])
}

/// Left, top and right edge of dark ink (luminance < 170) inside `[x0, y0, x1, y1]`.
fn ink_box(image: &RgbaImage, region: &[u32]) -> Option<(u32, u32, u32)> {
    let (mut min_x, mut min_y, mut max_x) = (u32::MAX, u32::MAX, 0);
    for y in region[1]..=region[3] {
        for x in region[0]..=region[2] {
            let p = image.get_pixel(x, y);
            let lum = (p[0] as u32 + p[1] as u32 + p[2] as u32) / 3;
            if lum < 170 {
                min_x = min_x.min(x);
                min_y = min_y.min(y);
                max_x = max_x.max(x);
            }
        }
    }
    (min_x != u32::MAX).then_some((min_x, min_y, max_x))
}

/// Checks the probes of `groups`; `frame_only` restricts colour probes to the window frame.
fn check(page: &'static str, groups: &[&str], frame_only: bool) {
    let spec = spec();
    // Frozen probes were measured with four resource columns. Exercise the new
    // default GPU layout separately rather than changing historical pixel evidence.
    let image = render_with(page, "", (1030., 705.), 0, |app| {
        app.configure_for_test(|config| {
            config.apps_page_visible_columns = "cpu;memory;drive;network".into()
        });
    });
    let mut failures = vec![];
    for group in groups {
        for probe in spec["probes"][group].as_array().unwrap() {
            if frame_only && probe.get("frame").is_none() {
                continue;
            }
            let (x, y) = (
                probe["x"].as_u64().unwrap() as u32,
                probe["y"].as_u64().unwrap() as u32,
            );
            let expected = probe["color"].as_str().unwrap();
            let actual = hex(&image, x, y);
            if actual != expected {
                failures.push(format!(
                    "{group} ({x},{y}) {}: expected {expected}, got {actual}",
                    probe["what"].as_str().unwrap()
                ));
            }
        }
        for text in spec["text"][group].as_array().unwrap() {
            let region: Vec<u32> = text["region"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_u64().unwrap() as u32)
                .collect();
            let (ex, ey) = (
                text["x"].as_u64().unwrap() as i64,
                text["y"].as_u64().unwrap() as i64,
            );
            // Right-aligned text with fixture-dependent content: compare the right edge instead
            let right = text.get("right").and_then(Value::as_u64).map(|r| r as i64);
            match ink_box(&image, &region) {
                Some((x, y, max_x))
                    if (right.map_or(x as i64 - ex, |r| max_x as i64 - r)).abs() <= 1
                        && (y as i64 - ey).abs() <= 1 => {}
                found => failures.push(format!(
                    "{group} text {}: expected ink at ({ex},{ey}) ±1, found {found:?}",
                    text["what"].as_str().unwrap()
                )),
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{} probe(s) failed:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
fn frame_processes() {
    check("apps-page", &["common", "processes"], true);
}

#[test]
fn frame_performance() {
    check("performance-page", &["common", "performance"], true);
}

#[test]
fn frame_details() {
    check("details-page", &["common", "details"], true);
}

#[test]
fn frame_services() {
    check("services-page", &["common", "services"], true);
}

#[test]
fn page_processes() {
    check("apps-page", &["common", "processes"], false);
}

#[test]
fn compact_fewer_details() {
    let image = render_with("apps-page", "-compact", (400., 380.), 0, |app| {
        app.set_fewer_details_for_test(true)
    });
    // Footer line 50 px above the bottom, no tab strip, disabled End task button
    assert_eq!(hex(&image, 200, 330), "#a0a0a0");
    assert_eq!(
        hex(&image, 200, 76),
        "#ffffff",
        "no tab strip line in compact mode"
    );
    assert_eq!(hex(&image, 309, 354), "#bfbfbf", "End task border");
    assert!(
        ink_box(&image, &[30, 32, 200, 60]).is_some(),
        "first app name"
    );
}

#[test]
fn page_performance() {
    check("performance-page", &["common", "performance"], false);
}

/// Visual check with several readings of history (not a probe): writes
/// `target/ui-snapshots/win10-performance-page-history.png`.
#[test]
#[ignore]
fn performance_history_snapshot() {
    render_with("performance-page", "-history", (1030., 705.), 6000, |_| {});
}

/// Visual check of every performance device page (not a probe).
#[test]
#[ignore]
fn performance_device_snapshots() {
    use egui::Key;
    for (n, key) in [Key::F2, Key::F3, Key::F4, Key::F5, Key::F6, Key::F7]
        .into_iter()
        .enumerate()
    {
        render_keys(
            "performance-page",
            &format!("-device{}", n + 2),
            (1030., 705.),
            3000,
            |_| {},
            &[key],
        );
    }
}

#[test]
fn page_details() {
    check("details-page", &["common", "details"], false);
}

#[test]
fn page_services() {
    check("services-page", &["common", "services"], false);
}

/// The device list separator of the Performance tab can be dragged wider; the width is saved.
#[test]
fn performance_list_separator_drags() {
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1030., 705.))
        .with_pixels_per_point(1.)
        .with_max_steps(400)
        .wgpu()
        .build_ui_state(
            |ui, state: &mut Option<App>| {
                if state.is_none() {
                    let ctx = ui.ctx().clone();
                    *state = Some(App::new(
                        &ctx,
                        AppOptions {
                            source: DataSource::Fixture(
                                mc_magpie::synthetic_series(20, 120),
                                HashMap::new(),
                            ),
                            config_path: None,
                            perf_overlay: false,
                            always_on_top_mode: AlwaysOnTopMode::Viewport,
                            fonts: Default::default(),
                            initial_config: Some(Config {
                                window_selected_page: "performance-page".into(),
                                ..Default::default()
                            }),
                        },
                    ));
                    ctx.request_repaint();
                    return;
                }
                let app = state.as_mut().unwrap();
                app.logic(&ui.ctx().clone());
                app.ui(ui);
            },
            None,
        );
    harness.run_steps(1);
    for _ in 0..200 {
        harness.run_steps(1);
        if harness.state().as_ref().unwrap().have_readings() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    harness.run_steps(4);

    let press = |pos: egui::Pos2, pressed: bool| egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    let start = egui::pos2(212., 400.);
    harness.event(egui::Event::PointerMoved(start));
    harness.run_steps(1);
    harness.event(press(start, true));
    harness.run_steps(1);
    for x in [230., 260., 300., 330.] {
        harness.event(egui::Event::PointerMoved(egui::pos2(x, 400.)));
        harness.run_steps(1);
    }
    harness.event(press(egui::pos2(330., 400.), false));
    harness.run_steps(1);
    harness.event(egui::Event::PointerMoved(egui::pos2(700., 600.)));
    harness.run_steps(3);

    // The list starts after the 1 px window border: separator at x 330 = width 329
    let width = harness
        .state()
        .as_ref()
        .unwrap()
        .config()
        .performance_sidebar_width;
    assert_eq!(width, 329);
    let image = harness.render().expect("offscreen wgpu render unavailable");
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/ui-snapshots");
    image
        .save(dir.join("win10-performance-wide-list.png"))
        .unwrap();
    assert_eq!(hex(&image, 330, 600), "#d9d9d9", "separator moved to x 330");
    assert_eq!(
        hex(&image, 212, 600),
        "#ffffff",
        "old separator position is free"
    );
    // Page geometry follows: graph grid starts at 241 + (330 − 212)
    assert_eq!(
        hex(&image, 241 + 118, 230),
        "#117dbb",
        "graph left border shifted"
    );
}

/// The Network header keeps sorting while it shows a tooltip.
#[test]
fn network_header_click_sorts() {
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1030., 705.))
        .with_pixels_per_point(1.)
        .wgpu()
        .build_ui_state(
            |ui, state: &mut Option<App>| {
                if state.is_none() {
                    let ctx = ui.ctx().clone();
                    *state = Some(App::new(
                        &ctx,
                        AppOptions {
                            source: DataSource::Fixture(
                                mc_magpie::synthetic_series(5, 50),
                                HashMap::new(),
                            ),
                            config_path: None,
                            perf_overlay: false,
                            always_on_top_mode: AlwaysOnTopMode::Viewport,
                            fonts: Default::default(),
                            initial_config: Some(Config {
                                window_selected_page: "apps-page".into(),
                                ..Default::default()
                            }),
                        },
                    ));
                    return;
                }
                let app = state.as_mut().unwrap();
                app.logic(&ui.ctx().clone());
                app.ui(ui);
            },
            None,
        );
    for _ in 0..50 {
        harness.run_steps(1);
        if harness.state().as_ref().unwrap().have_readings() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let at = egui::pos2(490., 110.);
    harness.event(egui::Event::PointerMoved(at));
    harness.run_steps(1);
    for pressed in [true, false] {
        harness.event(egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        });
        harness.run_steps(1);
    }
    harness.run_steps(2);
    assert_eq!(
        harness
            .state()
            .as_ref()
            .unwrap()
            .config()
            .apps_page_sorting_column_name,
        "network"
    );
}

fn app_harness(config_path: PathBuf) -> Harness<'static, Option<App>> {
    Harness::builder()
        .with_size(egui::vec2(1030., 705.))
        .with_pixels_per_point(1.)
        // Real frame pacing, so two clicks fall within the double-click window
        .with_step_dt(1. / 60.)
        .wgpu()
        .build_ui_state(
            move |ui, state: &mut Option<App>| {
                if state.is_none() {
                    let ctx = ui.ctx().clone();
                    *state = Some(App::new(
                        &ctx,
                        AppOptions {
                            source: DataSource::Fixture(
                                mc_magpie::synthetic_series(5, 50),
                                HashMap::new(),
                            ),
                            config_path: Some(config_path.clone()),
                            perf_overlay: false,
                            always_on_top_mode: AlwaysOnTopMode::Viewport,
                            fonts: Default::default(),
                            initial_config: None,
                        },
                    ));
                    return;
                }
                let app = state.as_mut().unwrap();
                app.logic(&ui.ctx().clone());
                app.ui(ui);
            },
            None,
        )
}

/// Performance acceptance uses one repeating sample: tooltip time must not change its bytes.
fn performance_harness(
    config: Config,
    reading: mc_magpie::Readings,
    pixels_per_point: f32,
) -> Harness<'static, Option<App>> {
    let config = Config {
        window_selected_page: "performance-page".into(),
        ..config
    };
    let mut harness = Harness::builder()
        .with_size(egui::vec2(
            config.window_width as f32,
            config.window_height as f32,
        ))
        .with_pixels_per_point(pixels_per_point)
        .with_step_dt(1. / 60.)
        .with_max_steps(400)
        .wgpu()
        .build_ui_state(
            move |ui, state: &mut Option<App>| {
                if state.is_none() {
                    let ctx = ui.ctx().clone();
                    *state = Some(App::new(
                        &ctx,
                        AppOptions {
                            source: DataSource::Fixture(vec![reading.clone()], HashMap::new()),
                            config_path: None,
                            perf_overlay: false,
                            always_on_top_mode: AlwaysOnTopMode::Viewport,
                            fonts: Default::default(),
                            initial_config: Some(config.clone()),
                        },
                    ));
                    ctx.request_repaint();
                    return;
                }
                let app = state.as_mut().unwrap();
                app.logic(&ui.ctx().clone());
                app.ui(ui);
            },
            None,
        );
    wait_ready(&mut harness);
    assert!(
        harness.state().as_ref().unwrap().have_readings(),
        "fixed performance fixture did not arrive within the bounded wait"
    );
    harness
}

fn wait_ready(harness: &mut Harness<'static, Option<App>>) {
    for _ in 0..100 {
        harness.run_steps(1);
        if harness.state().as_ref().is_some_and(|a| a.have_readings()) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    harness.run_steps(3);
}

fn button(harness: &mut Harness<'static, Option<App>>, pos: egui::Pos2, pressed: bool) {
    harness.event(egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    });
    harness.run_steps(1);
}

fn drag(harness: &mut Harness<'static, Option<App>>, from: egui::Pos2, to: egui::Pos2) {
    harness.event(egui::Event::PointerMoved(from));
    harness.run_steps(1);
    button(harness, from, true);
    for k in 1..=6 {
        let t = k as f32 / 6.;
        harness.event(egui::Event::PointerMoved(from + (to - from) * t));
        harness.run_steps(1);
    }
    button(harness, to, false);
}

fn click(harness: &mut Harness<'static, Option<App>>, pos: egui::Pos2) {
    harness.event(egui::Event::PointerMoved(pos));
    harness.run_steps(1);
    button(harness, pos, true);
    button(harness, pos, false);
}

/// Column widths, column order, sorting and collapsed groups survive closing and reopening.
#[test]
fn ui_state_survives_restart() {
    let dir = std::env::temp_dir().join(format!("mc-persist-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("config.toml");
    let first_config = mc_core::config::Config {
        window_selected_page: "apps-page".into(),
        ..Default::default()
    };
    first_config.save(&path).unwrap();

    {
        let mut h = app_harness(path.clone());
        wait_ready(&mut h);
        // Widen CPU: its separator 308 → 340
        drag(&mut h, egui::pos2(308., 110.), egui::pos2(340., 110.));
        // Network header (now 487–558) dropped onto Memory (341–412)
        drag(&mut h, egui::pos2(520., 110.), egui::pos2(370., 110.));
        // Sort by Disk (now 487–558)
        click(&mut h, egui::pos2(520., 110.));
        // Collapse "Background processes" (header row at y 246)
        let row = egui::pos2(100., 252.);
        h.event(egui::Event::PointerMoved(row));
        h.run_steps(1);
        for _ in 0..2 {
            button(&mut h, row, true);
            button(&mut h, row, false);
        }
        h.run_steps(3);
        // Dropping the harness drops the app and flushes the settings writer
    }

    let saved = mc_core::config::Config::load(&path).unwrap();
    assert!(
        saved.apps_page_column_widths.contains("cpu=104"),
        "{}",
        saved.apps_page_column_widths
    );
    assert!(
        saved
            .apps_page_column_order
            .starts_with("name;cpu;network;memory;drive"),
        "{}",
        saved.apps_page_column_order
    );
    assert_eq!(saved.apps_page_sorting_column_name, "drive");
    assert_eq!(
        saved.apps_page_sorting_order,
        mc_core::config::SortOrder::Descending
    );
    assert!(
        saved.apps_page_expanded.contains("!processes"),
        "{}",
        saved.apps_page_expanded
    );

    let mut h = app_harness(path);
    wait_ready(&mut h);
    let image = h.render().expect("offscreen wgpu render unavailable");
    let out = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/ui-snapshots");
    image.save(out.join("win10-restored-state.png")).unwrap();
    // CPU separator restored at 340 (header gradient), the old one at 308 is gone
    assert_ne!(hex(&image, 340, 110), "#ffffff");
    assert_eq!(hex(&image, 308, 110), "#ffffff");
    let _ = std::fs::remove_dir_all(&dir);
}

/// A resized window keeps its size after a restart (the size is read from the content area,
/// which is known on Wayland too).
#[test]
fn window_size_survives_restart() {
    let dir = std::env::temp_dir().join(format!("mc-size-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("config.toml");
    mc_core::config::Config {
        window_selected_page: "apps-page".into(),
        ..Default::default()
    }
    .save(&path)
    .unwrap();

    {
        let mut h = app_harness(path.clone());
        wait_ready(&mut h);
        h.set_size(egui::vec2(1200., 800.));
        h.run_steps(3);
        // A size below the minimum (e.g. while minimized) is not stored
        h.set_size(egui::vec2(100., 50.));
        h.run_steps(3);
    }
    let saved = mc_core::config::Config::load(&path).unwrap();
    assert_eq!((saved.window_width, saved.window_height), (1200, 800));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn performance_cpu_hover_preserves_menu_and_summary() {
    let mut h = performance_harness(
        Config {
            performance_page_cpu_graph: 2,
            performance_page_cpu_graph_bottom: 0,
            ..Default::default()
        },
        mc_magpie::synthetic_readings(0, 50),
        1.,
    );
    let out = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/ui-snapshots");
    std::fs::create_dir_all(&out).unwrap();
    let cpu0 = egui::pos2(300., 200.);
    let cpu1 = egui::pos2(500., 200.);
    h.hover_at(cpu0);
    h.run_steps(40);
    assert!(h.query_by_label("CPU 0").is_some());
    assert!(h.query_by_label("CPU 1").is_none());
    let first_tip = h.get_by_label("CPU 0").rect();
    let offset = first_tip.min - cpu0;
    assert!(
        (0. ..=12.).contains(&offset.x),
        "CPU tooltip near pointer: {first_tip:?}"
    );
    assert!(
        (12. ..=28.).contains(&offset.y),
        "CPU tooltip below pointer: {first_tip:?}"
    );
    let moved_cpu0 = cpu0 + egui::vec2(40., 20.);
    h.hover_at(moved_cpu0);
    h.run_steps(40);
    let moved_tip = h.get_by_label("CPU 0").rect();
    assert_eq!(
        moved_tip.min - first_tip.min,
        moved_cpu0 - cpu0,
        "tooltip follows the pointer inside the same graph cell"
    );
    h.hover_at(cpu1);
    h.run_steps(40);
    assert!(h.query_by_label("CPU 1").is_some());
    assert!(h.query_by_label("CPU 0").is_none());
    let image = h.render().expect("offscreen wgpu render unavailable");
    image
        .save(out.join("win10-performance-cpu-hover.png"))
        .unwrap();
    assert_eq!(hex(&image, 241, 200), "#117dbb", "CPU 0 left edge");
    assert_eq!(hex(&image, 433, 200), "#117dbb", "CPU 1 left edge");

    // Hover-only graph regions must leave the page's secondary click intact.
    for pressed in [true, false] {
        h.event(egui::Event::PointerButton {
            pos: cpu1,
            button: egui::PointerButton::Secondary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        });
        h.run_steps(1);
    }
    h.run_steps(4);
    for label in ["Change Top Graph To", "Graph summary view", "Copy"] {
        assert!(
            h.output().shapes.iter().any(|shape| {
                matches!(&shape.shape, egui::Shape::Text(text) if text.galley.job.text == label)
            }),
            "CPU context menu is missing {label}"
        );
    }
    h.render()
        .unwrap()
        .save(out.join("win10-performance-cpu-hover-menu.png"))
        .unwrap();
    h.key_press(egui::Key::Escape);
    h.run_steps(4);
    assert!(!h.output().shapes.iter().any(|shape| {
        matches!(&shape.shape, egui::Shape::Text(text) if text.galley.job.text == "Graph summary view")
    }));

    h.hover_at(cpu0);
    h.run_steps(40); // Start a fresh primary-click sequence after the secondary click.
    click(&mut h, cpu0);
    click(&mut h, cpu0);
    h.run_steps(4);
    assert_eq!(h.ctx.content_rect().size(), egui::vec2(400., 380.));
    let summary_cpu0 = egui::pos2(60., 60.);
    h.hover_at(summary_cpu0);
    h.run_steps(40);
    assert!(h.query_by_label("CPU 0").is_some(), "CPU hover in summary");
    let summary_offset = h.get_by_label("CPU 0").rect().min - summary_cpu0;
    assert!(
        (0. ..=12.).contains(&summary_offset.x),
        "summary tooltip near pointer"
    );
    assert!(
        (12. ..=28.).contains(&summary_offset.y),
        "summary tooltip below pointer"
    );
    let summary = h.render().expect("offscreen wgpu render unavailable");
    summary
        .save(out.join("win10-performance-cpu-summary-hover.png"))
        .unwrap();
    assert_eq!(hex(&summary, 13, 60), "#117dbb", "summary graph edge");
    click(&mut h, summary_cpu0);
    click(&mut h, summary_cpu0);
    h.run_steps(4);
    assert_eq!(h.ctx.content_rect().size(), egui::vec2(1030., 705.));
    h.remove_cursor();
    h.run_steps(3);
    let restored = h.render().expect("offscreen wgpu render unavailable");
    assert_eq!(hex(&restored, 212, 600), "#d9d9d9", "sidebar restored");
    assert_eq!(hex(&restored, 241, 200), "#117dbb", "CPU page restored");
    for x in 1..1029 {
        assert_eq!(
            hex(&restored, x, 655),
            "#ffffff",
            "no Performance footer separator at {x}"
        );
    }
}

#[test]
fn performance_memory_composition_regions_and_ram_hover() {
    use mc_core::format::{DataType, to_human_readable_nice};

    let reading = mc_magpie::synthetic_readings(0, 50);
    let mem = &reading.mem_info;
    let gib = 1024 * 1024 * 1024;
    let bytes = [
        mem.mem_total - mem.mem_available - gib,
        7 * gib,
        gib,
        gib / 2,
        mem.mem_free,
    ];
    assert_eq!(bytes.iter().sum::<u64>(), mem.mem_total);
    let values = bytes.map(|v| to_human_readable_nice(v as f32, &DataType::MemoryBytes));
    let overview = format!(
        "Used: {}\nCache: {}\nShared: {}\nBuff: {}\nFree: {}",
        values[0], values[1], values[2], values[3], values[4]
    );
    let mut h = performance_harness(
        Config {
            performance_selected_page: "memory".into(),
            ..Default::default()
        },
        reading,
        1.,
    );
    let image = h.render().expect("offscreen wgpu render unavailable");
    let out = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/ui-snapshots");
    std::fs::create_dir_all(&out).unwrap();
    image
        .save(out.join("win10-performance-memory-composition.png"))
        .unwrap();
    // The 52-point bar has exclusive bounds [241, 1005) × [389, 441); swap follows below.
    let edges = [241, 580, 747, 771, 783, 1005];
    for x in 241..1005 {
        for y in [389, 440] {
            assert_eq!(hex(&image, x, y), "#8b12ae", "composition frame ({x},{y})");
        }
        let expected = if edges[..5].contains(&x) || x == 1004 {
            "#8b12ae"
        } else if x < 783 {
            "#f4f2f4"
        } else {
            "#ffffff"
        };
        assert_eq!(
            hex(&image, x, 414),
            expected,
            "composition boundary/fill at {x}"
        );
    }
    assert_eq!(hex(&image, 1005, 414), "#ffffff", "exclusive right edge");
    assert_eq!(hex(&image, 990, 388), "#ffffff", "above composition");
    assert_eq!(hex(&image, 990, 441), "#ffffff", "below composition");

    for (index, (category, explanation)) in [
        ("Used", "MemAvailable"),
        ("Cache", "SReclaimable"),
        ("Shared", "Shmem"),
        ("Buff", "Buffers"),
        ("Free", "ZFS ARC"),
    ]
    .into_iter()
    .enumerate()
    {
        let x = (edges[index] + edges[index + 1]) as f32 / 2.;
        h.hover_at(egui::pos2(x, 414.));
        h.run_steps(40);
        let header = format!("{category} ({})\n\n", values[index]);
        let tooltip_node = h
            .query_by_label_contains(&header)
            .unwrap_or_else(|| panic!("no selected {category} tooltip at x={x}"));
        if index == 0 {
            let rect = tooltip_node.rect();
            let offset = rect.min - egui::pos2(x, 414.);
            assert!(
                (0. ..=12.).contains(&offset.x),
                "composition tooltip near pointer: {rect:?}"
            );
            assert!(
                (12. ..=28.).contains(&offset.y),
                "composition tooltip below pointer: {rect:?}"
            );
        }
        let tooltip = tooltip_node.value().unwrap();
        assert!(
            tooltip.starts_with(&header),
            "selected region must lead the tooltip"
        );
        assert!(tooltip.contains(explanation), "definition of {category}");
        assert!(tooltip.contains(&overview), "all-five byte overview");
    }
    h.render()
        .unwrap()
        .save(out.join("win10-performance-memory-composition-hover.png"))
        .unwrap();
    let ram_pointer = egui::pos2(300., 200.);
    h.hover_at(ram_pointer);
    h.run_steps(40);
    let ram_node = h.get_by_label_contains("MemAvailable");
    let offset = ram_node.rect().min - ram_pointer;
    assert!((0. ..=12.).contains(&offset.x), "RAM tooltip near pointer");
    assert!(
        (12. ..=28.).contains(&offset.y),
        "RAM tooltip below pointer"
    );
    let ram_tip = ram_node.value().unwrap();
    assert!(
        !ram_tip.starts_with("Used ("),
        "RAM graph has its own explanation"
    );
    h.render()
        .unwrap()
        .save(out.join("win10-performance-memory-ram-hover.png"))
        .unwrap();
    h.hover_at(egui::pos2(995., 200.));
    h.run_steps(40);
    let edge_tip = h.get_by_label_contains("MemAvailable").rect();
    assert!(
        h.ctx.content_rect().contains_rect(edge_tip),
        "long tooltip stays inside the window: {edge_tip:?}"
    );
    assert!(
        (12. ..=28.).contains(&(edge_tip.top() - 200.)),
        "edge fitting keeps the tooltip by the pointer, not at the graph bottom"
    );
    h.render()
        .unwrap()
        .save(out.join("win10-performance-memory-ram-hover-edge.png"))
        .unwrap();
}

#[test]
fn performance_memory_composition_hidpi_logical_geometry() {
    let mut h = performance_harness(
        Config {
            performance_selected_page: "memory".into(),
            ..Default::default()
        },
        mc_magpie::synthetic_readings(0, 50),
        2.,
    );
    assert_eq!(h.ctx.content_rect().size(), egui::vec2(1030., 705.));
    let image = h.render().expect("offscreen wgpu render unavailable");
    let out = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/ui-snapshots");
    std::fs::create_dir_all(&out).unwrap();
    image
        .save(out.join("win10-performance-memory-composition-2x.png"))
        .unwrap();
    assert_eq!(image.dimensions(), (2060, 1410));
    // Physical pixels verify the same logical bar, including two-pixel square lines.
    for x in 482..2010 {
        for y in [778, 779, 880, 881] {
            assert_eq!(hex(&image, x, y), "#8b12ae", "2x bar frame ({x},{y})");
        }
        let logical_x = x / 2;
        let expected = if [241, 580, 747, 771, 783, 1004].contains(&logical_x) {
            "#8b12ae"
        } else if logical_x < 783 {
            "#f4f2f4"
        } else {
            "#ffffff"
        };
        assert_eq!(hex(&image, x, 828), expected, "2x composition at {x}");
    }
    assert_eq!(hex(&image, 2010, 828), "#ffffff", "2x exclusive right edge");
    assert_eq!(hex(&image, 1980, 777), "#ffffff", "above 2x bar");
    assert_eq!(hex(&image, 1980, 882), "#ffffff", "below 2x bar");
}

#[test]
fn performance_memory_swap_block_and_visibility() {
    fn has_text(h: &Harness<'static, Option<App>>, label: &str) -> bool {
        h.output().shapes.iter().any(|shape| {
            matches!(&shape.shape, egui::Shape::Text(text) if text.galley.job.text == label)
        })
    }
    let out = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/ui-snapshots");
    std::fs::create_dir_all(&out).unwrap();
    let config = Config {
        performance_selected_page: "memory".into(),
        ..Default::default()
    };

    // Synthetic swap: 1 of 8 GiB used, zram0 holds 768 MiB in 256 MiB, no zswap pool.
    let mut h = performance_harness(config.clone(), mc_magpie::synthetic_readings(0, 50), 1.);
    let image = h.render().expect("offscreen wgpu render unavailable");
    image
        .save(out.join("win10-performance-memory-swap.png"))
        .unwrap();
    // The 20-point bar has exclusive bounds [241, 1005) × [478, 498); 1/8 ends at x 337.
    for x in 241..1005 {
        for y in [478, 497] {
            assert_eq!(hex(&image, x, y), "#8b12ae", "swap bar frame ({x},{y})");
        }
        let expected = if x == 241 || x == 337 || x == 1004 {
            "#8b12ae"
        } else if x < 337 {
            "#f4f2f4"
        } else {
            "#ffffff"
        };
        assert_eq!(hex(&image, x, 488), expected, "swap bar fill at {x}");
    }
    assert_eq!(hex(&image, 990, 477), "#ffffff", "above swap bar");
    assert!(has_text(&h, "Swap usage"));
    assert!(has_text(&h, "1.00 GiB / 8.00 GiB (13%)"));
    assert!(has_text(&h, "zram · RAM compression"));
    assert!(has_text(&h, "3.0×"));
    assert!(
        !has_text(&h, "zswap · Compressed cache"),
        "an empty zswap pool takes no row"
    );

    h.hover_at(egui::pos2(300., 488.));
    h.run_steps(40);
    let tooltip = h
        .get_by_label_contains("Logical swap slots")
        .value()
        .unwrap();
    assert!(tooltip.contains("/dev/zram0: 768 MiB of 4.00 GiB, priority 100"));
    assert!(tooltip.contains("/swapfile: 256 MiB of 4.00 GiB, priority -2"));
    h.hover_at(egui::pos2(300., 507.));
    h.run_steps(40);
    let tooltip = h.get_by_label_contains("zram (3.0×)").value().unwrap();
    assert!(tooltip.contains("Saved: 512 MiB"));
    h.render()
        .unwrap()
        .save(out.join("win10-performance-memory-swap-hover.png"))
        .unwrap();

    // Without swap the whole block, its caption and the menu toggle disappear.
    let mut reading = mc_magpie::synthetic_readings(0, 50);
    reading.mem_info.swap_total = 0;
    reading.mem_info.swap_free = 0;
    reading.mem_info.swap_devices.clear();
    let mut h = performance_harness(config, reading, 1.);
    let image = h.render().expect("offscreen wgpu render unavailable");
    image
        .save(out.join("win10-performance-memory-no-swap.png"))
        .unwrap();
    for label in [
        "Swap usage",
        "zram · RAM compression",
        "zswap · Compressed cache",
    ] {
        assert!(!has_text(&h, label), "{label} is hidden without swap");
    }
    // Composition takes the place right above the statistics: [464, 516).
    assert_eq!(hex(&image, 300, 464), "#8b12ae", "composition top");
    assert_eq!(hex(&image, 300, 515), "#8b12ae", "composition bottom");
    assert_eq!(
        hex(&image, 300, 516),
        "#ffffff",
        "nothing below composition"
    );
    h.hover_at(egui::pos2(600., 300.));
    h.event(egui::Event::PointerButton {
        pos: egui::pos2(600., 300.),
        button: egui::PointerButton::Secondary,
        pressed: true,
        modifiers: egui::Modifiers::NONE,
    });
    h.event(egui::Event::PointerButton {
        pos: egui::pos2(600., 300.),
        button: egui::PointerButton::Secondary,
        pressed: false,
        modifiers: egui::Modifiers::NONE,
    });
    h.run_steps(4);
    assert!(has_text(&h, "Graph summary view"), "context menu is open");
    assert!(!has_text(&h, "Show Swap Usage"));
}

#[test]
fn performance_disk_transfer_strip_and_graph_hover() {
    let mut h = performance_harness(
        Config {
            performance_selected_page: "disk-nvme0n1".into(),
            ..Default::default()
        },
        mc_magpie::synthetic_readings(0, 50),
        1.,
    );
    let out = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/ui-snapshots");
    std::fs::create_dir_all(&out).unwrap();
    for (name, size, lower_top, lower_bottom, right) in [
        ("default", egui::vec2(1030., 705.), 399, 465, 1004),
        ("minimum", egui::vec2(760., 540.), 234, 300, 734),
        ("tall", egui::vec2(1030., 785.), 479, 545, 1004),
    ] {
        h.set_size(size);
        h.remove_cursor();
        h.run_steps(4);
        let image = h.render().expect("offscreen wgpu render unavailable");
        image
            .save(out.join(format!("win10-performance-disk-{name}.png")))
            .unwrap();
        assert_eq!(lower_bottom - lower_top, 66);
        for y in lower_top..lower_bottom {
            assert_eq!(hex(&image, 241, y), "#4da60c", "{name} lower left at {y}");
            assert_eq!(
                hex(&image, right, y),
                "#4da60c",
                "{name} lower right at {y}"
            );
        }
        for x in 241..=right {
            assert_eq!(hex(&image, x, lower_top), "#4da60c", "{name} lower top");
            assert_eq!(
                hex(&image, x, lower_bottom - 1),
                "#4da60c",
                "{name} lower bottom"
            );
        }
        assert_eq!(hex(&image, 300, 151), "#4da60c", "{name} upper top");
        assert_eq!(
            hex(&image, 300, lower_top - 38),
            "#4da60c",
            "{name} upper bottom"
        );
        assert_eq!(
            hex(&image, 300, lower_top - 37),
            "#ffffff",
            "{name} gap after upper graph"
        );
        assert_eq!(
            hex(&image, 300, lower_bottom),
            "#ffffff",
            "{name} strip ends before stats"
        );

        if name == "default" {
            h.hover_at(egui::pos2(300., 200.));
            h.run_steps(40);
            assert!(
                h.query_by_label(
                    "Percentage of time the drive is processing read or write requests"
                )
                .is_some()
            );
            let rect = h
                .get_by_label("Percentage of time the drive is processing read or write requests")
                .rect();
            let offset = rect.min - egui::pos2(300., 200.);
            assert!(
                (0. ..=12.).contains(&offset.x),
                "Disk active-time tooltip near pointer"
            );
            assert!(
                (12. ..=28.).contains(&offset.y),
                "Disk active-time tooltip below pointer"
            );
            h.hover_at(egui::pos2(300., 430.));
            h.run_steps(40);
            assert!(
                h.query_by_label("Read and write activity on this drive")
                    .is_some()
            );
            let rect = h
                .get_by_label("Read and write activity on this drive")
                .rect();
            let offset = rect.min - egui::pos2(300., 430.);
            assert!(
                (0. ..=12.).contains(&offset.x),
                "Disk transfer tooltip near pointer"
            );
            assert!(
                (12. ..=28.).contains(&offset.y),
                "Disk transfer tooltip below pointer"
            );
            h.render()
                .unwrap()
                .save(out.join("win10-performance-disk-transfer-hover.png"))
                .unwrap();
        }
    }
}

#[test]
fn performance_memory_minimum_overflow_scroll_and_reset() {
    use mc_core::format::{DataType, to_human_readable_nice};

    let mut reading = mc_magpie::synthetic_readings(0, 50);
    reading.mem_info.zswapped = 32 * 1024 * 1024;
    reading.mem_info.zswap = 16 * 1024 * 1024;
    let h_bytes = |v: u64| to_human_readable_nice(v as f32, &DataType::MemoryBytes);
    let zswap_detail = format!(
        "{} in {} RAM",
        h_bytes(reading.mem_info.zswapped),
        h_bytes(reading.mem_info.zswap)
    );
    let mut h = performance_harness(
        Config {
            window_width: 760,
            window_height: 540,
            performance_selected_page: "memory".into(),
            performance_page_memory_swap_visible: true,
            ..Default::default()
        },
        reading,
        1.,
    );
    let out = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/ui-snapshots");
    std::fs::create_dir_all(&out).unwrap();
    let initial = h.render().expect("offscreen wgpu render unavailable");
    initial
        .save(out.join("win10-performance-memory-minimum-extras.png"))
        .unwrap();
    assert_eq!(
        hex(&initial, 300, 329),
        "#8b12ae",
        "initial composition top"
    );
    assert_eq!(
        hex(&initial, 300, 380),
        "#8b12ae",
        "initial 52-point composition bottom"
    );
    assert_eq!(hex(&initial, 241, 151), "#8b12ae", "52-point RAM top");
    assert_eq!(hex(&initial, 241, 202), "#8b12ae", "52-point RAM bottom");
    assert_eq!(hex(&initial, 241, 240), "#8b12ae", "52-point Swap top");
    assert_eq!(hex(&initial, 241, 291), "#8b12ae", "52-point Swap bottom");
    let in_use = h
        .output()
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::Shape::Text(text) if text.galley.job.text == "In use" => Some(text.pos),
            _ => None,
        })
        .unwrap();
    assert_eq!(in_use.y, 500., "stats start at 498, caption at 503");
    assert_eq!(hex(&initial, 241, 418), "#8b12ae", "swap bar top");
    assert_eq!(hex(&initial, 241, 437), "#8b12ae", "swap bar bottom");

    h.hover_at(egui::pos2(300., 350.));
    h.run_steps(40);
    assert!(h.query_by_label_contains("Used (").is_some());
    h.event(egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: egui::vec2(0., -1000.),
        phase: egui::TouchPhase::Move,
        modifiers: egui::Modifiers::NONE,
    });
    h.run_steps(40);
    assert!(
        h.query_by_label_contains("Used (").is_none(),
        "scrolled bar no longer under cursor"
    );
    h.hover_at(egui::pos2(300., 76.));
    h.run_steps(40);
    assert!(
        h.query_by_label_contains("Used (").is_none(),
        "no tooltip outside the clipped page"
    );
    let scrolled = h.render().expect("offscreen wgpu render unavailable");
    scrolled
        .save(out.join("win10-performance-memory-minimum-scrolled.png"))
        .unwrap();
    // Virtual 536 - viewport 413 = 123 points of reachable overflow.
    assert_eq!(
        hex(&scrolled, 300, 329 - 123),
        "#8b12ae",
        "scroll clamps at 123"
    );
    assert_eq!(
        hex(&scrolled, 300, 380 - 123),
        "#8b12ae",
        "scrolled composition bottom"
    );
    for label in [
        "In use",
        "Cached",
        "zram · RAM compression",
        "zswap · Compressed cache",
        zswap_detail.as_str(),
    ] {
        let (clip, text) = h
            .output()
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Text(text) if text.galley.job.text == label => {
                    Some((shape.clip_rect, text))
                }
                _ => None,
            })
            .unwrap();
        assert!(
            clip.contains(text.pos) && clip.contains(text.pos + text.galley.size()),
            "{label} is inside the page clip"
        );
        assert!(
            ink_box(
                &scrolled,
                &[
                    text.pos.x as u32,
                    text.pos.y as u32,
                    (text.pos.x + text.galley.size().x).ceil() as u32,
                    (text.pos.y + text.galley.size().y).ceil() as u32,
                ]
            )
            .is_some(),
            "{label} has rendered ink"
        );
    }
    for label in ["Swap used", "Swap available", "Compressed", "Savings"] {
        assert!(
            !h.output().shapes.iter().any(|shape| {
                matches!(&shape.shape, egui::Shape::Text(text) if text.galley.job.text == label)
            }),
            "{label} moved from the statistics into the swap block"
        );
    }
    assert_eq!(hex(&scrolled, 212, 400), "#d9d9d9", "sidebar remains fixed");
    assert_eq!(
        hex(&scrolled, 300, 490),
        "#ffffff",
        "Performance footer has no separator"
    );

    h.set_size(egui::vec2(1030., 705.));
    h.remove_cursor();
    h.run_steps(4);
    let resized = h.render().expect("offscreen wgpu render unavailable");
    resized
        .save(out.join("win10-performance-memory-extras-resized.png"))
        .unwrap();
    assert_eq!(
        hex(&resized, 241, 151),
        "#8b12ae",
        "resize clamps overflow to zero"
    );
    assert!(
        h.output().shapes.iter().any(|shape| {
            matches!(&shape.shape, egui::Shape::Text(text)
            if text.galley.job.text == "Memory" && text.pos == egui::pos2(238., 95.))
        }),
        "resize restores the title's unscrolled position"
    );
    assert!(
        ink_box(&resized, &[238, 102, 340, 128]).is_some(),
        "unscrolled Memory title"
    );
    h.set_size(egui::vec2(760., 540.));
    h.run_steps(4);
    h.hover_at(egui::pos2(500., 420.));
    h.event(egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: egui::vec2(0., -1000.),
        phase: egui::TouchPhase::Move,
        modifiers: egui::Modifiers::NONE,
    });
    h.run_steps(3);
    h.key_press(egui::Key::F1);
    h.run_steps(4);
    assert_eq!(
        h.state()
            .as_ref()
            .unwrap()
            .config()
            .performance_selected_page,
        "cpu"
    );
    h.key_press(egui::Key::F2);
    h.run_steps(4);
    assert_eq!(
        h.state()
            .as_ref()
            .unwrap()
            .config()
            .performance_selected_page,
        "memory"
    );
    let returned = h.render().expect("offscreen wgpu render unavailable");
    assert_eq!(
        hex(&returned, 300, 329),
        "#8b12ae",
        "Memory → CPU → Memory resets scroll"
    );

    h.event(egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: egui::vec2(0., -1000.),
        phase: egui::TouchPhase::Move,
        modifiers: egui::Modifiers::NONE,
    });
    h.run_steps(3);
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::M);
    h.run_steps(4);
    assert_eq!(h.ctx.content_rect().size(), egui::vec2(400., 380.));
    assert!(!h.output().shapes.iter().any(|shape| {
        matches!(&shape.shape, egui::Shape::Text(text) if text.galley.job.text == "Memory composition")
    }), "summary contains graphs without composition");
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::M);
    h.run_steps(4);
    assert_eq!(h.ctx.content_rect().size(), egui::vec2(760., 540.));
    let returned = h.render().expect("offscreen wgpu render unavailable");
    assert_eq!(
        hex(&returned, 300, 329),
        "#8b12ae",
        "summary transition resets scroll"
    );
}
