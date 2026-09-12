use std::collections::{HashSet, VecDeque};
use std::time::{Duration, Instant};

use ai_insight::{
    InsightConfig, InsightLine, InsightSnapshot, InsightUpdate, LevelMask, build_snapshot,
    spawn_insight,
};
use crossbeam_channel::Receiver;

const MAX_INSIGHTS: usize = 100;
const INSIGHT_SETTLE: Duration = Duration::from_secs(3);
const INSIGHT_COOLDOWN: Duration = Duration::from_secs(15);

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

    /// Adds high-severity fingerprints for a later forced insight after settle.
    ///
    /// Multiple fps in one burst coalesce: force fires once after quiet settle.
    pub(crate) fn note_high_severity(&mut self, fps: impl IntoIterator<Item = String>) {
        self.state.pending_high_fps.extend(fps);
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
        if !self.should_send(&snapshot, now) {
            // Drop duplicate high fps inside cooldown so ticks do not rebuild forever.
            self.suppress_duplicate_pending_high(now);
            return false;
        }

        self.queue(serial, snapshot, now);
        true
    }

    /// Returns whether clustering is worth doing at `now`.
    fn should_consider(&self, now: Instant) -> bool {
        // Settled pending high severity: evaluate even if the digest ring looks unchanged.
        if !self.state.pending_high_fps.is_empty() && self.settled(now) {
            return true;
        }
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

    /// True when pending high fps should force a send after settle.
    ///
    /// New fingerprints force even inside cooldown; the same set only re-arms after cooldown.
    fn should_force_high(&self, now: Instant) -> bool {
        if self.state.pending_high_fps.is_empty() || !self.settled(now) {
            return false;
        }
        self.pending_has_unsent_high() || self.cooled(now)
    }

    fn pending_has_unsent_high(&self) -> bool {
        self.state
            .pending_high_fps
            .iter()
            .any(|fp| !self.state.last_sent_high_fps.contains(fp))
    }

    /// Clears pending high fps that cannot force yet (duplicates inside cooldown).
    fn suppress_duplicate_pending_high(&mut self, now: Instant) {
        if self.state.pending_high_fps.is_empty() {
            return;
        }
        if self.should_force_high(now) {
            return;
        }
        if !self.settled(now) {
            return;
        }
        self.state.pending_high_fps.clear();
        self.state.last_built_error_at = self.state.last_error_at;
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

        // Force lane: fatal/ANR pending fps after settle (additive to digest rules below).
        if self.should_force_high(now) {
            return true;
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
                    self.state
                        .replies
                        .push_back(InsightReply::from_model_text(text));
                    while self.state.replies.len() > MAX_INSIGHTS {
                        self.state.replies.pop_front();
                    }
                    self.state.status = InsightStatus::Idle;
                    self.state.last_failure = None;
                    self.state.ever_succeeded = true;
                    updated = true;
                }
                InsightUpdate::Error { message, .. } => {
                    self.state.status = InsightStatus::RequestFailed;
                    self.state.last_failure = Some(message);
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
        // Record high fps that armed this send so the same crash does not force again until cooldown.
        self.state
            .last_sent_high_fps
            .extend(self.state.pending_high_fps.drain());
        self.state.last_built_error_at = self.state.last_error_at;
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

/// One stored insight reply with the local wall time it arrived.
#[derive(Debug, Clone)]
pub struct InsightReply {
    /// Local receive time, e.g. `22:31:05`.
    pub received_at: String,
    /// Model body without a HEALTHY/DEGRADING/FAILING verdict line.
    pub body: String,
}

impl InsightReply {
    /// Builds a reply stamped with the current local time.
    ///
    /// Drops a leading HEALTHY / DEGRADING / FAILING line when present.
    pub fn from_model_text(text: String) -> Self {
        Self {
            received_at: chrono::Local::now().format("%H:%M:%S").to_string(),
            body: body_without_verdict(&text),
        }
    }
}

/// Returns the model text with a leading verdict line removed when present.
fn body_without_verdict(text: &str) -> String {
    let text = text.trim();
    let Some((first, rest)) = text.split_once('\n') else {
        return if is_verdict_line(text) {
            String::new()
        } else {
            text.to_string()
        };
    };
    if is_verdict_line(first.trim()) {
        rest.trim_start().to_string()
    } else {
        text.to_string()
    }
}

fn is_verdict_line(line: &str) -> bool {
    matches!(
        line.split_whitespace().next(),
        Some("HEALTHY" | "DEGRADING" | "FAILING")
    )
}

pub struct InsightState {
    pub status: InsightStatus,
    pub replies: VecDeque<InsightReply>,
    /// Last user-facing failure text when [`InsightStatus::RequestFailed`].
    pub last_failure: Option<String>,
    pub(crate) last_analyze: Option<Instant>,
    pub(crate) last_error_at: Option<Instant>,
    pub(crate) last_built_error_at: Option<Instant>,
    pub(crate) last_sent_key: Option<String>,
    /// High-severity fingerprints waiting for settle before a forced refresh.
    pub(crate) pending_high_fps: HashSet<String>,
    /// High-severity fingerprints included in a prior successful force/queue.
    pub(crate) last_sent_high_fps: HashSet<String>,
    pub(crate) ever_succeeded: bool,
    pub(crate) generation: u64,
}

impl Default for InsightState {
    fn default() -> Self {
        Self {
            status: InsightStatus::Idle,
            replies: VecDeque::new(),
            last_failure: None,
            last_analyze: None,
            last_error_at: None,
            last_built_error_at: None,
            last_sent_key: None,
            pending_high_fps: HashSet::new(),
            last_sent_high_fps: HashSet::new(),
            ever_succeeded: false,
            generation: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ai_insight::generate_fingerprint;

    fn error_snapshot(now: Instant) -> InsightSnapshot {
        build_snapshot(
            [InsightLine {
                received_at: now,
                level: 'E',
                tag: "Foo".to_string(),
                message: "boom".to_string(),
                pid: 1,
            }],
            LevelMask::Error,
            "Pixel",
            "serial",
            now,
        )
    }

    fn fatal_snapshot(now: Instant) -> InsightSnapshot {
        build_snapshot(
            [InsightLine {
                received_at: now,
                level: 'E',
                tag: "AndroidRuntime".to_string(),
                message: "FATAL EXCEPTION: main".to_string(),
                pid: 1,
            }],
            LevelMask::Error,
            "Pixel",
            "serial",
            now,
        )
    }

    fn fatal_fp() -> String {
        generate_fingerprint("AndroidRuntime", "FATAL EXCEPTION: main")
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

    #[test]
    fn force_fatal_after_settle_inside_cooldown() {
        let now = Instant::now();
        let snapshot = fatal_snapshot(now);
        let mut controller = InsightController::default();
        controller.state.last_sent_key = Some("other".into());
        controller.state.last_analyze = Some(now);
        controller.state.last_error_at = Some(now - INSIGHT_SETTLE);
        controller.state.pending_high_fps.insert(fatal_fp());
        assert!(controller.should_force_high(now));
        assert!(controller.should_send(&snapshot, now));
    }

    #[test]
    fn force_same_fp_inside_cooldown_does_not_send() {
        let now = Instant::now();
        let snapshot = fatal_snapshot(now);
        let fp = fatal_fp();
        let mut controller = InsightController::default();
        controller.state.last_sent_key = Some(snapshot.digest_key());
        controller.state.last_analyze = Some(now);
        controller.state.last_error_at = Some(now - INSIGHT_SETTLE);
        controller.state.last_sent_high_fps.insert(fp.clone());
        controller.state.pending_high_fps.insert(fp);
        assert!(!controller.should_force_high(now));
        assert!(!controller.should_send(&snapshot, now));
    }

    #[test]
    fn force_same_fp_after_cooldown_sends() {
        let now = Instant::now();
        let snapshot = fatal_snapshot(now);
        let fp = fatal_fp();
        let mut controller = InsightController::default();
        controller.state.last_sent_key = Some(snapshot.digest_key());
        controller.state.last_analyze = Some(now - INSIGHT_COOLDOWN);
        controller.state.last_error_at = Some(now - INSIGHT_SETTLE);
        controller.state.last_sent_high_fps.insert(fp.clone());
        controller.state.pending_high_fps.insert(fp);
        assert!(controller.should_force_high(now));
        assert!(controller.should_send(&snapshot, now));
    }

    #[test]
    fn force_before_settle_does_not_send() {
        let now = Instant::now();
        let snapshot = fatal_snapshot(now);
        let mut controller = InsightController::default();
        controller.state.last_sent_key = Some(snapshot.digest_key());
        controller.state.last_analyze = Some(now);
        controller.state.last_error_at = Some(now);
        controller.state.pending_high_fps.insert(fatal_fp());
        assert!(!controller.should_force_high(now));
        assert!(!controller.should_send(&snapshot, now));
    }

    #[test]
    fn strips_verdict_headline() {
        assert_eq!(
            body_without_verdict("DEGRADING\nTop issues: crash"),
            "Top issues: crash"
        );
        assert_eq!(
            body_without_verdict("FAILING  disk full\nNext checks: free space"),
            "Next checks: free space"
        );
        assert_eq!(body_without_verdict("HEALTHY"), "");
        assert_eq!(
            body_without_verdict("Top issues: already clean"),
            "Top issues: already clean"
        );
    }

    #[test]
    fn reply_stamps_local_time() {
        let reply = InsightReply::from_model_text("DEGRADING\nTop issues: anr".to_string());
        assert_eq!(reply.body, "Top issues: anr");
        assert_eq!(reply.received_at.len(), 8);
        assert!(reply.received_at.chars().nth(2) == Some(':'));
    }
}
