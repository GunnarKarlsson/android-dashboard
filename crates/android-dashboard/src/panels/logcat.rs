use std::collections::VecDeque;

use eframe::egui;

use crate::app::{CachedLogLine, ErrorsTab, LogcatPane, LogcatTagFilter};
use crate::theme;
use crate::ui_elements;

/// Draws the all-logcat body and returns the number of rows in the log text area.
pub fn logcat_all_panel(ui: &mut egui::Ui, pane: &mut LogcatPane, auto_scroll: bool) -> usize {
    ui_elements::filter_row(ui, |ui| {
        ui.label("Tag filter:");
        let response = ui_elements::tag_filter_input(ui, &mut pane.tag_input, "logcat_tag_input");
        if response.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter)) {
            pane.add_tag();
            response.request_focus();
        }
    });

    let mut remove_tag_index = None;
    ui_elements::tag_filter_row(ui, &pane.tag_filters, &mut remove_tag_index);
    if let Some(index) = remove_tag_index {
        pane.remove_tag(index);
    }

    if let Some(error) = &pane.error {
        ui_elements::error_label(ui, error);
    }

    if pane.lines.is_empty() {
        ui.label("Waiting for log output…");
        return 0;
    }

    let tag_filters = pane.tag_filters.clone();
    let show_timestamps = pane.show_timestamps;
    let matching = filtered_line_indices(&pane.lines, &tag_filters, show_timestamps, false);

    show_log_scroll(
        ui,
        LogScrollArgs {
            lines: &pane.lines,
            matching: &matching,
            stick_to_bottom: auto_scroll,
            show_timestamps,
            scroll_id: egui::Id::new("logcat_all_scroll"),
            style: LogScrollStyle::ByLevel,
            tag_filters: Some(&tag_filters),
        },
    );
    matching.len()
}

/// Draws the error-logcat body and returns the number of rows in the log text area.
pub fn logcat_errors_panel(ui: &mut egui::Ui, pane: &mut LogcatPane, auto_scroll: bool) -> usize {
    ui_elements::folder_tab_bar(
        ui,
        &mut pane.errors_tab,
        &[
            (ErrorsTab::Errors, "Errors"),
            (ErrorsTab::Crashes, "Panics/Crashes"),
        ],
    );

    match pane.errors_tab {
        ErrorsTab::Errors => logcat_errors_tab(ui, pane, auto_scroll),
        ErrorsTab::Crashes => logcat_crashes_tab(ui, pane, auto_scroll),
    }
}

/// Draws the Errors tab body (unchanged E/F view with its own tag filters).
fn logcat_errors_tab(ui: &mut egui::Ui, pane: &mut LogcatPane, auto_scroll: bool) -> usize {
    ui_elements::filter_row(ui, |ui| {
        ui.label("Tag filter:");
        let response =
            ui_elements::tag_filter_input(ui, &mut pane.tag_input, "logcat_errors_tag_input");
        if response.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter)) {
            pane.add_tag();
            response.request_focus();
        }
    });

    let mut remove_tag_index = None;
    ui_elements::tag_filter_row(ui, &pane.tag_filters, &mut remove_tag_index);
    if let Some(index) = remove_tag_index {
        pane.remove_tag(index);
    }

    if let Some(error) = &pane.error {
        ui_elements::error_label(ui, error);
    }

    if pane.lines.is_empty() {
        ui.label("Waiting for error log output…");
        return 0;
    }

    let tag_filters = pane.tag_filters.clone();
    let show_timestamps = pane.show_timestamps;
    let matching = filtered_line_indices(&pane.lines, &tag_filters, show_timestamps, false);

    show_log_scroll(
        ui,
        LogScrollArgs {
            lines: &pane.lines,
            matching: &matching,
            stick_to_bottom: auto_scroll,
            show_timestamps,
            scroll_id: egui::Id::new("logcat_errors_scroll"),
            style: LogScrollStyle::ErrorsOnly,
            tag_filters: Some(&tag_filters),
        },
    );
    matching.len()
}

/// Draws the Panics/Crashes tab body over the same E/F ring with crash tags.
fn logcat_crashes_tab(ui: &mut egui::Ui, pane: &mut LogcatPane, auto_scroll: bool) -> usize {
    ui_elements::filter_row(ui, |ui| {
        ui.label("Tag filter:");
        let response = ui_elements::tag_filter_input(
            ui,
            &mut pane.crash_tag_input,
            "logcat_errors_crash_tag_input",
        );
        if response.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter)) {
            pane.add_crash_tag();
            response.request_focus();
        }
    });

    let mut remove_tag_index = None;
    ui_elements::tag_filter_row(ui, &pane.crash_tag_filters, &mut remove_tag_index);
    if let Some(index) = remove_tag_index {
        pane.remove_crash_tag(index);
    }

    if let Some(error) = &pane.error {
        ui_elements::error_label(ui, error);
    }

    if pane.lines.is_empty() {
        ui.label("Waiting for error log output…");
        return 0;
    }

    let tag_filters = pane.crash_tag_filters.clone();
    let show_timestamps = pane.show_timestamps;
    let matching = filtered_line_indices(&pane.lines, &tag_filters, show_timestamps, true);

    if matching.is_empty() {
        ui.label("No panics or crashes in the error buffer…");
        return 0;
    }

    show_log_scroll(
        ui,
        LogScrollArgs {
            lines: &pane.lines,
            matching: &matching,
            stick_to_bottom: auto_scroll,
            show_timestamps,
            scroll_id: egui::Id::new("logcat_errors_crash_scroll"),
            style: LogScrollStyle::ErrorsOnly,
            tag_filters: Some(&tag_filters),
        },
    );
    matching.len()
}

#[derive(Clone, Copy)]
enum LogScrollStyle {
    ByLevel,
    ErrorsOnly,
}

struct LogScrollArgs<'a> {
    lines: &'a VecDeque<CachedLogLine>,
    matching: &'a [usize],
    stick_to_bottom: bool,
    show_timestamps: bool,
    scroll_id: egui::Id,
    style: LogScrollStyle,
    tag_filters: Option<&'a [LogcatTagFilter]>,
}

fn filtered_line_indices(
    lines: &VecDeque<CachedLogLine>,
    tag_filters: &[LogcatTagFilter],
    show_timestamps: bool,
    crashes_only: bool,
) -> Vec<usize> {
    lines
        .iter()
        .enumerate()
        .filter(|(_, line)| {
            (!crashes_only || line.is_crash_or_panic())
                && line.matches_tag_filters(tag_filters, show_timestamps)
        })
        .map(|(index, _)| index)
        .collect()
}

fn show_log_scroll(ui: &mut egui::Ui, args: LogScrollArgs<'_>) {
    ui.style_mut().override_text_style = Some(egui::TextStyle::Monospace);
    let row_height = ui.text_style_height(&egui::TextStyle::Monospace);
    let total_rows = args.matching.len();

    // `stick_to_bottom` is egui's API for terminal/log follow. `animated(false)` keeps
    // follow updates from lerping through the buffer when content grows.
    egui::ScrollArea::both()
        .id_salt(args.scroll_id)
        .stick_to_bottom(args.stick_to_bottom)
        .animated(false)
        .auto_shrink([false, false])
        .max_height(ui.available_height())
        .show_rows(ui, row_height, total_rows, |ui, row_range| {
            for row in row_range {
                let line = &args.lines[args.matching[row]];
                let color = match args.style {
                    LogScrollStyle::ErrorsOnly => error_line_color(line.level),
                    LogScrollStyle::ByLevel => log_level_color(line.level),
                };
                let text = line.display(args.show_timestamps);
                if let Some(tag_filters) = args.tag_filters {
                    let mut job = build_highlight_job(ui, text, color, tag_filters);
                    job.wrap.max_rows = 1;
                    job.wrap.overflow_character = None;
                    ui.add(egui::Label::new(job).extend());
                } else {
                    ui.add(egui::Label::new(egui::RichText::new(text).color(color)).extend());
                }
            }
        });
}

fn build_highlight_job(
    ui: &egui::Ui,
    text: &str,
    base_color: egui::Color32,
    tag_filters: &[LogcatTagFilter],
) -> egui::text::LayoutJob {
    use egui::text::{LayoutJob, TextFormat};

    let font_id = ui.style().text_styles[&egui::TextStyle::Monospace].clone();
    let default_format = TextFormat {
        font_id: font_id.clone(),
        color: base_color,
        ..Default::default()
    };

    if tag_filters.is_empty() {
        return LayoutJob::single_section(text.to_owned(), default_format);
    }

    #[derive(Clone, Copy)]
    struct HighlightRange {
        start: usize,
        end: usize,
        color_index: usize,
    }

    let text_lower = text.to_lowercase();
    let mut ranges = Vec::new();

    for filter in tag_filters {
        let needle = filter.tag.to_lowercase();
        if needle.is_empty() {
            continue;
        }

        let mut search_start = 0;
        while let Some(rel) = text_lower[search_start..].find(&needle) {
            let start = search_start + rel;
            let end = start + needle.len();
            ranges.push(HighlightRange {
                start,
                end,
                color_index: filter.color_index,
            });
            search_start = end;
        }
    }

    ranges.sort_by_key(|range| (range.start, -(range.end as isize - range.start as isize)));

    let mut merged = Vec::new();
    let mut last_end = 0;
    for range in ranges {
        if range.start >= last_end {
            merged.push(range);
            last_end = range.end;
        }
    }

    let mut job = LayoutJob::default();
    let mut pos = 0;
    if merged.is_empty() {
        job.append(text, 0.0, default_format);
        return job;
    }

    for range in merged {
        if pos < range.start {
            job.append(&text[pos..range.start], 0.0, default_format.clone());
        }

        let (background, foreground) =
            theme::colors::TAG_HIGHLIGHTS[range.color_index % theme::colors::TAG_HIGHLIGHTS.len()];
        job.append(
            &text[range.start..range.end],
            0.0,
            TextFormat {
                font_id: font_id.clone(),
                color: foreground,
                background,
                ..Default::default()
            },
        );
        pos = range.end;
    }

    if pos < text.len() {
        job.append(&text[pos..], 0.0, default_format);
    }

    job
}

fn log_level_color(level: char) -> egui::Color32 {
    match level {
        'E' => theme::colors::LOG_ERROR,
        'W' => theme::colors::LOG_WARNING,
        'I' => theme::colors::LOG_INFO,
        'D' => theme::colors::LOG_DEBUG,
        'F' => theme::colors::LOG_FATAL,
        _ => theme::colors::LOG_DEFAULT,
    }
}

fn error_line_color(level: char) -> egui::Color32 {
    match level {
        'F' => theme::colors::LOG_FATAL,
        _ => theme::colors::LOG_ERROR,
    }
}
