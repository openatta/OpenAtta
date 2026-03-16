//! Skill executor — system prompt injection and tool filtering

use atta_types::{SkillDef, ToolRegistry, ToolSchema};

/// Build a system prompt for a skill with variable interpolation
///
/// Replaces `{{variable}}` placeholders with actual values from the
/// provided variables map. Appends available tool list.
pub fn build_skill_system_prompt(skill: &SkillDef, variables: &serde_json::Value) -> String {
    let mut prompt = skill.system_prompt.clone();

    // Replace {{variable}} placeholders
    if let Some(var_defs) = &skill.variables {
        for var_def in var_defs {
            let placeholder = format!("{{{{{}}}}}", var_def.name);
            let value = variables
                .get(&var_def.name)
                .and_then(|v| v.as_str())
                .or_else(|| var_def.default.as_ref().and_then(|d| d.as_str()))
                .unwrap_or("");

            prompt = prompt.replace(&placeholder, value);
        }
    }

    // Append tool list
    if !skill.tools.is_empty() {
        prompt.push_str("\n\n## Available Tools\n");
        for tool_name in &skill.tools {
            prompt.push_str(&format!("- `{}`\n", tool_name));
        }
    }

    // Append steps if defined
    if let Some(steps) = &skill.steps {
        prompt.push_str("\n## Execution Steps\n");
        for (i, step) in steps.iter().enumerate() {
            if let Some(desc) = &step.description {
                prompt.push_str(&format!("{}. {} — {}\n", i + 1, step.action, desc));
            } else {
                prompt.push_str(&format!("{}. {}\n", i + 1, step.action));
            }
        }
    }

    prompt
}

/// Filter the tool registry to only include tools listed in the skill
pub fn filter_tools_for_skill(registry: &dyn ToolRegistry, skill: &SkillDef) -> Vec<ToolSchema> {
    if skill.tools.is_empty() {
        // No tool restriction — return all
        return registry.list_schemas();
    }

    skill
        .tools
        .iter()
        .filter_map(|name| registry.get_schema(name))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use atta_types::RiskLevel;

    fn make_skill() -> SkillDef {
        SkillDef {
            id: "test".to_string(),
            version: "0.1.0".to_string(),
            name: Some("test".to_string()),
            description: None,
            system_prompt: "Review code on {{target_branch}} branch.".to_string(),
            tools: vec!["file_read".to_string(), "git_ops".to_string()],
            steps: None,
            output_format: None,
            requires_approval: false,
            risk_level: RiskLevel::Low,
            tags: vec![],
            variables: Some(vec![atta_types::skill::VariableDef {
                name: "target_branch".to_string(),
                description: Some("Branch to review".to_string()),
                required: false,
                default: Some(serde_json::json!("main")),
            }]),
            author: None,
            source: "builtin".to_string(),
        }
    }

    #[test]
    fn test_build_system_prompt_with_variables() {
        let skill = make_skill();
        let vars = serde_json::json!({"target_branch": "develop"});
        let prompt = build_skill_system_prompt(&skill, &vars);

        assert!(prompt.contains("develop"));
        assert!(!prompt.contains("{{target_branch}}"));
        assert!(prompt.contains("`file_read`"));
        assert!(prompt.contains("`git_ops`"));
    }

    #[test]
    fn test_build_system_prompt_with_defaults() {
        let skill = make_skill();
        let vars = serde_json::json!({});
        let prompt = build_skill_system_prompt(&skill, &vars);

        assert!(prompt.contains("main"));
    }
}
