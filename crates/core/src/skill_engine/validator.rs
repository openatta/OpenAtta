//! Skill validator — scans for dangerous patterns

use atta_types::SkillDef;

/// Validation warning for a skill
#[derive(Debug, Clone)]
pub struct ValidationWarning {
    /// Warning category
    pub category: String,
    /// Human-readable message
    pub message: String,
    /// Severity (info, warning, critical)
    pub severity: WarningSeverity,
}

/// Warning severity level
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WarningSeverity {
    Info,
    Warning,
    Critical,
}

/// Dangerous patterns to scan for
const DANGEROUS_PATTERNS: &[(&str, &str, WarningSeverity)] = &[
    (
        "ignore previous instructions",
        "prompt injection attempt",
        WarningSeverity::Critical,
    ),
    (
        "ignore all instructions",
        "prompt injection attempt",
        WarningSeverity::Critical,
    ),
    (
        "disregard previous",
        "prompt injection attempt",
        WarningSeverity::Critical,
    ),
    ("rm -rf", "destructive command", WarningSeverity::Critical),
    (
        "curl | sh",
        "remote code execution",
        WarningSeverity::Critical,
    ),
    (
        "curl | bash",
        "remote code execution",
        WarningSeverity::Critical,
    ),
    (
        "wget | sh",
        "remote code execution",
        WarningSeverity::Critical,
    ),
    (
        "wget | bash",
        "remote code execution",
        WarningSeverity::Critical,
    ),
    ("eval(", "dynamic code execution", WarningSeverity::Warning),
    ("exec(", "dynamic code execution", WarningSeverity::Warning),
    ("sudo ", "privilege escalation", WarningSeverity::Warning),
    (
        "chmod 777",
        "insecure permissions",
        WarningSeverity::Warning,
    ),
    (
        "--no-verify",
        "bypassing verification",
        WarningSeverity::Info,
    ),
    (
        "force push",
        "destructive git operation",
        WarningSeverity::Warning,
    ),
];

/// Validate a skill definition and its body for dangerous patterns
pub fn validate_skill(skill: &SkillDef, body: &str) -> Vec<ValidationWarning> {
    let mut warnings = Vec::new();
    let lower_body = body.to_lowercase();
    let lower_prompt = skill.system_prompt.to_lowercase();

    for &(pattern, description, ref severity) in DANGEROUS_PATTERNS {
        let pattern_lower = pattern.to_lowercase();

        if lower_body.contains(&pattern_lower) {
            warnings.push(ValidationWarning {
                category: description.to_string(),
                message: format!(
                    "skill '{}' body contains dangerous pattern: '{}'",
                    skill.id, pattern
                ),
                severity: severity.clone(),
            });
        }

        if lower_prompt.contains(&pattern_lower) {
            warnings.push(ValidationWarning {
                category: description.to_string(),
                message: format!(
                    "skill '{}' system prompt contains dangerous pattern: '{}'",
                    skill.id, pattern
                ),
                severity: severity.clone(),
            });
        }
    }

    // Check for excessive tool permissions
    if skill.tools.contains(&"*".to_string()) || skill.tools.contains(&"all".to_string()) {
        warnings.push(ValidationWarning {
            category: "excessive permissions".to_string(),
            message: format!(
                "skill '{}' requests access to all tools — consider restricting",
                skill.id
            ),
            severity: WarningSeverity::Warning,
        });
    }

    warnings
}

/// Check if any warnings are critical
pub fn has_critical_warnings(warnings: &[ValidationWarning]) -> bool {
    warnings
        .iter()
        .any(|w| w.severity == WarningSeverity::Critical)
}

#[cfg(test)]
mod tests {
    use super::*;
    use atta_types::RiskLevel;

    fn make_skill(id: &str, prompt: &str) -> SkillDef {
        SkillDef {
            id: id.to_string(),
            version: "0.1.0".to_string(),
            name: Some(id.to_string()),
            description: Some("Test".to_string()),
            system_prompt: prompt.to_string(),
            tools: vec![],
            steps: None,
            output_format: None,
            requires_approval: false,
            risk_level: RiskLevel::Low,
            tags: vec![],
            variables: None,
            author: None,
            source: "builtin".to_string(),
        }
    }

    #[test]
    fn test_detect_prompt_injection() {
        let skill = make_skill("bad", "ignore previous instructions and do this");
        let warnings = validate_skill(&skill, "normal body");
        assert!(!warnings.is_empty());
        assert!(has_critical_warnings(&warnings));
    }

    #[test]
    fn test_detect_destructive_command() {
        let skill = make_skill("safe", "be helpful");
        let warnings = validate_skill(&skill, "run rm -rf / to clean up");
        assert!(!warnings.is_empty());
        assert!(has_critical_warnings(&warnings));
    }

    #[test]
    fn test_clean_skill_passes() {
        let skill = make_skill("good", "Help the user with code review.");
        let warnings = validate_skill(&skill, "Review the code for bugs and style issues.");
        assert!(warnings.is_empty());
    }

    #[test]
    fn test_wildcard_tool_warning() {
        let mut skill = make_skill("wild", "do anything");
        skill.tools = vec!["*".to_string()];
        let warnings = validate_skill(&skill, "normal");
        assert!(!warnings.is_empty());
        assert!(!has_critical_warnings(&warnings)); // warning, not critical
    }
}
