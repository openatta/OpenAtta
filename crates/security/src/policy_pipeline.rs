//! Tool profile pipeline — restricts available tools by category
//!
//! Defines pre-built profiles (Minimal, Coding, Research, etc.) that map
//! to groups of tool names. SecurityGuard uses `filter_tools_by_profile`
//! to limit which tool schemas are exposed to the LLM.

use atta_types::ToolSchema;

use crate::policy::ToolProfile;

/// File system tools
pub const GROUP_FS: &[&str] = &[
    "file_read",
    "file_write",
    "file_edit",
    "file_list",
    "apply_patch",
];

/// Shell / process tools
pub const GROUP_SHELL: &[&str] = &["shell", "bash", "exec", "process"];

/// Web / network tools
pub const GROUP_WEB: &[&str] = &["web_fetch", "web_search", "http_request", "browser"];

/// Search / indexing tools
pub const GROUP_SEARCH: &[&str] = &["search", "grep", "find"];

/// Memory / context tools
pub const GROUP_MEMORY: &[&str] = &["memory_store", "memory_search", "memory_recall"];

/// Communication / messaging tools
pub const GROUP_MESSAGING: &[&str] = &["send_message", "email", "notification", "slack"];

/// Agent delegation tools
pub const GROUP_DELEGATION: &[&str] = &["delegation", "delegate"];

/// Code analysis tools
pub const GROUP_CODE: &[&str] = &["code_analysis", "lint", "format", "test"];

/// Resolve a profile to its allowed tool names.
/// Returns `None` for `Full` (all tools allowed).
pub fn resolve_profile(profile: &ToolProfile) -> Option<Vec<&'static str>> {
    match profile {
        ToolProfile::Full => None,
        ToolProfile::Minimal => {
            let mut tools = Vec::new();
            tools.extend_from_slice(&["file_read", "file_list"]);
            tools.extend_from_slice(GROUP_SEARCH);
            Some(tools)
        }
        ToolProfile::Coding => {
            let mut tools = Vec::new();
            tools.extend_from_slice(GROUP_FS);
            tools.extend_from_slice(GROUP_SHELL);
            tools.extend_from_slice(GROUP_SEARCH);
            tools.extend_from_slice(GROUP_CODE);
            tools.extend_from_slice(GROUP_MEMORY);
            Some(tools)
        }
        ToolProfile::Messaging => {
            let mut tools = Vec::new();
            tools.extend_from_slice(GROUP_MESSAGING);
            tools.extend_from_slice(&["file_read"]);
            tools.extend_from_slice(GROUP_SEARCH);
            Some(tools)
        }
        ToolProfile::Research => {
            let mut tools = Vec::new();
            tools.extend_from_slice(GROUP_WEB);
            tools.extend_from_slice(GROUP_SEARCH);
            tools.extend_from_slice(GROUP_MEMORY);
            tools.extend_from_slice(&["file_read"]);
            Some(tools)
        }
        ToolProfile::Custom(_) => None, // handled separately
    }
}

/// Filter tool schemas by the given profile
pub fn filter_tools_by_profile(tools: &[ToolSchema], profile: &ToolProfile) -> Vec<ToolSchema> {
    match profile {
        ToolProfile::Full => tools.to_vec(),
        ToolProfile::Custom(names) => tools
            .iter()
            .filter(|t| names.iter().any(|n| n == &t.name))
            .cloned()
            .collect(),
        _ => {
            if let Some(allowed) = resolve_profile(profile) {
                tools
                    .iter()
                    .filter(|t| allowed.contains(&t.name.as_str()))
                    .cloned()
                    .collect()
            } else {
                tools.to_vec()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_schema(name: &str) -> ToolSchema {
        ToolSchema {
            name: name.to_string(),
            description: "test".to_string(),
            parameters: serde_json::json!({}),
        }
    }

    #[test]
    fn test_full_profile_returns_all() {
        let tools = vec![make_schema("file_read"), make_schema("shell")];
        let filtered = filter_tools_by_profile(&tools, &ToolProfile::Full);
        assert_eq!(filtered.len(), 2);
    }

    #[test]
    fn test_minimal_profile_filters() {
        let tools = vec![
            make_schema("file_read"),
            make_schema("shell"),
            make_schema("search"),
        ];
        let filtered = filter_tools_by_profile(&tools, &ToolProfile::Minimal);
        assert!(filtered.iter().any(|t| t.name == "file_read"));
        assert!(filtered.iter().any(|t| t.name == "search"));
        assert!(!filtered.iter().any(|t| t.name == "shell"));
    }

    #[test]
    fn test_custom_profile() {
        let tools = vec![
            make_schema("file_read"),
            make_schema("shell"),
            make_schema("web_fetch"),
        ];
        let filtered =
            filter_tools_by_profile(&tools, &ToolProfile::Custom(vec!["shell".to_string()]));
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].name, "shell");
    }

    #[test]
    fn test_coding_profile() {
        let tools = vec![
            make_schema("file_read"),
            make_schema("file_write"),
            make_schema("shell"),
            make_schema("web_fetch"),
            make_schema("send_message"),
        ];
        let filtered = filter_tools_by_profile(&tools, &ToolProfile::Coding);
        assert!(filtered.iter().any(|t| t.name == "file_read"));
        assert!(filtered.iter().any(|t| t.name == "shell"));
        assert!(!filtered.iter().any(|t| t.name == "web_fetch"));
        assert!(!filtered.iter().any(|t| t.name == "send_message"));
    }

    #[test]
    fn test_research_profile() {
        let tools = vec![
            make_schema("web_fetch"),
            make_schema("web_search"),
            make_schema("search"),
            make_schema("shell"),
        ];
        let filtered = filter_tools_by_profile(&tools, &ToolProfile::Research);
        assert!(filtered.iter().any(|t| t.name == "web_fetch"));
        assert!(filtered.iter().any(|t| t.name == "search"));
        assert!(!filtered.iter().any(|t| t.name == "shell"));
    }
}
