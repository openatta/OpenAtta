//! Tool registration — collects all native tools

use std::sync::Arc;

use atta_types::NativeTool;

/// Register all available native tools
#[cfg(feature = "memory")]
pub fn register_all_tools(memory_store: crate::MemoryStoreRef) -> Vec<Arc<dyn NativeTool>> {
    let mut tools = register_core_tools();

    tools.push(Arc::new(crate::memory::MemoryStoreTool::new(
        memory_store.clone(),
    )));
    tools.push(Arc::new(crate::memory::MemoryRecallTool::new(
        memory_store.clone(),
    )));
    tools.push(Arc::new(crate::memory::MemoryForgetTool::new(memory_store)));

    tools
}

/// Register all available native tools (no memory feature)
#[cfg(not(feature = "memory"))]
pub fn register_all_tools() -> Vec<Arc<dyn NativeTool>> {
    register_core_tools()
}

/// Core tools shared across all feature configurations
fn register_core_tools() -> Vec<Arc<dyn NativeTool>> {
    #[allow(unused_mut)]
    let mut tools: Vec<Arc<dyn NativeTool>> = vec![
        // Core file tools
        Arc::new(crate::shell::ShellTool),
        Arc::new(crate::file_read::FileReadTool),
        Arc::new(crate::file_write::FileWriteTool),
        Arc::new(crate::file_edit::FileEditTool),
        Arc::new(crate::apply_patch::ApplyPatchTool),
        Arc::new(crate::glob_search::GlobSearchTool),
        Arc::new(crate::content_search::ContentSearchTool),
        // Git
        Arc::new(crate::git_ops::GitOpsTool),
        // Process management
        Arc::new(crate::process::ProcessTool),
        // Scheduling
        Arc::new(crate::cron::CronTool::default()),
        // Delegation
        Arc::new(crate::delegation::DelegationTool::default()),
        // Cron management
        Arc::new(crate::cron_list::CronListTool::default()),
        Arc::new(crate::cron_remove::CronRemoveTool::default()),
        Arc::new(crate::cron_update::CronUpdateTool::default()),
        Arc::new(crate::cron_run::CronRunTool::default()),
        Arc::new(crate::cron_runs::CronRunsTool::default()),
        // Sub-agent management
        Arc::new(crate::subagent_spawn::SubagentSpawnTool::new(None)),
        Arc::new(crate::subagent_list::SubagentListTool::new(None)),
        Arc::new(crate::subagent_manage::SubagentManageTool::new(None)),
        // Delegation status
        Arc::new(crate::delegate_status::DelegateStatusTool),
        // IPC: Agent communication
        Arc::new(crate::agents_list::AgentsListTool),
        Arc::new(crate::agents_send::AgentsSendTool),
        Arc::new(crate::agents_inbox::AgentsInboxTool),
        // IPC: Shared state
        Arc::new(crate::state_get::StateGetTool),
        Arc::new(crate::state_set::StateSetTool),
        // Task planning
        Arc::new(crate::task_plan::TaskPlanTool),
        // Image metadata
        Arc::new(crate::image_info::ImageInfoTool),
        // URL validation
        Arc::new(crate::url_validation::UrlValidationTool),
        // CLI discovery
        Arc::new(crate::cli_discovery::CliDiscoveryTool),
        // Push notifications
        Arc::new(crate::pushover::PushoverTool),
        // Enhanced scheduling
        Arc::new(crate::schedule::ScheduleTool),
        // Screenshots & PDF
        Arc::new(crate::screenshot::ScreenshotTool),
        Arc::new(crate::pdf_read::PdfReadTool),
        // Model routing
        Arc::new(crate::model_routing::ModelRoutingTool),
        // Proxy config
        Arc::new(crate::proxy_config::ProxyConfigTool),
        // Flow starter
        Arc::new(crate::start_flow::StartFlowTool::default()),
    ];

    // Web tools (feature-gated)
    #[cfg(feature = "web")]
    {
        tools.push(Arc::new(crate::web_fetch::WebFetchTool));
        tools.push(Arc::new(crate::web_search::WebSearchTool));
        tools.push(Arc::new(crate::http_request::HttpRequestTool));
    }

    // Browser automation (feature-gated)
    // Registers with StubBackend by default; the server can late-bind a real
    // backend (CDP or Chromium) after async initialization.
    #[cfg(feature = "browser")]
    {
        tools.push(Arc::new(crate::browser::BrowserTool::default()));
    }

    tools
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_register_all_tools() {
        #[cfg(feature = "memory")]
        let tools = register_all_tools(None);
        #[cfg(not(feature = "memory"))]
        let tools = register_all_tools();

        assert!(tools.len() >= 35);

        // Check names are unique
        let names: Vec<&str> = tools.iter().map(|t| t.name()).collect();
        let mut deduped = names.clone();
        deduped.sort();
        deduped.dedup();
        assert_eq!(names.len(), deduped.len(), "duplicate tool names found");
    }
}
