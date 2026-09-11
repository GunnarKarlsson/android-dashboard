mod app;
mod format;
mod insight;
mod insight_store;
mod layout;
mod layout_store;
mod logcat_pane;
#[cfg(target_os = "macos")]
mod macos;
mod metrics;
mod panels;
mod roster;
mod session;
mod theme;
mod ui_elements;

use adb_client::Adb;
use egui_tiles::Tree;

use crate::app::App;
use crate::layout::PanelId;
use crate::panels::settings::{SettingsAction, SettingsDialog};

struct TerminalApp {
    inner: App,
    layout_tree: Tree<PanelId>,
    layout_saver: layout_store::LayoutSaver,
    settings: SettingsDialog,
}

impl TerminalApp {
    fn new(adb_error: Option<String>) -> Self {
        let should_refresh = adb_error.is_none();
        let mut inner = App::new(adb_error, insight_store::load_or_default());
        if should_refresh {
            inner.refresh_devices();
        }
        Self {
            inner,
            layout_tree: layout_store::load_or_default(),
            layout_saver: layout_store::LayoutSaver::default(),
            settings: SettingsDialog::default(),
        }
    }
}

impl eframe::App for TerminalApp {
    fn update(&mut self, ctx: &eframe::egui::Context, frame: &mut eframe::Frame) {
        self.inner.tick(ctx);

        #[cfg(target_os = "macos")]
        {
            let title = ui_elements::title_bar(ctx, frame);
            if title.reset_layout {
                self.layout_tree = layout::create_default_tree();
                if layout_store::save(&self.layout_tree) {
                    self.layout_saver.clear_dirty();
                } else {
                    self.layout_saver.mark_edit();
                }
            }
            if title.open_ai_settings {
                self.settings.open_from(self.inner.insight.config());
            }
        }

        if let Some(action) = panels::settings::show(ctx, &mut self.settings) {
            match action {
                SettingsAction::Save(config) => {
                    insight_store::save(&config);
                    self.inner.insight.set_config(config);
                }
                SettingsAction::Cancel => {}
            }
        }

        let mut layout_dirty = false;
        eframe::egui::CentralPanel::default()
            .frame(ui_elements::shell_frame(ctx))
            .show(ctx, |ui| {
                ui_elements::canvas_margin_frame().show(ui, |ui| {
                    layout::show(
                        ui,
                        &mut self.layout_tree,
                        &mut self.inner,
                        &mut layout_dirty,
                    );
                });
            });
        if layout_dirty {
            self.layout_saver.mark_edit();
        }
        self.layout_saver.tick(ctx, &self.layout_tree);
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.layout_saver.flush(&self.layout_tree);
        self.inner.shutdown();
    }
}

fn main() -> eframe::Result<()> {
    init_tracing();

    let adb_error = Adb::check_available().err().map(|e| e.to_string());

    let icon = eframe::icon_data::from_png_bytes(include_bytes!("../assets/icons/app.png"))
        .expect("app icon");

    let mut viewport = eframe::egui::ViewportBuilder::default()
        .with_inner_size(theme::DEFAULT_WINDOW_SIZE)
        .with_title("Android Debug Dashboard")
        .with_icon(icon);
    #[cfg(target_os = "macos")]
    {
        // Content draws under the traffic lights; we paint a dark grey title strip.
        viewport = viewport
            .with_fullsize_content_view(true)
            .with_titlebar_shown(false)
            .with_title_shown(false);
    }

    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };

    eframe::run_native(
        "Android Debug Dashboard",
        options,
        Box::new(|cc| {
            theme::configure(&cc.egui_ctx);

            Ok(Box::new(TerminalApp::new(adb_error)))
        }),
    )
}

/// Installs a stderr `tracing` subscriber.
/// Uses `RUST_LOG` when set; otherwise `ai_insight=info,android_dashboard=info`.
fn init_tracing() {
    use tracing_subscriber::EnvFilter;

    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("ai_insight=info,android_dashboard=info"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .with_ansi(true)
        .init();
}
