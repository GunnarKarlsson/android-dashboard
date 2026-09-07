use std::collections::VecDeque;

use eframe::egui;

use crate::app::{App, InsightStatus};
use crate::theme;
use crate::ui_elements;

const REPLY_GAP: f32 = 8.0;
const REPLY_SEPARATOR_GAP: f32 = 4.0;

/// Draws the insight body and returns the number of text lines in the reply area.
pub fn insight_panel(ui: &mut egui::Ui, app: &mut App, auto_scroll: bool) -> usize {
    if app.selected_serial.is_none() {
        ui_elements::panel_loading(ui);
        return 0;
    }

    ui_elements::panel_body(ui, theme::colors::INSIGHT_BODY, |ui| {
        if app.insight.replies.is_empty() {
            match app.insight.status {
                InsightStatus::RequestFailed => {
                    ui_elements::error_label(ui, "request failed, see log");
                }
                InsightStatus::Idle | InsightStatus::RequestSent => {
                    ui.label("...");
                }
            }
            return 0;
        }

        egui::ScrollArea::vertical()
            .id_salt(egui::Id::new("insight_scroll"))
            .stick_to_bottom(auto_scroll)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                for (index, reply) in app.insight.replies.iter().enumerate() {
                    if index > 0 {
                        ui.add_space(REPLY_GAP);
                        ui.separator();
                        ui.add_space(REPLY_SEPARATOR_GAP);
                    }
                    ui.label(reply.as_str());
                }
            });
        insight_reply_line_count(&app.insight.replies)
    })
}

/// Counts newline-separated lines across insight replies.
fn insight_reply_line_count(replies: &VecDeque<String>) -> usize {
    replies
        .iter()
        .map(|reply| reply.lines().count().max(1))
        .sum()
}
