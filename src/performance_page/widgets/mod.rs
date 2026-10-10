/* performance_page/widgets/graph_widget.rs
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

pub use eject_failure_dialog::EjectFailureDialog;
pub use graph_widget_settings::GraphWidgetSettingsExt;
pub use mc_graph_widget::{
    AnimationFrame, DatasetGroup, FillingSettings, GraphWidget, RoundingSettings, ScalingSettings,
};
pub use mem_composition_widget::MemoryCompositionWidget;
pub use partition_usage_item::PartitionUsageItem;
pub use sidebar_drop_hint::SidebarDropHint;
pub use smart_data_dialog::SmartDataDialog;
pub use smart_failure_dialog::SmartFailureDialog;
pub use smart_nvme_dialog_row::SmartNvmeDialogRow;
pub use smart_sata_dialog_row::SmartSataDialogRow;

mod eject_failure_dialog;
mod eject_failure_row;
mod graph_widget_settings;
mod mem_composition_widget;
mod partition_usage_item;
mod sidebar_drop_hint;
mod smart_data_dialog;
mod smart_failure_dialog;
mod smart_nvme_dialog_row;
mod smart_sata_dialog_row;
