//! Shared utility functions.

use std::path::Path;

/// Categorized error formatting macro.
///
/// Usage: `shell_err!("NETWORK", "connection failed: {}", e)`
#[allow(unused_macros)]
macro_rules! shell_err {
    ($cat:expr, $($arg:tt)*) => {
        Err(format!(concat!("[", $cat, "] "), $($arg)*))
    };
}
#[allow(unused_imports)]
pub(crate) use shell_err;

/// Recursively copy a directory tree, preserving symlinks on Unix.
pub fn copy_dir_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());

        if file_type.is_symlink() {
            // Preserve symlinks instead of following them (avoids circular links)
            let target = std::fs::read_link(&src_path)?;
            #[cfg(unix)]
            std::os::unix::fs::symlink(&target, &dst_path)?;
            #[cfg(not(unix))]
            {
                // On Windows, symlink creation requires privileges; fall back to copy
                if src_path.is_dir() {
                    copy_dir_recursive(&src_path, &dst_path)?;
                } else {
                    std::fs::copy(&src_path, &dst_path)?;
                }
            }
        } else if file_type.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            std::fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}

// ── URL validation ──

/// Policy controlling which URLs are allowed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum UrlPolicy {
    /// Only allow localhost URLs (for installer/upgrade).
    AllowLocalOnly,
    /// Allow remote HTTPS URLs, block private IPs (for connections).
    AllowRemote,
}

/// Validate a URL against the given security policy.
pub fn validate_url(url: &str, policy: UrlPolicy) -> Result<(), String> {
    // Basic parse
    let parsed = url::Url::parse(url).map_err(|e| format!("[NETWORK] Invalid URL: {e}"))?;

    let scheme = parsed.scheme();
    let host_str = parsed
        .host_str()
        .ok_or_else(|| "[NETWORK] URL has no host".to_string())?;

    // Reject userinfo (user:pass@host) in all cases
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err("[SECURITY] URL must not contain userinfo (user:pass@host)".into());
    }

    match policy {
        UrlPolicy::AllowLocalOnly => {
            // Must be localhost
            let is_local = host_str == "localhost"
                || host_str == "127.0.0.1"
                || host_str == "::1"
                || host_str == "[::1]";
            if !is_local {
                return Err(format!(
                    "[SECURITY] Only localhost URLs are allowed, got: {host_str}"
                ));
            }
            // Allow http or https for localhost
            if scheme != "http" && scheme != "https" {
                return Err(format!("[NETWORK] URL must use HTTP(S), got: {scheme}"));
            }
        }
        UrlPolicy::AllowRemote => {
            let is_localhost = host_str == "localhost"
                || host_str == "127.0.0.1"
                || host_str == "::1"
                || host_str == "[::1]";

            // Allow http for localhost, require https for everything else
            if is_localhost {
                if scheme != "http" && scheme != "https" {
                    return Err(format!("[NETWORK] URL must use HTTP(S), got: {scheme}"));
                }
            } else {
                if scheme != "https" {
                    return Err(format!(
                        "[NETWORK] Remote URLs must use HTTPS: {url}"
                    ));
                }
                // Block private IPs and local domains
                if is_private_host(host_str) {
                    return Err(format!(
                        "[SECURITY] Private/reserved IP addresses are not allowed: {host_str}"
                    ));
                }
            }
        }
    }

    Ok(())
}

/// Check if a hostname or IP is a private/reserved address.
fn is_private_host(host: &str) -> bool {
    // .local domains
    if host.ends_with(".local") {
        return true;
    }

    // Try parsing as IPv4
    if let Ok(ip) = host.parse::<std::net::Ipv4Addr>() {
        return ip.is_loopback()           // 127.0.0.0/8
            || ip.is_private()            // 10/8, 172.16/12, 192.168/16
            || ip.is_link_local()         // 169.254/16
            || ip.is_unspecified();       // 0.0.0.0
    }

    // Try parsing as IPv6 (strip brackets)
    let v6_str = host.trim_start_matches('[').trim_end_matches(']');
    if let Ok(ip) = v6_str.parse::<std::net::Ipv6Addr>() {
        return ip.is_loopback()           // ::1
            || ip.is_unspecified()        // ::
            || is_ipv6_private(&ip);
    }

    false
}

/// Check if an IPv6 address is in a private/link-local range.
fn is_ipv6_private(ip: &std::net::Ipv6Addr) -> bool {
    let segments = ip.segments();
    // fe80::/10 (link-local)
    if segments[0] & 0xffc0 == 0xfe80 {
        return true;
    }
    // fd00::/8 (unique local)
    if segments[0] & 0xff00 == 0xfd00 {
        return true;
    }
    // fc00::/7 (unique local, broader)
    if segments[0] & 0xfe00 == 0xfc00 {
        return true;
    }
    false
}

// ── Audit logging ──

/// Append a JSON audit line to `$ATTA_HOME/log/config-audit.jsonl`.
pub fn audit_log(home: &Path, action: &str, file: &str, detail: &str) {
    let log_dir = home.join("log");
    let _ = std::fs::create_dir_all(&log_dir);
    let log_path = log_dir.join("config-audit.jsonl");

    let now = chrono::Utc::now().to_rfc3339();
    let entry = serde_json::json!({
        "timestamp": now,
        "action": action,
        "file": file,
        "user": "shell",
        "detail": detail,
    });

    if let Ok(line) = serde_json::to_string(&entry) {
        use std::io::Write;
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)
        {
            let _ = writeln!(f, "{line}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── UrlPolicy::AllowLocalOnly ──

    #[test]
    fn allow_local_accepts_localhost_http() {
        assert!(validate_url("http://localhost:3000", UrlPolicy::AllowLocalOnly).is_ok());
    }

    #[test]
    fn allow_local_accepts_127_0_0_1() {
        assert!(validate_url("http://127.0.0.1:8080", UrlPolicy::AllowLocalOnly).is_ok());
    }

    #[test]
    fn allow_local_accepts_ipv6_loopback() {
        assert!(validate_url("http://[::1]:3000", UrlPolicy::AllowLocalOnly).is_ok());
    }

    #[test]
    fn allow_local_rejects_remote_host() {
        let result = validate_url("http://example.com:3000", UrlPolicy::AllowLocalOnly);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("[SECURITY]"));
    }

    #[test]
    fn allow_local_rejects_ftp_scheme() {
        let result = validate_url("ftp://localhost/file", UrlPolicy::AllowLocalOnly);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("[NETWORK]"));
    }

    // ── UrlPolicy::AllowRemote ──

    #[test]
    fn allow_remote_accepts_https() {
        assert!(validate_url("https://api.example.com", UrlPolicy::AllowRemote).is_ok());
    }

    #[test]
    fn allow_remote_accepts_localhost_http() {
        assert!(validate_url("http://localhost:3000", UrlPolicy::AllowRemote).is_ok());
    }

    #[test]
    fn allow_remote_rejects_http_for_remote() {
        let result = validate_url("http://api.example.com", UrlPolicy::AllowRemote);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("HTTPS"));
    }

    #[test]
    fn allow_remote_rejects_private_10_ip() {
        let result = validate_url("https://10.0.0.1:3000", UrlPolicy::AllowRemote);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("[SECURITY]"));
    }

    #[test]
    fn allow_remote_rejects_private_192_ip() {
        let result = validate_url("https://192.168.1.1:3000", UrlPolicy::AllowRemote);
        assert!(result.is_err());
    }

    #[test]
    fn allow_remote_rejects_private_172_ip() {
        let result = validate_url("https://172.16.0.1:3000", UrlPolicy::AllowRemote);
        assert!(result.is_err());
    }

    #[test]
    fn allow_remote_rejects_link_local() {
        let result = validate_url("https://169.254.1.1:3000", UrlPolicy::AllowRemote);
        assert!(result.is_err());
    }

    #[test]
    fn allow_remote_rejects_dot_local_domain() {
        let result = validate_url("https://myserver.local:3000", UrlPolicy::AllowRemote);
        assert!(result.is_err());
    }

    // ── Userinfo rejection ──

    #[test]
    fn rejects_url_with_userinfo() {
        let result = validate_url("http://admin:pass@localhost:3000", UrlPolicy::AllowLocalOnly);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("userinfo"));
    }

    // ── Invalid URLs ──

    #[test]
    fn rejects_invalid_url() {
        let result = validate_url("not a url", UrlPolicy::AllowLocalOnly);
        assert!(result.is_err());
    }

    // ── is_private_host edge cases ──

    #[test]
    fn public_ip_is_not_private() {
        assert!(!is_private_host("8.8.8.8"));
    }

    #[test]
    fn ipv6_link_local_is_private() {
        assert!(is_private_host("[fe80::1]"));
    }

    #[test]
    fn ipv6_unique_local_is_private() {
        assert!(is_private_host("[fd12::1]"));
    }

    #[test]
    fn regular_hostname_is_not_private() {
        assert!(!is_private_host("example.com"));
    }

    // ── audit_log ──

    #[test]
    fn audit_log_creates_jsonl_file() {
        let tmp = tempfile::TempDir::new().unwrap();
        audit_log(tmp.path(), "test_action", "test.yaml", "some detail");

        let log_path = tmp.path().join("log/config-audit.jsonl");
        assert!(log_path.exists());

        let content = std::fs::read_to_string(&log_path).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(content.trim()).unwrap();
        assert_eq!(parsed["action"], "test_action");
        assert_eq!(parsed["file"], "test.yaml");
        assert_eq!(parsed["detail"], "some detail");
        assert_eq!(parsed["user"], "shell");
    }

    #[test]
    fn audit_log_appends_multiple_entries() {
        let tmp = tempfile::TempDir::new().unwrap();
        audit_log(tmp.path(), "first", "a.yaml", "d1");
        audit_log(tmp.path(), "second", "b.yaml", "d2");

        let log_path = tmp.path().join("log/config-audit.jsonl");
        let content = std::fs::read_to_string(&log_path).unwrap();
        let lines: Vec<&str> = content.trim().lines().collect();
        assert_eq!(lines.len(), 2);
    }

    // ── copy_dir_recursive ──

    #[test]
    fn copy_dir_recursive_copies_files() {
        let tmp = tempfile::TempDir::new().unwrap();
        let src = tmp.path().join("src");
        let dst = tmp.path().join("dst");
        std::fs::create_dir_all(src.join("sub")).unwrap();
        std::fs::write(src.join("a.txt"), "hello").unwrap();
        std::fs::write(src.join("sub/b.txt"), "world").unwrap();

        copy_dir_recursive(&src, &dst).unwrap();

        assert_eq!(std::fs::read_to_string(dst.join("a.txt")).unwrap(), "hello");
        assert_eq!(
            std::fs::read_to_string(dst.join("sub/b.txt")).unwrap(),
            "world"
        );
    }
}
