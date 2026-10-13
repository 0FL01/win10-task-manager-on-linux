/* Copyright 2026 Mission Center Developers
 * SPDX-License-Identifier: GPL-3.0-or-later
 */
//! Compact vertical module diagrams alongside Performance metadata.
use crate::{
    cx::{Command, Cx},
    win10::{
        metrics as m,
        paint::{self, Font},
        palette as pal,
    },
};
use egui::{Align, Rect, Ui, vec2};
use mc_core::{
    format::{DataType, to_human_readable_nice},
    i18n::{i18n, n_},
    spd::{SpdModule, decode, match_slot},
};
use mc_magpie::{MemoryDevice, Readings, SpdEeprom};

pub struct MemorySlots {
    slots: Vec<MemoryDevice>,
    total: u64,
    modules: Vec<SpdModule>,
    rejected: Vec<(String, String)>,
    status: String,
    pending: bool,
    selected: Option<usize>,
    privileged: bool,
}

impl MemorySlots {
    pub fn new(readings: &Readings) -> Self {
        Self {
            slots: readings.mem_devices.clone(),
            total: readings.mem_info.max_devices,
            modules: vec![],
            rejected: vec![],
            status: String::new(),
            pending: false,
            selected: None,
            privileged: false,
        }
    }
    pub fn update(&mut self, readings: &Readings) {
        if self.slots != readings.mem_devices {
            self.slots.clone_from(&readings.mem_devices);
            self.selected = None;
        }
        self.total = readings.mem_info.max_devices;
    }
    pub fn clear_privileged_spd(&mut self) {
        if self.privileged {
            self.modules.clear();
            self.rejected.clear();
            self.status.clear();
        }
        self.pending = false;
        self.privileged = false;
    }
    pub fn update_spd(&mut self, privileged: bool, result: &Result<Vec<SpdEeprom>, String>) {
        self.privileged = privileged;
        self.pending = false;
        self.modules.clear();
        self.rejected.clear();
        match result {
            Ok(eeproms) => {
                for eeprom in eeproms {
                    match decode(eeprom) {
                        Ok(module) => self.modules.push(module),
                        Err(error) => self.rejected.push((eeprom.source.clone(), error)),
                    }
                }
                self.status = if eeproms.is_empty() {
                    i18n(
                        "No readable SPD EEPROMs. Enable extended hardware monitoring or check the kernel SPD driver.",
                    )
                } else {
                    i18n("SPD read completed.")
                };
            }
            Err(error) => self.status.clone_from(error),
        }
    }
    pub fn ui(&mut self, ui: &mut Ui, rect: Rect, module_height: f32, cx: &mut Cx) {
        if self.privileged && !cx.config.extended_hardware_monitoring {
            self.clear_privileged_spd();
        }
        let painter = ui.painter().clone();
        paint::text(
            &painter,
            rect.left(),
            rect.top(),
            &i18n("Memory slots"),
            Font::Body,
            pal::LABEL,
            Align::Min,
        );
        let area = Rect::from_min_max(rect.min + vec2(0., m::MEMORY_SLOT_HEADER_H), rect.max);
        let cell_height = module_height + m::MEMORY_SLOT_LABEL_H;
        let cell_width = m::MEMORY_SLOT_W
            .min(((area.width() - m::MEMORY_SLOT_GAP) / 2.).max(m::MEMORY_SLOT_DRAW_W + 4.));
        let mut child = ui.new_child(
            egui::UiBuilder::new()
                .id_salt("compact-memory-slots")
                .max_rect(area),
        );
        // Large server inventories stay reachable inside this compact metadata area.
        egui::ScrollArea::both()
            .auto_shrink([false, false])
            .show(&mut child, |ui| {
                let columns = ((area.width() + m::MEMORY_SLOT_GAP)
                    / (cell_width + m::MEMORY_SLOT_GAP))
                    .floor()
                    .max(1.) as usize;
                let rows = self.slots.len().div_ceil(columns).max(1);
                let (grid, _) = ui.allocate_exact_size(
                    vec2(area.width(), rows as f32 * cell_height),
                    egui::Sense::hover(),
                );
                for (index, slot) in self.slots.iter().enumerate() {
                    let cell = Rect::from_min_size(
                        grid.min
                            + vec2(
                                (index % columns) as f32 * (cell_width + m::MEMORY_SLOT_GAP),
                                (index / columns) as f32 * cell_height,
                            ),
                        vec2(cell_width, cell_height),
                    );
                    if !ui.is_rect_visible(cell) {
                        continue;
                    }
                    let name = if slot.locator.is_empty() {
                        format!(
                            "{} {}",
                            i18n("Slot"),
                            slot.slot_index.map_or(index, |i| i as usize) + 1
                        )
                    } else {
                        slot.locator.clone()
                    };
                    let occupied = slot.size > 0;
                    let so = slot
                        .form_factor
                        .to_ascii_uppercase()
                        .replace('-', "")
                        .contains("SODIMM");
                    let height = module_height;
                    let drawing = Rect::from_min_size(
                        cell.min + vec2((cell_width - m::MEMORY_SLOT_DRAW_W) / 2., 0.),
                        vec2(m::MEMORY_SLOT_DRAW_W, height),
                    );
                    let p = ui.painter();
                    paint::outline(
                        p,
                        drawing.left(),
                        drawing.top(),
                        drawing.right(),
                        drawing.bottom(),
                        if occupied {
                            pal::MEMORY.line
                        } else {
                            pal::LABEL
                        },
                    );
                    if occupied {
                        paint::fill(
                            p,
                            drawing.left() + 1.,
                            drawing.top() + 1.,
                            drawing.right() - 1.,
                            drawing.bottom() - 1.,
                            pal::MEMORY.fill,
                        );
                        let chips = if so { 4 } else { 8 };
                        let pitch = (height - 8.) / chips as f32;
                        for chip in 0..chips {
                            let y = drawing.top() + 4. + chip as f32 * pitch;
                            paint::outline(
                                p,
                                drawing.left() + 4.,
                                y,
                                drawing.right() - 4.,
                                y + pitch - 3.,
                                pal::MEMORY.line,
                            );
                        }
                        paint::vline(
                            p,
                            drawing.right() - 2.,
                            drawing.top() + 4.,
                            drawing.bottom() - 4.,
                            pal::MEMORY.line,
                        );
                        paint::hline(
                            p,
                            drawing.right() - 3.,
                            drawing.right(),
                            drawing.top() + height * 0.42,
                            pal::WHITE,
                        );
                    } else {
                        paint::fill(
                            p,
                            drawing.left() + 1.,
                            drawing.top() + 1.,
                            drawing.right() - 1.,
                            drawing.bottom() - 1.,
                            pal::MEMORY_EMPTY_FILL,
                        );
                    }
                    let compact = name
                        .rsplit_once("DIMM")
                        .map(|(_, suffix)| suffix)
                        .filter(|suffix| !suffix.is_empty())
                        .unwrap_or(&name);
                    let short = paint::elide(p, compact, Font::Body, cell_width);
                    paint::text(
                        p,
                        cell.center().x,
                        cell.top() + height + m::MEMORY_SLOT_NAME_DY,
                        &short,
                        Font::Body,
                        pal::TEXT,
                        Align::Center,
                    );
                    let value = if occupied {
                        if slot.size >= 1024 * 1024 * 1024
                            && slot.size.is_multiple_of(1024 * 1024 * 1024)
                        {
                            format!("{} GiB", slot.size / (1024 * 1024 * 1024))
                        } else {
                            to_human_readable_nice(slot.size as f32, &DataType::MemoryBytes)
                        }
                    } else {
                        i18n("Empty")
                    };
                    paint::text(
                        p,
                        cell.center().x,
                        cell.top() + height + m::MEMORY_SLOT_CAPACITY_DY,
                        &paint::elide(p, &value, Font::Body, cell_width),
                        Font::Body,
                        pal::LABEL,
                        Align::Center,
                    );
                    let tooltip = format!(
                        "{name}\n{value} · {}\n{}",
                        slot.form_factor,
                        i18n("Click to view module and read SPD")
                    );
                    let response = ui
                        .interact(
                            cell.intersect(ui.clip_rect()),
                            ui.id().with(("memory-slot", index)),
                            egui::Sense::click(),
                        )
                        .on_hover_text(tooltip);
                    if response.clicked() {
                        self.selected = Some(index);
                        if occupied && !self.pending {
                            cx.send(Command::Magpie(mc_magpie::Request::ReadSpd));
                            self.pending = true;
                            self.status.clear();
                        }
                    }
                }
                if self.slots.is_empty() {
                    ui.label(i18n("Firmware did not expose slot locations."));
                }
                if self.total > self.slots.len() as u64 {
                    ui.label(i18n("Additional slots have unknown locations."));
                }
            });
        self.module_dialog(ui, cx);
    }
    fn module_dialog(&mut self, ui: &Ui, _cx: &mut Cx) {
        let Some(index) = self.selected else {
            return;
        };
        let response = egui::Modal::new(ui.id().with("memory-module")).show(ui.ctx(), |ui| {
            ui.set_width(m::MEMORY_MODULE_DIALOG_W);
            let mut close = false;
            egui::ScrollArea::vertical().max_height(m::SPD_SCROLL_MAX_H).show(ui, |ui| {
                let Some(slot) = self.slots.get(index) else { return; };
                ui.heading(if slot.locator.is_empty() {i18n("Memory module")} else {slot.locator.clone()});
                if slot.size == 0 { ui.label(i18n("Empty")); return; }
                metadata(ui, n_("Capacity"), &to_human_readable_nice(slot.size as f32, &DataType::MemoryBytes));
                for (label,value) in [(n_("Form factor"),Some(&slot.form_factor)),(n_("Type"),Some(&slot.ram_type)),(n_("Manufacturer"),slot.manufacturer.as_ref()),(n_("Part number"),slot.part_number.as_ref()),(n_("Serial number"),slot.serial_number.as_ref())] {
                    if let Some(value) = value.filter(|s| !s.is_empty()) {metadata(ui,label,value);}
                }
                if slot.speed > 0 { metadata(ui,n_("Configured speed (SMBIOS)"),&format!("{} MT/s",slot.speed)); }
                if let Some(v) = slot.configured_voltage_v {metadata(ui,n_("Configured voltage (SMBIOS)"),&format!("{v:.3} V"));}
                if slot.rank > 0 {metadata(ui,n_("Ranks (SMBIOS)"),&slot.rank.to_string());}
                ui.separator();
                ui.label(i18n("JEDEC capabilities stored on the module; these are not the currently applied timings."));
                if self.pending {ui.label(i18n("Reading SPD…"));} else {
                    ui.label(&self.status);
                    if let Some(module) = match_slot(slot,&self.modules,&self.slots) {spd_ui(ui,module);} else {
                        if !self.modules.is_empty() { ui.label(i18n("No unique SPD serial match. Choose an EEPROM below; bus addresses do not identify physical slots.")); }
                        for module in &self.modules {
                            egui::CollapsingHeader::new(format!("{} · {} · {}",module.source,module.kind,module.part_number.as_deref().unwrap_or(""))).show(ui,|ui|spd_ui(ui,module));
                        }
                    }
                    for (source,error) in &self.rejected {ui.label(format!("{source}: {error}"));}
                }
            });
            if ui.button(i18n("Close")).clicked() {close=true;}
            close
        });
        if response.inner || response.should_close() {
            self.selected = None;
        }
    }
}
fn metadata(ui: &mut Ui, label: &str, value: &str) {
    ui.horizontal(|ui| {
        ui.label(i18n(label));
        let response = ui.label(value);
        paint::copy_value(ui, response.rect, value);
    });
}
fn spd_ui(ui: &mut Ui, module: &SpdModule) {
    metadata(ui, n_("SPD source"), &module.source);
    metadata(ui, n_("Form factor"), &module.form_factor);
    if let Some(part) = &module.part_number {
        metadata(ui, n_("Part number"), part);
    }
    if let Some(serial) = &module.serial {
        metadata(ui, n_("Serial number"), serial);
    }
    for (label, value) in &module.rows {
        metadata(ui, label, value);
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn revocation_clears_privileged_spd_even_when_page_is_hidden() {
        let mut slots = MemorySlots::new(&Readings::new());
        let corrupt = SpdEeprom {
            source: "0-0050".into(),
            driver: "ee1004".into(),
            hex: "00".into(),
        };
        slots.update_spd(true, &Ok(vec![corrupt.clone()]));
        assert_eq!(slots.rejected.len(), 1);
        slots.clear_privileged_spd();
        assert!(slots.rejected.is_empty());
        slots.update_spd(false, &Ok(vec![corrupt]));
        slots.clear_privileged_spd();
        assert_eq!(slots.rejected.len(), 1);
    }
}
