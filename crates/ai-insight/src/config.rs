//! Chat Completions settings from the process environment.

const DEFAULT_BASE_URL: &str = "https://api.deepseek.com";
const DEFAULT_MODEL: &str = "deepseek-v4-pro";

/// Chat Completions endpoint, model, and API key for one request.
#[derive(Debug, Clone)]
pub struct InsightConfig {
    pub api_key: String,
    pub base_url: String,
    pub model: String,
}

impl InsightConfig {
    /// Reads `AI_PROVIDER_API_KEY`, `AI_PROVIDER_BASE_URL`, and `AI_PROVIDER_MODEL` from the environment.
    pub fn from_env() -> Self {
        Self {
            api_key: std::env::var("AI_PROVIDER_API_KEY").unwrap_or_default(),
            base_url: std::env::var("AI_PROVIDER_BASE_URL")
                .unwrap_or_else(|_| DEFAULT_BASE_URL.into()),
            model: std::env::var("AI_PROVIDER_MODEL").unwrap_or_else(|_| DEFAULT_MODEL.into()),
        }
    }

    /// Returns true when `api_key` is non-empty after trim.
    pub fn has_api_key(&self) -> bool {
        !self.api_key.trim().is_empty()
    }

    /// Builds `{base_url}/chat/completions` with no trailing slash on the base.
    pub fn completions_url(&self) -> String {
        format!(
            "{}/chat/completions",
            self.base_url.trim().trim_end_matches('/')
        )
    }

    /// Returns the host portion of `base_url` for logs (no scheme, no path).
    pub fn host_for_log(&self) -> String {
        self.base_url
            .trim()
            .trim_start_matches("https://")
            .trim_start_matches("http://")
            .split('/')
            .next()
            .unwrap_or(self.base_url.trim())
            .to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn completions_url_strips_trailing_slash() {
        let config = InsightConfig {
            api_key: String::new(),
            base_url: "https://api.deepseek.com/".into(),
            model: DEFAULT_MODEL.into(),
        };
        assert_eq!(
            config.completions_url(),
            "https://api.deepseek.com/chat/completions"
        );
    }

    #[test]
    fn host_for_log_drops_scheme() {
        let config = InsightConfig {
            api_key: String::new(),
            base_url: "https://api.deepseek.com/v1".into(),
            model: DEFAULT_MODEL.into(),
        };
        assert_eq!(config.host_for_log(), "api.deepseek.com");
    }

    #[test]
    fn has_api_key_rejects_blank() {
        let config = InsightConfig {
            api_key: "  ".into(),
            base_url: DEFAULT_BASE_URL.into(),
            model: DEFAULT_MODEL.into(),
        };
        assert!(!config.has_api_key());
    }
}
