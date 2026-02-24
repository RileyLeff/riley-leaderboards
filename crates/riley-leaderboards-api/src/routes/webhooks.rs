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
///
/// Validates the signature and payload synchronously, then returns 202 Accepted
/// immediately and spawns the sync work (git fetch, reset, DB sync, webhooks)
/// in the background via TaskTracker. This avoids GitHub's 10-second webhook
/// timeout for slow git operations.
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
            Err(e) => {
                tracing::error!("webhook secret resolve failed: {e}");
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({ "error": "webhook processing failed" })),
                );
            }
        },
        None => {
            tracing::error!("webhook secret not configured");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": "webhook processing failed" })),
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
            tracing::error!("sync repo_path not configured");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": "webhook processing failed" })),
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
        .unwrap_or("main")
        .to_string();

    let push_ref = match payload.get("ref").and_then(|r| r.as_str()) {
        Some(r) => r,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": "missing 'ref' field in push payload" })),
            );
        }
    };
    let expected_ref = format!("refs/heads/{expected_branch}");
    if push_ref != expected_ref {
        tracing::info!("ignoring push to {push_ref} (expected {expected_ref})");
        return (
            StatusCode::OK,
            Json(serde_json::json!({ "ignored": true, "reason": "branch mismatch" })),
        );
    }

    // Extract the commit message for the version note (before spawning)
    let note = payload
        .get("head_commit")
        .and_then(|hc| hc.get("message"))
        .and_then(|m| m.as_str())
        .map(|s| s.to_string());

    // Spawn the heavy work (git fetch, reset, sync) in the background.
    // This returns 202 immediately, avoiding GitHub's 10-second timeout.
    let state2 = Arc::clone(&state);
    state.task_tracker.spawn(async move {
        if let Err(e) = run_sync(&state2, &repo_path, &expected_branch, note).await {
            tracing::error!("webhook sync failed: {e}");
        }
    });

    (
        StatusCode::ACCEPTED,
        Json(serde_json::json!({ "status": "sync queued" })),
    )
}

/// Execute the git fetch + reset + sync pipeline. Runs in a spawned task.
async fn run_sync(
    state: &Arc<AppState>,
    repo_path: &str,
    expected_branch: &str,
    note: Option<String>,
) -> Result<(), String> {
    // Serialize webhook processing to prevent concurrent git operations
    let _sync_guard = state.sync_mutex.lock().await;

    let timeout = std::time::Duration::from_secs(60);

    // Fetch
    let fetch_result = tokio::time::timeout(
        timeout,
        tokio::process::Command::new("git")
            .args(["-C", repo_path, "fetch", "origin", expected_branch])
            .output(),
    )
    .await;

    match fetch_result {
        Ok(Ok(output)) if output.status.success() => {
            tracing::info!("git fetch succeeded for {repo_path}");
        }
        Ok(Ok(output)) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("git fetch failed for {repo_path}: {stderr}"));
        }
        Ok(Err(e)) => {
            return Err(format!("failed to run git fetch: {e}"));
        }
        Err(_) => {
            return Err(format!("git fetch timed out for {repo_path}"));
        }
    }

    // Reset
    let reset_target = format!("origin/{expected_branch}");
    let reset_result = tokio::time::timeout(
        timeout,
        tokio::process::Command::new("git")
            .args(["-C", repo_path, "reset", "--hard", &reset_target])
            .output(),
    )
    .await;

    match reset_result {
        Ok(Ok(output)) if output.status.success() => {
            tracing::info!("git reset --hard succeeded for {repo_path}");
        }
        Ok(Ok(output)) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("git reset failed for {repo_path}: {stderr}"));
        }
        Ok(Err(e)) => {
            return Err(format!("failed to run git reset: {e}"));
        }
        Err(_) => {
            return Err(format!("git reset timed out for {repo_path}"));
        }
    }

    // Sync
    match riley_leaderboards_core::sync::execute::sync_dir(
        &state.pool,
        Path::new(repo_path),
        note.as_deref(),
    )
    .await
    {
        Ok(results) => {
            // Fire outbound webhooks for synced boards/versions
            for r in &results {
                match &r.action {
                    riley_leaderboards_core::sync::execute::SyncAction::Created {
                        version_number,
                    } => {
                        let _ = crate::outbound_webhooks::fire(
                            &state.config.webhooks,
                            riley_leaderboards_core::config::WebhookEvent::BoardCreated,
                            &r.slug,
                            &r.name,
                            None,
                            None,
                            Some(&state.task_tracker),
                        );
                        let _ = crate::outbound_webhooks::fire(
                            &state.config.webhooks,
                            riley_leaderboards_core::config::WebhookEvent::VersionCreated,
                            &r.slug,
                            &r.name,
                            Some(crate::outbound_webhooks::VersionInfo {
                                version_number: *version_number,
                                note: note.clone(),
                            }),
                            None,
                            Some(&state.task_tracker),
                        );
                        if let Some(ref event_bus) = state.event_bus {
                            event_bus.publish_version(
                                &r.slug,
                                *version_number,
                                note.clone(),
                            );
                        }
                    }
                    riley_leaderboards_core::sync::execute::SyncAction::Updated {
                        version_number,
                    } => {
                        let _ = crate::outbound_webhooks::fire(
                            &state.config.webhooks,
                            riley_leaderboards_core::config::WebhookEvent::VersionCreated,
                            &r.slug,
                            &r.name,
                            Some(crate::outbound_webhooks::VersionInfo {
                                version_number: *version_number,
                                note: note.clone(),
                            }),
                            None,
                            Some(&state.task_tracker),
                        );
                        if let Some(ref event_bus) = state.event_bus {
                            event_bus.publish_version(
                                &r.slug,
                                *version_number,
                                note.clone(),
                            );
                        }
                    }
                    _ => {}
                }
            }

            tracing::info!(
                "webhook sync completed: {} boards processed",
                results.len()
            );
            Ok(())
        }
        Err(e) => Err(format!("sync failed: {e}")),
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
