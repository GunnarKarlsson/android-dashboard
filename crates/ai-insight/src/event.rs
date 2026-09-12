use std::time::Instant;

use crate::fingerprint::redact;
use crate::severity::is_high_severity;

const MAX_FOLD_LINES: usize = 32;

/// One log line input to the reducer.
#[derive(Debug, Clone)]
pub struct InsightLine {
    pub received_at: Instant,
    pub level: char,
    pub tag: String,
    pub message: String,
    pub pid: u32,
}

/// One insight incident: a single error line, or a folded fatal/ANR stack.
#[derive(Debug, Clone)]
pub struct InsightEvent {
    pub received_at: Instant,
    pub level: char,
    pub tag: String,
    pub pid: u32,
    /// First line message; used for fingerprinting.
    pub headline: String,
    /// Redacted sample lines from the event (headline first).
    pub samples: Vec<String>,
}

/// Folds time-ordered lines into events. Every line is consumed.
///
/// Most lines become a one-line event. A fatal/ANR head folds following
/// continuation lines (same pid) into that event until a new head or limit.
pub fn fold_lines_to_events(lines: impl IntoIterator<Item = InsightLine>) -> Vec<InsightEvent> {
    let mut events = Vec::new();
    let mut builder: Option<LineStackBuilder> = None;

    for line in lines {
        if let Some(stack) = builder.as_mut() {
            if is_stack_continuation(stack, &line) {
                stack.lines.push(line);
                continue;
            }
            // Push the completed stack to the events and set builder to None
            events.push(builder.take().expect("is line stack builder").into_event());
        }

        if is_stack_head(&line) {
            builder = Some(LineStackBuilder::start(line));
        } else {
            events.push(InsightEvent::from_line(line));
        }
    }

    if let Some(stack) = builder {
        events.push(stack.into_event());
    }
    events
}

/// Returns true when this line starts a multi-line fatal/ANR event.
pub fn is_stack_head(line: &InsightLine) -> bool {
    is_stack_head_message(line.level, &line.message)
}

/// Returns true when level/message start a multi-line fatal/ANR event.
pub fn is_stack_head_message(level: char, message: &str) -> bool {
    if level == 'F' {
        return true;
    }
    let message = message.to_ascii_lowercase();
    message.contains("fatal exception")
        || message.contains("anr in")
        || message.contains("fatal signal")
}

fn is_stack_continuation(builder: &LineStackBuilder, line: &InsightLine) -> bool {
    if line.pid != builder.pid || is_stack_head(line) {
        return false;
    }
    if builder.lines.len() >= MAX_FOLD_LINES {
        return false;
    }

    let msg = line.message.trim_start();
    let lower = msg.to_ascii_lowercase();
    if lower.starts_with("at ")
        || lower.starts_with("caused by:")
        || lower.starts_with("process:")
        || lower.starts_with("pid:")
        || lower.starts_with("reason:")
        || msg.starts_with('\t')
    {
        return true;
    }

    match builder.kind {
        StackKind::Crash => matches!(line.tag.as_str(), "AndroidRuntime" | "DEBUG" | "libc"),
        StackKind::Anr => line.tag == "ActivityManager" || line.tag == builder.tag,
    }
}

#[derive(Clone, Copy)]
enum StackKind {
    Crash,
    Anr,
}

struct LineStackBuilder {
    kind: StackKind,
    pid: u32,
    tag: String,
    level: char,
    received_at: Instant,
    lines: Vec<InsightLine>,
}

impl LineStackBuilder {
    fn start(line: InsightLine) -> Self {
        let lower = line.message.to_ascii_lowercase();
        let kind = if lower.contains("anr in") {
            StackKind::Anr
        } else {
            StackKind::Crash
        };
        Self {
            kind,
            pid: line.pid,
            tag: line.tag.clone(),
            level: line.level,
            received_at: line.received_at,
            lines: vec![line],
        }
    }

    fn into_event(self) -> InsightEvent {
        InsightEvent::from_lines(self.tag, self.level, self.pid, self.received_at, self.lines)
    }
}

impl InsightEvent {
    fn from_line(line: InsightLine) -> Self {
        Self::from_lines(
            line.tag.clone(),
            line.level,
            line.pid,
            line.received_at,
            vec![line],
        )
    }

    fn from_lines(
        tag: String,
        level: char,
        pid: u32,
        received_at: Instant,
        lines: Vec<InsightLine>,
    ) -> Self {
        let headline = lines.first().map(|l| l.message.clone()).unwrap_or_default();
        // Keep every folded line (full stack / ANR dump); single-line events stay one sample.
        let samples: Vec<String> = lines.iter().map(|l| redact(&l.message)).collect();
        Self {
            received_at,
            level,
            tag,
            pid,
            headline,
            samples,
        }
    }

    /// Returns true when this event's headline is high severity.
    pub fn is_high_severity(&self) -> bool {
        is_high_severity(self.level, &self.tag, &self.headline)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(pid: u32, tag: &str, message: &str) -> InsightLine {
        InsightLine {
            received_at: Instant::now(),
            level: 'E',
            tag: tag.to_string(),
            message: message.to_string(),
            pid,
        }
    }

    #[test]
    fn single_error_is_one_event() {
        let events = fold_lines_to_events([line(1, "OkHttp", "boom")]);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].headline, "boom");
        assert_eq!(events[0].samples.len(), 1);
    }

    #[test]
    fn fatal_stack_folds_to_one_event() {
        let events = fold_lines_to_events([
            line(3178, "AndroidRuntime", "FATAL EXCEPTION: main"),
            line(
                3178,
                "AndroidRuntime",
                "Process: com.android.settings, PID: 3178",
            ),
            line(
                3178,
                "AndroidRuntime",
                "android.app.RemoteServiceException$CrashedByAdbException: shell-induced crash",
            ),
            line(
                3178,
                "AndroidRuntime",
                "at android.app.ActivityThread.throwRemoteServiceException(ActivityThread.java:2257)",
            ),
            line(99, "OkHttp", "other error"),
        ]);
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].headline, "FATAL EXCEPTION: main");
        assert_eq!(events[0].samples.len(), 4);
        assert!(
            events[0]
                .samples
                .iter()
                .any(|s| s.contains("shell-induced"))
        );
        assert_eq!(events[1].headline, "other error");
    }

    #[test]
    fn anr_head_folds_activity_manager_followups() {
        let events = fold_lines_to_events([
            line(
                14205,
                "ActivityManager",
                "ANR in com.example.myapp (com.example.myapp/.MainActivity)",
            ),
            line(14205, "ActivityManager", "PID: 14205"),
            line(
                14205,
                "ActivityManager",
                "Reason: Input dispatching timed out",
            ),
            line(1, "OkHttp", "unrelated"),
        ]);
        assert_eq!(events.len(), 2);
        assert!(events[0].headline.contains("ANR in"));
        assert_eq!(events[1].tag, "OkHttp");
    }

    #[test]
    fn different_pid_does_not_fold() {
        let events = fold_lines_to_events([
            line(1, "AndroidRuntime", "FATAL EXCEPTION: main"),
            line(2, "AndroidRuntime", "at other.Process.main"),
        ]);
        assert_eq!(events.len(), 2);
    }
}
