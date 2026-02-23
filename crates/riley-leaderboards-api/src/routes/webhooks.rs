use std::path::Path;
use std::sync::Arc;

use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::Json;
use hmac::{Hmac, Mac};
use sha2::Sha256;

use crate::AppState;

type HmacSha256 = Hmac<Sha256>;

/// POST /webhooks/github — receives a GitHub push event, verifies HMAC, triggers sync.
pub async fn github(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    // Get the configured webhook secret
    let secret = match state
        .config
        .sync
        .as_ref()
        .and_then(|s| s.webhook_secret.as_ref())
    {
        Some(cv) => match cv.resolve() {
            Ok(s) => s,
            Err(_) => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({ "error": "webhook secret not configured" })),
                );
            }
        },
        None => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": "webhook secret not configured" })),
            );
        }
    };

    // Get the repo_path for sync
    let repo_path = match state
        .config
        .sync
        .as_ref()
        .and_then(|s| s.repo_path.as_ref())
    {
        Some(p) => p.clone(),
        None => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": "sync repo_path not configured" })),
            );
        }
    };

    // Verify HMAC-SHA256 signature
    let signature = match headers.get("x-hub-signature-256") {
        Some(sig) => match sig.to_str() {
            Ok(s) => s.to_string(),
            Err(_) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({ "error": "invalid signature header" })),
                );
            }
        },
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": "missing X-Hub-Signature-256 header" })),
                );
        }
    };

    if !verify_signature(&secret, &body, &signature) {
        return (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({ "error": "invalid signature" })),
        );
    }

    // Parse the push event to extract the commit message for the version note
    let note = extract_commit_message(&body);

    // Run sync
    match riley_leaderboards_core::sync::execute::sync_dir(
        &state.pool,
        Path::new(&repo_path),
        note.as_deref(),
    )
    .await
    {
        Ok(results) => {
            let summary: Vec<serde_json::Value> = results
                .iter()
                .map(|r| {
                    serde_json::json!({
                        "slug": r.slug,
                        "action": format!("{:?}", r.action),
                    })
                })
                .collect();
            (StatusCode::OK, Json(serde_json::json!({ "synced": summary })))
        }
        Err(e) => {
            tracing::error!("webhook sync failed: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": format!("sync failed: {e}") })),
            )
        }
    }
}

/// Verify the X-Hub-Signature-256 header against the body.
fn verify_signature(secret: &str, body: &[u8], signature: &str) -> bool {
    let expected_hex = match signature.strip_prefix("sha256=") {
        Some(hex) => hex,
        None => return false,
    };

    let mut mac = match HmacSha256::new_from_slice(secret.as_bytes()) {
        Ok(mac) => mac,
        Err(_) => return false,
    };
    mac.update(body);

    let computed = hex::encode(mac.finalize().into_bytes());
    // Constant-time comparison via hmac's verify is better, but we've already
    // computed the hex. Use a timing-safe comparison.
    constant_time_eq(computed.as_bytes(), expected_hex.as_bytes())
}

/// Constant-time byte comparison to prevent timing attacks.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

/// Extract the head commit message from a GitHub push event payload.
fn extract_commit_message(body: &[u8]) -> Option<String> {
    let payload: serde_json::Value = serde_json::from_slice(body).ok()?;
    payload
        .get("head_commit")
        .and_then(|hc| hc.get("message"))
        .and_then(|m| m.as_str())
        .map(|s| s.to_string())
}
