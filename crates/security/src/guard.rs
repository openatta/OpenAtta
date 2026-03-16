//! SecurityGuard — wraps ToolRegistry with security checks

use std::sync::Arc;

use atta_types::{AttaError, RiskLevel, ToolDef, ToolRegistry, ToolSchema};
use tracing::{info, warn};

use crate::approval::manager::ApprovalManager;
use crate::approval::types::ToolApprovalRequest;
use crate::classifier::CommandClassifier;
use crate::estop::EstopManager;
use crate::policy::{AutonomyLevel, SecurityPolicy};
use crate::tracker::ActionTracker;

/// Approval lifecycle events emitted by SecurityGuard
#[derive(Debug, Clone)]
pub enum ApprovalEvent {
    /// A tool call is waiting for approval
    Pending { tool_name: String },
    /// A tool call was approved
    Granted { tool_name: String },
    /// A tool call was denied
    Denied { tool_name: String },
}

/// Security-aware wrapper around a ToolRegistry.
///
/// Intercepts tool invocations to apply:
/// 1. Risk classification
/// 2. Autonomy level enforcement
/// 3. Interactive approval for high-risk calls in Supervised mode
/// 4. Rate limiting
/// 5. Path safety checks
/// 6. Shell command validation
pub struct SecurityGuard {
    inner: Arc<dyn ToolRegistry>,
    policy: SecurityPolicy,
    tracker: ActionTracker,
    approval_manager: Option<Arc<ApprovalManager>>,
    estop: Option<Arc<EstopManager>>,
    leak_detector: crate::scrub::detector::LeakDetector,
    approval_event_tx: Option<tokio::sync::mpsc::Sender<ApprovalEvent>>,
}

impl SecurityGuard {
    /// Create a new SecurityGuard wrapping the given registry
    pub fn new(inner: Arc<dyn ToolRegistry>, policy: SecurityPolicy) -> Self {
        Self {
            inner,
            policy,
            tracker: ActionTracker::new(),
            approval_manager: None,
            estop: None,
            leak_detector: crate::scrub::detector::LeakDetector::new(),
            approval_event_tx: None,
        }
    }

    /// Attach an approval manager for interactive approval of high-risk calls
    pub fn with_approval_manager(mut self, manager: Arc<ApprovalManager>) -> Self {
        self.approval_manager = Some(manager);
        self
    }

    /// Attach an E-Stop manager for emergency stop checks
    pub fn with_estop(mut self, estop: Arc<EstopManager>) -> Self {
        self.estop = Some(estop);
        self
    }

    /// Attach an approval event channel for UI notifications
    pub fn with_approval_events(mut self, tx: tokio::sync::mpsc::Sender<ApprovalEvent>) -> Self {
        self.approval_event_tx = Some(tx);
        self
    }

    /// Get a reference to the security policy
    pub fn policy(&self) -> &SecurityPolicy {
        &self.policy
    }

    /// Check if the call is allowed under the current policy.
    /// Returns (possibly modified) arguments — path arguments are replaced with
    /// their canonical form to prevent TOCTOU attacks.
    async fn check_permission(
        &self,
        tool_name: &str,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, AttaError> {
        // E-Stop check — highest priority, before any other checks
        if let Some(ref estop) = self.estop {
            estop.check(tool_name, args)?;
        }

        let risk = CommandClassifier::classify(tool_name, args);

        // Autonomy level check
        match (&self.policy.autonomy_level, &risk) {
            (AutonomyLevel::ReadOnly, RiskLevel::Medium | RiskLevel::High) => {
                return Err(AttaError::PermissionDenied {
                    permission: format!(
                        "tool '{}' requires {:?} risk level, but autonomy is ReadOnly",
                        tool_name, risk
                    ),
                });
            }
            (AutonomyLevel::Supervised, RiskLevel::High) => {
                // Route to approval manager if available
                if let Some(ref manager) = self.approval_manager {
                    let approval_req = ToolApprovalRequest {
                        tool_name: tool_name.to_string(),
                        arguments: args.clone(),
                        risk_level: risk.clone(),
                        description: format!("High-risk tool '{}' in Supervised mode", tool_name),
                    };
                    // Emit Pending event before approval
                    if let Some(ref tx) = self.approval_event_tx {
                        if let Err(e) = tx.try_send(ApprovalEvent::Pending {
                            tool_name: tool_name.to_string(),
                        }) {
                            warn!(tool = tool_name, error = %e, "failed to send approval pending event");
                        }
                    }
                    match manager.check_and_approve(&approval_req).await {
                        Ok(()) => {
                            // Emit Granted event
                            if let Some(ref tx) = self.approval_event_tx {
                                if let Err(e) = tx.try_send(ApprovalEvent::Granted {
                                    tool_name: tool_name.to_string(),
                                }) {
                                    warn!(tool = tool_name, error = %e, "failed to send approval granted event");
                                }
                            }
                        }
                        Err(e) => {
                            // Emit Denied event
                            if let Some(ref tx) = self.approval_event_tx {
                                if let Err(e) = tx.try_send(ApprovalEvent::Denied {
                                    tool_name: tool_name.to_string(),
                                }) {
                                    warn!(tool = tool_name, error = %e, "failed to send approval denied event");
                                }
                            }
                            return Err(e);
                        }
                    }
                    // Approval granted — fall through to remaining checks
                } else {
                    warn!(
                        tool = tool_name,
                        risk = ?risk,
                        "high-risk tool call in Supervised mode — requires approval"
                    );
                    return Err(AttaError::PermissionDenied {
                        permission: format!(
                            "tool '{}' is high-risk and requires approval in Supervised mode",
                            tool_name
                        ),
                    });
                }
            }
            _ => {}
        }

        // Rate limiting — total calls
        let total = self.tracker.record_call();
        if total > self.policy.max_calls_per_minute {
            return Err(AttaError::RateLimited(format!(
                "exceeded {} calls/minute (current: {})",
                self.policy.max_calls_per_minute, total
            )));
        }

        // Rate limiting — high-risk calls
        if risk == RiskLevel::High {
            let hr_count = self.tracker.record_high_risk_call();
            if hr_count > self.policy.max_high_risk_per_minute {
                return Err(AttaError::RateLimited(format!(
                    "exceeded {} high-risk calls/minute (current: {})",
                    self.policy.max_high_risk_per_minute, hr_count
                )));
            }
        }

        // Shell command validation
        if tool_name == "shell" || tool_name == "bash" || tool_name == "exec" {
            if let Some(cmd) = args.get("command").and_then(|v| v.as_str()) {
                CommandClassifier::validate_shell_command(cmd, &self.policy)?;
            }
        }

        // Network access check
        if !self.policy.allow_network
            && matches!(
                tool_name,
                "web_fetch" | "web_search" | "http_request" | "browser"
            )
        {
            return Err(AttaError::SecurityViolation(format!(
                "network access denied for tool '{}'",
                tool_name
            )));
        }

        // Path check for file operations — canonicalize to prevent TOCTOU
        let mut effective_args = args.clone();
        if matches!(
            tool_name,
            "file_read" | "file_write" | "file_edit" | "apply_patch"
        ) {
            if let Some(path) = args.get("path").and_then(|v| v.as_str()) {
                let canonical = self.validate_path(path)?;
                // Replace path with canonical form to eliminate TOCTOU window
                if let Some(obj) = effective_args.as_object_mut() {
                    obj.insert(
                        "path".to_string(),
                        serde_json::Value::String(canonical.to_string_lossy().to_string()),
                    );
                }
            }
        }

        // URL domain check for network tools
        if matches!(
            tool_name,
            "web_fetch" | "web_search" | "http_request" | "browser"
        ) {
            if let Some(url) = args.get("url").and_then(|v| v.as_str()) {
                self.validate_url_domain(url)?;
            }
        }

        info!(
            tool = tool_name,
            risk = ?risk,
            "security check passed"
        );
        Ok(effective_args)
    }

    /// Validate a file path with canonicalization and sandbox enforcement.
    /// Returns the canonical path for TOCTOU-safe usage.
    fn validate_path(&self, path: &str) -> Result<std::path::PathBuf, AttaError> {
        // Null byte injection check
        if path.contains('\0') {
            return Err(AttaError::SecurityViolation(
                "null byte in path".to_string(),
            ));
        }

        // Canonicalize to resolve symlinks and ..
        let canonical = match std::fs::canonicalize(path) {
            Ok(p) => p,
            Err(_) => {
                // File may not exist yet (for writes), use parent directory
                let parent = std::path::Path::new(path).parent();
                match parent.and_then(|p| std::fs::canonicalize(p).ok()) {
                    Some(p) => p.join(std::path::Path::new(path).file_name().unwrap_or_default()),
                    None => std::path::PathBuf::from(path),
                }
            }
        };
        let canonical_str = canonical.to_string_lossy();

        // Check forbidden paths against canonical path
        for forbidden in &self.policy.forbidden_paths {
            if canonical_str.contains(forbidden) {
                return Err(AttaError::SecurityViolation(format!(
                    "access to forbidden path: {}",
                    forbidden
                )));
            }
        }

        // Workspace root constraint
        if let Some(ref root) = self.policy.workspace_root {
            let root_canonical =
                std::fs::canonicalize(root).unwrap_or_else(|_| std::path::PathBuf::from(root));
            let root_str = root_canonical.to_string_lossy();

            if !canonical_str.starts_with(root_str.as_ref()) {
                // Check allowed_roots
                let in_allowed = self.policy.allowed_roots.iter().any(|allowed| {
                    let allowed_canonical = std::fs::canonicalize(allowed)
                        .unwrap_or_else(|_| std::path::PathBuf::from(allowed));
                    canonical_str.starts_with(&*allowed_canonical.to_string_lossy())
                });

                if !in_allowed {
                    return Err(AttaError::SandboxViolation {
                        path: path.to_string(),
                    });
                }
            }
        }

        Ok(canonical)
    }

    /// Validate a URL domain against allowlist/blocklist and SSRF protection
    fn validate_url_domain(&self, url: &str) -> Result<(), AttaError> {
        let domain = Self::extract_domain(url);

        // Blocklist takes priority
        for blocked in &self.policy.url_blocklist {
            if domain == *blocked || domain.ends_with(&format!(".{}", blocked)) {
                return Err(AttaError::SecurityViolation(format!(
                    "domain '{}' is blocked",
                    domain
                )));
            }
        }

        // If allowlist is non-empty, only allow listed domains
        if !self.policy.url_allowlist.is_empty() {
            let allowed = self
                .policy
                .url_allowlist
                .iter()
                .any(|a| domain == *a || domain.ends_with(&format!(".{}", a)));
            if !allowed {
                return Err(AttaError::SecurityViolation(format!(
                    "domain '{}' is not in allowlist",
                    domain
                )));
            }
        }

        // SSRF protection: check if the host resolves to a private/reserved IP
        if Self::is_ip_literal(&domain) {
            if let Ok(ip) = domain
                .trim_start_matches('[')
                .trim_end_matches(']')
                .parse::<std::net::IpAddr>()
            {
                if Self::is_private_ip(&ip) {
                    return Err(AttaError::SecurityViolation(format!(
                        "SSRF blocked: IP '{}' is in a private/reserved range",
                        ip
                    )));
                }
            }
        }

        Ok(())
    }

    /// Check if a host string is an IP address literal (v4 or bracketed v6)
    fn is_ip_literal(host: &str) -> bool {
        host.parse::<std::net::Ipv4Addr>().is_ok()
            || host.starts_with('[')
            || host.parse::<std::net::Ipv6Addr>().is_ok()
    }

    /// Check if an IP address is in a private or reserved range (SSRF protection)
    fn is_private_ip(ip: &std::net::IpAddr) -> bool {
        match ip {
            std::net::IpAddr::V4(v4) => {
                v4.is_loopback()                          // 127.0.0.0/8
                    || v4.is_private()                    // 10/8, 172.16/12, 192.168/16
                    || v4.is_link_local()                 // 169.254.0.0/16
                    || v4.is_broadcast()                  // 255.255.255.255
                    || v4.is_unspecified()                 // 0.0.0.0
                    || v4.octets()[0] == 100 && (v4.octets()[1] & 0xC0) == 64  // 100.64.0.0/10 (CGN)
                    || v4.octets()[0] == 192 && v4.octets()[1] == 0 && v4.octets()[2] == 0  // 192.0.0.0/24
                    || v4.octets()[0] == 198 && (v4.octets()[1] & 0xFE) == 18 // 198.18.0.0/15 (benchmark)
            }
            std::net::IpAddr::V6(v6) => {
                v6.is_loopback()                          // ::1
                    || v6.is_unspecified()                 // ::
                    || (v6.segments()[0] & 0xFE00) == 0xFC00  // fc00::/7 (ULA)
                    || (v6.segments()[0] & 0xFFC0) == 0xFE80 // fe80::/10 (link-local)
            }
        }
    }

    /// Asynchronously resolve a hostname and validate all resolved IPs are not private.
    /// Returns the first non-private IP for DNS pinning.
    pub async fn resolve_and_validate_ip(host: &str) -> Result<std::net::IpAddr, AttaError> {
        use tokio::net::lookup_host;

        let addr_str = format!("{}:0", host);
        let addrs: Vec<std::net::SocketAddr> = lookup_host(&addr_str)
            .await
            .map_err(|e| {
                AttaError::SecurityViolation(format!("DNS resolution failed for '{}': {}", host, e))
            })?
            .collect();

        if addrs.is_empty() {
            return Err(AttaError::SecurityViolation(format!(
                "DNS resolution returned no addresses for '{}'",
                host
            )));
        }

        // Check ALL resolved IPs — if any is private, block the request
        for addr in &addrs {
            if Self::is_private_ip(&addr.ip()) {
                return Err(AttaError::SecurityViolation(format!(
                    "SSRF blocked: '{}' resolves to private IP {}",
                    host,
                    addr.ip()
                )));
            }
        }

        // Return first IP for DNS pinning
        Ok(addrs[0].ip())
    }

    /// Extract the domain from a URL string
    fn extract_domain(url: &str) -> String {
        // Strip scheme
        let without_scheme = url
            .strip_prefix("https://")
            .or_else(|| url.strip_prefix("http://"))
            .unwrap_or(url);
        // Take everything up to first / or :
        without_scheme
            .split('/')
            .next()
            .unwrap_or(without_scheme)
            .split(':')
            .next()
            .unwrap_or(without_scheme)
            .to_lowercase()
    }
}

#[async_trait::async_trait]
impl ToolRegistry for SecurityGuard {
    fn register(&self, tool: ToolDef) {
        self.inner.register(tool);
    }

    fn unregister(&self, name: &str) {
        self.inner.unregister(name);
    }

    fn get(&self, name: &str) -> Option<ToolDef> {
        self.inner.get(name)
    }

    fn get_schema(&self, name: &str) -> Option<ToolSchema> {
        self.inner.get_schema(name)
    }

    fn list_schemas(&self) -> Vec<ToolSchema> {
        let all = self.inner.list_schemas();
        crate::policy_pipeline::filter_tools_by_profile(&all, &self.policy.tool_profile)
    }

    fn list_all(&self) -> Vec<ToolDef> {
        self.inner.list_all()
    }

    async fn invoke(
        &self,
        tool_name: &str,
        arguments: &serde_json::Value,
    ) -> Result<serde_json::Value, AttaError> {
        // check_permission returns canonicalized args (TOCTOU-safe)
        let safe_args = self.check_permission(tool_name, arguments).await?;
        let result = self.inner.invoke(tool_name, &safe_args).await?;
        // Layer 1: prefix-based secret scrubbing
        let scrubbed = crate::scrub::scrub_json_value(&result);
        // Layer 2: regex-based leak detection
        Ok(self.scrub_with_detector(&scrubbed))
    }
}

impl SecurityGuard {
    /// Recursively scrub all string values through the LeakDetector
    fn scrub_with_detector(&self, value: &serde_json::Value) -> serde_json::Value {
        match value {
            serde_json::Value::String(s) => serde_json::Value::String(self.leak_detector.scrub(s)),
            serde_json::Value::Array(arr) => {
                serde_json::Value::Array(arr.iter().map(|v| self.scrub_with_detector(v)).collect())
            }
            serde_json::Value::Object(obj) => {
                let map = obj
                    .iter()
                    .map(|(k, v)| (k.clone(), self.scrub_with_detector(v)))
                    .collect();
                serde_json::Value::Object(map)
            }
            other => other.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::policy::AutonomyLevel;
    use atta_types::ToolDef;

    /// Stub registry for testing
    struct StubRegistry;

    #[async_trait::async_trait]
    impl ToolRegistry for StubRegistry {
        fn register(&self, _tool: ToolDef) {}
        fn unregister(&self, _name: &str) {}
        fn get(&self, _name: &str) -> Option<ToolDef> {
            None
        }
        fn get_schema(&self, _name: &str) -> Option<ToolSchema> {
            None
        }
        fn list_schemas(&self) -> Vec<ToolSchema> {
            vec![]
        }
        fn list_all(&self) -> Vec<ToolDef> {
            vec![]
        }
        async fn invoke(
            &self,
            _tool_name: &str,
            _arguments: &serde_json::Value,
        ) -> Result<serde_json::Value, AttaError> {
            Ok(serde_json::json!({"ok": true}))
        }
    }

    #[tokio::test]
    async fn test_readonly_blocks_medium_risk() {
        let inner: Arc<dyn ToolRegistry> = Arc::new(StubRegistry);
        let policy = SecurityPolicy {
            autonomy_level: AutonomyLevel::ReadOnly,
            ..Default::default()
        };

        let guard = SecurityGuard::new(inner, policy);
        let result = guard
            .invoke(
                "web_fetch",
                &serde_json::json!({"url": "http://example.com"}),
            )
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_readonly_allows_read() {
        let inner: Arc<dyn ToolRegistry> = Arc::new(StubRegistry);
        let policy = SecurityPolicy {
            autonomy_level: AutonomyLevel::ReadOnly,
            ..Default::default()
        };

        let guard = SecurityGuard::new(inner, policy);
        let result = guard
            .invoke("file_read", &serde_json::json!({"path": "/tmp/test.txt"}))
            .await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_supervised_blocks_high_risk() {
        let inner: Arc<dyn ToolRegistry> = Arc::new(StubRegistry);
        let policy = SecurityPolicy::default(); // Supervised

        let guard = SecurityGuard::new(inner, policy);
        let result = guard
            .invoke("shell", &serde_json::json!({"command": "ls"}))
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_full_allows_everything() {
        let inner: Arc<dyn ToolRegistry> = Arc::new(StubRegistry);
        let policy = SecurityPolicy {
            autonomy_level: AutonomyLevel::Full,
            ..Default::default()
        };

        let guard = SecurityGuard::new(inner, policy);
        let result = guard
            .invoke("shell", &serde_json::json!({"command": "ls"}))
            .await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_dangerous_shell_command_blocked() {
        let inner: Arc<dyn ToolRegistry> = Arc::new(StubRegistry);
        let policy = SecurityPolicy {
            autonomy_level: AutonomyLevel::Full,
            ..Default::default()
        };

        let guard = SecurityGuard::new(inner, policy);
        let result = guard
            .invoke("shell", &serde_json::json!({"command": "rm -rf /"}))
            .await;
        assert!(result.is_err());
    }

    #[test]
    fn test_is_private_ip_v4() {
        use std::net::IpAddr;
        assert!(SecurityGuard::is_private_ip(
            &"127.0.0.1".parse::<IpAddr>().unwrap()
        ));
        assert!(SecurityGuard::is_private_ip(
            &"10.0.0.1".parse::<IpAddr>().unwrap()
        ));
        assert!(SecurityGuard::is_private_ip(
            &"172.16.0.1".parse::<IpAddr>().unwrap()
        ));
        assert!(SecurityGuard::is_private_ip(
            &"192.168.1.1".parse::<IpAddr>().unwrap()
        ));
        assert!(SecurityGuard::is_private_ip(
            &"169.254.1.1".parse::<IpAddr>().unwrap()
        ));
        assert!(!SecurityGuard::is_private_ip(
            &"8.8.8.8".parse::<IpAddr>().unwrap()
        ));
        assert!(!SecurityGuard::is_private_ip(
            &"1.1.1.1".parse::<IpAddr>().unwrap()
        ));
    }

    #[test]
    fn test_is_private_ip_v6() {
        use std::net::IpAddr;
        assert!(SecurityGuard::is_private_ip(
            &"::1".parse::<IpAddr>().unwrap()
        ));
        assert!(SecurityGuard::is_private_ip(
            &"fc00::1".parse::<IpAddr>().unwrap()
        ));
        assert!(SecurityGuard::is_private_ip(
            &"fe80::1".parse::<IpAddr>().unwrap()
        ));
        assert!(!SecurityGuard::is_private_ip(
            &"2001:4860:4860::8888".parse::<IpAddr>().unwrap()
        ));
    }

    #[test]
    fn test_ssrf_ip_literal_blocked() {
        let inner: Arc<dyn ToolRegistry> = Arc::new(StubRegistry);
        let policy = SecurityPolicy {
            autonomy_level: AutonomyLevel::Full,
            ..Default::default()
        };
        let guard = SecurityGuard::new(inner, policy);
        // IP literal pointing to localhost
        assert!(guard.validate_url_domain("http://127.0.0.1/admin").is_err());
        // Private range
        assert!(guard.validate_url_domain("http://10.0.0.1/secret").is_err());
        assert!(guard.validate_url_domain("http://192.168.1.1/api").is_err());
        // Public IP should pass
        assert!(guard.validate_url_domain("http://8.8.8.8/dns").is_ok());
        // Normal domain should pass
        assert!(guard.validate_url_domain("http://example.com/page").is_ok());
    }

    #[tokio::test]
    async fn test_network_denied() {
        let inner: Arc<dyn ToolRegistry> = Arc::new(StubRegistry);
        let policy = SecurityPolicy {
            autonomy_level: AutonomyLevel::Full,
            allow_network: false,
            ..Default::default()
        };

        let guard = SecurityGuard::new(inner, policy);
        let result = guard
            .invoke(
                "web_fetch",
                &serde_json::json!({"url": "http://example.com"}),
            )
            .await;
        assert!(result.is_err());
    }
}
