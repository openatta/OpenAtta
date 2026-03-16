//! AttaOS Native Tools
//!
//! 50+ native Rust tools for file I/O, shell execution, web access,
//! git operations, and more.

use std::sync::Arc;

// Re-export traits from atta-types
pub use atta_types::{CronScheduler, SubAgentRegistry};

/// Reference to the sub-agent registry (None if not configured)
pub type AgentRegistryRef = Option<Arc<dyn SubAgentRegistry>>;

/// Reference to the cron scheduler (None if not configured)
pub type CronSchedulerRef = Option<Arc<dyn CronScheduler>>;

/// Reference to the memory store (None if not configured)
#[cfg(feature = "memory")]
pub type MemoryStoreRef = Option<Arc<dyn atta_memory::MemoryStore>>;

pub mod agents_inbox;
pub mod agents_list;
pub mod agents_send;
pub mod apply_patch;
pub mod cli_discovery;
pub mod content_search;
pub mod cron;
pub mod cron_list;
pub mod cron_remove;
pub mod cron_run;
pub mod cron_runs;
pub mod cron_update;
pub mod delegate_status;
pub mod delegation;
pub mod file_edit;
pub mod file_read;
pub mod file_write;
pub mod git_ops;
pub mod glob_search;
pub mod image_info;
pub mod model_routing;
pub mod process;
pub mod proxy_config;
pub mod pushover;
pub mod registry;
pub mod schedule;
pub mod shell;
pub mod start_flow;
pub mod state_get;
pub mod state_set;
pub mod subagent_list;
pub mod subagent_manage;
pub mod subagent_spawn;
pub mod task_plan;
pub mod url_validation;

#[cfg(feature = "web")]
pub mod http_request;
#[cfg(feature = "web")]
pub mod web_fetch;
#[cfg(feature = "web")]
pub mod web_search;

#[cfg(feature = "memory")]
pub mod memory;

#[cfg(feature = "browser")]
pub mod browser;

#[cfg(feature = "browser-chromium")]
pub mod browser_chromium;

#[cfg(feature = "browser-cdp")]
pub mod browser_cdp;

pub mod pdf_read;
pub mod screenshot;

pub use registry::register_all_tools;
