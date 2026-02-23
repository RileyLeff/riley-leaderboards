use serde::Deserialize;
use std::path::{Path, PathBuf};

use crate::error::{Error, Result};

#[derive(Debug, Clone, Deserialize)]
pub struct RileyLeaderboardsConfig {
    pub server: Option<ServerConfig>,
    pub database: DatabaseConfig,
    pub redis: Option<RedisConfig>,
    pub auth: Option<AuthConfig>,
    pub sync: Option<SyncConfig>,
    /// Outbound webhook destinations, notified on board/version events.
    #[serde(default)]
    pub webhooks: Vec<WebhookConfig>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RedisConfig {
    pub url: ConfigValue,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WebhookConfig {
    pub url: String,
    pub events: Vec<WebhookEvent>,
    /// Optional board slug patterns (glob-style). Empty = all boards.
    #[serde(default)]
    pub boards: Vec<String>,
    /// Optional HMAC-SHA256 secret for signing payloads.
    pub secret: Option<ConfigValue>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub enum WebhookEvent {
    #[serde(rename = "version.created")]
    VersionCreated,
    #[serde(rename = "board.created")]
    BoardCreated,
    #[serde(rename = "board.updated")]
    BoardUpdated,
    #[serde(rename = "board.deleted")]
    BoardDeleted,
}

impl WebhookEvent {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::VersionCreated => "version.created",
            Self::BoardCreated => "board.created",
            Self::BoardUpdated => "board.updated",
            Self::BoardDeleted => "board.deleted",
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct AuthConfig {
    pub jwks_url: Option<String>,
    pub required_role: Option<String>,
    /// Admin token — full read/write access. Alias: `api_token` (v1 compat).
    pub admin_token: Option<ConfigValue>,
    /// Legacy alias for `admin_token`. If both are set, startup fails.
    pub api_token: Option<ConfigValue>,
    /// Read-only tokens — can fetch boards, versions, entries, references.
    #[serde(default)]
    pub read_tokens: Vec<ConfigValue>,
    /// Whether reads require authentication. Default: false (public reads).
    #[serde(default)]
    pub require_read_auth: bool,
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
    /// Requests per second per IP. 0 = disabled.
    #[serde(default)]
    pub rate_limit_per_second: u64,
    /// Rate limit burst size.
    #[serde(default = "default_rate_limit_burst")]
    pub rate_limit_burst: u32,
    /// Enable SSE streaming endpoint for live board updates.
    #[serde(default)]
    pub sse_enabled: bool,
    /// Maximum concurrent SSE connections per server.
    #[serde(default = "default_sse_max_connections")]
    pub sse_max_connections: usize,
    /// Minimum interval (ms) between score.updated SSE events per board.
    #[serde(default = "default_sse_score_debounce_ms")]
    pub sse_score_debounce_ms: u64,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            host: default_host(),
            port: default_port(),
            cors_origins: Vec::new(),
            behind_proxy: false,
            rate_limit_per_second: 0,
            rate_limit_burst: default_rate_limit_burst(),
            sse_enabled: false,
            sse_max_connections: default_sse_max_connections(),
            sse_score_debounce_ms: default_sse_score_debounce_ms(),
        }
    }
}

fn default_rate_limit_burst() -> u32 {
    50
}

fn default_sse_max_connections() -> usize {
    1000
}

fn default_sse_score_debounce_ms() -> u64 {
    1000
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
    validate_webhook_board_patterns(&config)?;
    Ok(config)
}

/// Reject board filter patterns with `*` in unsupported positions.
/// Only `*` as standalone or as a trailing suffix (e.g., `dc-*`) is supported.
fn validate_webhook_board_patterns(config: &RileyLeaderboardsConfig) -> Result<()> {
    for (i, wh) in config.webhooks.iter().enumerate() {
        for pattern in &wh.boards {
            if pattern == "*" {
                continue;
            }
            if let Some(prefix) = pattern.strip_suffix('*') {
                if prefix.contains('*') {
                    return Err(Error::Config(format!(
                        "webhooks[{i}]: board pattern \"{pattern}\" has unsupported '*' placement \
                         (only trailing '*' is supported, e.g., \"dc-*\")"
                    )));
                }
            } else if pattern.contains('*') {
                return Err(Error::Config(format!(
                    "webhooks[{i}]: board pattern \"{pattern}\" has unsupported '*' placement \
                     (only trailing '*' is supported, e.g., \"dc-*\")"
                )));
            }
        }
    }
    Ok(())
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

    #[test]
    fn parse_webhook_config() {
        let toml_str = r#"
[database]
url = "postgres://localhost/leaderboards"

[[webhooks]]
url = "https://api.netlify.com/build_hooks/abc123"
events = ["version.created"]
boards = ["dc-*", "nfl-*"]

[[webhooks]]
url = "https://api.vercel.com/v1/deploy/xyz"
events = ["version.created", "board.created"]
secret = "env:OUTBOUND_WEBHOOK_SECRET"
"#;
        let config: RileyLeaderboardsConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.webhooks.len(), 2);

        let wh0 = &config.webhooks[0];
        assert_eq!(wh0.url, "https://api.netlify.com/build_hooks/abc123");
        assert_eq!(wh0.events, vec![WebhookEvent::VersionCreated]);
        assert_eq!(wh0.boards, vec!["dc-*", "nfl-*"]);
        assert!(wh0.secret.is_none());

        let wh1 = &config.webhooks[1];
        assert_eq!(wh1.events, vec![WebhookEvent::VersionCreated, WebhookEvent::BoardCreated]);
        assert!(wh1.secret.is_some());
        assert!(wh1.boards.is_empty());
    }

    #[test]
    fn parse_config_no_webhooks_defaults_to_empty() {
        let toml_str = r#"
[database]
url = "postgres://localhost/leaderboards"
"#;
        let config: RileyLeaderboardsConfig = toml::from_str(toml_str).unwrap();
        assert!(config.webhooks.is_empty());
    }

    #[test]
    fn validate_webhook_board_patterns_rejects_leading_star() {
        let config = RileyLeaderboardsConfig {
            server: None,
            database: DatabaseConfig {
                url: ConfigValue::new("postgres://localhost/test"),
                max_connections: 2,
                schema: None,
            },
            redis: None,
            auth: None,
            sync: None,
            webhooks: vec![WebhookConfig {
                url: "https://example.com".to_string(),
                events: vec![WebhookEvent::VersionCreated],
                boards: vec!["*-rankings".to_string()],
                secret: None,
            }],
        };
        let err = validate_webhook_board_patterns(&config).unwrap_err();
        assert!(err.to_string().contains("unsupported '*' placement"));
    }

    #[test]
    fn validate_webhook_board_patterns_rejects_middle_star() {
        let config = RileyLeaderboardsConfig {
            server: None,
            database: DatabaseConfig {
                url: ConfigValue::new("postgres://localhost/test"),
                max_connections: 2,
                schema: None,
            },
            redis: None,
            auth: None,
            sync: None,
            webhooks: vec![WebhookConfig {
                url: "https://example.com".to_string(),
                events: vec![WebhookEvent::VersionCreated],
                boards: vec!["dc-*-rankings".to_string()],
                secret: None,
            }],
        };
        let err = validate_webhook_board_patterns(&config).unwrap_err();
        assert!(err.to_string().contains("unsupported '*' placement"));
    }

    #[test]
    fn validate_webhook_board_patterns_accepts_valid() {
        let config = RileyLeaderboardsConfig {
            server: None,
            database: DatabaseConfig {
                url: ConfigValue::new("postgres://localhost/test"),
                max_connections: 2,
                schema: None,
            },
            redis: None,
            auth: None,
            sync: None,
            webhooks: vec![WebhookConfig {
                url: "https://example.com".to_string(),
                events: vec![WebhookEvent::VersionCreated],
                boards: vec!["dc-*".to_string(), "*".to_string(), "exact-match".to_string()],
                secret: None,
            }],
        };
        validate_webhook_board_patterns(&config).unwrap();
    }
}
