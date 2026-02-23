pub mod boards;
pub mod collections;
pub mod entries;
pub mod references;
pub mod scores;
pub mod versions;
pub mod webhooks;

use riley_leaderboards_core::error::Error as CoreError;

/// Validate that a metadata JSON value does not exceed the configured size limit.
pub fn check_metadata_size(
    metadata: Option<&serde_json::Value>,
    max_bytes: usize,
) -> Result<(), CoreError> {
    if let Some(meta) = metadata {
        let size = serde_json::to_string(meta)
            .map(|s| s.len())
            .unwrap_or(0);
        if size > max_bytes {
            return Err(CoreError::Validation(format!(
                "metadata too large ({size} bytes, max {max_bytes})",
            )));
        }
    }
    Ok(())
}
