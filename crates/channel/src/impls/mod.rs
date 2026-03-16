//! Channel implementations

#[cfg(feature = "terminal")]
pub mod terminal;

#[cfg(feature = "webhook")]
pub mod webhook;

#[cfg(feature = "telegram")]
pub mod telegram;

#[cfg(feature = "discord")]
pub mod discord;

#[cfg(feature = "slack")]
pub mod slack;

#[cfg(feature = "lark")]
pub mod lark;

#[cfg(feature = "dingtalk")]
pub mod dingtalk;

#[cfg(feature = "qq")]
pub mod qq;

#[cfg(feature = "email")]
pub mod email;

#[cfg(feature = "irc")]
pub mod irc_channel;

#[cfg(feature = "signal")]
pub mod signal;

#[cfg(feature = "imessage")]
pub mod imessage;

#[cfg(feature = "whatsapp")]
pub mod whatsapp;

#[cfg(feature = "whatsapp-web")]
pub mod whatsapp_web;

#[cfg(feature = "whatsapp-storage")]
pub mod whatsapp_storage;

#[cfg(feature = "matrix")]
pub mod matrix;

#[cfg(feature = "mattermost")]
pub mod mattermost;

#[cfg(feature = "nostr")]
pub mod nostr;

#[cfg(feature = "mqtt")]
pub mod mqtt;

#[cfg(feature = "wati")]
pub mod wati;

#[cfg(feature = "nextcloud-talk")]
pub mod nextcloud_talk;

#[cfg(feature = "clawdtalk")]
pub mod clawdtalk;
