use adb_client::{StorageCategory, StorageOverview};
use eframe::egui;

use crate::app::{App, AppStorageState};
use crate::format::{format_bytes, format_bytes_mb, format_gb_from_bytes, truncate_package_name};
use crate::theme;
use crate::ui_elements;

use super::donut::show_usage_donut;

pub fn storage_usage_panel(ui: &mut egui::Ui, app: &App) {
    if let Some(error) = &app.storage_breakdown_error {
        ui_elements::error_label(ui, error);
    }
    if let Some(error) = &app.app_storage.error {
        ui_elements::error_label(ui, error);
    }

    ui_elements::panel_body(ui, theme::colors::MEMORY_DISK_BODY, |ui| {
        if app.selected_serial.is_none() {
            ui_elements::panel_loading(ui);
            return;
        }

        egui::ScrollArea::both()
            .id_salt(egui::Id::new("storage_usage_scroll"))
            .auto_shrink([false, false])
            .max_height(ui.available_height())
            .show(ui, |ui| {
                if let Some(breakdown) = &app.storage_breakdown {
                    show_storage_categories(ui, &breakdown.categories);
                } else if app.storage_breakdown_rx.is_some() {
                    ui.label("Loading storage…");
                }
                ui.separator();
                show_app_storage(ui, &app.app_storage);
            });
    });
}

pub fn storage_gauge_panel(ui: &mut egui::Ui, app: &App) {
    if app.selected_serial.is_none() {
        ui_elements::panel_loading(ui);
        return;
    }

    if let Some(error) = &app.storage_gauge_error {
        ui_elements::error_label(ui, error);
    }

    let Some(overview) = &app.storage_gauge else {
        if app.storage_gauge_rx.is_some() {
            ui_elements::panel_loading(ui);
        } else {
            ui_elements::panel_loading(ui);
        }
        return;
    };

    show_storage_donut(ui, overview);
}

fn show_storage_donut(ui: &mut egui::Ui, overview: &StorageOverview) {
    show_usage_donut(
        ui,
        egui::Id::new("storage_gauge"),
        overview.used_fraction(),
        format_gb_from_bytes(overview.used_bytes),
        format_gb_from_bytes(overview.total_bytes),
        theme::colors::STORAGE_TRACK,
        theme::colors::STORAGE_USED,
    );
}

fn show_storage_categories(ui: &mut egui::Ui, categories: &[StorageCategory]) {
    ui.label("Storage breakdown");

    egui::Grid::new("storage_categories")
        .num_columns(2)
        .spacing([12.0, 4.0])
        .striped(true)
        .show(ui, |ui| {
            ui.label("Category");
            ui.label("Size");
            ui.end_row();

            for category in categories {
                ui.label(&category.name);
                ui.label(format_bytes(category.bytes));
                ui.end_row();
            }
        });
}

fn show_app_storage(ui: &mut egui::Ui, storage: &AppStorageState) {
    ui.horizontal(|ui| {
        ui.label("Apps");
        if storage.scanning {
            ui.label(format!(
                "({}/{})",
                storage.sizes.len(),
                storage.packages.len()
            ));
        }
    });

    if storage.packages.is_empty() {
        ui.label("Loading package list…");
        return;
    }

    egui::Grid::new("app_storage")
        .num_columns(2)
        .spacing([12.0, 4.0])
        .striped(true)
        .show(ui, |ui| {
            ui.label("Package");
            ui.label("Storage");
            ui.end_row();

            for (package, bytes) in storage.sorted_rows() {
                ui.label(truncate_package_name(package))
                    .on_hover_text(package);
                ui.label(match bytes {
                    Some(value) => format_bytes_mb(value),
                    None => "…".to_string(),
                });
                ui.end_row();
            }
        });
}
