use std::sync::LazyLock;

use chrono::{DateTime, Utc};
use hmac::{Hmac, Mac};
use serde::Serialize;
use sha2::Sha256;
use tokio::task::JoinHandle;

use riley_leaderboards_core::config::{WebhookConfig, WebhookEvent};

type HmacSha256 = Hmac<Sha256>;

/// Shared HTTP client for all webhook deliveries. Reuses connection pool and TLS sessions.
static HTTP_CLIENT: LazyLock<reqwest::Client> = LazyLock::new(|| {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .expect("failed to build reqwest client")
});

/// Payload sent to outbound webhook endpoints.
#[derive(Debug, Serialize)]
pub struct WebhookPayload {
    pub event: &'static str,
    pub timestamp: String,
    pub board: BoardInfo,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<VersionInfo>,
}

#[derive(Debug, Serialize)]
pub struct BoardInfo {
    pub slug: String,
    pub name: String,
}

#[derive(Debug, Serialize)]
pub struct VersionInfo {
    pub version_number: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// Fire outbound webhooks for the given event and board.
/// Returns a `Vec<JoinHandle<()>>` — server call sites can ignore these (fire-and-forget),
/// while CLI call sites can `join_all()` to ensure delivery before exit.
/// The `timestamp` parameter allows passing the DB-sourced timestamp (e.g. `created_at`);
/// when `None`, falls back to `Utc::now()`.
pub fn fire(
    configs: &[WebhookConfig],
    event: WebhookEvent,
    board_slug: &str,
    board_name: &str,
    version_info: Option<VersionInfo>,
    timestamp: Option<DateTime<Utc>>,
) -> Vec<JoinHandle<()>> {
    let ts = timestamp.unwrap_or_else(Utc::now).to_rfc3339();
    let payload = WebhookPayload {
        event: event.as_str(),
        timestamp: ts,
        board: BoardInfo {
            slug: board_slug.to_string(),
            name: board_name.to_string(),
        },
        version: version_info,
    };

    let body = match serde_json::to_vec(&payload) {
        Ok(b) => b,
        Err(e) => {
            tracing::error!("failed to serialize webhook payload: {e}");
            return vec![];
        }
    };

    let mut handles = Vec::new();

    for config in configs {
        if !config.events.contains(&event) {
            continue;
        }
        if !matches_board_filter(&config.boards, board_slug) {
            continue;
        }

        let url = config.url.clone();
        let body = body.clone();
        let secret = match &config.secret {
            Some(cv) => match cv.resolve() {
                Ok(s) => Some(s),
                Err(e) => {
                    tracing::error!(
                        "webhook to {}: secret resolution failed ({e}), skipping delivery",
                        config.url
                    );
                    continue;
                }
            },
            None => None,
        };

        handles.push(tokio::spawn(async move {
            deliver(&url, &body, secret.as_deref()).await;
        }));
    }

    handles
}

/// Check if a board slug matches the webhook's board filter patterns.
/// Empty filter means "all boards".
pub fn matches_board_filter(patterns: &[String], slug: &str) -> bool {
    if patterns.is_empty() {
        return true;
    }
    patterns.iter().any(|pattern| glob_match(pattern, slug))
}

/// Simple glob matching: `*` matches any sequence of characters.
/// Only supports `*` at the end of a pattern (e.g., `dc-*`) or as a
/// standalone wildcard. No `?` or `[...]` support — keeps it simple.
fn glob_match(pattern: &str, value: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    if let Some(prefix) = pattern.strip_suffix('*') {
        value.starts_with(prefix)
    } else {
        pattern == value
    }
}

/// Deliver a webhook payload with retries.
/// 3 attempts with backoff: 1s, 5s.
/// 10 second timeout per attempt.
async fn deliver(url: &str, body: &[u8], secret: Option<&str>) {
    let client = &*HTTP_CLIENT;
    let delays = [1, 5, 0]; // 0 = final attempt, no sleep after

    for (attempt, delay_secs) in delays.iter().enumerate() {
        let mut request = client
            .post(url)
            .header("Content-Type", "application/json")
            .body(body.to_vec());

        if let Some(secret) = secret {
            if let Some(sig) = compute_signature(secret, body) {
                request = request.header("X-Webhook-Signature-256", format!("sha256={sig}"));
            }
        }

        match request.send().await {
            Ok(resp) if resp.status().is_success() => {
                tracing::debug!("webhook delivered to {url} (attempt {})", attempt + 1);
                return;
            }
            Ok(resp) if resp.status().is_client_error() => {
                tracing::warn!(
                    "webhook to {url} returned {} (not retrying)",
                    resp.status()
                );
                return;
            }
            Ok(resp) => {
                tracing::warn!(
                    "webhook to {url} returned {} (attempt {})",
                    resp.status(),
                    attempt + 1
                );
            }
            Err(e) => {
                tracing::warn!("webhook to {url} failed: {e} (attempt {})", attempt + 1);
            }
        }

        if *delay_secs > 0 {
            tokio::time::sleep(std::time::Duration::from_secs(*delay_secs)).await;
        }
    }

    tracing::error!("webhook to {url} exhausted all retries");
}

/// Compute HMAC-SHA256 signature of the payload body.
fn compute_signature(secret: &str, body: &[u8]) -> Option<String> {
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).ok()?;
    mac.update(body);
    Some(hex::encode(mac.finalize().into_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glob_match_exact() {
        assert!(glob_match("dc-sandwiches", "dc-sandwiches"));
        assert!(!glob_match("dc-sandwiches", "dc-pizza"));
    }

    #[test]
    fn glob_match_wildcard_suffix() {
        assert!(glob_match("dc-*", "dc-sandwiches"));
        assert!(glob_match("dc-*", "dc-pizza"));
        assert!(!glob_match("dc-*", "nfl-rankings"));
    }

    #[test]
    fn glob_match_standalone_wildcard() {
        assert!(glob_match("*", "anything"));
        assert!(glob_match("*", ""));
    }

    #[test]
    fn matches_board_filter_empty_matches_all() {
        assert!(matches_board_filter(&[], "any-board"));
    }

    #[test]
    fn matches_board_filter_with_patterns() {
        let patterns = vec!["dc-*".to_string(), "nfl-*".to_string()];
        assert!(matches_board_filter(&patterns, "dc-sandwiches"));
        assert!(matches_board_filter(&patterns, "nfl-rankings"));
        assert!(!matches_board_filter(&patterns, "mlb-standings"));
    }

    #[test]
    fn matches_board_filter_exact_match() {
        let patterns = vec!["dc-sandwiches".to_string()];
        assert!(matches_board_filter(&patterns, "dc-sandwiches"));
        assert!(!matches_board_filter(&patterns, "dc-pizza"));
    }

    #[test]
    fn compute_signature_produces_hex() {
        let sig = compute_signature("test-secret", b"hello world").unwrap();
        // Verify it's valid hex of the right length (64 chars = 32 bytes = SHA256)
        assert_eq!(sig.len(), 64);
        assert!(sig.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn compute_signature_is_deterministic() {
        let sig1 = compute_signature("secret", b"payload").unwrap();
        let sig2 = compute_signature("secret", b"payload").unwrap();
        assert_eq!(sig1, sig2);
    }

    #[test]
    fn compute_signature_differs_with_different_secrets() {
        let sig1 = compute_signature("secret-1", b"payload").unwrap();
        let sig2 = compute_signature("secret-2", b"payload").unwrap();
        assert_ne!(sig1, sig2);
    }

    #[test]
    fn payload_serialization_version_created() {
        let payload = WebhookPayload {
            event: "version.created",
            timestamp: "2026-02-23T12:00:00Z".to_string(),
            board: BoardInfo {
                slug: "dc-sandwiches".to_string(),
                name: "Best Sandwiches in DC".to_string(),
            },
            version: Some(VersionInfo {
                version_number: 3,
                note: Some("Added Mangialardo's".to_string()),
            }),
        };

        let json: serde_json::Value = serde_json::to_value(&payload).unwrap();
        assert_eq!(json["event"], "version.created");
        assert_eq!(json["board"]["slug"], "dc-sandwiches");
        assert_eq!(json["board"]["name"], "Best Sandwiches in DC");
        assert_eq!(json["version"]["version_number"], 3);
        assert_eq!(json["version"]["note"], "Added Mangialardo's");
    }

    #[test]
    fn payload_serialization_board_event_no_version() {
        let payload = WebhookPayload {
            event: "board.created",
            timestamp: "2026-02-23T12:00:00Z".to_string(),
            board: BoardInfo {
                slug: "dc-pizza".to_string(),
                name: "Best Pizza in DC".to_string(),
            },
            version: None,
        };

        let json: serde_json::Value = serde_json::to_value(&payload).unwrap();
        assert_eq!(json["event"], "board.created");
        assert!(json.get("version").is_none()); // skip_serializing_if
    }

    #[test]
    fn webhook_event_as_str() {
        assert_eq!(WebhookEvent::VersionCreated.as_str(), "version.created");
        assert_eq!(WebhookEvent::BoardCreated.as_str(), "board.created");
        assert_eq!(WebhookEvent::BoardUpdated.as_str(), "board.updated");
        assert_eq!(WebhookEvent::BoardDeleted.as_str(), "board.deleted");
    }
}
