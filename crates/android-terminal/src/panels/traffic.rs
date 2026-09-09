use adb_client::ProtocolStats;
use eframe::egui;

use crate::app::App;
use crate::format::format_bytes_mb;
use crate::theme;
use crate::ui_elements;

pub fn protocols_panel(ui: &mut egui::Ui, app: &App) {
    if let Some(error) = &app.metrics.protocol_error {
        ui_elements::error_label(ui, error);
    }

    ui_elements::panel_body(ui, theme::colors::APP_TRAFFIC_BODY, |ui| {
        if app.roster.selected_serial.is_none() {
            ui_elements::panel_loading(ui);
            return;
        }

        let Some(stats) = &app.metrics.protocol_stats else {
            if app.has_protocol() {
                ui.label("Fetching app traffic…");
            } else {
                ui_elements::panel_loading(ui);
            }
            return;
        };

        show_protocol_stats(ui, stats);
    });
}

fn show_protocol_stats(ui: &mut egui::Ui, stats: &ProtocolStats) {
    if stats.apps.is_empty() {
        ui.label("No app traffic reported.");
        return;
    }

    egui::ScrollArea::both()
        .id_salt(egui::Id::new("app_traffic_scroll"))
        .auto_shrink([false, false])
        .max_height(ui.available_height())
        .show(ui, |ui| {
            egui::Grid::new("app_traffic")
                .num_columns(6)
                .spacing(theme::GRID_SPACING)
                .striped(true)
                .show(ui, |ui| {
                    ui.label("Package");
                    ui.label("Total");
                    ui.label("Foreground");
                    ui.label("Background");
                    ui.label("WiFi");
                    ui.label("Mobile");
                    ui.end_row();

                    for app in &stats.apps {
                        ui.label(&app.package);
                        ui.label(format_bytes_mb(app.total_bytes));
                        ui.label(format_bytes_mb(app.foreground_bytes));
                        ui.label(format_bytes_mb(app.background_bytes));
                        ui.label(format_bytes_mb(app.wifi_bytes));
                        ui.label(format_bytes_mb(app.mobile_bytes));
                        ui.end_row();
                    }
                });
        });
}
