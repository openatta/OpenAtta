//! 包管理类型

use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// 包类型
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PackageType {
    Plugin,
    Flow,
    Skill,
    Tool,
    Mcp,
}

/// 已安装包记录
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageRecord {
    pub name: String,
    pub version: String,
    pub package_type: PackageType,
    pub installed_at: DateTime<Utc>,
    pub installed_by: String,
}

/// Service Account（Enterprise API Key 认证）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceAccount {
    pub id: uuid::Uuid,
    pub name: String,
    pub api_key_hash: String,
    pub roles: Vec<crate::auth::Role>,
    pub created_at: DateTime<Utc>,
    pub enabled: bool,
}

/// manifest.json 解析结构
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub spec_version: String,
    pub package: ManifestPackage,
    pub runtime: Option<ManifestRuntime>,
    pub permissions: Option<Vec<String>>,
    pub dependencies: Option<Vec<ManifestDependency>>,
    pub resource_limits: Option<ManifestResourceLimits>,
    pub integrity: ManifestIntegrity,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestPackage {
    pub name: String,
    pub version: String,
    #[serde(rename = "type")]
    pub package_type: PackageType,
    pub description: Option<String>,
    pub author: Option<String>,
    pub organization: Option<String>,
    pub license: Option<String>,
    pub homepage: Option<String>,
    pub keywords: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestRuntime {
    pub engine: String,
    pub component_model: bool,
    pub wit_interface: String,
    pub min_host_version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestDependency {
    pub name: String,
    pub version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestResourceLimits {
    pub max_memory_mb: u64,
    pub timeout_seconds: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestIntegrity {
    pub content_sha256: String,
}

/// 包来源
#[derive(Debug, Clone)]
pub enum PackageSource {
    File(PathBuf),
    Url(String),
}

/// 依赖解析结果
#[derive(Debug, Clone)]
pub enum ResolvedDep {
    Satisfied(PackageRecord),
    VersionMismatch { required: String, installed: String },
    Missing(String),
}

/// 已安装包
#[derive(Debug, Clone)]
pub struct InstalledPackage {
    pub manifest: Manifest,
}
