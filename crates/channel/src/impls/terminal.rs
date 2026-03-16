//! Terminal channel for development and testing
//!
//! Reads user input from stdin and prints agent responses to stdout.

use atta_types::AttaError;
use tracing::info;

use crate::traits::{Channel, ChannelMessage, ChatType, SendMessage};

/// Terminal channel — interactive stdin/stdout
pub struct TerminalChannel {
    name: String,
}

impl TerminalChannel {
    /// Create a new terminal channel
    pub fn new() -> Self {
        Self {
            name: "terminal".to_string(),
        }
    }
}

impl Default for TerminalChannel {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl Channel for TerminalChannel {
    fn name(&self) -> &str {
        &self.name
    }

    async fn send(&self, message: SendMessage) -> Result<(), AttaError> {
        println!("\n\u{1F916} {}", message.content);
        Ok(())
    }

    async fn listen(&self, tx: tokio::sync::mpsc::Sender<ChannelMessage>) -> Result<(), AttaError> {
        use tokio::io::AsyncBufReadExt;
        let stdin = tokio::io::stdin();
        let reader = tokio::io::BufReader::new(stdin);
        let mut lines = reader.lines();

        println!("AttaOS Chat \u{2014} type your message (Ctrl+D to quit):");

        loop {
            print!("\n\u{1F464} ");
            // Flush stdout to show prompt
            use std::io::Write;
            let _ = std::io::stdout().flush();

            match lines.next_line().await {
                Ok(Some(line)) => {
                    let line = line.trim().to_string();
                    if line.is_empty() {
                        continue;
                    }

                    let msg = ChannelMessage {
                        id: uuid::Uuid::new_v4().to_string(),
                        sender: "terminal-user".to_string(),
                        content: line,
                        channel: "terminal".to_string(),
                        reply_target: None,
                        timestamp: chrono::Utc::now(),
                        thread_ts: None,
                        metadata: serde_json::json!({}),
                        chat_type: ChatType::default(),
                        bot_mentioned: false,
                        group_id: None,
                    };

                    if tx.send(msg).await.is_err() {
                        break;
                    }
                }
                Ok(None) => {
                    info!("stdin closed (EOF)");
                    break;
                }
                Err(e) => {
                    tracing::error!(error = %e, "stdin read error");
                    break;
                }
            }
        }

        Ok(())
    }

    async fn health_check(&self) -> Result<(), AttaError> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_terminal_channel_name() {
        let ch = TerminalChannel::new();
        assert_eq!(ch.name(), "terminal");
    }

    #[tokio::test]
    async fn test_terminal_health_check() {
        let ch = TerminalChannel::new();
        assert!(ch.health_check().await.is_ok());
    }
}
