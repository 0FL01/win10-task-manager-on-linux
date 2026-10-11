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

use egui_kittest::Harness;
use image::RgbaImage;
use mc_core::config::Config;
use mc_ui::{App, AppOptions, DataSource};
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
                            fonts: Default::default(),
                            initial_config: Some(Config {
                                window_selected_page: page.into(),
                                first_time_running: false,
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
                            fonts: Default::default(),
                            initial_config: Some(Config {
                                window_selected_page: "performance-page".into(),
                                first_time_running: false,
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
                            fonts: Default::default(),
                            initial_config: Some(Config {
                                window_selected_page: "apps-page".into(),
                                first_time_running: false,
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
        first_time_running: false,
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
        first_time_running: false,
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
