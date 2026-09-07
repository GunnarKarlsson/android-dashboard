use std::sync::LazyLock;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use crossbeam_channel::{Receiver, Sender};
use regex::Regex;

use crate::adb::run_adb_for_serial;
use crate::background::{signal_stop_and_detach, sleep_until_stop};
use crate::error::AdbError;

const REFRESH_INTERVAL: Duration = Duration::from_secs(60);
const BATCH_SIZE: usize = 20;

static STORAGE_BYTES: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^(.+?): (\d+) bytes").expect("valid storage bytes regex"));

static PKG_MARKER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^@PKG@(.+)$").expect("valid package marker regex"));

/// Per-app storage sizes from `cmd package get-package-storage-stats`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageStorage {
    pub package: String,
    pub total_bytes: u64,
}

/// Incremental update from the app storage poller.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppStorageUpdate {
    PackageList(Vec<String>),
    PackageStorage(PackageStorage),
    ScanComplete,
    Error(String),
}

/// Background poller that streams app storage sizes as they are measured.
pub struct AppStoragePoller {
    stop_tx: Sender<()>,
    join_handle: Option<JoinHandle<()>>,
}

impl AppStoragePoller {
    pub fn spawn(serial: &str) -> Result<(Receiver<AppStorageUpdate>, Self), AdbError> {
        let (update_tx, update_rx) = crossbeam_channel::unbounded();
        let (stop_tx, stop_rx) = crossbeam_channel::unbounded();
        let serial = serial.to_string();

        let join_handle = thread::spawn(move || {
            while stop_rx.try_recv().is_err() {
                if run_scan(&serial, &update_tx, &stop_rx).is_err() {
                    break;
                }
                sleep_until_stop(&stop_rx, REFRESH_INTERVAL);
            }
        });

        Ok((
            update_rx,
            AppStoragePoller {
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

impl Drop for AppStoragePoller {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn run_scan(
    serial: &str,
    update_tx: &Sender<AppStorageUpdate>,
    stop_rx: &Receiver<()>,
) -> Result<(), ()> {
    let packages = match fetch_package_list(serial) {
        Ok(packages) => packages,
        Err(err) => {
            if update_tx
                .send(AppStorageUpdate::Error(err.user_message()))
                .is_err()
            {
                return Err(());
            }
            return Ok(());
        }
    };

    if update_tx
        .send(AppStorageUpdate::PackageList(packages.clone()))
        .is_err()
    {
        return Err(());
    }

    for chunk in packages.chunks(BATCH_SIZE) {
        if stop_rx.try_recv().is_ok() {
            return Err(());
        }

        let batch = match fetch_package_storage_batch(serial, chunk) {
            Ok(batch) => batch,
            Err(err) => {
                if update_tx
                    .send(AppStorageUpdate::Error(err.user_message()))
                    .is_err()
                {
                    return Err(());
                }
                continue;
            }
        };

        for (package, total_bytes) in batch {
            if update_tx
                .send(AppStorageUpdate::PackageStorage(PackageStorage {
                    package,
                    total_bytes,
                }))
                .is_err()
            {
                return Err(());
            }
        }
    }

    if update_tx.send(AppStorageUpdate::ScanComplete).is_err() {
        return Err(());
    }

    Ok(())
}

fn fetch_package_list(serial: &str) -> Result<Vec<String>, AdbError> {
    let output = run_adb_for_serial(serial, &["shell", "pm", "list", "packages"])?;
    let mut packages: Vec<String> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| {
            line.strip_prefix("package:")
                .map(str::trim)
                .map(str::to_string)
        })
        .collect();
    packages.sort();
    Ok(packages)
}

fn fetch_package_storage_batch(
    serial: &str,
    packages: &[String],
) -> Result<Vec<(String, u64)>, AdbError> {
    if packages.is_empty() {
        return Ok(Vec::new());
    }

    let script = build_batch_script(packages);
    let output = run_adb_for_serial(serial, &["shell", "sh", "-c", &script])?;
    Ok(parse_batch_output(
        &String::from_utf8_lossy(&output.stdout),
        packages,
    ))
}

fn build_batch_script(packages: &[String]) -> String {
    packages
        .iter()
        .map(|package| {
            let quoted = shell_quote(package);
            format!(
                "pkg={quoted}; \
                 printf '@PKG@%s\\n' \"$pkg\"; \
                 cmd package get-package-storage-stats \"$pkg\" 2>/dev/null || true"
            )
        })
        .collect::<Vec<_>>()
        .join("; ")
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn parse_batch_output(text: &str, packages: &[String]) -> Vec<(String, u64)> {
    let mut results = Vec::new();
    let mut current_package = None;
    let mut current_body = String::new();

    for line in text.lines() {
        if let Some(caps) = PKG_MARKER.captures(line.trim()) {
            if let Some(package) = current_package.take() {
                results.push((package, parse_storage_stats_total(&current_body)));
                current_body.clear();
            }
            current_package = Some(caps[1].to_string());
            continue;
        }

        if current_package.is_some() {
            if !current_body.is_empty() {
                current_body.push('\n');
            }
            current_body.push_str(line);
        }
    }

    if let Some(package) = current_package {
        results.push((package, parse_storage_stats_total(&current_body)));
    }

    fill_missing_packages(packages, results)
}

fn fill_missing_packages(
    expected: &[String],
    mut results: Vec<(String, u64)>,
) -> Vec<(String, u64)> {
    for package in expected {
        if !results.iter().any(|(name, _)| name == package) {
            results.push((package.clone(), 0));
        }
    }

    results.sort_by(|left, right| {
        expected
            .iter()
            .position(|pkg| pkg == &left.0)
            .unwrap_or(usize::MAX)
            .cmp(
                &expected
                    .iter()
                    .position(|pkg| pkg == &right.0)
                    .unwrap_or(usize::MAX),
            )
    });

    results
}

fn parse_storage_stats_total(text: &str) -> u64 {
    STORAGE_BYTES
        .captures_iter(text)
        .filter_map(|caps| caps.get(2)?.as_str().parse::<u64>().ok())
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_storage_stats_total_sums_bytes() {
        let sample = r#"code: 125628416 bytes (119.81 Mb)
data: 9515008 bytes (9.07 Mb)
cache: 184320 bytes (180.00 Kb)
apk: 70806309 bytes (67.53 Mb)
dexopt artifacts: 54725272 bytes (52.19 Mb)
"#;
        assert_eq!(
            parse_storage_stats_total(sample),
            125_628_416 + 9_515_008 + 184_320 + 70_806_309 + 54_725_272
        );
    }

    #[test]
    fn parse_package_list() {
        let sample = "package:com.android.chrome\npackage:com.google.android.gms\n";
        let list = sample
            .lines()
            .filter_map(|line| {
                line.strip_prefix("package:")
                    .map(str::trim)
                    .map(str::to_string)
            })
            .collect::<Vec<_>>();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0], "com.android.chrome");
    }

    #[test]
    fn parse_batch_output_splits_packages() {
        let sample = r#"@PKG@com.android.chrome
code: 1000 bytes
data: 2000 bytes
@PKG@com.android.settings
code: 500 bytes
"#;

        let expected = vec![
            "com.android.chrome".to_string(),
            "com.android.settings".to_string(),
        ];
        let parsed = parse_batch_output(sample, &expected);
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].0, "com.android.chrome");
        assert_eq!(parsed[0].1, 3000);
        assert_eq!(parsed[1].0, "com.android.settings");
        assert_eq!(parsed[1].1, 500);
    }

    #[test]
    fn shell_quote_escapes_single_quotes() {
        assert_eq!(shell_quote("com.example.app"), "'com.example.app'");
        assert_eq!(shell_quote("a'b"), "'a'\\''b'");
    }

    #[test]
    fn batch_script_uses_device_side_pkg_variable() {
        let script = build_batch_script(&["com.android.chrome".to_string()]);
        assert!(script.contains("pkg='com.android.chrome'"));
        assert!(script.contains("printf '@PKG@%s\\n' \"$pkg\""));
    }
}
