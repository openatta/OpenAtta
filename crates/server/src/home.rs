//! AttaHome — $ATTA_HOME directory layout management
//!
//! Resolution order: `--home` CLI arg → `ATTA_HOME` env → `~/.atta`

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use tracing::info;

/// Represents the AttaOS home directory and its subdirectory layout.
///
/// ```text
/// $ATTA_HOME/
/// ├── bin/              # attaos binary
/// ├── etc/              # attaos.yaml
/// ├── data/             # data.db, estop.json
/// ├── log/              # log files
/// ├── cache/            # open-skills sync cache
/// ├── models/           # embedding model cache (fastembed)
/// ├── run/              # attaos.pid
/// ├── lib/
/// │   ├── webui/        # Vue SPA dist files
/// │   ├── skills/       # built-in skills (atta-*)
/// │   ├── flows/        # built-in flow templates
/// │   └── tools/        # reserved for v0.2
/// └── exts/
///     ├── skills/       # user/third-party skills
///     ├── flows/        # user flow definitions
///     ├── tools/        # user tool definitions (v0.2)
///     └── mcp/          # MCP server configs
/// ```
pub struct AttaHome {
    root: PathBuf,
}

impl AttaHome {
    /// Resolve the ATTA_HOME directory.
    ///
    /// Priority: `cli_home` arg → `ATTA_HOME` env var → `~/.atta`
    pub fn resolve(cli_home: Option<&str>) -> Self {
        let root = if let Some(home) = cli_home {
            PathBuf::from(home)
        } else if let Ok(env_home) = std::env::var("ATTA_HOME") {
            PathBuf::from(env_home)
        } else {
            let user_home = std::env::var("HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|_| PathBuf::from("."));
            user_home.join(".atta")
        };

        Self { root }
    }

    /// Create all required subdirectories under `$ATTA_HOME`.
    pub fn ensure_dirs(&self) -> Result<()> {
        let dirs = [
            self.bin(),
            self.etc(),
            self.data(),
            self.log(),
            self.cache(),
            self.models(),
            self.run(),
            self.lib_webui(),
            self.lib_skills(),
            self.lib_flows(),
            self.lib_tools(),
            self.exts_skills(),
            self.exts_flows(),
            self.exts_tools(),
            self.exts_mcp(),
        ];

        for dir in &dirs {
            std::fs::create_dir_all(dir)
                .with_context(|| format!("failed to create directory: {}", dir.display()))?;
        }

        Ok(())
    }

    /// Root directory (`$ATTA_HOME`)
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Binary directory (`$ATTA_HOME/bin/`)
    pub fn bin(&self) -> PathBuf {
        self.root.join("bin")
    }

    /// Configuration directory (`$ATTA_HOME/etc/`)
    pub fn etc(&self) -> PathBuf {
        self.root.join("etc")
    }

    /// Data directory (`$ATTA_HOME/data/`)
    pub fn data(&self) -> PathBuf {
        self.root.join("data")
    }

    /// Log directory (`$ATTA_HOME/log/`)
    pub fn log(&self) -> PathBuf {
        self.root.join("log")
    }

    /// Cache directory (`$ATTA_HOME/cache/`)
    pub fn cache(&self) -> PathBuf {
        self.root.join("cache")
    }

    /// Models directory (`$ATTA_HOME/models/`)
    pub fn models(&self) -> PathBuf {
        self.root.join("models")
    }

    /// Runtime directory (`$ATTA_HOME/run/`)
    pub fn run(&self) -> PathBuf {
        self.root.join("run")
    }

    /// Library directory (`$ATTA_HOME/lib/`)
    #[allow(dead_code)]
    pub fn lib(&self) -> PathBuf {
        self.root.join("lib")
    }

    /// Built-in skills directory (`$ATTA_HOME/lib/skills/`)
    pub fn lib_skills(&self) -> PathBuf {
        self.root.join("lib/skills")
    }

    /// Built-in flow templates directory (`$ATTA_HOME/lib/flows/`)
    pub fn lib_flows(&self) -> PathBuf {
        self.root.join("lib/flows")
    }

    /// Built-in WebUI directory (`$ATTA_HOME/lib/webui/`)
    pub fn lib_webui(&self) -> PathBuf {
        self.root.join("lib/webui")
    }

    /// Built-in tools directory (`$ATTA_HOME/lib/tools/`) — reserved for v0.2
    pub fn lib_tools(&self) -> PathBuf {
        self.root.join("lib/tools")
    }

    /// Extensions directory (`$ATTA_HOME/exts/`)
    #[allow(dead_code)]
    pub fn exts(&self) -> PathBuf {
        self.root.join("exts")
    }

    /// User/third-party skills directory (`$ATTA_HOME/exts/skills/`)
    pub fn exts_skills(&self) -> PathBuf {
        self.root.join("exts/skills")
    }

    /// User flow definitions directory (`$ATTA_HOME/exts/flows/`)
    pub fn exts_flows(&self) -> PathBuf {
        self.root.join("exts/flows")
    }

    /// User tool definitions directory (`$ATTA_HOME/exts/tools/`) — reserved for v0.2
    pub fn exts_tools(&self) -> PathBuf {
        self.root.join("exts/tools")
    }

    /// MCP server configs directory (`$ATTA_HOME/exts/mcp/`)
    pub fn exts_mcp(&self) -> PathBuf {
        self.root.join("exts/mcp")
    }

    /// Seed built-in skills and flows from the project source tree (dev mode).
    ///
    /// If `$ATTA_HOME/lib/skills/` is empty and a `skills/` directory exists
    /// in the current working directory, recursively copy it over.
    /// Same for `flows/`. Existing content is never overwritten.
    pub fn seed_builtins(&self) -> Result<()> {
        let cwd = std::env::current_dir().unwrap_or_default();

        Self::copy_dir_if_empty(&cwd.join("skills"), &self.lib_skills(), "skills")?;
        Self::copy_dir_if_empty(&cwd.join("flows"), &self.lib_flows(), "flows")?;

        Ok(())
    }

    /// Recursively copy `src` → `dst` only when `dst` is empty and `src` exists.
    fn copy_dir_if_empty(src: &Path, dst: &Path, label: &str) -> Result<()> {
        if !src.exists() {
            return Ok(());
        }

        // Check whether dst already has content
        let has_content = dst.exists()
            && std::fs::read_dir(dst)
                .map(|mut rd| rd.next().is_some())
                .unwrap_or(false);

        if has_content {
            return Ok(());
        }

        Self::copy_dir_recursive(src, dst)?;
        info!(src = %src.display(), dst = %dst.display(), "seeded built-in {label}");
        Ok(())
    }

    /// Recursive directory copy helper.
    fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<()> {
        std::fs::create_dir_all(dst)
            .with_context(|| format!("failed to create {}", dst.display()))?;

        for entry in std::fs::read_dir(src)
            .with_context(|| format!("failed to read {}", src.display()))?
        {
            let entry = entry?;
            let src_path = entry.path();
            let dst_path = dst.join(entry.file_name());

            if src_path.is_dir() {
                Self::copy_dir_recursive(&src_path, &dst_path)?;
            } else {
                std::fs::copy(&src_path, &dst_path).with_context(|| {
                    format!(
                        "failed to copy {} → {}",
                        src_path.display(),
                        dst_path.display()
                    )
                })?;
            }
        }
        Ok(())
    }

    /// Config file path (`$ATTA_HOME/etc/attaos.yaml`)
    pub fn config_file(&self) -> PathBuf {
        self.etc().join("attaos.yaml")
    }

    /// Keys env file path (`$ATTA_HOME/etc/keys.env`)
    pub fn keys_env(&self) -> PathBuf {
        self.etc().join("keys.env")
    }

    /// Database file path (`$ATTA_HOME/data/data.db`)
    pub fn database(&self) -> PathBuf {
        self.data().join("data.db")
    }

    /// E-Stop state file path (`$ATTA_HOME/data/estop.json`)
    pub fn estop_file(&self) -> PathBuf {
        self.data().join("estop.json")
    }

    /// PID file path (`$ATTA_HOME/run/attaos.pid`)
    pub fn pid_file(&self) -> PathBuf {
        self.run().join("attaos.pid")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_from_cli() {
        let home = AttaHome::resolve(Some("/tmp/test-atta"));
        assert_eq!(home.root(), Path::new("/tmp/test-atta"));
    }

    #[test]
    fn test_directory_layout() {
        let home = AttaHome::resolve(Some("/tmp/test-atta"));
        assert_eq!(home.bin(), PathBuf::from("/tmp/test-atta/bin"));
        assert_eq!(home.etc(), PathBuf::from("/tmp/test-atta/etc"));
        assert_eq!(home.data(), PathBuf::from("/tmp/test-atta/data"));
        assert_eq!(home.log(), PathBuf::from("/tmp/test-atta/log"));
        assert_eq!(home.cache(), PathBuf::from("/tmp/test-atta/cache"));
        assert_eq!(home.models(), PathBuf::from("/tmp/test-atta/models"));
        assert_eq!(home.run(), PathBuf::from("/tmp/test-atta/run"));
        assert_eq!(
            home.lib_skills(),
            PathBuf::from("/tmp/test-atta/lib/skills")
        );
        assert_eq!(home.lib_flows(), PathBuf::from("/tmp/test-atta/lib/flows"));
        assert_eq!(home.lib_webui(), PathBuf::from("/tmp/test-atta/lib/webui"));
        assert_eq!(home.lib_tools(), PathBuf::from("/tmp/test-atta/lib/tools"));
        assert_eq!(
            home.exts_skills(),
            PathBuf::from("/tmp/test-atta/exts/skills")
        );
        assert_eq!(
            home.exts_flows(),
            PathBuf::from("/tmp/test-atta/exts/flows")
        );
        assert_eq!(
            home.exts_tools(),
            PathBuf::from("/tmp/test-atta/exts/tools")
        );
        assert_eq!(home.exts_mcp(), PathBuf::from("/tmp/test-atta/exts/mcp"));
    }

    #[test]
    fn test_special_paths() {
        let home = AttaHome::resolve(Some("/tmp/test-atta"));
        assert_eq!(
            home.config_file(),
            PathBuf::from("/tmp/test-atta/etc/attaos.yaml")
        );
        assert_eq!(
            home.database(),
            PathBuf::from("/tmp/test-atta/data/data.db")
        );
        assert_eq!(
            home.estop_file(),
            PathBuf::from("/tmp/test-atta/data/estop.json")
        );
        assert_eq!(
            home.pid_file(),
            PathBuf::from("/tmp/test-atta/run/attaos.pid")
        );
    }
}
