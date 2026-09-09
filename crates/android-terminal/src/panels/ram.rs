use eframe::egui;

use crate::format::format_gb_from_kb;
use crate::metrics::MetricStore;
use crate::theme;
use crate::ui_elements;

use super::donut::show_usage_donut;

pub fn ram_gauge_panel(ui: &mut egui::Ui, metrics: &MetricStore) {
    if let Some(error) = &metrics.ram_error {
        ui_elements::error_label(ui, error);
    }

    let Some(memory) = &metrics.ram_memory else {
        ui_elements::panel_loading(ui);
        return;
    };

    show_ram_donut(ui, memory);
}

fn show_ram_donut(ui: &mut egui::Ui, memory: &adb_client::MemoryStats) {
    show_usage_donut(
        ui,
        egui::Id::new("ram_gauge"),
        memory.used_fraction(),
        format_gb_from_kb(memory.used_kb()),
        format_gb_from_kb(memory.total_kb),
        theme::colors::RAM_TRACK,
        theme::colors::RAM_USED,
    );
}
