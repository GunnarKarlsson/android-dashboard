use std::collections::VecDeque;
use std::time::{Duration, Instant};

use ai_insight::{
    build_snapshot, spawn_insight, InsightLine, InsightSnapshot, InsightUpdate, LevelMask,
};
use crossbeam_channel::Receiver;

const MAX_INSIGHTS: usize = 100;
const INSIGHT_SETTLE: Duration = Duration::from_secs(5);
const INSIGHT_COOLDOWN: Duration = Duration::from_secs(30);

pub struct InsightController {
    pub auto_update_feed: bool,
    pub state: InsightState,
    rx: Option<Receiver<InsightUpdate>>,
    serial: Option<String>,
}

impl Default for InsightController {
    fn default() -> Self {
        Self {
            auto_update_feed: true,
            state: InsightState::default(),
            rx: None,
            serial: None,
        }
    }
}

impl InsightController {
    /// Clears request state and the in-flight channel. Leaves `auto_update_feed` unchanged.
    pub(crate) fn reset(&mut self) {
        self.state = InsightState::default();
        self.rx = None;
        self.serial = None;
    }

    /// Records that the errors pane accepted a line or flushed pending.
    pub(crate) fn note_error(&mut self) {
        self.state.last_error_at = Some(Instant::now());
    }

    /// Builds a snapshot. If settle/cooldown/digest say send, queues the worker.
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

        let now = Instant::now();
        let snapshot = build_snapshot(lines, LevelMask::Error, model, serial, now);
        if !self.should_send(&snapshot, now) {
            return false;
        }

        self.queue(serial, snapshot, now);
        true
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
            None => self
                .state
                .last_error_at
                .is_some_and(|at| now.saturating_duration_since(at) >= INSIGHT_SETTLE),
            Some(prev) => {
                let cooled = self
                    .state
                    .last_analyze
                    .is_none_or(|at| now.saturating_duration_since(at) >= INSIGHT_COOLDOWN);
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
                    tracing::info!(stored = self.state.replies.len(), "insight reply stored");
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
            snapshot,
            self.state.generation,
            serial.to_string(),
        ));
        tracing::info!(
            generation = self.state.generation,
            "insight request queued"
        );
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
}
