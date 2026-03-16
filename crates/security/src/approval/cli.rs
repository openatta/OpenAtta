//! CLI approval backend — prompts via stdin

use atta_types::AttaError;
use tracing::info;

use super::manager::ApprovalBackend;
use super::types::{ToolApprovalRequest, ToolApprovalResponse};

/// Approval backend that prompts the user via stdin
pub struct CliApprovalBackend;

impl CliApprovalBackend {
    /// Create a new CLI approval backend
    pub fn new() -> Self {
        Self
    }
}

impl Default for CliApprovalBackend {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl ApprovalBackend for CliApprovalBackend {
    async fn request_approval(
        &self,
        req: &ToolApprovalRequest,
    ) -> Result<ToolApprovalResponse, AttaError> {
        // Print prompt to stderr so it doesn't mix with stdout
        eprintln!("\n╔══════════════════════════════════════════╗");
        eprintln!("║         TOOL APPROVAL REQUIRED           ║");
        eprintln!("╠══════════════════════════════════════════╣");
        eprintln!("║ Tool: {:<34} ║", req.tool_name);
        eprintln!("║ Risk: {:?}{:>30} ║", req.risk_level, "");
        eprintln!("╠══════════════════════════════════════════╣");
        eprintln!("║ Arguments:                               ║");

        let args_str = serde_json::to_string_pretty(&req.arguments)
            .unwrap_or_else(|_| req.arguments.to_string());
        for line in args_str.lines().take(10) {
            let truncated = if line.len() > 40 {
                format!("{}...", &line[..37])
            } else {
                line.to_string()
            };
            eprintln!("║  {:<40}║", truncated);
        }

        eprintln!("╠══════════════════════════════════════════╣");
        eprintln!("║ [y] Yes  [n] No  [a] Always (session)    ║");
        eprintln!("╚══════════════════════════════════════════╝");
        eprint!("Your choice: ");

        // Read from stdin in a blocking task
        let response = tokio::task::spawn_blocking(|| {
            let mut input = String::new();
            std::io::stdin().read_line(&mut input).ok();
            input.trim().to_lowercase()
        })
        .await
        .map_err(|e| AttaError::Other(anyhow::anyhow!("failed to read stdin: {e}")))?;

        let result = match response.as_str() {
            "y" | "yes" => ToolApprovalResponse::Yes,
            "a" | "always" => ToolApprovalResponse::Always,
            _ => ToolApprovalResponse::No,
        };

        info!(tool = %req.tool_name, response = ?result, "CLI approval response");
        Ok(result)
    }
}
