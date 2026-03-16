//! AttaConfig — configuration loaded from `$ATTA_HOME/etc/attaos.yaml`
//!
//! If the config file is missing, all defaults apply (no error).
//! If the file exists but fields are missing, serde defaults fill in.

use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// Top-level configuration for AttaOS.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct AttaConfig {
    /// HTTP server settings
    pub server: ServerConfig,
    /// LLM provider settings
    pub llm: LlmConfig,
    /// Logging settings
    pub log: LogConfig,
    /// Community skill sync settings
    pub skill_sync: SkillSyncConfig,
    /// Channel configurations
    pub channels: Vec<atta_channel::ChannelConfig>,
    /// Authentication settings
    pub auth: AuthConfig,
}

impl AttaConfig {
    /// Load config from a YAML file. Returns defaults if the file doesn't exist.
    pub fn load(path: &Path) -> Result<Self> {
        if !path.exists() {
            tracing::info!(
                path = %path.display(),
                "config file not found, using defaults"
            );
            return Ok(Self::default());
        }

        let content = std::fs::read_to_string(path)
            .with_context(|| format!("failed to read config file: {}", path.display()))?;

        let config: AttaConfig = serde_yml::from_str(&content)
            .with_context(|| format!("failed to parse config file: {}", path.display()))?;

        tracing::info!(path = %path.display(), "loaded config");
        Ok(config)
    }
}

/// HTTP server configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ServerConfig {
    /// Listening port
    pub port: u16,
    /// Bind address
    pub bind: String,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            port: 3000,
            bind: "127.0.0.1".to_string(),
        }
    }
}

/// LLM provider configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct LlmConfig {
    /// Preferred provider name (anthropic, openai, deepseek)
    pub provider: String,
    /// Model ID override
    pub model: String,
}

impl Default for LlmConfig {
    fn default() -> Self {
        Self {
            provider: "auto".to_string(),
            model: String::new(),
        }
    }
}

/// Logging configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct LogConfig {
    /// Log level (trace, debug, info, warn, error)
    pub level: String,
}

impl Default for LogConfig {
    fn default() -> Self {
        Self {
            level: "info".to_string(),
        }
    }
}

/// Community skill sync configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SkillSyncConfig {
    /// Whether skill sync is enabled
    pub enabled: bool,
    /// Remote repository URL
    pub repo_url: String,
    /// Sync interval in seconds (default: 7 days)
    pub interval_secs: u64,
}

impl Default for SkillSyncConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            repo_url: "https://github.com/besoeasy/open-skills".to_string(),
            interval_secs: 7 * 24 * 60 * 60, // 7 days
        }
    }
}

/// Authentication configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AuthConfig {
    /// Auth mode: "none" (desktop default), "oidc", "api_key"
    pub mode: String,
    /// OIDC issuer URL (required when mode = "oidc")
    pub issuer: Option<String>,
    /// OIDC audience (required when mode = "oidc")
    pub audience: Option<String>,
    /// OIDC secret or RSA public key PEM (required when mode = "oidc")
    pub secret: Option<String>,
}

impl Default for AuthConfig {
    fn default() -> Self {
        Self {
            mode: "none".to_string(),
            issuer: None,
            audience: None,
            secret: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = AttaConfig::default();
        assert_eq!(config.server.port, 3000);
        assert_eq!(config.server.bind, "127.0.0.1");
        assert_eq!(config.log.level, "info");
        assert!(config.skill_sync.enabled);
        assert!(config.channels.is_empty());
    }

    #[test]
    fn test_load_missing_file() {
        let config = AttaConfig::load(Path::new("/nonexistent/attaos.yaml")).unwrap();
        assert_eq!(config.server.port, 3000);
    }

    #[test]
    fn test_deserialize_partial_yaml() {
        let yaml = "server:\n  port: 8080\n";
        let config: AttaConfig = serde_yml::from_str(yaml).unwrap();
        assert_eq!(config.server.port, 8080);
        assert_eq!(config.server.bind, "127.0.0.1"); // default
        assert_eq!(config.log.level, "info"); // default
    }

    #[test]
    fn test_deserialize_with_channels() {
        let yaml = r#"
channels:
  - type: terminal
    enabled: true
  - type: webhook
    enabled: false
    settings:
      outgoing_url: "http://localhost:9999/hook"
      name: "my-webhook"
"#;
        let config: AttaConfig = serde_yml::from_str(yaml).unwrap();
        assert_eq!(config.channels.len(), 2);
        assert_eq!(config.channels[0].channel_type, "terminal");
        assert!(config.channels[0].enabled);
        assert_eq!(config.channels[1].channel_type, "webhook");
        assert!(!config.channels[1].enabled);
    }

    #[test]
    fn test_channels_default_enabled() {
        let yaml = r#"
channels:
  - type: terminal
"#;
        let config: AttaConfig = serde_yml::from_str(yaml).unwrap();
        assert_eq!(config.channels.len(), 1);
        assert!(config.channels[0].enabled); // defaults to true
    }

    #[test]
    fn test_deserialize_auth_config() {
        let yaml = r#"
auth:
  mode: oidc
  issuer: "https://auth.example.com"
  audience: "attaos"
  secret: "my-secret-key"
"#;
        let config: AttaConfig = serde_yml::from_str(yaml).unwrap();
        assert_eq!(config.auth.mode, "oidc");
        assert_eq!(config.auth.issuer.as_deref(), Some("https://auth.example.com"));
        assert_eq!(config.auth.audience.as_deref(), Some("attaos"));
    }

    #[test]
    fn test_default_auth_is_none() {
        let config = AttaConfig::default();
        assert_eq!(config.auth.mode, "none");
        assert!(config.auth.issuer.is_none());
    }
}
