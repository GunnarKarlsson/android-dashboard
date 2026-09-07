//! Shared UI widgets: panel cards, footers, icon buttons, filter rows.

use eframe::egui::{self, Context, FontFamily, FontId, TextStyle, Ui};

use crate::theme::{self, colors};

/// Show an error label using the theme error color.
pub fn error_label(ui: &mut Ui, text: impl AsRef<str>) {
    ui.colored_label(colors::ERROR, text.as_ref());
}

/// Run panel body content with a dedicated text color (headers stay on the default style).
pub fn panel_body<R>(ui: &mut Ui, color: egui::Color32, add_body: impl FnOnce(&mut Ui) -> R) -> R {
    let mut style = ui.style().as_ref().clone();
    style.visuals.override_text_color = Some(color);
    ui.scope(|ui| {
        ui.set_style(style);
        add_body(ui)
    })
    .inner
}

fn panel_separator(ui: &mut Ui) {
    let spacing = ui.spacing().item_spacing.y;
    let (rect, response) = ui.allocate_at_least(
        egui::vec2(ui.available_width(), spacing),
        egui::Sense::hover(),
    );
    if ui.is_rect_visible(rect) {
        ui.painter().hline(
            rect.x_range(),
            rect.center().y,
            egui::Stroke::new(1.0, colors::PANEL_SEPARATOR),
        );
    }
    ui.advance_cursor_after_rect(response.rect);
}

/// Dark canvas behind panel cards.
pub fn shell_frame(_ctx: &Context) -> egui::Frame {
    egui::Frame::NONE.fill(colors::BG_EXTREME)
}

/// Insets panel cards from the window edge.
pub fn canvas_margin_frame() -> egui::Frame {
    egui::Frame::NONE.inner_margin(egui::Margin::same(theme::PANEL_CANVAS_MARGIN))
}

/// macOS title strip: dark grey bar with off-white app title; traffic lights stay native.
#[cfg(target_os = "macos")]
pub fn title_bar(ctx: &Context) {
    egui::TopBottomPanel::top("os_title_bar")
        .exact_height(theme::TITLE_BAR_HEIGHT)
        .frame(egui::Frame::NONE.fill(colors::TITLE_BAR))
        .show_separator_line(false)
        .show(ctx, |ui| {
            let rect = ui.max_rect();
            let response = ui.interact(
                rect,
                ui.id().with("title_bar_drag"),
                egui::Sense::click_and_drag(),
            );
            if response.drag_started_by(egui::PointerButton::Primary) {
                ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
            }
            if response.double_clicked() {
                let maximized = ui.input(|i| i.viewport().maximized.unwrap_or(false));
                ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
            }

            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "Android Terminal",
                FontId::new(13.0, FontFamily::Proportional),
                colors::OFF_WHITE,
            );
        });
}

fn panel_frame(ui: &Ui) -> egui::Frame {
    egui::Frame::default()
        .fill(colors::PANEL_BG)
        .stroke(egui::Stroke::new(1.0, colors::PANEL_BORDER))
        .corner_radius(egui::CornerRadius::same(theme::PANEL_CORNER_RADIUS))
        .inner_margin(panel_padding(ui))
}

fn panel_padding(ui: &Ui) -> egui::Margin {
    ui.style().spacing.window_margin
}

const ICON_BUTTON_PADDING: f32 = 6.0;

fn icon_button_widget(ui: &mut Ui, icon: &str) -> egui::Response {
    let galley = egui::WidgetText::from(icon).into_galley(
        ui,
        Some(egui::TextWrapMode::Extend),
        f32::INFINITY,
        egui::TextStyle::Button,
    );
    let ink = if galley.mesh_bounds.is_positive() {
        galley.mesh_bounds
    } else {
        galley.rect
    };
    let inner = ink.size().x.max(ink.size().y);
    let button_size = inner + 2.0 * ICON_BUTTON_PADDING;
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(button_size, button_size), egui::Sense::click());

    if ui.is_rect_visible(rect) {
        let visuals = ui.style().interact(&response);
        ui.painter().rect(
            rect,
            visuals.corner_radius,
            visuals.weak_bg_fill,
            egui::Stroke::NONE,
            egui::StrokeKind::Inside,
        );
        let pos = rect.center() - ink.center().to_vec2();
        ui.painter().galley(pos, galley, visuals.text_color());
    }

    if let Some(cursor) = ui.visuals().interact_cursor {
        if response.hovered() {
            ui.ctx().set_cursor_icon(cursor);
        }
    }

    response
}

/// Icon-only button.
pub fn icon_button(ui: &mut Ui, icon: &str) -> egui::Response {
    icon_button_widget(ui, icon)
}

/// Space below a toolbar row (filter, etc.), matching panel padding.
pub fn section_gap(ui: &mut Ui) {
    ui.add_space(panel_padding(ui).bottom as f32);
}

/// Filter row with themed spacing underneath.
pub fn filter_row(ui: &mut Ui, add_contents: impl FnOnce(&mut Ui)) {
    ui.horizontal(add_contents);
    section_gap(ui);
}

/// Stable palette index for a tag name.
pub fn tag_color_index(tag: &str) -> usize {
    use std::hash::{Hash, Hasher};

    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    tag.to_lowercase().hash(&mut hasher);
    hasher.finish() as usize % colors::TAG_HIGHLIGHTS.len()
}

/// Removable tag-filter badge. Returns `true` when the remove control is clicked.
pub fn tag_filter_badge(ui: &mut Ui, label: &str, bg: egui::Color32, fg: egui::Color32) -> bool {
    let mut remove = false;
    egui::Frame::default()
        .fill(bg)
        .corner_radius(egui::CornerRadius::same(4))
        .inner_margin(egui::Margin::symmetric(6, 2))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                ui.label(egui::RichText::new(label).color(fg).monospace());
                if ui
                    .small_button("×")
                    .on_hover_text("Remove tag filter")
                    .clicked()
                {
                    remove = true;
                }
            });
        });
    remove
}

/// Row of active tag-filter badges. `on_remove` is called with the badge index.
pub fn tag_filter_row(
    ui: &mut Ui,
    tags: &[crate::app::LogcatTagFilter],
    on_remove: &mut Option<usize>,
) {
    if tags.is_empty() {
        return;
    }

    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
        for (index, filter) in tags.iter().enumerate() {
            let (bg, fg) =
                colors::TAG_HIGHLIGHTS[filter.color_index % colors::TAG_HIGHLIGHTS.len()];
            let label = format!("tag:{}", filter.tag);
            if tag_filter_badge(ui, &label, bg, fg) {
                *on_remove = Some(index);
            }
        }
    });
    section_gap(ui);
}

pub fn panel_loading(ui: &mut Ui) {
    ui.label("Loading...");
}

/// One chrome for every panel: uniform padding on all four sides, title, then body.
pub fn panel<R>(
    ui: &mut Ui,
    title: impl Into<egui::RichText>,
    add_body: impl FnOnce(&mut Ui) -> R,
) -> R {
    panel_with_header_actions(ui, title, |_| {}, add_body)
}

/// Like [`panel`], with extra widgets on the header row (e.g. the devices refresh icon).
pub fn panel_with_header_actions<R>(
    ui: &mut Ui,
    title: impl Into<egui::RichText>,
    add_header_actions: impl FnOnce(&mut Ui),
    add_body: impl FnOnce(&mut Ui) -> R,
) -> R {
    panel_frame(ui)
        .show(ui, |ui| {
            ui.set_min_size(ui.max_rect().size());
            ui.horizontal(|ui| {
                ui.heading(title);
                add_header_actions(ui);
            });
            panel_separator(ui);
            add_body(ui)
        })
        .inner
}

/// Like [`panel_with_header_actions`], with a bottom Stream footer inside the card.
///
/// `add_contents` receives the current auto-scroll flag for stick-to-bottom; the footer
/// is the only control that toggles it. `show_timestamps` adds a Timestamp on/off control
/// to the left of Stream when `Some`.
pub fn panel_with_footer<R>(
    ui: &mut Ui,
    title: impl Into<egui::RichText>,
    add_header_actions: impl FnOnce(&mut Ui),
    add_contents: impl FnOnce(&mut Ui, bool) -> R,
    auto_scroll: &mut bool,
    show_timestamps: Option<&mut bool>,
) -> R {
    // Same structure as `Frame::begin`/`end`, but always paint/allocate the tile-sized
    // content rect. Overflowing body content must not push the bottom stroke outside the
    // pane clip (that drops the bottom border).
    let frame = panel_frame(ui);
    let where_to_put_background = ui.painter().add(egui::Shape::Noop);
    let outer_rect_bounds = ui.available_rect_before_wrap();
    let mut content_rect = outer_rect_bounds - frame.total_margin();
    content_rect.max.x = content_rect.max.x.max(content_rect.min.x);
    content_rect.max.y = content_rect.max.y.max(content_rect.min.y);

    let mut content_ui = ui.new_child(egui::UiBuilder::new().max_rect(content_rect));
    content_ui.set_clip_rect(content_ui.clip_rect().intersect(content_rect));
    content_ui.set_min_size(content_rect.size());

    content_ui.horizontal(|ui| {
        ui.heading(title);
        add_header_actions(ui);
    });
    panel_separator(&mut content_ui);

    // Pin the footer to the card bottom. Sequential allocation lets a ScrollArea
    // grow the body and push the toggle below the clip rect (error logcat).
    let footer_height = panel_footer_height(&content_ui);
    let max_rect = content_ui.max_rect();
    let footer_top = (max_rect.bottom() - footer_height).max(max_rect.top());
    let footer_rect = egui::Rect::from_min_max(
        egui::pos2(max_rect.left(), footer_top),
        max_rect.right_bottom(),
    );
    let body_rect = egui::Rect::from_min_max(
        egui::pos2(max_rect.left(), content_ui.next_widget_position().y),
        egui::pos2(max_rect.right(), footer_rect.top()),
    );

    let result = content_ui
        .allocate_new_ui(egui::UiBuilder::new().max_rect(body_rect), |ui| {
            ui.set_clip_rect(ui.clip_rect().intersect(body_rect));
            add_contents(ui, *auto_scroll)
        })
        .inner;

    content_ui.allocate_new_ui(egui::UiBuilder::new().max_rect(footer_rect), |ui| {
        panel_footer(ui, auto_scroll, show_timestamps);
    });

    let widget_rect = frame.widget_rect(content_rect);
    if ui.is_rect_visible(widget_rect) {
        ui.painter()
            .set(where_to_put_background, frame.paint(content_rect));
    }
    ui.allocate_rect(frame.outer_rect(content_rect), egui::Sense::hover());

    result
}

/// Vertical padding above and below the footer status text.
const FOOTER_PAD_Y: f32 = 8.0;

/// Vertical space reserved for [`panel_footer`].
fn panel_footer_height(ui: &Ui) -> f32 {
    let text = ui.text_style_height(&TextStyle::Small);
    text + 2.0 * FOOTER_PAD_Y
}

/// Extra inset from the footer’s right edge for the Stream label.
const FOOTER_LABEL_INSET_X: f32 = 8.0;

/// Downward shift of the Stream label inside the footer.
const FOOTER_LABEL_OFFSET_Y: f32 = 3.0;

/// Draws the panel footer bar: top hairline, then Stream and optional Timestamp controls.
pub fn panel_footer(
    ui: &mut Ui,
    auto_scroll: &mut bool,
    show_timestamps: Option<&mut bool>,
) {
    let rect = ui.max_rect();
    ui.allocate_rect(rect, egui::Sense::hover());

    ui.painter().hline(
        rect.x_range(),
        rect.top(),
        egui::Stroke::new(1.0, colors::PANEL_SEPARATOR),
    );

    let stream_label = if *auto_scroll {
        "Stream: Active"
    } else {
        "Stream: Paused"
    };

    let content_rect = egui::Rect::from_min_max(
        egui::pos2(rect.left(), rect.top() + FOOTER_LABEL_OFFSET_Y),
        egui::pos2(
            rect.right() - FOOTER_LABEL_INSET_X,
            rect.bottom() + FOOTER_LABEL_OFFSET_Y,
        ),
    );

    ui.allocate_new_ui(egui::UiBuilder::new().max_rect(content_rect), |ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            footer_toggle(ui, stream_label, "Toggle logcat updates", auto_scroll);
            if let Some(show_timestamps) = show_timestamps {
                ui.add_space(ui.spacing().item_spacing.x);
                let timestamp_label = if *show_timestamps {
                    "Timestamp: on"
                } else {
                    "Timestamp: off"
                };
                footer_toggle(ui, timestamp_label, "Toggle timestamps", show_timestamps);
            }
        });
    });
}

fn footer_toggle(ui: &mut Ui, label: &str, hover: &str, flag: &mut bool) {
    let response = ui
        .add(
            egui::Label::new(
                egui::RichText::new(label)
                    .small()
                    .color(colors::FOOTER_TEXT),
            )
            .sense(egui::Sense::click()),
        )
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(hover);
    if response.clicked() {
        *flag = !*flag;
    }
}
