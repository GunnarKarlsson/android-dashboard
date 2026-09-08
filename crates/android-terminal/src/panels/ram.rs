use eframe::egui;

use crate::app::App;
use crate::format::format_gb_from_kb;
use crate::theme;
use crate::ui_elements;

use super::donut::show_usage_donut;

pub fn ram_gauge_panel(ui: &mut egui::Ui, app: &App) {
    if app.selected_serial.is_none() {
        ui_elements::panel_loading(ui);
        return;
    }

    if let Some(error) = &app.ram_error {
        ui_elements::error_label(ui, error);
    }

    let Some(memory) = &app.ram_memory else {
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
