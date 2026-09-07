use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::sync::LazyLock;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use crossbeam_channel::{Receiver, Sender};
use regex::Regex;

use crate::background::signal_stop_and_detach;
use crate::error::AdbError;

const KILL_WAIT_ATTEMPTS: u32 = 10;
const KILL_WAIT_SLEEP: Duration = Duration::from_millis(20);
const MESSAGE_WRAP_WIDTH: usize = 120;
const PID_TID_FIELD_WIDTH: usize = 5;

static MULTIPLE_SPACES: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\s+").expect("valid multiple spaces regex"));

static LOGCAT_LINE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"^(\d{2}-\d{2} \d{2}:\d{2}:\d{2}\.\d{3})\s+(\d+)\s+(\d+)\s+([VDIWEF])\s+([^:]*): ?(.*)$",
    )
    .expect("valid logcat regex")
});

/// A single line from `adb logcat -v threadtime`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogEntry {
    pub timestamp: String,
    pub pid: u32,
    pub tid: u32,
    pub level: char,
    pub tag: String,
    pub message: String,
}

impl LogEntry {
    pub fn format_line(&self) -> String {
        self.format_line_with_timestamp(true)
    }

    pub fn format_line_with_timestamp(&self, show_timestamp: bool) -> String {
        let is_continuation = self.timestamp.is_empty() && self.tag.is_empty();

        if is_continuation {
            return self.message.clone();
        }

        let suffix = format!("{} {}: {}", self.level, self.tag, self.message);

        if show_timestamp {
            let body = format!(
                "{pid:>width$} {tid:>width$} {suffix}",
                pid = self.pid,
                tid = self.tid,
                suffix = suffix,
                width = PID_TID_FIELD_WIDTH,
            );

            if self.timestamp.is_empty() {
                body
            } else {
                format!("{} {}", self.timestamp, body)
            }
        } else {
            suffix
        }
    }

    /// Returns true for Error (`E`) and Fatal (`F`) log levels.
    pub fn is_error_level(&self) -> bool {
        matches!(self.level, 'E' | 'F')
    }

    /// Diagnostic message from adb/logcat (stderr, spawn failures, etc.).
    pub fn adb_diagnostic(message: impl Into<String>) -> Self {
        LogEntry {
            timestamp: String::new(),
            pid: 0,
            tid: 0,
            level: 'E',
            tag: "adb".to_string(),
            message: message.into(),
        }
    }

    fn raw(message: String) -> Self {
        Self::adb_diagnostic(message)
    }
}

/// Streaming handle for an `adb logcat` subprocess.
pub struct LogcatStream {
    stop_tx: Sender<()>,
    child: Option<Arc<std::sync::Mutex<Child>>>,
    join_handle: Option<JoinHandle<()>>,
}

impl LogcatStream {
    /// Spawns `adb -s <serial> logcat -v threadtime` and returns a receiver of parsed entries.
    pub fn spawn(serial: &str) -> Result<(Receiver<LogEntry>, Self), AdbError> {
        Self::spawn_with_filters(serial, Vec::new())
    }

    /// Spawns a separate logcat process filtered to Error and Fatal (`*:E`).
    pub fn spawn_errors(serial: &str) -> Result<(Receiver<LogEntry>, Self), AdbError> {
        Self::spawn_with_filters(serial, vec!["*:E".to_string()])
    }

    fn spawn_with_filters(
        serial: &str,
        filters: Vec<String>,
    ) -> Result<(Receiver<LogEntry>, Self), AdbError> {
        let (entry_tx, entry_rx) = crossbeam_channel::unbounded();
        let (stop_tx, stop_rx) = crossbeam_channel::unbounded();
        let serial = serial.to_string();

        let child = match spawn_logcat_child(&serial, &filters) {
            Ok(child) => child,
            Err(err) => {
                let _ = entry_tx.send(LogEntry::raw(format!("logcat error: {err}")));
                return Ok((
                    entry_rx,
                    LogcatStream {
                        stop_tx,
                        child: None,
                        join_handle: None,
                    },
                ));
            }
        };

        let child = Arc::new(std::sync::Mutex::new(child));
        let reader_child = child.clone();
        let join_handle = thread::spawn(move || {
            stream_logcat(reader_child.clone(), entry_tx, stop_rx);
            kill_child(&reader_child);
        });

        Ok((
            entry_rx,
            LogcatStream {
                stop_tx,
                child: Some(child),
                join_handle: Some(join_handle),
            },
        ))
    }

    /// Stops the background logcat stream and waits for the reader thread to exit.
    pub fn stop(mut self) {
        self.shutdown();
    }

    fn shutdown(&mut self) {
        let _ = self.stop_tx.send(());
        if let Some(child) = self.child.take() {
            kill_child(&child);
        }
        signal_stop_and_detach(&self.stop_tx, &mut self.join_handle);
    }
}

impl Drop for LogcatStream {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn spawn_logcat_child(serial: &str, filters: &[String]) -> Result<Child, AdbError> {
    Command::new("adb")
        .args(["-s", serial, "logcat", "-v", "threadtime"])
        .args(filters)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|err| {
            if err.kind() == std::io::ErrorKind::NotFound {
                AdbError::NotFound
            } else {
                AdbError::Io(err)
            }
        })
}

fn stream_logcat(
    child: Arc<std::sync::Mutex<Child>>,
    entry_tx: Sender<LogEntry>,
    stop_rx: Receiver<()>,
) {
    let stderr_tx = entry_tx.clone();
    let stderr_child = child.clone();
    thread::spawn(move || read_logcat_stderr(stderr_child, stderr_tx));

    let stdout = {
        let mut guard = match child.lock() {
            Ok(guard) => guard,
            Err(_) => return,
        };
        match guard.stdout.take() {
            Some(stdout) => stdout,
            None => return,
        }
    };

    let mut reader = BufReader::new(stdout);
    let mut line_buffer = Vec::new();
    loop {
        if stop_rx.try_recv().is_ok() {
            break;
        }

        let line = match read_line_lossy(&mut reader, &mut line_buffer) {
            Ok(Some(line)) => line,
            Ok(None) => break,
            Err(err) => {
                let _ = entry_tx.send(LogEntry::raw(format!("logcat read error: {err}")));
                break;
            }
        };

        for entry in parse_logcat_line(&line) {
            if entry_tx.send(entry).is_err() {
                break;
            }
        }

        if stop_rx.try_recv().is_ok() {
            break;
        }
    }

    report_logcat_exit(&child, &entry_tx);
}

fn read_logcat_stderr(child: Arc<std::sync::Mutex<Child>>, entry_tx: Sender<LogEntry>) {
    let stderr = {
        let mut guard = match child.lock() {
            Ok(guard) => guard,
            Err(_) => return,
        };
        guard.stderr.take()
    };

    let Some(stderr) = stderr else {
        return;
    };

    let mut reader = BufReader::new(stderr);
    let mut line_buffer = Vec::new();
    loop {
        match read_line_lossy(&mut reader, &mut line_buffer) {
            Ok(Some(line)) if !line.trim().is_empty() => {
                let _ = entry_tx.send(LogEntry::adb_diagnostic(line));
            }
            Ok(Some(_)) => {}
            Ok(None) => break,
            Err(err) => {
                let _ = entry_tx.send(LogEntry::adb_diagnostic(format!(
                    "logcat stderr read error: {err}"
                )));
                break;
            }
        }
    }
}

/// Read one line as UTF-8 lossy text. Unlike [`BufRead::lines`], invalid bytes do not
/// terminate the stream.
fn read_line_lossy(
    reader: &mut impl BufRead,
    buffer: &mut Vec<u8>,
) -> std::io::Result<Option<String>> {
    buffer.clear();
    let bytes_read = reader.read_until(b'\n', buffer)?;
    if bytes_read == 0 {
        return Ok(None);
    }

    while matches!(buffer.last(), Some(b'\n') | Some(b'\r')) {
        buffer.pop();
    }

    Ok(Some(String::from_utf8_lossy(buffer).into_owned()))
}

fn report_logcat_exit(child: &Arc<std::sync::Mutex<Child>>, entry_tx: &Sender<LogEntry>) {
    if let Ok(mut guard) = child.lock() {
        if let Ok(status) = guard.wait() {
            if !status.success() {
                let _ = entry_tx.send(LogEntry::adb_diagnostic(format!("logcat exited: {status}")));
            }
        }
    }
}

fn kill_child(child: &Arc<std::sync::Mutex<Child>>) {
    if let Ok(mut guard) = child.lock() {
        let _ = guard.kill();
        for _ in 0..KILL_WAIT_ATTEMPTS {
            if guard.try_wait().ok().flatten().is_some() {
                return;
            }
            thread::sleep(KILL_WAIT_SLEEP);
        }
        let _ = guard.wait();
    }
}

fn parse_logcat_line(line: &str) -> Vec<LogEntry> {
    let Some(captures) = LOGCAT_LINE.captures(line) else {
        return Vec::new();
    };

    let Some(pid) = captures.get(2).and_then(|m| m.as_str().parse().ok()) else {
        return Vec::new();
    };

    let Some(tid) = captures.get(3).and_then(|m| m.as_str().parse().ok()) else {
        return Vec::new();
    };

    let message = MULTIPLE_SPACES
        .replace_all(
            &captures.get(6).map(|m| m.as_str()).unwrap_or_default(),
            " ",
        )
        .trim()
        .to_string();

    let timestamp = captures
        .get(1)
        .map(|m| m.as_str().to_string())
        .unwrap_or_default();
    let level = captures
        .get(4)
        .and_then(|m| m.as_str().chars().next())
        .unwrap_or('I');
    let tag = captures
        .get(5)
        .map(|m| m.as_str().to_string())
        .unwrap_or_default();

    let mut entries = Vec::new();

    if message.is_empty() {
        entries.push(LogEntry {
            timestamp,
            pid,
            tid,
            level,
            tag,
            message: String::new(),
        });
        return entries;
    }

    let mut start = 0;
    while start < message.len() {
        // Calculate maximum chunk size remaining
        let max_len = std::cmp::min(MESSAGE_WRAP_WIDTH, message.len() - start);

        // Find a safe UTF-8 character boundary. We search backwards from the max_len
        // offset to avoid splitting a multi-byte character.
        let mut chunk_len = max_len;
        while !message.is_char_boundary(start + chunk_len) {
            chunk_len -= 1;
        }

        // In the extremely unlikely event a single character is wider than MESSAGE_WRAP_WIDTH, handle it
        if chunk_len == 0 {
            chunk_len = message[start..].chars().next().unwrap().len_utf8();
        }

        let chunk = &message[start..start + chunk_len];

        entries.push(LogEntry {
            timestamp: if start == 0 {
                timestamp.clone()
            } else {
                String::new()
            },
            pid,
            tid,
            level,
            tag: if start == 0 {
                tag.clone()
            } else {
                String::new()
            },
            message: chunk.to_string(),
        });

        start += chunk_len;
    }

    entries
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adb_diagnostic_is_error_level() {
        let entry = LogEntry::adb_diagnostic("device offline");
        assert!(entry.is_error_level());
        assert_eq!(entry.tag, "adb");
    }

    #[test]
    fn parse_standard_logcat_line() {
        let entry =
            parse_logcat_line("03-15 10:23:45.123  1234  5678 I MyTag: Hello world").unwrap();

        assert_eq!(entry.timestamp, "03-15 10:23:45.123");
        assert_eq!(entry.pid, 1234);
        assert_eq!(entry.tid, 5678);
        assert_eq!(entry.level, 'I');
        assert_eq!(entry.tag, "MyTag");
        assert_eq!(entry.message, "Hello world");
    }

    #[test]
    fn parse_error_logcat_line() {
        let entry =
            parse_logcat_line("09-01 17:00:01.456  9999  9999 E AndroidRuntime: FATAL EXCEPTION")
                .unwrap();

        assert_eq!(entry.level, 'E');
        assert_eq!(entry.tag, "AndroidRuntime");
        assert_eq!(entry.message, "FATAL EXCEPTION");
    }

    #[test]
    fn parse_line_without_message() {
        let entry = parse_logcat_line("09-01 17:00:01.456  100  100 W Tag:").unwrap();

        assert_eq!(entry.level, 'W');
        assert_eq!(entry.tag, "Tag");
        assert_eq!(entry.message, "");
    }

    #[test]
    fn compact_line_omits_timestamp_pid_and_tid() {
        let info = parse_logcat_line(
            "09-02 11:05:45.782  1959  1959 I artd    : GetBestInfo checking vdex next to the dex file (/data/user_de/0/com.google.android.gms/app_chimera/m/000000b4/oat/arm64/dl-Appsearch.optional_261631100400.vdex)",
        )
        .unwrap();
        assert_eq!(
            info.format_line_with_timestamp(false),
            "I artd    : GetBestInfo checking vdex next to the dex file (/data/user_de/0/com.google.android.gms/app_chimera/m/000000b4/oat/arm64/dl-Appsearch.optional_261631100400.vdex)",
        );

        let error = parse_logcat_line(
            "09-02 11:05:45.782  5353 25353 E AndroidRuntime: \tat kotlinx.coroutines.DispatchedTask.run(DispatchedTask.kt:100)",
        )
        .unwrap();
        assert_eq!(
            error.format_line_with_timestamp(false),
            "E AndroidRuntime: \tat kotlinx.coroutines.DispatchedTask.run(DispatchedTask.kt:100)",
        );
    }

    #[test]
    fn skip_unrecognized_line() {
        assert!(parse_logcat_line("--------- beginning of main").is_none());
    }

    #[test]
    fn error_level_filter() {
        let error = parse_logcat_line("09-01 17:00:01.456  1  1 E Tag: boom").unwrap();
        let fatal = parse_logcat_line("09-01 17:00:01.456  1  1 F Tag: boom").unwrap();
        let warning = parse_logcat_line("09-01 17:00:01.456  1  1 W Tag: boom").unwrap();

        assert!(error.is_error_level());
        assert!(fatal.is_error_level());
        assert!(!warning.is_error_level());
    }

    #[test]
    fn read_line_lossy_replaces_invalid_utf8() {
        let mut reader = BufReader::new(&b"hello \xFF world\n"[..]);
        let mut buffer = Vec::new();

        let line = read_line_lossy(&mut reader, &mut buffer).unwrap().unwrap();
        assert!(line.contains("hello"));
        assert!(line.contains("world"));
        assert!(line.contains('\u{FFFD}'));
        assert!(read_line_lossy(&mut reader, &mut buffer).unwrap().is_none());
    }
}
