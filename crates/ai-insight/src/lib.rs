mod client;
mod config;
mod fingerprint;
mod reduce;
mod worker;

pub use config::InsightConfig;
pub use fingerprint::{generate_device_label, generate_fingerprint, redact};
pub use reduce::{InsightLine, InsightSnapshot, LevelMask, build_snapshot, log_snapshot};
pub use worker::{InsightUpdate, spawn_insight};
