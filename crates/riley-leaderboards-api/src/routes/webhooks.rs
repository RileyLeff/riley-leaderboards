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

    // Serialize webhook processing to prevent concurrent git operations
    let _sync_guard = state.sync_mutex.lock().await;

    // Fetch + hard reset to match remote (avoids merge conflicts on a read-only clone)
    let timeout = std::time::Duration::from_secs(60);

    let fetch_result = tokio::time::timeout(
        timeout,
        tokio::process::Command::new("git")
            .args(["-C", &repo_path, "fetch", "origin", expected_branch])
            .output(),
    )
    .await;

    match fetch_result {
        Ok(Ok(output)) if output.status.success() => {
            tracing::info!("git fetch succeeded for {repo_path}");
        }
        Ok(Ok(output)) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            tracing::error!("git fetch failed for {repo_path}: {stderr}");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": "failed to update repository" })),
            );
        }
        Ok(Err(e)) => {
            tracing::error!("failed to run git fetch: {e}");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": "failed to update repository" })),
            );
        }
        Err(_) => {
            tracing::error!("git fetch timed out for {repo_path}");
            return (
                StatusCode::GATEWAY_TIMEOUT,
                Json(serde_json::json!({ "error": "repository update timed out" })),
            );
        }
    }

    let reset_target = format!("origin/{expected_branch}");
    let reset_result = tokio::time::timeout(
        timeout,
        tokio::process::Command::new("git")
            .args(["-C", &repo_path, "reset", "--hard", &reset_target])
            .output(),
    )
    .await;

    match reset_result {
        Ok(Ok(output)) if output.status.success() => {
            tracing::info!("git reset --hard succeeded for {repo_path}");
        }
        Ok(Ok(output)) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            tracing::error!("git reset failed for {repo_path}: {stderr}");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": "failed to update repository" })),
            );
        }
        Ok(Err(e)) => {
            tracing::error!("failed to run git reset: {e}");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": "failed to update repository" })),
            );
        }
        Err(_) => {
            tracing::error!("git reset timed out for {repo_path}");
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
            // Fire outbound webhooks for synced versions
            for r in &results {
                let version_number = match &r.action {
                    riley_leaderboards_core::sync::execute::SyncAction::Created { version_number }
                    | riley_leaderboards_core::sync::execute::SyncAction::Updated { version_number } => {
                        Some(*version_number)
                    }
                    _ => None,
                };
                if let Some(vnum) = version_number {
                    crate::outbound_webhooks::fire(
                        &state.config.webhooks,
                        riley_leaderboards_core::config::WebhookEvent::VersionCreated,
                        &r.slug,
                        &r.name,
                        Some(crate::outbound_webhooks::VersionInfo {
                            version_number: vnum,
                            note: note.clone(),
                        }),
                    );
                }
            }

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

