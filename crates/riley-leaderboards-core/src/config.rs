use serde::Deserialize;
use std::path::{Path, PathBuf};

use crate::error::{Error, Result};

#[derive(Debug, Clone, Deserialize)]
pub struct RileyLeaderboardsConfig {
    pub server: Option<ServerConfig>,
    pub database: DatabaseConfig,
    pub sync: Option<SyncConfig>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SyncConfig {
    pub repo_path: Option<String>,
    pub webhook_secret: Option<ConfigValue>,
    pub sync_branch: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ServerConfig {
    #[serde(default = "default_host")]
    pub host: String,
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default)]
    pub cors_origins: Vec<String>,
    #[serde(default)]
    pub behind_proxy: bool,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            host: default_host(),
            port: default_port(),
            cors_origins: Vec::new(),
            behind_proxy: false,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct DatabaseConfig {
    pub url: ConfigValue,
    #[serde(default = "default_max_connections")]
    pub max_connections: u32,
    #[serde(default)]
    pub schema: Option<String>,
}

/// A config value that may contain an `"env:VAR_NAME"` reference.
///
/// Stored as the raw string from the TOML file. Call `resolve()` to
/// dereference env vars.
#[derive(Debug, Clone, Deserialize)]
#[serde(transparent)]
pub struct ConfigValue(String);

impl ConfigValue {
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }

    pub fn resolve(&self) -> Result<String> {
        if let Some(var_name) = self.0.strip_prefix("env:") {
            std::env::var(var_name).map_err(|_| {
                Error::Config(format!("environment variable {var_name} not set"))
            })
        } else {
            Ok(self.0.clone())
        }
    }
}

fn default_host() -> String {
    "0.0.0.0".to_string()
}

fn default_port() -> u16 {
    8082
}

fn default_max_connections() -> u32 {
    10
}

/// Load config by searching for `riley_leaderboards.toml` in the standard
/// resolution order:
///
/// 1. Explicit path (CLI flag or `RILEY_LEADERBOARDS_CONFIG` env var)
/// 2. Current working directory
/// 3. Walk up parent directories
/// 4. `~/.config/riley_leaderboards/config.toml`
/// 5. `/etc/riley_leaderboards/config.toml`
pub fn load_config(explicit_path: Option<&Path>) -> Result<RileyLeaderboardsConfig> {
    // 1. Explicit path
    if let Some(path) = explicit_path {
        return load_from_path(path);
    }

    // 1b. Env var
    if let Ok(path) = std::env::var("RILEY_LEADERBOARDS_CONFIG") {
        return load_from_path(Path::new(&path));
    }

    // 2-3. CWD and walk up
    if let Ok(cwd) = std::env::current_dir() {
        let mut dir = cwd.as_path();
        loop {
            let candidate = dir.join("riley_leaderboards.toml");
            if candidate.exists() {
                return load_from_path(&candidate);
            }
            match dir.parent() {
                Some(parent) => dir = parent,
                None => break,
            }
        }
    }

    // 4. ~/.config
    if let Some(home) = home_dir() {
        let candidate = home.join(".config/riley_leaderboards/config.toml");
        if candidate.exists() {
            return load_from_path(&candidate);
        }
    }

    // 5. /etc
    let etc = Path::new("/etc/riley_leaderboards/config.toml");
    if etc.exists() {
        return load_from_path(etc);
    }

    Err(Error::Config(
        "no config file found (searched cwd, parent dirs, ~/.config, /etc)".to_string(),
    ))
}

fn load_from_path(path: &Path) -> Result<RileyLeaderboardsConfig> {
    let contents = std::fs::read_to_string(path).map_err(|e| {
        Error::Config(format!("failed to read config from {}: {e}", path.display()))
    })?;
    let config: RileyLeaderboardsConfig = toml::from_str(&contents).map_err(|e| {
        Error::Config(format!("failed to parse config from {}: {e}", path.display()))
    })?;
    Ok(config)
}

fn home_dir() -> Option<PathBuf> {
    std::env::var("HOME").ok().map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_value_resolves_literal() {
        let val = ConfigValue::new("postgres://localhost/test");
        assert_eq!(val.resolve().unwrap(), "postgres://localhost/test");
    }

    #[test]
    fn config_value_resolves_env() {
        // SAFETY: test runs single-threaded, no concurrent env access
        unsafe { std::env::set_var("TEST_RILEY_LB_URL", "postgres://from-env/db") };
        let val = ConfigValue::new("env:TEST_RILEY_LB_URL");
        assert_eq!(val.resolve().unwrap(), "postgres://from-env/db");
        unsafe { std::env::remove_var("TEST_RILEY_LB_URL") };
    }

    #[test]
    fn config_value_env_missing_errors() {
        let val = ConfigValue::new("env:NONEXISTENT_VAR_12345");
        assert!(val.resolve().is_err());
    }

    #[test]
    fn parse_minimal_config() {
        let toml_str = r#"
[database]
url = "postgres://localhost/leaderboards"
"#;
        let config: RileyLeaderboardsConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.database.max_connections, 10);
        assert!(config.database.schema.is_none());
        assert!(config.server.is_none());
    }

    #[test]
    fn parse_full_config() {
        let toml_str = r#"
[server]
host = "127.0.0.1"
port = 9090
cors_origins = ["https://example.com"]
behind_proxy = true

[database]
url = "postgres://localhost/leaderboards"
max_connections = 20
schema = "lb"
"#;
        let config: RileyLeaderboardsConfig = toml::from_str(toml_str).unwrap();
        let server = config.server.unwrap();
        assert_eq!(server.host, "127.0.0.1");
        assert_eq!(server.port, 9090);
        assert!(server.behind_proxy);
        assert_eq!(config.database.max_connections, 20);
        assert_eq!(config.database.schema.as_deref(), Some("lb"));
    }
}
