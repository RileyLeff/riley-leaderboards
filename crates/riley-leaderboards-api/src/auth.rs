use std::collections::HashMap;
use std::sync::Arc;

use axum::extract::State;
use axum::http::{Method, Request, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use hmac::Mac;
use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode, decode_header};
use serde::Deserialize;
use tokio::sync::RwLock;

use riley_leaderboards_core::config::AuthConfig;

use crate::AppState;

/// Runtime auth mode, initialized from config at startup.
pub enum AuthMode {
    /// No auth configured — all endpoints are open.
    NoAuth,
    /// JWT mode — validate Bearer tokens against JWKS keys.
    /// Read-only tokens are API tokens that bypass JWT for read operations.
    Jwt {
        jwks_cache: Arc<JwksCache>,
        required_role: Option<String>,
        read_token_hashes: Vec<Vec<u8>>,
        require_read_auth: bool,
    },
    /// API token mode — validate Bearer tokens against configured secrets.
    ApiToken {
        /// HMAC-SHA256 hash of the admin token (full read/write access).
        admin_token_hash: Vec<u8>,
        /// HMAC-SHA256 hashes of read-only tokens.
        read_token_hashes: Vec<Vec<u8>>,
        /// Whether read endpoints require authentication.
        require_read_auth: bool,
    },
}

impl AuthMode {
    /// Build the auth mode from config. For JWT mode, fetches the JWKS on startup.
    pub async fn from_config(config: Option<&AuthConfig>) -> anyhow::Result<Self> {
        let Some(auth) = config else {
            return Ok(Self::NoAuth);
        };

        // Resolve the effective admin token (admin_token or legacy api_token alias)
        let effective_admin_token = match (&auth.admin_token, &auth.api_token) {
            (Some(_), Some(_)) => {
                anyhow::bail!("auth config: admin_token and api_token are mutually exclusive (api_token is a legacy alias for admin_token)");
            }
            (Some(t), None) | (None, Some(t)) => Some(t),
            (None, None) => None,
        };

        // Resolve read tokens
        let read_token_hashes: Vec<Vec<u8>> = auth
            .read_tokens
            .iter()
            .map(|t| {
                let resolved = t.resolve().map_err(anyhow::Error::from)?;
                Ok(hash_token(&resolved))
            })
            .collect::<anyhow::Result<Vec<_>>>()?;

        let require_read_auth = auth.require_read_auth;

        match (&auth.jwks_url, effective_admin_token) {
            (Some(_), Some(_)) => {
                anyhow::bail!("auth config: jwks_url and admin_token/api_token are mutually exclusive");
            }
            (Some(url), None) => {
                let cache = JwksCache::new(url).await?;
                let cache = Arc::new(cache);
                cache.spawn_refresh_task();
                Ok(Self::Jwt {
                    jwks_cache: cache,
                    required_role: auth.required_role.clone(),
                    read_token_hashes,
                    require_read_auth,
                })
            }
            (None, Some(token_val)) => {
                let token = token_val.resolve().map_err(anyhow::Error::from)?;
                let admin_token_hash = hash_token(&token);
                Ok(Self::ApiToken {
                    admin_token_hash,
                    read_token_hashes,
                    require_read_auth,
                })
            }
            (None, None) => {
                if !read_token_hashes.is_empty()
                    || require_read_auth
                    || auth.required_role.is_some()
                {
                    anyhow::bail!(
                        "auth config: auth fields (read_tokens, require_read_auth, or \
                         required_role) set without admin_token or jwks_url — no auth \
                         mechanism is configured"
                    );
                }
                Ok(Self::NoAuth)
            }
        }
    }
}

/// Hash a token using HMAC-SHA256 with a fixed key, for constant-time comparison.
fn hash_token(token: &str) -> Vec<u8> {
    let mut mac =
        hmac::Hmac::<sha2::Sha256>::new_from_slice(b"riley-leaderboards-api-token").unwrap();
    mac.update(token.as_bytes());
    mac.finalize().into_bytes().to_vec()
}

/// Verify an incoming token against a stored hash using constant-time comparison.
fn verify_token(incoming: &str, expected_hash: &[u8]) -> bool {
    let mut mac =
        hmac::Hmac::<sha2::Sha256>::new_from_slice(b"riley-leaderboards-api-token").unwrap();
    mac.update(incoming.as_bytes());
    mac.verify_slice(expected_hash).is_ok()
}

// -- JWKS Cache --

/// Maximum age of cached JWKS keys before auth fails closed.
/// If refresh has been failing for longer than this, reject all JWTs
/// rather than accepting tokens signed with potentially revoked keys.
const JWKS_MAX_STALE_SECS: u64 = 7200; // 2 hours

/// Fetches and caches JWKS keys for JWT validation.
pub struct JwksCache {
    url: String,
    client: reqwest::Client,
    /// Map of kid -> (DecodingKey, Algorithm)
    keys: RwLock<HashMap<String, (DecodingKey, Algorithm)>>,
    /// Timestamp of last successful refresh.
    last_refresh: RwLock<std::time::Instant>,
}

#[derive(Deserialize)]
struct JwkSet {
    keys: Vec<JwkEntry>,
}

#[derive(Deserialize)]
struct JwkEntry {
    kid: Option<String>,
    kty: String,
    alg: Option<String>,
    n: Option<String>,
    e: Option<String>,
}

impl JwksCache {
    pub async fn new(url: &str) -> anyhow::Result<Self> {
        let client = reqwest::Client::new();
        let cache = Self {
            url: url.to_string(),
            client,
            keys: RwLock::new(HashMap::new()),
            last_refresh: RwLock::new(std::time::Instant::now()),
        };
        cache.refresh().await?;
        Ok(cache)
    }

    /// Construct a cache from a static key (useful for testing).
    pub fn from_static(kid: String, key: DecodingKey, algorithm: Algorithm) -> Self {
        let mut keys = HashMap::new();
        keys.insert(kid, (key, algorithm));
        Self {
            url: String::new(),
            client: reqwest::Client::new(),
            keys: RwLock::new(keys),
            last_refresh: RwLock::new(std::time::Instant::now()),
        }
    }

    /// Fetch JWKS from the configured URL and update the cache.
    pub async fn refresh(&self) -> anyhow::Result<()> {
        let resp = self
            .client
            .get(&self.url)
            .send()
            .await?
            .error_for_status()?;
        let jwk_set: JwkSet = resp.json().await?;

        let mut new_keys = HashMap::new();
        for jwk in &jwk_set.keys {
            let Some(kid) = &jwk.kid else { continue };
            let Some(key_and_alg) = parse_jwk(jwk) else {
                tracing::warn!("skipping unsupported JWK kid={kid}");
                continue;
            };
            new_keys.insert(kid.clone(), key_and_alg);
        }

        if new_keys.is_empty() {
            tracing::warn!("JWKS refresh returned zero usable keys — keeping existing cache");
            return Ok(());
        }

        tracing::info!("JWKS refreshed: {} keys", new_keys.len());
        *self.keys.write().await = new_keys;
        *self.last_refresh.write().await = std::time::Instant::now();
        Ok(())
    }

    /// Get a decoding key by kid. Returns None if the kid is unknown or if
    /// the cache is stale beyond the maximum allowed age.
    pub async fn get_key(&self, kid: &str) -> std::result::Result<Option<(DecodingKey, Algorithm)>, &'static str> {
        let elapsed = self.last_refresh.read().await.elapsed();
        if elapsed.as_secs() > JWKS_MAX_STALE_SECS {
            return Err("JWKS cache is stale — refresh has been failing");
        }
        Ok(self.keys.read().await.get(kid).cloned())
    }

    /// Spawn a background task that refreshes the JWKS every 60 minutes.
    pub fn spawn_refresh_task(self: &Arc<Self>) {
        let cache = Arc::clone(self);
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(3600));
            interval.tick().await; // skip the initial tick (already fetched in new())
            loop {
                interval.tick().await;
                if let Err(e) = cache.refresh().await {
                    tracing::error!("JWKS refresh failed: {e}");
                }
            }
        });
    }
}

/// Parse a JWK entry into a DecodingKey + Algorithm. Only RSA keys are supported.
fn parse_jwk(jwk: &JwkEntry) -> Option<(DecodingKey, Algorithm)> {
    if jwk.kty != "RSA" {
        return None;
    }
    let n = jwk.n.as_ref()?;
    let e = jwk.e.as_ref()?;

    let key = DecodingKey::from_rsa_components(n, e).ok()?;
    let alg = match jwk.alg.as_deref() {
        Some("RS256") | None => Algorithm::RS256,
        Some("RS384") => Algorithm::RS384,
        Some("RS512") => Algorithm::RS512,
        _ => return None,
    };

    Some((key, alg))
}

// -- JWT Claims --

#[derive(Deserialize)]
struct Claims {
    #[allow(dead_code)]
    sub: Option<String>,
    roles: Option<Vec<String>>,
}

// -- Auth Middleware --

/// Check if a token matches any of the given read-only token hashes.
fn is_read_token(incoming: &str, read_token_hashes: &[Vec<u8>]) -> bool {
    read_token_hashes
        .iter()
        .any(|hash| verify_token(incoming, hash))
}

/// Axum middleware that enforces auth on operations.
///
/// Write operations (POST, PATCH, PUT, DELETE) require admin auth (admin token or JWT with role).
/// Read operations (GET, HEAD, OPTIONS) are public by default, but can require auth when
/// `require_read_auth` is enabled — any valid token (admin, read-only, or JWT) suffices.
pub async fn require_auth(
    State(state): State<Arc<AppState>>,
    request: Request<axum::body::Body>,
    next: Next,
) -> Result<Response, Response> {
    let is_read = matches!(
        *request.method(),
        Method::GET | Method::HEAD | Method::OPTIONS
    );

    match &state.auth_mode {
        AuthMode::NoAuth => Ok(next.run(request).await),

        AuthMode::ApiToken {
            admin_token_hash,
            read_token_hashes,
            require_read_auth,
        } => {
            if is_read && !require_read_auth {
                return Ok(next.run(request).await);
            }

            let token = extract_bearer_token(&request)
                .ok_or_else(|| auth_error("missing or invalid Authorization header"))?;

            // Admin token: allows everything
            if verify_token(token, admin_token_hash) {
                return Ok(next.run(request).await);
            }

            // Read-only token: only allows reads
            if is_read && is_read_token(token, read_token_hashes) {
                return Ok(next.run(request).await);
            }

            Err(auth_error(if is_read {
                "invalid API token"
            } else {
                "write operations require an admin token"
            }))
        }

        AuthMode::Jwt {
            jwks_cache,
            required_role,
            read_token_hashes,
            require_read_auth,
        } => {
            if is_read && !require_read_auth {
                return Ok(next.run(request).await);
            }

            let token = extract_bearer_token(&request)
                .ok_or_else(|| auth_error("missing or invalid Authorization header"))?;

            // For reads, check read-only tokens first (cheaper than JWT validation)
            if is_read && is_read_token(token, read_token_hashes) {
                return Ok(next.run(request).await);
            }

            // Validate JWT — for writes, enforce required_role; for reads, any valid JWT
            let role_to_check = if is_read { None } else { required_role.as_deref() };
            validate_jwt(token, jwks_cache, role_to_check).await?;

            Ok(next.run(request).await)
        }
    }
}

/// Extract the Bearer token from the Authorization header.
/// Per RFC 7235, the scheme is case-insensitive.
fn extract_bearer_token(request: &Request<axum::body::Body>) -> Option<&str> {
    let value = request.headers().get("authorization")?.to_str().ok()?;
    // Case-insensitive check for "Bearer " prefix (RFC 7235 Section 2.1)
    if value.len() > 7 && value[..7].eq_ignore_ascii_case("bearer ") {
        Some(&value[7..])
    } else {
        None
    }
}

/// Validate a JWT against the JWKS cache.
async fn validate_jwt(
    token: &str,
    jwks_cache: &JwksCache,
    required_role: Option<&str>,
) -> Result<(), Response> {
    // Decode header to get kid
    let header = decode_header(token).map_err(|e| auth_error(&format!("invalid JWT: {e}")))?;

    let kid = header
        .kid
        .as_deref()
        .ok_or_else(|| auth_error("JWT missing kid header"))?;

    // Look up the key (also checks cache staleness)
    let (key, expected_alg) = jwks_cache
        .get_key(kid)
        .await
        .map_err(|e| auth_error(e))?
        .ok_or_else(|| auth_error("unknown signing key"))?;

    // Validate algorithm matches
    if header.alg != expected_alg {
        return Err(auth_error("JWT algorithm mismatch"));
    }

    // Decode and validate
    let mut validation = Validation::new(expected_alg);
    validation.validate_exp = true;
    validation.validate_nbf = true;
    validation.validate_aud = false; // we don't enforce audience

    let token_data = decode::<Claims>(token, &key, &validation)
        .map_err(|e| auth_error(&format!("JWT validation failed: {e}")))?;

    // Check required role
    if let Some(role) = required_role {
        let has_role = token_data
            .claims
            .roles
            .as_ref()
            .is_some_and(|roles| roles.iter().any(|r| r == role));

        if !has_role {
            return Err(auth_error("insufficient permissions"));
        }
    }

    Ok(())
}

fn auth_error(message: &str) -> Response {
    (
        StatusCode::UNAUTHORIZED,
        axum::Json(serde_json::json!({ "error": message })),
    )
        .into_response()
}
