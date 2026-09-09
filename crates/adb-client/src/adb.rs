use std::io;
use std::process::{Command, Output};
use std::sync::{LazyLock, Mutex};

use crate::error::AdbError;

/// All `adb -s` shells share this mutex; poller threads do not run shells in parallel.
static ADB_SHELL_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

/// Entry point for running `adb` commands.
pub struct Adb;

impl Adb {
    /// Verifies that `adb` is installed and runnable.
    pub fn check_available() -> Result<(), AdbError> {
        let output = run_adb(&["version"])?;

        if output.status.success() {
            Ok(())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
            Err(AdbError::VersionCheckFailed(stderr))
        }
    }

    /// Lists devices attached to the host via `adb devices -l`.
    pub fn list_devices() -> Result<Vec<crate::device::DeviceInfo>, AdbError> {
        let output = run_adb(&["devices", "-l"])?;
        crate::device::devices_from_output(&output)
    }
}

pub(crate) fn run_adb(args: &[&str]) -> Result<Output, AdbError> {
    let output = Command::new("adb")
        .args(args)
        .output()
        .map_err(map_io_error)?;

    if output.status.success() {
        Ok(output)
    } else {
        let command = format!("adb {}", args.join(" "));
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        Err(AdbError::CommandFailed { command, stderr })
    }
}

pub(crate) fn run_adb_for_serial(serial: &str, args: &[&str]) -> Result<Output, AdbError> {
    let _guard = ADB_SHELL_LOCK.lock().expect("adb shell lock");
    let mut full_args = Vec::with_capacity(2 + args.len());
    full_args.push("-s");
    full_args.push(serial);
    full_args.extend_from_slice(args);
    run_adb(&full_args)
}

fn map_io_error(err: io::Error) -> AdbError {
    if err.kind() == io::ErrorKind::NotFound {
        AdbError::NotFound
    } else {
        AdbError::Io(err)
    }
}
