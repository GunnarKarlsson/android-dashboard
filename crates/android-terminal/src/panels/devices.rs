use adb_client::DeviceState;
use eframe::egui;

use crate::app::App;
use crate::theme;
use crate::ui_elements;

pub fn devices_panel(ui: &mut egui::Ui, app: &mut App, icon: Option<egui::ImageSource<'static>>) {
    let mut refresh = false;
    let refreshed_at = app.devices_refreshed_at;
    ui_elements::panel_with_custom_footer(
        ui,
        icon,
        "Devices",
        |_| {},
        |ui| {
            show_devices_body(ui, app);
        },
        |ui| {
            refresh = ui_elements::devices_footer(ui, refreshed_at);
        },
    );
    if refresh {
        app.refresh_devices();
    }
}

fn show_devices_body(ui: &mut egui::Ui, app: &mut App) {
    if let Some(error) = &app.adb_error {
        ui_elements::error_label(ui, "ADB not available");
        ui.label(error);
        return;
    }

    if let Some(error) = &app.list_error {
        ui_elements::error_label(ui, error);
    }

    if app.devices.is_empty() {
        ui.label("No devices found.");
        return;
    }

    egui::ScrollArea::vertical()
        .id_salt(egui::Id::new("device_list"))
        .auto_shrink([false, false])
        .max_height(ui.available_height())
        .show(ui, |ui| {
            ui.with_layout(egui::Layout::top_down_justified(egui::Align::LEFT), |ui| {
                let device_count = app.devices.len();
                for index in 0..device_count {
                    let device = &app.devices[index];
                    let selected = app.selected_serial.as_deref() == Some(device.serial.as_str());
                    let label = format!("{}\n{}", device.model, device.serial);

                    if device.state == DeviceState::Device {
                        let color = if selected {
                            theme::colors::OFF_WHITE
                        } else {
                            theme::colors::HEADER_ICON
                        };
                        let response = ui
                            .add(
                                egui::Label::new(egui::RichText::new(label).color(color))
                                    .sense(egui::Sense::click()),
                            )
                            .on_hover_cursor(egui::CursorIcon::PointingHand);
                        if response.clicked() && !selected {
                            let serial = app.devices[index].serial.clone();
                            app.select_device(serial);
                        }
                    } else {
                        ui.add_enabled_ui(false, |ui| {
                            ui.label(format!(
                                "{}\n{} ({})",
                                device.model,
                                device.serial,
                                device_state_label(&device.state)
                            ));
                        });
                    }
                }
            });
        });
}

fn device_state_label(state: &DeviceState) -> &str {
    match state {
        DeviceState::Device => "device",
        DeviceState::Offline => "offline",
        DeviceState::Unauthorized => "unauthorized",
        DeviceState::Other(value) => value,
    }
}
