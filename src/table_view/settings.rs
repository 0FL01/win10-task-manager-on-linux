use glib::g_critical;
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use gtk::{gio, glib};

use crate::settings;
use crate::table_view::SettingsValues::*;
use crate::table_view::TableView;
use std::sync::atomic::{AtomicBool, Ordering};

static HIDE_IS_ZERO: AtomicBool = AtomicBool::new(false);

pub fn configure(table_view: &TableView) {
    let settings = settings!();

    settings
        .bind(
            "apps-page-show-column-separators",
            &*table_view,
            "show-column-separators",
        )
        .build();

    table_view.set_use_merged_stats(settings.boolean("apps-page-merged-process-stats"));
    settings.connect_changed(Some("apps-page-merged-process-stats"), {
        let this = table_view.downgrade();
        move |settings, _| {
            if let Some(this) = this.upgrade() {
                this.imp()
                    .use_merged_stats
                    .set(settings.boolean("apps-page-merged-process-stats"));
            }
        }
    });

    HIDE_IS_ZERO.store(
        settings.boolean("apps-page-hide-is-zero"),
        Ordering::Relaxed,
    );

    settings.connect_changed(Some("apps-page-hide-is-zero"), move |settings, _| {
        HIDE_IS_ZERO.store(
            settings.boolean("apps-page-hide-is-zero"),
            Ordering::Relaxed,
        );
    });

    configure_sorting(table_view, &settings);
}

pub fn get_hide_is_zero() -> bool {
    HIDE_IS_ZERO.load(Ordering::Relaxed)
}

fn configure_sorting(table_view: &TableView, settings: &gio::Settings) {
    let column_view = table_view.column_view();

    let sorting_column_name_key = &table_view.format_settings_key(&SortingColumnName);
    let sorting_order_key = &table_view.format_settings_key(&SortingOrder);

    if !settings.boolean("apps-page-remember-sorting") {
        let _ = settings.set_string(sorting_column_name_key, "");
        let _ = settings.set_enum(sorting_order_key, gtk::ffi::GTK_SORT_ASCENDING);
        return;
    }

    let saved_id = settings.string(sorting_column_name_key);
    let order = settings.enum_(sorting_order_key);

    let columns = column_view.columns();
    let mut matched_column = None;
    for i in 0..columns.n_items() {
        let Some(column) = columns
            .item(i)
            .and_then(|i| i.downcast::<gtk::ColumnViewColumn>().ok())
        else {
            continue;
        };
        let Some(column_id) = column.id() else {
            continue;
        };

        if column_id == saved_id.as_str() {
            matched_column = Some(column);
            break;
        }
    }

    let order = match order {
        gtk::ffi::GTK_SORT_ASCENDING => gtk::SortType::Ascending,
        gtk::ffi::GTK_SORT_DESCENDING => gtk::SortType::Descending,
        255 => return,
        _ => {
            g_critical!(
                "MissionCenter::ProcessTree",
                "Unknown column sorting order retrieved from settings, sorting in ascending order as a fallback"
            );
            gtk::SortType::Ascending
        }
    };
    column_view.sort_by_column(matched_column.as_ref(), order);
}
