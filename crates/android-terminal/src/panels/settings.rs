use ai_insight::InsightConfig;
use eframe::egui;

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

    let mut open = true;
    let mut save = false;
    let mut cancel = false;
    egui::Window::new("Configure AI Provider")
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .show(ctx, |ui| {
            egui::Grid::new("ai_provider_fields")
                .num_columns(2)
                .spacing([8.0, 8.0])
                .show(ui, |ui| {
                    ui.label("Base URL");
                    ui.add(
                        egui::TextEdit::singleline(&mut dialog.base_url)
                            .desired_width(280.0)
                            .hint_text("https://api.example.com"),
                    );
                    ui.end_row();

                    ui.label("Model");
                    ui.add(
                        egui::TextEdit::singleline(&mut dialog.model)
                            .desired_width(280.0)
                            .hint_text("model-name"),
                    );
                    ui.end_row();

                    ui.label("API key");
                    ui.add(
                        egui::TextEdit::singleline(&mut dialog.api_key)
                            .desired_width(280.0)
                            .password(true)
                            .hint_text("sk-…"),
                    );
                    ui.end_row();
                });

            ui.add_space(8.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Save").clicked() {
                    save = true;
                }
                if ui.button("Cancel").clicked() {
                    cancel = true;
                }
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
