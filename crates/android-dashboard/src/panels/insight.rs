use std::collections::VecDeque;

use eframe::egui;

use crate::app::{InsightController, InsightStatus};
use crate::theme;
use crate::ui_elements;

const REPLY_GAP: f32 = 8.0;
const REPLY_SEPARATOR_GAP: f32 = 4.0;

/// Draws the insight body and returns the number of text lines in the reply area.
pub fn insight_panel(ui: &mut egui::Ui, insight: &InsightController, auto_scroll: bool) -> usize {
    ui_elements::panel_body(ui, theme::colors::INSIGHT_BODY, |ui| {
        if insight.state.replies.is_empty() {
            match insight.state.status {
                InsightStatus::RequestFailed => {
                    ui.label(insight.state.last_failure.as_deref().unwrap_or(
                        "The app received an error from the ai provider. Check your configuration in the settings panel",
                    ));
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
                for (index, reply) in insight.state.replies.iter().enumerate() {
                    if index > 0 {
                        ui.add_space(REPLY_GAP);
                        ui.separator();
                        ui.add_space(REPLY_SEPARATOR_GAP);
                    }
                    ui.label(reply.as_str());
                }
            });
        insight_reply_line_count(&insight.state.replies)
    })
}

/// Counts newline-separated lines across insight replies.
fn insight_reply_line_count(replies: &VecDeque<String>) -> usize {
    replies
        .iter()
        .map(|reply| reply.lines().count().max(1))
        .sum()
}
