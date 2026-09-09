use adb_client::{
    AppStoragePoller, AppStorageUpdate, LogEntry, LogcatStream, NetworkPoller, NetworkUpdate,
    ProtocolPoller, ProtocolUpdate, RamPoller, RamUpdate, StorageBreakdownPoller,
    StorageBreakdownUpdate, StorageGaugePoller, StorageGaugeUpdate,
};
use crossbeam_channel::Receiver;

use crate::logcat_pane::LogcatPane;
use crate::metrics::MetricStore;

const MAX_DRAIN_PER_FRAME: usize = 500;

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

#[derive(Default)]
pub struct SessionStartErrors {
    pub logcat: Option<String>,
    pub ram: Option<String>,
    pub storage_gauge: Option<String>,
    pub storage_breakdown: Option<String>,
    pub network: Option<String>,
    pub protocol: Option<String>,
    pub app_storage: Option<String>,
}

pub struct DrainOutcome {
    pub ui_changed: bool,
    pub error_accepted: bool,
}

impl DeviceSession {
    /// Spawns one logcat child and the metric pollers for `serial`.
    pub fn start(&mut self, serial: &str) -> SessionStartErrors {
        let mut errors = SessionStartErrors::default();

        match LogcatStream::spawn(serial) {
            Ok((rx, stream)) => {
                self.logcat_rx = Some(rx);
                self.logcat_stream = Some(stream);
            }
            Err(err) => errors.logcat = Some(err.user_message()),
        }

        match RamPoller::spawn(serial) {
            Ok((rx, poller)) => {
                self.ram_rx = Some(rx);
                self.ram_poller = Some(poller);
            }
            Err(err) => errors.ram = Some(err.user_message()),
        }

        match StorageGaugePoller::spawn(serial) {
            Ok((rx, poller)) => {
                self.storage_gauge_rx = Some(rx);
                self.storage_gauge_poller = Some(poller);
            }
            Err(err) => errors.storage_gauge = Some(err.user_message()),
        }

        match StorageBreakdownPoller::spawn(serial) {
            Ok((rx, poller)) => {
                self.storage_breakdown_rx = Some(rx);
                self.storage_breakdown_poller = Some(poller);
            }
            Err(err) => errors.storage_breakdown = Some(err.user_message()),
        }

        match NetworkPoller::spawn(serial) {
            Ok((rx, poller)) => {
                self.network_rx = Some(rx);
                self.network_poller = Some(poller);
            }
            Err(err) => errors.network = Some(err.user_message()),
        }

        match ProtocolPoller::spawn(serial) {
            Ok((rx, poller)) => {
                self.protocol_rx = Some(rx);
                self.protocol_poller = Some(poller);
            }
            Err(err) => errors.protocol = Some(err.user_message()),
        }

        // Heavy per-package scan; start after fast pollers and an internal delay.
        match AppStoragePoller::spawn(serial) {
            Ok((rx, poller)) => {
                self.app_storage_rx = Some(rx);
                self.app_storage_poller = Some(poller);
            }
            Err(err) => errors.app_storage = Some(err.user_message()),
        }

        errors
    }

    /// Stops all workers and drops their receivers.
    pub fn stop(&mut self) {
        if let Some(stream) = self.logcat_stream.take() {
            stream.stop();
        }
        self.logcat_rx = None;

        if let Some(poller) = self.ram_poller.take() {
            poller.stop();
        }
        self.ram_rx = None;

        if let Some(poller) = self.storage_gauge_poller.take() {
            poller.stop();
        }
        self.storage_gauge_rx = None;

        if let Some(poller) = self.storage_breakdown_poller.take() {
            poller.stop();
        }
        self.storage_breakdown_rx = None;

        if let Some(poller) = self.network_poller.take() {
            poller.stop();
        }
        self.network_rx = None;

        if let Some(poller) = self.protocol_poller.take() {
            poller.stop();
        }
        self.protocol_rx = None;

        if let Some(poller) = self.app_storage_poller.take() {
            poller.stop();
        }
        self.app_storage_rx = None;
    }

    /// Drains worker channels into both logcat panes and `metrics`.
    pub fn drain_into(
        &mut self,
        logcat: &mut LogcatPane,
        logcat_errors: &mut LogcatPane,
        metrics: &mut MetricStore,
    ) -> DrainOutcome {
        let mut ui_changed = false;
        let error_accepted = self.drain_logcat(logcat, logcat_errors, &mut ui_changed);
        ui_changed |= self.drain_ram(metrics);
        ui_changed |= self.drain_storage_gauge(metrics);
        ui_changed |= self.drain_network(metrics);
        ui_changed |= self.drain_protocols(metrics);
        ui_changed |= self.drain_app_storage(metrics);
        ui_changed |= self.drain_storage_breakdown(metrics);
        DrainOutcome {
            ui_changed,
            error_accepted,
        }
    }

    pub(crate) fn has_app_storage(&self) -> bool {
        self.app_storage_rx.is_some()
    }

    fn drain_logcat(
        &mut self,
        logcat: &mut LogcatPane,
        logcat_errors: &mut LogcatPane,
        ui_changed: &mut bool,
    ) -> bool {
        let entries = take_log_entries(self.logcat_rx.as_ref());
        let flush_pending = logcat_errors.auto_update_feed && !logcat_errors.pending.is_empty();
        let accept_errors_only = logcat_errors.accept_errors_only;
        let accepted_entry = entries
            .iter()
            .any(|entry| !accept_errors_only || entry.is_error_level());
        let updated_all = ingest_log_entries(logcat, &entries);
        let updated_errors = ingest_log_entries(logcat_errors, &entries);
        *ui_changed |= updated_all || updated_errors;
        flush_pending || accepted_entry
    }

    fn drain_network(&mut self, metrics: &mut MetricStore) -> bool {
        let Some(rx) = self.network_rx.as_ref() else {
            return false;
        };

        let mut updated = false;
        while let Ok(update) = rx.try_recv() {
            match update {
                NetworkUpdate::Stats(stats) => {
                    metrics.network_stats = Some(stats);
                    metrics.network_error = None;
                    updated = true;
                }
                NetworkUpdate::Error(message) => {
                    metrics.network_error = Some(message);
                    updated = true;
                }
            }
        }
        updated
    }

    fn drain_protocols(&mut self, metrics: &mut MetricStore) -> bool {
        let Some(rx) = self.protocol_rx.as_ref() else {
            return false;
        };

        let mut updated = false;
        while let Ok(update) = rx.try_recv() {
            match update {
                ProtocolUpdate::Stats(stats) => {
                    metrics.protocol_stats = Some(stats);
                    metrics.protocol_error = None;
                    updated = true;
                }
                ProtocolUpdate::Error(message) => {
                    metrics.protocol_error = Some(message);
                    updated = true;
                }
            }
        }
        updated
    }

    fn drain_app_storage(&mut self, metrics: &mut MetricStore) -> bool {
        let Some(rx) = self.app_storage_rx.as_ref() else {
            return false;
        };

        let mut updated = false;
        while let Ok(update) = rx.try_recv() {
            match update {
                AppStorageUpdate::PackageList(packages) => {
                    if metrics.app_storage.packages.is_empty() {
                        metrics.app_storage.set_packages(packages);
                    } else {
                        metrics.app_storage.merge_packages(packages);
                    }
                    metrics.app_storage.error = None;
                    updated = true;
                }
                AppStorageUpdate::PackageStorage(storage) => {
                    metrics
                        .app_storage
                        .set_size(&storage.package, storage.total_bytes);
                    updated = true;
                }
                AppStorageUpdate::ScanComplete => {
                    metrics.app_storage.scanning = false;
                    updated = true;
                }
                AppStorageUpdate::Error(message) => {
                    metrics.app_storage.error = Some(message);
                    updated = true;
                }
            }
        }
        updated
    }

    fn drain_storage_breakdown(&mut self, metrics: &mut MetricStore) -> bool {
        let Some(rx) = self.storage_breakdown_rx.as_ref() else {
            return false;
        };

        let mut updated = false;
        while let Ok(update) = rx.try_recv() {
            match update {
                StorageBreakdownUpdate::Breakdown(breakdown) => {
                    metrics.storage_breakdown = Some(breakdown);
                    metrics.storage_breakdown_error = None;
                    updated = true;
                }
                StorageBreakdownUpdate::Error(message) => {
                    metrics.storage_breakdown_error = Some(message);
                    updated = true;
                }
            }
        }
        updated
    }

    fn drain_ram(&mut self, metrics: &mut MetricStore) -> bool {
        let Some(rx) = self.ram_rx.as_ref() else {
            return false;
        };

        let mut updated = false;
        while let Ok(update) = rx.try_recv() {
            match update {
                RamUpdate::Memory(memory) => {
                    metrics.ram_memory = Some(memory);
                    metrics.ram_error = None;
                    updated = true;
                }
                RamUpdate::Error(message) => {
                    metrics.ram_error = Some(message);
                    updated = true;
                }
            }
        }
        updated
    }

    fn drain_storage_gauge(&mut self, metrics: &mut MetricStore) -> bool {
        let Some(rx) = self.storage_gauge_rx.as_ref() else {
            return false;
        };

        let mut updated = false;
        while let Ok(update) = rx.try_recv() {
            match update {
                StorageGaugeUpdate::Overview(overview) => {
                    metrics.storage_gauge = Some(overview);
                    metrics.storage_gauge_error = None;
                    updated = true;
                }
                StorageGaugeUpdate::Error(message) => {
                    metrics.storage_gauge_error = Some(message);
                    updated = true;
                }
            }
        }
        updated
    }
}

/// Appends `entries` to `pane`. Flushes pending first when auto-update is on.
/// Returns true when the visible ring buffer changed.
fn ingest_log_entries(pane: &mut LogcatPane, entries: &[LogEntry]) -> bool {
    let mut updated = false;
    if pane.auto_update_feed {
        updated |= pane.flush_pending();
    }
    for entry in entries {
        updated |= pane.append_entry(entry);
    }
    updated
}

fn take_log_entries(rx: Option<&Receiver<LogEntry>>) -> Vec<LogEntry> {
    let Some(rx) = rx else {
        return Vec::new();
    };

    rx.try_iter().take(MAX_DRAIN_PER_FRAME).collect()
}
