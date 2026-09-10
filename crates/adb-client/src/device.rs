use std::process::Output;

use crate::adb::get_system_property_or_default;
use crate::error::AdbError;

/// Connection state reported by `adb devices`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeviceState {
    Device,
    Offline,
    Unauthorized,
    Other(String),
}

impl DeviceState {
    fn parse(raw: &str) -> Self {
        match raw {
            "device" => DeviceState::Device,
            "offline" => DeviceState::Offline,
            "unauthorized" => DeviceState::Unauthorized,
            other => DeviceState::Other(other.to_string()),
        }
    }
}

/// A device or emulator visible to `adb`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceInfo {
    pub serial: String,
    pub model: String,
    pub state: DeviceState,
    pub release: String,
    pub build: String,
}

impl DeviceInfo {
    pub(crate) fn is_online(&self) -> bool {
        self.state == DeviceState::Device
    }

    pub(crate) fn fetch_details(&mut self) {
        if self.is_online() {
            if self.model.is_empty() {
                self.model = get_system_property_or_default(&self.serial, "ro.product.model");
            }
            self.release = get_system_property_or_default(&self.serial, "ro.build.version.release");
            self.build = get_system_property_or_default(&self.serial, "ro.build.display.id");
        }
        if self.model.is_empty() {
            self.model = self.serial.clone();
        }
    }

    pub fn display_details(&self) -> String {
        if self.release.is_empty() {
            return format!("{})", self.build);
        }
        format!("{} (Android {}, {})", self.serial, self.release, self.build)
    }
}

pub(crate) fn devices_from_output(output: &Output) -> Result<Vec<DeviceInfo>, AdbError> {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut devices = Vec::new();

    for line in stdout.lines().skip(1) {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let device = parse_device_line(line).ok_or_else(|| {
            AdbError::ParseFailed(format!("failed to parse adb devices line: {line}"))
        })?;

        devices.push(device);
    }

    Ok(devices)
}




fn parse_device_line(line: &str) -> Option<DeviceInfo> {
    let mut parts = line.split_whitespace();
    let serial = parts.next()?.to_string();
    let state = DeviceState::parse(parts.next()?);

    let mut model = String::new();
    for part in parts {
        if let Some(value) = part.strip_prefix("model:") {
            model = value.to_string();
            break;
        }
    }

    Some(DeviceInfo {
        serial,
        model,
        state,
        build: "".to_string(),
        release: "".to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_device_line_with_model() {
        let device = parse_device_line(
            "emulator-5554 device product:sdk_gphone64_arm64 model:sdk_gphone64_arm64 device:emu64a transport_id:1",
        )
        .unwrap();

        assert_eq!(device.serial, "emulator-5554");
        assert_eq!(device.state, DeviceState::Device);
        assert_eq!(device.model, "sdk_gphone64_arm64");
    }

    #[test]
    fn parse_device_line_without_model() {
        let device = parse_device_line("ABCD1234 device").unwrap();

        assert_eq!(device.serial, "ABCD1234");
        assert_eq!(device.state, DeviceState::Device);
        assert_eq!(device.model, "");
    }

    #[test]
    fn parse_unauthorized_device() {
        let device = parse_device_line("R58M123ABC unauthorized").unwrap();

        assert_eq!(device.serial, "R58M123ABC");
        assert_eq!(device.state, DeviceState::Unauthorized);
        assert_eq!(device.model, "");
    }

    #[test]
    fn parse_offline_device() {
        let device = parse_device_line("emulator-5556 offline").unwrap();

        assert_eq!(device.serial, "emulator-5556");
        assert_eq!(device.state, DeviceState::Offline);
    }

    #[test]
    fn devices_from_sample_output() {
        let output = Output {
            status: std::process::ExitStatus::default(),
            stdout: b"List of devices attached\nemulator-5554 device product:sdk model:sdk_gphone64_arm64 device:emu64a transport_id:1\n"
                .to_vec(),
            stderr: Vec::new(),
        };

        let devices = devices_from_output(&output).unwrap();
        assert_eq!(devices.len(), 1);
        assert_eq!(devices[0].serial, "emulator-5554");
        assert_eq!(devices[0].model, "sdk_gphone64_arm64");
        assert_eq!(devices[0].release, "");
        assert_eq!(devices[0].build, "");
    }

    #[test]
    fn details_falls_model_back_to_serial_without_adb() {
        let mut device = parse_device_line("R58M123ABC offline").unwrap();
        device.fetch_details();

        assert_eq!(device.model, "R58M123ABC");
        assert_eq!(device.release, "");
        assert_eq!(device.build, "");
    }
}
