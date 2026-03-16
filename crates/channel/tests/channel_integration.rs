//! Integration tests for the Channel system
//!
//! Tests the Channel trait, ChannelRegistry, DraftManager, supervisor,
//! dispatch loop, and webhook push model using in-process mocks.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use chrono::Utc;
use serde_json::json;
use tokio::sync::mpsc;
use tokio::sync::Mutex;
use tokio::time::timeout;
use tokio_util::sync::CancellationToken;

use atta_channel::{
    utf8_truncate, Channel, ChannelMessage, ChannelMessageHandler, ChannelRegistry, ChatType,
    DraftManager, SendMessage,
};
use atta_types::AttaError;

// ---------------------------------------------------------------------------
// Mock implementations
// ---------------------------------------------------------------------------

/// A mock channel that captures sent messages and can push incoming messages.
struct MockChannel {
    name: String,
    sent: Mutex<Vec<SendMessage>>,
    health_ok: bool,
    send_fail: bool,
    typing_starts: AtomicU32,
    typing_stops: AtomicU32,
}

impl MockChannel {
    fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            sent: Mutex::new(Vec::new()),
            health_ok: true,
            send_fail: false,
            typing_starts: AtomicU32::new(0),
            typing_stops: AtomicU32::new(0),
        }
    }

    fn failing(name: &str) -> Self {
        Self {
            name: name.to_string(),
            sent: Mutex::new(Vec::new()),
            health_ok: false,
            send_fail: true,
            typing_starts: AtomicU32::new(0),
            typing_stops: AtomicU32::new(0),
        }
    }

    async fn sent_messages(&self) -> Vec<SendMessage> {
        self.sent.lock().await.clone()
    }
}

#[async_trait]
impl Channel for MockChannel {
    fn name(&self) -> &str {
        &self.name
    }

    async fn send(&self, message: SendMessage) -> Result<(), AttaError> {
        if self.send_fail {
            return Err(AttaError::Channel("mock send failure".to_string()));
        }
        self.sent.lock().await.push(message);
        Ok(())
    }

    async fn listen(&self, _tx: mpsc::Sender<ChannelMessage>) -> Result<(), AttaError> {
        // Block forever in push mode
        std::future::pending::<()>().await;
        Ok(())
    }

    async fn health_check(&self) -> Result<(), AttaError> {
        if self.health_ok {
            Ok(())
        } else {
            Err(AttaError::Channel("unhealthy".to_string()))
        }
    }

    async fn start_typing(&self, _recipient: &str) -> Result<(), AttaError> {
        self.typing_starts.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }

    async fn stop_typing(&self, _recipient: &str) -> Result<(), AttaError> {
        self.typing_stops.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }
}

/// A channel that immediately returns from listen() (simulating disconnect).
struct DisconnectingChannel {
    name: String,
    disconnect_count: AtomicU32,
}

impl DisconnectingChannel {
    fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            disconnect_count: AtomicU32::new(0),
        }
    }
}

#[async_trait]
impl Channel for DisconnectingChannel {
    fn name(&self) -> &str {
        &self.name
    }

    async fn send(&self, _message: SendMessage) -> Result<(), AttaError> {
        Ok(())
    }

    async fn listen(&self, _tx: mpsc::Sender<ChannelMessage>) -> Result<(), AttaError> {
        self.disconnect_count.fetch_add(1, Ordering::Relaxed);
        // Immediate return simulates a disconnect
        Ok(())
    }

    async fn health_check(&self) -> Result<(), AttaError> {
        Ok(())
    }
}

/// A channel whose listen() fails with an error.
struct ErrorChannel {
    name: String,
    error_count: AtomicU32,
}

impl ErrorChannel {
    fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            error_count: AtomicU32::new(0),
        }
    }
}

#[async_trait]
impl Channel for ErrorChannel {
    fn name(&self) -> &str {
        &self.name
    }

    async fn send(&self, _message: SendMessage) -> Result<(), AttaError> {
        Ok(())
    }

    async fn listen(&self, _tx: mpsc::Sender<ChannelMessage>) -> Result<(), AttaError> {
        self.error_count.fetch_add(1, Ordering::Relaxed);
        Err(AttaError::Channel("connection lost".to_string()))
    }

    async fn health_check(&self) -> Result<(), AttaError> {
        Ok(())
    }
}

/// A channel that pushes one message then blocks forever.
struct SingleMessageChannel {
    name: String,
    message: ChannelMessage,
}

impl SingleMessageChannel {
    fn new(name: &str, message: ChannelMessage) -> Self {
        Self {
            name: name.to_string(),
            message,
        }
    }
}

#[async_trait]
impl Channel for SingleMessageChannel {
    fn name(&self) -> &str {
        &self.name
    }

    async fn send(&self, _message: SendMessage) -> Result<(), AttaError> {
        Ok(())
    }

    async fn listen(&self, tx: mpsc::Sender<ChannelMessage>) -> Result<(), AttaError> {
        tx.send(self.message.clone())
            .await
            .map_err(|_| AttaError::Channel("tx closed".to_string()))?;
        std::future::pending::<()>().await;
        Ok(())
    }

    async fn health_check(&self) -> Result<(), AttaError> {
        Ok(())
    }
}

/// Mock message handler that records all handled messages.
struct RecordingHandler {
    messages: Mutex<Vec<(String, String)>>, // (channel_name, content)
}

impl RecordingHandler {
    fn new() -> Self {
        Self {
            messages: Mutex::new(Vec::new()),
        }
    }
}

#[async_trait]
impl ChannelMessageHandler for RecordingHandler {
    async fn handle(&self, msg: &ChannelMessage, channel: &dyn Channel) {
        self.messages
            .lock()
            .await
            .push((channel.name().to_string(), msg.content.clone()));
    }
}

/// Build a test ChannelMessage.
fn make_message(channel: &str, content: &str) -> ChannelMessage {
    ChannelMessage {
        id: uuid::Uuid::new_v4().to_string(),
        sender: "test-user".to_string(),
        content: content.to_string(),
        channel: channel.to_string(),
        reply_target: None,
        timestamp: Utc::now(),
        thread_ts: None,
        metadata: json!({}),
        chat_type: ChatType::default(),
        bot_mentioned: false,
        group_id: None,
    }
}

/// Build a test SendMessage.
fn make_send_message(recipient: &str, content: &str) -> SendMessage {
    SendMessage {
        recipient: recipient.to_string(),
        content: content.to_string(),
        subject: None,
        thread_ts: None,
        metadata: json!({}),
    }
}

// ===========================================================================
// Channel trait tests
// ===========================================================================

#[tokio::test]
async fn test_mock_channel_send_captures() {
    let ch = MockChannel::new("test");
    let msg = make_send_message("user-1", "hello");
    ch.send(msg).await.unwrap();

    let sent = ch.sent_messages().await;
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].content, "hello");
    assert_eq!(sent[0].recipient, "user-1");
}

#[tokio::test]
async fn test_mock_channel_send_multiple() {
    let ch = MockChannel::new("test");
    for i in 0..5 {
        ch.send(make_send_message("u", &format!("msg-{i}")))
            .await
            .unwrap();
    }
    assert_eq!(ch.sent_messages().await.len(), 5);
}

#[tokio::test]
async fn test_failing_channel_send() {
    let ch = MockChannel::failing("broken");
    let result = ch.send(make_send_message("u", "hi")).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_channel_health_check_ok() {
    let ch = MockChannel::new("test");
    assert!(ch.health_check().await.is_ok());
}

#[tokio::test]
async fn test_channel_health_check_fail() {
    let ch = MockChannel::failing("broken");
    assert!(ch.health_check().await.is_err());
}

#[tokio::test]
async fn test_channel_typing_indicators() {
    let ch = MockChannel::new("test");
    ch.start_typing("user").await.unwrap();
    ch.start_typing("user").await.unwrap();
    ch.stop_typing("user").await.unwrap();

    assert_eq!(ch.typing_starts.load(Ordering::Relaxed), 2);
    assert_eq!(ch.typing_stops.load(Ordering::Relaxed), 1);
}

#[tokio::test]
async fn test_default_trait_methods() {
    let ch = MockChannel::new("test");

    // Default: supports_draft_updates is false
    assert!(!ch.supports_draft_updates());

    // Default: send_draft delegates to send
    let result = ch
        .send_draft(make_send_message("u", "draft"))
        .await
        .unwrap();
    assert_eq!(result, ""); // Default returns empty string

    // Default: update_draft/finalize_draft/cancel_draft are no-ops
    assert!(ch.update_draft("id", "new content").await.is_ok());
    assert!(ch.finalize_draft("id").await.is_ok());
    assert!(ch.cancel_draft("id").await.is_ok());

    // Default: approval prompt is no-op
    assert!(ch
        .send_approval_prompt("u", "req-1", "shell", &json!({}), None)
        .await
        .is_ok());

    // Default: reactions are no-ops
    assert!(ch.add_reaction("msg-1", "thumbsup").await.is_ok());
    assert!(ch.remove_reaction("msg-1", "thumbsup").await.is_ok());

    // send_draft actually sent via send()
    assert_eq!(ch.sent_messages().await.len(), 1);
}

// ===========================================================================
// ChannelRegistry tests
// ===========================================================================

#[tokio::test]
async fn test_registry_insert_get_remove() {
    let registry = ChannelRegistry::new();
    assert!(registry.is_empty().await);

    let ch: Arc<dyn Channel> = Arc::new(MockChannel::new("alpha"));
    registry.insert("alpha".to_string(), ch).await;

    assert_eq!(registry.len().await, 1);
    assert!(!registry.is_empty().await);

    let found = registry.get("alpha").await;
    assert!(found.is_some());
    assert_eq!(found.unwrap().name(), "alpha");

    assert!(registry.get("nonexistent").await.is_none());

    let removed = registry.remove("alpha").await;
    assert!(removed.is_some());
    assert!(registry.is_empty().await);
}

#[tokio::test]
async fn test_registry_list_sorted() {
    let registry = ChannelRegistry::new();
    for name in ["charlie", "alpha", "bravo"] {
        registry
            .insert(name.to_string(), Arc::new(MockChannel::new(name)))
            .await;
    }
    let mut names = registry.list().await;
    names.sort();
    assert_eq!(names, vec!["alpha", "bravo", "charlie"]);
}

#[tokio::test]
async fn test_registry_overwrite() {
    let registry = ChannelRegistry::new();
    registry
        .insert("ch".to_string(), Arc::new(MockChannel::new("old")))
        .await;
    registry
        .insert("ch".to_string(), Arc::new(MockChannel::new("new")))
        .await;

    assert_eq!(registry.len().await, 1);
    // The new channel replaced the old one
    let ch = registry.get("ch").await.unwrap();
    assert_eq!(ch.name(), "new");
}

#[tokio::test]
async fn test_registry_remove_nonexistent() {
    let registry = ChannelRegistry::new();
    let removed = registry.remove("ghost").await;
    assert!(removed.is_none());
}

// ===========================================================================
// DraftManager tests
// ===========================================================================

#[test]
fn test_draft_accumulate_and_rate_limit() {
    let mgr = DraftManager::new(Duration::from_secs(60));

    // First call always flushes
    let result = mgr.accumulate("hello");
    assert_eq!(result, Some("hello".to_string()));

    // Second call within interval is rate-limited
    let result = mgr.accumulate(" world");
    assert!(result.is_none());

    // peek() shows accumulated text
    assert_eq!(mgr.peek(), "hello world");
}

#[test]
fn test_draft_flush_bypasses_rate_limit() {
    let mgr = DraftManager::new(Duration::from_secs(60));
    mgr.accumulate("one");
    mgr.accumulate(" two");

    let flushed = mgr.flush();
    assert_eq!(flushed, "one two");
}

#[test]
fn test_draft_reset_clears_everything() {
    let mgr = DraftManager::new(Duration::from_millis(10));
    mgr.accumulate("data");
    mgr.reset();
    assert_eq!(mgr.peek(), "");

    // After reset, next accumulate should flush immediately
    let result = mgr.accumulate("fresh");
    assert_eq!(result, Some("fresh".to_string()));
}

#[test]
fn test_draft_rapid_accumulation() {
    let mgr = DraftManager::new(Duration::from_secs(60));
    mgr.accumulate("a");
    for _ in 0..100 {
        let _ = mgr.accumulate("b");
    }
    let text = mgr.peek();
    assert_eq!(text.len(), 101); // "a" + 100 * "b"
    assert!(text.starts_with('a'));
}

// ===========================================================================
// utf8_truncate tests
// ===========================================================================

#[test]
fn test_utf8_truncate_exact_boundary() {
    // 3-byte UTF-8 chars
    let s = "你好世界";
    assert_eq!(utf8_truncate(s, 3), "你");
    assert_eq!(utf8_truncate(s, 4), "你"); // can't split 好
    assert_eq!(utf8_truncate(s, 5), "你"); // still can't
    assert_eq!(utf8_truncate(s, 6), "你好");
}

#[test]
fn test_utf8_truncate_emoji() {
    let s = "hello 👋 world";
    // "hello " is 6 bytes, 👋 is 4 bytes
    let truncated = utf8_truncate(s, 8);
    assert_eq!(truncated, "hello "); // can't fit the emoji
}

#[test]
fn test_utf8_truncate_zero() {
    assert_eq!(utf8_truncate("hello", 0), "");
}

#[test]
fn test_utf8_truncate_larger_than_string() {
    assert_eq!(utf8_truncate("hi", 100), "hi");
}

// ===========================================================================
// Supervisor tests
// ===========================================================================

#[tokio::test]
async fn test_supervisor_cancel_stops_before_listen() {
    // Cancel before starting — supervisor should exit on the first loop check.
    let ch: Arc<dyn Channel> = Arc::new(DisconnectingChannel::new("test"));
    let (tx, _rx) = mpsc::channel(10);
    let cancel = CancellationToken::new();
    cancel.cancel(); // pre-cancel

    let handle = tokio::spawn(async move {
        atta_channel::supervisor::supervised_listener(ch, tx, cancel).await;
    });

    let result = timeout(Duration::from_secs(2), handle).await;
    assert!(result.is_ok(), "supervisor should stop immediately when pre-cancelled");
}

#[tokio::test]
async fn test_supervisor_retries_on_disconnect() {
    let ch = Arc::new(DisconnectingChannel::new("flaky"));
    let (tx, _rx) = mpsc::channel(10);
    let cancel = CancellationToken::new();

    let cancel_clone = cancel.clone();
    let ch_clone = ch.clone();
    let handle = tokio::spawn(async move {
        atta_channel::supervisor::supervised_listener(ch_clone, tx, cancel_clone).await;
    });

    // Wait enough for at least 2 reconnection attempts (backoff: 1s, 2s...)
    tokio::time::sleep(Duration::from_millis(1500)).await;
    cancel.cancel();

    let _ = timeout(Duration::from_secs(2), handle).await;

    let count = ch.disconnect_count.load(Ordering::Relaxed);
    assert!(
        count >= 2,
        "supervisor should have retried at least twice, got {count}"
    );
}

#[tokio::test]
async fn test_supervisor_retries_on_error() {
    let ch = Arc::new(ErrorChannel::new("erroring"));
    let (tx, _rx) = mpsc::channel(10);
    let cancel = CancellationToken::new();

    let cancel_clone = cancel.clone();
    let ch_clone = ch.clone();
    let handle = tokio::spawn(async move {
        atta_channel::supervisor::supervised_listener(ch_clone, tx, cancel_clone).await;
    });

    tokio::time::sleep(Duration::from_millis(1500)).await;
    cancel.cancel();

    let _ = timeout(Duration::from_secs(2), handle).await;

    let count = ch.error_count.load(Ordering::Relaxed);
    assert!(
        count >= 2,
        "supervisor should have retried after errors, got {count}"
    );
}

#[tokio::test]
async fn test_supervisor_delivers_messages() {
    let msg = make_message("push-ch", "hello from channel");
    let ch: Arc<dyn Channel> = Arc::new(SingleMessageChannel::new("push-ch", msg));
    let (tx, mut rx) = mpsc::channel(10);
    let cancel = CancellationToken::new();

    let cancel_clone = cancel.clone();
    tokio::spawn(async move {
        atta_channel::supervisor::supervised_listener(ch, tx, cancel_clone).await;
    });

    // Should receive the message
    let received = timeout(Duration::from_secs(2), rx.recv()).await;
    assert!(received.is_ok());
    let received = received.unwrap().unwrap();
    assert_eq!(received.content, "hello from channel");
    assert_eq!(received.channel, "push-ch");

    cancel.cancel();
}

// ===========================================================================
// Dispatch loop tests
// ===========================================================================

#[tokio::test]
async fn test_dispatch_loop_routes_to_handler() {
    let handler = Arc::new(RecordingHandler::new());
    let ch: Arc<dyn Channel> = Arc::new(MockChannel::new("test-ch"));

    let mut channels = HashMap::new();
    channels.insert("test-ch".to_string(), ch);

    let cancel = CancellationToken::new();
    let (tx, rx) = mpsc::channel(10);

    let handler_clone = handler.clone();
    let cancel_clone = cancel.clone();
    tokio::spawn(async move {
        atta_channel::dispatch::run_message_dispatch_loop(rx, channels, handler_clone, cancel_clone)
            .await;
    });

    tx.send(make_message("test-ch", "hello")).await.unwrap();

    // Give the handler time to process
    tokio::time::sleep(Duration::from_millis(100)).await;
    cancel.cancel();

    let msgs = handler.messages.lock().await;
    assert_eq!(msgs.len(), 1);
    assert_eq!(msgs[0].0, "test-ch");
    assert_eq!(msgs[0].1, "hello");
}

#[tokio::test]
async fn test_dispatch_loop_unknown_channel_skipped() {
    let handler = Arc::new(RecordingHandler::new());
    let channels: HashMap<String, Arc<dyn Channel>> = HashMap::new();

    let cancel = CancellationToken::new();
    let (tx, rx) = mpsc::channel(10);

    let handler_clone = handler.clone();
    let cancel_clone = cancel.clone();
    tokio::spawn(async move {
        atta_channel::dispatch::run_message_dispatch_loop(rx, channels, handler_clone, cancel_clone)
            .await;
    });

    // Send to a channel that doesn't exist in the map
    tx.send(make_message("ghost-ch", "hello")).await.unwrap();

    tokio::time::sleep(Duration::from_millis(100)).await;
    cancel.cancel();

    let msgs = handler.messages.lock().await;
    assert!(msgs.is_empty(), "unknown channel messages should be skipped");
}

#[tokio::test]
async fn test_dispatch_loop_cancel_stops() {
    let handler = Arc::new(RecordingHandler::new());
    let channels: HashMap<String, Arc<dyn Channel>> = HashMap::new();
    let cancel = CancellationToken::new();
    let (_tx, rx) = mpsc::channel::<ChannelMessage>(10);

    let cancel_clone = cancel.clone();
    let handle = tokio::spawn(async move {
        atta_channel::dispatch::run_message_dispatch_loop(rx, channels, handler, cancel_clone)
            .await;
    });

    cancel.cancel();
    let result = timeout(Duration::from_secs(2), handle).await;
    assert!(result.is_ok(), "dispatch loop should stop on cancel");
}

#[tokio::test]
async fn test_dispatch_loop_tx_dropped_stops() {
    let handler = Arc::new(RecordingHandler::new());
    let channels: HashMap<String, Arc<dyn Channel>> = HashMap::new();
    let cancel = CancellationToken::new();
    let (tx, rx) = mpsc::channel::<ChannelMessage>(10);

    let handle = tokio::spawn(async move {
        atta_channel::dispatch::run_message_dispatch_loop(rx, channels, handler, cancel).await;
    });

    // Drop the sender
    drop(tx);

    let result = timeout(Duration::from_secs(2), handle).await;
    assert!(result.is_ok(), "dispatch loop should stop when tx dropped");
}

#[tokio::test]
async fn test_dispatch_loop_multiple_channels() {
    let handler = Arc::new(RecordingHandler::new());
    let ch_a: Arc<dyn Channel> = Arc::new(MockChannel::new("ch-a"));
    let ch_b: Arc<dyn Channel> = Arc::new(MockChannel::new("ch-b"));

    let mut channels = HashMap::new();
    channels.insert("ch-a".to_string(), ch_a);
    channels.insert("ch-b".to_string(), ch_b);

    let cancel = CancellationToken::new();
    let (tx, rx) = mpsc::channel(10);

    let handler_clone = handler.clone();
    let cancel_clone = cancel.clone();
    tokio::spawn(async move {
        atta_channel::dispatch::run_message_dispatch_loop(rx, channels, handler_clone, cancel_clone)
            .await;
    });

    tx.send(make_message("ch-a", "from alpha")).await.unwrap();
    tx.send(make_message("ch-b", "from bravo")).await.unwrap();
    tx.send(make_message("ch-a", "another alpha"))
        .await
        .unwrap();

    tokio::time::sleep(Duration::from_millis(200)).await;
    cancel.cancel();

    let mut msgs = handler.messages.lock().await.clone();
    msgs.sort_by(|a, b| a.1.cmp(&b.1));

    assert_eq!(msgs.len(), 3);
    assert!(msgs.iter().any(|(ch, _)| ch == "ch-a"));
    assert!(msgs.iter().any(|(ch, _)| ch == "ch-b"));
}

// ===========================================================================
// send_with_retry tests
// ===========================================================================

#[tokio::test]
async fn test_send_with_retry_success_first_try() {
    let ch = MockChannel::new("ok");
    let msg = make_send_message("user", "hi");
    let result = atta_channel::dispatch::send_with_retry(&ch, msg, 3).await;
    assert!(result.is_ok());
    assert_eq!(ch.sent_messages().await.len(), 1);
}

#[tokio::test]
async fn test_send_with_retry_all_fail() {
    let ch = MockChannel::failing("broken");
    let msg = make_send_message("user", "hi");
    let result = atta_channel::dispatch::send_with_retry(&ch, msg, 2).await;
    assert!(result.is_err());
}

// ===========================================================================
// ChannelMessage / SendMessage type tests
// ===========================================================================

#[test]
fn test_channel_message_clone_preserves_fields() {
    let msg = ChannelMessage {
        id: "msg-1".to_string(),
        sender: "alice".to_string(),
        content: "hello".to_string(),
        channel: "slack".to_string(),
        reply_target: Some("parent-msg".to_string()),
        timestamp: Utc::now(),
        thread_ts: Some("ts-123".to_string()),
        metadata: json!({"key": "value"}),
        chat_type: ChatType::default(),
        bot_mentioned: false,
        group_id: None,
    };

    let cloned = msg.clone();
    assert_eq!(cloned.id, msg.id);
    assert_eq!(cloned.sender, msg.sender);
    assert_eq!(cloned.content, msg.content);
    assert_eq!(cloned.channel, msg.channel);
    assert_eq!(cloned.reply_target, msg.reply_target);
    assert_eq!(cloned.thread_ts, msg.thread_ts);
    assert_eq!(cloned.metadata, msg.metadata);
}

#[test]
fn test_channel_message_serde_roundtrip() {
    let msg = make_message("test-ch", "hello world");
    let json = serde_json::to_string(&msg).unwrap();
    let deserialized: ChannelMessage = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.content, "hello world");
    assert_eq!(deserialized.channel, "test-ch");
    assert_eq!(deserialized.sender, "test-user");
}

#[test]
fn test_send_message_serde_roundtrip() {
    let msg = SendMessage {
        recipient: "user-1".to_string(),
        content: "response".to_string(),
        subject: Some("Re: test".to_string()),
        thread_ts: Some("ts-456".to_string()),
        metadata: json!({"priority": "high"}),
    };
    let json = serde_json::to_string(&msg).unwrap();
    let deserialized: SendMessage = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.recipient, "user-1");
    assert_eq!(deserialized.content, "response");
    assert_eq!(deserialized.subject, Some("Re: test".to_string()));
    assert_eq!(deserialized.thread_ts, Some("ts-456".to_string()));
}

// ===========================================================================
// process_channel_message tests
// ===========================================================================

#[tokio::test]
async fn test_process_channel_message() {
    let handler = RecordingHandler::new();
    let ch = MockChannel::new("test-ch");
    let msg = make_message("test-ch", "ping");

    atta_channel::process_channel_message(&msg, &ch, &handler).await;

    let msgs = handler.messages.lock().await;
    assert_eq!(msgs.len(), 1);
    assert_eq!(msgs[0].0, "test-ch");
    assert_eq!(msgs[0].1, "ping");
}
