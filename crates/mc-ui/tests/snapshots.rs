/* crates/mc-ui/tests/snapshots.rs
 *
 * Copyright 2026 Mission Center Developers
 *
 * SPDX-License-Identifier: GPL-3.0-or-later
 */

//! Offscreen renders of the real UI on synthetic data at other sizes and scale factors, and of
//! the dialogs. Images are written to `target/ui-snapshots/` for visual review; the test fails if
//! rendering is impossible.

use std::collections::HashMap;
use std::path::PathBuf;

use egui_kittest::Harness;
use mc_core::config::Config;
use mc_ui::cx::Dialog;
use mc_ui::{App, AppOptions, DataSource};

fn out_dir() -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/ui-snapshots");
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

struct State {
    app: Option<App>,
    page: &'static str,
    dialog: Option<Option<Dialog>>,
    /// Start from settings written by the GNOME-style version (migrated by `App::new`)
    legacy: bool,
    /// Per-process network data unavailable (as reported without nethogs)
    no_network: bool,
    gpu: bool,
    /// Select the integrated adapter (amdgpu-like unified video engine)
    integrated: bool,
}

fn series(no_network: bool) -> Vec<mc_magpie::Readings> {
    let mut series = mc_magpie::synthetic_series(20, 120);
    if no_network {
        for r in &mut series {
            r.network_stats_error = Some(mc_magpie::NetworkStatsError::PermissionsError(
                Default::default(),
            ));
        }
    }
    series
}

fn render(
    name: &str,
    page: &'static str,
    size: (f32, f32),
    ppp: f32,
    dialog: Option<Option<Dialog>>,
) {
    let mut harness = Harness::builder()
        .with_size(egui::vec2(size.0, size.1))
        .with_pixels_per_point(ppp)
        .with_max_steps(400)
        .wgpu()
        .build_ui_state(
            |ui, state: &mut State| {
                if state.app.is_none() {
                    let ctx = ui.ctx().clone();
                    let mut config = if state.legacy {
                        Config {
                            window_selected_page: state.page.into(),
                            first_time_running: false,
                            window_width: 600,
                            window_height: 400,
                            performance_page_cpu_graph: 1,
                            performance_page_cpu_graph_bottom: 1,
                            layout_version: 0,
                            ..Default::default()
                        }
                    } else {
                        Config {
                            window_selected_page: state.page.into(),
                            first_time_running: false,
                            ..Default::default()
                        }
                    };
                    if state.gpu {
                        config.performance_selected_page = if state.integrated {
                            "gpu-0000:03:00.0".into()
                        } else {
                            "gpu-0000:01:00.0".into()
                        };
                        config.details_page_visible_columns =
                            "name;pid;cpu;gpu;gpu_engine;dedicated_gpu_memory;shared_gpu_memory"
                                .into();
                    }
                    state.app = Some(App::new(
                        &ctx,
                        AppOptions {
                            source: DataSource::Fixture(series(state.no_network), HashMap::new()),
                            config_path: None,
                            perf_overlay: false,
                            fonts: Default::default(),
                            initial_config: Some(config),
                        },
                    ));
                    if let Some(dialog) = state.dialog.take() {
                        state.app.as_mut().unwrap().open_dialog_for_test(dialog);
                    }
                    ctx.request_repaint();
                    return;
                }
                let app = state.app.as_mut().unwrap();
                app.logic(&ui.ctx().clone());
                app.ui(ui);
            },
            State {
                app: None,
                page,
                dialog,
                legacy: name.starts_with("legacy"),
                no_network: name.starts_with("no-network"),
                gpu: name.starts_with("gpu-"),
                integrated: name.starts_with("gpu-integrated"),
            },
        );

    harness.run_steps(1);
    for _ in 0..200 {
        harness.run_steps(1);
        if harness.state_mut().app.as_ref().unwrap().have_readings() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    std::thread::sleep(std::time::Duration::from_millis(300));
    harness.run_steps(8);
    if name == "gpu-performance" || name == "gpu-integrated-performance" {
        for _ in 0..22 {
            std::thread::sleep(std::time::Duration::from_millis(100));
            harness.run_steps(1);
        }
    }
    if name == "perf-context-submenu" {
        // Right-click the CPU page, then hover the first item ("Change Top Graph To")
        let at = egui::pos2(520., 250.);
        harness.input_mut().events.extend([
            egui::Event::PointerMoved(at),
            egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Secondary,
                pressed: true,
                modifiers: Default::default(),
            },
            egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Secondary,
                pressed: false,
                modifiers: Default::default(),
            },
        ]);
        harness.run_steps(4);
        harness
            .input_mut()
            .events
            .push(egui::Event::PointerMoved(at + egui::vec2(60., 14.)));
        harness.run_steps(6);
    }
    if name == "gpu-summary" {
        harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::M);
        harness.run_steps(2);
    }

    let image = harness.render().expect("offscreen wgpu render unavailable");
    image.save(out_dir().join(format!("{name}.png"))).unwrap();
    assert_eq!(image.width(), (size.0 * ppp) as u32);
}

#[test]
fn small_window_720x500() {
    render("small-720x500", "performance-page", (720., 500.), 1., None);
}

#[test]
fn hidpi_2x() {
    render("hidpi-2x-900x600", "apps-page", (900., 600.), 2., None);
}

#[test]
fn about_dialog() {
    render(
        "dialog-about",
        "apps-page",
        (1030., 705.),
        1.,
        Some(Some(Dialog::About)),
    );
}

#[test]
fn run_task_dialog() {
    render(
        "dialog-run-task",
        "apps-page",
        (1030., 705.),
        1.,
        Some(None),
    );
}

#[test]
fn legacy_settings_min_size_performance() {
    render(
        "legacy-min-760x540-performance",
        "performance-page",
        (760., 540.),
        1.,
        None,
    );
}

#[test]
fn min_size_processes() {
    render("min-760x540-processes", "apps-page", (760., 540.), 1., None);
}

#[test]
fn no_network_data_processes() {
    render("no-network-processes", "apps-page", (1030., 705.), 1., None);
}

#[test]
fn performance_context_submenu() {
    render(
        "perf-context-submenu",
        "performance-page",
        (1030., 705.),
        1.,
        None,
    );
}

#[test]
fn gpu_views_render_with_engine_choices_and_memory() {
    render("gpu-processes", "apps-page", (1030., 705.), 1., None);
    render("gpu-details", "details-page", (1030., 705.), 1., None);
    render(
        "gpu-performance",
        "performance-page",
        (1030., 705.),
        1.,
        None,
    );
    render("gpu-summary", "performance-page", (400., 380.), 1., None);
    render(
        "gpu-integrated-performance",
        "performance-page",
        (1030., 705.),
        1.,
        None,
    );
}
