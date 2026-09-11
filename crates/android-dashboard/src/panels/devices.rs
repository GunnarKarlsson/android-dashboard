use adb_client::DeviceState;
use eframe::egui;

use crate::roster::DeviceRoster;
use crate::theme;
use crate::ui_elements;

pub enum DevicesAction {
    Refresh,
    Select(String),
}

pub fn devices_panel(
    ui: &mut egui::Ui,
    roster: &DeviceRoster,
    icon: Option<egui::ImageSource<'static>>,
) -> Option<DevicesAction> {
    let mut refresh = false;
    let mut selected = None;
    let refreshed_at = roster.devices_refreshed_at;
    let list_job_in_flight = roster.list_job_in_flight;
    ui_elements::panel_with_custom_footer(
        ui,
        icon,
        "Devices",
        |_| {},
        |ui| {
            selected = show_devices_body(ui, roster);
        },
        |ui| {
            refresh = ui_elements::devices_footer(ui, refreshed_at, list_job_in_flight);
        },
    );
    if refresh {
        Some(DevicesAction::Refresh)
    } else {
        selected.map(DevicesAction::Select)
    }
}

fn show_devices_body(ui: &mut egui::Ui, roster: &DeviceRoster) -> Option<String> {
    if let Some(error) = &roster.adb_error {
        ui_elements::error_label(ui, "ADB not available");
        ui.label(error);
        return None;
    }

    if let Some(error) = &roster.list_error {
        ui_elements::error_label(ui, error);
    }

    if roster.devices.is_empty() {
        if !roster.list_job_in_flight {
            ui.label("No devices found.");
        }
        return None;
    }

    let mut selected = None;
    egui::ScrollArea::vertical()
        .id_salt(egui::Id::new("device_list"))
        .auto_shrink([false, false])
        .max_height(ui.available_height())
        .show(ui, |ui| {
            ui.with_layout(egui::Layout::top_down_justified(egui::Align::LEFT), |ui| {
                let device_count = roster.devices.len();
                for index in 0..device_count {
                    let device = &roster.devices[index];
                    let is_selected =
                        roster.selected_serial.as_deref() == Some(device.serial.as_str());
                    let label = format!("{}\n{}", device.model, device.display_details());

                    if device.state == DeviceState::Device {
                        let color = if is_selected {
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
                        if response.clicked() && !is_selected {
                            selected = Some(roster.devices[index].serial.clone());
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
    selected
}

fn device_state_label(state: &DeviceState) -> &str {
    match state {
        DeviceState::Device => "device",
        DeviceState::Offline => "offline",
        DeviceState::Unauthorized => "unauthorized",
        DeviceState::Other(value) => value,
    }
}
