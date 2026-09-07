use std::collections::HashMap;
use std::sync::LazyLock;
use std::thread::{self, JoinHandle};
use std::time::{Duration, SystemTime};

use crossbeam_channel::{Receiver, Sender};
use regex::Regex;

use crate::adb::run_adb_for_serial;
use crate::background::{signal_stop_and_detach, sleep_until_stop};
use crate::error::AdbError;

const DEFAULT_POLL_INTERVAL: Duration = Duration::from_secs(2);
const MAX_BACKOFF_INTERVAL: Duration = Duration::from_secs(5);
const BACKOFF_STEP: Duration = Duration::from_secs(1);

static PACKAGE_UID: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^package:(\S+)\s+uid:(\d+)\s*$").expect("valid package uid regex")
});

static IDENT_LINE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"ident=\[\{type=(\d+),.*?}\]\s+uid=(\d+)\s+set=(\S+)\s+tag=(0x[0-9a-fA-F]+)")
        .expect("valid ident regex")
});

static HISTORY_LINE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\brb=(\d+)\b.*\btb=(\d+)\b").expect("valid history regex"));

static TOP_CALLER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\{uid=(\d+),package=([^}]+)\}").expect("valid top caller regex"));

/// Per-app traffic from `dumpsys netstats --uid` (`tag=0x0` rows).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppTraffic {
    pub uid: u32,
    pub package: String,
    pub total_bytes: u64,
    pub foreground_bytes: u64,
    pub background_bytes: u64,
    pub wifi_bytes: u64,
    pub mobile_bytes: u64,
}

/// Snapshot of per-app network usage since boot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProtocolStats {
    pub apps: Vec<AppTraffic>,
    pub timestamp: SystemTime,
}

/// Update from the protocol poller — either a successful snapshot or a transient error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProtocolUpdate {
    Stats(ProtocolStats),
    Error(String),
}

/// Background poller for per-app traffic stats.
pub struct ProtocolPoller {
    stop_tx: Sender<()>,
    join_handle: Option<JoinHandle<()>>,
}

impl ProtocolPoller {
    /// Polls app traffic stats every two seconds.
    pub fn spawn(serial: &str) -> Result<(Receiver<ProtocolUpdate>, Self), AdbError> {
        Self::spawn_with_interval(serial, DEFAULT_POLL_INTERVAL)
    }

    pub fn spawn_with_interval(
        serial: &str,
        interval: Duration,
    ) -> Result<(Receiver<ProtocolUpdate>, Self), AdbError> {
        let (update_tx, update_rx) = crossbeam_channel::unbounded();
        let (stop_tx, stop_rx) = crossbeam_channel::unbounded();
        let serial = serial.to_string();

        let join_handle = thread::spawn(move || {
            let mut poll_interval = interval;
            while stop_rx.try_recv().is_err() {
                match fetch_protocol_stats(&serial) {
                    Ok(stats) => {
                        poll_interval = interval;
                        if update_tx.send(ProtocolUpdate::Stats(stats)).is_err() {
                            break;
                        }
                    }
                    Err(err) => {
                        poll_interval = next_backoff_interval(poll_interval);
                        if update_tx
                            .send(ProtocolUpdate::Error(err.user_message()))
                            .is_err()
                        {
                            break;
                        }
                    }
                }

                sleep_until_stop(&stop_rx, poll_interval);
            }
        });

        Ok((
            update_rx,
            ProtocolPoller {
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

impl Drop for ProtocolPoller {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn fetch_protocol_stats(serial: &str) -> Result<ProtocolStats, AdbError> {
    let packages_out = run_adb_for_serial(serial, &["shell", "pm", "list", "packages", "-U"])?;
    let package_names = parse_package_uids(&String::from_utf8_lossy(&packages_out.stdout));

    let dump = run_adb_for_serial(serial, &["shell", "dumpsys", "netstats", "--uid"])?;
    let dump_text = String::from_utf8_lossy(&dump.stdout);
    let traffic = parse_uid_traffic(&dump_text);
    let top_callers = parse_top_callers(&dump_text);
    let apps = build_app_traffic(traffic, &package_names, &top_callers);

    Ok(ProtocolStats {
        apps,
        timestamp: SystemTime::now(),
    })
}

fn parse_package_uids(text: &str) -> HashMap<u32, Vec<String>> {
    let mut packages: HashMap<u32, Vec<String>> = HashMap::new();
    for line in text.lines() {
        let Some(caps) = PACKAGE_UID.captures(line.trim()) else {
            continue;
        };
        let name = caps[1].to_string();
        let Ok(uid) = caps[2].parse::<u32>() else {
            continue;
        };
        packages.entry(uid).or_default().push(name);
    }
    packages
}

#[derive(Default)]
struct UidTraffic {
    foreground_bytes: u64,
    background_bytes: u64,
    wifi_bytes: u64,
    mobile_bytes: u64,
}

fn parse_uid_traffic(text: &str) -> HashMap<u32, UidTraffic> {
    let mut traffic: HashMap<u32, UidTraffic> = HashMap::new();
    let mut in_uid_stats = false;
    let mut current_uid = None;
    let mut current_set = None;
    let mut current_network = None;
    let mut counting = false;

    for line in text.lines() {
        let trimmed = line.trim();

        match trimmed {
            "UID stats:" => {
                in_uid_stats = true;
                current_uid = None;
                counting = false;
                continue;
            }
            "UID tag stats:" => {
                in_uid_stats = false;
                counting = false;
                continue;
            }
            _ => {}
        }

        if !in_uid_stats {
            continue;
        }

        if trimmed.starts_with("ident=") {
            if let Some(caps) = IDENT_LINE.captures(trimmed) {
                let network_type: u32 = caps[1].parse().unwrap_or(u32::MAX);
                let uid: u32 = caps[2].parse().unwrap_or(0);
                let set = caps[3].to_string();
                let tag = &caps[4];
                counting = tag == "0x0" && !set.eq_ignore_ascii_case("ALL");
                if counting {
                    current_uid = Some(uid);
                    current_set = Some(set);
                    current_network = Some(network_type);
                } else {
                    current_uid = None;
                }
            } else {
                current_uid = None;
                counting = false;
            }
            continue;
        }

        if !counting {
            continue;
        }

        let Some(uid) = current_uid else {
            continue;
        };
        let Some(set) = current_set.as_deref() else {
            continue;
        };
        let Some(network_type) = current_network else {
            continue;
        };

        let Some(caps) = HISTORY_LINE.captures(trimmed) else {
            continue;
        };
        let rb: u64 = caps[1].parse().unwrap_or(0);
        let tb: u64 = caps[2].parse().unwrap_or(0);
        let bytes = rb.saturating_add(tb);

        let entry = traffic.entry(uid).or_default();
        match set.to_ascii_uppercase().as_str() {
            "FOREGROUND" => entry.foreground_bytes = entry.foreground_bytes.saturating_add(bytes),
            "DEFAULT" | "BACKGROUND" => {
                entry.background_bytes = entry.background_bytes.saturating_add(bytes)
            }
            _ => {}
        }
        match network_type {
            1 => entry.wifi_bytes = entry.wifi_bytes.saturating_add(bytes),
            0 => entry.mobile_bytes = entry.mobile_bytes.saturating_add(bytes),
            _ => {}
        }
    }

    traffic
}

fn parse_top_callers(text: &str) -> HashMap<u32, String> {
    let mut callers = HashMap::new();
    let mut in_section = false;

    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed == "Top openSession callers:" {
            in_section = true;
            continue;
        }
        if !in_section {
            continue;
        }
        if trimmed.is_empty() {
            continue;
        }
        if !line.starts_with(' ') && !trimmed.starts_with('{') {
            break;
        }
        for caps in TOP_CALLER.captures_iter(line) {
            let uid: u32 = caps[1].parse().unwrap_or(0);
            let package = caps[2].to_string();
            callers.entry(uid).or_insert(package);
        }
    }

    callers
}

fn is_overlay_package(name: &str) -> bool {
    let lower = name.to_lowercase();
    lower.contains("auto_generated") || lower.contains("_rro") || lower.contains(".overlay")
}

fn package_display_rank(name: &str) -> (u8, usize) {
    let lower = name.to_lowercase();
    let tier = if lower.ends_with(".gsf") {
        4
    } else if lower.contains("cellbroadcast") || lower.contains("tethering") {
        3
    } else if lower.contains("networkstack") {
        2
    } else if lower.contains("keychain")
        || lower.contains("inputdevices")
        || lower.contains("providers.")
        || lower.contains("localtransport")
        || lower.contains("dynsystem")
        || lower.contains("emulator.")
        || lower.contains("location.fused")
        || lower.contains("server.telecom")
        || lower.contains("deviceaswebcam")
    {
        5
    } else {
        0
    };
    (tier, name.len())
}

fn pick_display_package(packages: &[String]) -> Option<String> {
    if packages.is_empty() {
        return None;
    }
    if packages.len() == 1 {
        return Some(packages[0].clone());
    }

    let filtered: Vec<&String> = packages.iter().filter(|p| !is_overlay_package(p)).collect();
    let pool: Vec<&String> = if filtered.is_empty() {
        packages.iter().collect()
    } else {
        filtered
    };

    if let Some(android) = pool.iter().find(|p| p.as_str() == "android") {
        return Some((*android).clone());
    }

    pool.iter()
        .min_by_key(|p| package_display_rank(p))
        .map(|p| (*p).clone())
}

fn resolve_package_name(
    uid: u32,
    package_names: &HashMap<u32, Vec<String>>,
    top_callers: &HashMap<u32, String>,
) -> String {
    if let Some(name) = top_callers.get(&uid) {
        return name.clone();
    }
    if let Some(name) = pick_display_package(
        package_names
            .get(&uid)
            .map(|names| names.as_slice())
            .unwrap_or(&[]),
    ) {
        return name;
    }
    format!("uid:{uid}")
}

fn build_app_traffic(
    traffic: HashMap<u32, UidTraffic>,
    package_names: &HashMap<u32, Vec<String>>,
    top_callers: &HashMap<u32, String>,
) -> Vec<AppTraffic> {
    let mut apps: Vec<AppTraffic> = traffic
        .into_iter()
        .filter_map(|(uid, stats)| {
            let total_bytes = stats
                .foreground_bytes
                .saturating_add(stats.background_bytes);
            if total_bytes == 0 {
                return None;
            }

            Some(AppTraffic {
                uid,
                package: resolve_package_name(uid, package_names, top_callers),
                total_bytes,
                foreground_bytes: stats.foreground_bytes,
                background_bytes: stats.background_bytes,
                wifi_bytes: stats.wifi_bytes,
                mobile_bytes: stats.mobile_bytes,
            })
        })
        .collect();

    apps.sort_by(|a, b| b.total_bytes.cmp(&a.total_bytes));
    apps
}

fn next_backoff_interval(current: Duration) -> Duration {
    (current + BACKOFF_STEP).min(MAX_BACKOFF_INTERVAL)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PACKAGES_SAMPLE: &str = r#"package:com.google.android.youtube uid:10166
package:com.android.server.telecom uid:1000
package:android uid:1000
package:com.google.android.gsf uid:10144
package:com.google.android.gms uid:10144
"#;

    const TOP_CALLERS_SAMPLE: &str = r#"Top openSession callers:
  {uid=10144,package=com.google.android.gms}=3810
  {uid=1000,package=android}=1753

Poll counts per reason:
"#;

    const UID_STATS_SAMPLE: &str = r#"Xt stats:
  ident=[{type=1}] uid=-1 set=ALL tag=0x0
    NetworkStatsHistory: bucketDuration=3600
      st=1 rb=999999 rp=1 tb=999999 tp=1 op=0
UID stats:
  ident=[{type=1, ratType=COMBINED, wifiNetworkKey="AndroidWifi"open, metered=false, defaultNetwork=true, oemManaged=OEM_NONE, subId=-1}] uid=10144 set=FOREGROUND tag=0x0
    NetworkStatsHistory: bucketDuration=7200
      st=1 rb=1000 rp=1 tb=500 tp=1 op=0
      st=2 rb=200 rp=1 tb=100 tp=1 op=0
  ident=[{type=1, ratType=COMBINED, wifiNetworkKey="AndroidWifi"open, metered=false, defaultNetwork=true, oemManaged=OEM_NONE, subId=-1}] uid=10144 set=DEFAULT tag=0x0
    NetworkStatsHistory: bucketDuration=7200
      st=1 rb=300 rp=1 tb=200 tp=1 op=0
  ident=[{type=0, ratType=3, subscriberId=310260..., metered=true, defaultNetwork=true, oemManaged=OEM_NONE, subId=1}] uid=10144 set=FOREGROUND tag=0x0
    NetworkStatsHistory: bucketDuration=7200
      st=1 rb=50 rp=1 tb=50 tp=1 op=0
  ident=[{type=1, ratType=COMBINED, wifiNetworkKey="AndroidWifi"open, metered=false, defaultNetwork=true, oemManaged=OEM_NONE, subId=-1}] uid=10166 set=DEFAULT tag=0x0
    NetworkStatsHistory: bucketDuration=7200
      st=1 rb=400 rp=1 tb=100 tp=1 op=0
  ident=[{type=1, ratType=COMBINED, wifiNetworkKey="AndroidWifi"open, metered=false, defaultNetwork=true, oemManaged=OEM_NONE, subId=-1}] uid=10144 set=ALL tag=0x0
    NetworkStatsHistory: bucketDuration=7200
      st=1 rb=999999 rp=1 tb=999999 tp=1 op=0
UID tag stats:
  ident=[{type=1, ratType=COMBINED, wifiNetworkKey="AndroidWifi"open, metered=false, defaultNetwork=true, oemManaged=OEM_NONE, subId=-1}] uid=10144 set=FOREGROUND tag=0xffffff82
    NetworkStatsHistory: bucketDuration=7200
      st=1 rb=888888 rp=1 tb=888888 tp=1 op=0
"#;

    #[test]
    fn parse_package_list() {
        let packages = parse_package_uids(PACKAGES_SAMPLE);
        assert_eq!(
            packages.get(&10166),
            Some(&vec!["com.google.android.youtube".to_string()])
        );
        assert_eq!(packages.get(&1000).map(|names| names.len()), Some(2));
    }

    #[test]
    fn parse_uid_stats_traffic() {
        let traffic = parse_uid_traffic(UID_STATS_SAMPLE);

        let gms = traffic.get(&10144).expect("uid 10144");
        assert_eq!(gms.foreground_bytes, 1000 + 500 + 200 + 100 + 50 + 50);
        assert_eq!(gms.background_bytes, 300 + 200);
        assert_eq!(gms.wifi_bytes, 1000 + 500 + 200 + 100 + 300 + 200);
        assert_eq!(gms.mobile_bytes, 50 + 50);

        let youtube = traffic.get(&10166).expect("uid 10166");
        assert_eq!(youtube.background_bytes, 500);
        assert_eq!(youtube.wifi_bytes, 500);
    }

    #[test]
    fn parse_top_callers_from_dump() {
        let callers = parse_top_callers(TOP_CALLERS_SAMPLE);
        assert_eq!(
            callers.get(&10144),
            Some(&"com.google.android.gms".to_string())
        );
        assert_eq!(callers.get(&1000), Some(&"android".to_string()));
    }

    #[test]
    fn pick_display_package_prefers_primary_app() {
        let packages = vec![
            "com.google.android.gsf".to_string(),
            "com.google.android.gms".to_string(),
        ];
        assert_eq!(
            pick_display_package(&packages),
            Some("com.google.android.gms".to_string())
        );

        let system = vec![
            "com.android.server.telecom".to_string(),
            "android".to_string(),
        ];
        assert_eq!(pick_display_package(&system), Some("android".to_string()));
    }

    #[test]
    fn build_app_traffic_sorted_by_total() {
        let traffic = parse_uid_traffic(UID_STATS_SAMPLE);
        let packages = parse_package_uids(PACKAGES_SAMPLE);
        let top_callers = parse_top_callers(TOP_CALLERS_SAMPLE);
        let apps = build_app_traffic(traffic, &packages, &top_callers);

        assert_eq!(apps.len(), 2);
        assert!(apps[0].total_bytes >= apps[1].total_bytes);
        assert_eq!(apps[0].package, "com.google.android.gms");
        assert_eq!(apps[1].package, "com.google.android.youtube");
    }

    #[test]
    fn ignores_uid_tag_stats_section() {
        let traffic = parse_uid_traffic(UID_STATS_SAMPLE);
        let total: u64 = traffic
            .values()
            .map(|s| s.foreground_bytes + s.background_bytes)
            .sum();
        assert!(total < 1_000_000);
    }
}
