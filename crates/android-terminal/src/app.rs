use std::time::{Duration, Instant};

use adb_client::{
    AppStoragePoller, AppStorageUpdate, DeviceInfo, LogEntry, LogcatStream, NetworkPoller,
    NetworkUpdate, ProtocolPoller, ProtocolUpdate, RamPoller, RamUpdate, StorageBreakdownPoller,
    StorageBreakdownUpdate, StorageGaugePoller, StorageGaugeUpdate,
};
use crossbeam_channel::Receiver;
use eframe::egui;

use crate::logcat_pane::{add_tag_filter, remove_tag_filter};
use crate::metrics::MetricStore;
use crate::roster::{first_ready_serial, DeviceRoster, RosterEvent};

pub use crate::insight::{InsightController, InsightStatus};
pub use crate::logcat_pane::{CachedLogLine, LogcatPane, LogcatTagFilter};
pub use crate::metrics::AppStorageState;

const MAX_DRAIN_PER_FRAME: usize = 500;
const REPAINT_INTERVAL: Duration = Duration::from_millis(200);

pub struct App {
    pub roster: DeviceRoster,
    pub logcat_rx: Option<Receiver<LogEntry>>,
    pub logcat_stream: Option<LogcatStream>,
    pub network_rx: Option<Receiver<NetworkUpdate>>,
    pub network_poller: Option<NetworkPoller>,
    pub protocol_rx: Option<Receiver<ProtocolUpdate>>,
    pub protocol_poller: Option<ProtocolPoller>,
    pub app_storage_rx: Option<Receiver<AppStorageUpdate>>,
    pub app_storage_poller: Option<AppStoragePoller>,
    pub storage_breakdown_rx: Option<Receiver<StorageBreakdownUpdate>>,
    pub storage_breakdown_poller: Option<StorageBreakdownPoller>,
    pub ram_rx: Option<Receiver<RamUpdate>>,
    pub ram_poller: Option<RamPoller>,
    pub storage_gauge_rx: Option<Receiver<StorageGaugeUpdate>>,
    pub storage_gauge_poller: Option<StorageGaugePoller>,
    pub metrics: MetricStore,
    pub logcat: LogcatPane,
    pub logcat_errors: LogcatPane,
    pub insight: InsightController,
}

impl App {
    pub fn new(
        adb_error: Option<String>,
        devices: Vec<DeviceInfo>,
        list_error: Option<String>,
    ) -> Self {
        let devices_refreshed_at = if adb_error.is_none() {
            Some(Instant::now())
        } else {
            None
        };
        let mut app = App {
            roster: DeviceRoster {
                adb_error,
                devices,
                list_error,
                devices_refreshed_at,
                selected_serial: None,
            },
            logcat_rx: None,
            logcat_stream: None,
            network_rx: None,
            network_poller: None,
            protocol_rx: None,
            protocol_poller: None,
            app_storage_rx: None,
            app_storage_poller: None,
            storage_breakdown_rx: None,
            storage_breakdown_poller: None,
            ram_rx: None,
            ram_poller: None,
            storage_gauge_rx: None,
            storage_gauge_poller: None,
            metrics: MetricStore::default(),
            logcat: LogcatPane::default(),
            logcat_errors: LogcatPane {
                accept_errors_only: true,
                ..LogcatPane::default()
            },
            insight: InsightController::default(),
        };
        if let Some(serial) = first_ready_serial(&app.roster.devices) {
            app.select_device(serial);
        }
        app
    }

    pub fn add_logcat_tag(&mut self) {
        add_tag_filter(&mut self.logcat.tag_input, &mut self.logcat.tag_filters);
    }

    pub fn remove_logcat_tag(&mut self, index: usize) {
        remove_tag_filter(&mut self.logcat.tag_filters, index);
    }

    pub fn add_error_logcat_tag(&mut self) {
        add_tag_filter(
            &mut self.logcat_errors.tag_input,
            &mut self.logcat_errors.tag_filters,
        );
    }

    pub fn remove_error_logcat_tag(&mut self, index: usize) {
        remove_tag_filter(&mut self.logcat_errors.tag_filters, index);
    }

    pub fn refresh_devices(&mut self) {
        match self.roster.refresh() {
            RosterEvent::Unchanged => {}
            RosterEvent::LostSelection => self.deselect_device(),
            RosterEvent::AutoSelect(serial) => self.select_device(serial),
        }
    }

    pub fn select_device(&mut self, serial: String) {
        if self.roster.selected_serial.as_deref() == Some(serial.as_str()) {
            return;
        }

        self.stop_streams();
        self.clear_device_data();
        self.roster.selected_serial = Some(serial.clone());
        self.start_streams(&serial);
    }

    pub fn deselect_device(&mut self) {
        if self.roster.selected_serial.is_none() {
            return;
        }

        self.stop_streams();
        self.clear_device_data();
        self.roster.selected_serial = None;
    }

    pub fn shutdown(&mut self) {
        self.stop_streams();
    }

    fn start_streams(&mut self, serial: &str) {
        match LogcatStream::spawn(serial) {
            Ok((rx, stream)) => {
                self.logcat_rx = Some(rx);
                self.logcat_stream = Some(stream);
            }
            Err(err) => {
                let message = err.user_message();
                self.logcat.error = Some(message.clone());
                self.logcat_errors.error = Some(message);
            }
        }

        match RamPoller::spawn(serial) {
            Ok((rx, poller)) => {
                self.ram_rx = Some(rx);
                self.ram_poller = Some(poller);
            }
            Err(err) => self.metrics.ram_error = Some(err.user_message()),
        }

        match StorageGaugePoller::spawn(serial) {
            Ok((rx, poller)) => {
                self.storage_gauge_rx = Some(rx);
                self.storage_gauge_poller = Some(poller);
            }
            Err(err) => self.metrics.storage_gauge_error = Some(err.user_message()),
        }

        match StorageBreakdownPoller::spawn(serial) {
            Ok((rx, poller)) => {
                self.storage_breakdown_rx = Some(rx);
                self.storage_breakdown_poller = Some(poller);
            }
            Err(err) => self.metrics.storage_breakdown_error = Some(err.user_message()),
        }

        match NetworkPoller::spawn(serial) {
            Ok((rx, poller)) => {
                self.network_rx = Some(rx);
                self.network_poller = Some(poller);
            }
            Err(err) => self.metrics.network_error = Some(err.user_message()),
        }

        match ProtocolPoller::spawn(serial) {
            Ok((rx, poller)) => {
                self.protocol_rx = Some(rx);
                self.protocol_poller = Some(poller);
            }
            Err(err) => self.metrics.protocol_error = Some(err.user_message()),
        }

        // Heavy per-package scan; start after fast pollers and an internal delay.
        match AppStoragePoller::spawn(serial) {
            Ok((rx, poller)) => {
                self.app_storage_rx = Some(rx);
                self.app_storage_poller = Some(poller);
                self.metrics.app_storage.scanning = true;
            }
            Err(err) => self.metrics.app_storage.error = Some(err.user_message()),
        }
    }

    fn clear_device_data(&mut self) {
        self.logcat.reset_view();
        self.logcat_errors.reset_view();
        self.metrics = MetricStore::default();
        self.insight.reset();
    }

    /// Queues an insight POST when recent errors settle or the digest changes.
    fn maybe_request_insight(&mut self) {
        let Some(serial) = self.roster.selected_serial.clone() else {
            return;
        };
        let model = self
            .roster
            .devices
            .iter()
            .find(|device| device.serial == serial)
            .map(|device| device.model.clone())
            .unwrap_or_else(|| "unknown".to_string());
        if self
            .insight
            .maybe_request(&serial, &model, self.logcat_errors.insight_lines())
        {
            self.insight
                .request(&serial, &model, self.logcat_errors.insight_lines());
        }
    }

    fn drain_insight(&mut self) -> bool {
        self.insight.drain(self.roster.selected_serial.as_deref())
    }

    fn stop_streams(&mut self) {
        self.stop_logcat();
        self.stop_ram();
        self.stop_storage_gauge();
        self.stop_network();
        self.stop_protocols();
        self.stop_app_storage();
        self.stop_storage_breakdown();
    }

    fn stop_network(&mut self) {
        if let Some(poller) = self.network_poller.take() {
            poller.stop();
        }
        self.network_rx = None;
    }

    fn stop_protocols(&mut self) {
        if let Some(poller) = self.protocol_poller.take() {
            poller.stop();
        }
        self.protocol_rx = None;
    }

    fn stop_app_storage(&mut self) {
        if let Some(poller) = self.app_storage_poller.take() {
            poller.stop();
        }
        self.app_storage_rx = None;
    }

    fn stop_storage_breakdown(&mut self) {
        if let Some(poller) = self.storage_breakdown_poller.take() {
            poller.stop();
        }
        self.storage_breakdown_rx = None;
    }

    fn stop_ram(&mut self) {
        if let Some(poller) = self.ram_poller.take() {
            poller.stop();
        }
        self.ram_rx = None;
    }

    fn stop_storage_gauge(&mut self) {
        if let Some(poller) = self.storage_gauge_poller.take() {
            poller.stop();
        }
        self.storage_gauge_rx = None;
    }

    fn stop_logcat(&mut self) {
        if let Some(stream) = self.logcat_stream.take() {
            stream.stop();
        }
        self.logcat_rx = None;
    }

    fn drain_logcat(&mut self) -> bool {
        let entries = take_log_entries(self.logcat_rx.as_ref());
        let flush_pending =
            self.logcat_errors.auto_update_feed && !self.logcat_errors.pending.is_empty();
        let accept_errors_only = self.logcat_errors.accept_errors_only;
        let accepted_entry = entries
            .iter()
            .any(|entry| !accept_errors_only || entry.is_error_level());
        let updated_all = ingest_log_entries(&mut self.logcat, &entries);
        let updated_errors = ingest_log_entries(&mut self.logcat_errors, &entries);
        if flush_pending || accepted_entry {
            self.insight.note_error();
        }
        updated_all || updated_errors
    }

    fn drain_network(&mut self) -> bool {
        let Some(rx) = self.network_rx.as_ref() else {
            return false;
        };

        let mut updated = false;
        while let Ok(update) = rx.try_recv() {
            match update {
                NetworkUpdate::Stats(stats) => {
                    self.metrics.network_stats = Some(stats);
                    self.metrics.network_error = None;
                    updated = true;
                }
                NetworkUpdate::Error(message) => {
                    self.metrics.network_error = Some(message);
                    updated = true;
                }
            }
        }
        updated
    }

    fn drain_protocols(&mut self) -> bool {
        let Some(rx) = self.protocol_rx.as_ref() else {
            return false;
        };

        let mut updated = false;
        while let Ok(update) = rx.try_recv() {
            match update {
                ProtocolUpdate::Stats(stats) => {
                    self.metrics.protocol_stats = Some(stats);
                    self.metrics.protocol_error = None;
                    updated = true;
                }
                ProtocolUpdate::Error(message) => {
                    self.metrics.protocol_error = Some(message);
                    updated = true;
                }
            }
        }
        updated
    }

    fn drain_app_storage(&mut self) -> bool {
        let Some(rx) = self.app_storage_rx.as_ref() else {
            return false;
        };

        let mut updated = false;
        while let Ok(update) = rx.try_recv() {
            match update {
                AppStorageUpdate::PackageList(packages) => {
                    if self.metrics.app_storage.packages.is_empty() {
                        self.metrics.app_storage.set_packages(packages);
                    } else {
                        self.metrics.app_storage.merge_packages(packages);
                    }
                    self.metrics.app_storage.error = None;
                    updated = true;
                }
                AppStorageUpdate::PackageStorage(storage) => {
                    self.metrics
                        .app_storage
                        .set_size(&storage.package, storage.total_bytes);
                    updated = true;
                }
                AppStorageUpdate::ScanComplete => {
                    self.metrics.app_storage.scanning = false;
                    updated = true;
                }
                AppStorageUpdate::Error(message) => {
                    self.metrics.app_storage.error = Some(message);
                    updated = true;
                }
            }
        }
        updated
    }

    fn drain_storage_breakdown(&mut self) -> bool {
        let Some(rx) = self.storage_breakdown_rx.as_ref() else {
            return false;
        };

        let mut updated = false;
        while let Ok(update) = rx.try_recv() {
            match update {
                StorageBreakdownUpdate::Breakdown(breakdown) => {
                    self.metrics.storage_breakdown = Some(breakdown);
                    self.metrics.storage_breakdown_error = None;
                    updated = true;
                }
                StorageBreakdownUpdate::Error(message) => {
                    self.metrics.storage_breakdown_error = Some(message);
                    updated = true;
                }
            }
        }
        updated
    }

    fn drain_ram(&mut self) -> bool {
        let Some(rx) = self.ram_rx.as_ref() else {
            return false;
        };

        let mut updated = false;
        while let Ok(update) = rx.try_recv() {
            match update {
                RamUpdate::Memory(memory) => {
                    self.metrics.ram_memory = Some(memory);
                    self.metrics.ram_error = None;
                    updated = true;
                }
                RamUpdate::Error(message) => {
                    self.metrics.ram_error = Some(message);
                    updated = true;
                }
            }
        }
        updated
    }

    fn drain_storage_gauge(&mut self) -> bool {
        let Some(rx) = self.storage_gauge_rx.as_ref() else {
            return false;
        };

        let mut updated = false;
        while let Ok(update) = rx.try_recv() {
            match update {
                StorageGaugeUpdate::Overview(overview) => {
                    self.metrics.storage_gauge = Some(overview);
                    self.metrics.storage_gauge_error = None;
                    updated = true;
                }
                StorageGaugeUpdate::Error(message) => {
                    self.metrics.storage_gauge_error = Some(message);
                    updated = true;
                }
            }
        }
        updated
    }

    pub fn tick(&mut self, ctx: &egui::Context) {
        let mut needs_repaint = false;
        if self.drain_logcat() {
            needs_repaint = true;
        }
        if self.drain_ram() {
            needs_repaint = true;
        }
        if self.drain_storage_gauge() {
            needs_repaint = true;
        }
        if self.drain_network() {
            needs_repaint = true;
        }
        if self.drain_protocols() {
            needs_repaint = true;
        }
        if self.drain_app_storage() {
            needs_repaint = true;
        }
        if self.drain_storage_breakdown() {
            needs_repaint = true;
        }
        if self.drain_insight() {
            needs_repaint = true;
        }
        self.maybe_request_insight();
        if needs_repaint {
            ctx.request_repaint();
        }
        if self.roster.selected_serial.is_some() {
            ctx.request_repaint_after(REPAINT_INTERVAL);
        }
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
