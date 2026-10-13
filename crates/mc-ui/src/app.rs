/* crates/mc-ui/src/app.rs
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

//! Application shell: state, data intake, layout and command execution.

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use egui::{Align, Context, Key, KeyboardShortcut, Layout, Modifiers, RichText, Ui};
use mc_core::config::{Config, ConfigWriter};
use mc_core::i18n::i18n;
use mc_core::i18n::n_;
use mc_magpie::{Event, INTERVAL_STEP, Icon, MagpieClient, Readings, Request};

use crate::always_on_top::AlwaysOnTopState;
use crate::cx::{Command, Cx, Dialog, MainPage};
use crate::dialogs::Dialogs;
use crate::icons::IconCache;
use crate::perf::PerformancePage;
use crate::table::{TableKind, TableView};
use crate::win10::details::DetailsPage;
use crate::win10::performance::PerformanceView;
use crate::win10::processes::ProcessesPage;
use crate::win10::services::ServicesView;
use crate::win10::{chrome, menu, metrics};
use crate::{AlwaysOnTopError, AlwaysOnTopMode, AlwaysOnTopRequest, AlwaysOnTopResult};

pub enum DataSource {
    Magpie,
    Fixture(Vec<Readings>, HashMap<String, Icon>),
}

pub struct AppOptions {
    pub source: DataSource,
    /// `None` keeps settings in memory only (tests, benchmarks).
    pub config_path: Option<PathBuf>,
    pub perf_overlay: bool,
    /// Fonts discovered before startup; the default uses egui's bundled fonts only.
    pub fonts: crate::win10::fonts::Fonts,
    /// Settings to start with when `config_path` is `None`.
    pub initial_config: Option<Config>,
    /// Native shell capability; external window control is acknowledged asynchronously.
    pub always_on_top_mode: AlwaysOnTopMode,
}

#[derive(Default)]
pub struct PerfStats {
    pub ui_pass_ms: VecDeque<f32>,
    /// `ui()` invocations (egui may run several passes per frame)
    pub passes: u64,
    /// Frames actually rendered
    pub frames: u64,
    pub readings_applied: u64,
    /// Why egui asked for another frame, by source location
    pub repaint_causes: HashMap<String, u64>,
}

impl PerfStats {
    fn record(&mut self, ms: f32) {
        if self.ui_pass_ms.len() >= 240 {
            self.ui_pass_ms.pop_front();
        }
        self.ui_pass_ms.push_back(ms);
        self.passes += 1;
    }

    pub fn percentiles(&self) -> (f32, f32) {
        if self.ui_pass_ms.is_empty() {
            return (0., 0.);
        }
        let mut v: Vec<f32> = self.ui_pass_ms.iter().copied().collect();
        v.sort_by(|a, b| a.total_cmp(b));
        let p = |q: f32| v[((v.len() - 1) as f32 * q).round() as usize];
        (p(0.5), p(0.95))
    }
}

pub struct App {
    config: Config,
    saved_config: Config,
    writer: Option<ConfigWriter>,
    always_on_top: AlwaysOnTopState,
    launch_request: Option<[bool; 2]>,
    launch_pending: bool,
    launch_status: String,
    hardware_pending: bool,
    hardware_status: String,
    hardware_notice: bool,
    magpie: MagpieClient,
    icons: IconCache,

    page: MainPage,
    perf: PerformancePage,
    gpu_indices: mc_core::gpu::GpuIndices,
    apps: TableView,
    processes: ProcessesPage,
    performance: PerformanceView,
    details: DetailsPage,
    services_view: ServicesView,
    services: TableView,
    dialogs: Dialogs,

    full_size: Option<egui::Vec2>,

    grid_offset: u32,
    stashed: Option<Readings>,
    have_readings: bool,
    /// Apply the next snapshot even while paused (Refresh now)
    apply_next: bool,
    /// `TableView::expanded_version` last written to the settings
    saved_expanded_version: u64,
    /// Text of the open "Create new task" dialog
    run_task: Option<String>,
    reduced: bool,

    pub stats: PerfStats,
    show_overlay: bool,
    fatal: Option<String>,
    quit: bool,
}

impl App {
    pub fn new(ctx: &Context, options: AppOptions) -> Self {
        let mut config = options
            .config_path
            .as_deref()
            .and_then(Config::load)
            .or(options.initial_config)
            .unwrap_or_default();
        config.sanitize();
        // Settings from older versions get the Win10 layout defaults once; keeping the
        // pre-migration copy as "saved" makes the first `persist_config` write them
        let loaded = config.clone();
        let migrated = config.migrate_layout(metrics::min_window_size_i32());

        let waker_ctx = ctx.clone();
        // A non-zero delay makes egui render exactly one frame per wake-up; a zero delay
        // (`request_repaint()`) always schedules a second "settling" frame.
        let waker: Arc<dyn Fn() + Send + Sync> =
            Arc::new(move || waker_ctx.request_repaint_after(Duration::from_millis(1)));
        let magpie = match options.source {
            DataSource::Magpie => MagpieClient::new(waker.clone()),
            DataSource::Fixture(readings, icons) => {
                MagpieClient::from_fixtures(readings, icons, waker.clone())
            }
        };
        magpie.set_update_speed(config.app_update_interval);
        if config.extended_hardware_monitoring {
            magpie.send(mc_magpie::Request::SetHardwareAccess {
                enabled: true,
                interactive: false,
            });
        }

        ctx.set_fonts(options.fonts.definitions());
        crate::win10::style::install(ctx);

        let perf = PerformancePage::new(&config);
        let apps = TableView::new(TableKind::Apps, &mut config);
        let services = TableView::new(TableKind::Services, &mut config);
        let page = MainPage::from_settings_name(&config.window_selected_page);
        let mut details = DetailsPage::default();
        details.restore(&config);
        let mut services_view = ServicesView::default();
        services_view.restore(&config);
        let always_on_top =
            AlwaysOnTopState::new(options.always_on_top_mode, config.window_always_on_top);
        if options.always_on_top_mode == AlwaysOnTopMode::Viewport && config.window_always_on_top {
            ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(
                egui::WindowLevel::AlwaysOnTop,
            ));
        }
        // Paused at startup still shows the first snapshot
        let apply_next = config.app_update_paused;
        let expanded_version = apps.expanded_version;

        Self {
            saved_config: if migrated { loaded } else { config.clone() },
            writer: options.config_path.map(ConfigWriter::new),
            always_on_top,
            launch_request: None,
            launch_pending: false,
            launch_status: String::new(),
            hardware_pending: false,
            hardware_status: String::new(),
            hardware_notice: false,
            config,
            magpie,
            icons: IconCache::new(waker),
            page,
            perf,
            gpu_indices: Default::default(),
            apps,
            processes: ProcessesPage::default(),
            performance: PerformanceView::default(),
            details,
            services_view,
            saved_expanded_version: expanded_version,
            services,
            dialogs: Dialogs::default(),
            full_size: None,
            grid_offset: 0,
            stashed: None,
            have_readings: false,
            apply_next,
            run_task: None,
            reduced: false,
            stats: PerfStats::default(),
            show_overlay: options.perf_overlay,
            fatal: None,
            quit: false,
        }
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    pub fn take_launch_request(&mut self) -> Option<[bool; 2]> {
        self.launch_request.take()
    }

    pub fn finish_launch_shortcuts(&mut self, result: Result<[bool; 2], String>) {
        self.launch_pending = false;
        match result {
            Ok(settings) => {
                self.config.launch_ctrl_shift_escape = settings[0];
                self.config.launch_ctrl_alt_delete = settings[1];
                self.launch_status = i18n("Launch shortcuts saved in the desktop settings.");
                self.persist_config();
            }
            Err(error) => self.launch_status = error,
        }
    }

    /// Takes the next native-shell request once, retaining its pending state until acknowledgement.
    pub fn take_always_on_top_request(&mut self) -> Option<AlwaysOnTopRequest> {
        self.always_on_top.take_request()
    }

    /// Applies a native-shell acknowledgement; stale results never change state or preferences.
    pub fn finish_always_on_top(&mut self, result: AlwaysOnTopResult) {
        if let Some(enabled) = self.always_on_top.finish(result) {
            self.config.window_always_on_top = enabled;
            self.persist_config();
        }
    }

    /// Adjusts the in-memory settings (tests and benchmarks).
    pub fn configure_for_test(&mut self, f: impl FnOnce(&mut Config)) {
        f(&mut self.config);
        self.page = MainPage::from_settings_name(&self.config.window_selected_page);
        self.perf.selected = self.config.performance_selected_page.clone();
    }

    /// Opens a dialog (tests); `None` opens File → Run new task.
    pub fn open_dialog_for_test(&mut self, dialog: Option<Dialog>) {
        match dialog {
            Some(dialog) => self.dialogs.open(dialog, &self.magpie),
            None => self.run_task = Some("htop".into()),
        }
    }

    /// Shows the compact "Fewer details" view without resizing the window (tests).
    pub fn set_fewer_details_for_test(&mut self, on: bool) {
        self.config.window_fewer_details = on;
    }

    pub fn have_readings(&self) -> bool {
        self.have_readings
    }

    fn refresh_interval(&self) -> Duration {
        Duration::from_secs_f64(self.config.app_update_interval as f64 * INTERVAL_STEP)
    }

    fn apply_readings(&mut self, mut readings: Readings) {
        if !self.config.extended_hardware_monitoring {
            readings.hardware = None;
        }
        let interval_ms = self.refresh_interval().as_millis() as u64;
        let gpu =
            mc_core::gpu::GpuSnapshot::from_readings(&readings, &mut self.gpu_indices, interval_ms);
        self.perf.update(&readings, &gpu, &mut self.config);
        if !self.have_readings {
            self.magpie.send(mc_magpie::Request::AboutSystem);
        }
        if readings.includes_app_data {
            self.apps.update(&readings, &gpu);
            self.services.update(&readings, &gpu);
            self.details.update(&readings, &gpu);
            self.services_view.update(&self.services);
        }
        self.grid_offset = self.grid_offset.wrapping_add(1);
        self.perf.set_grid_offset(self.grid_offset);
        self.have_readings = true;
        self.stats.readings_applied += 1;
    }

    fn handle_event(&mut self, event: Event) {
        match event {
            Event::IconsReady(icons) => self.icons.merge_app_icons(icons),
            Event::Fatal(message) => self.fatal = Some(message),
            Event::Spd { privileged, result } => {
                if !privileged || self.config.extended_hardware_monitoring {
                    self.perf.update_spd(privileged, &result);
                } else {
                    self.perf.clear_privileged_spd();
                }
            }
            Event::HardwareAccess(result) => {
                self.hardware_pending = false;
                match result {
                    Ok(enabled) => {
                        self.config.extended_hardware_monitoring = enabled;
                        self.hardware_status = if enabled {
                            i18n("Hardware access enabled.")
                        } else {
                            i18n("Hardware access disabled.")
                        };
                    }
                    Err(error) => {
                        self.config.extended_hardware_monitoring = false;
                        self.hardware_status = error;
                        self.hardware_notice = true;
                    }
                }
                self.apply_next = true;
                if !self.config.extended_hardware_monitoring {
                    self.perf.clear_privileged_spd();
                }
                self.magpie.send(mc_magpie::Request::RefreshNow);
            }
            Event::About(about) => {
                self.perf.update_about(&about);
                self.dialogs
                    .handle_event(Event::About(about), &self.apps, &self.magpie);
            }
            other => self.dialogs.handle_event(other, &self.apps, &self.magpie),
        }
    }

    /// Non-visual per-frame work; runs even when the window is hidden.
    pub fn logic(&mut self, ctx: &Context) {
        ctx.set_theme(match self.config.theme {
            mc_core::config::Theme::Light => egui::ThemePreference::Light,
            mc_core::config::Theme::Dark => egui::ThemePreference::Dark,
            mc_core::config::Theme::System => egui::ThemePreference::System,
        });
        while let Some(event) = self.magpie.try_event() {
            self.handle_event(event);
        }

        let held = ctx.input(|i| i.modifiers.command_only() && i.keys_down.is_empty())
            && ctx.input(|i| i.viewport().focused.unwrap_or(false));
        let paused = held || (self.config.app_update_paused && !self.apply_next);
        if let Some(readings) = self.magpie.take_readings() {
            if paused {
                self.stashed = Some(readings);
            } else {
                self.apply_next = false;
                self.apply_readings(readings);
            }
        } else if !paused && let Some(readings) = self.stashed.take() {
            self.apply_readings(readings);
        }

        self.icons.poll(ctx);

        // Reduced activity: minimized and not focused
        let (minimized, focused) = ctx.input(|i| {
            (
                i.viewport().minimized.unwrap_or(false),
                i.viewport().focused.unwrap_or(true),
            )
        });
        let reduced = minimized && !focused;
        if reduced != self.reduced {
            self.reduced = reduced;
            self.magpie.set_reduced_activity(reduced);
            log::debug!(target: "TaskManager::Window", "Reduced activity: {reduced}");
        }

        // Window geometry persistence. The size comes from the content area: `inner_rect` needs
        // the window position, which Wayland does not report, so it is always `None` there
        let (maximized, size) = ctx.input(|i| (i.viewport().maximized, i.content_rect().size()));
        if let Some(max) = maximized {
            self.config.is_maximized = max;
        }
        let min = metrics::MIN_WINDOW_SIZE;
        if maximized != Some(true)
            && !minimized
            && !self.config.window_fewer_details
            && !self.perf.summary_mode
            && size.x >= min[0]
            && size.y >= min[1]
        {
            self.config.window_width = size.x.round() as i32;
            self.config.window_height = size.y.round() as i32;
        }

        if self.apps.expanded_version != self.saved_expanded_version {
            self.saved_expanded_version = self.apps.expanded_version;
            self.config.apps_page_expanded = self.apps.persistent_expanded();
        }
        self.persist_config();

        if self.quit {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    fn persist_config(&mut self) {
        if self.config == self.saved_config {
            return;
        }
        if self.config.app_update_interval != self.saved_config.app_update_interval {
            self.magpie
                .set_update_speed(self.config.app_update_interval);
        }
        if let Some(writer) = &self.writer {
            writer.save(&self.config);
        }
        self.saved_config = self.config.clone();
    }

    fn execute(&mut self, ctx: &Context, command: Command) {
        match command {
            Command::Magpie(request) => {
                self.magpie.send(request);
            }
            Command::CopyText(text) => ctx.copy_text(text),
            Command::OpenUrl(url) => ctx.open_url(egui::OpenUrl::new_tab(url)),
            Command::OpenDialog(dialog) => self.dialogs.open(dialog, &self.magpie),
            Command::SetPage(page) => self.set_page(page),
            Command::SetAlwaysOnTop(enabled) => match self.always_on_top.mode {
                AlwaysOnTopMode::Viewport => {
                    ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(if enabled {
                        egui::WindowLevel::AlwaysOnTop
                    } else {
                        egui::WindowLevel::Normal
                    }));
                    self.config.window_always_on_top = enabled;
                    self.persist_config();
                }
                AlwaysOnTopMode::External => self.always_on_top.request(enabled),
                AlwaysOnTopMode::Unavailable => {}
            },
            Command::SetLaunchShortcuts(settings) => {
                if !self.launch_pending {
                    self.launch_pending = true;
                    self.launch_request = Some(settings);
                }
            }
            Command::SetHardwareAccess(enabled) => {
                if !self.hardware_pending {
                    self.hardware_notice = true;
                    self.hardware_status = if enabled {
                        i18n("Waiting for hardware authorization…")
                    } else {
                        i18n("Disabling hardware access…")
                    };
                    self.hardware_pending =
                        self.magpie.send(mc_magpie::Request::SetHardwareAccess {
                            enabled,
                            interactive: true,
                        });
                    if !self.hardware_pending {
                        self.hardware_status =
                            i18n("The hardware request could not be queued. Try again.");
                    }
                }
            }
            Command::InstallHardwareService => {
                if !self.hardware_pending {
                    self.hardware_notice = true;
                    self.hardware_status = i18n("Waiting for service installation authorization…");
                    self.hardware_pending = self.magpie.send(Request::InstallHardwareService);
                    if !self.hardware_pending {
                        self.hardware_status =
                            i18n("The hardware request could not be queued. Try again.");
                    }
                }
            }
            Command::ToggleSummaryMode => {
                self.perf.summary_mode = !self.perf.summary_mode;
                let size = if self.perf.summary_mode {
                    self.full_size = ctx.input(|i| i.viewport().inner_rect.map(|r| r.size()));
                    egui::Vec2::from(metrics::COMPACT_WINDOW_SIZE)
                } else {
                    self.full_size.unwrap_or(egui::vec2(
                        self.config.window_width as f32,
                        self.config.window_height as f32,
                    ))
                };
                ctx.send_viewport_cmd(egui::ViewportCommand::MinInnerSize(egui::Vec2::from(
                    if self.perf.summary_mode {
                        metrics::COMPACT_MIN_SIZE
                    } else {
                        metrics::MIN_WINDOW_SIZE
                    },
                )));
                ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(size));
            }
            Command::SelectDeviceCategory(action) => {
                self.perf.select_category(action, &mut self.config)
            }
            Command::SelectDevice(name) => self.perf.select(&name, &mut self.config),
            Command::OpenNetworkSettings(wireless) => open_network_settings(wireless),
            Command::RunTask(command) => run_task(command),
            Command::RefreshNow => {
                self.apply_next = true;
                self.magpie.send(Request::RefreshNow);
            }
            Command::Quit => self.quit = true,
        }
    }

    fn set_page(&mut self, page: MainPage) {
        if page != MainPage::Performance && self.perf.summary_mode {
            return;
        }
        self.page = page;
        self.config.window_selected_page = page.settings_name().into();
    }

    fn shortcuts(&mut self, ctx: &Context, commands: &mut Vec<Command>) {
        let ctrl = |k| KeyboardShortcut::new(Modifiers::COMMAND, k);
        let alt = |k| KeyboardShortcut::new(Modifiers::ALT, k);
        let pressed = |s: KeyboardShortcut| ctx.input_mut(|i| i.consume_shortcut(&s));

        if pressed(ctrl(Key::Q)) || pressed(ctrl(Key::W)) {
            commands.push(Command::Quit);
        }
        if pressed(KeyboardShortcut::new(
            Modifiers::COMMAND | Modifiers::SHIFT,
            Key::Slash,
        )) || pressed(ctrl(Key::Questionmark))
        {
            commands.push(Command::OpenDialog(Dialog::KeyboardShortcuts));
        }
        for (k, page) in [Key::Num1, Key::Num2, Key::Num3, Key::Num4]
            .into_iter()
            .zip(MainPage::ALL)
        {
            if pressed(ctrl(k)) || pressed(alt(k)) {
                commands.push(Command::SetPage(page));
            }
        }
        if pressed(ctrl(Key::A)) {
            commands.push(Command::OpenDialog(Dialog::AboutSystem));
        }
        if pressed(KeyboardShortcut::new(Modifiers::NONE, Key::F5)) {
            commands.push(Command::RefreshNow);
        }
        if pressed(KeyboardShortcut::new(Modifiers::NONE, Key::F12)) {
            self.show_overlay = !self.show_overlay;
        }

        match self.page {
            MainPage::Performance => {
                if pressed(ctrl(Key::M)) {
                    commands.push(Command::ToggleSummaryMode);
                }
                if pressed(ctrl(Key::C))
                    && let Some(text) = self.perf.data_summary()
                {
                    commands.push(Command::CopyText(text));
                }
                let keys = [
                    Key::F1,
                    Key::F2,
                    Key::F3,
                    Key::F4,
                    Key::F5,
                    Key::F6,
                    Key::F7,
                    Key::F8,
                ];
                for (n, k) in keys.into_iter().enumerate() {
                    if pressed(KeyboardShortcut::new(Modifiers::NONE, k)) {
                        self.perf.select_nth_shown(n, &mut self.config);
                    }
                }
            }
            MainPage::Processes | MainPage::Details | MainPage::Services => {
                let table = if self.page == MainPage::Services {
                    &mut self.services
                } else {
                    &mut self.apps
                };
                if self.page != MainPage::Services
                    && pressed(KeyboardShortcut::new(Modifiers::NONE, Key::Delete))
                {
                    commands.extend(table.process_action(Request::TerminateProcesses));
                }
                if pressed(ctrl(Key::L)) {
                    table.collapse_all();
                }
                if pressed(ctrl(Key::E)) {
                    commands.extend(table.process_action(Request::TerminateProcesses));
                    commands.extend(table.service_action(Request::StopService));
                }
                if pressed(ctrl(Key::X)) {
                    commands.extend(table.process_action(Request::KillProcesses));
                    commands.extend(table.service_action(Request::StopService));
                }
                if pressed(ctrl(Key::I))
                    && let Some(node) = table.selected_node()
                {
                    let dialog = if node.service.is_some()
                        && node.kind == mc_core::table::RowKind::Service
                    {
                        Dialog::ServiceDetails(node.key.clone())
                    } else {
                        Dialog::ProcessDetails(node.key.clone())
                    };
                    commands.push(Command::OpenDialog(dialog));
                }
                if self.page == MainPage::Services {
                    if pressed(ctrl(Key::S)) {
                        commands.extend(table.service_action(Request::StartService));
                    }
                    if pressed(ctrl(Key::R)) {
                        commands.extend(table.service_action(Request::RestartService));
                    }
                }
            }
        }
    }

    fn menu(&mut self, ui: &mut Ui, index: usize, commands: &mut Vec<Command>) {
        match index {
            0 => {
                if menu::item(ui, &i18n("Run new task"), "", true) {
                    self.run_task = Some(String::new());
                }
                menu::separator(ui);
                if menu::item(ui, &i18n("Exit"), "", true) {
                    commands.push(Command::Quit);
                }
            }
            1 => {
                let viewport = self.always_on_top.mode == AlwaysOnTopMode::Viewport;
                let checked = if viewport {
                    self.config.window_always_on_top
                } else {
                    self.always_on_top.actual.unwrap_or(false)
                };
                let enabled = viewport || self.always_on_top.enabled();
                // Own addition: explain pending/unavailable state even on a disabled menu row.
                let reason = if self.always_on_top.pending() {
                    Some(i18n(
                        "Waiting for the window manager to confirm always on top.",
                    ))
                } else {
                    self.always_on_top.error.map(|error| match error {
                        AlwaysOnTopError::Unavailable => {
                            i18n("Always on top is unavailable on this desktop.")
                        }
                        AlwaysOnTopError::NotFound => {
                            i18n("The window manager could not find the Task Manager window.")
                        }
                        AlwaysOnTopError::Ambiguous => {
                            i18n("The window manager could not identify a unique Task Manager window.")
                        }
                        AlwaysOnTopError::Rejected => {
                            i18n("The window manager did not apply the requested always-on-top state.")
                        }
                        AlwaysOnTopError::TimedOut => {
                            i18n("The window manager did not confirm always on top in time.")
                        }
                        AlwaysOnTopError::Failed => {
                            i18n("Could not communicate with the window manager to set always on top.")
                        }
                        AlwaysOnTopError::Cleanup => {
                            i18n("Could not remove the window manager script. Further always-on-top changes are disabled.")
                        }
                    })
                };
                if menu::check_enabled(
                    ui,
                    &i18n("Always on top"),
                    checked,
                    enabled,
                    reason.as_deref(),
                ) {
                    commands.push(Command::SetAlwaysOnTop(!checked));
                }
                menu::separator(ui);
                menu::submenu(ui, &i18n("Keyboard Shortcuts"), |ui| {
                    let current = [
                        self.config.launch_ctrl_shift_escape,
                        self.config.launch_ctrl_alt_delete,
                    ];
                    for (index, label) in ["Ctrl+Shift+Esc", "Ctrl+Alt+Delete"].iter().enumerate() {
                        if menu::check_enabled(
                            ui,
                            label,
                            current[index],
                            !self.launch_pending,
                            Some(&self.launch_status),
                        ) {
                            let mut next = current;
                            next[index] = !next[index];
                            commands.push(Command::SetLaunchShortcuts(next));
                        }
                    }
                    menu::separator(ui);
                    if menu::item(ui, &i18n("Show Keyboard Shortcuts"), "Ctrl+?", true) {
                        commands.push(Command::OpenDialog(Dialog::KeyboardShortcuts));
                    }
                });
                if menu::check_enabled(
                    ui,
                    &i18n("Extended hardware monitoring"),
                    self.config.extended_hardware_monitoring,
                    !self.hardware_pending,
                    Some(&self.hardware_status),
                ) {
                    commands.push(Command::SetHardwareAccess(
                        !self.config.extended_hardware_monitoring,
                    ));
                }
                if menu::item(
                    ui,
                    &i18n("Install hardware monitoring service…"),
                    "",
                    !self.hardware_pending,
                ) {
                    commands.push(Command::InstallHardwareService);
                }
                menu::separator(ui);
                if menu::item(
                    ui,
                    &mc_core::i18n::i18n_f("About {}", &[crate::APP_NAME]),
                    "",
                    true,
                ) {
                    commands.push(Command::OpenDialog(Dialog::About));
                }
            }
            _ => {
                menu::submenu(ui, &i18n("Theme"), |ui| {
                    use mc_core::config::Theme;
                    for (label, theme) in [
                        (n_("Light"), Theme::Light),
                        (n_("Dark"), Theme::Dark),
                        (n_("System"), Theme::System),
                    ] {
                        if menu::radio(ui, &i18n(label), self.config.theme == theme) {
                            self.config.theme = theme;
                        }
                    }
                });
                menu::separator(ui);
                if menu::item(ui, &i18n("Refresh now"), "F5", true) {
                    commands.push(Command::RefreshNow);
                }
                menu::separator(ui);
                if menu::check(
                    ui,
                    &i18n("Group by type"),
                    self.config.apps_page_group_by_type,
                ) {
                    self.config.apps_page_group_by_type = !self.config.apps_page_group_by_type;
                }
                if menu::item(ui, &i18n("Expand all"), "", true) {
                    self.apps.expand_all();
                }
                if menu::item(ui, &i18n("Collapse all"), "Ctrl+L", true) {
                    self.apps.collapse_all();
                }
                menu::separator(ui);
                // Own addition: Docker, bridge, veth and Multipass adapters (hidden by default)
                if menu::check(
                    ui,
                    &i18n("Show virtual network adapters"),
                    self.config.performance_show_network_virtual,
                ) {
                    self.config.performance_show_network_virtual =
                        !self.config.performance_show_network_virtual;
                }
                menu::separator(ui);
                menu::submenu(ui, &i18n("Update speed"), |ui| {
                    for (label, interval) in [(n_("High"), 10), (n_("Normal"), 20), (n_("Low"), 80)]
                    {
                        let selected = !self.config.app_update_paused
                            && self.config.app_update_interval == interval;
                        if menu::radio(ui, &i18n(label), selected) {
                            self.config.app_update_interval = interval;
                            self.config.app_update_paused = false;
                        }
                    }
                    if menu::radio(ui, &i18n("Paused"), self.config.app_update_paused) {
                        self.config.app_update_paused = true;
                    }
                });
            }
        }
    }

    /// Switches between the full window and the compact "Fewer details" app list; the full
    /// window size is restored when leaving compact mode.
    fn toggle_fewer_details(&mut self, ctx: &Context) {
        self.config.window_fewer_details = !self.config.window_fewer_details;
        let size = if self.config.window_fewer_details {
            self.full_size = ctx.input(|i| i.viewport().inner_rect.map(|r| r.size()));
            egui::vec2(
                metrics::COMPACT_WINDOW_SIZE[0],
                metrics::COMPACT_WINDOW_SIZE[1],
            )
        } else {
            self.full_size.unwrap_or(egui::vec2(
                self.config.window_width as f32,
                self.config.window_height as f32,
            ))
        };
        // The full layout needs a larger minimum than the compact app list
        let min = if self.config.window_fewer_details {
            metrics::COMPACT_MIN_SIZE
        } else {
            metrics::MIN_WINDOW_SIZE
        };
        ctx.send_viewport_cmd(egui::ViewportCommand::MinInnerSize(min.into()));
        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(size));
    }

    /// File → Run new task: Win32-style "Create new task" dialog.
    fn run_task_dialog(&mut self, ctx: &Context, commands: &mut Vec<Command>) {
        let Some(text) = self.run_task.as_mut() else {
            return;
        };
        let mut open = true;
        let mut submit = false;
        let mut cancel = false;
        egui::Window::new(i18n("Create new task"))
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0., 0.])
            .fixed_size([360., 0.])
            .open(&mut open)
            .show(ctx, |ui| {
                ui.label(i18n(
                    "Type the name of a program and Task Manager will start it.",
                ));
                ui.add_space(4.);
                ui.horizontal(|ui| {
                    ui.label(i18n("Open:"));
                    let edit =
                        ui.add(egui::TextEdit::singleline(text).desired_width(f32::INFINITY));
                    if !edit.has_focus() && !edit.lost_focus() {
                        edit.request_focus();
                    }
                    if edit.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                        submit = true;
                    }
                });
                ui.add_space(6.);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui
                        .add(egui::Button::new(i18n("Cancel")).min_size([75., 23.].into()))
                        .clicked()
                    {
                        cancel = true;
                    }
                    let ok = egui::Button::new(i18n("OK")).min_size([75., 23.].into());
                    if ui.add_enabled(!text.trim().is_empty(), ok).clicked() {
                        submit = true;
                    }
                });
            });
        if submit && !text.trim().is_empty() {
            commands.push(Command::RunTask(text.trim().to_owned()));
            self.run_task = None;
        } else if cancel || !open || ctx.input(|i| i.key_pressed(Key::Escape)) {
            self.run_task = None;
        }
    }

    /// Draws the whole window.
    pub fn ui(&mut self, ui: &mut Ui) {
        let started = Instant::now();
        let ctx = ui.ctx().clone();

        let mut commands = vec![];
        self.shortcuts(&ctx, &mut commands);

        // The whole viewport is the window (client-side decorations); ignore parent margins
        let window = ctx.content_rect();
        let mut root = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(window)
                .layout(Layout::top_down(Align::Min)),
        );
        root.set_clip_rect(window);
        let ui = &mut root;
        let maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
        chrome::window_background(ui, window, !maximized);
        let compact = self.config.window_fewer_details;
        let graph_summary = self.page == MainPage::Performance && self.perf.summary_mode;
        if !graph_summary {
            chrome::title_bar(ui, window, crate::APP_NAME, maximized);
        }
        if !compact && !graph_summary {
            let menus = [i18n("File"), i18n("Options"), i18n("View")];
            chrome::menu_bar(ui, window, &menus, |ui, index| {
                self.menu(ui, index, &mut commands)
            });

            let labels: Vec<String> = MainPage::ALL.iter().map(|p| p.label()).collect();
            let selected = MainPage::ALL
                .iter()
                .position(|p| *p == self.page)
                .unwrap_or(0);
            if let Some(index) = chrome::tabs(ui, window, &labels, selected) {
                commands.push(Command::SetPage(MainPage::ALL[index]));
            }
        }

        let line_y = if graph_summary {
            window.bottom() - 1.
        } else {
            chrome::footer_line(ui, window, compact || self.page != MainPage::Performance)
        };
        let expanded = !self.config.window_fewer_details;
        let toggle = if expanded {
            i18n("Fewer details")
        } else {
            i18n("More details")
        };
        if !graph_summary && chrome::details_toggle(ui, window, line_y, &toggle, expanded).clicked()
        {
            self.toggle_fewer_details(&ctx);
        }
        if !compact && self.page == MainPage::Details {
            let pid = self.details.selected_pid();
            if chrome::footer_button(ui, window, line_y, &i18n("End task"), pid.is_some()).clicked()
                && let Some(pid) = pid
            {
                commands.push(Command::Magpie(Request::TerminateProcesses(vec![pid])));
            }
        } else if compact || self.page == MainPage::Processes {
            let enabled = self.apps.selected_node().is_some();
            if chrome::footer_button(ui, window, line_y, &i18n("End task"), enabled).clicked() {
                commands.extend(self.apps.process_action(Request::TerminateProcesses));
            }
        }

        let content_top = if graph_summary {
            1.
        } else if compact {
            metrics::TITLE_BOTTOM + 1.
        } else {
            metrics::CONTENT_TOP
        };
        let content = egui::Rect::from_min_max(
            egui::pos2(window.left() + 1., window.top() + content_top),
            egui::pos2(window.right() - 1., line_y),
        );
        let mut cx = Cx {
            icons: &mut self.icons,
            config: &mut self.config,
            commands: &mut commands,
        };
        match self.page {
            _ if compact => {
                self.processes
                    .compact_ui(ui, &mut cx, &mut self.apps, content);
            }
            MainPage::Processes => {
                self.processes.ui(ui, &mut cx, &mut self.apps, content);
            }
            MainPage::Performance => {
                self.performance.ui(ui, &mut cx, &mut self.perf, content);
            }
            MainPage::Details => {
                self.details.ui(ui, &mut cx, content);
            }
            MainPage::Services => {
                self.services_view.ui(ui, &mut cx, content);
            }
        }

        if !maximized {
            chrome::resize_edges(ui, window);
        }

        {
            let mut cx = Cx {
                icons: &mut self.icons,
                config: &mut self.config,
                commands: &mut commands,
            };
            self.dialogs.ui(&ctx, &mut cx, &self.apps, &self.services);
        }

        self.run_task_dialog(&ctx, &mut commands);
        if self.hardware_notice {
            let response = egui::Modal::new(egui::Id::new("hardware-monitoring-status")).show(&ctx, |ui| {
                ui.set_width(430.);
                ui.heading(i18n("Extended hardware monitoring"));
                if self.hardware_pending { ui.spinner(); }
                ui.label(&self.hardware_status);
                ui.label(i18n("Without an installed service, the helper runs only until Task Manager closes. Install the service for persistent access across restarts."));
                if ui.add_enabled(!self.hardware_pending, egui::Button::new(i18n("Install service…"))).clicked() {
                    commands.push(Command::InstallHardwareService);
                }
                if ui.button(i18n("Close")).clicked() { self.hardware_notice = false; }
            });
            if response.should_close() {
                self.hardware_notice = false;
            }
        }

        if let Some(message) = self.fatal.clone() {
            crate::dialogs::fatal_dialog(&ctx, &message, &mut commands);
        }

        for command in commands {
            self.execute(&ctx, command);
        }

        self.stats.record(started.elapsed().as_secs_f32() * 1000.);
        self.stats.frames = ctx.cumulative_frame_nr();
        if ctx.current_pass_index() == 0 {
            for cause in ctx.repaint_causes() {
                *self
                    .stats
                    .repaint_causes
                    .entry(cause.to_string())
                    .or_default() += 1;
            }
        }
        if self.show_overlay {
            self.overlay(&ctx);
        }
    }

    fn overlay(&self, ctx: &Context) {
        let (p50, p95) = self.stats.percentiles();
        let (published, dropped) = self.magpie.readings_stats();
        egui::Area::new(egui::Id::new("perf-overlay"))
            .anchor(egui::Align2::RIGHT_BOTTOM, egui::vec2(-8., -8.))
            .order(egui::Order::Tooltip)
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.label(
                        RichText::new(format!(
                            "UI pass p50/p95: {p50:.2}/{p95:.2} ms\nframes: {} (passes {})\nreadings: {} applied, {published} published, {dropped} dropped",
                            self.stats.frames, self.stats.passes, self.stats.readings_applied
                        ))
                        .monospace(),
                    );
                });
            });
    }
}

/// Starts `command` through `sh -c`, detached, from a helper thread; failures are only logged.
fn run_task(command: String) {
    let _ = std::thread::Builder::new()
        .name("mc-run-task".into())
        .spawn(move || {
            let child = std::process::Command::new("sh")
                .arg("-c")
                .arg(&command)
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn();
            match child {
                // Reap the process when it exits so it does not linger as a zombie
                Ok(mut child) => {
                    let _ = child.wait();
                }
                Err(e) => {
                    log::warn!(target: "TaskManager::RunTask", "Failed to run '{command}': {e}")
                }
            }
        });
}

fn open_network_settings(wireless: bool) {
    let desktop = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();
    let (program, args): (&str, Vec<&str>) = if desktop.contains("KDE") {
        ("systemsettings", vec!["kcm_networkmanagement"])
    } else {
        (
            "gnome-control-center",
            vec![if wireless { "wifi" } else { "network" }],
        )
    };
    // Spawned off the UI thread; failures are only logged
    let program = program.to_owned();
    let args: Vec<String> = args.into_iter().map(str::to_owned).collect();
    let _ = std::thread::Builder::new()
        .name("mc-spawn".into())
        .spawn(move || {
            if let Err(e) = std::process::Command::new(&program).args(&args).spawn() {
                log::warn!(target: "TaskManager::Network", "Failed to open network settings: {e}");
            }
        });
}

#[cfg(test)]
mod tests {
    use egui::{Event, PointerButton, Pos2, RawInput, Rect, vec2};

    use super::*;

    #[test]
    fn hardware_access_click_reports_pending_and_failed_authorization() {
        let ctx = Context::default();
        let mut app = App::new(
            &ctx,
            AppOptions {
                source: DataSource::Fixture(mc_magpie::synthetic_series(1, 1), HashMap::new()),
                config_path: None,
                perf_overlay: false,
                fonts: Default::default(),
                initial_config: Some(Config::default()),
                always_on_top_mode: AlwaysOnTopMode::Viewport,
            },
        );
        app.execute(&ctx, Command::SetHardwareAccess(true));
        assert!(app.hardware_notice);
        assert!(app.hardware_pending);
        assert!(app.hardware_status.contains("Waiting"));
        app.handle_event(mc_magpie::Event::HardwareAccess(Err(
            "The host hardware helper is not installed.".into(),
        )));
        assert!(!app.hardware_pending);
        assert!(!app.config.extended_hardware_monitoring);
        let mut output = ctx.run_ui(
            RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1030., 705.))),
                ..Default::default()
            },
            |ui| app.ui(ui),
        );
        output.textures_delta.clear();
        output = ctx.run_ui(
            RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1030., 705.))),
                ..Default::default()
            },
            |ui| app.ui(ui),
        );
        output.textures_delta.clear();
        assert!(output.shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Text(text) if text.galley.job.text.contains("not installed"))));
        assert!(output.shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Text(text) if text.galley.job.text.contains("Install service"))));
        app.execute(&ctx, Command::InstallHardwareService);
        assert!(app.hardware_pending);
        assert!(app.hardware_status.contains("service installation"));
    }

    #[test]
    fn options_always_on_top_click_waits_for_acknowledgement() {
        let ctx = Context::default();
        let mut app = App::new(
            &ctx,
            AppOptions {
                source: DataSource::Fixture(mc_magpie::synthetic_series(1, 1), HashMap::new()),
                config_path: None,
                perf_overlay: false,
                fonts: Default::default(),
                initial_config: Some(Config {
                    window_always_on_top: false,
                    ..Default::default()
                }),
                always_on_top_mode: AlwaysOnTopMode::External,
            },
        );
        let initial = app.take_always_on_top_request().unwrap();
        app.finish_always_on_top(AlwaysOnTopResult {
            id: initial.id,
            actual: Some(false),
            error: None,
        });

        let mut at = Pos2::ZERO;
        let mut commands = Vec::new();
        // Lay out the Options menu once, then press and release on its first (check) row.
        for frame in 0..3 {
            let events = if frame == 0 {
                Vec::new()
            } else {
                vec![
                    Event::PointerMoved(at),
                    Event::PointerButton {
                        pos: at,
                        button: PointerButton::Primary,
                        pressed: frame == 1,
                        modifiers: Modifiers::NONE,
                    },
                ]
            };
            let mut output = ctx.run_ui(
                RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(400., 200.))),
                    events,
                    ..Default::default()
                },
                |ui| {
                    menu::style(ui.style_mut());
                    at = ui.cursor().min + vec2(60., metrics::POPUP_ITEM_H / 2.);
                    app.menu(ui, 1, &mut commands);
                },
            );
            output.textures_delta.clear();
        }
        assert_eq!(commands, vec![Command::SetAlwaysOnTop(true)]);
        for command in commands {
            app.execute(&ctx, command);
        }
        let request = app.take_always_on_top_request().unwrap();
        assert!(request.enabled);
        assert!(app.always_on_top.pending());
        assert!(!app.config().window_always_on_top);

        app.finish_always_on_top(AlwaysOnTopResult {
            id: request.id,
            actual: Some(true),
            error: None,
        });
        assert!(app.config().window_always_on_top);
    }
}
