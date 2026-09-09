use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};

use adb_client::{
    Adb, AppStoragePoller, AppStorageUpdate, DeviceInfo, DeviceState, LogEntry, LogcatStream,
    MemoryStats, NetworkPoller, NetworkStats, NetworkUpdate, ProtocolPoller, ProtocolStats,
    ProtocolUpdate, RamPoller, RamUpdate, StorageBreakdown, StorageBreakdownPoller,
    StorageBreakdownUpdate, StorageGaugePoller, StorageGaugeUpdate, StorageOverview,
};
use ai_insight::{build_snapshot, spawn_insight, InsightLine, InsightUpdate, LevelMask};
use crossbeam_channel::Receiver;
use eframe::egui;

use crate::ui_elements;

pub const MAX_LOG_LINES: usize = 10_000;
const MAX_DRAIN_PER_FRAME: usize = 500;
const MAX_INSIGHTS: usize = 100;
const INSIGHT_SETTLE: Duration = Duration::from_secs(5);
const INSIGHT_COOLDOWN: Duration = Duration::from_secs(30);
const REPAINT_INTERVAL: Duration = Duration::from_millis(200);

pub struct App {
    pub adb_error: Option<String>,
    pub devices: Vec<DeviceInfo>,
    pub list_error: Option<String>,
    pub devices_refreshed_at: Option<Instant>,
    pub selected_serial: Option<String>,
    pub logcat_rx: Option<Receiver<LogEntry>>,
    pub logcat_stream: Option<LogcatStream>,
    pub error_logcat_rx: Option<Receiver<LogEntry>>,
    pub error_logcat_stream: Option<LogcatStream>,
    pub network_rx: Option<Receiver<NetworkUpdate>>,
    pub network_poller: Option<NetworkPoller>,
    pub protocol_rx: Option<Receiver<ProtocolUpdate>>,
    pub protocol_poller: Option<ProtocolPoller>,
    pub network_stats: Option<NetworkStats>,
    pub network_error: Option<String>,
    pub protocol_stats: Option<ProtocolStats>,
    pub protocol_error: Option<String>,
    pub app_storage_rx: Option<Receiver<AppStorageUpdate>>,
    pub app_storage_poller: Option<AppStoragePoller>,
    pub app_storage: AppStorageState,
    pub storage_breakdown_rx: Option<Receiver<StorageBreakdownUpdate>>,
    pub storage_breakdown_poller: Option<StorageBreakdownPoller>,
    pub storage_breakdown: Option<StorageBreakdown>,
    pub storage_breakdown_error: Option<String>,
    pub ram_rx: Option<Receiver<RamUpdate>>,
    pub ram_poller: Option<RamPoller>,
    pub ram_memory: Option<MemoryStats>,
    pub ram_error: Option<String>,
    pub storage_gauge_rx: Option<Receiver<StorageGaugeUpdate>>,
    pub storage_gauge_poller: Option<StorageGaugePoller>,
    pub storage_gauge: Option<StorageOverview>,
    pub storage_gauge_error: Option<String>,
    pub log_lines: VecDeque<CachedLogLine>,
    pub pending_log_lines: VecDeque<CachedLogLine>,
    pub error_lines: VecDeque<CachedLogLine>,
    pub pending_error_lines: VecDeque<CachedLogLine>,
    pub logcat_error: Option<String>,
    pub error_logcat_error: Option<String>,
    pub auto_update_feed: bool,
    pub error_auto_update_feed: bool,
    pub insight_auto_update_feed: bool,
    pub logcat_show_timestamps: bool,
    pub error_show_timestamps: bool,
    pub logcat_filter: String,
    pub error_logcat_filter: String,
    pub logcat_tag_input: String,
    pub logcat_tag_filters: Vec<LogcatTagFilter>,
    pub error_logcat_tag_input: String,
    pub error_logcat_tag_filters: Vec<LogcatTagFilter>,
    pub logcat: LogcatPane,
    pub logcat_errors: LogcatPane,
    pub insight: InsightState,
    insight_rx: Option<Receiver<InsightUpdate>>,
    insight_serial: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InsightStatus {
    Idle,
    RequestSent,
    RequestFailed,
}

pub struct InsightState {
    pub status: InsightStatus,
    pub replies: VecDeque<String>,
    last_analyze: Option<Instant>,
    last_error_at: Option<Instant>,
    last_sent_key: Option<String>,
    ever_succeeded: bool,
    generation: u64,
}

impl Default for InsightState {
    fn default() -> Self {
        Self {
            status: InsightStatus::Idle,
            replies: VecDeque::new(),
            last_analyze: None,
            last_error_at: None,
            last_sent_key: None,
            ever_succeeded: false,
            generation: 0,
        }
    }
}

#[derive(Default)]
pub struct AppStorageState {
    pub packages: Vec<String>,
    pub sizes: HashMap<String, u64>,
    pub scanning: bool,
    pub error: Option<String>,
}

impl AppStorageState {
    pub fn set_packages(&mut self, packages: Vec<String>) {
        self.packages = packages;
        self.sizes.clear();
        self.scanning = true;
    }

    pub fn merge_packages(&mut self, packages: Vec<String>) {
        self.packages = packages;
        self.sizes
            .retain(|package, _| self.packages.iter().any(|pkg| pkg == package));
        self.scanning = true;
    }

    pub fn set_size(&mut self, package: &str, bytes: u64) {
        self.sizes.insert(package.to_string(), bytes);
    }

    pub fn sorted_rows(&self) -> Vec<(&str, Option<u64>)> {
        let mut rows: Vec<(&str, Option<u64>)> = self
            .packages
            .iter()
            .map(|pkg| (pkg.as_str(), self.sizes.get(pkg).copied()))
            .collect();
        rows.sort_by(|a, b| match (a.1, b.1) {
            (Some(left), Some(right)) => right.cmp(&left),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => a.0.cmp(b.0),
        });
        rows
    }
}

#[derive(Clone, Debug)]
pub struct LogcatTagFilter {
    pub tag: String,
    pub color_index: usize,
}

/// Visible ring buffer, pending ring buffer, and filter state for one logcat pane.
pub struct LogcatPane {
    pub lines: VecDeque<CachedLogLine>,
    pub pending: VecDeque<CachedLogLine>,
    pub filter: String,
    pub tag_input: String,
    pub tag_filters: Vec<LogcatTagFilter>,
    pub auto_update_feed: bool,
    pub show_timestamps: bool,
    pub error: Option<String>,
    pub accept_errors_only: bool,
}

impl Default for LogcatPane {
    fn default() -> Self {
        Self {
            lines: VecDeque::new(),
            pending: VecDeque::new(),
            filter: String::new(),
            tag_input: String::new(),
            tag_filters: Vec::new(),
            auto_update_feed: true,
            show_timestamps: true,
            error: None,
            accept_errors_only: false,
        }
    }
}

#[derive(Clone)]
pub struct CachedLogLine {
    full: String,
    compact: String,
    pub level: char,
    pub tag: String,
    pub message: String,
    pub received_at: Instant,
}

impl CachedLogLine {
    pub fn from_entry(entry: &LogEntry) -> Self {
        CachedLogLine {
            full: entry.format_line_with_timestamp(true),
            compact: entry.format_line_with_timestamp(false),
            level: entry.level,
            tag: entry.tag.clone(),
            message: entry.message.clone(),
            received_at: Instant::now(),
        }
    }

    pub fn display(&self, show_timestamp: bool) -> &str {
        if show_timestamp {
            &self.full
        } else {
            &self.compact
        }
    }

    pub fn matches_filter(&self, filter_lower: &str) -> bool {
        self.full.to_lowercase().contains(filter_lower)
    }

    pub fn matches_tag_filters(
        &self,
        tag_filters: &[LogcatTagFilter],
        show_timestamps: bool,
    ) -> bool {
        if tag_filters.is_empty() {
            return true;
        }

        let display = self.display(show_timestamps).to_lowercase();
        tag_filters
            .iter()
            .all(|filter| display.contains(&filter.tag.to_lowercase()))
    }
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
            adb_error,
            devices,
            list_error,
            devices_refreshed_at,
            selected_serial: None,
            logcat_rx: None,
            logcat_stream: None,
            error_logcat_rx: None,
            error_logcat_stream: None,
            network_rx: None,
            network_poller: None,
            protocol_rx: None,
            protocol_poller: None,
            network_stats: None,
            network_error: None,
            protocol_stats: None,
            protocol_error: None,
            app_storage_rx: None,
            app_storage_poller: None,
            app_storage: AppStorageState::default(),
            storage_breakdown_rx: None,
            storage_breakdown_poller: None,
            storage_breakdown: None,
            storage_breakdown_error: None,
            ram_rx: None,
            ram_poller: None,
            ram_memory: None,
            ram_error: None,
            storage_gauge_rx: None,
            storage_gauge_poller: None,
            storage_gauge: None,
            storage_gauge_error: None,
            log_lines: VecDeque::new(),
            pending_log_lines: VecDeque::new(),
            error_lines: VecDeque::new(),
            pending_error_lines: VecDeque::new(),
            logcat_error: None,
            error_logcat_error: None,
            auto_update_feed: true,
            error_auto_update_feed: true,
            insight_auto_update_feed: true,
            logcat_show_timestamps: true,
            error_show_timestamps: true,
            logcat_filter: String::new(),
            error_logcat_filter: String::new(),
            logcat_tag_input: String::new(),
            logcat_tag_filters: Vec::new(),
            error_logcat_tag_input: String::new(),
            error_logcat_tag_filters: Vec::new(),
            logcat: LogcatPane::default(),
            logcat_errors: LogcatPane {
                accept_errors_only: true,
                ..LogcatPane::default()
            },
            insight: InsightState::default(),
            insight_rx: None,
            insight_serial: None,
        };
        if let Some(serial) = first_ready_serial(&app.devices) {
            app.select_device(serial);
        }
        app
    }

    pub fn add_logcat_tag(&mut self) {
        add_tag_filter(&mut self.logcat_tag_input, &mut self.logcat_tag_filters);
    }

    pub fn remove_logcat_tag(&mut self, index: usize) {
        remove_tag_filter(&mut self.logcat_tag_filters, index);
    }

    pub fn add_error_logcat_tag(&mut self) {
        add_tag_filter(
            &mut self.error_logcat_tag_input,
            &mut self.error_logcat_tag_filters,
        );
    }

    pub fn remove_error_logcat_tag(&mut self, index: usize) {
        remove_tag_filter(&mut self.error_logcat_tag_filters, index);
    }

    pub fn refresh_devices(&mut self) {
        self.list_error = None;
        self.devices_refreshed_at = Some(Instant::now());
        match Adb::list_devices() {
            Ok(devices) => {
                self.devices = devices;
                if let Some(serial) = &self.selected_serial {
                    let still_connected = self.devices.iter().any(|device| {
                        device.serial == *serial && device.state == DeviceState::Device
                    });
                    if !still_connected {
                        self.deselect_device();
                    }
                }
                if self.selected_serial.is_none() {
                    if let Some(serial) = first_ready_serial(&self.devices) {
                        self.select_device(serial);
                    }
                }
            }
            Err(err) => self.list_error = Some(err.user_message()),
        }
    }

    pub fn select_device(&mut self, serial: String) {
        if self.selected_serial.as_deref() == Some(serial.as_str()) {
            return;
        }

        self.stop_streams();
        self.clear_device_data();
        self.selected_serial = Some(serial.clone());
        self.start_streams(&serial);
    }

    pub fn deselect_device(&mut self) {
        if self.selected_serial.is_none() {
            return;
        }

        self.stop_streams();
        self.clear_device_data();
        self.selected_serial = None;
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
            Err(err) => self.logcat_error = Some(err.user_message()),
        }

        match LogcatStream::spawn_errors(serial) {
            Ok((rx, stream)) => {
                self.error_logcat_rx = Some(rx);
                self.error_logcat_stream = Some(stream);
            }
            Err(err) => self.error_logcat_error = Some(err.user_message()),
        }

        match RamPoller::spawn(serial) {
            Ok((rx, poller)) => {
                self.ram_rx = Some(rx);
                self.ram_poller = Some(poller);
            }
            Err(err) => self.ram_error = Some(err.user_message()),
        }

        match StorageGaugePoller::spawn(serial) {
            Ok((rx, poller)) => {
                self.storage_gauge_rx = Some(rx);
                self.storage_gauge_poller = Some(poller);
            }
            Err(err) => self.storage_gauge_error = Some(err.user_message()),
        }

        match StorageBreakdownPoller::spawn(serial) {
            Ok((rx, poller)) => {
                self.storage_breakdown_rx = Some(rx);
                self.storage_breakdown_poller = Some(poller);
            }
            Err(err) => self.storage_breakdown_error = Some(err.user_message()),
        }

        match NetworkPoller::spawn(serial) {
            Ok((rx, poller)) => {
                self.network_rx = Some(rx);
                self.network_poller = Some(poller);
            }
            Err(err) => self.network_error = Some(err.user_message()),
        }

        match ProtocolPoller::spawn(serial) {
            Ok((rx, poller)) => {
                self.protocol_rx = Some(rx);
                self.protocol_poller = Some(poller);
            }
            Err(err) => self.protocol_error = Some(err.user_message()),
        }

        // Heavy per-package scan; start after fast pollers and an internal delay.
        match AppStoragePoller::spawn(serial) {
            Ok((rx, poller)) => {
                self.app_storage_rx = Some(rx);
                self.app_storage_poller = Some(poller);
                self.app_storage.scanning = true;
            }
            Err(err) => self.app_storage.error = Some(err.user_message()),
        }
    }

    fn clear_device_data(&mut self) {
        self.log_lines.clear();
        self.error_lines.clear();
        self.logcat_error = None;
        self.error_logcat_error = None;
        self.logcat_filter.clear();
        self.error_logcat_filter.clear();
        self.logcat_tag_input.clear();
        self.logcat_tag_filters.clear();
        self.error_logcat_tag_input.clear();
        self.error_logcat_tag_filters.clear();
        self.network_stats = None;
        self.network_error = None;
        self.protocol_stats = None;
        self.protocol_error = None;
        self.app_storage = AppStorageState::default();
        self.storage_breakdown = None;
        self.storage_breakdown_error = None;
        self.ram_memory = None;
        self.ram_error = None;
        self.storage_gauge = None;
        self.storage_gauge_error = None;
        self.insight = InsightState::default();
        self.insight_rx = None;
        self.insight_serial = None;
    }

    fn request_insight(&mut self) {
        let Some(serial) = self.selected_serial.clone() else {
            return;
        };
        if self.insight.status == InsightStatus::RequestSent {
            return;
        }

        let model = self
            .devices
            .iter()
            .find(|device| device.serial == serial)
            .map(|device| device.model.as_str())
            .unwrap_or("unknown");
        let now = Instant::now();
        let lines = self.error_lines.iter().map(|line| InsightLine {
            received_at: line.received_at,
            level: line.level,
            tag: line.tag.clone(),
            message: line.message.clone(),
        });
        let snapshot = build_snapshot(lines, LevelMask::Error, model, &serial, now);
        if snapshot.clusters.is_empty() {
            return;
        }
        let key = snapshot.digest_key();
        self.insight.generation = self.insight.generation.wrapping_add(1);
        self.insight.status = InsightStatus::RequestSent;
        self.insight.last_analyze = Some(now);
        self.insight.last_sent_key = Some(key);
        self.insight_serial = Some(serial.clone());
        self.insight_rx = Some(spawn_insight(snapshot, self.insight.generation, serial));
        tracing::info!(
            generation = self.insight.generation,
            "insight request queued"
        );
    }

    /// Queues an insight POST when recent errors settle or the digest changes.
    fn maybe_request_insight(&mut self) {
        if self.selected_serial.is_none() {
            return;
        }
        if self.insight.status == InsightStatus::RequestSent {
            return;
        }

        let Some(serial) = self.selected_serial.clone() else {
            return;
        };
        let model = self
            .devices
            .iter()
            .find(|device| device.serial == serial)
            .map(|device| device.model.as_str())
            .unwrap_or("unknown");
        let now = Instant::now();
        let lines = self.error_lines.iter().map(|line| InsightLine {
            received_at: line.received_at,
            level: line.level,
            tag: line.tag.clone(),
            message: line.message.clone(),
        });
        let snapshot = build_snapshot(lines, LevelMask::Error, model, &serial, now);
        if snapshot.clusters.is_empty() {
            return;
        }
        let key = snapshot.digest_key();

        let should_send = match self.insight.last_sent_key.as_deref() {
            None => self
                .insight
                .last_error_at
                .is_some_and(|at| at.elapsed() >= INSIGHT_SETTLE),
            Some(prev) => {
                let cooled = self
                    .insight
                    .last_analyze
                    .is_none_or(|at| at.elapsed() >= INSIGHT_COOLDOWN);
                let key_changed = key != prev;
                let high = snapshot.has_new_high_severity(prev);
                if key_changed {
                    high || cooled
                } else {
                    self.insight.status == InsightStatus::RequestFailed && cooled
                }
            }
        };

        if should_send {
            self.request_insight();
        }
    }

    fn drain_insight(&mut self) -> bool {
        let Some(rx) = self.insight_rx.as_ref() else {
            return false;
        };

        let mut updated = false;
        while let Ok(update) = rx.try_recv() {
            let (generation, serial) = match &update {
                InsightUpdate::Started { generation, serial }
                | InsightUpdate::Reply {
                    generation, serial, ..
                }
                | InsightUpdate::Error {
                    generation, serial, ..
                } => (*generation, serial.as_str()),
            };
            if generation != self.insight.generation {
                continue;
            }
            if self.insight_serial.as_deref() != Some(serial) {
                continue;
            }
            if self.selected_serial.as_deref() != Some(serial) {
                continue;
            }
            match update {
                InsightUpdate::Started { .. } => {
                    self.insight.status = InsightStatus::RequestSent;
                    updated = true;
                }
                InsightUpdate::Reply { text, .. } => {
                    self.insight.replies.push_back(text);
                    while self.insight.replies.len() > MAX_INSIGHTS {
                        self.insight.replies.pop_front();
                    }
                    self.insight.status = InsightStatus::Idle;
                    self.insight.ever_succeeded = true;
                    tracing::info!(stored = self.insight.replies.len(), "insight reply stored");
                    updated = true;
                }
                InsightUpdate::Error { .. } => {
                    self.insight.status = InsightStatus::RequestFailed;
                    if !self.insight.ever_succeeded {
                        self.insight.last_sent_key = None;
                        self.insight.last_error_at = Some(Instant::now());
                    }
                    updated = true;
                }
            }
        }
        updated
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
        if let Some(stream) = self.error_logcat_stream.take() {
            stream.stop();
        }
        self.error_logcat_rx = None;
    }

    fn drain_logcat(&mut self) -> bool {
        let entries = take_log_entries(self.logcat_rx.as_ref());

        if self.auto_update_feed {
            let mut updated = false;
            if !self.pending_log_lines.is_empty() {
                self.log_lines.extend(self.pending_log_lines.drain(..));
                trim_buffer(&mut self.log_lines);
                updated = true;
            }
            if !entries.is_empty() {
                for entry in entries {
                    self.log_lines.push_back(CachedLogLine::from_entry(&entry));
                }
                trim_buffer(&mut self.log_lines);
                updated = true;
            }
            updated
        } else {
            if !entries.is_empty() {
                for entry in entries {
                    self.pending_log_lines
                        .push_back(CachedLogLine::from_entry(&entry));
                }
                trim_buffer(&mut self.pending_log_lines);
            }
            // Even though we received logs, we didn't update the visible lines.
            false
        }
    }

    fn drain_error_logcat(&mut self) -> bool {
        let entries = take_log_entries(self.error_logcat_rx.as_ref());

        if self.error_auto_update_feed {
            let mut updated = false;
            if !self.pending_error_lines.is_empty() {
                self.error_lines.extend(self.pending_error_lines.drain(..));
                trim_buffer(&mut self.error_lines);
                self.insight.last_error_at = Some(Instant::now());
                updated = true;
            }
            if !entries.is_empty() {
                for entry in entries {
                    if entry.is_error_level() {
                        self.error_lines
                            .push_back(CachedLogLine::from_entry(&entry));
                        self.insight.last_error_at = Some(Instant::now());
                        updated = true;
                    }
                }
                if updated {
                    trim_buffer(&mut self.error_lines);
                }
            }
            updated
        } else {
            if !entries.is_empty() {
                for entry in entries {
                    if entry.is_error_level() {
                        self.pending_error_lines
                            .push_back(CachedLogLine::from_entry(&entry));
                        self.insight.last_error_at = Some(Instant::now());
                    }
                }
                trim_buffer(&mut self.pending_error_lines);
            }
            false
        }
    }

    fn drain_network(&mut self) -> bool {
        let Some(rx) = self.network_rx.as_ref() else {
            return false;
        };

        let mut updated = false;
        while let Ok(update) = rx.try_recv() {
            match update {
                NetworkUpdate::Stats(stats) => {
                    self.network_stats = Some(stats);
                    self.network_error = None;
                    updated = true;
                }
                NetworkUpdate::Error(message) => {
                    self.network_error = Some(message);
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
                    self.protocol_stats = Some(stats);
                    self.protocol_error = None;
                    updated = true;
                }
                ProtocolUpdate::Error(message) => {
                    self.protocol_error = Some(message);
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
                    if self.app_storage.packages.is_empty() {
                        self.app_storage.set_packages(packages);
                    } else {
                        self.app_storage.merge_packages(packages);
                    }
                    self.app_storage.error = None;
                    updated = true;
                }
                AppStorageUpdate::PackageStorage(storage) => {
                    self.app_storage
                        .set_size(&storage.package, storage.total_bytes);
                    updated = true;
                }
                AppStorageUpdate::ScanComplete => {
                    self.app_storage.scanning = false;
                    updated = true;
                }
                AppStorageUpdate::Error(message) => {
                    self.app_storage.error = Some(message);
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
                    self.storage_breakdown = Some(breakdown);
                    self.storage_breakdown_error = None;
                    updated = true;
                }
                StorageBreakdownUpdate::Error(message) => {
                    self.storage_breakdown_error = Some(message);
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
                    self.ram_memory = Some(memory);
                    self.ram_error = None;
                    updated = true;
                }
                RamUpdate::Error(message) => {
                    self.ram_error = Some(message);
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
                    self.storage_gauge = Some(overview);
                    self.storage_gauge_error = None;
                    updated = true;
                }
                StorageGaugeUpdate::Error(message) => {
                    self.storage_gauge_error = Some(message);
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
        if self.drain_error_logcat() {
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
        if self.selected_serial.is_some() {
            ctx.request_repaint_after(REPAINT_INTERVAL);
        }
    }
}

/// Adds a trimmed tag filter if it is non-empty and not already present (case-insensitive).
fn add_tag_filter(input: &mut String, filters: &mut Vec<LogcatTagFilter>) {
    let tag = input.trim().to_string();
    if tag.is_empty() {
        return;
    }

    if filters
        .iter()
        .any(|filter| filter.tag.eq_ignore_ascii_case(&tag))
    {
        input.clear();
        return;
    }

    filters.push(LogcatTagFilter {
        tag: tag.clone(),
        color_index: ui_elements::tag_color_index(&tag),
    });
    input.clear();
}

/// Removes the tag filter at `index` if it exists.
fn remove_tag_filter(filters: &mut Vec<LogcatTagFilter>, index: usize) {
    if index < filters.len() {
        filters.remove(index);
    }
}

fn take_log_entries(rx: Option<&Receiver<LogEntry>>) -> Vec<LogEntry> {
    let Some(rx) = rx else {
        return Vec::new();
    };

    rx.try_iter().take(MAX_DRAIN_PER_FRAME).collect()
}

fn trim_buffer(buffer: &mut VecDeque<CachedLogLine>) {
    while buffer.len() > MAX_LOG_LINES {
        buffer.pop_front();
    }
}

fn first_ready_serial(devices: &[DeviceInfo]) -> Option<String> {
    devices
        .iter()
        .find(|device| device.state == DeviceState::Device)
        .map(|device| device.serial.clone())
}
