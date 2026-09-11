use std::collections::HashSet;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use egui_tiles::Tree;
use serde::{Deserialize, Serialize};

use crate::layout::{PanelId, create_default_tree};

const LAYOUT_VERSION: u32 = 1;
const SAVE_DEBOUNCE: Duration = Duration::from_millis(500);

#[derive(Serialize, Deserialize)]
struct LayoutFile {
    version: u32,
    tree: Tree<PanelId>,
}

/// Pretty-prints the tile tree as versioned JSON.
pub(crate) fn encode(tree: &Tree<PanelId>) -> Result<String, serde_json::Error> {
    serde_json::to_string_pretty(&LayoutFile {
        version: LAYOUT_VERSION,
        tree: tree.clone(),
    })
}

/// Parses versioned layout JSON into a tile tree.
/// Returns None when the JSON is invalid, the version is not 1, or the pane set is incomplete.
pub(crate) fn decode(json: &str) -> Option<Tree<PanelId>> {
    let file: LayoutFile = serde_json::from_str(json).ok()?;
    (file.version == LAYOUT_VERSION && panes_are_complete(&file.tree)).then_some(file.tree)
}

/// Returns true when the tree has a root and each PanelId appears exactly once.
fn panes_are_complete(tree: &Tree<PanelId>) -> bool {
    let Some(root) = tree.root else {
        return false;
    };
    if tree.tiles.get(root).is_none() {
        return false;
    }

    let mut seen = HashSet::new();
    for tile in tree.tiles.tiles() {
        if let egui_tiles::Tile::Pane(pane) = tile {
            if !seen.insert(*pane) {
                return false;
            }
        }
    }
    seen.len() == PanelId::ALL.len() && PanelId::ALL.iter().all(|pane| seen.contains(pane))
}

/// Returns the on-disk path of the layout JSON, if a config directory exists.
pub(crate) fn layout_path() -> Option<PathBuf> {
    Some(
        dirs::config_dir()?
            .join("android-dashboard")
            .join("layout.json"),
    )
}

/// Reads layout JSON from `path`. Missing or invalid files yield the default tree.
pub(crate) fn load_from_path(path: &Path) -> Tree<PanelId> {
    match fs::read_to_string(path) {
        Err(err) if err.kind() == ErrorKind::NotFound => create_default_tree(),
        Err(err) => {
            tracing::warn!(
                error = %err,
                path = %path.display(),
                "failed to read layout"
            );
            create_default_tree()
        }
        Ok(json) => decode(&json).unwrap_or_else(|| {
            tracing::warn!(path = %path.display(), "invalid layout file");
            create_default_tree()
        }),
    }
}

/// Writes versioned layout JSON to `path` via a sibling `.tmp` file.
pub(crate) fn save_to_path(path: &Path, tree: &Tree<PanelId>) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
        }
    }
    let json = encode(tree).map_err(|err| std::io::Error::new(ErrorKind::InvalidData, err))?;
    let tmp = tmp_path(path);
    fs::write(&tmp, json)?;
    fs::rename(&tmp, path)?;
    Ok(())
}

/// Loads the saved layout, or the default tree when none is available.
pub(crate) fn load_or_default() -> Tree<PanelId> {
    match layout_path() {
        Some(path) => load_from_path(&path),
        None => create_default_tree(),
    }
}

/// Writes the in-memory tree to the layout JSON path.
/// Returns true when the file was written.
pub(crate) fn save(tree: &Tree<PanelId>) -> bool {
    let Some(path) = layout_path() else {
        tracing::warn!("no config directory; layout not saved");
        return false;
    };
    if let Err(err) = save_to_path(&path, tree) {
        tracing::warn!(
            error = %err,
            path = %path.display(),
            "failed to save layout"
        );
        return false;
    }
    true
}

/// Debounces layout writes until edits have been idle.
pub(crate) struct LayoutSaver {
    dirty: bool,
    last_edit: Instant,
}

impl Default for LayoutSaver {
    fn default() -> Self {
        Self {
            dirty: false,
            last_edit: Instant::now(),
        }
    }
}

impl LayoutSaver {
    /// Records that the tile tree changed. Restarts the save delay.
    pub(crate) fn mark_edit(&mut self) {
        self.dirty = true;
        self.last_edit = Instant::now();
    }

    /// Writes the tree once the save delay has elapsed since the last edit.
    pub(crate) fn tick(&mut self, ctx: &egui::Context, tree: &Tree<PanelId>) {
        if !self.dirty {
            return;
        }
        let wait = SAVE_DEBOUNCE.saturating_sub(self.last_edit.elapsed());
        if wait.is_zero() {
            if save(tree) {
                self.dirty = false;
            } else {
                ctx.request_repaint_after(SAVE_DEBOUNCE);
            }
        } else {
            ctx.request_repaint_after(wait);
        }
    }

    /// Writes a pending layout immediately. Clears dirty even if the write fails.
    pub(crate) fn flush(&mut self, tree: &Tree<PanelId>) {
        if !self.dirty {
            return;
        }
        save(tree);
        self.dirty = false;
    }

    /// Drops a pending write without saving.
    pub(crate) fn clear_dirty(&mut self) {
        self.dirty = false;
    }
}

fn tmp_path(path: &Path) -> PathBuf {
    let mut tmp = path.as_os_str().to_os_string();
    tmp.push(".tmp");
    PathBuf::from(tmp)
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;

    static TEMP_SEQ: AtomicU64 = AtomicU64::new(0);

    fn temp_layout_path() -> PathBuf {
        let n = TEMP_SEQ.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "android-dashboard-layout-test-{}-{n}.json",
            std::process::id()
        ))
    }

    struct TempLayout(PathBuf);

    impl TempLayout {
        fn new() -> Self {
            Self(temp_layout_path())
        }
    }

    impl Drop for TempLayout {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.0);
            let _ = fs::remove_file(tmp_path(&self.0));
        }
    }

    fn tree_without_logcat_all() -> Tree<PanelId> {
        let mut tree = create_default_tree();
        let ids: Vec<_> = tree
            .tiles
            .iter()
            .filter_map(|(id, tile)| match tile {
                egui_tiles::Tile::Pane(PanelId::LogcatAll) => Some(*id),
                _ => None,
            })
            .collect();
        for id in ids {
            tree.tiles.remove(id);
        }
        tree
    }

    #[test]
    fn encode_decode_default_tree_round_trips() {
        let original = create_default_tree();
        let json = encode(&original).expect("encode");
        let restored = decode(&json).expect("decode");
        assert_eq!(original, restored);
    }

    #[test]
    fn decode_rejects_garbage() {
        assert!(decode("not json").is_none());
    }

    #[test]
    fn decode_rejects_other_version() {
        let mut value: serde_json::Value =
            serde_json::from_str(&encode(&create_default_tree()).expect("encode")).expect("json");
        value["version"] = serde_json::json!(2);
        assert!(decode(&value.to_string()).is_none());
    }

    #[test]
    fn decode_rejects_missing_pane() {
        let json = encode(&tree_without_logcat_all()).expect("encode");
        assert!(decode(&json).is_none());
    }

    #[test]
    fn load_missing_path_yields_default_tree() {
        let temp = TempLayout::new();
        assert!(!temp.0.exists());
        assert_eq!(load_from_path(&temp.0), create_default_tree());
    }

    #[test]
    fn save_then_load_round_trips() {
        let temp = TempLayout::new();
        let original = create_default_tree();
        save_to_path(&temp.0, &original).expect("save");
        assert_eq!(load_from_path(&temp.0), original);
    }
}
