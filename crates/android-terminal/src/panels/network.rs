use adb_client::NetworkStats;
use eframe::egui;

use crate::app::App;
use crate::format::{format_bytes_mb, format_rate_mb};
use crate::theme;
use crate::ui_elements;

pub fn network_panel(ui: &mut egui::Ui, app: &App) {
    if let Some(error) = &app.network_error {
        ui_elements::error_label(ui, error);
    }

    if app.roster.selected_serial.is_none() {
        ui_elements::panel_loading(ui);
        return;
    }

    let Some(stats) = &app.network_stats else {
        if app.network_rx.is_some() {
            ui.label("Fetching network stats…");
        } else {
            ui_elements::panel_loading(ui);
        }
        return;
    };

    if stats.interfaces.is_empty() {
        ui.label("No network interfaces reported.");
        return;
    }

    let rows = network_rows_from_stats(stats);
    show_network_table(ui, &rows);
}

fn show_network_table(ui: &mut egui::Ui, stats: &[NetworkRow]) {
    egui::ScrollArea::both()
        .id_salt(egui::Id::new("network_table_scroll"))
        .auto_shrink([false, false])
        .max_height(ui.available_height())
        .show(ui, |ui| {
            egui::Grid::new("network_stats")
                .num_columns(5)
                .spacing(theme::GRID_SPACING)
                .striped(true)
                .show(ui, |ui| {
                    ui.label("Interface");
                    ui.label("Transport");
                    ui.label("RX");
                    ui.label("TX");
                    ui.label("Rate");
                    ui.end_row();

                    for row in stats {
                        ui.label(&row.interface);
                        ui.label(&row.transport);
                        ui.label(&row.rx);
                        ui.label(&row.tx);
                        ui.label(&row.rate);
                        ui.end_row();
                    }
                });
        });
}

#[derive(Clone)]
struct NetworkRow {
    interface: String,
    transport: String,
    rx: String,
    tx: String,
    rate: String,
}

fn network_rows_from_stats(stats: &NetworkStats) -> Vec<NetworkRow> {
    let mut interfaces: Vec<_> = stats.interfaces.iter().collect();
    interfaces.sort_by(|a, b| b.tx_bytes.cmp(&a.tx_bytes));
    interfaces
        .into_iter()
        .map(|iface| NetworkRow {
            interface: iface.interface.clone(),
            transport: iface.transport.clone(),
            rx: format_bytes_mb(iface.rx_bytes),
            tx: format_bytes_mb(iface.tx_bytes),
            rate: format_rate_mb(iface.rx_rate_bps, iface.tx_rate_bps),
        })
        .collect()
}
