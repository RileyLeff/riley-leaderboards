use hmac::{Hmac, Mac};
use serde::Serialize;
use sha2::Sha256;

use riley_leaderboards_core::config::{WebhookConfig, WebhookEvent};

type HmacSha256 = Hmac<Sha256>;

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

/// Fire outbound webhooks for the given event and board. Non-blocking —
/// spawns a task for each matching webhook and returns immediately.
pub fn fire(
    configs: &[WebhookConfig],
    event: WebhookEvent,
    board_slug: &str,
    board_name: &str,
    version_info: Option<VersionInfo>,
) {
    let payload = WebhookPayload {
        event: event.as_str(),
        timestamp: chrono::Utc::now().to_rfc3339(),
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
            return;
        }
    };

    for config in configs {
        if !config.events.contains(&event) {
            continue;
        }
        if !matches_board_filter(&config.boards, board_slug) {
            continue;
        }

        let url = config.url.clone();
        let body = body.clone();
        let secret = config.secret.as_ref().and_then(|cv| cv.resolve().ok());

        tokio::spawn(async move {
            deliver(&url, &body, secret.as_deref()).await;
        });
    }
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
/// 3 attempts with exponential backoff: 1s, 5s, 25s.
/// 10 second timeout per attempt.
async fn deliver(url: &str, body: &[u8], secret: Option<&str>) {
    let client = reqwest::Client::new();
    let delays = [1, 5, 25];

    for (attempt, delay_secs) in delays.iter().enumerate() {
        let mut request = client
            .post(url)
            .header("Content-Type", "application/json")
            .timeout(std::time::Duration::from_secs(10))
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

        tokio::time::sleep(std::time::Duration::from_secs(*delay_secs)).await;
    }

    tracing::error!("webhook to {url} exhausted all retries");
}

/// Compute HMAC-SHA256 signature of the payload body.
fn compute_signature(secret: &str, body: &[u8]) -> Option<String> {
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).ok()?;
    mac.update(body);
    Some(hex::encode(mac.finalize().into_bytes()))
}
