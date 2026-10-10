/* application.rs
 *
 * Copyright 2024 Romeo Calota
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

use std::cell::{BorrowError, Cell, Ref, RefCell};
use std::collections::HashMap;
use std::process::Command;

use adw::glib::g_warning;
use adw::{prelude::*, subclass::prelude::*};
use gtk::{
    gio,
    glib::{self, g_critical, property::PropertySet, WeakRef},
    Image,
};

use magpie_types::apps::icon::Icon;

use crate::about_system_dialog::AboutSystemDialog;
use crate::first_run_dialog::FirstRunDialog;
use crate::table_view::cached_icon::{decode_icon, CachedIcon, DecodedIcon, IconApply, IconState};
use crate::table_view::columns::NameCell;
use crate::{config::VERSION, i18n::i18n, magpie_client::Readings};

pub const INTERVAL_STEP: f64 = 0.05;
pub const BASE_INTERVAL: f64 = 1f64;

#[macro_export]
macro_rules! app {
    () => {{
        use ::gtk::glib::object::Cast;
        ::gtk::gio::Application::default()
            .and_then(|app| app.downcast::<$crate::MissionCenterApplication>().ok())
            .expect("Failed to get MissionCenterApplication instance")
    }};
}

#[macro_export]
macro_rules! settings {
    () => {
        $crate::app!().settings()
    };
}

mod imp {
    use super::*;
    use crate::setup_readable_settings_cache;

    pub struct MissionCenterApplication {
        pub settings: gio::Settings,
        pub sys_info: RefCell<Option<crate::magpie_client::MagpieClient>>,
        pub window: RefCell<Option<crate::MissionCenterWindow>>,

        pub apps_icons_cache: Cell<Option<HashMap<String, CachedIcon>>>,
        pub pending_icon_cells: RefCell<Vec<WeakRef<NameCell>>>,
    }

    impl Default for MissionCenterApplication {
        fn default() -> Self {
            Self {
                settings: gio::Settings::new("io.missioncenter.MissionCenter"),
                sys_info: RefCell::new(None),
                window: RefCell::new(None),
                apps_icons_cache: Cell::new(None),
                pending_icon_cells: RefCell::new(Vec::new()),
            }
        }
    }

    #[glib::object_subclass]
    impl ObjectSubclass for MissionCenterApplication {
        const NAME: &'static str = "MissioncenterApplication";
        type Type = super::MissionCenterApplication;
        type ParentType = adw::Application;
    }

    impl ObjectImpl for MissionCenterApplication {
        fn constructed(&self) {
            self.parent_constructed();
            let obj = self.obj();

            obj.set_default();

            obj.setup_gactions();
            obj.set_accels_for_action("app.quit", &["<primary>q"]);
        }
    }

    impl ApplicationImpl for MissionCenterApplication {
        fn activate(&self) {
            use gtk::glib::*;

            let application = self.obj();
            // Get the current window or create one if necessary
            let window = if let Some(window) = application.window() {
                window
            } else {
                let settings = &self.settings;

                let sys_info = crate::magpie_client::MagpieClient::new();

                let window = crate::MissionCenterWindow::new(&*application, &settings, &sys_info);

                setup_readable_settings_cache(&settings);

                window.connect_default_height_notify({
                    move |window| {
                        let settings = settings!();
                        settings
                            .set_int("window-height", window.default_height())
                            .unwrap_or_else(|err| {
                                g_critical!(
                                    "MissionCenter",
                                    "Failed to save window height: {}",
                                    err
                                );
                            });
                    }
                });
                window.connect_default_width_notify({
                    move |window| {
                        let settings = settings!();
                        settings
                            .set_int("window-width", window.default_width())
                            .unwrap_or_else(|err| {
                                g_critical!(
                                    "MissionCenter",
                                    "Failed to save window width: {}",
                                    err
                                );
                            });
                    }
                });

                window
                    .set_default_size(settings.int("window-width"), settings.int("window-height"));

                window.connect_maximized_notify({
                    move |window| {
                        let settings = settings!();
                        settings
                            .set_boolean("is-maximized", window.is_maximized())
                            .unwrap_or_else(|err| {
                                g_critical!(
                                    "MissionCenter",
                                    "Failed to save window maximization: {}",
                                    err
                                );
                            });
                    }
                });

                window.set_maximized(settings.boolean("is-maximized"));

                sys_info.set_core_count_affects_percentages(
                    settings.boolean("apps-page-core-count-affects-percentages"),
                );

                settings.connect_changed(
                    Some("apps-page-core-count-affects-percentages"),
                    move |settings, _| {
                        let app = app!();
                        match app.sys_info() {
                            Ok(sys_info) => {
                                sys_info.set_core_count_affects_percentages(
                                    settings.boolean("apps-page-core-count-affects-percentages"),
                                );
                            }
                            Err(e) => {
                                g_critical!(
                                    "MissionCenter",
                                    "Failed to get sys_info from MissionCenterApplication: {}",
                                    e
                                );
                            }
                        };
                    },
                );

                self.sys_info.set(Some(sys_info));

                let provider = gtk::CssProvider::new();
                provider.load_from_bytes(&Bytes::from_static(include_bytes!(
                    "../resources/ui/style.css"
                )));

                gtk::style_context_add_provider_for_display(
                    &gtk::gdk::Display::default().expect("Could not connect to a display."),
                    &provider,
                    gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
                );

                window.upcast()
            };

            window.present();

            self.window
                .set(window.downcast_ref::<crate::MissionCenterWindow>().cloned());
        }
    }

    impl GtkApplicationImpl for MissionCenterApplication {}

    impl AdwApplicationImpl for MissionCenterApplication {}
}

glib::wrapper! {
    pub struct MissionCenterApplication(ObjectSubclass<imp::MissionCenterApplication>)
        @extends gio::Application, gtk::Application, adw::Application,
        @implements gio::ActionGroup, gio::ActionMap;
}

impl MissionCenterApplication {
    pub fn apply_app_icon(&self, image: &Image, app_id: String, width: i32) -> IconApply {
        let this = self.imp();

        // if it was default::default, then this has no side effects and we can safely return
        let Some(mut icons) = this.apps_icons_cache.take() else {
            return IconApply::Unavailable;
        };

        let retval = match icons.get_mut(&app_id) {
            Some(CachedIcon::CachedData((bytes, caches))) => {
                let width = width * image.scale_factor().max(1);

                match caches.get(&width) {
                    Some(IconState::Ready(pixbuf)) => {
                        #[allow(deprecated)]
                        image.set_from_pixbuf(Some(pixbuf));

                        IconApply::Applied
                    }
                    Some(IconState::Failed) => {
                        CachedIcon::apply_blank(image);

                        IconApply::Unavailable
                    }
                    Some(IconState::Pending) => {
                        CachedIcon::apply_blank(image);

                        IconApply::Pending
                    }
                    None => {
                        caches.insert(width, IconState::Pending);
                        self.decode_app_icon(app_id.clone(), width, bytes.clone());
                        CachedIcon::apply_blank(image);

                        IconApply::Pending
                    }
                }
            }
            Some(icon) => {
                if icon.apply_to_image(image, width) {
                    IconApply::Applied
                } else {
                    IconApply::Unavailable
                }
            }
            None => {
                CachedIcon::apply_blank(image);

                IconApply::Unavailable
            }
        };

        this.apps_icons_cache.set(Some(icons));

        retval
    }

    fn decode_app_icon(&self, app_id: String, width: i32, bytes: glib::Bytes) {
        let imp = self.imp().downgrade();

        glib::spawn_future_local(async move {
            let Some(imp) = imp.upgrade() else {
                return;
            };

            let decoded = gio::spawn_blocking(move || decode_icon(&bytes, width))
                .await
                .ok()
                .flatten();

            let state = match decoded.and_then(DecodedIcon::into_pixbuf) {
                Some(pixbuf) => IconState::Ready(pixbuf),
                None => IconState::Failed,
            };

            if let Some(mut icons) = imp.apps_icons_cache.take() {
                if let Some(CachedIcon::CachedData((_, caches))) = icons.get_mut(&app_id) {
                    caches.insert(width, state);
                }

                imp.apps_icons_cache.set(Some(icons));
            }

            let mut pending_icon_cells = imp.pending_icon_cells.borrow_mut();
            let pending_len = pending_icon_cells.len();

            let waiting = std::mem::take(&mut *pending_icon_cells);
            let still_pending = waiting.into_iter().filter(|cell| {
                cell.upgrade()
                    .is_some_and(|cell| cell.reapply_icon() == IconApply::Pending)
            });

            pending_icon_cells.reserve(pending_len);
            pending_icon_cells.extend(still_pending);
        });
    }

    pub fn register_pending_icon_cell(&self, cell: &NameCell) {
        let mut cells = self.imp().pending_icon_cells.borrow_mut();

        if cells
            .iter()
            .any(|pending| pending.upgrade().is_some_and(|pending| &pending == cell))
        {
            return;
        }

        cells.push(cell.downgrade());
    }

    pub fn new(application_id: &str, flags: &gio::ApplicationFlags) -> Self {
        use glib::g_message;

        let this: Self = glib::Object::builder()
            .property("application-id", application_id)
            .property("flags", flags)
            .build();

        g_message!(
            "MissionCenter::Application",
            "Starting Mission Center v{}",
            VERSION
        );

        this
    }

    pub fn restart(&self) {
        #[cfg(unix)]
        use std::os::unix::process::CommandExt;

        let exe = std::env::current_exe().expect("Failed to get exe");

        #[cfg(unix)]
        {
            let _ = Command::new(exe).exec();
        }

        #[cfg(windows)]
        {
            Command::new(&exe)
                .spawn()
                .expect("Failed to spawn new process");

            std::process::exit(0);
        }
    }

    pub fn set_initial_readings(&self, readings: Readings) {
        use gtk::glib::*;

        let Some(window) = self.window() else {
            g_critical!(
                "MissionCenter::Application",
                "No active window, when trying to refresh data"
            );
            return;
        };

        window.set_initial_readings(readings)
    }

    pub fn set_app_icons(&self, icons: HashMap<String, Icon>) {
        self.imp()
            .apps_icons_cache
            .set(Some(CachedIcon::convert_hash_map(icons)))
    }

    pub fn merge_app_icons(&self, icons: HashMap<String, Icon>) {
        let old = self.imp().apps_icons_cache.take();

        let Some(mut old) = old else {
            self.set_app_icons(icons);
            return;
        };

        let icons = CachedIcon::convert_hash_map(icons);

        for (app_id, icon) in icons {
            old.insert(app_id, icon);
        }

        self.imp().apps_icons_cache.set(Some(old));
    }

    pub fn missing_icons(&self, mut appids: Vec<&String>) -> Option<Vec<String>> {
        let this = self.imp();
        let icons = this.apps_icons_cache.take();

        this.apps_icons_cache.set(icons.clone());

        let Some(apps) = icons else {
            return Some(appids.drain(..).map(|app_id| app_id.to_string()).collect());
        };

        let out: Vec<_> = appids
            .drain(..)
            .filter_map(|appid| {
                if !apps.contains_key(appid) {
                    Some(appid.clone())
                } else {
                    None
                }
            })
            .collect();

        if out.is_empty() {
            None
        } else {
            Some(out)
        }
    }

    pub fn setup_animations(&self) {
        use gtk::glib::*;

        let Some(window) = self.window() else {
            g_critical!(
                "MissionCenter::Application",
                "No active window, when trying to refresh data"
            );
            return;
        };

        window.setup_animations()
    }

    pub fn refresh_readings(&self, readings: &mut Readings) -> bool {
        use gtk::glib::*;

        let Some(window) = self.window() else {
            g_critical!(
                "MissionCenter::Application",
                "No active window, when trying to refresh data"
            );
            return false;
        };

        window.update_readings(readings)
    }

    pub fn settings(&self) -> gio::Settings {
        self.imp().settings.clone()
    }

    pub fn sys_info(&self) -> Result<Ref<'_, crate::magpie_client::MagpieClient>, BorrowError> {
        match self.imp().sys_info.try_borrow() {
            Ok(sys_info_ref) => Ok(Ref::map(sys_info_ref, |sys_info_opt| match sys_info_opt {
                Some(sys_info) => sys_info,
                None => {
                    panic!("MissionCenter::Application::sys_info() called before sys_info was initialized");
                }
            })),
            Err(e) => Err(e),
        }
    }

    pub fn window(&self) -> Option<crate::MissionCenterWindow> {
        self.imp().window.borrow().clone()
    }

    fn setup_gactions(&self) {
        let quit_action = gio::ActionEntry::builder("quit")
            .activate(move |app: &Self, _, _| app.quit())
            .build();
        let preferences_action = gio::ActionEntry::builder("preferences")
            .activate(move |app: &Self, _, _| {
                app.show_preferences();
            })
            .build();
        let about_action = gio::ActionEntry::builder("about")
            .activate(move |app: &Self, _, _| app.show_about())
            .build();
        let about_system_action = gio::ActionEntry::builder("system-about")
            .activate(move |app: &Self, _, _| app.show_system_about())
            .build();
        let keyboard_shortcuts_action = gio::ActionEntry::builder("keyboard-shortcuts")
            .activate(move |app: &Self, _, _| app.show_keyboard_shortcuts())
            .build();
        let open_wiki_action = gio::ActionEntry::builder("open-wiki")
            .activate(move |_, _, _| {
                gio::AppInfo::launch_default_for_uri(
                    "https://gitlab.com/mission-center-devs/mission-center/-/wikis/home",
                    None::<&gio::AppLaunchContext>,
                )
                .unwrap_or_else(|_| {
                    g_critical!(
                        "MissionCenter::Application",
                        "Failed to open Mission Center Wiki"
                    );
                });
            })
            .build();
        let first_run_dialog_action = gio::ActionEntry::builder("first-run-dialog")
            .activate(move |app: &Self, _, _| app.show_first_run_dialog())
            .build();

        self.add_action_entries([
            quit_action,
            preferences_action,
            about_action,
            about_system_action,
            keyboard_shortcuts_action,
            open_wiki_action,
            first_run_dialog_action,
        ]);

        self.set_accels_for_action("app.preferences", &["<Control>comma"]);
        self.set_accels_for_action("app.keyboard-shortcuts", &["<Control>question"]);
    }

    fn show_preferences(&self) {
        let Some(window) = self.window() else {
            g_critical!(
                "MissionCenter::Application",
                "No active window, when trying to show preferences"
            );
            return;
        };

        let preferences = crate::preferences::PreferencesDialog::new();
        preferences.present(Some(&window));
    }

    fn show_keyboard_shortcuts(&self) {
        let Some(app_window) = self.window() else {
            return;
        };

        let builder =
            gtk::Builder::from_resource("/io/missioncenter/MissionCenter/ui/keyboard_shortcuts.ui");
        let dialog = builder
            .object::<adw::ShortcutsDialog>("keyboard_shortcuts")
            .expect("Failed to get shortcuts window");

        dialog.present(Some(&app_window));
    }

    fn show_system_about(&self) {
        let app = app!();
        let Ok(magpie) = app.sys_info() else {
            g_warning!("MissionCenter::Disk", "Failed to get magpie client");
            return;
        };

        let about = magpie.about_system();

        let dialog = AboutSystemDialog::new(about);

        let Some(window) = self.window() else {
            g_critical!(
                "MissionCenter::Application",
                "No active window, when trying to show about dialog"
            );
            return;
        };

        dialog.present(Some(&window));
    }

    pub fn show_first_run_dialog(&self) {
        FirstRunDialog::run()
    }

    fn show_about(&self) {
        let Some(window) = self.window() else {
            g_critical!(
                "MissionCenter::Application",
                "No active window, when trying to show about dialog"
            );
            return;
        };

        let about = adw::AboutDialog::builder()
            .application_name("Mission Center")
            .application_icon("io.missioncenter.MissionCenter")
            .developer_name("Mission Center Developers")
            .developers(["Romeo Calota", "QwertyChouskie", "jojo2357", "Jan Luca"])
            .translator_credits(i18n("translator-credits"))
            .version(VERSION)
            .issue_url("https://gitlab.com/mission-center-devs/mission-center/-/issues")
            .copyright("© 2023-2025 Mission Center Developers")
            .license_type(gtk::License::Gpl30)
            .website("https://missioncenter.io")
            .release_notes(r#"<p>Noteworthy changes:</p>
<ul>
<li>Add a new Battery page to the Performance tab, with charge graphs and detailed battery information (@jlo62)</li>
<li>Show per-partition usage details on the disk page, including used and free space (@jojo2357)</li>
<li>Overhaul the graphing backend: smoother animations and rendering, a new loading shimmer while data loads, and less stuttering on application startup (@jojo2357, @kicsyromy)</li>
<li>Improve application detection, fixing wrong names and icons, phantom entries for PWAs, and constantly cycling processes (@kicsyromy)</li>
<li>Improvements to the Snap package, which was upgraded to the core24 snap (@kicsyromy)</li>
<li>Improvements to the AppImage package which now uses quick-sharun, is smaller and no longer depends on FUSE (@kicsyromy)</li>
</ul>
<p>Minor features:</p>
<ul>
<li>Add a second CPU graph that can show temperature, power draw or CPU frequency (@jlo62)</li>
<li>Add memory compression (zRAM/zswap) statistics to the Memory page (@timatgca)</li>
<li>Refactor and redesign the Preferences dialog (@kicsyromy)</li>
<li>Allow disabling entire device categories and individual network types (@kicsyromy)</li>
<li>Update to GNOME 50 Platform (@kicsyromy)</li>
<li>Pause UI refreshes by holding the CTRL key, just like the Windows Task Manager (@jlo62)</li>
<li>More accurate per-process memory usage using a hybrid PSS approximation (@kicsyromy)</li>
<li>Show per-process swap usage instead of shared memory in the Apps page (@jlo62)</li>
<li>Allow sending process signals that require elevation via `pkexec` (@jojo2357)</li>
<li>Add an option to select what the middle GPU graph displays (@jlo62)</li>
<li>Add a first run dialog and setup script for advanced features (@jlo62)</li>
<li>Add `--app-id`/`-a` command-line flag for setting custom application IDs (@kicsyromy)</li>
<li>Collapse and expand rows using double-click or the left and right arrow keys (@jojo2357)</li>
<li>Display the network connection state in the network details sidebar (@jojo2357)</li>
<li>Show total GPU memory on the graph when GTT is not available (@jlo62)</li>
<li>Add an option to gray out zero values in the Apps page (@jlo62)</li>
<li>Enhance the About System dialog with more information and better copy functionality (@jlo62)</li>
<li>Add and rebind keyboard shortcuts, including Alt+1/2/3 for switching pages and F9 for toggling the sidebar (@jlo62)</li>
<li>Add a visual indicator when CPU speed falls back to BogoMIPS (@bleys1)</li>
<li>Move icon extraction to backend to improve portability and remote monitoring (@jojo2357)</li>
</ul>
<p>Bug fixes:</p>
<ul>
<li>Fix pahntom CPU usage spikes when the system is idle (@kicsyromy)</li>
<li>Fix L3 and L4 cache sizes on CPUs with SNC-like topologies (@kicsyromy)</li>
<li>Treat ZFS ARC as free memory (@kicsyromy)</li>
<li>Fix blurry application icons on HiDPI displays (@kicsyromy)</li>
<li>Fix the SMART dialog so that it properly adapts to narrow widths (@kicsyromy)</li>
<li>Detect drives becoming ejectable at runtime (@jojo2357)</li>
<li>Fix a race condition in the Apps page (@jojo2357)</li>
<li>Fix graphing inconsistency on the disk page (@jojo2357)</li>
<li>Read CPU temperatures from the hwmon directory used by the SteamDeck (@jlo62)</li>
</ul>
<p>Translation updates</p>
<ul>
<li>Basque Ibai Oihanguren Sala</li>
<li>Bulgarian Alexander Stoilov</li>
<li>Catalan Xusi Fons Jaime Muñoz Martín</li>
<li>Chinese (Traditional Han script) Kisaragi Hiu 默想刃Mokuso YB</li>
<li>Chinese (Simplified Han script) lumingzh</li>
<li>Czech Pavel Borecki</li>
<li>Dutch Luc van der Werf</li>
<li>Finnish Henri Koivuranta Jiri Grönroos veiskiboi</li>
<li>French Norbert V Nota Inutilis ziyad arif Christophe Jaillet</li>
<li>Georgian Temuri Doghonadze</li>
<li>German Aircraft192 anon</li>
<li>Indonesian Arif Budiman</li>
<li>Irish Aindriú Mac Giolla Eoin</li>
<li>Italian Pierfrancesco Passerini</li>
<li>Kabyle ButterflyOfFire</li>
<li>Korean murllock329</li>
<li>Norwegian Bokmål ovl-1 Telaneo</li>
<li>Occitan Quentin PAGÈS</li>
<li>Polish NooB9496</li>
<li>Portuguese Hugo Carvalho Jonathan Teixeira</li>
<li>Portuguese (Brazil) Rafael Henrique Rubens Stuginski Jr</li>
<li>Russian Anatoly Bogomolov Maxim</li>
<li>Serbian Марко М. Костић</li>
<li>Spanish Xusi Fons</li>
<li>Swedish Game Nobz Jonas Viktor Engkvist</li>
<li>Tamil தமிழ்நேரம்</li>
<li>Turkish İsmail POLAT Sabri Ünal</li>
<li>Ukrainian anonymous Димко</li>
<li>Vietnamese lebao3105 Loc Huynh</li>
</ul>"#)
            .build();

        about.add_credit_section(
            Some("Standing on the shoulders of giants"),
            &[
                "GTK https://www.gtk.org/",
                "GNOME https://www.gnome.org/",
                "Libadwaita https://gitlab.gnome.org/GNOME/libadwaita",
                "Blueprint Compiler https://jwestman.pages.gitlab.gnome.org/blueprint-compiler/",
                "NVTOP https://github.com/Syllo/nvtop",
                "Workbench https://github.com/sonnyp/Workbench",
                "And many more... Thank you all!",
            ],
        );

        about.present(Some(&window));
    }
}
