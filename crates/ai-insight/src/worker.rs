use std::thread;

use crossbeam_channel::{Receiver, Sender};

use crate::client::{InsightError, complete};
use crate::config::InsightConfig;
use crate::reduce::InsightSnapshot;

/// Result of one background Chat Completions call.
#[derive(Debug, Clone)]
pub enum InsightUpdate {
    Started {
        generation: u64,
        serial: String,
    },
    Reply {
        generation: u64,
        serial: String,
        text: String,
    },
    Error {
        generation: u64,
        serial: String,
        message: String,
    },
}

/// Starts a thread that POSTs `snapshot` and sends [`InsightUpdate`] values on a channel.
///
/// Does nothing when `config` is not configured: no thread, no HTTP.
///
/// - `generation` — caller sequence id; echoed on every update
/// - `serial` — adb serial the snapshot was built for; echoed on every update
pub fn spawn_insight(
    config: InsightConfig,
    snapshot: InsightSnapshot,
    generation: u64,
    serial: String,
) -> Receiver<InsightUpdate> {
    let (tx, rx) = crossbeam_channel::unbounded();
    if !config.is_configured() {
        tracing::warn!("insight request skipped: AI provider is not configured");
        return rx;
    }
    thread::spawn(move || run_insight(tx, config, snapshot, generation, serial));
    rx
}

fn run_insight(
    tx: Sender<InsightUpdate>,
    config: InsightConfig,
    snapshot: InsightSnapshot,
    generation: u64,
    serial: String,
) {
    let _ = tx.send(InsightUpdate::Started {
        generation,
        serial: serial.clone(),
    });

    match complete(&config, &snapshot, generation) {
        Ok(text) => {
            let _ = tx.send(InsightUpdate::Reply {
                generation,
                serial,
                text,
            });
        }
        Err(err) => {
            let _ = tx.send(InsightUpdate::Error {
                generation,
                serial,
                message: user_facing_error(&err),
            });
        }
    }
}

/// Builds a short panel message for a failed insight request.
fn user_facing_error(err: &InsightError) -> String {
    match err {
        InsightError::Http { status, .. } => format!(
            "The app received a {status} error from the ai provider. Check your configuration in the settings panel"
        ),
        InsightError::NotConfigured => {
            "AI provider base URL, model, and API key must be set. Check your configuration in the settings panel"
                .to_string()
        }
        InsightError::EmptyReply => {
            "The ai provider returned an empty reply. Check your configuration in the settings panel"
                .to_string()
        }
        InsightError::Transport(_) => {
            "The app could not reach the ai provider. Check your configuration in the settings panel"
                .to_string()
        }
    }
}
