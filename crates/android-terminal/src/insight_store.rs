use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use ai_insight::InsightConfig;
use serde::{Deserialize, Serialize};

const INSIGHT_VERSION: u32 = 1;

#[derive(Serialize, Deserialize)]
struct InsightFile {
    version: u32,
    base_url: String,
    model: String,
    api_key: String,
}

impl InsightFile {
    fn from_config(config: &InsightConfig) -> Self {
        Self {
            version: INSIGHT_VERSION,
            base_url: config.base_url.clone(),
            model: config.model.clone(),
            api_key: config.api_key.clone(),
        }
    }

    fn into_config(self) -> Option<InsightConfig> {
        (self.version == INSIGHT_VERSION).then_some(InsightConfig {
            base_url: self.base_url,
            model: self.model,
            api_key: self.api_key,
        })
    }
}

/// Returns the on-disk path of the insight JSON, if a config directory exists.
pub(crate) fn insight_path() -> Option<PathBuf> {
    Some(
        dirs::config_dir()?
            .join("android-terminal")
            .join("insight.json"),
    )
}

/// Reads insight JSON from `path`. Missing or invalid files yield an empty config.
pub(crate) fn load_from_path(path: &Path) -> InsightConfig {
    match fs::read_to_string(path) {
        Err(err) if err.kind() == ErrorKind::NotFound => InsightConfig::default(),
        Err(err) => {
            tracing::warn!(
                error = %err,
                path = %path.display(),
                "failed to read insight config"
            );
            InsightConfig::default()
        }
        Ok(json) => decode(&json).unwrap_or_else(|| {
            tracing::warn!(path = %path.display(), "invalid insight config file");
            InsightConfig::default()
        }),
    }
}

/// Writes insight JSON to `path` via a sibling `.tmp` file.
pub(crate) fn save_to_path(path: &Path, config: &InsightConfig) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
        }
    }
    let json = encode(config).map_err(|err| std::io::Error::new(ErrorKind::InvalidData, err))?;
    let tmp = tmp_path(path);
    fs::write(&tmp, json)?;
    fs::rename(&tmp, path)?;
    Ok(())
}

/// Loads the saved insight config, or empty when none is available.
pub(crate) fn load_or_default() -> InsightConfig {
    match insight_path() {
        Some(path) => load_from_path(&path),
        None => InsightConfig::default(),
    }
}

/// Writes the config to the insight JSON path.
/// Returns true when the file was written.
pub(crate) fn save(config: &InsightConfig) -> bool {
    let Some(path) = insight_path() else {
        tracing::warn!("no config directory; insight config not saved");
        return false;
    };
    if let Err(err) = save_to_path(&path, config) {
        tracing::warn!(
            error = %err,
            path = %path.display(),
            "failed to save insight config"
        );
        return false;
    }
    true
}

fn encode(config: &InsightConfig) -> Result<String, serde_json::Error> {
    serde_json::to_string_pretty(&InsightFile::from_config(config))
}

fn decode(json: &str) -> Option<InsightConfig> {
    serde_json::from_str::<InsightFile>(json)
        .ok()
        .and_then(InsightFile::into_config)
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

    fn temp_path() -> PathBuf {
        let n = TEMP_SEQ.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "android-terminal-insight-test-{}-{n}.json",
            std::process::id()
        ))
    }

    struct TempFile(PathBuf);

    impl TempFile {
        fn new() -> Self {
            Self(temp_path())
        }
    }

    impl Drop for TempFile {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.0);
            let _ = fs::remove_file(tmp_path(&self.0));
        }
    }

    fn sample() -> InsightConfig {
        InsightConfig {
            api_key: "sk-test".into(),
            base_url: "https://api.example.com".into(),
            model: "example-model".into(),
        }
    }

    #[test]
    fn encode_decode_round_trips() {
        let original = sample();
        let json = encode(&original).expect("encode");
        let restored = decode(&json).expect("decode");
        assert_eq!(restored.api_key, original.api_key);
        assert_eq!(restored.base_url, original.base_url);
        assert_eq!(restored.model, original.model);
    }

    #[test]
    fn decode_rejects_garbage() {
        assert!(decode("not json").is_none());
    }

    #[test]
    fn decode_rejects_other_version() {
        let mut value: serde_json::Value =
            serde_json::from_str(&encode(&sample()).expect("encode")).expect("json");
        value["version"] = serde_json::json!(2);
        assert!(decode(&value.to_string()).is_none());
    }

    #[test]
    fn load_missing_path_yields_empty() {
        let temp = TempFile::new();
        assert!(!temp.0.exists());
        let config = load_from_path(&temp.0);
        assert!(!config.is_configured());
    }

    #[test]
    fn save_then_load_round_trips() {
        let temp = TempFile::new();
        let original = sample();
        save_to_path(&temp.0, &original).expect("save");
        let loaded = load_from_path(&temp.0);
        assert_eq!(loaded.api_key, original.api_key);
        assert_eq!(loaded.base_url, original.base_url);
        assert_eq!(loaded.model, original.model);
    }
}
