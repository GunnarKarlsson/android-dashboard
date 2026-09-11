use std::collections::HashMap;
use std::thread::{self, JoinHandle};
use std::time::{Duration, SystemTime};

use crossbeam_channel::{Receiver, Sender};

use crate::adb::run_adb_for_serial;
use crate::background::{signal_stop_and_detach, sleep_until_stop};
use crate::error::AdbError;

const DEFAULT_POLL_INTERVAL: Duration = Duration::from_secs(30);
const USER_STORAGE_ROOT: &str = "/storage/emulated/0";

/// User-visible storage category (photos, downloads, etc.).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StorageCategory {
    pub name: String,
    pub bytes: u64,
}

/// Total user storage from `df` on `/storage/emulated`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StorageOverview {
    pub total_bytes: u64,
    pub used_bytes: u64,
    pub available_bytes: u64,
    pub use_percent: u32,
}

impl StorageOverview {
    pub fn used_fraction(&self) -> f32 {
        if self.total_bytes == 0 {
            0.0
        } else {
            self.used_bytes as f32 / self.total_bytes as f32
        }
    }
}

/// Folder-based category sizes for user storage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StorageBreakdown {
    pub categories: Vec<StorageCategory>,
    pub timestamp: SystemTime,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StorageBreakdownUpdate {
    Breakdown(StorageBreakdown),
    Error(String),
}

struct FolderSpec {
    folder: &'static str,
    category: &'static str,
}

const FOLDER_SPECS: &[FolderSpec] = &[
    FolderSpec {
        folder: "DCIM",
        category: "Photos & videos",
    },
    FolderSpec {
        folder: "Pictures",
        category: "Photos & videos",
    },
    FolderSpec {
        folder: "Movies",
        category: "Photos & videos",
    },
    FolderSpec {
        folder: "Music",
        category: "Audio",
    },
    FolderSpec {
        folder: "Podcasts",
        category: "Audio",
    },
    FolderSpec {
        folder: "Audiobooks",
        category: "Audio",
    },
    FolderSpec {
        folder: "Ringtones",
        category: "Audio",
    },
    FolderSpec {
        folder: "Documents",
        category: "Documents",
    },
    FolderSpec {
        folder: "Download",
        category: "Downloads",
    },
    FolderSpec {
        folder: "Android",
        category: "Other",
    },
];

const CATEGORY_ORDER: &[&str] = &[
    "Photos & videos",
    "Documents",
    "Downloads",
    "Audio",
    "Other",
];

/// Background poller for folder-based storage category sizes.
pub struct StorageBreakdownPoller {
    stop_tx: Sender<()>,
    join_handle: Option<JoinHandle<()>>,
}

impl StorageBreakdownPoller {
    pub fn spawn(serial: &str) -> Result<(Receiver<StorageBreakdownUpdate>, Self), AdbError> {
        Self::spawn_with_interval(serial, DEFAULT_POLL_INTERVAL)
    }

    pub fn spawn_with_interval(
        serial: &str,
        interval: Duration,
    ) -> Result<(Receiver<StorageBreakdownUpdate>, Self), AdbError> {
        let (update_tx, update_rx) = crossbeam_channel::unbounded();
        let (stop_tx, stop_rx) = crossbeam_channel::unbounded();
        let serial = serial.to_string();

        let join_handle = thread::spawn(move || {
            while stop_rx.try_recv().is_err() {
                match fetch_storage_breakdown(&serial) {
                    Ok(breakdown) => {
                        if update_tx
                            .send(StorageBreakdownUpdate::Breakdown(breakdown))
                            .is_err()
                        {
                            break;
                        }
                    }
                    Err(err) => {
                        if update_tx
                            .send(StorageBreakdownUpdate::Error(err.user_message()))
                            .is_err()
                        {
                            break;
                        }
                    }
                }

                sleep_until_stop(&stop_rx, interval);
            }
        });

        Ok((
            update_rx,
            StorageBreakdownPoller {
                stop_tx,
                join_handle: Some(join_handle),
            },
        ))
    }

    pub fn stop(mut self) {
        self.shutdown();
    }

    fn shutdown(&mut self) {
        signal_stop_and_detach(&self.stop_tx, &mut self.join_handle);
    }
}

impl Drop for StorageBreakdownPoller {
    fn drop(&mut self) {
        self.shutdown();
    }
}

pub fn fetch_storage_overview(serial: &str) -> Result<StorageOverview, AdbError> {
    let df = run_adb_for_serial(serial, &["shell", "df", "-k", USER_STORAGE_ROOT])?;
    parse_user_storage_df(&String::from_utf8_lossy(&df.stdout))
}

fn fetch_storage_breakdown(serial: &str) -> Result<StorageBreakdown, AdbError> {
    let folder_paths: Vec<String> = FOLDER_SPECS
        .iter()
        .map(|spec| folder_path(spec.folder))
        .collect();
    let mut du_args = vec!["shell".to_string(), "du".to_string(), "-sb".to_string()];
    du_args.extend(folder_paths.iter().cloned());

    let du = run_adb_for_serial(
        serial,
        &du_args.iter().map(String::as_str).collect::<Vec<_>>(),
    )?;
    let folder_sizes = parse_du_bytes(&String::from_utf8_lossy(&du.stdout));
    let categories = aggregate_categories(&folder_sizes);

    Ok(StorageBreakdown {
        categories,
        timestamp: SystemTime::now(),
    })
}

fn folder_path(folder: &str) -> String {
    format!("{USER_STORAGE_ROOT}/{folder}")
}

fn parse_user_storage_df(text: &str) -> Result<StorageOverview, AdbError> {
    for line in text.lines().skip(1) {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 6 {
            continue;
        }

        let total_kb: u64 = parts[1]
            .parse()
            .map_err(|_| AdbError::ParseFailed(format!("invalid df size: {}", parts[1])))?;
        let used_kb: u64 = parts[2]
            .parse()
            .map_err(|_| AdbError::ParseFailed(format!("invalid df used: {}", parts[2])))?;
        let available_kb: u64 = parts[3]
            .parse()
            .map_err(|_| AdbError::ParseFailed(format!("invalid df avail: {}", parts[3])))?;
        let use_percent = parts[4].trim_end_matches('%').parse().unwrap_or(0);

        return Ok(StorageOverview {
            total_bytes: total_kb.saturating_mul(1024),
            used_bytes: used_kb.saturating_mul(1024),
            available_bytes: available_kb.saturating_mul(1024),
            use_percent,
        });
    }

    Err(AdbError::ParseFailed("missing user storage df line".into()))
}

fn parse_du_bytes(text: &str) -> HashMap<String, u64> {
    let mut sizes = HashMap::new();

    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let Some((bytes_raw, path)) = line.split_once('\t').or_else(|| line.split_once(' ')) else {
            continue;
        };

        let Ok(bytes) = bytes_raw.parse::<u64>() else {
            continue;
        };

        sizes.insert(path.trim().to_string(), bytes);
    }

    sizes
}

fn aggregate_categories(folder_sizes: &HashMap<String, u64>) -> Vec<StorageCategory> {
    let mut totals: HashMap<&str, u64> = HashMap::new();

    for spec in FOLDER_SPECS {
        let path = folder_path(spec.folder);
        let bytes = folder_sizes.get(&path).copied().unwrap_or(0);
        *totals.entry(spec.category).or_default() += bytes;
    }

    CATEGORY_ORDER
        .iter()
        .map(|name| StorageCategory {
            name: (*name).to_string(),
            bytes: totals.get(name).copied().unwrap_or(0),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_user_storage_df_sample() {
        let sample = r#"Filesystem     1K-blocks    Used Available Use% Mounted on
/dev/fuse        6082144 5324108    615824  90% /storage/emulated
"#;

        let overview = parse_user_storage_df(sample).unwrap();
        assert_eq!(overview.total_bytes, 6_082_144 * 1024);
        assert_eq!(overview.used_bytes, 5_324_108 * 1024);
        assert_eq!(overview.available_bytes, 615_824 * 1024);
        assert_eq!(overview.use_percent, 90);
    }

    #[test]
    fn parse_user_storage_df_empty_or_garbage() {
        assert!(parse_user_storage_df("").is_err());
        assert!(
            parse_user_storage_df("Filesystem     1K-blocks    Used Available Use% Mounted on\n")
                .is_err()
        );
        assert!(parse_user_storage_df(
            "Filesystem     1K-blocks    Used Available Use% Mounted on\n/dev/fuse  abc  def  ghi  90% /storage/emulated\n"
        )
        .is_err());
    }

    #[test]
    fn parse_du_bytes_sample() {
        let sample = r#"7222	/storage/emulated/0/Download
83987	/storage/emulated/0/Pictures
"#;

        let sizes = parse_du_bytes(sample);
        assert_eq!(sizes.get("/storage/emulated/0/Download"), Some(&7222));
        assert_eq!(sizes.get("/storage/emulated/0/Pictures"), Some(&83987));
    }

    #[test]
    fn parse_du_bytes_empty_or_garbage() {
        assert!(parse_du_bytes("").is_empty());
        assert!(parse_du_bytes("garbage").is_empty());
        assert!(parse_du_bytes("notanumber\t/path").is_empty());
    }

    #[test]
    fn aggregate_categories_groups_folders() {
        let mut sizes = HashMap::new();
        sizes.insert(folder_path("DCIM"), 1000);
        sizes.insert(folder_path("Pictures"), 2000);
        sizes.insert(folder_path("Movies"), 500);
        sizes.insert(folder_path("Documents"), 800);
        sizes.insert(folder_path("Download"), 400);
        sizes.insert(folder_path("Music"), 300);
        sizes.insert(folder_path("Android"), 600);

        let categories = aggregate_categories(&sizes);
        assert_eq!(categories.len(), 5);
        assert_eq!(categories[0].name, "Photos & videos");
        assert_eq!(categories[0].bytes, 3500);
        assert_eq!(categories[1].name, "Documents");
        assert_eq!(categories[1].bytes, 800);
        assert_eq!(categories[2].name, "Downloads");
        assert_eq!(categories[2].bytes, 400);
        assert_eq!(categories[3].name, "Audio");
        assert_eq!(categories[3].bytes, 300);
        assert_eq!(categories[4].name, "Other");
        assert_eq!(categories[4].bytes, 600);
    }
}
