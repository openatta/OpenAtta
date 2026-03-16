//! ChannelMedia section — channel media capabilities

use super::super::section::{PromptContext, PromptSection};

/// ChannelMedia section — describes the channel's media capabilities
pub struct ChannelMediaSection;

impl PromptSection for ChannelMediaSection {
    fn name(&self) -> &str {
        "Channel"
    }

    fn priority(&self) -> u32 {
        90
    }

    fn build(&self, ctx: &PromptContext) -> Option<String> {
        let channel = ctx.channel.as_deref()?;

        let capabilities = match channel {
            "terminal" => "text-only, no rich formatting",
            "telegram" => "Markdown, images, files, inline keyboards",
            "discord" => "Markdown, embeds, images, reactions, threads",
            "slack" => "mrkdwn, blocks, images, reactions, threads",
            "email" => "HTML, attachments",
            "webhook" => "JSON payloads",
            _ => "text",
        };

        Some(format!("Channel: **{channel}** — supports: {capabilities}"))
    }
}
