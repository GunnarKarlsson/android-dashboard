use std::time::Instant;

use adb_client::DeviceInfo;

#[allow(dead_code)]
pub struct DeviceRoster {
    pub adb_error: Option<String>,
    pub devices: Vec<DeviceInfo>,
    pub list_error: Option<String>,
    pub devices_refreshed_at: Option<Instant>,
    pub selected_serial: Option<String>,
}
