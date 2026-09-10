use std::thread::{self, JoinHandle};
use std::time::Duration;

use crossbeam_channel::{Receiver, Sender};

use crate::background::{signal_stop_and_detach, sleep_until_stop};
use crate::error::AdbError;
use crate::storage_breakdown::{StorageOverview, fetch_storage_overview};

const DEFAULT_POLL_INTERVAL: Duration = Duration::from_secs(10);
const MAX_BACKOFF_INTERVAL: Duration = Duration::from_secs(30);
const BACKOFF_STEP: Duration = Duration::from_secs(5);

/// Update from the storage gauge poller — overview snapshot or transient error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StorageGaugeUpdate {
    Overview(StorageOverview),
    Error(String),
}

/// Background poller for user storage totals (`df` on `/storage/emulated/0`).
pub struct StorageGaugePoller {
    stop_tx: Sender<()>,
    join_handle: Option<JoinHandle<()>>,
}

impl StorageGaugePoller {
    pub fn spawn(serial: &str) -> Result<(Receiver<StorageGaugeUpdate>, Self), AdbError> {
        Self::spawn_with_interval(serial, DEFAULT_POLL_INTERVAL)
    }

    pub fn spawn_with_interval(
        serial: &str,
        interval: Duration,
    ) -> Result<(Receiver<StorageGaugeUpdate>, Self), AdbError> {
        let (storage_tx, storage_rx) = crossbeam_channel::unbounded();
        let (stop_tx, stop_rx) = crossbeam_channel::unbounded();
        let serial = serial.to_string();

        let join_handle = thread::spawn(move || {
            let mut poll_interval = interval;
            while stop_rx.try_recv().is_err() {
                match fetch_storage_overview(&serial) {
                    Ok(overview) => {
                        poll_interval = interval;
                        if storage_tx
                            .send(StorageGaugeUpdate::Overview(overview))
                            .is_err()
                        {
                            break;
                        }
                    }
                    Err(err) => {
                        poll_interval = next_backoff_interval(poll_interval);
                        if storage_tx
                            .send(StorageGaugeUpdate::Error(err.user_message()))
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
            storage_rx,
            StorageGaugePoller {
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

impl Drop for StorageGaugePoller {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn next_backoff_interval(current: Duration) -> Duration {
    (current + BACKOFF_STEP).min(MAX_BACKOFF_INTERVAL)
}
