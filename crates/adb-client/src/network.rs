use std::collections::HashMap;
use std::sync::LazyLock;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime};

use crossbeam_channel::{Receiver, Sender};
use regex::Regex;

use crate::adb::run_adb_for_serial;
use crate::background::{signal_stop_and_detach, sleep_until_stop};
use crate::error::AdbError;

const DEFAULT_POLL_INTERVAL: Duration = Duration::from_secs(1);
const TRANSPORT_REFRESH_INTERVAL: Duration = Duration::from_secs(10);
const MAX_BACKOFF_INTERVAL: Duration = Duration::from_secs(5);
const BACKOFF_STEP: Duration = Duration::from_secs(1);

const VIRTUAL_IFACE_PREFIXES: &[&str] = &[
    "dummy", "ifb", "tunl", "sit", "ip6tnl", "gre", "ip6gre", "teql",
];

static NETSTATS_IFACE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"iface=(\S+)\s+ident=\[\{type=(\w+)").expect("valid netstats regex")
});

/// Per-interface network counters and live throughput.
#[derive(Debug, Clone, PartialEq)]
pub struct NetworkInterfaceStats {
    pub interface: String,
    pub transport: String,
    pub rx_bytes: u64,
    pub tx_bytes: u64,
    pub rx_rate_bps: f64,
    pub tx_rate_bps: f64,
}

/// Snapshot of device network activity.
#[derive(Debug, Clone, PartialEq)]
pub struct NetworkStats {
    pub interfaces: Vec<NetworkInterfaceStats>,
    pub timestamp: SystemTime,
    /// Device-wide RX rate from real transports (WiFi / Mobile / Ethernet).
    pub rx_rate_bps: f64,
    /// Device-wide TX rate from real transports (WiFi / Mobile / Ethernet).
    pub tx_rate_bps: f64,
}

/// Update from the network poller — either a successful snapshot or a transient error.
#[derive(Debug, Clone, PartialEq)]
pub enum NetworkUpdate {
    Stats(NetworkStats),
    Error(String),
}

/// Background poller for network stats.
pub struct NetworkPoller {
    stop_tx: Sender<()>,
    join_handle: Option<JoinHandle<()>>,
}

impl NetworkPoller {
    /// Polls `/proc/net/dev` every second.
    pub fn spawn(serial: &str) -> Result<(Receiver<NetworkUpdate>, Self), AdbError> {
        Self::spawn_with_interval(serial, DEFAULT_POLL_INTERVAL)
    }

    pub fn spawn_with_interval(
        serial: &str,
        interval: Duration,
    ) -> Result<(Receiver<NetworkUpdate>, Self), AdbError> {
        let (update_tx, update_rx) = crossbeam_channel::unbounded();
        let (stop_tx, stop_rx) = crossbeam_channel::unbounded();
        let serial = serial.to_string();

        let join_handle = thread::spawn(move || {
            let mut poll_interval = interval;
            let mut previous: Option<NetworkSnapshot> = None;
            let mut transports = TransportCache::default();

            while stop_rx.try_recv().is_err() {
                match fetch_network_stats(&serial, previous.as_ref(), &mut transports) {
                    Ok(stats) => {
                        poll_interval = interval;
                        previous = Some(NetworkSnapshot::from_stats(&stats));
                        if update_tx.send(NetworkUpdate::Stats(stats)).is_err() {
                            break;
                        }
                    }
                    Err(err) => {
                        poll_interval = next_backoff_interval(poll_interval);
                        if update_tx
                            .send(NetworkUpdate::Error(err.user_message()))
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
            NetworkPoller {
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

impl Drop for NetworkPoller {
    fn drop(&mut self) {
        self.shutdown();
    }
}

#[derive(Debug, Clone, Default)]
struct TransportCache {
    map: HashMap<String, String>,
    last_refresh: Option<Instant>,
}

#[derive(Debug, Clone)]
struct NetworkSnapshot {
    bytes: HashMap<String, (u64, u64)>,
    instant: Instant,
}

impl NetworkSnapshot {
    fn from_stats(stats: &NetworkStats) -> Self {
        let bytes = stats
            .interfaces
            .iter()
            .map(|iface| (iface.interface.clone(), (iface.rx_bytes, iface.tx_bytes)))
            .collect();
        NetworkSnapshot {
            bytes,
            instant: Instant::now(),
        }
    }
}

fn fetch_network_stats(
    serial: &str,
    previous: Option<&NetworkSnapshot>,
    transports: &mut TransportCache,
) -> Result<NetworkStats, AdbError> {
    let proc_net = run_adb_for_serial(serial, &["shell", "cat", "/proc/net/dev"])?;
    let raw_interfaces = parse_proc_net_dev(&String::from_utf8_lossy(&proc_net.stdout));

    refresh_transports(serial, &raw_interfaces, transports);

    let elapsed_secs = previous
        .map(|prev| prev.instant.elapsed().as_secs_f64())
        .unwrap_or(0.0);

    let interfaces: Vec<NetworkInterfaceStats> = raw_interfaces
        .into_iter()
        .filter(|(iface, _, _)| iface != "lo")
        .map(|(iface, rx_bytes, tx_bytes)| {
            let transport = transports
                .map
                .get(&iface)
                .cloned()
                .unwrap_or_else(|| infer_transport(&iface).to_string());

            let (rx_rate_bps, tx_rate_bps) = if let Some(prev) = previous {
                if let Some((prev_rx, prev_tx)) = prev.bytes.get(&iface) {
                    (
                        compute_rate(rx_bytes, *prev_rx, elapsed_secs).unwrap_or(0.0),
                        compute_rate(tx_bytes, *prev_tx, elapsed_secs).unwrap_or(0.0),
                    )
                } else {
                    (0.0, 0.0)
                }
            } else {
                (0.0, 0.0)
            };

            NetworkInterfaceStats {
                interface: iface,
                transport,
                rx_bytes,
                tx_bytes,
                rx_rate_bps,
                tx_rate_bps,
            }
        })
        .collect();

    let (rx_rate_bps, tx_rate_bps) = device_totals(&interfaces);

    Ok(NetworkStats {
        interfaces,
        timestamp: SystemTime::now(),
        rx_rate_bps,
        tx_rate_bps,
    })
}

fn refresh_transports(
    serial: &str,
    raw_interfaces: &[(String, u64, u64)],
    cache: &mut TransportCache,
) {
    let unknown_iface = raw_interfaces
        .iter()
        .any(|(iface, _, _)| iface != "lo" && !cache.map.contains_key(iface));
    let stale = cache
        .last_refresh
        .map(|t| t.elapsed() >= TRANSPORT_REFRESH_INTERVAL)
        .unwrap_or(true);

    if !unknown_iface && !stale {
        return;
    }

    if let Ok(output) = run_adb_for_serial(serial, &["shell", "dumpsys", "netstats", "detail"]) {
        cache
            .map
            .extend(parse_netstats_transports(&String::from_utf8_lossy(
                &output.stdout,
            )));
    }

    for (iface, _, _) in raw_interfaces {
        if iface == "lo" {
            continue;
        }
        cache
            .map
            .entry(iface.clone())
            .or_insert_with(|| infer_transport(iface).to_string());
    }
    cache.last_refresh = Some(Instant::now());
}

fn device_totals(interfaces: &[NetworkInterfaceStats]) -> (f64, f64) {
    interfaces
        .iter()
        .filter(|iface| counts_toward_device_total(iface))
        .fold((0.0, 0.0), |(rx, tx), iface| {
            (rx + iface.rx_rate_bps, tx + iface.tx_rate_bps)
        })
}

fn counts_toward_device_total(iface: &NetworkInterfaceStats) -> bool {
    matches!(iface.transport.as_str(), "WiFi" | "Mobile" | "Ethernet")
        && !is_virtual_interface(&iface.interface)
}

fn is_virtual_interface(iface: &str) -> bool {
    let lower = iface.to_lowercase();
    VIRTUAL_IFACE_PREFIXES
        .iter()
        .any(|prefix| lower.starts_with(prefix))
}

fn parse_proc_net_dev(text: &str) -> Vec<(String, u64, u64)> {
    text.lines()
        .skip(2)
        .filter_map(|line| {
            let (iface_part, rest) = line.split_once(':')?;
            let iface = iface_part.trim().to_string();
            if iface.is_empty() {
                return None;
            }

            let parts: Vec<&str> = rest.split_whitespace().collect();
            if parts.len() < 9 {
                return None;
            }

            let rx_bytes = parts[0].parse().ok()?;
            let tx_bytes = parts[8].parse().ok()?;
            Some((iface, rx_bytes, tx_bytes))
        })
        .collect()
}

fn parse_netstats_transports(text: &str) -> HashMap<String, String> {
    NETSTATS_IFACE
        .captures_iter(text)
        .filter_map(|caps| {
            let iface = caps.get(1)?.as_str().to_string();
            let transport = format_transport(caps.get(2)?.as_str());
            Some((iface, transport))
        })
        .collect()
}

fn format_transport(raw: &str) -> String {
    match raw {
        "WIFI" => "WiFi".to_string(),
        "MOBILE" => "Mobile".to_string(),
        "ETHERNET" => "Ethernet".to_string(),
        other => {
            let lower = other.to_lowercase();
            if let Some(first) = lower.chars().next() {
                first.to_uppercase().collect::<String>() + &lower[1..]
            } else {
                other.to_string()
            }
        }
    }
}

fn infer_transport(iface: &str) -> &'static str {
    let lower = iface.to_lowercase();
    if lower.starts_with("wlan") || lower.starts_with("wifi") {
        "WiFi"
    } else if lower.starts_with("rmnet")
        || lower.starts_with("ccmni")
        || lower.starts_with("pdp")
        || lower.starts_with("wwan")
    {
        "Mobile"
    } else if lower.starts_with("eth") {
        "Ethernet"
    } else {
        "Other"
    }
}

fn compute_rate(current: u64, previous: u64, elapsed_secs: f64) -> Option<f64> {
    if elapsed_secs <= 0.0 {
        return Some(0.0);
    }

    let delta = current.saturating_sub(previous) as f64;
    if current < previous {
        return None;
    }

    Some(delta / elapsed_secs)
}

fn next_backoff_interval(current: Duration) -> Duration {
    (current + BACKOFF_STEP).min(MAX_BACKOFF_INTERVAL)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PROC_NET_DEV_SAMPLE: &str = r#"Inter-|   Receive                                                |  Transmit
 face |bytes    packets errs drop fifo frame compressed multicast|bytes    packets errs drop fifo colls carrier compressed
    lo: 1234567    1000    0    0    0     0          0         0  1234567    1000    0    0    0     0       0          0
  wlan0: 10485760   5000    0    0    0     0          0         0  5242880   2500    0    0    0     0       0          0
 rmnet0: 2097152   1000    0    0    0     0          0         0  1048576    500    0    0    0     0       0          0
"#;

    const NETSTATS_SAMPLE: &str = r#"
Active interfaces:
  iface=wlan0 ident=[{type=WIFI, subType=COMBINED, networkId="..."}]
  iface=rmnet0 ident=[{type=MOBILE, subType=COMBINED, networkId="..."}]
"#;

    #[test]
    fn parse_proc_net_dev_sample() {
        let interfaces = parse_proc_net_dev(PROC_NET_DEV_SAMPLE);
        assert_eq!(interfaces.len(), 3);
        assert_eq!(interfaces[1], ("wlan0".to_string(), 10_485_760, 5_242_880));
        assert_eq!(interfaces[2], ("rmnet0".to_string(), 2_097_152, 1_048_576));
    }

    #[test]
    fn parse_netstats_transports_sample() {
        let transports = parse_netstats_transports(NETSTATS_SAMPLE);
        assert_eq!(transports.get("wlan0"), Some(&"WiFi".to_string()));
        assert_eq!(transports.get("rmnet0"), Some(&"Mobile".to_string()));
    }

    #[test]
    fn infer_transport_from_iface_name() {
        assert_eq!(infer_transport("wlan0"), "WiFi");
        assert_eq!(infer_transport("rmnet_data0"), "Mobile");
        assert_eq!(infer_transport("eth0"), "Ethernet");
        assert_eq!(infer_transport("dummy0"), "Other");
    }

    #[test]
    fn virtual_interface_denylist() {
        assert!(is_virtual_interface("dummy0"));
        assert!(is_virtual_interface("ifb0"));
        assert!(is_virtual_interface("tunl0"));
        assert!(is_virtual_interface("sit0"));
        assert!(is_virtual_interface("ip6tnl0"));
        assert!(!is_virtual_interface("wlan0"));
        assert!(!is_virtual_interface("eth0"));
        assert!(!is_virtual_interface("rmnet_data0"));
    }

    fn iface(name: &str, transport: &str, rx_rate: f64, tx_rate: f64) -> NetworkInterfaceStats {
        NetworkInterfaceStats {
            interface: name.to_string(),
            transport: transport.to_string(),
            rx_bytes: 0,
            tx_bytes: 0,
            rx_rate_bps: rx_rate,
            tx_rate_bps: tx_rate,
        }
    }

    #[test]
    fn device_totals_sum_real_transports() {
        let (rx, tx) = device_totals(&[
            iface("wlan0", "WiFi", 1000.0, 100.0),
            iface("rmnet0", "Mobile", 500.0, 50.0),
            iface("dummy0", "Other", 9999.0, 9999.0),
            iface("ifb0", "Other", 8888.0, 8888.0),
        ]);
        assert_eq!(rx, 1500.0);
        assert_eq!(tx, 150.0);
    }

    #[test]
    fn device_totals_exclude_virtual_even_if_labeled_ethernet() {
        let (rx, tx) = device_totals(&[iface("dummy0", "Ethernet", 100.0, 20.0)]);
        assert_eq!(rx, 0.0);
        assert_eq!(tx, 0.0);
    }

    #[test]
    fn compute_rate_handles_counter_reset() {
        assert_eq!(compute_rate(100, 200, 2.0), None);
        assert_eq!(compute_rate(300, 100, 2.0), Some(100.0));
    }

    #[test]
    fn backoff_caps_at_five_seconds() {
        assert_eq!(
            next_backoff_interval(Duration::from_secs(4)),
            Duration::from_secs(5)
        );
    }
}
