//! 对话上下文管理
//!
//! [`ConversationContext`] 维护 Agent 与 LLM 之间的对话历史，
//! 支持 system / user / assistant / tool 四种角色的消息管理。

use crate::llm::{Message, ToolCall};

/// 对话上下文
///
/// 存储当前 Agent 执行周期内的对话消息序列，
/// 提供按角色添加消息和获取完整消息列表的方法。
#[derive(Debug, Clone)]
pub struct ConversationContext {
    /// 对话消息列表
    messages: Vec<Message>,
    /// 上下文窗口最大 token 数（用于未来截断策略）
    max_tokens: usize,
}

impl ConversationContext {
    /// 创建新的对话上下文
    ///
    /// # Arguments
    /// * `max_tokens` - 上下文窗口最大 token 数
    pub fn new(max_tokens: usize) -> Self {
        Self {
            messages: Vec::new(),
            max_tokens,
        }
    }

    /// 返回上下文窗口最大 token 数
    pub fn max_tokens(&self) -> usize {
        self.max_tokens
    }

    /// 设置 system prompt（替换已有的 system 消息）
    pub fn set_system(&mut self, content: &str) {
        // 移除已有的 system 消息
        self.messages.retain(|m| !matches!(m, Message::System(_)));
        // 在头部插入新的 system 消息
        self.messages
            .insert(0, Message::System(content.to_string()));
    }

    /// 添加用户消息
    pub fn add_user(&mut self, content: &str) {
        self.messages.push(Message::User(content.to_string()));
    }

    /// 添加 assistant（LLM）纯文本消息
    pub fn add_assistant(&mut self, content: &str) {
        self.messages.push(Message::Assistant(content.to_string()));
    }

    /// 添加 assistant 发起的 tool 调用消息
    pub fn add_assistant_tool_calls(&mut self, calls: Vec<ToolCall>) {
        self.messages.push(Message::AssistantToolCalls(calls));
    }

    /// 添加 tool 执行结果消息
    ///
    /// # Arguments
    /// * `tool_call_id` - 对应的 ToolCall ID
    /// * `result` - Tool 执行结果文本
    pub fn add_tool_result(&mut self, tool_call_id: &str, result: &str) {
        self.messages.push(Message::ToolResult {
            tool_call_id: tool_call_id.to_string(),
            content: result.to_string(),
        });
    }

    /// Truncate conversation history to fit within `max_tokens`.
    ///
    /// Preserves the system message (if any) and the most recent messages.
    /// Uses a rough heuristic of 4 characters ≈ 1 token.
    pub fn truncate_to_fit(&mut self) {
        let estimated_tokens = self
            .messages
            .iter()
            .map(Self::estimate_message_tokens)
            .sum::<usize>();

        if estimated_tokens <= self.max_tokens {
            return;
        }

        // Separate system message from the rest
        let has_system = matches!(self.messages.first(), Some(Message::System(_)));
        let system_msg = if has_system {
            Some(self.messages.remove(0))
        } else {
            None
        };

        let system_tokens = system_msg
            .as_ref()
            .map(Self::estimate_message_tokens)
            .unwrap_or(0);
        let budget = self.max_tokens.saturating_sub(system_tokens);

        // Keep messages from the end until budget is exhausted
        let mut kept = Vec::new();
        let mut used = 0;
        for msg in self.messages.iter().rev() {
            let t = Self::estimate_message_tokens(msg);
            if used + t > budget {
                break;
            }
            used += t;
            kept.push(msg.clone());
        }
        kept.reverse();

        self.messages = if let Some(sys) = system_msg {
            let mut v = vec![sys];
            v.extend(kept);
            v
        } else {
            kept
        };
    }

    /// Rough token estimate: 4 chars ≈ 1 token
    fn estimate_message_tokens(msg: &Message) -> usize {
        let chars = match msg {
            Message::System(s) | Message::User(s) | Message::Assistant(s) => s.len(),
            Message::AssistantToolCalls(calls) => calls
                .iter()
                .map(|c| c.name.len() + c.arguments.to_string().len() + c.id.len())
                .sum(),
            Message::ToolResult { content, .. } => content.len(),
        };
        (chars / 4).max(1)
    }

    /// 返回完整消息列表引用
    pub fn messages(&self) -> &[Message] {
        &self.messages
    }

    /// 返回最后一条用户消息的内容
    pub fn last_user_message(&self) -> Option<String> {
        self.messages.iter().rev().find_map(|m| match m {
            Message::User(content) => Some(content.clone()),
            _ => None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_context() {
        let ctx = ConversationContext::new(4096);
        assert_eq!(ctx.max_tokens(), 4096);
        assert!(ctx.messages().is_empty());
    }

    #[test]
    fn test_set_system() {
        let mut ctx = ConversationContext::new(4096);
        ctx.set_system("You are a helpful assistant.");
        assert_eq!(ctx.messages().len(), 1);
        match &ctx.messages()[0] {
            Message::System(content) => {
                assert_eq!(content, "You are a helpful assistant.");
            }
            other => panic!("expected Message::System, got {:?}", other),
        }
    }

    #[test]
    fn test_set_system_replaces_existing() {
        let mut ctx = ConversationContext::new(4096);
        ctx.set_system("First prompt");
        ctx.set_system("Second prompt");
        let system_msgs: Vec<_> = ctx
            .messages()
            .iter()
            .filter(|m| matches!(m, Message::System(_)))
            .collect();
        assert_eq!(system_msgs.len(), 1);
        match system_msgs[0] {
            Message::System(content) => assert_eq!(content, "Second prompt"),
            other => panic!("expected Message::System, got {:?}", other),
        }
    }

    #[test]
    fn test_message_ordering() {
        let mut ctx = ConversationContext::new(4096);
        ctx.set_system("System prompt");
        ctx.add_user("Hello");
        ctx.add_assistant("Hi there");
        ctx.add_tool_result("call_1", "tool output");

        let msgs = ctx.messages();
        assert_eq!(msgs.len(), 4);
        assert!(matches!(msgs[0], Message::System(_)));
        assert!(matches!(msgs[1], Message::User(_)));
        assert!(matches!(msgs[2], Message::Assistant(_)));
        assert!(matches!(msgs[3], Message::ToolResult { .. }));
    }

    #[test]
    fn test_add_assistant_tool_calls() {
        let mut ctx = ConversationContext::new(4096);
        ctx.add_assistant_tool_calls(vec![ToolCall {
            id: "tc_1".to_string(),
            name: "search".to_string(),
            arguments: serde_json::json!({"query": "rust"}),
        }]);

        let msgs = ctx.messages();
        assert_eq!(msgs.len(), 1);
        match &msgs[0] {
            Message::AssistantToolCalls(calls) => {
                assert_eq!(calls.len(), 1);
                assert_eq!(calls[0].name, "search");
            }
            other => panic!("expected Message::AssistantToolCalls, got {:?}", other),
        }
    }

    #[test]
    fn test_tool_result_structured() {
        let mut ctx = ConversationContext::new(4096);
        ctx.add_tool_result("call_1", "tool output");

        let msgs = ctx.messages();
        assert_eq!(msgs.len(), 1);
        match &msgs[0] {
            Message::ToolResult {
                tool_call_id,
                content,
            } => {
                assert_eq!(tool_call_id, "call_1");
                assert_eq!(content, "tool output");
            }
            other => panic!("expected Message::ToolResult, got {:?}", other),
        }
    }
}
