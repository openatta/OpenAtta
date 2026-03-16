//! Authentication middleware
//!
//! Provides `CurrentUser` extractor for axum handlers.
//! Supports multiple auth modes: NoAuth (desktop), OidcBearer, ApiKey.

use std::sync::Arc;

use axum::{
    extract::FromRequestParts,
    http::{request::Parts, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use jsonwebtoken::{decode, DecodingKey, Validation};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tracing::{debug, warn};

use atta_store::StateStore;
use atta_types::{Actor, ActorType, Role};

/// Authentication mode
#[derive(Debug, Clone)]
pub enum AuthMode {
    /// No authentication — desktop default. All requests are treated as Owner.
    NoAuth,
    /// OIDC Bearer token validation (Enterprise)
    OidcBearer {
        issuer: String,
        audience: String,
        /// HMAC secret or RSA public key (PEM) for JWT verification
        secret: String,
    },
    /// API Key based authentication (service accounts)
    ApiKey,
}

impl Default for AuthMode {
    fn default() -> Self {
        Self::NoAuth
    }
}

/// JWT claims extracted from Bearer token
#[derive(Debug, Deserialize)]
struct JwtClaims {
    /// Subject — user identifier
    sub: String,
    /// Roles claim (optional)
    #[serde(default)]
    roles: Vec<String>,
}

/// Current authenticated user, extractable from request
#[derive(Debug, Clone)]
pub struct CurrentUser {
    pub actor: Actor,
    pub roles: Vec<Role>,
}

impl CurrentUser {
    /// Create a default desktop owner user
    pub fn desktop_owner() -> Self {
        Self {
            actor: Actor::user("owner"),
            roles: vec![Role::Owner],
        }
    }
}

/// Error response for auth failures
#[derive(Debug, Serialize)]
pub struct AuthError {
    pub error: String,
}

impl IntoResponse for AuthError {
    fn into_response(self) -> Response {
        (StatusCode::UNAUTHORIZED, Json(self)).into_response()
    }
}

/// Parse a role string into a Role enum
fn parse_role(s: &str) -> Option<Role> {
    match s.to_lowercase().as_str() {
        "owner" => Some(Role::Owner),
        "admin" => Some(Role::Admin),
        "operator" => Some(Role::Operator),
        "developer" => Some(Role::Developer),
        "approver" => Some(Role::Approver),
        "viewer" => Some(Role::Viewer),
        _ => None,
    }
}

/// Hash an API key using SHA-256 for storage lookup
fn hash_api_key(key: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(key.as_bytes());
    let hash = hasher.finalize();
    hash.iter().map(|b| format!("{b:02x}")).collect()
}

/// axum extractor for CurrentUser
///
/// In NoAuth mode (desktop), always returns the Owner user.
/// In Enterprise mode, validates Bearer token or API key from headers.
#[async_trait::async_trait]
impl<S: Send + Sync> FromRequestParts<S> for CurrentUser {
    type Rejection = AuthError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        // Try to get AuthMode from extensions (injected by router layer)
        let auth_mode = parts
            .extensions
            .get::<AuthMode>()
            .cloned()
            .unwrap_or_default();

        // In NoAuth mode, always return desktop owner
        if matches!(auth_mode, AuthMode::NoAuth) {
            return Ok(CurrentUser::desktop_owner());
        }

        // Check for Authorization header
        let auth_header = parts.headers.get("authorization");

        match auth_header {
            Some(header_value) => {
                let auth_str = header_value.to_str().unwrap_or("");

                if let Some(bearer_token) = auth_str.strip_prefix("Bearer ") {
                    return validate_bearer_token(bearer_token, &auth_mode);
                }

                if let Some(api_key) = auth_str.strip_prefix("ApiKey ") {
                    // Get StateStore from extensions for API key lookup
                    if let Some(store) = parts.extensions.get::<Arc<dyn StateStore>>() {
                        return validate_api_key(api_key, store).await;
                    }
                    warn!("ApiKey auth attempted but no StateStore in extensions");
                    return Err(AuthError {
                        error: "server configuration error: store not available".to_string(),
                    });
                }

                Err(AuthError {
                    error: "unsupported authorization scheme".to_string(),
                })
            }
            None => {
                // No auth header in Enterprise mode — reject
                Err(AuthError {
                    error: "authorization header required".to_string(),
                })
            }
        }
    }
}

/// Validate a JWT Bearer token and extract user info
fn validate_bearer_token(token: &str, auth_mode: &AuthMode) -> Result<CurrentUser, AuthError> {
    let (issuer, audience, secret) = match auth_mode {
        AuthMode::OidcBearer {
            issuer,
            audience,
            secret,
        } => (issuer, audience, secret),
        _ => {
            return Err(AuthError {
                error: "bearer token not supported in current auth mode".to_string(),
            });
        }
    };

    let mut validation = Validation::default();
    validation.set_issuer(&[issuer]);
    validation.set_audience(&[audience]);

    let key = DecodingKey::from_secret(secret.as_bytes());

    let token_data = decode::<JwtClaims>(token, &key, &validation).map_err(|e| {
        debug!(error = %e, "JWT validation failed");
        AuthError {
            error: format!("invalid token: {}", e),
        }
    })?;

    let claims = token_data.claims;

    // Parse roles from claims
    let roles: Vec<Role> = claims.roles.iter().filter_map(|r| parse_role(r)).collect();

    let roles = if roles.is_empty() {
        vec![Role::Viewer] // Default to Viewer if no valid roles
    } else {
        roles
    };

    debug!(actor = %claims.sub, ?roles, "authenticated via Bearer token");

    Ok(CurrentUser {
        actor: Actor::user(&claims.sub),
        roles,
    })
}

/// Validate an API key by looking up its hash in the store
async fn validate_api_key(
    api_key: &str,
    store: &Arc<dyn StateStore>,
) -> Result<CurrentUser, AuthError> {
    let key_hash = hash_api_key(api_key);

    let service_account = store
        .get_service_account_by_key(&key_hash)
        .await
        .map_err(|e| {
            warn!(error = %e, "failed to lookup API key");
            AuthError {
                error: "internal error during API key validation".to_string(),
            }
        })?;

    match service_account {
        Some(sa) => {
            let roles = if sa.roles.is_empty() {
                vec![Role::Viewer]
            } else {
                sa.roles.clone()
            };

            debug!(actor = %sa.name, ?roles, "authenticated via API key");

            Ok(CurrentUser {
                actor: Actor {
                    actor_type: ActorType::Service,
                    id: sa.name.clone(),
                },
                roles,
            })
        }
        None => {
            debug!("API key not found");
            Err(AuthError {
                error: "invalid API key".to_string(),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_desktop_owner() {
        let user = CurrentUser::desktop_owner();
        assert_eq!(user.actor.id, "owner");
        assert_eq!(user.roles, vec![Role::Owner]);
    }

    #[test]
    fn test_default_auth_mode() {
        let mode = AuthMode::default();
        assert!(matches!(mode, AuthMode::NoAuth));
    }

    #[test]
    fn test_parse_role() {
        assert_eq!(parse_role("owner"), Some(Role::Owner));
        assert_eq!(parse_role("Admin"), Some(Role::Admin));
        assert_eq!(parse_role("VIEWER"), Some(Role::Viewer));
        assert_eq!(parse_role("unknown"), None);
    }

    #[test]
    fn test_hash_api_key() {
        let hash1 = hash_api_key("test-key-123");
        let hash2 = hash_api_key("test-key-123");
        assert_eq!(hash1, hash2);
        assert_ne!(hash_api_key("different-key"), hash1);
    }
}
