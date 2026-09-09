use std::time::Instant;

use adb_client::{Adb, DeviceInfo, DeviceState};

pub struct DeviceRoster {
    pub adb_error: Option<String>,
    pub devices: Vec<DeviceInfo>,
    pub list_error: Option<String>,
    pub devices_refreshed_at: Option<Instant>,
    pub selected_serial: Option<String>,
}

pub enum RosterEvent {
    Unchanged,
    LostSelection,
    AutoSelect(String),
}

impl DeviceRoster {
    /// Relists connected devices and reports whether the current selection is still valid.
    pub fn refresh(&mut self) -> RosterEvent {
        self.list_error = None;
        self.devices_refreshed_at = Some(Instant::now());
        match Adb::list_devices() {
            Ok(devices) => {
                self.devices = devices;
                let lost_selection = self.selected_serial.as_ref().is_some_and(|serial| {
                    !self.devices.iter().any(|device| {
                        device.serial == *serial && device.state == DeviceState::Device
                    })
                });

                if lost_selection {
                    if let Some(serial) = first_ready_serial(&self.devices) {
                        return RosterEvent::AutoSelect(serial);
                    }
                    return RosterEvent::LostSelection;
                }

                if self.selected_serial.is_none() {
                    if let Some(serial) = first_ready_serial(&self.devices) {
                        return RosterEvent::AutoSelect(serial);
                    }
                }

                RosterEvent::Unchanged
            }
            Err(err) => {
                self.list_error = Some(err.user_message());
                RosterEvent::Unchanged
            }
        }
    }
}

/// Returns the serial of the first device whose state is `Device`.
pub(crate) fn first_ready_serial(devices: &[DeviceInfo]) -> Option<String> {
    devices
        .iter()
        .find(|device| device.state == DeviceState::Device)
        .map(|device| device.serial.clone())
}
