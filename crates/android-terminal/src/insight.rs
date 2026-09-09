use std::collections::VecDeque;
use std::time::Instant;

#[allow(dead_code)]
pub struct InsightController;

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
