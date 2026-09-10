use std::thread::{self, JoinHandle};
use std::time::Duration;

use crossbeam_channel::{Receiver, Sender};

use crate::background::{signal_stop_and_detach, sleep_until_stop};
use crate::error::AdbError;
use crate::stats::{MemoryStats, fetch_memory_stats};

const DEFAULT_POLL_INTERVAL: Duration = Duration::from_secs(2);
const MAX_BACKOFF_INTERVAL: Duration = Duration::from_secs(5);
const BACKOFF_STEP: Duration = Duration::from_secs(1);

/// Update from the RAM poller — memory snapshot or transient error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RamUpdate {
    Memory(MemoryStats),
    Error(String),
}

/// Background poller for RAM usage (`/proc/meminfo`).
pub struct RamPoller {
    stop_tx: Sender<()>,
    join_handle: Option<JoinHandle<()>>,
}

impl RamPoller {
    pub fn spawn(serial: &str) -> Result<(Receiver<RamUpdate>, Self), AdbError> {
        Self::spawn_with_interval(serial, DEFAULT_POLL_INTERVAL)
    }

    pub fn spawn_with_interval(
        serial: &str,
        interval: Duration,
    ) -> Result<(Receiver<RamUpdate>, Self), AdbError> {
        let (ram_tx, ram_rx) = crossbeam_channel::unbounded();
        let (stop_tx, stop_rx) = crossbeam_channel::unbounded();
        let serial = serial.to_string();

        let join_handle = thread::spawn(move || {
            let mut poll_interval = interval;
            while stop_rx.try_recv().is_err() {
                match fetch_memory_stats(&serial) {
                    Ok(memory) => {
                        poll_interval = interval;
                        if ram_tx.send(RamUpdate::Memory(memory)).is_err() {
                            break;
                        }
                    }
                    Err(err) => {
                        poll_interval = next_backoff_interval(poll_interval);
                        if ram_tx.send(RamUpdate::Error(err.user_message())).is_err() {
                            break;
                        }
                    }
                }

                sleep_until_stop(&stop_rx, poll_interval);
            }
        });

        Ok((
            ram_rx,
            RamPoller {
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

impl Drop for RamPoller {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn next_backoff_interval(current: Duration) -> Duration {
    (current + BACKOFF_STEP).min(MAX_BACKOFF_INTERVAL)
}
