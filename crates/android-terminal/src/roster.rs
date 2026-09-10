use std::time::Instant;

use adb_client::{DeviceInfo, DeviceListJob, DeviceListUpdate, DeviceState};
use crossbeam_channel::Receiver;

pub struct DeviceRoster {
    pub adb_error: Option<String>,
    pub devices: Vec<DeviceInfo>,
    pub list_error: Option<String>,
    pub devices_refreshed_at: Option<Instant>,
    pub selected_serial: Option<String>,
    pub list_job_in_flight: bool,
    list_generation: u64,
    list_rx: Option<Receiver<DeviceListUpdate>>,
    list_job: Option<DeviceListJob>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum RosterEvent {
    Unchanged,
    LostSelection,
    AutoSelect(String),
}

impl DeviceRoster {
    pub fn new(adb_error: Option<String>) -> Self {
        Self {
            adb_error,
            devices: Vec::new(),
            list_error: None,
            devices_refreshed_at: None,
            selected_serial: None,
            list_job_in_flight: false,
            list_generation: 0,
            list_rx: None,
            list_job: None,
        }
    }

    /// Starts a background device list if one is not already in flight.
    pub fn request_refresh(&mut self) {
        if self.list_job_in_flight {
            return;
        }
        self.list_error = None;
        self.list_job_in_flight = true;
        self.list_generation = self.list_generation.wrapping_add(1);
        self.list_rx = None;
        self.list_job = None;
        let (rx, job) = DeviceListJob::spawn(self.list_generation);
        self.list_rx = Some(rx);
        self.list_job = Some(job);
    }

    /// Applies finished list updates that match the current generation.
    pub fn drain_list(&mut self) -> Option<RosterEvent> {
        let rx = self.list_rx.as_ref()?;

        let Ok(update) = rx.try_recv() else {
            return None;
        };

        let generation = match &update {
            DeviceListUpdate::Devices { generation, .. }
            | DeviceListUpdate::Error { generation, .. } => *generation,
        };
        if generation != self.list_generation {
            return None;
        }

        self.list_job_in_flight = false;
        self.list_rx = None;
        self.list_job = None;
        self.devices_refreshed_at = Some(Instant::now());

        Some(match update {
            DeviceListUpdate::Devices { devices, .. } => {
                self.list_error = None;
                self.apply_devices(devices)
            }
            DeviceListUpdate::Error { message, .. } => {
                self.list_error = Some(message);
                RosterEvent::Unchanged
            }
        })
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
    use crossbeam_channel::Sender;

    fn roster(selected: Option<&str>) -> DeviceRoster {
        DeviceRoster {
            adb_error: None,
            devices: Vec::new(),
            list_error: None,
            devices_refreshed_at: None,
            selected_serial: selected.map(str::to_string),
            list_job_in_flight: false,
            list_generation: 0,
            list_rx: None,
            list_job: None,
        }
    }

    fn device(serial: &str, state: DeviceState) -> DeviceInfo {
        DeviceInfo {
            serial: serial.to_string(),
            model: serial.to_string(),
            state,
            release: "".to_string(),
            build: "".to_string(),
        }
    }

    /// Arms a list job with a test channel instead of spawning `DeviceListJob`.
    fn begin_list_job(roster: &mut DeviceRoster) -> Sender<DeviceListUpdate> {
        let (tx, rx) = crossbeam_channel::unbounded();
        roster.list_error = None;
        roster.list_job_in_flight = true;
        roster.list_generation = roster.list_generation.wrapping_add(1);
        roster.list_rx = Some(rx);
        roster.list_job = None;
        tx
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

    #[test]
    fn begin_list_job_sets_in_flight() {
        let mut roster = roster(None);
        let _tx = begin_list_job(&mut roster);
        assert!(roster.list_job_in_flight);
        assert_eq!(roster.list_generation, 1);
    }

    #[test]
    fn matching_devices_applies_and_clears_in_flight() {
        let mut roster = roster(None);
        let tx = begin_list_job(&mut roster);
        let generation = roster.list_generation;
        tx.send(DeviceListUpdate::Devices {
            generation,
            devices: vec![device("a", DeviceState::Device)],
        })
        .unwrap();

        let event = roster.drain_list();
        assert_eq!(event, Some(RosterEvent::AutoSelect("a".to_string())));
        assert!(!roster.list_job_in_flight);
        assert_eq!(roster.devices.len(), 1);
        assert!(roster.devices_refreshed_at.is_some());
        assert!(roster.list_rx.is_none());
    }

    #[test]
    fn list_error_leaves_devices_and_selection() {
        let mut roster = roster(Some("a"));
        roster.devices = vec![device("a", DeviceState::Device)];
        let tx = begin_list_job(&mut roster);
        let generation = roster.list_generation;
        tx.send(DeviceListUpdate::Error {
            generation,
            message: "boom".to_string(),
        })
        .unwrap();

        let event = roster.drain_list();
        assert_eq!(event, Some(RosterEvent::Unchanged));
        assert_eq!(roster.list_error.as_deref(), Some("boom"));
        assert_eq!(roster.selected_serial.as_deref(), Some("a"));
        assert_eq!(roster.devices.len(), 1);
        assert!(!roster.list_job_in_flight);
    }

    #[test]
    fn mismatched_generation_is_ignored() {
        let mut roster = roster(None);
        let tx = begin_list_job(&mut roster);
        tx.send(DeviceListUpdate::Devices {
            generation: roster.list_generation.wrapping_sub(1),
            devices: vec![device("stale", DeviceState::Device)],
        })
        .unwrap();

        assert_eq!(roster.drain_list(), None);
        assert!(roster.list_job_in_flight);
        assert!(roster.devices.is_empty());
    }

    #[test]
    fn request_refresh_while_in_flight_is_noop() {
        let mut roster = roster(None);
        let _tx = begin_list_job(&mut roster);
        let generation = roster.list_generation;
        roster.request_refresh();
        assert!(roster.list_job_in_flight);
        assert_eq!(roster.list_generation, generation);
        assert!(roster.list_job.is_none());
    }
}
