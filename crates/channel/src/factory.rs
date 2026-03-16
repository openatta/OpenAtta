//! Channel factory — creates channel instances from configuration

use std::sync::Arc;

use atta_types::AttaError;

use crate::traits::Channel;

/// Channel configuration for factory creation
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ChannelConfig {
    /// Channel type identifier (e.g., "terminal", "telegram", "slack")
    #[serde(rename = "type")]
    pub channel_type: String,
    /// Whether this channel is enabled
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Channel-specific settings
    #[serde(default)]
    pub settings: serde_json::Value,
}

fn default_true() -> bool {
    true
}

/// Create a channel instance from configuration
pub fn create_channel(config: &ChannelConfig) -> Result<Arc<dyn Channel>, AttaError> {
    match config.channel_type.as_str() {
        #[cfg(feature = "terminal")]
        "terminal" => Ok(Arc::new(crate::impls::terminal::TerminalChannel::new())),

        #[cfg(feature = "webhook")]
        "webhook" => {
            let url = config
                .settings
                .get("outgoing_url")
                .and_then(|v| v.as_str())
                .ok_or_else(|| {
                    AttaError::Validation("webhook channel requires 'outgoing_url' setting".into())
                })?;
            let name = config
                .settings
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("webhook");
            Ok(Arc::new(crate::impls::webhook::WebhookChannel::new(
                name.to_string(),
                url.to_string(),
            )))
        }

        #[cfg(feature = "telegram")]
        "telegram" => {
            let bot_token = config
                .settings
                .get("bot_token")
                .and_then(|v| v.as_str())
                .ok_or_else(|| {
                    AttaError::Validation("telegram channel requires 'bot_token' setting".into())
                })?;
            let mut ch = crate::impls::telegram::TelegramChannel::new(bot_token.to_string());
            if let Some(secret) = config.settings.get("webhook_secret").and_then(|v| v.as_str()) {
                ch = ch.with_webhook_secret(secret.to_string());
            }
            if let Some(username) = config.settings.get("bot_username").and_then(|v| v.as_str()) {
                ch = ch.with_bot_username(username.to_string());
            }
            Ok(Arc::new(ch))
        }

        #[cfg(feature = "discord")]
        "discord" => {
            let bot_token = config
                .settings
                .get("bot_token")
                .and_then(|v| v.as_str())
                .ok_or_else(|| {
                    AttaError::Validation("discord channel requires 'bot_token' setting".into())
                })?;
            Ok(Arc::new(crate::impls::discord::DiscordChannel::new(
                bot_token.to_string(),
            )))
        }

        #[cfg(feature = "slack")]
        "slack" => {
            let bot_token = config
                .settings
                .get("bot_token")
                .and_then(|v| v.as_str())
                .ok_or_else(|| {
                    AttaError::Validation("slack channel requires 'bot_token' setting".into())
                })?;
            let app_token = config
                .settings
                .get("app_token")
                .and_then(|v| v.as_str())
                .ok_or_else(|| {
                    AttaError::Validation("slack channel requires 'app_token' setting".into())
                })?;
            Ok(Arc::new(crate::impls::slack::SlackChannel::new(
                bot_token.to_string(),
                app_token.to_string(),
            )))
        }

        #[cfg(feature = "lark")]
        "lark" => {
            let app_id = config
                .settings
                .get("app_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| {
                    AttaError::Validation("lark channel requires 'app_id' setting".into())
                })?;
            let app_secret = config
                .settings
                .get("app_secret")
                .and_then(|v| v.as_str())
                .ok_or_else(|| {
                    AttaError::Validation("lark channel requires 'app_secret' setting".into())
                })?;
            Ok(Arc::new(crate::impls::lark::LarkChannel::new(
                app_id.to_string(),
                app_secret.to_string(),
            )))
        }

        #[cfg(feature = "dingtalk")]
        "dingtalk" => {
            let app_key = config
                .settings
                .get("app_key")
                .and_then(|v| v.as_str())
                .ok_or_else(|| {
                    AttaError::Validation("dingtalk channel requires 'app_key' setting".into())
                })?;
            let app_secret = config
                .settings
                .get("app_secret")
                .and_then(|v| v.as_str())
                .ok_or_else(|| {
                    AttaError::Validation("dingtalk channel requires 'app_secret' setting".into())
                })?;
            Ok(Arc::new(crate::impls::dingtalk::DingtalkChannel::new(
                app_key.to_string(),
                app_secret.to_string(),
                None,
            )))
        }

        #[cfg(feature = "email")]
        "email" => {
            let imap_host = config
                .settings
                .get("imap_host")
                .and_then(|v| v.as_str())
                .ok_or_else(|| {
                    AttaError::Validation("email channel requires 'imap_host' setting".into())
                })?;
            let imap_port = config
                .settings
                .get("imap_port")
                .and_then(|v| v.as_u64())
                .unwrap_or(993) as u16;
            let smtp_host = config
                .settings
                .get("smtp_host")
                .and_then(|v| v.as_str())
                .ok_or_else(|| {
                    AttaError::Validation("email channel requires 'smtp_host' setting".into())
                })?;
            let smtp_port = config
                .settings
                .get("smtp_port")
                .and_then(|v| v.as_u64())
                .unwrap_or(587) as u16;
            let username = config
                .settings
                .get("username")
                .and_then(|v| v.as_str())
                .ok_or_else(|| {
                    AttaError::Validation("email channel requires 'username' setting".into())
                })?;
            let password = config
                .settings
                .get("password")
                .and_then(|v| v.as_str())
                .ok_or_else(|| {
                    AttaError::Validation("email channel requires 'password' setting".into())
                })?;
            Ok(Arc::new(crate::impls::email::EmailChannel::new(
                imap_host.to_string(),
                imap_port,
                smtp_host.to_string(),
                smtp_port,
                username.to_string(),
                password.to_string(),
            )))
        }

        #[cfg(feature = "qq")]
        "qq" => {
            let app_id = config
                .settings
                .get("app_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| {
                    AttaError::Validation("qq channel requires 'app_id' setting".into())
                })?;
            let token = config
                .settings
                .get("token")
                .and_then(|v| v.as_str())
                .ok_or_else(|| {
                    AttaError::Validation("qq channel requires 'token' setting".into())
                })?;
            let sandbox = config
                .settings
                .get("sandbox")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            Ok(Arc::new(crate::impls::qq::QqChannel::new(
                app_id.to_string(),
                token.to_string(),
                sandbox,
            )))
        }

        #[cfg(feature = "mattermost")]
        "mattermost" => {
            let url = config
                .settings
                .get("url")
                .and_then(|v| v.as_str())
                .ok_or_else(|| {
                    AttaError::Validation("mattermost channel requires 'url' setting".into())
                })?;
            let token = config
                .settings
                .get("token")
                .and_then(|v| v.as_str())
                .ok_or_else(|| {
                    AttaError::Validation("mattermost channel requires 'token' setting".into())
                })?;
            Ok(Arc::new(crate::impls::mattermost::MattermostChannel::new(
                url.to_string(),
                token.to_string(),
            )))
        }

        #[cfg(feature = "irc")]
        "irc" => {
            let server = config
                .settings
                .get("server")
                .and_then(|v| v.as_str())
                .ok_or_else(|| {
                    AttaError::Validation("irc channel requires 'server' setting".into())
                })?;
            let port = config
                .settings
                .get("port")
                .and_then(|v| v.as_u64())
                .unwrap_or(6667) as u16;
            let nick = config
                .settings
                .get("nick")
                .and_then(|v| v.as_str())
                .ok_or_else(|| {
                    AttaError::Validation("irc channel requires 'nick' setting".into())
                })?;
            let irc_channel = config
                .settings
                .get("channel")
                .and_then(|v| v.as_str())
                .ok_or_else(|| {
                    AttaError::Validation("irc channel requires 'channel' setting".into())
                })?;
            Ok(Arc::new(crate::impls::irc_channel::IrcChannel::new(
                server.to_string(),
                port,
                nick.to_string(),
                irc_channel.to_string(),
            )))
        }

        #[cfg(feature = "signal")]
        "signal" => {
            let phone = config
                .settings
                .get("phone_number")
                .and_then(|v| v.as_str())
                .ok_or_else(|| {
                    AttaError::Validation("signal channel requires 'phone_number' setting".into())
                })?;
            Ok(Arc::new(crate::impls::signal::SignalChannel::new(
                phone.to_string(),
            )))
        }

        #[cfg(all(feature = "imessage", target_os = "macos"))]
        "imessage" => Ok(Arc::new(crate::impls::imessage::IMessageChannel::new())),

        #[cfg(feature = "whatsapp")]
        "whatsapp" => {
            let phone_id = config
                .settings
                .get("phone_number_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| {
                    AttaError::Validation(
                        "whatsapp channel requires 'phone_number_id' setting".into(),
                    )
                })?;
            let access_token = config
                .settings
                .get("access_token")
                .and_then(|v| v.as_str())
                .ok_or_else(|| {
                    AttaError::Validation("whatsapp channel requires 'access_token' setting".into())
                })?;
            Ok(Arc::new(crate::impls::whatsapp::WhatsappChannel::new(
                access_token.to_string(),
                phone_id.to_string(),
            )))
        }

        #[cfg(feature = "matrix")]
        "matrix" => {
            let homeserver = config
                .settings
                .get("homeserver")
                .and_then(|v| v.as_str())
                .ok_or_else(|| {
                    AttaError::Validation("matrix channel requires 'homeserver' setting".into())
                })?;
            let username = config
                .settings
                .get("username")
                .and_then(|v| v.as_str())
                .ok_or_else(|| {
                    AttaError::Validation("matrix channel requires 'username' setting".into())
                })?;
            let password = config
                .settings
                .get("password")
                .and_then(|v| v.as_str())
                .ok_or_else(|| {
                    AttaError::Validation("matrix channel requires 'password' setting".into())
                })?;
            Ok(Arc::new(crate::impls::matrix::MatrixChannel::new(
                homeserver.to_string(),
                username.to_string(),
                password.to_string(),
            )))
        }

        #[cfg(feature = "nostr")]
        "nostr" => {
            let private_key = config
                .settings
                .get("private_key")
                .and_then(|v| v.as_str())
                .ok_or_else(|| {
                    AttaError::Validation("nostr channel requires 'private_key' setting".into())
                })?;
            let relay_urls: Vec<String> = config
                .settings
                .get("relay_urls")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                        .collect()
                })
                .unwrap_or_else(|| vec!["wss://relay.damus.io".to_string()]);
            Ok(Arc::new(crate::impls::nostr::NostrChannel::new(
                relay_urls,
                private_key.to_string(),
            )))
        }

        #[cfg(feature = "mqtt")]
        "mqtt" => {
            let host = config
                .settings
                .get("host")
                .and_then(|v| v.as_str())
                .ok_or_else(|| {
                    AttaError::Validation("mqtt channel requires 'host' setting".into())
                })?;
            let port = config
                .settings
                .get("port")
                .and_then(|v| v.as_u64())
                .unwrap_or(1883) as u16;
            let client_id = config
                .settings
                .get("client_id")
                .and_then(|v| v.as_str())
                .unwrap_or("atta-mqtt");
            let subscribe_topic = config
                .settings
                .get("subscribe_topic")
                .or_else(|| config.settings.get("topic"))
                .and_then(|v| v.as_str())
                .ok_or_else(|| {
                    AttaError::Validation(
                        "mqtt channel requires 'subscribe_topic' or 'topic' setting".into(),
                    )
                })?;
            let publish_topic = config
                .settings
                .get("publish_topic")
                .and_then(|v| v.as_str())
                .unwrap_or(subscribe_topic);
            Ok(Arc::new(crate::impls::mqtt::MqttChannel::new(
                host.to_string(),
                port,
                client_id.to_string(),
                subscribe_topic.to_string(),
                publish_topic.to_string(),
            )))
        }

        #[cfg(feature = "wati")]
        "wati" => {
            let api_url = config
                .settings
                .get("api_url")
                .and_then(|v| v.as_str())
                .ok_or_else(|| {
                    AttaError::Validation("wati channel requires 'api_url' setting".into())
                })?;
            let api_key = config
                .settings
                .get("api_key")
                .and_then(|v| v.as_str())
                .ok_or_else(|| {
                    AttaError::Validation("wati channel requires 'api_key' setting".into())
                })?;
            Ok(Arc::new(crate::impls::wati::WatiChannel::new(
                api_url.to_string(),
                api_key.to_string(),
            )))
        }

        #[cfg(feature = "nextcloud-talk")]
        "nextcloud-talk" => {
            let url = config
                .settings
                .get("url")
                .and_then(|v| v.as_str())
                .ok_or_else(|| {
                    AttaError::Validation("nextcloud-talk channel requires 'url' setting".into())
                })?;
            let username = config
                .settings
                .get("username")
                .and_then(|v| v.as_str())
                .ok_or_else(|| {
                    AttaError::Validation(
                        "nextcloud-talk channel requires 'username' setting".into(),
                    )
                })?;
            let password = config
                .settings
                .get("password")
                .and_then(|v| v.as_str())
                .ok_or_else(|| {
                    AttaError::Validation(
                        "nextcloud-talk channel requires 'password' setting".into(),
                    )
                })?;
            let conversation_token = config
                .settings
                .get("conversation_token")
                .and_then(|v| v.as_str())
                .ok_or_else(|| {
                    AttaError::Validation(
                        "nextcloud-talk channel requires 'conversation_token' setting".into(),
                    )
                })?;
            Ok(Arc::new(
                crate::impls::nextcloud_talk::NextcloudTalkChannel::new(
                    url.to_string(),
                    username.to_string(),
                    password.to_string(),
                    conversation_token.to_string(),
                ),
            ))
        }

        #[cfg(feature = "clawdtalk")]
        "clawdtalk" => {
            let api_url = config
                .settings
                .get("api_url")
                .and_then(|v| v.as_str())
                .ok_or_else(|| {
                    AttaError::Validation("clawdtalk channel requires 'api_url' setting".into())
                })?;
            let api_key = config
                .settings
                .get("api_key")
                .and_then(|v| v.as_str())
                .ok_or_else(|| {
                    AttaError::Validation("clawdtalk channel requires 'api_key' setting".into())
                })?;
            Ok(Arc::new(crate::impls::clawdtalk::ClawdTalkChannel::new(
                api_url.to_string(),
                api_key.to_string(),
            )))
        }

        other => Err(AttaError::Validation(format!(
            "unknown channel type: {other}"
        ))),
    }
}
