use crate::error::{DeviceOpsError, Result};
use serde::Deserialize;
use std::path::PathBuf;

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub security: SecurityConfig,
    pub execution: ExecutionConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SecurityConfig {
    pub enabled: bool,
    #[serde(default)]
    pub command_allowlist: Vec<String>,
    #[serde(default)]
    pub path_allowlist: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ExecutionConfig {
    #[serde(default = "default_timeout")]
    pub default_timeout: u64,
}

fn default_timeout() -> u64 {
    300 // 5 minutes
}

impl Config {
    pub fn load(path: Option<PathBuf>) -> Result<Self> {
        let config_path =
            path.unwrap_or_else(|| PathBuf::from("/greengrass/v2/config/device-ops-config.json"));

        if !config_path.exists() {
            tracing::warn!("Config file not found, using defaults");
            return Ok(Self::default());
        }

        let content = std::fs::read_to_string(&config_path)
            .map_err(|e| DeviceOpsError::ConfigError(format!("Failed to read config: {}", e)))?;

        serde_json::from_str(&content)
            .map_err(|e| DeviceOpsError::ConfigError(format!("Failed to parse config: {}", e)))
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            security: SecurityConfig {
                enabled: false,
                command_allowlist: vec![],
                path_allowlist: vec![],
            },
            execution: ExecutionConfig {
                default_timeout: default_timeout(),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = Config::default();
        assert_eq!(config.execution.default_timeout, 300);
        assert!(!config.security.enabled);
    }
}
