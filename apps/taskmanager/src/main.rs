/* apps/taskmanager/src/main.rs
 *
 * Copyright 2026 Mission Center Developers
 *
 * This program is free software: you can redistribute it and/or modify
 * it under the terms of the GNU General Public License as published by
 * the Free Software Foundation, either version 3 of the License, or
 * (at your option) any later version.
 *
 * This program is distributed in the hope that it will be useful,
 * but WITHOUT ANY WARRANTY; without even the implied warranty of
 * MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
 * GNU General Public License for more details.
 *
 * You should have received a copy of the GNU General Public License
 * along with this program.  If not, see <http://www.gnu.org/licenses/>.
 *
 * SPDX-License-Identifier: GPL-3.0-or-later
 */

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use mc_core::config::Config;
use mc_ui::{App, AppOptions, DataSource};

const APP_ID: &str = "io.github.taskmanager.TaskManager";

struct Args {
    fixture: Option<PathBuf>,
    dump: Option<(usize, PathBuf)>,
    renderer: Option<String>,
    perf_overlay: bool,
    no_config: bool,
    stats_file: Option<PathBuf>,
    bench_seconds: Option<f64>,
    synthetic: Option<usize>,
}

fn usage() -> ! {
    println!(
        "taskmanager [--renderer wgpu|glow] [--fixture DIR] [--dump-readings N DIR]\n\
         \x20             [--synthetic PROCESSES] [--perf-overlay] [--no-config]\n\
         \x20             [--stats-file FILE] [--bench-seconds SECONDS]"
    );
    std::process::exit(0);
}

fn parse_args() -> Args {
    let mut args = Args {
        fixture: None,
        dump: None,
        renderer: None,
        perf_overlay: false,
        no_config: false,
        stats_file: None,
        bench_seconds: None,
        synthetic: None,
    };
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--fixture" => args.fixture = it.next().map(PathBuf::from),
            "--dump-readings" => {
                let n = it
                    .next()
                    .and_then(|n| n.parse().ok())
                    .unwrap_or_else(|| usage());
                let dir = it.next().map(PathBuf::from).unwrap_or_else(|| usage());
                args.dump = Some((n, dir));
            }
            "--renderer" => args.renderer = it.next(),
            "--perf-overlay" => args.perf_overlay = true,
            "--no-config" => args.no_config = true,
            "--stats-file" => args.stats_file = it.next().map(PathBuf::from),
            "--bench-seconds" => args.bench_seconds = it.next().and_then(|s| s.parse().ok()),
            "--synthetic" => args.synthetic = it.next().and_then(|s| s.parse().ok()),
            "-h" | "--help" => usage(),
            _ => {}
        }
    }
    args
}

fn locale_dir() -> PathBuf {
    std::env::var_os("MC_LOCALEDIR")
        .map(PathBuf::from)
        .or_else(|| option_env!("MC_LOCALEDIR").map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("/usr/share/locale"))
}

/// Records `n` snapshots from the real daemon into a fixture directory.
fn dump_readings(n: usize, dir: PathBuf) {
    let client = mc_magpie::MagpieClient::new(Arc::new(|| {}));
    let mut readings = Vec::with_capacity(n);
    let mut icons = HashMap::new();
    let deadline = Instant::now() + Duration::from_secs(30 + n as u64 * 3);
    while readings.len() < n && Instant::now() < deadline {
        while let Some(event) = client.try_event() {
            match event {
                mc_magpie::Event::IconsReady(i) => icons.extend(i),
                mc_magpie::Event::Fatal(msg) => {
                    eprintln!("{msg}");
                    std::process::exit(1);
                }
                _ => {}
            }
        }
        if let Some(r) = client.take_readings() {
            readings.push(r);
            eprintln!("captured {}/{n}", readings.len());
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    match mc_magpie::write_fixture_dir(&dir, &readings, &icons) {
        Ok(()) => println!("wrote {} readings to {}", readings.len(), dir.display()),
        Err(e) => {
            eprintln!("failed to write fixtures: {e}");
            std::process::exit(1);
        }
    }
}

struct Shell {
    app: App,
    stats_file: Option<PathBuf>,
    quit_at: Option<Instant>,
}

impl eframe::App for Shell {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.app.logic(ctx);
        // Checked on frames caused by new readings (every refresh interval); deliberately
        // no timer of its own so benchmarks do not add frames.
        if self.quit_at.is_some_and(|at| Instant::now() >= at) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.app.ui(ui);
    }

    #[cfg(feature = "glow")]
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.write_stats();
    }

    #[cfg(not(feature = "glow"))]
    fn on_exit(&mut self) {
        self.write_stats();
    }
}

impl Shell {
    fn write_stats(&self) {
        if let Some(path) = &self.stats_file {
            let (p50, p95) = self.app.stats.percentiles();
            let mut text = format!(
                "frames={}\npasses={}\nreadings_applied={}\nui_pass_p50_ms={p50:.3}\nui_pass_p95_ms={p95:.3}\n",
                self.app.stats.frames, self.app.stats.passes, self.app.stats.readings_applied
            );
            let mut causes: Vec<_> = self.app.stats.repaint_causes.iter().collect();
            causes.sort_by(|a, b| b.1.cmp(a.1));
            for (cause, n) in causes {
                text.push_str(&format!("cause {n} {cause}\n"));
            }
            let _ = std::fs::write(path, text);
        }
    }
}

fn main() -> eframe::Result {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
    let args = parse_args();

    if let Some((n, dir)) = args.dump {
        dump_readings(n, dir);
        return Ok(());
    }

    mc_core::i18n::init(&locale_dir());

    let source = if let Some(processes) = args.synthetic {
        DataSource::Fixture(mc_magpie::synthetic_series(60, processes), HashMap::new())
    } else {
        match &args.fixture {
            Some(dir) => match mc_magpie::load_fixture_dir(dir) {
                Ok((readings, icons)) => DataSource::Fixture(readings, icons),
                Err(e) => {
                    eprintln!("failed to load fixture {}: {e}", dir.display());
                    std::process::exit(1);
                }
            },
            None => DataSource::Magpie,
        }
    };

    let config_path = (!args.no_config).then(Config::default_path);
    if let Some(path) = &config_path {
        Config::migrate_legacy_location(path);
    }
    let mut initial = config_path
        .as_deref()
        .and_then(Config::load)
        .unwrap_or_default();
    // Same one-time migration as `App::new` (which also persists it): old 600×400 windows open
    // at the reference size
    let min = mc_ui::win10::metrics::MIN_WINDOW_SIZE;
    initial.migrate_layout(mc_ui::win10::metrics::min_window_size_i32());

    // The compact "Fewer details" view is restored with its own size
    let (inner, min_size, maximized) = if initial.window_fewer_details {
        let compact = mc_ui::win10::metrics::COMPACT_WINDOW_SIZE;
        (compact, mc_ui::win10::metrics::COMPACT_MIN_SIZE, false)
    } else {
        (
            [
                (initial.window_width as f32).max(min[0]),
                (initial.window_height as f32).max(min[1]),
            ],
            min,
            initial.is_maximized,
        )
    };
    let mut viewport = egui::ViewportBuilder::default()
        .with_title(mc_ui::APP_NAME)
        .with_app_id(APP_ID)
        .with_decorations(false)
        .with_min_inner_size(min_size)
        .with_inner_size(inner)
        .with_maximized(maximized);
    if let Some((rgba, width, height)) = mc_ui::icons::app_icon_rgba(128) {
        viewport = viewport.with_icon(egui::IconData {
            rgba,
            width,
            height,
        });
    }

    let mut native = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };
    match args.renderer.as_deref() {
        #[cfg(feature = "glow")]
        Some("glow") => native.renderer = eframe::Renderer::Glow,
        #[cfg(feature = "wgpu")]
        Some("wgpu") => native.renderer = eframe::Renderer::Wgpu,
        Some(other) => {
            eprintln!("renderer '{other}' is not compiled in");
            std::process::exit(2);
        }
        #[cfg(feature = "glow")]
        None => native.renderer = eframe::Renderer::Glow,
        #[cfg(not(feature = "glow"))]
        None => {}
    }

    let fonts = mc_ui::win10::fonts::Fonts::discover();
    let perf_overlay = args.perf_overlay;
    let stats_file = args.stats_file.clone();
    let quit_at = args
        .bench_seconds
        .map(|s| Instant::now() + Duration::from_secs_f64(s));
    eframe::run_native(
        mc_ui::APP_NAME,
        native,
        Box::new(move |cc| {
            let app = App::new(
                &cc.egui_ctx,
                AppOptions {
                    source,
                    config_path,
                    perf_overlay,
                    fonts,
                    initial_config: None,
                },
            );
            Ok(Box::new(Shell {
                app,
                stats_file,
                quit_at,
            }))
        }),
    )
}
