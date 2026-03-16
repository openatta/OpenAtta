//! Package management API handlers

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use chrono::Utc;
use serde::Deserialize;

use atta_types::{Action, AttaError, PackageRecord, PackageType, Resource, ResourceType};

use crate::middleware::CurrentUser;
use crate::server::response::{check_authz, error_response};
use crate::server::AppState;

/// Request body for installing a package
#[derive(Debug, Deserialize)]
pub struct InstallPackageRequest {
    /// Package source: "git", "registry", or "local"
    pub source: String,
    /// URL or path to the package
    pub url: String,
    /// Package name
    pub name: String,
    /// Package type (plugin, flow, skill, tool, mcp)
    #[serde(default = "default_package_type")]
    pub package_type: PackageType,
    /// Package version (defaults to "0.1.0")
    #[serde(default = "default_version")]
    pub version: String,
}

fn default_package_type() -> PackageType {
    PackageType::Plugin
}

fn default_version() -> String {
    "0.1.0".to_string()
}

/// Parse a package type string from the URL path segment
fn parse_package_type(s: &str) -> Result<PackageType, AttaError> {
    match s {
        "plugin" | "plugins" => Ok(PackageType::Plugin),
        "flow" | "flows" => Ok(PackageType::Flow),
        "skill" | "skills" => Ok(PackageType::Skill),
        "tool" | "tools" => Ok(PackageType::Tool),
        "mcp" => Ok(PackageType::Mcp),
        _ => Err(AttaError::Validation(format!(
            "invalid package type: '{}'. Expected one of: plugin, flow, skill, tool, mcp",
            s
        ))),
    }
}

/// POST /api/v1/packages/install
///
/// Install a new package from a given source.
pub async fn install_package(
    State(state): State<AppState>,
    user: CurrentUser,
    Json(req): Json<InstallPackageRequest>,
) -> impl IntoResponse {
    if let Err(resp) = check_authz(&state, &user, Action::Create, &Resource::all(ResourceType::Package)).await {
        return resp;
    }

    // Validate source
    match req.source.as_str() {
        "git" | "registry" | "local" => {}
        _ => {
            return error_response(
                StatusCode::BAD_REQUEST,
                &AttaError::Validation(format!(
                    "invalid source: '{}'. Expected one of: git, registry, local",
                    req.source
                )),
            );
        }
    }

    let record = PackageRecord {
        name: req.name,
        version: req.version,
        package_type: req.package_type,
        installed_at: Utc::now(),
        installed_by: user.actor.id,
    };

    match state.store.register_package(&record).await {
        Ok(()) => (StatusCode::CREATED, Json(record)).into_response(),
        Err(e) => error_response(StatusCode::INTERNAL_SERVER_ERROR, &e),
    }
}

/// POST /api/v1/packages/{pkg_type}/{name}/upgrade
///
/// Upgrade an existing package by bumping its version.
pub async fn upgrade_package(
    State(state): State<AppState>,
    user: CurrentUser,
    Path((pkg_type, name)): Path<(String, String)>,
) -> impl IntoResponse {
    if let Err(resp) = check_authz(&state, &user, Action::Update, &Resource::new(ResourceType::Package, &name)).await {
        return resp;
    }

    // Validate package type from path
    let _package_type = match parse_package_type(&pkg_type) {
        Ok(pt) => pt,
        Err(e) => return error_response(StatusCode::BAD_REQUEST, &e),
    };

    // Look up existing package
    let existing = match state.store.get_package(&name).await {
        Ok(Some(pkg)) => pkg,
        Ok(None) => {
            return error_response(
                StatusCode::NOT_FOUND,
                &AttaError::NotFound {
                    entity_type: "package".to_string(),
                    id: name,
                },
            );
        }
        Err(e) => return error_response(StatusCode::INTERNAL_SERVER_ERROR, &e),
    };

    // Bump the patch version
    let new_version = bump_patch_version(&existing.version);

    let updated = PackageRecord {
        name: existing.name,
        version: new_version,
        package_type: existing.package_type,
        installed_at: Utc::now(),
        installed_by: user.actor.id,
    };

    match state.store.register_package(&updated).await {
        Ok(()) => (StatusCode::OK, Json(updated)).into_response(),
        Err(e) => error_response(StatusCode::INTERNAL_SERVER_ERROR, &e),
    }
}

/// GET /api/v1/packages/{pkg_type}/{name}/dependencies
///
/// Return the dependencies for a given package (currently returns an empty list
/// since PackageRecord does not store dependency info).
pub async fn package_dependencies(
    State(state): State<AppState>,
    _user: CurrentUser,
    Path((pkg_type, name)): Path<(String, String)>,
) -> impl IntoResponse {
    // Validate package type from path
    let _package_type = match parse_package_type(&pkg_type) {
        Ok(pt) => pt,
        Err(e) => return error_response(StatusCode::BAD_REQUEST, &e),
    };

    // Look up existing package
    match state.store.get_package(&name).await {
        Ok(Some(pkg)) => {
            // PackageRecord does not contain dependency info,
            // so return the package along with an empty dependencies list.
            let response = serde_json::json!({
                "package": pkg.name,
                "version": pkg.version,
                "dependencies": []
            });
            (StatusCode::OK, Json(response)).into_response()
        }
        Ok(None) => error_response(
            StatusCode::NOT_FOUND,
            &AttaError::NotFound {
                entity_type: "package".to_string(),
                id: name,
            },
        ),
        Err(e) => error_response(StatusCode::INTERNAL_SERVER_ERROR, &e),
    }
}

/// Bump the patch component of a semver-like version string.
/// e.g. "1.2.3" -> "1.2.4", "0.1.0" -> "0.1.1".
/// Falls back to appending ".1" for non-standard versions.
fn bump_patch_version(version: &str) -> String {
    let parts: Vec<&str> = version.split('.').collect();
    if parts.len() == 3 {
        if let Ok(patch) = parts[2].parse::<u64>() {
            return format!("{}.{}.{}", parts[0], parts[1], patch + 1);
        }
    }
    format!("{}.1", version)
}
