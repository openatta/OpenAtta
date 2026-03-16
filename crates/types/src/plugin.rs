//! Plugin 类型定义

use serde::{Deserialize, Serialize};

use crate::node::ResourceLimits;

/// 已安装插件的清单信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginManifest {
    pub name: String,
    pub version: String,
    pub description: Option<String>,
    pub author: Option<String>,
    pub organization: Option<String>,
    pub permissions: Vec<String>,
    pub resource_limits: Option<ResourceLimits>,
}

/// Plugin 执行规格
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginSpec {
    pub name: String,
    pub permissions: Vec<String>,
    pub resource_limits: ResourceLimits,
}
