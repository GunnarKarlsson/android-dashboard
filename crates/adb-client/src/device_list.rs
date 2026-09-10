use std::thread::{self, JoinHandle};

use crossbeam_channel::Receiver;

use crate::adb::Adb;
use crate::device::DeviceInfo;

/// Result of one background `adb devices -l` run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeviceListUpdate {
    Devices {
        generation: u64,
        devices: Vec<DeviceInfo>,
    },
    Error {
        generation: u64,
        message: String,
    },
}

/// One-shot thread that lists devices and sends a single [`DeviceListUpdate`].
pub struct DeviceListJob {
    join_handle: Option<JoinHandle<()>>,
}

impl DeviceListJob {
    /// Spawns a thread that calls [`Adb::list_devices`] and sends one update.
    ///
    /// - `generation` — caller sequence id; echoed on the update
    pub fn spawn(generation: u64) -> (Receiver<DeviceListUpdate>, Self) {
        let (tx, rx) = crossbeam_channel::unbounded();
        let join_handle = thread::spawn(move || {
            let update = match Adb::list_devices() {
                Ok(devices) => DeviceListUpdate::Devices {
                    generation,
                    devices,
                },
                Err(err) => DeviceListUpdate::Error {
                    generation,
                    message: err.user_message(),
                },
            };
            let _ = tx.send(update);
        });
        (
            rx,
            DeviceListJob {
                join_handle: Some(join_handle),
            },
        )
    }
}

impl Drop for DeviceListJob {
    fn drop(&mut self) {
        let _ = self.join_handle.take();
    }
}
