use ai_insight::InsightConfig;
use eframe::egui;

use crate::theme::{self, colors};

/// Label column + spacing + text field width, matching the grid below.
const AI_PROVIDER_DIALOG_WIDTH: f32 = 372.0;

/// Gap from the window top/right edges to the dialog.
const AI_PROVIDER_DIALOG_EDGE: f32 = 24.0;

pub enum SettingsAction {
    Save(InsightConfig),
    Cancel,
}

/// Draft fields for the AI provider dialog.
#[derive(Default)]
pub struct SettingsDialog {
    open: bool,
    base_url: String,
    model: String,
    api_key: String,
}

impl SettingsDialog {
    /// Copies live config into drafts and opens the dialog.
    pub fn open_from(&mut self, config: &InsightConfig) {
        self.base_url = config.base_url.clone();
        self.model = config.model.clone();
        self.api_key = config.api_key.clone();
        self.open = true;
    }
}

/// Draws the AI provider window when open. Returns Save or Cancel when the user finishes.
pub fn show(ctx: &egui::Context, dialog: &mut SettingsDialog) -> Option<SettingsAction> {
    if !dialog.open {
        return None;
    }

    let screen = ctx.screen_rect();
    let pos = egui::pos2(
        screen.max.x - AI_PROVIDER_DIALOG_EDGE - AI_PROVIDER_DIALOG_WIDTH,
        theme::TITLE_BAR_HEIGHT + AI_PROVIDER_DIALOG_EDGE,
    );

    let mut open = true;
    let mut save = false;
    let mut cancel = false;
    egui::Window::new("Configure AI Provider")
        .id(egui::Id::new("ai_provider_settings"))
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .movable(false)
        .fixed_pos(pos)
        .default_width(AI_PROVIDER_DIALOG_WIDTH)
        .min_width(AI_PROVIDER_DIALOG_WIDTH)
        .max_width(AI_PROVIDER_DIALOG_WIDTH)
        .default_height(0.0)
        .min_height(0.0)
        .frame(
            egui::Frame::default()
                .fill(colors::PANEL_BG)
                .stroke(egui::Stroke::new(1.0, colors::PANEL_BORDER))
                .corner_radius(egui::CornerRadius::same(theme::PANEL_CORNER_RADIUS))
                .inner_margin(egui::Margin::same(theme::PANEL_INNER_PADDING)),
        )
        .show(ctx, |ui| {
            egui::Grid::new("ai_provider_fields")
                .num_columns(2)
                .spacing([8.0, 6.0])
                .show(ui, |ui| {
                    ui.label("Base URL");
                    ui.add(
                        egui::TextEdit::singleline(&mut dialog.base_url)
                            .desired_width(280.0)
                            .hint_text(
                                egui::RichText::new("https://api.example.com")
                                    .color(colors::PLACEHOLDER_TEXT),
                            ),
                    );
                    ui.end_row();

                    ui.label("Model");
                    ui.add(
                        egui::TextEdit::singleline(&mut dialog.model)
                            .desired_width(280.0)
                            .hint_text(
                                egui::RichText::new("model-name").color(colors::PLACEHOLDER_TEXT),
                            ),
                    );
                    ui.end_row();

                    ui.label("API key");
                    ui.add(
                        egui::TextEdit::singleline(&mut dialog.api_key)
                            .desired_width(280.0)
                            .password(true)
                            .hint_text(egui::RichText::new("sk-…").color(colors::PLACEHOLDER_TEXT)),
                    );
                    ui.end_row();
                });

            ui.add_space(theme::ITEM_SPACING_Y);
            ui.scope(|ui| {
                style_dialog_buttons(ui);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::TOP), |ui| {
                    if ui.button("Save").clicked() {
                        save = true;
                    }
                    if ui.button("Cancel").clicked() {
                        cancel = true;
                    }
                });
            });
        });

    if save {
        dialog.open = false;
        return Some(SettingsAction::Save(InsightConfig {
            base_url: dialog.base_url.clone(),
            model: dialog.model.clone(),
            api_key: dialog.api_key.clone(),
        }));
    }
    if cancel || !open {
        dialog.open = false;
        return Some(SettingsAction::Cancel);
    }
    None
}

/// Panel-matched button chrome: body text, panel fill/border, selection fill when pressed.
fn style_dialog_buttons(ui: &mut egui::Ui) {
    ui.style_mut().text_styles.insert(
        egui::TextStyle::Button,
        egui::FontId::new(theme::FONT_BODY, egui::FontFamily::Proportional),
    );

    let stroke = egui::Stroke::new(1.0, colors::PANEL_BORDER);
    let widgets = &mut ui.visuals_mut().widgets;
    for style in [
        &mut widgets.inactive,
        &mut widgets.hovered,
        &mut widgets.active,
    ] {
        style.weak_bg_fill = colors::PANEL_BG;
        style.bg_fill = colors::PANEL_BG;
        style.bg_stroke = stroke;
        style.expansion = 0.0;
    }
    widgets.active.weak_bg_fill = colors::SELECTION;
    widgets.active.bg_fill = colors::SELECTION;
}
