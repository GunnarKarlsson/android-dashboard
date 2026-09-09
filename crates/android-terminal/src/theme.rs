//! Central egui style and theme configuration.

use std::sync::Arc;

use eframe::egui::{self, Context, FontData, FontDefinitions, FontFamily, FontId, TextStyle};

const JETBRAINS_MONO: &[u8] = include_bytes!("../assets/fonts/JetBrainsMonoNerdFont-Regular.ttf");
const JETBRAINS_MONO_BOLD: &[u8] = include_bytes!("../assets/fonts/JetBrainsMonoNerdFont-Bold.ttf");

/// All UI colors are defined here once.
pub mod colors {
    use egui::Color32;

    pub const BG: Color32 = Color32::from_rgb(10, 24, 52);
    pub const BG_WIDGET: Color32 = Color32::from_rgb(18, 40, 78);
    pub const BG_HOVER: Color32 = Color32::from_rgb(28, 56, 104);
    pub const BG_EXTREME: Color32 = Color32::from_rgb(6, 16, 36);
    pub const BG_FAINT: Color32 = Color32::from_rgb(16, 36, 72);
    /// Panel card fill.
    pub const PANEL_BG: Color32 = Color32::from_rgb(12, 22, 40);
    /// Thin outline around each panel card.
    pub const PANEL_BORDER: Color32 = Color32::from_rgb(58, 64, 74);

    /// Text and widget foreground.
    pub const OFF_WHITE: Color32 = Color32::from_rgb(232, 232, 228);
    /// Panel header icons.
    pub const HEADER_ICON: Color32 = Color32::from_rgb(176, 180, 188);
    /// Muted footer status text inside panel cards.
    pub const FOOTER_TEXT: Color32 = Color32::from_rgb(140, 146, 156);
    /// Placeholder hint in empty text fields.
    pub const PLACEHOLDER_TEXT: Color32 = Color32::from_rgb(96, 102, 112);
    /// Resize handle highlight (gap stays empty when idle).
    pub const PANEL_SPLITTER_HOVER: Color32 = Color32::from_rgb(96, 102, 112);
    /// Header rule inside a panel card.
    pub const PANEL_SEPARATOR: Color32 = Color32::from_rgb(56, 62, 72);
    /// Selected icon toggle / device list fill.
    pub const SELECTION: Color32 = Color32::from_rgb(72, 78, 88);

    pub const ERROR: Color32 = Color32::from_rgb(220, 80, 80);
    pub const LOG_ERROR: Color32 = ERROR;
    pub const LOG_WARNING: Color32 = Color32::from_rgb(220, 180, 60);
    pub const LOG_INFO: Color32 = Color32::from_rgb(120, 180, 255);
    pub const LOG_DEBUG: Color32 = Color32::from_rgb(140, 140, 140);
    pub const LOG_FATAL: Color32 = Color32::from_rgb(255, 60, 60);
    pub const LOG_DEFAULT: Color32 = Color32::GRAY;

    pub const RAM_USED: Color32 = Color32::from_rgb(80, 200, 220);
    pub const RAM_TRACK: Color32 = Color32::from_rgb(40, 50, 65);

    pub const STORAGE_USED: Color32 = Color32::from_rgb(240, 120, 60);
    pub const STORAGE_TRACK: Color32 = Color32::from_rgb(55, 40, 35);

    /// Memory / Disk panel body text.
    pub const MEMORY_DISK_BODY: Color32 = Color32::from_rgb(180, 230, 80);
    /// Insight panel body text.
    pub const INSIGHT_BODY: Color32 = Color32::from_rgb(255, 160, 200);
    /// App Traffic panel body text.
    pub const APP_TRAFFIC_BODY: Color32 = Color32::from_rgb(200, 170, 255);

    /// Background and foreground pairs for logcat tag highlight badges.
    pub const TAG_HIGHLIGHTS: &[(Color32, Color32)] = &[
        (
            Color32::from_rgb(58, 68, 82),
            Color32::from_rgb(210, 218, 228),
        ),
        (
            Color32::from_rgb(52, 72, 68),
            Color32::from_rgb(196, 220, 210),
        ),
        (
            Color32::from_rgb(72, 58, 68),
            Color32::from_rgb(220, 200, 214),
        ),
        (
            Color32::from_rgb(58, 62, 78),
            Color32::from_rgb(200, 206, 228),
        ),
        (
            Color32::from_rgb(68, 64, 52),
            Color32::from_rgb(220, 214, 196),
        ),
        (
            Color32::from_rgb(52, 66, 72),
            Color32::from_rgb(196, 214, 222),
        ),
        (
            Color32::from_rgb(70, 58, 58),
            Color32::from_rgb(228, 204, 204),
        ),
        (
            Color32::from_rgb(60, 70, 60),
            Color32::from_rgb(208, 220, 206),
        ),
    ];
}

/// Corner radius for panel cards.
pub const PANEL_CORNER_RADIUS: u8 = 10;

/// Empty space between adjacent panel cards (canvas shows through).
pub const PANEL_GAP: f32 = 12.0;

/// Padding inside a panel card, around title and body.
pub const PANEL_INNER_PADDING: i8 = 8;

/// Padding between the window edge and panel cards.
pub const PANEL_CANVAS_MARGIN: i8 = 12;

/// Height of the custom macOS title bar under the native traffic lights.
pub const TITLE_BAR_HEIGHT: f32 = 36.0;

/// Width of the native traffic-light hit region from the left edge.
pub const TRAFFIC_LIGHTS_WIDTH: f32 = 96.0;

/// Default window inner size `[width, height]`.
pub const DEFAULT_WINDOW_SIZE: [f32; 2] = [1400.0, 900.0];

pub const FONT_SMALL: f32 = 10.0;
pub const FONT_BODY: f32 = 13.0;
pub const FONT_BUTTON: f32 = 14.0;
pub const FONT_HEADING: f32 = 16.0;

pub const ITEM_SPACING_X: f32 = 8.0;
pub const ITEM_SPACING_Y: f32 = 6.0;

/// Horizontal and vertical spacing inside data grids.
pub const GRID_SPACING: [f32; 2] = [12.0, 4.0];

/// Panel header icons.
pub mod icons {
    /// RAM module icon.
    pub fn ram() -> egui::ImageSource<'static> {
        egui::include_image!("../assets/icons/ram.svg")
    }

    /// Hard-drive icon.
    pub fn disc() -> egui::ImageSource<'static> {
        egui::include_image!("../assets/icons/disc.svg")
    }

    /// Network sitemap icon.
    pub fn network() -> egui::ImageSource<'static> {
        egui::include_image!("../assets/icons/network.svg")
    }

    /// Triangle warning icon.
    pub fn errors() -> egui::ImageSource<'static> {
        egui::include_image!("../assets/icons/errors.svg")
    }

    /// Cat icon.
    pub fn logcat() -> egui::ImageSource<'static> {
        egui::include_image!("../assets/icons/cat.svg")
    }

    /// Microchip icon.
    pub fn insight() -> egui::ImageSource<'static> {
        egui::include_image!("../assets/icons/insight.svg")
    }

    /// Traffic-light icon.
    pub fn traffic() -> egui::ImageSource<'static> {
        egui::include_image!("../assets/icons/traffic.svg")
    }

    /// Mobile-device icon.
    pub fn device() -> egui::ImageSource<'static> {
        egui::include_image!("../assets/icons/device.svg")
    }
}

/// Apply app-wide egui styling.
pub fn configure(ctx: &Context) {
    egui_extras::install_image_loaders(ctx);
    install_fonts(ctx);

    ctx.all_styles_mut(apply_shared_style);
    ctx.set_visuals_of(egui::Theme::Dark, dark_blue_visuals());
    ctx.set_theme(egui::Theme::Dark);
    ctx.send_viewport_cmd(egui::ViewportCommand::SetTheme(egui::SystemTheme::Dark));
}

fn apply_shared_style(style: &mut egui::Style) {
    style.spacing.window_margin = egui::Margin::same(PANEL_INNER_PADDING);
    style.spacing.item_spacing = egui::vec2(ITEM_SPACING_X, ITEM_SPACING_Y);

    let bold = FontFamily::Name("jetbrains_mono_bold".into());
    style.text_styles.insert(
        TextStyle::Small,
        FontId::new(FONT_SMALL, FontFamily::Proportional),
    );
    style.text_styles.insert(
        TextStyle::Body,
        FontId::new(FONT_BODY, FontFamily::Proportional),
    );
    style.text_styles.insert(
        TextStyle::Button,
        FontId::new(FONT_BUTTON, FontFamily::Proportional),
    );
    style
        .text_styles
        .insert(TextStyle::Heading, FontId::new(FONT_HEADING, bold));
    style.text_styles.insert(
        TextStyle::Monospace,
        FontId::new(FONT_BODY, FontFamily::Monospace),
    );
}

fn dark_blue_visuals() -> egui::Visuals {
    let mut visuals = egui::Visuals::dark();

    visuals.panel_fill = colors::BG;
    visuals.window_fill = colors::BG;
    visuals.extreme_bg_color = colors::BG_EXTREME;
    visuals.faint_bg_color = colors::BG_FAINT;
    visuals.code_bg_color = colors::BG_EXTREME;
    visuals.override_text_color = Some(colors::OFF_WHITE);
    visuals.selection.bg_fill = colors::SELECTION;

    visuals.widgets.noninteractive.bg_fill = colors::BG;
    visuals.widgets.noninteractive.weak_bg_fill = colors::BG;
    visuals.widgets.noninteractive.fg_stroke.color = colors::OFF_WHITE;
    visuals.widgets.noninteractive.bg_stroke = egui::Stroke::NONE;

    visuals.widgets.inactive.bg_fill = colors::BG_WIDGET;
    visuals.widgets.inactive.weak_bg_fill = colors::BG_WIDGET;
    visuals.widgets.inactive.fg_stroke.color = colors::OFF_WHITE;

    visuals.widgets.hovered.bg_fill = colors::BG_HOVER;
    visuals.widgets.hovered.weak_bg_fill = colors::BG_HOVER;
    visuals.widgets.hovered.fg_stroke.color = colors::OFF_WHITE;

    visuals.widgets.active.bg_fill = colors::BG_HOVER;
    visuals.widgets.active.weak_bg_fill = colors::BG_HOVER;
    visuals.widgets.active.fg_stroke.color = colors::OFF_WHITE;

    visuals.widgets.open.bg_fill = colors::BG_WIDGET;
    visuals.widgets.open.weak_bg_fill = colors::BG_WIDGET;
    visuals.widgets.open.fg_stroke.color = colors::OFF_WHITE;

    visuals
}

fn install_fonts(ctx: &Context) {
    let mut fonts = FontDefinitions::default();

    fonts.font_data.insert(
        "jetbrains_mono".to_owned(),
        Arc::new(FontData::from_static(JETBRAINS_MONO)),
    );
    fonts.font_data.insert(
        "jetbrains_mono_bold".to_owned(),
        Arc::new(FontData::from_static(JETBRAINS_MONO_BOLD)),
    );

    fonts
        .families
        .entry(FontFamily::Proportional)
        .or_default()
        .insert(0, "jetbrains_mono".to_owned());
    fonts
        .families
        .entry(FontFamily::Monospace)
        .or_default()
        .insert(0, "jetbrains_mono".to_owned());
    fonts.families.insert(
        FontFamily::Name("jetbrains_mono_bold".into()),
        vec!["jetbrains_mono_bold".to_owned()],
    );

    ctx.set_fonts(fonts);
}
