use std::collections::VecDeque;
use std::time::Instant;

use adb_client::LogEntry;
use ai_insight::InsightLine;

use crate::ui_elements;

pub const MAX_LOG_LINES: usize = 10_000;

#[derive(Clone, Debug)]
pub struct LogcatTagFilter {
    pub tag: String,
    pub color_index: usize,
}

/// Visible ring buffer, pending ring buffer, and filter state for one logcat pane.
pub struct LogcatPane {
    pub lines: VecDeque<CachedLogLine>,
    pub pending: VecDeque<CachedLogLine>,
    pub tag_input: String,
    pub tag_filters: Vec<LogcatTagFilter>,
    pub auto_update_feed: bool,
    pub show_timestamps: bool,
    pub error: Option<String>,
    pub accept_errors_only: bool,
}

impl Default for LogcatPane {
    fn default() -> Self {
        Self {
            lines: VecDeque::new(),
            pending: VecDeque::new(),
            tag_input: String::new(),
            tag_filters: Vec::new(),
            auto_update_feed: true,
            show_timestamps: true,
            error: None,
            accept_errors_only: false,
        }
    }
}

impl LogcatPane {
    /// Clears lines, pending, tags, and error, and turns auto-update on.
    /// Leaves `accept_errors_only` and `show_timestamps` unchanged.
    pub(crate) fn reset_view(&mut self) {
        self.lines.clear();
        self.pending.clear();
        self.tag_input.clear();
        self.tag_filters.clear();
        self.auto_update_feed = true;
        self.error = None;
    }

    /// Appends `entry` to the visible ring buffer when auto-update is on, or to the pending
    /// ring buffer when it is off.
    ///
    /// Skips the entry when `accept_errors_only` is set and the entry is not Error or Fatal.
    /// Returns true when the visible ring buffer changed.
    pub(crate) fn append_entry(&mut self, entry: &LogEntry) -> bool {
        if self.accept_errors_only && !entry.is_error_level() {
            return false;
        }

        if self.auto_update_feed {
            self.flush_pending();
            self.lines.push_back(CachedLogLine::from_entry(entry));
            trim_buffer(&mut self.lines);
            true
        } else {
            self.pending.push_back(CachedLogLine::from_entry(entry));
            trim_buffer(&mut self.pending);
            false
        }
    }

    /// Moves pending lines onto the visible ring buffer. Returns true if any line was moved.
    pub(crate) fn flush_pending(&mut self) -> bool {
        if self.pending.is_empty() {
            return false;
        }

        self.lines.extend(self.pending.drain(..));
        trim_buffer(&mut self.lines);
        true
    }

    /// Returns log lines from the visible ring buffer followed by the pending ring buffer.
    pub(crate) fn insight_lines(&self) -> impl Iterator<Item = InsightLine> + '_ {
        self.lines
            .iter()
            .chain(self.pending.iter())
            .map(CachedLogLine::to_insight_line)
    }

    /// Adds a trimmed tag filter if it is non-empty and not already present (case-insensitive).
    pub fn add_tag(&mut self) {
        add_tag_filter(&mut self.tag_input, &mut self.tag_filters);
    }

    /// Removes the tag filter at `index` if it exists.
    pub fn remove_tag(&mut self, index: usize) {
        remove_tag_filter(&mut self.tag_filters, index);
    }
}

#[derive(Clone)]
pub struct CachedLogLine {
    full: String,
    compact: String,
    pub level: char,
    pub tag: String,
    pub message: String,
    pub received_at: Instant,
}

impl CachedLogLine {
    pub fn from_entry(entry: &LogEntry) -> Self {
        CachedLogLine {
            full: entry.format_line_with_timestamp(true),
            compact: entry.format_line_with_timestamp(false),
            level: entry.level,
            tag: entry.tag.clone(),
            message: entry.message.clone(),
            received_at: Instant::now(),
        }
    }

    /// Builds an `InsightLine` from this cached log line.
    fn to_insight_line(&self) -> InsightLine {
        InsightLine {
            received_at: self.received_at,
            level: self.level,
            tag: self.tag.clone(),
            message: self.message.clone(),
        }
    }

    pub fn display(&self, show_timestamp: bool) -> &str {
        if show_timestamp {
            &self.full
        } else {
            &self.compact
        }
    }

    pub fn matches_tag_filters(
        &self,
        tag_filters: &[LogcatTagFilter],
        show_timestamps: bool,
    ) -> bool {
        if tag_filters.is_empty() {
            return true;
        }

        let display = self.display(show_timestamps).to_lowercase();
        tag_filters
            .iter()
            .all(|filter| display.contains(&filter.tag.to_lowercase()))
    }
}

/// Adds a trimmed tag filter if it is non-empty and not already present (case-insensitive).
fn add_tag_filter(input: &mut String, filters: &mut Vec<LogcatTagFilter>) {
    let tag = input.trim().to_string();
    if tag.is_empty() {
        return;
    }

    if filters
        .iter()
        .any(|filter| filter.tag.eq_ignore_ascii_case(&tag))
    {
        input.clear();
        return;
    }

    filters.push(LogcatTagFilter {
        tag: tag.clone(),
        color_index: ui_elements::tag_color_index(&tag),
    });
    input.clear();
}

/// Removes the tag filter at `index` if it exists.
fn remove_tag_filter(filters: &mut Vec<LogcatTagFilter>, index: usize) {
    if index < filters.len() {
        filters.remove(index);
    }
}

fn trim_buffer(buffer: &mut VecDeque<CachedLogLine>) {
    while buffer.len() > MAX_LOG_LINES {
        buffer.pop_front();
    }
}
