use adb_client::{
    AppStoragePoller, AppStorageUpdate, LogEntry, LogcatStream, NetworkPoller, NetworkUpdate,
    ProtocolPoller, ProtocolUpdate, RamPoller, RamUpdate, StorageBreakdownPoller,
    StorageBreakdownUpdate, StorageGaugePoller, StorageGaugeUpdate,
};
use crossbeam_channel::Receiver;

#[allow(dead_code)]
pub struct DeviceSession {
    logcat_rx: Option<Receiver<LogEntry>>,
    logcat_stream: Option<LogcatStream>,
    ram_rx: Option<Receiver<RamUpdate>>,
    ram_poller: Option<RamPoller>,
    storage_gauge_rx: Option<Receiver<StorageGaugeUpdate>>,
    storage_gauge_poller: Option<StorageGaugePoller>,
    storage_breakdown_rx: Option<Receiver<StorageBreakdownUpdate>>,
    storage_breakdown_poller: Option<StorageBreakdownPoller>,
    network_rx: Option<Receiver<NetworkUpdate>>,
    network_poller: Option<NetworkPoller>,
    protocol_rx: Option<Receiver<ProtocolUpdate>>,
    protocol_poller: Option<ProtocolPoller>,
    app_storage_rx: Option<Receiver<AppStorageUpdate>>,
    app_storage_poller: Option<AppStoragePoller>,
}

impl Default for DeviceSession {
    fn default() -> Self {
        Self {
            logcat_rx: None,
            logcat_stream: None,
            ram_rx: None,
            ram_poller: None,
            storage_gauge_rx: None,
            storage_gauge_poller: None,
            storage_breakdown_rx: None,
            storage_breakdown_poller: None,
            network_rx: None,
            network_poller: None,
            protocol_rx: None,
            protocol_poller: None,
            app_storage_rx: None,
            app_storage_poller: None,
        }
    }
}
