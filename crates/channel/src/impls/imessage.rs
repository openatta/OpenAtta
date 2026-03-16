//! iMessage channel (macOS only)
//!
//! Uses `osascript` (AppleScript) for sending messages and polls the
//! Messages.app SQLite database (`~/Library/Messages/chat.db`) for
//! incoming messages. This is a best-effort integration that requires
//! Full Disk Access permission on macOS.

#[cfg(target_os = "macos")]
use atta_types::AttaError;
#[cfg(target_os = "macos")]
use tracing::{debug, error, info, warn};

#[cfg(target_os = "macos")]
use crate::traits::{Channel, ChannelMessage, ChatType, SendMessage};

/// iMessage channel (macOS only)
#[cfg(target_os = "macos")]
pub struct ImessageChannel {
    name: String,
    /// Path to the Messages database
    db_path: String,
    /// Last rowid processed (for incremental polling)
    last_rowid: std::sync::atomic::AtomicI64,
    /// Poll interval
    poll_interval: std::time::Duration,
}

#[cfg(target_os = "macos")]
impl ImessageChannel {
    /// Create a new iMessage channel
    pub fn new() -> Self {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
        Self {
            name: "imessage".to_string(),
            db_path: format!("{}/Library/Messages/chat.db", home),
            last_rowid: std::sync::atomic::AtomicI64::new(0),
            poll_interval: std::time::Duration::from_secs(2),
        }
    }

    /// Create with a custom database path (for testing)
    pub fn with_db_path(mut self, path: String) -> Self {
        self.db_path = path;
        self
    }

    /// Set polling interval
    pub fn with_poll_interval(mut self, interval: std::time::Duration) -> Self {
        self.poll_interval = interval;
        self
    }

    /// Send a message via osascript (AppleScript)
    async fn send_via_applescript(recipient: &str, content: &str) -> Result<(), AttaError> {
        let script = format!(
            r#"tell application "Messages"
    set targetService to 1st account whose service type = iMessage
    set targetBuddy to participant "{}" of targetService
    send "{}" to targetBuddy
end tell"#,
            recipient.replace('"', "\\\""),
            content.replace('"', "\\\"").replace('\n', "\\n"),
        );

        let output = tokio::process::Command::new("osascript")
            .arg("-e")
            .arg(&script)
            .output()
            .await
            .map_err(|e| AttaError::Other(e.into()))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(AttaError::Other(anyhow::anyhow!(
                "osascript failed: {}",
                stderr
            )));
        }

        Ok(())
    }

    /// Query the Messages SQLite database for new messages
    async fn poll_messages_db(
        db_path: &str,
        last_rowid: i64,
    ) -> Result<Vec<(i64, String, String, i64)>, AttaError> {
        // Use sqlite3 command-line tool to avoid linking SQLite directly
        // Format: rowid|sender|text|date
        let query = format!(
            "SELECT m.ROWID, h.id, m.text, m.date \
             FROM message m \
             LEFT JOIN handle h ON m.handle_id = h.ROWID \
             WHERE m.ROWID > {} AND m.is_from_me = 0 AND m.text IS NOT NULL \
             ORDER BY m.ROWID ASC \
             LIMIT 100;",
            last_rowid
        );

        let output = tokio::process::Command::new("sqlite3")
            .arg("-separator")
            .arg("|")
            .arg(db_path)
            .arg(&query)
            .output()
            .await
            .map_err(|e| AttaError::Other(e.into()))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(AttaError::Other(anyhow::anyhow!(
                "sqlite3 query failed: {}",
                stderr
            )));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut results = Vec::new();

        for line in stdout.lines() {
            let parts: Vec<&str> = line.splitn(4, '|').collect();
            if parts.len() >= 4 {
                let rowid = parts[0].parse::<i64>().unwrap_or(0);
                let sender = parts[1].to_string();
                let text = parts[2].to_string();
                let date = parts[3].parse::<i64>().unwrap_or(0);
                results.push((rowid, sender, text, date));
            }
        }

        Ok(results)
    }
}

#[cfg(target_os = "macos")]
impl Default for ImessageChannel {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(target_os = "macos")]
#[async_trait::async_trait]
impl Channel for ImessageChannel {
    fn name(&self) -> &str {
        &self.name
    }

    async fn send(&self, message: SendMessage) -> Result<(), AttaError> {
        debug!(
            to = %message.recipient,
            "Sending iMessage via AppleScript"
        );

        Self::send_via_applescript(&message.recipient, &message.content).await?;
        debug!("iMessage sent to {}", message.recipient);
        Ok(())
    }

    async fn listen(&self, tx: tokio::sync::mpsc::Sender<ChannelMessage>) -> Result<(), AttaError> {
        info!(
            db_path = %self.db_path,
            "iMessage listener starting (polling Messages.app database)"
        );

        // Initialize last_rowid to current max to only get new messages
        match Self::poll_messages_db(&self.db_path, i64::MAX - 1).await {
            Ok(_) => {
                // Get the current max rowid
                let output = tokio::process::Command::new("sqlite3")
                    .arg(&self.db_path)
                    .arg("SELECT MAX(ROWID) FROM message;")
                    .output()
                    .await
                    .map_err(|e| AttaError::Other(e.into()))?;

                let max_rowid = String::from_utf8_lossy(&output.stdout)
                    .trim()
                    .parse::<i64>()
                    .unwrap_or(0);

                self.last_rowid
                    .store(max_rowid, std::sync::atomic::Ordering::Relaxed);
                debug!(max_rowid, "iMessage initial rowid set");
            }
            Err(e) => {
                warn!(error = %e, "Failed to initialize iMessage rowid, starting from 0");
            }
        }

        loop {
            let current_rowid = self.last_rowid.load(std::sync::atomic::Ordering::Relaxed);

            match Self::poll_messages_db(&self.db_path, current_rowid).await {
                Ok(messages) => {
                    for (rowid, sender, text, date) in messages {
                        // macOS Messages dates are in "Apple epoch" (seconds since 2001-01-01)
                        // plus nanoseconds. Convert to Unix timestamp.
                        let apple_epoch_offset: i64 = 978_307_200; // seconds from Unix to Apple epoch
                        let unix_ts = (date / 1_000_000_000) + apple_epoch_offset;

                        let timestamp = chrono::DateTime::from_timestamp(unix_ts, 0)
                            .unwrap_or_else(chrono::Utc::now);

                        let channel_msg = ChannelMessage {
                            id: format!("imsg-{}", rowid),
                            sender: sender.clone(),
                            content: text,
                            channel: "imessage".to_string(),
                            reply_target: None,
                            timestamp,
                            thread_ts: None,
                            metadata: serde_json::json!({
                                "rowid": rowid,
                                "apple_date": date,
                            }),
                            chat_type: ChatType::default(),
                            bot_mentioned: false,
                            group_id: None,
                        };

                        if tx.send(channel_msg).await.is_err() {
                            debug!("iMessage listener: receiver dropped, stopping");
                            return Ok(());
                        }

                        self.last_rowid
                            .fetch_max(rowid, std::sync::atomic::Ordering::Relaxed);
                    }
                }
                Err(e) => {
                    error!(error = %e, "iMessage poll failed");
                }
            }

            tokio::time::sleep(self.poll_interval).await;
        }
    }

    async fn health_check(&self) -> Result<(), AttaError> {
        // Check if the database file exists
        let exists = tokio::fs::metadata(&self.db_path).await.is_ok();
        if !exists {
            return Err(AttaError::Other(anyhow::anyhow!(
                "iMessage database not found at {}. \
                 Ensure Messages.app is configured and Full Disk Access is granted.",
                self.db_path
            )));
        }
        Ok(())
    }
}

// Stub for non-macOS platforms
#[cfg(not(target_os = "macos"))]
pub struct ImessageChannel {
    _private: (),
}

#[cfg(not(target_os = "macos"))]
impl ImessageChannel {
    /// iMessage is only available on macOS. This is a stub.
    pub fn new() -> Self {
        Self { _private: () }
    }
}

#[cfg(not(target_os = "macos"))]
impl Default for ImessageChannel {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(not(target_os = "macos"))]
#[async_trait::async_trait]
impl crate::traits::Channel for ImessageChannel {
    fn name(&self) -> &str {
        "imessage"
    }

    async fn send(
        &self,
        _message: crate::traits::SendMessage,
    ) -> Result<(), atta_types::AttaError> {
        Err(atta_types::AttaError::Other(anyhow::anyhow!(
            "iMessage channel is only available on macOS"
        )))
    }

    async fn listen(
        &self,
        _tx: tokio::sync::mpsc::Sender<crate::traits::ChannelMessage>,
    ) -> Result<(), atta_types::AttaError> {
        Err(atta_types::AttaError::Other(anyhow::anyhow!(
            "iMessage channel is only available on macOS"
        )))
    }

    async fn health_check(&self) -> Result<(), atta_types::AttaError> {
        Err(atta_types::AttaError::Other(anyhow::anyhow!(
            "iMessage channel is only available on macOS"
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_imessage_channel_name() {
        let ch = ImessageChannel::new();
        #[cfg(target_os = "macos")]
        assert_eq!(ch.name(), "imessage");
        #[cfg(not(target_os = "macos"))]
        {
            use crate::traits::Channel;
            assert_eq!(ch.name(), "imessage");
        }
    }
}
