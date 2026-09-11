use std::collections::VecDeque;
use std::time::{Duration, Instant};

use ai_insight::{
    InsightConfig, InsightLine, InsightSnapshot, InsightUpdate, LevelMask, build_snapshot,
    spawn_insight,
};
use crossbeam_channel::Receiver;

const MAX_INSIGHTS: usize = 100;
const INSIGHT_SETTLE: Duration = Duration::from_secs(5);
const INSIGHT_COOLDOWN: Duration = Duration::from_secs(30);

pub struct InsightController {
    pub auto_update_feed: bool,
    pub state: InsightState,
    config: InsightConfig,
    rx: Option<Receiver<InsightUpdate>>,
    serial: Option<String>,
}

impl Default for InsightController {
    fn default() -> Self {
        Self::new(InsightConfig::default())
    }
}

impl InsightController {
    pub(crate) fn new(config: InsightConfig) -> Self {
        Self {
            auto_update_feed: true,
            state: InsightState::default(),
            config,
            rx: None,
            serial: None,
        }
    }

    /// Returns the live provider config.
    pub(crate) fn config(&self) -> &InsightConfig {
        &self.config
    }

    /// Replaces the live provider config.
    pub(crate) fn set_config(&mut self, config: InsightConfig) {
        self.config = config;
    }

    /// Clears request state and the in-flight channel. Leaves `auto_update_feed` and `config` unchanged.
    pub(crate) fn reset(&mut self) {
        self.state = InsightState::default();
        self.rx = None;
        self.serial = None;
    }

    /// Records that the errors pane accepted a line or flushed pending.
    pub(crate) fn note_error(&mut self) {
        self.state.last_error_at = Some(Instant::now());
    }

    /// Builds a snapshot when settle/cooldown/digest can send, then queues the worker.
    /// Returns true if a request started.
    pub(crate) fn maybe_request(
        &mut self,
        serial: &str,
        model: &str,
        lines: impl IntoIterator<Item = InsightLine>,
    ) -> bool {
        if self.state.status == InsightStatus::RequestSent {
            return false;
        }
        if !self.config.is_configured() {
            return false;
        }

        let now = Instant::now();
        if !self.should_consider(now) {
            return false;
        }

        let snapshot = build_snapshot(lines, LevelMask::Error, model, serial, now);
        self.state.last_built_error_at = self.state.last_error_at;
        if !self.should_send(&snapshot, now) {
            return false;
        }

        self.queue(serial, snapshot, now);
        true
    }

    /// Returns whether clustering is worth doing at `now`.
    fn should_consider(&self, now: Instant) -> bool {
        match self.state.last_sent_key.as_deref() {
            None => self.settled(now),
            Some(_) => {
                if !self.cooled(now) {
                    self.ring_changed()
                } else {
                    self.ring_changed() || self.state.status == InsightStatus::RequestFailed
                }
            }
        }
    }

    fn settled(&self, now: Instant) -> bool {
        self.state
            .last_error_at
            .is_some_and(|at| now.saturating_duration_since(at) >= INSIGHT_SETTLE)
    }

    fn cooled(&self, now: Instant) -> bool {
        self.state
            .last_analyze
            .is_none_or(|at| now.saturating_duration_since(at) >= INSIGHT_COOLDOWN)
    }

    fn ring_changed(&self) -> bool {
        self.state.last_error_at != self.state.last_built_error_at
    }

    /// Returns whether settle, cooldown, digest change, and request status allow sending
    /// `snapshot` at `now`.
    fn should_send(&self, snapshot: &InsightSnapshot, now: Instant) -> bool {
        if self.state.status == InsightStatus::RequestSent {
            return false;
        }
        if snapshot.clusters.is_empty() {
            return false;
        }
        let key = snapshot.digest_key();

        match self.state.last_sent_key.as_deref() {
            None => self.settled(now),
            Some(prev) => {
                let cooled = self.cooled(now);
                let key_changed = key != prev;
                let high = snapshot.has_new_high_severity(prev);
                if key_changed {
                    high || cooled
                } else {
                    self.state.status == InsightStatus::RequestFailed && cooled
                }
            }
        }
    }

    /// Applies queued insight channel updates that match the current generation and serial.
    pub(crate) fn drain(&mut self, selected_serial: Option<&str>) -> bool {
        let Some(rx) = self.rx.as_ref() else {
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
            if generation != self.state.generation {
                continue;
            }
            if self.serial.as_deref() != Some(serial) {
                continue;
            }
            if selected_serial != Some(serial) {
                continue;
            }
            match update {
                InsightUpdate::Started { .. } => {
                    self.state.status = InsightStatus::RequestSent;
                    updated = true;
                }
                InsightUpdate::Reply { text, .. } => {
                    self.state.replies.push_back(text);
                    while self.state.replies.len() > MAX_INSIGHTS {
                        self.state.replies.pop_front();
                    }
                    self.state.status = InsightStatus::Idle;
                    self.state.ever_succeeded = true;
                    updated = true;
                }
                InsightUpdate::Error { .. } => {
                    self.state.status = InsightStatus::RequestFailed;
                    if !self.state.ever_succeeded {
                        self.state.last_sent_key = None;
                        self.state.last_error_at = Some(Instant::now());
                    }
                    updated = true;
                }
            }
        }
        updated
    }

    fn queue(&mut self, serial: &str, snapshot: InsightSnapshot, now: Instant) {
        if snapshot.clusters.is_empty() {
            return;
        }
        let key = snapshot.digest_key();
        self.state.generation = self.state.generation.wrapping_add(1);
        self.state.status = InsightStatus::RequestSent;
        self.state.last_analyze = Some(now);
        self.state.last_sent_key = Some(key);
        self.serial = Some(serial.to_string());
        self.rx = Some(spawn_insight(
            self.config.clone(),
            snapshot,
            self.state.generation,
            serial.to_string(),
        ));
    }
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
    pub(crate) last_analyze: Option<Instant>,
    pub(crate) last_error_at: Option<Instant>,
    pub(crate) last_built_error_at: Option<Instant>,
    pub(crate) last_sent_key: Option<String>,
    pub(crate) ever_succeeded: bool,
    pub(crate) generation: u64,
}

impl Default for InsightState {
    fn default() -> Self {
        Self {
            status: InsightStatus::Idle,
            replies: VecDeque::new(),
            last_analyze: None,
            last_error_at: None,
            last_built_error_at: None,
            last_sent_key: None,
            ever_succeeded: false,
            generation: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn error_snapshot(now: Instant) -> InsightSnapshot {
        build_snapshot(
            [InsightLine {
                received_at: now,
                level: 'E',
                tag: "Foo".to_string(),
                message: "boom".to_string(),
            }],
            LevelMask::Error,
            "Pixel",
            "serial",
            now,
        )
    }

    #[test]
    fn empty_clusters_do_not_send() {
        let now = Instant::now();
        let snapshot = build_snapshot([], LevelMask::Error, "Pixel", "serial", now);
        let mut controller = InsightController::default();
        controller.state.last_error_at = Some(now - INSIGHT_SETTLE);
        assert!(!controller.should_send(&snapshot, now));
    }

    #[test]
    fn first_error_after_settle_sends() {
        let now = Instant::now();
        let mut controller = InsightController::default();
        controller.state.last_error_at = Some(now - INSIGHT_SETTLE);
        assert!(controller.should_send(&error_snapshot(now), now));
    }

    #[test]
    fn same_digest_inside_cooldown_does_not_send() {
        let now = Instant::now();
        let snapshot = error_snapshot(now);
        let mut controller = InsightController::default();
        controller.state.last_sent_key = Some(snapshot.digest_key());
        controller.state.last_analyze = Some(now);
        assert!(!controller.should_send(&snapshot, now));
    }

    #[test]
    fn request_sent_does_not_send() {
        let now = Instant::now();
        let mut controller = InsightController::default();
        controller.state.status = InsightStatus::RequestSent;
        controller.state.last_error_at = Some(now - INSIGHT_SETTLE);
        assert!(!controller.should_send(&error_snapshot(now), now));
    }

    #[test]
    fn consider_first_send_before_settle() {
        let now = Instant::now();
        let mut controller = InsightController::default();
        controller.state.last_error_at = Some(now);
        assert!(!controller.should_consider(now));
    }

    #[test]
    fn consider_first_send_after_settle() {
        let now = Instant::now();
        let mut controller = InsightController::default();
        controller.state.last_error_at = Some(now - INSIGHT_SETTLE);
        assert!(controller.should_consider(now));
    }

    #[test]
    fn consider_cooldown_ring_unchanged() {
        let now = Instant::now();
        let at = now - Duration::from_secs(1);
        let mut controller = InsightController::default();
        controller.state.last_sent_key = Some("key".into());
        controller.state.last_analyze = Some(now);
        controller.state.last_error_at = Some(at);
        controller.state.last_built_error_at = Some(at);
        assert!(!controller.should_consider(now));
    }

    #[test]
    fn consider_cooldown_new_error() {
        let now = Instant::now();
        let mut controller = InsightController::default();
        controller.state.last_sent_key = Some("key".into());
        controller.state.last_analyze = Some(now);
        controller.state.last_built_error_at = Some(now - Duration::from_secs(2));
        controller.state.last_error_at = Some(now);
        assert!(controller.should_consider(now));
    }

    #[test]
    fn consider_cooled_idle_ring_unchanged() {
        let now = Instant::now();
        let at = now - INSIGHT_COOLDOWN;
        let mut controller = InsightController::default();
        controller.state.last_sent_key = Some("key".into());
        controller.state.last_analyze = Some(at);
        controller.state.last_error_at = Some(at);
        controller.state.last_built_error_at = Some(at);
        assert!(!controller.should_consider(now));
    }

    #[test]
    fn consider_cooled_failed_ring_unchanged() {
        let now = Instant::now();
        let at = now - INSIGHT_COOLDOWN;
        let mut controller = InsightController::default();
        controller.state.status = InsightStatus::RequestFailed;
        controller.state.last_sent_key = Some("key".into());
        controller.state.last_analyze = Some(at);
        controller.state.last_error_at = Some(at);
        controller.state.last_built_error_at = Some(at);
        assert!(controller.should_consider(now));
    }
}
