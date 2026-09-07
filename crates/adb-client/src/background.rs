use std::thread::{self, JoinHandle};
use std::time::Duration;

use crossbeam_channel::{Receiver, Sender};

const STOP_POLL_STEP: Duration = Duration::from_millis(100);

/// Signal a background worker to stop without blocking on `join`.
///
/// Dropping the join handle detaches the thread so app shutdown is not stalled
/// by in-flight adb commands.
pub(crate) fn signal_stop_and_detach(
    stop_tx: &Sender<()>,
    join_handle: &mut Option<JoinHandle<()>>,
) {
    let _ = stop_tx.send(());
    let _ = join_handle.take();
}

/// Sleeps `duration`, returning early when `stop_rx` receives.
pub(crate) fn sleep_until_stop(stop_rx: &Receiver<()>, duration: Duration) {
    let mut elapsed = Duration::ZERO;

    while elapsed < duration {
        if stop_rx.try_recv().is_ok() {
            return;
        }
        thread::sleep(STOP_POLL_STEP);
        elapsed += STOP_POLL_STEP;
    }
}
