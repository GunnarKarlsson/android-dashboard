mod client;
mod config;
mod fingerprint;
mod reduce;
mod worker;

pub use config::InsightConfig;
pub use fingerprint::{generate_device_label, generate_fingerprint, redact};
pub use reduce::{build_snapshot, log_snapshot, InsightLine, InsightSnapshot, LevelMask};
pub use worker::{spawn_insight, InsightUpdate};
