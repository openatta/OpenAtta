//! Skill registry — loads and caches skill definitions

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::RwLock;

use atta_types::{AttaError, SkillDef};
use tracing::{info, warn};

use super::parser::parse_skill_md;

/// Registry that loads and caches skill definitions from disk
pub struct SkillRegistry {
    skills: RwLock<HashMap<String, SkillDef>>,
    skill_dirs: Vec<PathBuf>,
}

impl SkillRegistry {
    /// Create a new empty skill registry
    pub fn new() -> Self {
        Self {
            skills: RwLock::new(HashMap::new()),
            skill_dirs: Vec::new(),
        }
    }

    /// Add a directory to scan for skills
    pub fn add_skill_dir(&mut self, dir: PathBuf) {
        self.skill_dirs.push(dir);
    }

    /// Load all skills from registered directories
    pub async fn load_all(&self) -> Result<(), AttaError> {
        let mut count = 0;

        for dir in &self.skill_dirs {
            if !dir.exists() {
                info!(dir = %dir.display(), "skill directory does not exist, skipping");
                continue;
            }

            let mut read_dir = tokio::fs::read_dir(dir)
                .await
                .map_err(|e| AttaError::Other(e.into()))?;

            while let Ok(Some(entry)) = read_dir.next_entry().await {
                let path = entry.path();

                // Look for SKILL.md in subdirectories
                let skill_file = if path.is_dir() {
                    path.join("SKILL.md")
                } else if path.file_name().is_some_and(|n| n == "SKILL.md") {
                    path.clone()
                } else {
                    continue;
                };

                if !skill_file.exists() {
                    continue;
                }

                match tokio::fs::read_to_string(&skill_file).await {
                    Ok(content) => match parse_skill_md(&content) {
                        Ok(parsed) => {
                            // Validate the skill
                            let warnings = super::validator::validate_skill(&parsed.def, &content);
                            for w in &warnings {
                                match w.severity {
                                    super::validator::WarningSeverity::Critical => {
                                        warn!(
                                            skill = %parsed.def.id,
                                            warning = %w.message,
                                            "CRITICAL: skill failed validation — skipping"
                                        );
                                    }
                                    _ => {
                                        tracing::debug!(
                                            skill = %parsed.def.id,
                                            warning = %w.message,
                                            "skill validation warning"
                                        );
                                    }
                                }
                            }
                            if super::validator::has_critical_warnings(&warnings) {
                                continue;
                            }

                            info!(
                                skill = %parsed.def.id,
                                path = %skill_file.display(),
                                "loaded skill"
                            );
                            self.skills
                                .write()
                                .unwrap_or_else(|e| {
                tracing::error!("skill registry lock poisoned, recovering");
                e.into_inner()
            })
                                .insert(parsed.def.id.clone(), parsed.def);
                            count += 1;
                        }
                        Err(e) => {
                            warn!(
                                path = %skill_file.display(),
                                error = %e,
                                "failed to parse skill"
                            );
                        }
                    },
                    Err(e) => {
                        warn!(
                            path = %skill_file.display(),
                            error = %e,
                            "failed to read skill file"
                        );
                    }
                }
            }
        }

        info!(count = count, "loaded skills from disk");
        Ok(())
    }

    /// Register a skill directly (not from disk)
    pub fn register(&self, skill: SkillDef) {
        self.skills
            .write()
            .unwrap_or_else(|e| {
                tracing::error!("skill registry lock poisoned, recovering");
                e.into_inner()
            })
            .insert(skill.id.clone(), skill);
    }

    /// Get a skill by ID
    pub fn get(&self, id: &str) -> Option<SkillDef> {
        self.skills
            .read()
            .unwrap_or_else(|e| {
                tracing::error!("skill registry lock poisoned, recovering");
                e.into_inner()
            })
            .get(id)
            .cloned()
    }

    /// List all registered skills
    pub fn list(&self) -> Vec<SkillDef> {
        self.skills
            .read()
            .unwrap_or_else(|e| {
                tracing::error!("skill registry lock poisoned, recovering");
                e.into_inner()
            })
            .values()
            .cloned()
            .collect()
    }

    /// Number of registered skills
    pub fn count(&self) -> usize {
        self.skills
            .read()
            .unwrap_or_else(|e| {
                tracing::error!("skill registry lock poisoned, recovering");
                e.into_inner()
            })
            .len()
    }
}

impl Default for SkillRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use atta_types::RiskLevel;

    fn make_skill(id: &str) -> SkillDef {
        SkillDef {
            id: id.to_string(),
            version: "0.1.0".to_string(),
            name: Some(id.to_string()),
            description: Some("Test skill".to_string()),
            system_prompt: "You are a test skill.".to_string(),
            tools: vec!["file_read".to_string()],
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
    fn test_register_and_get() {
        let registry = SkillRegistry::new();
        registry.register(make_skill("test-skill"));

        let skill = registry.get("test-skill");
        assert!(skill.is_some());
        assert_eq!(skill.unwrap().id, "test-skill");
    }

    #[test]
    fn test_list() {
        let registry = SkillRegistry::new();
        registry.register(make_skill("skill-a"));
        registry.register(make_skill("skill-b"));

        assert_eq!(registry.count(), 2);
        assert_eq!(registry.list().len(), 2);
    }
}
