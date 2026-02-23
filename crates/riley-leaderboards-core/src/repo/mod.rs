pub mod boards;
pub mod entries;
pub mod references;
pub mod scores;
pub mod versions;

use crate::error::{Error, Result};

/// Validate that a slug is URL-safe: lowercase alphanumeric and hyphens only,
/// 1-128 characters, no leading/trailing hyphens.
pub fn validate_slug(slug: &str) -> Result<()> {
    if slug.is_empty() || slug.len() > 128 {
        return Err(Error::Validation(
            "slug must be 1-128 characters".to_string(),
        ));
    }
    if !slug
        .bytes()
        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    {
        return Err(Error::Validation(format!(
            "slug '{slug}' must contain only lowercase letters, digits, and hyphens"
        )));
    }
    if slug.starts_with('-') || slug.ends_with('-') {
        return Err(Error::Validation(format!(
            "slug '{slug}' must not start or end with a hyphen"
        )));
    }
    Ok(())
}

/// Validate that a name is non-empty and within a reasonable length.
pub fn validate_name(name: &str) -> Result<()> {
    if name.is_empty() {
        return Err(Error::Validation("name must not be empty".to_string()));
    }
    if name.len() > 256 {
        return Err(Error::Validation(
            "name must be 256 characters or fewer".to_string(),
        ));
    }
    Ok(())
}
