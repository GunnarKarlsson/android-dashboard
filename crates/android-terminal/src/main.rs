mod app;
mod format;
mod insight;
mod layout;
mod logcat_pane;
#[cfg(target_os = "macos")]
mod macos;
mod metrics;
mod panels;
mod roster;
mod session;
mod theme;
mod ui_elements;

use adb_client::{Adb, DeviceInfo};
use egui_tiles::Tree;

use crate::app::App;
use crate::layout::PanelId;

struct TerminalApp {
    inner: App,
    layout_tree: Tree<PanelId>,
}

impl TerminalApp {
    fn new(
        adb_error: Option<String>,
        devices: Vec<DeviceInfo>,
        list_error: Option<String>,
    ) -> Self {
        Self {
            inner: App::new(adb_error, devices, list_error),
            layout_tree: layout::create_default_tree(),
        }
    }
}

impl eframe::App for TerminalApp {
    fn update(&mut self, ctx: &eframe::egui::Context, frame: &mut eframe::Frame) {
        self.inner.tick(ctx);

        #[cfg(target_os = "macos")]
        ui_elements::title_bar(ctx, frame);

        eframe::egui::CentralPanel::default()
            .frame(ui_elements::shell_frame(ctx))
            .show(ctx, |ui| {
                ui_elements::canvas_margin_frame().show(ui, |ui| {
                    layout::show(ui, &mut self.layout_tree, &mut self.inner);
                });
            });
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.inner.shutdown();
    }
}

fn main() -> eframe::Result<()> {
    load_dotenv();
    init_tracing();
    tracing::info!("android-terminal started");

    let adb_error = Adb::check_available().err().map(|e| e.to_string());

    let mut viewport = eframe::egui::ViewportBuilder::default()
        .with_inner_size(theme::DEFAULT_WINDOW_SIZE)
        .with_title("Android Terminal");
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
        "Android Terminal",
        options,
        Box::new(|cc| {
            theme::configure(&cc.egui_ctx);

            let (devices, list_error) = if adb_error.is_none() {
                // Listing is synchronous and can block a frame. Do not move it off the UI thread.
                match Adb::list_devices() {
                    Ok(devices) => (devices, None),
                    Err(err) => (Vec::new(), Some(err.to_string())),
                }
            } else {
                (Vec::new(), None)
            };

            Ok(Box::new(TerminalApp::new(adb_error, devices, list_error)))
        }),
    )
}

/// Loads `crates/android-terminal/.env` into the process environment.
fn load_dotenv() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(".env");
    if let Err(err) = dotenvy::from_path(&path) {
        if err.not_found() {
            return;
        }
        eprintln!("failed to load .env: {err}");
    }
}

/// Installs a stderr `tracing` subscriber.
/// Uses `RUST_LOG` when set; otherwise `ai_insight=info,android_terminal=info`.
fn init_tracing() {
    use tracing_subscriber::EnvFilter;

    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("ai_insight=info,android_terminal=info"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .with_ansi(true)
        .init();
}
