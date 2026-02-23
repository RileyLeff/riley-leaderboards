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

    // Check event type — only process push events, require the header
    let event_type = match headers.get("x-github-event") {
        Some(event) => match event.to_str() {
            Ok(s) => s.to_string(),
            Err(_) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({ "error": "invalid X-GitHub-Event header" })),
                );
            }
        },
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": "missing X-GitHub-Event header" })),
            );
        }
    };

    match event_type.as_str() {
        "push" => {} // proceed
        "ping" => {
            return (
                StatusCode::OK,
                Json(serde_json::json!({ "pong": true })),
            );
        }
        _ => {
            return (
                StatusCode::OK,
                Json(serde_json::json!({ "ignored": true, "reason": "event type not handled" })),
            );
        }
    }

    // Parse the JSON body once for ref and commit message extraction
    let payload: serde_json::Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": "invalid JSON payload" })),
            );
        }
    };

    // Check branch — only sync pushes to the configured branch
    let expected_branch = state
        .config
        .sync
        .as_ref()
        .and_then(|s| s.sync_branch.as_deref())
        .unwrap_or("main");

    if let Some(push_ref) = payload.get("ref").and_then(|r| r.as_str()) {
        let expected_ref = format!("refs/heads/{expected_branch}");
        if push_ref != expected_ref {
            tracing::info!("ignoring push to {push_ref} (expected {expected_ref})");
            return (
                StatusCode::OK,
                Json(serde_json::json!({ "ignored": true, "reason": "branch mismatch" })),
            );
        }
    }

    // Pull latest changes from the repository (with 60-second timeout)
    let pull_result = tokio::time::timeout(
        std::time::Duration::from_secs(60),
        tokio::process::Command::new("git")
            .args(["-C", &repo_path, "pull", "origin", expected_branch])
            .output(),
    )
    .await;

    match pull_result {
        Ok(Ok(output)) if output.status.success() => {
            tracing::info!("git pull succeeded for {repo_path}");
        }
        Ok(Ok(output)) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            tracing::error!("git pull failed for {repo_path}: {stderr}");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": "failed to update repository" })),
            );
        }
        Ok(Err(e)) => {
            tracing::error!("failed to run git pull: {e}");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": "failed to update repository" })),
            );
        }
        Err(_) => {
            tracing::error!("git pull timed out for {repo_path}");
            return (
                StatusCode::GATEWAY_TIMEOUT,
                Json(serde_json::json!({ "error": "repository update timed out" })),
            );
        }
    }

    // Extract the commit message for the version note
    let note = payload
        .get("head_commit")
        .and_then(|hc| hc.get("message"))
        .and_then(|m| m.as_str())
        .map(|s| s.to_string());

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
                Json(serde_json::json!({ "error": "sync failed" })),
            )
        }
    }
}

/// Verify the X-Hub-Signature-256 header against the body using the hmac
/// crate's built-in constant-time comparison.
fn verify_signature(secret: &str, body: &[u8], signature: &str) -> bool {
    let expected_hex = match signature.strip_prefix("sha256=") {
        Some(hex) => hex,
        None => return false,
    };

    let expected_bytes = match hex::decode(expected_hex) {
        Ok(bytes) => bytes,
        Err(_) => return false,
    };

    let mut mac = match HmacSha256::new_from_slice(secret.as_bytes()) {
        Ok(mac) => mac,
        Err(_) => return false,
    };
    mac.update(body);

    mac.verify_slice(&expected_bytes).is_ok()
}

