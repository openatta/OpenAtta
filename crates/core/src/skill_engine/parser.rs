//! SKILL.md parser
//!
//! Parses SKILL.md files with YAML frontmatter and Markdown body.

use atta_types::{AttaError, SkillDef};

/// Parsed skill with definition and body
pub struct ParsedSkill {
    /// Skill definition from YAML frontmatter
    pub def: SkillDef,
    /// Markdown body (used as system_prompt if not specified in frontmatter)
    pub body: String,
}

/// Parse a SKILL.md file content into a skill definition
///
/// Format:
/// ```text
/// ---
/// name: my-skill
/// description: "A skill description"
/// version: "0.1.0"
/// ...
/// ---
///
/// # Skill Body (Markdown)
/// Instructions for the agent...
/// ```
pub fn parse_skill_md(content: &str) -> Result<ParsedSkill, AttaError> {
    let content = content.trim();

    // Split on YAML frontmatter delimiters
    if !content.starts_with("---") {
        return Err(AttaError::Validation(
            "SKILL.md must start with YAML frontmatter (---)".into(),
        ));
    }

    let after_first = &content[3..];
    let end_pos = after_first
        .find("\n---")
        .ok_or_else(|| AttaError::Validation("missing closing --- for YAML frontmatter".into()))?;

    let yaml_str = after_first[..end_pos].trim();
    let body = after_first[end_pos + 4..].trim().to_string();

    // Parse the YAML frontmatter into a SkillFrontmatter
    let fm: SkillFrontmatter =
        serde_yml::from_str(yaml_str).map_err(|e| AttaError::Validation(e.to_string()))?;

    // Build SkillDef
    let system_prompt = if body.is_empty() {
        fm.description.clone().unwrap_or_default()
    } else {
        body.clone()
    };

    let def = SkillDef {
        id: fm.id.unwrap_or_else(|| fm.name.clone()),
        version: fm.version.unwrap_or_else(|| "0.1.0".to_string()),
        name: Some(fm.name),
        description: fm.description,
        system_prompt,
        tools: fm.tools.unwrap_or_default(),
        steps: None,
        output_format: None,
        requires_approval: fm.requires_approval.unwrap_or(false),
        risk_level: fm
            .risk_level
            .and_then(|r| match r.as_str() {
                "low" => Some(atta_types::RiskLevel::Low),
                "medium" => Some(atta_types::RiskLevel::Medium),
                "high" => Some(atta_types::RiskLevel::High),
                _ => None,
            })
            .unwrap_or_default(),
        tags: fm.tags.unwrap_or_default(),
        variables: fm.variables.map(|vars| {
            vars.into_iter()
                .map(|v| atta_types::skill::VariableDef {
                    name: v.name,
                    description: v.description,
                    required: v.required.unwrap_or(false),
                    default: v.default,
                })
                .collect()
        }),
        author: fm.author,
        source: "builtin".to_string(),
    };

    Ok(ParsedSkill { def, body })
}

/// YAML frontmatter structure for SKILL.md
#[derive(serde::Deserialize)]
struct SkillFrontmatter {
    /// Explicit base58 ID (optional — auto-generated from name if missing)
    id: Option<String>,
    name: String,
    description: Option<String>,
    version: Option<String>,
    author: Option<String>,
    tags: Option<Vec<String>>,
    tools: Option<Vec<String>>,
    requires_approval: Option<bool>,
    risk_level: Option<String>,
    variables: Option<Vec<SkillVariable>>,
}

#[derive(serde::Deserialize)]
struct SkillVariable {
    name: String,
    description: Option<String>,
    required: Option<bool>,
    default: Option<serde_json::Value>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_skill_md() {
        let content = r#"---
name: code-review
description: "Review code for bugs and style"
version: "0.1.0"
author: "OpenAtta"
tags: [code, review]
tools: [file_read, glob_search, git_ops]
requires_approval: false
risk_level: low
variables:
  - name: target_branch
    description: "Branch to review"
    required: false
    default: "main"
---

# Code Review

You are a code reviewer. Analyze changes and provide feedback.
Use `git_ops` to view the diff against {{target_branch}}.
"#;

        let parsed = parse_skill_md(content).unwrap();
        // No explicit id → falls back to name
        assert_eq!(parsed.def.id, "code-review");
        assert_eq!(parsed.def.name.as_deref(), Some("code-review"));
        assert_eq!(parsed.def.version, "0.1.0");
        assert_eq!(parsed.def.tools.len(), 3);
        assert!(parsed.def.tools.contains(&"file_read".to_string()));
        assert_eq!(parsed.def.author.as_deref(), Some("OpenAtta"));
        assert!(!parsed.body.is_empty());
    }

    #[test]
    fn test_parse_skill_md_with_explicit_id() {
        let content = r#"---
id: Lb8bsmEbY1DXx1eAFkG4r5
name: code-review
description: "Review code"
---

# Code Review
You review code.
"#;

        let parsed = parse_skill_md(content).unwrap();
        assert_eq!(parsed.def.id, "Lb8bsmEbY1DXx1eAFkG4r5");
        assert_eq!(parsed.def.name.as_deref(), Some("code-review"));
    }

    #[test]
    fn test_parse_skill_md_no_frontmatter() {
        let content = "# No frontmatter";
        assert!(parse_skill_md(content).is_err());
    }

    #[test]
    fn test_parse_skill_md_unclosed_frontmatter() {
        let content = "---\nname: test\n# No closing";
        assert!(parse_skill_md(content).is_err());
    }
}
