use std::thread::{self, JoinHandle};
use std::time::{Duration, SystemTime};

use crossbeam_channel::{Receiver, Sender};

use crate::adb::run_adb_for_serial;
use crate::background::{signal_stop_and_detach, sleep_until_stop};
use crate::error::AdbError;

const DEFAULT_POLL_INTERVAL: Duration = Duration::from_secs(2);
const MAX_BACKOFF_INTERVAL: Duration = Duration::from_secs(5);
const BACKOFF_STEP: Duration = Duration::from_secs(1);

/// Update from the stats poller — either a successful snapshot or a transient error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StatsUpdate {
    Stats(SystemStats),
    Error(String),
}

/// Memory statistics from `/proc/meminfo` (values in kB).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryStats {
    pub total_kb: u64,
    pub free_kb: u64,
    pub available_kb: u64,
    pub buffers_kb: u64,
    pub cached_kb: u64,
}

impl MemoryStats {
    pub fn used_kb(&self) -> u64 {
        self.total_kb.saturating_sub(self.available_kb)
    }

    pub fn used_fraction(&self) -> f32 {
        if self.total_kb == 0 {
            0.0
        } else {
            self.used_kb() as f32 / self.total_kb as f32
        }
    }
}

/// Snapshot of device memory usage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SystemStats {
    pub memory: MemoryStats,
    pub timestamp: SystemTime,
}

/// Background poller for memory and disk stats.
pub struct StatsPoller {
    stop_tx: Sender<()>,
    join_handle: Option<JoinHandle<()>>,
}

impl StatsPoller {
    /// Polls device stats every two seconds.
    pub fn spawn(serial: &str) -> Result<(Receiver<StatsUpdate>, Self), AdbError> {
        Self::spawn_with_interval(serial, DEFAULT_POLL_INTERVAL)
    }

    pub fn spawn_with_interval(
        serial: &str,
        interval: Duration,
    ) -> Result<(Receiver<StatsUpdate>, Self), AdbError> {
        let (stats_tx, stats_rx) = crossbeam_channel::unbounded();
        let (stop_tx, stop_rx) = crossbeam_channel::unbounded();
        let serial = serial.to_string();

        let join_handle = thread::spawn(move || {
            let mut poll_interval = interval;
            while stop_rx.try_recv().is_err() {
                match fetch_system_stats(&serial) {
                    Ok(stats) => {
                        poll_interval = interval;
                        if stats_tx.send(StatsUpdate::Stats(stats)).is_err() {
                            break;
                        }
                    }
                    Err(err) => {
                        poll_interval = next_backoff_interval(poll_interval);
                        if stats_tx
                            .send(StatsUpdate::Error(err.user_message()))
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
            stats_rx,
            StatsPoller {
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

impl Drop for StatsPoller {
    fn drop(&mut self) {
        self.shutdown();
    }
}

pub fn fetch_memory_stats(serial: &str) -> Result<MemoryStats, AdbError> {
    let meminfo = run_adb_for_serial(serial, &["shell", "cat", "/proc/meminfo"])?;
    parse_meminfo(&String::from_utf8_lossy(&meminfo.stdout))
}

pub(crate) fn fetch_system_stats(serial: &str) -> Result<SystemStats, AdbError> {
    let memory = fetch_memory_stats(serial)?;

    Ok(SystemStats {
        memory,
        timestamp: SystemTime::now(),
    })
}

fn parse_meminfo(text: &str) -> Result<MemoryStats, AdbError> {
    let mut total_kb = None;
    let mut free_kb = None;
    let mut available_kb = None;
    let mut buffers_kb = None;
    let mut cached_kb = None;

    for line in text.lines() {
        let (key, value) = match line.split_once(':') {
            Some(parts) => parts,
            None => continue,
        };

        let kb = parse_kb_value(value)?;
        match key.trim() {
            "MemTotal" => total_kb = Some(kb),
            "MemFree" => free_kb = Some(kb),
            "MemAvailable" => available_kb = Some(kb),
            "Buffers" => buffers_kb = Some(kb),
            "Cached" => cached_kb = Some(kb),
            _ => {}
        }
    }

    Ok(MemoryStats {
        total_kb: total_kb.ok_or_else(|| AdbError::ParseFailed("missing MemTotal".into()))?,
        free_kb: free_kb.unwrap_or(0),
        available_kb: available_kb.unwrap_or(0),
        buffers_kb: buffers_kb.unwrap_or(0),
        cached_kb: cached_kb.unwrap_or(0),
    })
}

fn parse_kb_value(raw: &str) -> Result<u64, AdbError> {
    let kb = raw
        .trim()
        .split_whitespace()
        .next()
        .ok_or_else(|| AdbError::ParseFailed(format!("invalid meminfo value: {raw}")))?;

    kb.parse()
        .map_err(|_| AdbError::ParseFailed(format!("invalid meminfo kB value: {raw}")))
}

fn next_backoff_interval(current: Duration) -> Duration {
    (current + BACKOFF_STEP).min(MAX_BACKOFF_INTERVAL)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_caps_at_five_seconds() {
        assert_eq!(
            next_backoff_interval(Duration::from_secs(2)),
            Duration::from_secs(3)
        );
        assert_eq!(
            next_backoff_interval(Duration::from_secs(4)),
            Duration::from_secs(5)
        );
        assert_eq!(
            next_backoff_interval(Duration::from_secs(5)),
            Duration::from_secs(5)
        );
    }

    #[test]
    fn parse_meminfo_sample() {
        let sample = r#"MemTotal:        8025332 kB
MemFree:          123456 kB
MemAvailable:    3456789 kB
Buffers:          111111 kB
Cached:           222222 kB
"#;

        let memory = parse_meminfo(sample).unwrap();
        assert_eq!(memory.total_kb, 8_025_332);
        assert_eq!(memory.free_kb, 123_456);
        assert_eq!(memory.available_kb, 3_456_789);
        assert_eq!(memory.buffers_kb, 111_111);
        assert_eq!(memory.cached_kb, 222_222);
        assert_eq!(memory.used_kb(), 8_025_332 - 3_456_789);
    }
}
