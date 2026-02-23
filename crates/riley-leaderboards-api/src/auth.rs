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
    Jwt {
        jwks_cache: Arc<JwksCache>,
        required_role: Option<String>,
    },
    /// API token mode — validate Bearer tokens against a configured secret.
    ApiToken {
        /// HMAC-SHA256 hash of the configured token (we don't store plaintext).
        token_hash: Vec<u8>,
    },
}

impl AuthMode {
    /// Build the auth mode from config. For JWT mode, fetches the JWKS on startup.
    pub async fn from_config(config: Option<&AuthConfig>) -> anyhow::Result<Self> {
        let Some(auth) = config else {
            return Ok(Self::NoAuth);
        };

        match (&auth.jwks_url, &auth.api_token) {
            (Some(_), Some(_)) => {
                anyhow::bail!("auth config: jwks_url and api_token are mutually exclusive");
            }
            (Some(url), None) => {
                let cache = JwksCache::new(url).await?;
                let cache = Arc::new(cache);
                cache.spawn_refresh_task();
                Ok(Self::Jwt {
                    jwks_cache: cache,
                    required_role: auth.required_role.clone(),
                })
            }
            (None, Some(token_val)) => {
                let token = token_val.resolve().map_err(|e| anyhow::anyhow!("{e}"))?;
                let token_hash = hash_token(&token);
                Ok(Self::ApiToken { token_hash })
            }
            (None, None) => Ok(Self::NoAuth),
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

/// Fetches and caches JWKS keys for JWT validation.
pub struct JwksCache {
    url: String,
    client: reqwest::Client,
    /// Map of kid -> (DecodingKey, Algorithm)
    keys: RwLock<HashMap<String, (DecodingKey, Algorithm)>>,
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
            tracing::warn!("JWKS refresh returned zero usable keys");
        } else {
            tracing::info!("JWKS refreshed: {} keys", new_keys.len());
        }

        *self.keys.write().await = new_keys;
        Ok(())
    }

    /// Get a decoding key by kid.
    pub async fn get_key(&self, kid: &str) -> Option<(DecodingKey, Algorithm)> {
        self.keys.read().await.get(kid).cloned()
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

/// Axum middleware that enforces auth on write operations (POST, PATCH, PUT, DELETE).
/// Read operations (GET, HEAD, OPTIONS) pass through unconditionally.
pub async fn require_auth(
    State(state): State<Arc<AppState>>,
    request: Request<axum::body::Body>,
    next: Next,
) -> Result<Response, Response> {
    // Read operations are always public
    if matches!(
        *request.method(),
        Method::GET | Method::HEAD | Method::OPTIONS
    ) {
        return Ok(next.run(request).await);
    }

    match &state.auth_mode {
        AuthMode::NoAuth => Ok(next.run(request).await),
        AuthMode::ApiToken { token_hash } => {
            let token = extract_bearer_token(&request)
                .ok_or_else(|| auth_error("missing or invalid Authorization header"))?;

            if !verify_token(token, token_hash) {
                return Err(auth_error("invalid API token"));
            }

            Ok(next.run(request).await)
        }
        AuthMode::Jwt {
            jwks_cache,
            required_role,
        } => {
            let token = extract_bearer_token(&request)
                .ok_or_else(|| auth_error("missing or invalid Authorization header"))?;

            validate_jwt(token, jwks_cache, required_role.as_deref()).await?;

            Ok(next.run(request).await)
        }
    }
}

/// Extract the Bearer token from the Authorization header.
fn extract_bearer_token(request: &Request<axum::body::Body>) -> Option<&str> {
    request
        .headers()
        .get("authorization")?
        .to_str()
        .ok()?
        .strip_prefix("Bearer ")
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

    // Look up the key
    let (key, expected_alg) = jwks_cache
        .get_key(kid)
        .await
        .ok_or_else(|| auth_error("unknown signing key"))?;

    // Validate algorithm matches
    if header.alg != expected_alg {
        return Err(auth_error("JWT algorithm mismatch"));
    }

    // Decode and validate
    let mut validation = Validation::new(expected_alg);
    validation.validate_exp = true;
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
