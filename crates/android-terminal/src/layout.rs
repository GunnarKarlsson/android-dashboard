use eframe::egui;
use egui_tiles::{Behavior, ResizeState, TileId, Tree, UiResponse};
use serde::{Deserialize, Serialize};

use crate::app::App;
use crate::panels;
use crate::panels::devices::DevicesAction;
use crate::theme;
use crate::ui_elements;

// Share is egui's term for the relative width of a panel.
const COL_SHARE: f32 = 1.0;
const GAUGES_SHARE: f32 = 1.5;
const STORAGE_DETAILS_SHARE: f32 = 2.5;
const LOGCAT_SHARE: f32 = 2.0;
const INSIGHT_SHARE: f32 = 1.5;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PanelId {
    Devices,
    Ram,
    Storage,
    LogcatAll,
    LogcatErrors,
    Insight,
    SystemStats,
    Network,
    Protocols,
}

impl PanelId {
    fn title(self) -> &'static str {
        match self {
            PanelId::Devices => "Devices",
            PanelId::Ram => "RAM",
            PanelId::Storage => "Storage",
            PanelId::LogcatAll => "Logcat",
            PanelId::LogcatErrors => "Logcat Errors",
            PanelId::Insight => "Insight",
            PanelId::SystemStats => "Storage Details",
            PanelId::Network => "Network Activity",
            PanelId::Protocols => "App Traffic",
        }
    }

    /// Returns the header icon for this panel.
    fn icon(self) -> Option<egui::ImageSource<'static>> {
        Some(match self {
            PanelId::Ram => theme::icons::ram(),
            PanelId::Storage | PanelId::SystemStats => theme::icons::disc(),
            PanelId::Network => theme::icons::network(),
            PanelId::LogcatErrors => theme::icons::errors(),
            PanelId::LogcatAll => theme::icons::logcat(),
            PanelId::Insight => theme::icons::insight(),
            PanelId::Protocols => theme::icons::traffic(),
            PanelId::Devices => theme::icons::device(),
        })
    }
}

pub fn create_default_tree() -> Tree<PanelId> {
    let mut tiles = egui_tiles::Tiles::default();

    let devices = tiles.insert_pane(PanelId::Devices);
    let ram = tiles.insert_pane(PanelId::Ram);
    let storage = tiles.insert_pane(PanelId::Storage);
    let gauges = tiles.insert_horizontal_tile(vec![ram, storage]);
    let system_stats = tiles.insert_pane(PanelId::SystemStats);
    let left_column = tiles.insert_vertical_tile(vec![devices, gauges, system_stats]);

    let logcat_all = tiles.insert_pane(PanelId::LogcatAll);
    let network = tiles.insert_pane(PanelId::Network);
    let protocols = tiles.insert_pane(PanelId::Protocols);
    let middle_column = tiles.insert_vertical_tile(vec![logcat_all, network, protocols]);

    let logcat_errors = tiles.insert_pane(PanelId::LogcatErrors);
    let insight = tiles.insert_pane(PanelId::Insight);
    let right_column = tiles.insert_vertical_tile(vec![logcat_errors, insight]);

    let root = tiles.insert_horizontal_tile(vec![left_column, middle_column, right_column]);

    set_linear_shares(
        &mut tiles,
        root,
        &[
            (left_column, COL_SHARE),
            (middle_column, COL_SHARE),
            (right_column, COL_SHARE),
        ],
    );
    set_linear_shares(
        &mut tiles,
        left_column,
        &[
            (devices, COL_SHARE),
            (gauges, GAUGES_SHARE),
            (system_stats, STORAGE_DETAILS_SHARE),
        ],
    );
    set_linear_shares(
        &mut tiles,
        gauges,
        &[(ram, COL_SHARE), (storage, COL_SHARE)],
    );
    set_linear_shares(
        &mut tiles,
        middle_column,
        &[
            (logcat_all, LOGCAT_SHARE),
            (network, COL_SHARE),
            (protocols, COL_SHARE),
        ],
    );
    set_linear_shares(
        &mut tiles,
        right_column,
        &[(logcat_errors, LOGCAT_SHARE), (insight, INSIGHT_SHARE)],
    );

    Tree::new("android_terminal_tiles", root, tiles)
}

fn set_linear_shares(
    tiles: &mut egui_tiles::Tiles<PanelId>,
    container_id: egui_tiles::TileId,
    shares: &[(egui_tiles::TileId, f32)],
) {
    let Some(egui_tiles::Tile::Container(egui_tiles::Container::Linear(linear))) =
        tiles.get_mut(container_id)
    else {
        return;
    };

    for (tile_id, share) in shares {
        linear.shares.set_share(*tile_id, *share);
    }
}

pub fn show(ui: &mut egui::Ui, tree: &mut Tree<PanelId>, app: &mut App) {
    let mut behavior = AppTilesBehavior { app };
    tree.ui(&mut behavior, ui);
}

struct AppTilesBehavior<'a> {
    app: &'a mut App,
}

impl Behavior<PanelId> for AppTilesBehavior<'_> {
    fn tab_title_for_pane(&mut self, pane: &PanelId) -> egui::WidgetText {
        pane.title().into()
    }

    fn gap_width(&self, _style: &egui::Style) -> f32 {
        theme::PANEL_GAP
    }

    fn resize_stroke(&self, _style: &egui::Style, resize_state: ResizeState) -> egui::Stroke {
        match resize_state {
            ResizeState::Idle => egui::Stroke::NONE,
            ResizeState::Hovering | ResizeState::Dragging => {
                egui::Stroke::new(1.0, theme::colors::PANEL_SPLITTER_HOVER)
            }
        }
    }

    fn pane_ui(&mut self, ui: &mut egui::Ui, _tile_id: TileId, pane: &mut PanelId) -> UiResponse {
        let device_selected = self.app.roster.selected_serial.is_some();
        match pane {
            PanelId::Devices => {
                if let Some(action) =
                    panels::devices::devices_panel(ui, &self.app.roster, pane.icon())
                {
                    match action {
                        DevicesAction::Refresh => self.app.refresh_devices(),
                        DevicesAction::Select(serial) => self.app.select_device(serial),
                    }
                }
            }
            PanelId::Ram => {
                ui_elements::panel(ui, pane.icon(), pane.title(), |ui| {
                    if !device_selected {
                        ui_elements::panel_loading(ui);
                    } else {
                        panels::ram::ram_gauge_panel(ui, &self.app.metrics);
                    }
                });
            }
            PanelId::Storage => {
                ui_elements::panel(ui, pane.icon(), pane.title(), |ui| {
                    if !device_selected {
                        ui_elements::panel_loading(ui);
                    } else {
                        panels::storage::storage_gauge_panel(ui, &self.app.metrics);
                    }
                });
            }
            PanelId::LogcatAll => {
                let mut show_timestamps = self.app.logcat.show_timestamps;
                let mut auto_scroll = self.app.logcat.auto_update_feed;
                ui_elements::panel_with_footer(
                    ui,
                    pane.icon(),
                    pane.title(),
                    |_| {},
                    |ui, auto_scroll| {
                        if !device_selected {
                            ui_elements::panel_loading(ui);
                            0
                        } else {
                            panels::logcat::logcat_all_panel(ui, &mut self.app.logcat, auto_scroll)
                        }
                    },
                    &mut auto_scroll,
                    Some(&mut show_timestamps),
                );
                self.app.logcat.show_timestamps = show_timestamps;
                self.app.logcat.auto_update_feed = auto_scroll;
            }
            PanelId::Insight => {
                let mut auto_scroll = self.app.insight.auto_update_feed;
                ui_elements::panel_with_footer(
                    ui,
                    pane.icon(),
                    pane.title(),
                    |_| {},
                    |ui, auto_scroll| {
                        if !device_selected {
                            ui_elements::panel_loading(ui);
                            0
                        } else {
                            panels::insight::insight_panel(ui, &self.app.insight, auto_scroll)
                        }
                    },
                    &mut auto_scroll,
                    None,
                );
                self.app.insight.auto_update_feed = auto_scroll;
            }
            PanelId::LogcatErrors => {
                let mut show_timestamps = self.app.logcat_errors.show_timestamps;
                let mut auto_scroll = self.app.logcat_errors.auto_update_feed;
                ui_elements::panel_with_footer(
                    ui,
                    pane.icon(),
                    pane.title(),
                    |_| {},
                    |ui, auto_scroll| {
                        if !device_selected {
                            ui_elements::panel_loading(ui);
                            0
                        } else {
                            panels::logcat::logcat_errors_panel(
                                ui,
                                &mut self.app.logcat_errors,
                                auto_scroll,
                            )
                        }
                    },
                    &mut auto_scroll,
                    Some(&mut show_timestamps),
                );
                self.app.logcat_errors.show_timestamps = show_timestamps;
                self.app.logcat_errors.auto_update_feed = auto_scroll;
            }
            PanelId::SystemStats => {
                ui_elements::panel(ui, pane.icon(), pane.title(), |ui| {
                    if !device_selected {
                        ui_elements::panel_loading(ui);
                    } else {
                        panels::storage::storage_usage_panel(ui, &self.app.metrics);
                    }
                });
            }
            PanelId::Network => {
                ui_elements::panel(ui, pane.icon(), pane.title(), |ui| {
                    if !device_selected {
                        ui_elements::panel_loading(ui);
                    } else {
                        panels::network::network_panel(ui, &self.app.metrics);
                    }
                });
            }
            PanelId::Protocols => {
                ui_elements::panel(ui, pane.icon(), pane.title(), |ui| {
                    if !device_selected {
                        ui_elements::panel_loading(ui);
                    } else {
                        panels::traffic::protocols_panel(ui, &self.app.metrics);
                    }
                });
            }
        }

        UiResponse::None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_tree_json_round_trips() {
        let original = create_default_tree();
        let json = serde_json::to_string(&original).expect("serialize default tree");
        let restored: Tree<PanelId> = serde_json::from_str(&json).expect("deserialize default tree");
        assert_eq!(original, restored);
    }

    #[test]
    fn default_tree_pretty_json_has_panes_and_shares() {
        let json = serde_json::to_string_pretty(&create_default_tree()).expect("pretty json");
        assert!(
            json.contains("\"Pane\": \"Devices\"") || json.contains("\"Pane\":\"Devices\""),
            "missing Devices pane: {json}"
        );
        assert!(json.contains("\"Linear\""), "missing Linear container: {json}");
        assert!(json.contains("\"shares\""), "missing shares: {json}");
    }
}
