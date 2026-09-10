//! Chat Completions settings from the process environment.

/// Chat Completions endpoint, model, and API key for one request.
#[derive(Debug, Clone)]
pub struct InsightConfig {
    pub api_key: String,
    pub base_url: String,
    pub model: String,
}

impl InsightConfig {
    /// Reads `AI_PROVIDER_API_KEY`, `AI_PROVIDER_BASE_URL`, and `AI_PROVIDER_MODEL` from the environment.
    ///
    /// Missing variables become empty strings. Insight runs only when all three are non-blank.
    pub fn from_env() -> Self {
        Self {
            api_key: std::env::var("AI_PROVIDER_API_KEY").unwrap_or_default(),
            base_url: std::env::var("AI_PROVIDER_BASE_URL").unwrap_or_default(),
            model: std::env::var("AI_PROVIDER_MODEL").unwrap_or_default(),
        }
    }

    /// Returns true when `api_key` is non-empty after trim.
    pub fn has_api_key(&self) -> bool {
        !self.api_key.trim().is_empty()
    }

    /// Returns true when API key, base URL, and model are all non-empty after trim.
    pub fn is_configured(&self) -> bool {
        self.has_api_key() && !self.base_url.trim().is_empty() && !self.model.trim().is_empty()
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

    fn config(api_key: &str, base_url: &str, model: &str) -> InsightConfig {
        InsightConfig {
            api_key: api_key.into(),
            base_url: base_url.into(),
            model: model.into(),
        }
    }

    #[test]
    fn completions_url_strips_trailing_slash() {
        let config = config("", "https://api.example.com/", "example-model");
        assert_eq!(
            config.completions_url(),
            "https://api.example.com/chat/completions"
        );
    }

    #[test]
    fn host_for_log_drops_scheme() {
        let config = config("", "https://api.example.com/v1", "example-model");
        assert_eq!(config.host_for_log(), "api.example.com");
    }

    #[test]
    fn has_api_key_rejects_blank() {
        assert!(!config("  ", "https://api.example.com", "example-model").has_api_key());
    }

    #[test]
    fn is_configured_requires_all_three() {
        assert!(!config("", "", "").is_configured());
        assert!(!config("sk-test", "", "").is_configured());
        assert!(!config("sk-test", "https://api.example.com", "").is_configured());
        assert!(!config("sk-test", "", "example-model").is_configured());
        assert!(!config("  ", "https://api.example.com", "example-model").is_configured());
        assert!(!config("sk-test", "  ", "example-model").is_configured());
        assert!(!config("sk-test", "https://api.example.com", "  ").is_configured());
        assert!(config("sk-test", "https://api.example.com", "example-model").is_configured());
    }
}
