use std::collections::HashMap;
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::fingerprint::{generate_device_label, generate_fingerprint, redact};

const TIME_WINDOW: Duration = Duration::from_secs(180);
const PREV_TIME_WINDOW: Duration = Duration::from_secs(180);
const MAX_CLUSTERS: usize = 8;
const MAX_SAMPLES: usize = 2;
const MAX_JSON_BYTES: usize = 6 * 1024;
const DIGEST_COUNT_BUCKET: u32 = 5;

/// Enum indicating allowed logcat levels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LevelMask {
    Error,
}

impl LevelMask {
    pub fn contains(self, level: char) -> bool {
        match self {
            LevelMask::Error => matches!(level, 'E' | 'F'),
        }
    }

    pub fn labels(self) -> &'static [&'static str] {
        match self {
            LevelMask::Error => &["E", "F"],
        }
    }
}

/// One log line input to the reducer.
#[derive(Debug, Clone)]
pub struct InsightLine {
    pub received_at: Instant,
    pub level: char,
    pub tag: String,
    pub message: String,
}

/// One error shape in the current time window: fingerprint, counts, and redacted samples.
#[derive(Debug, Clone, Serialize)]
pub struct InsightCluster {
    pub fingerprint: String,
    pub tag: String,
    pub level: char,
    pub count: u32,
    #[serde(rename = "first")]
    pub first_secs_ago: u64,
    #[serde(rename = "last")]
    pub last_secs_ago: u64,
    pub samples: Vec<String>,
}

/// Counts for matching lines for the current time window and the time window before it.
#[derive(Debug, Clone, Serialize)]
pub struct SnapshotDeltas {
    pub count_now: u32,
    pub count_prev: u32,
}

/// Reduced digest of recent log lines for one device: clusters, level mask, and deltas.
#[derive(Debug, Clone, Serialize)]
pub struct InsightSnapshot {
    pub device_label: String,
    pub device_model: String,
    pub time_window_sec: u64,
    pub levels: Vec<&'static str>,
    pub clusters: Vec<InsightCluster>,
    pub deltas: SnapshotDeltas,
    pub metrics: Option<serde_json::Value>,
}

impl InsightSnapshot {
    pub fn to_pretty_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    /// Builds a short key from top clusters as `fingerprint:{count / DIGEST_COUNT_BUCKET}` pairs joined by `|`.
    pub fn digest_key(&self) -> String {
        self.clusters
            .iter()
            .map(|cluster| {
                format!(
                    "{}:{}",
                    cluster.fingerprint,
                    cluster.count / DIGEST_COUNT_BUCKET
                )
            })
            .collect::<Vec<_>>()
            .join("|")
    }

    /// Returns true if any fatal or AndroidRuntime cluster fingerprint is absent from `previous_key`.
    pub fn has_new_high_severity(&self, previous_key: &str) -> bool {
        self.clusters.iter().any(|cluster| {
            (cluster.level == 'F' || cluster.tag == "AndroidRuntime")
                && !previous_key.contains(cluster.fingerprint.as_str())
        })
    }
}

pub fn log_snapshot(snapshot: &InsightSnapshot) {
    match snapshot.to_pretty_json() {
        Ok(json) => eprintln!("insight snapshot:\n{json}"),
        Err(err) => eprintln!("insight snapshot serialize error: {err}"),
    }
}

pub fn build_snapshot(
    lines: impl IntoIterator<Item = InsightLine>,
    mask: LevelMask,
    device_model: &str,
    device_serial: &str,
    now: Instant,
) -> InsightSnapshot {
    let prev_cut = now.checked_sub(TIME_WINDOW + PREV_TIME_WINDOW);
    let now_cut = now.checked_sub(TIME_WINDOW);

    let mut now_map: HashMap<String, InsightCluster> = HashMap::new();
    let mut count_now = 0u32;
    let mut count_prev = 0u32;

    for line in lines {
        if !mask.contains(line.level) {
            continue;
        }
        if prev_cut.is_some_and(|cut| line.received_at < cut) {
            continue;
        }

        let in_now = now_cut.is_none_or(|cut| line.received_at >= cut);
        if in_now {
            count_now += 1;
            let fp = generate_fingerprint(&line.tag, &line.message);
            let age = now.saturating_duration_since(line.received_at).as_secs();
            let cluster = now_map.entry(fp.clone()).or_insert_with(|| InsightCluster {
                fingerprint: fp,
                tag: line.tag.clone(),
                level: line.level,
                count: 0,
                first_secs_ago: age,
                last_secs_ago: age,
                samples: Vec::new(),
            });
            cluster.count += 1;
            cluster.first_secs_ago = cluster.first_secs_ago.max(age);
            cluster.last_secs_ago = cluster.last_secs_ago.min(age);
            if cluster.samples.len() < MAX_SAMPLES {
                cluster.samples.push(redact(&line.message));
            }
        } else {
            count_prev += 1;
        }
    }

    let mut clusters: Vec<InsightCluster> = now_map.into_values().collect();
    clusters.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.tag.cmp(&b.tag)));
    clusters.truncate(MAX_CLUSTERS);
    trim_to_json_budget(&mut clusters);

    InsightSnapshot {
        device_label: generate_device_label(device_model, device_serial),
        device_model: device_model.to_string(),
        time_window_sec: TIME_WINDOW.as_secs(),
        levels: mask.labels().to_vec(),
        clusters,
        deltas: SnapshotDeltas {
            count_now,
            count_prev,
        },
        metrics: None,
    }
}

fn trim_to_json_budget(clusters: &mut Vec<InsightCluster>) {
    while clusters.len() > 1 {
        let Ok(json) = serde_json::to_string(&InsightSnapshotLite { clusters }) else {
            break;
        };
        if json.len() <= MAX_JSON_BYTES {
            break;
        }
        clusters.pop();
    }
}

#[derive(Serialize)]
struct InsightSnapshotLite<'a> {
    clusters: &'a [InsightCluster],
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(now: Instant, ago: u64, level: char, tag: &str, message: &str) -> InsightLine {
        InsightLine {
            received_at: now - Duration::from_secs(ago),
            level,
            tag: tag.to_string(),
            message: message.to_string(),
        }
    }

    #[test]
    fn empty_buffer_snapshot() {
        let now = Instant::now();
        let snap = build_snapshot([], LevelMask::Error, "Pixel 8", "serial-1", now);
        assert!(snap.clusters.is_empty());
        assert_eq!(snap.deltas.count_now, 0);
        assert_eq!(snap.deltas.count_prev, 0);
        assert_eq!(snap.levels, ["E", "F"]);
        assert!(snap.metrics.is_none());
        assert!(!snap.device_label.contains("serial-1"));
    }

    #[test]
    fn clusters_same_fingerprint() {
        let now = Instant::now();
        let lines = [
            line(now, 10, 'E', "OkHttp", "failed host 1"),
            line(now, 8, 'E', "OkHttp", "failed host 2"),
            line(now, 6, 'E', "OkHttp", "failed host 3"),
            line(now, 4, 'E', "System", "disk full"),
        ];
        let snap = build_snapshot(lines, LevelMask::Error, "Pixel 8", "serial-1", now);
        assert_eq!(snap.clusters.len(), 2);
        assert_eq!(snap.clusters[0].tag, "OkHttp");
        assert_eq!(snap.clusters[0].count, 3);
        assert_eq!(snap.clusters[1].tag, "System");
        assert_eq!(snap.clusters[1].count, 1);
        assert_eq!(snap.deltas.count_now, 4);
    }

    #[test]
    fn ignores_info_and_old_lines() {
        let now = Instant::now();
        let lines = [
            line(now, 10, 'I', "OkHttp", "ok"),
            line(now, 400, 'E', "OkHttp", "ancient"),
            line(now, 200, 'E', "OkHttp", "previous window"),
            line(now, 5, 'F', "AndroidRuntime", "FATAL EXCEPTION"),
        ];
        let snap = build_snapshot(lines, LevelMask::Error, "Pixel 8", "serial-1", now);
        assert_eq!(snap.deltas.count_now, 1);
        assert_eq!(snap.deltas.count_prev, 1);
        assert_eq!(snap.clusters.len(), 1);
        assert_eq!(snap.clusters[0].tag, "AndroidRuntime");
    }

    #[test]
    fn samples_are_redacted() {
        let now = Instant::now();
        let lines = [line(
            now,
            3,
            'E',
            "AndroidRuntime",
            "token Bearer secret.jwt password=s3cret",
        )];
        let snap = build_snapshot(lines, LevelMask::Error, "Pixel 8", "serial-1", now);
        assert_eq!(snap.clusters.len(), 1);
        assert_eq!(snap.clusters[0].tag, "AndroidRuntime");
        assert!(!snap.clusters[0].samples[0].contains("secret.jwt"));
        assert!(!snap.clusters[0].samples[0].contains("s3cret"));
    }

    #[test]
    fn digest_key_stable_for_same_count_bucket() {
        let now = Instant::now();
        let lines_a = [
            line(now, 10, 'E', "OkHttp", "failed host 1"),
            line(now, 8, 'E', "OkHttp", "failed host 2"),
            line(now, 6, 'E', "OkHttp", "failed host 3"),
        ];
        let lines_b = [
            line(now, 10, 'E', "OkHttp", "failed host 1"),
            line(now, 8, 'E', "OkHttp", "failed host 2"),
            line(now, 6, 'E', "OkHttp", "failed host 3"),
            line(now, 4, 'E', "OkHttp", "failed host 4"),
        ];
        let snap_a = build_snapshot(lines_a, LevelMask::Error, "Pixel 8", "serial-1", now);
        let snap_b = build_snapshot(lines_b, LevelMask::Error, "Pixel 8", "serial-1", now);
        assert_eq!(snap_a.digest_key(), snap_b.digest_key());
        assert_eq!(snap_a.clusters[0].count / DIGEST_COUNT_BUCKET, 0);
        assert_eq!(snap_b.clusters[0].count / DIGEST_COUNT_BUCKET, 0);
    }

    #[test]
    fn digest_key_changes_for_new_fingerprint() {
        let now = Instant::now();
        let lines_a = [line(now, 5, 'E', "OkHttp", "failed host")];
        let lines_b = [
            line(now, 5, 'E', "OkHttp", "failed host"),
            line(now, 3, 'F', "AndroidRuntime", "FATAL EXCEPTION"),
        ];
        let snap_a = build_snapshot(lines_a, LevelMask::Error, "Pixel 8", "serial-1", now);
        let snap_b = build_snapshot(lines_b, LevelMask::Error, "Pixel 8", "serial-1", now);
        assert_ne!(snap_a.digest_key(), snap_b.digest_key());
        assert!(snap_b.has_new_high_severity(&snap_a.digest_key()));
        assert!(!snap_a.has_new_high_severity(&snap_a.digest_key()));
    }
}
