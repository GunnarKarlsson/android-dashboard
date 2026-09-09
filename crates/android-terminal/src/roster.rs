use std::time::Instant;

use adb_client::{Adb, DeviceInfo, DeviceState};

pub struct DeviceRoster {
    pub adb_error: Option<String>,
    pub devices: Vec<DeviceInfo>,
    pub list_error: Option<String>,
    pub devices_refreshed_at: Option<Instant>,
    pub selected_serial: Option<String>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum RosterEvent {
    Unchanged,
    LostSelection,
    AutoSelect(String),
}

impl DeviceRoster {
    /// Relists connected devices and reports whether the current selection is still valid.
    /// Listing is synchronous and can block a frame. Do not move it off the UI thread.
    pub fn refresh(&mut self) -> RosterEvent {
        self.list_error = None;
        self.devices_refreshed_at = Some(Instant::now());
        match Adb::list_devices() {
            Ok(devices) => self.apply_devices(devices),
            Err(err) => {
                self.list_error = Some(err.user_message());
                RosterEvent::Unchanged
            }
        }
    }

    /// Replaces the device list and reports whether the current selection is still valid.
    fn apply_devices(&mut self, devices: Vec<DeviceInfo>) -> RosterEvent {
        self.devices = devices;
        let lost_selection = self.selected_serial.as_ref().is_some_and(|serial| {
            !self
                .devices
                .iter()
                .any(|device| device.serial == *serial && device.state == DeviceState::Device)
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
}

/// Returns the serial of the first device whose state is `Device`.
pub(crate) fn first_ready_serial(devices: &[DeviceInfo]) -> Option<String> {
    devices
        .iter()
        .find(|device| device.state == DeviceState::Device)
        .map(|device| device.serial.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roster(selected: Option<&str>) -> DeviceRoster {
        DeviceRoster {
            adb_error: None,
            devices: Vec::new(),
            list_error: None,
            devices_refreshed_at: None,
            selected_serial: selected.map(str::to_string),
        }
    }

    fn device(serial: &str, state: DeviceState) -> DeviceInfo {
        DeviceInfo {
            serial: serial.to_string(),
            model: serial.to_string(),
            state,
        }
    }

    #[test]
    fn selected_still_connected_is_unchanged() {
        let mut roster = roster(Some("a"));
        let event = roster.apply_devices(vec![device("a", DeviceState::Device)]);
        assert_eq!(event, RosterEvent::Unchanged);
    }

    #[test]
    fn selected_gone_another_device_auto_selects() {
        let mut roster = roster(Some("gone"));
        let event = roster.apply_devices(vec![device("b", DeviceState::Device)]);
        assert_eq!(event, RosterEvent::AutoSelect("b".to_string()));
    }

    #[test]
    fn selected_gone_none_ready_loses_selection() {
        let mut roster = roster(Some("gone"));
        let event = roster.apply_devices(vec![device("b", DeviceState::Offline)]);
        assert_eq!(event, RosterEvent::LostSelection);
    }

    #[test]
    fn none_selected_device_appears_auto_selects() {
        let mut roster = roster(None);
        let event = roster.apply_devices(vec![device("a", DeviceState::Device)]);
        assert_eq!(event, RosterEvent::AutoSelect("a".to_string()));
    }
}
