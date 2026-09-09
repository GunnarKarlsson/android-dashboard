use std::time::{Duration, Instant};

use adb_client::DeviceInfo;
use eframe::egui;

use crate::metrics::MetricStore;
use crate::roster::{first_ready_serial, DeviceRoster, RosterEvent};
use crate::session::{DeviceSession, SessionStartErrors};

pub use crate::insight::{InsightController, InsightStatus};
pub use crate::logcat_pane::{CachedLogLine, LogcatPane, LogcatTagFilter};
pub use crate::metrics::PackageStorageState;

const REPAINT_INTERVAL: Duration = Duration::from_millis(200);

pub struct App {
    pub roster: DeviceRoster,
    session: DeviceSession,
    pub logcat: LogcatPane,
    pub logcat_errors: LogcatPane,
    pub metrics: MetricStore,
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
            session: DeviceSession::default(),
            logcat: LogcatPane::default(),
            logcat_errors: LogcatPane {
                accept_errors_only: true,
                ..LogcatPane::default()
            },
            metrics: MetricStore::default(),
            insight: InsightController::default(),
        };
        if let Some(serial) = first_ready_serial(&app.roster.devices) {
            app.select_device(serial);
        }
        app
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

        self.session.stop();
        self.clear_view_state();
        self.roster.selected_serial = Some(serial.clone());
        let errors = self.session.start(&serial);
        self.apply_start_errors(errors);
    }

    pub fn deselect_device(&mut self) {
        if self.roster.selected_serial.is_none() {
            return;
        }

        self.session.stop();
        self.clear_view_state();
        self.roster.selected_serial = None;
    }

    pub fn shutdown(&mut self) {
        self.session.stop();
    }

    /// Copies spawn failures onto the logcat panes and metric snapshots.
    fn apply_start_errors(&mut self, errors: SessionStartErrors) {
        if let Some(message) = errors.logcat {
            self.logcat.error = Some(message.clone());
            self.logcat_errors.error = Some(message);
        }
        self.metrics.ram_error = errors.ram;
        self.metrics.storage_gauge_error = errors.storage_gauge;
        self.metrics.storage_breakdown_error = errors.storage_breakdown;
        self.metrics.network_error = errors.network;
        self.metrics.protocol_error = errors.protocol;
        if let Some(message) = errors.app_storage {
            self.metrics.app_storage.error = Some(message);
        } else if self.session.has_app_storage() {
            self.metrics.app_storage.scanning = true;
        }
    }

    /// Resets logcat panes, metric snapshots, and insight request state.
    fn clear_view_state(&mut self) {
        self.logcat.reset_view();
        self.logcat_errors.reset_view();
        self.metrics = MetricStore::default();
        self.insight.reset();
    }

    pub fn tick(&mut self, ctx: &egui::Context) {
        let outcome = self.session.drain_into(
            &mut self.logcat,
            &mut self.logcat_errors,
            &mut self.metrics,
        );
        if outcome.error_accepted {
            self.insight.note_error();
        }
        let mut needs_repaint = outcome.ui_changed;
        if self.insight.drain(self.roster.selected_serial.as_deref()) {
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
        self.insight
            .maybe_request(&serial, &model, self.logcat_errors.insight_lines());
    }
}
