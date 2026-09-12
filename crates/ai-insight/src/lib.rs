mod client;
mod config;
mod event;
mod fingerprint;
mod reduce;
mod severity;
mod worker;

pub use config::InsightConfig;
pub use event::{
    InsightEvent, InsightLine, fold_lines_to_events, is_stack_head, is_stack_head_message,
};
pub use fingerprint::{generate_device_label, generate_fingerprint, redact};
pub use reduce::{InsightSnapshot, LevelMask, build_snapshot, log_snapshot};
pub use severity::is_high_severity;
pub use worker::{InsightUpdate, spawn_insight};
