//! Skill sync — git clone/pull from a remote repository

use std::path::PathBuf;
use std::time::Duration;

use atta_types::AttaError;
use tracing::{info, warn};

/// Configuration for skill sync
pub struct SkillSyncConfig {
    /// Remote repository URL
    pub repo_url: String,
    /// Local directory for synced skills
    pub local_dir: PathBuf,
    /// Minimum interval between syncs
    pub sync_interval: Duration,
    /// Git branch to sync
    pub branch: String,
    /// Whether to validate skills after sync
    pub validate: bool,
    /// Whether sync is enabled at all
    pub enabled: bool,
}

impl SkillSyncConfig {
    /// Create a new config with explicit paths.
    ///
    /// `cache_dir` is typically `$ATTA_HOME/cache/open-skills`.
    pub fn new(cache_dir: PathBuf, repo_url: String, interval_secs: u64, enabled: bool) -> Self {
        Self {
            repo_url,
            local_dir: cache_dir,
            sync_interval: Duration::from_secs(interval_secs),
            branch: "main".to_string(),
            validate: true,
            enabled,
        }
    }
}

/// Skill synchronizer — clones/pulls a remote skills repository
pub struct SkillSync {
    config: SkillSyncConfig,
}

impl SkillSync {
    /// Create a new SkillSync with the given config
    pub fn new(config: SkillSyncConfig) -> Self {
        Self { config }
    }

    /// Whether sync is enabled
    pub fn enabled(&self) -> bool {
        self.config.enabled
    }

    /// Check if a sync is needed based on the marker file timestamp
    pub fn needs_sync(&self) -> bool {
        if !self.config.enabled {
            return false;
        }

        let marker = self.marker_path();
        if !marker.exists() {
            return true;
        }

        match std::fs::metadata(&marker) {
            Ok(meta) => {
                let modified = meta.modified().unwrap_or(std::time::SystemTime::UNIX_EPOCH);
                let elapsed = std::time::SystemTime::now()
                    .duration_since(modified)
                    .unwrap_or(Duration::from_secs(u64::MAX));
                elapsed >= self.config.sync_interval
            }
            Err(_) => true,
        }
    }

    /// Perform the sync (git clone or git pull)
    pub async fn sync(&self) -> Result<PathBuf, AttaError> {
        // Ensure parent directory exists
        if let Some(parent) = self.config.local_dir.parent() {
            std::fs::create_dir_all(parent)?;
        }

        if self.config.local_dir.join(".git").exists() {
            // Pull existing repo
            info!(
                dir = %self.config.local_dir.display(),
                "pulling open-skills repository"
            );

            let output = tokio::process::Command::new("git")
                .args(["pull", "--ff-only", "origin", &self.config.branch])
                .current_dir(&self.config.local_dir)
                .output()
                .await
                .map_err(|e| AttaError::Other(anyhow::anyhow!("git pull failed: {e}")))?;

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                warn!(error = %stderr, "git pull failed, will try fresh clone");
                // On pull failure, remove and re-clone
                let _ = std::fs::remove_dir_all(&self.config.local_dir);
                return self.clone_repo().await;
            }
        } else {
            return self.clone_repo().await;
        }

        self.update_marker()?;
        info!(
            dir = %self.config.local_dir.display(),
            "open-skills sync completed"
        );
        Ok(self.config.local_dir.clone())
    }

    /// Clone the repository fresh
    async fn clone_repo(&self) -> Result<PathBuf, AttaError> {
        info!(
            url = %self.config.repo_url,
            dir = %self.config.local_dir.display(),
            "cloning open-skills repository"
        );

        let output = tokio::process::Command::new("git")
            .args([
                "clone",
                "--depth",
                "1",
                "--branch",
                &self.config.branch,
                &self.config.repo_url,
                self.config.local_dir.to_string_lossy().as_ref(),
            ])
            .output()
            .await
            .map_err(|e| AttaError::Other(anyhow::anyhow!("git clone failed: {e}")))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(AttaError::Other(anyhow::anyhow!(
                "git clone failed: {stderr}"
            )));
        }

        self.update_marker()?;
        Ok(self.config.local_dir.clone())
    }

    /// Update the sync marker timestamp
    fn update_marker(&self) -> Result<(), AttaError> {
        let marker = self.marker_path();
        std::fs::write(&marker, chrono::Utc::now().to_rfc3339())?;
        Ok(())
    }

    /// Path to the sync marker file
    fn marker_path(&self) -> PathBuf {
        self.config.local_dir.with_extension("sync-marker")
    }

    /// Get the local skills directory path
    pub fn local_dir(&self) -> &PathBuf {
        &self.config.local_dir
    }
}
