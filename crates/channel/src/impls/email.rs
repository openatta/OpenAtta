//! Email channel
//!
//! Uses IMAP IDLE for listening to incoming emails and SMTP (via lettre)
//! for sending outgoing messages. Supports the `subject` field in SendMessage.

use atta_types::AttaError;
use tracing::{debug, info, warn};

use crate::traits::{Channel, ChannelMessage, ChatType, SendMessage};

/// Email channel configuration
pub struct EmailChannel {
    name: String,
    /// IMAP server host
    imap_host: String,
    /// IMAP server port (993 for IMAPS)
    imap_port: u16,
    /// SMTP server host
    smtp_host: String,
    /// SMTP server port (587 for STARTTLS, 465 for SMTPS)
    smtp_port: u16,
    /// Email address (used as sender and IMAP login)
    email_address: String,
    /// Password or app-specific password
    password: String,
    /// IMAP mailbox to monitor (default: "INBOX")
    mailbox: String,
    /// Whether to use implicit TLS for SMTP (port 465 style)
    smtp_tls: bool,
}

impl EmailChannel {
    /// Create a new email channel
    pub fn new(
        imap_host: String,
        imap_port: u16,
        smtp_host: String,
        smtp_port: u16,
        email_address: String,
        password: String,
    ) -> Self {
        Self {
            name: "email".to_string(),
            imap_host,
            imap_port,
            smtp_host,
            smtp_port,
            email_address,
            password,
            mailbox: "INBOX".to_string(),
            smtp_tls: true,
        }
    }

    /// Set the IMAP mailbox to monitor
    pub fn with_mailbox(mut self, mailbox: String) -> Self {
        self.mailbox = mailbox;
        self
    }

    /// Set whether to use implicit TLS for SMTP
    pub fn with_smtp_tls(mut self, tls: bool) -> Self {
        self.smtp_tls = tls;
        self
    }

    /// Parse a raw email into a ChannelMessage
    fn parse_email(&self, uid: u32, raw: &[u8]) -> Option<ChannelMessage> {
        let raw_str = String::from_utf8_lossy(raw);

        // Simple header parser — production code should use the `mail-parser` crate
        let mut from = String::new();
        let mut subject = String::new();
        let mut date_str = String::new();
        let mut message_id = String::new();
        let mut in_reply_to = None;
        let mut body = String::new();
        let mut in_headers = true;

        for line in raw_str.lines() {
            if in_headers {
                if line.is_empty() {
                    in_headers = false;
                    continue;
                }
                let lower = line.to_lowercase();
                if lower.starts_with("from:") {
                    from = line[5..].trim().to_string();
                } else if lower.starts_with("subject:") {
                    subject = line[8..].trim().to_string();
                } else if lower.starts_with("date:") {
                    date_str = line[5..].trim().to_string();
                } else if lower.starts_with("message-id:") {
                    message_id = line[11..].trim().to_string();
                } else if lower.starts_with("in-reply-to:") {
                    in_reply_to = Some(line[12..].trim().to_string());
                }
            } else {
                body.push_str(line);
                body.push('\n');
            }
        }

        if from.is_empty() {
            return None;
        }

        // Extract email address from "Name <addr>" format
        let sender = if let Some(start) = from.find('<') {
            if let Some(end) = from.find('>') {
                from[start + 1..end].to_string()
            } else {
                from.clone()
            }
        } else {
            from.clone()
        };

        let timestamp = chrono::Utc::now(); // Simplified; production would parse date_str
        let _ = date_str; // suppress unused warning

        if message_id.is_empty() {
            message_id = format!("email-uid-{}", uid);
        }

        Some(ChannelMessage {
            id: message_id,
            sender,
            content: body.trim().to_string(),
            channel: "email".to_string(),
            reply_target: in_reply_to,
            timestamp,
            thread_ts: None,
            metadata: serde_json::json!({
                "subject": subject,
                "from_raw": from,
                "uid": uid,
            }),
            chat_type: ChatType::default(),
            bot_mentioned: false,
            group_id: None,
        })
    }
}

#[async_trait::async_trait]
impl Channel for EmailChannel {
    fn name(&self) -> &str {
        &self.name
    }

    async fn send(&self, message: SendMessage) -> Result<(), AttaError> {
        #[cfg(feature = "email")]
        {
            use lettre::message::header::ContentType;
            use lettre::transport::smtp::authentication::Credentials;
            use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};

            let subject = message.subject.as_deref().unwrap_or("(no subject)");

            let email = Message::builder()
                .from(
                    self.email_address
                        .parse()
                        .map_err(|e| AttaError::Channel(format!("invalid from address: {e}")))?,
                )
                .to(message
                    .recipient
                    .parse()
                    .map_err(|e| AttaError::Channel(format!("invalid to address: {e}")))?)
                .subject(subject)
                .header(ContentType::TEXT_PLAIN)
                .body(message.content.clone())
                .map_err(|e| AttaError::Channel(format!("failed to build email: {e}")))?;

            let creds = Credentials::new(self.email_address.clone(), self.password.clone());

            let mailer = if self.smtp_tls {
                // Implicit TLS (port 465)
                AsyncSmtpTransport::<Tokio1Executor>::relay(&self.smtp_host)
                    .map_err(|e| AttaError::Channel(format!("SMTP relay error: {e}")))?
                    .port(self.smtp_port)
                    .credentials(creds)
                    .build()
            } else {
                // STARTTLS (port 587)
                AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&self.smtp_host)
                    .map_err(|e| AttaError::Channel(format!("SMTP starttls error: {e}")))?
                    .port(self.smtp_port)
                    .credentials(creds)
                    .build()
            };

            mailer
                .send(email)
                .await
                .map_err(|e| AttaError::Channel(format!("SMTP send failed: {e}")))?;

            debug!(to = %message.recipient, subject, "email sent");
            Ok(())
        }

        #[cfg(not(feature = "email"))]
        {
            let _ = message;
            Err(AttaError::Channel(
                "Email channel requires the 'email' feature".to_string(),
            ))
        }
    }

    async fn listen(&self, tx: tokio::sync::mpsc::Sender<ChannelMessage>) -> Result<(), AttaError> {
        #[cfg(feature = "email")]
        {
            use futures::StreamExt;
            use tokio::net::TcpStream;

            let poll_interval = tokio::time::Duration::from_secs(30);
            let mut highest_uid_seen: u32 = 0;

            info!(
                host = %self.imap_host,
                port = self.imap_port,
                mailbox = %self.mailbox,
                poll_interval_secs = poll_interval.as_secs(),
                "Email IMAP polling listener starting"
            );

            loop {
                // Connect to IMAP server via TCP
                let tcp_stream =
                    match TcpStream::connect((self.imap_host.as_str(), self.imap_port)).await {
                        Ok(s) => s,
                        Err(e) => {
                            warn!(error = %e, "Email IMAP TCP connect failed, retrying in 30s");
                            tokio::time::sleep(poll_interval).await;
                            continue;
                        }
                    };

                // Wrap with TLS (port 993 = IMAPS)
                let tls_connector =
                    match tokio_native_tls::native_tls::TlsConnector::builder().build() {
                        Ok(c) => tokio_native_tls::TlsConnector::from(c),
                        Err(e) => {
                            warn!(error = %e, "Email TLS connector build failed, retrying in 30s");
                            tokio::time::sleep(poll_interval).await;
                            continue;
                        }
                    };

                let tls_stream = match tls_connector.connect(&self.imap_host, tcp_stream).await {
                    Ok(s) => s,
                    Err(e) => {
                        warn!(error = %e, "Email TLS handshake failed, retrying in 30s");
                        tokio::time::sleep(poll_interval).await;
                        continue;
                    }
                };

                // Create IMAP client from the TLS stream
                let client = async_imap::Client::new(tls_stream);

                // Login
                let mut session = match client.login(&self.email_address, &self.password).await {
                    Ok(s) => s,
                    Err((e, _)) => {
                        warn!(error = %e, "Email IMAP login failed, retrying in 30s");
                        tokio::time::sleep(poll_interval).await;
                        continue;
                    }
                };

                info!("Email IMAP logged in as {}", self.email_address);

                // SELECT mailbox
                if let Err(e) = session.select(&self.mailbox).await {
                    warn!(error = %e, mailbox = %self.mailbox, "Email IMAP SELECT failed, retrying");
                    let _ = session.logout().await;
                    tokio::time::sleep(poll_interval).await;
                    continue;
                }

                info!(mailbox = %self.mailbox, "Email IMAP mailbox selected, entering poll loop");

                let mut session_broken = false;

                // IMAP poll loop: search UNSEEN, fetch, mark seen, sleep
                loop {
                    let unseen = match session.search("UNSEEN").await {
                        Ok(u) => u,
                        Err(e) => {
                            warn!(error = %e, "Email IMAP SEARCH UNSEEN failed, reconnecting");
                            session_broken = true;
                            break;
                        }
                    };

                    for uid in unseen.iter() {
                        if *uid <= highest_uid_seen {
                            continue;
                        }

                        let messages_stream = match session.fetch(uid.to_string(), "RFC822").await {
                            Ok(m) => m,
                            Err(e) => {
                                warn!(error = %e, uid, "Email IMAP FETCH failed");
                                continue;
                            }
                        };

                        let collected: Vec<_> = messages_stream.collect::<Vec<_>>().await;
                        for msg_result in collected {
                            let msg = match msg_result {
                                Ok(m) => m,
                                Err(e) => {
                                    warn!(error = %e, "Email IMAP fetch stream error");
                                    continue;
                                }
                            };
                            if let Some(body) = msg.body() {
                                if let Some(channel_msg) = self.parse_email(*uid, body) {
                                    if tx.send(channel_msg).await.is_err() {
                                        info!("Email listener: receiver dropped, stopping");
                                        let _ = session.logout().await;
                                        return Ok(());
                                    }
                                }
                            }
                        }

                        if *uid > highest_uid_seen {
                            highest_uid_seen = *uid;
                        }

                        // Mark as Seen
                        if let Err(e) = session.store(uid.to_string(), "+FLAGS (\\Seen)").await {
                            warn!(error = %e, uid, "Email IMAP STORE flags failed");
                        }
                    }

                    // Poll interval sleep before next check
                    tokio::time::sleep(poll_interval).await;

                    // NOOP to keep connection alive and detect disconnections
                    if let Err(e) = session.noop().await {
                        warn!(error = %e, "Email IMAP NOOP failed, reconnecting");
                        session_broken = true;
                        break;
                    }
                }

                if !session_broken {
                    let _ = session.logout().await;
                }

                warn!("Email IMAP session ended, reconnecting in 30s");
                tokio::time::sleep(poll_interval).await;
            }
        }

        #[cfg(not(feature = "email"))]
        {
            let _ = tx;
            warn!("Email channel requires the 'email' feature");
            std::future::pending::<()>().await;
            Ok(())
        }
    }

    async fn health_check(&self) -> Result<(), AttaError> {
        if self.imap_host.is_empty() || self.smtp_host.is_empty() {
            return Err(AttaError::Validation(
                "Email channel: IMAP/SMTP host not configured".to_string(),
            ));
        }
        if self.email_address.is_empty() || self.password.is_empty() {
            return Err(AttaError::Validation(
                "Email channel: credentials not configured".to_string(),
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_email_channel_name() {
        let ch = EmailChannel::new(
            "imap.gmail.com".to_string(),
            993,
            "smtp.gmail.com".to_string(),
            587,
            "test@gmail.com".to_string(),
            "password".to_string(),
        );
        assert_eq!(ch.name(), "email");
    }

    #[test]
    fn test_parse_email_basic() {
        let ch = EmailChannel::new(
            "imap.example.com".to_string(),
            993,
            "smtp.example.com".to_string(),
            587,
            "bot@example.com".to_string(),
            "pass".to_string(),
        );

        let raw = b"From: Alice <alice@example.com>\r\n\
                     Subject: Hello\r\n\
                     Message-ID: <msg001@example.com>\r\n\
                     \r\n\
                     Hello, world!\r\n";

        let msg = ch.parse_email(1, raw).unwrap();
        assert_eq!(msg.sender, "alice@example.com");
        assert_eq!(msg.content, "Hello, world!");
        assert_eq!(msg.channel, "email");
    }

    #[test]
    fn test_parse_email_with_reply() {
        let ch = EmailChannel::new(
            "host".to_string(),
            993,
            "host".to_string(),
            587,
            "a@b.com".to_string(),
            "p".to_string(),
        );

        let raw = b"From: bob@example.com\r\n\
                     In-Reply-To: <original@example.com>\r\n\
                     \r\n\
                     Re: your message\r\n";

        let msg = ch.parse_email(2, raw).unwrap();
        assert_eq!(msg.reply_target, Some("<original@example.com>".to_string()));
    }

    #[tokio::test]
    async fn test_health_check_valid_config() {
        let ch = EmailChannel::new(
            "imap.example.com".to_string(),
            993,
            "smtp.example.com".to_string(),
            587,
            "test@example.com".to_string(),
            "password".to_string(),
        );
        assert!(ch.health_check().await.is_ok());
    }

    #[tokio::test]
    async fn test_health_check_missing_host() {
        let ch = EmailChannel::new(
            "".to_string(),
            993,
            "smtp.example.com".to_string(),
            587,
            "test@example.com".to_string(),
            "password".to_string(),
        );
        assert!(ch.health_check().await.is_err());
    }
}
