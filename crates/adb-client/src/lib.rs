//! ADB client library for Android device communication.

mod adb;
mod app_storage;
mod background;
mod device;
mod error;
mod logcat;
mod network;
mod protocols;
mod ram;
mod stats;
mod storage_breakdown;
mod storage_gauge;

pub use adb::Adb;
pub use app_storage::{AppStoragePoller, AppStorageUpdate, PackageStorage};
pub use device::{DeviceInfo, DeviceState};
pub use error::AdbError;
pub use logcat::{LogEntry, LogcatStream};
pub use network::{NetworkInterfaceStats, NetworkPoller, NetworkStats, NetworkUpdate};
pub use protocols::{AppTraffic, ProtocolPoller, ProtocolStats, ProtocolUpdate};
pub use ram::{RamPoller, RamUpdate};
pub use stats::MemoryStats;
pub use storage_breakdown::{
    fetch_storage_overview, StorageBreakdown, StorageBreakdownPoller, StorageBreakdownUpdate,
    StorageCategory, StorageOverview,
};
pub use storage_gauge::{StorageGaugePoller, StorageGaugeUpdate};
