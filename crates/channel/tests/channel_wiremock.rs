//! Wiremock-based integration tests for channel HTTP contracts.
//!
//! Tests cover:
//!  - TG-CH1: Telegram Bot API contract (10 tests)
//!  - TG-CH2: Slack Web API contract (10 tests)
//!  - TG-CH3: Webhook HTTP contract (4 tests)
//!
//! Each test spins up an isolated mock HTTP server on a random port so all
//! tests can execute in parallel without interfering with each other.

use chrono::Utc;
use serde_json::json;
use wiremock::matchers::{body_partial_json, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use atta_channel::impls::discord::DiscordChannel;
use atta_channel::impls::dingtalk::DingtalkChannel;
use atta_channel::impls::lark::LarkChannel;
use atta_channel::impls::matrix::MatrixChannel;
use atta_channel::impls::mattermost::MattermostChannel;
use atta_channel::impls::nextcloud_talk::NextcloudTalkChannel;
use atta_channel::impls::signal::SignalChannel;
use atta_channel::impls::slack::SlackChannel;
use atta_channel::impls::telegram::TelegramChannel;
use atta_channel::impls::wati::WatiChannel;
use atta_channel::impls::webhook::WebhookChannel;
use atta_channel::impls::whatsapp::WhatsappChannel;
use atta_channel::{Channel, ChannelMessage, ChatType, SendMessage};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn telegram_channel(mock_url: &str) -> TelegramChannel {
    TelegramChannel::new("TEST_TOKEN".into()).with_api_base(mock_url.to_string())
}

fn slack_channel(mock_url: &str) -> SlackChannel {
    SlackChannel::new("xoxb-test".into(), "xapp-test".into()).with_api_base(mock_url.to_string())
}

fn send_msg(recipient: &str, content: &str) -> SendMessage {
    SendMessage {
        recipient: recipient.to_string(),
        content: content.to_string(),
        subject: None,
        thread_ts: None,
        metadata: json!({}),
    }
}

fn send_msg_with_thread(recipient: &str, content: &str, thread_ts: &str) -> SendMessage {
    SendMessage {
        recipient: recipient.to_string(),
        content: content.to_string(),
        subject: None,
        thread_ts: Some(thread_ts.to_string()),
        metadata: json!({}),
    }
}

// ===========================================================================
// TG-CH1: Telegram Bot API contract
// ===========================================================================

/// Test 1: Successful sendMessage returns Ok(())
#[tokio::test]
async fn test_telegram_send_message_ok() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/botTEST_TOKEN/sendMessage"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "ok": true,
            "result": {
                "message_id": 1,
                "chat": { "id": 123 },
                "text": "hello"
            }
        })))
        .mount(&server)
        .await;

    let ch = telegram_channel(&server.uri());
    let result = ch.send(send_msg("123", "hello")).await;
    assert!(result.is_ok(), "send() should succeed on HTTP 200: {result:?}");
}

/// Test 2: HTTP 500 from Telegram causes send() to return an error
#[tokio::test]
async fn test_telegram_send_message_http_error() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/botTEST_TOKEN/sendMessage"))
        .respond_with(ResponseTemplate::new(500).set_body_string("Internal Server Error"))
        .mount(&server)
        .await;

    let ch = telegram_channel(&server.uri());
    let result = ch.send(send_msg("123", "hello")).await;
    assert!(
        result.is_err(),
        "send() should return error on HTTP 500"
    );
}

/// Test 3: health_check() succeeds when getMe returns 200
#[tokio::test]
async fn test_telegram_health_check_ok() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/botTEST_TOKEN/getMe"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "ok": true,
            "result": { "id": 42, "is_bot": true, "first_name": "TestBot" }
        })))
        .mount(&server)
        .await;

    let ch = telegram_channel(&server.uri());
    let result = ch.health_check().await;
    assert!(result.is_ok(), "health_check() should succeed on HTTP 200: {result:?}");
}

/// Test 4: health_check() returns error when getMe returns 401
#[tokio::test]
async fn test_telegram_health_check_failure() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/botTEST_TOKEN/getMe"))
        .respond_with(ResponseTemplate::new(401).set_body_json(json!({
            "ok": false,
            "error_code": 401,
            "description": "Unauthorized"
        })))
        .mount(&server)
        .await;

    let ch = telegram_channel(&server.uri());
    let result = ch.health_check().await;
    assert!(result.is_err(), "health_check() should fail on HTTP 401");
}

/// Test 5: start_typing() sends POST to sendChatAction with correct body
#[tokio::test]
async fn test_telegram_start_typing() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/botTEST_TOKEN/sendChatAction"))
        .and(body_partial_json(json!({
            "chat_id": "123",
            "action": "typing"
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "ok": true })))
        .expect(1)
        .mount(&server)
        .await;

    let ch = telegram_channel(&server.uri());
    let result = ch.start_typing("123").await;
    assert!(result.is_ok(), "start_typing() should succeed: {result:?}");
}

/// Test 6: send_draft() returns draft_id as "{chat_id}:{message_id}"
#[tokio::test]
async fn test_telegram_send_draft_returns_draft_id() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/botTEST_TOKEN/sendMessage"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "ok": true,
            "result": {
                "message_id": 42,
                "chat": { "id": 999 },
                "text": "draft"
            }
        })))
        .mount(&server)
        .await;

    let ch = telegram_channel(&server.uri());
    let draft_id = ch.send_draft(send_msg("999", "draft")).await;
    assert!(draft_id.is_ok(), "send_draft() should succeed: {draft_id:?}");
    assert_eq!(
        draft_id.unwrap(),
        "999:42",
        "draft_id should be 'chat_id:message_id'"
    );
}

/// Test 7: update_draft() sends POST to editMessageText with correct fields
#[tokio::test]
async fn test_telegram_update_draft() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/botTEST_TOKEN/editMessageText"))
        .and(body_partial_json(json!({
            "chat_id": "123",
            "message_id": 456,
            "text": "updated text"
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "ok": true })))
        .expect(1)
        .mount(&server)
        .await;

    let ch = telegram_channel(&server.uri());
    let result = ch.update_draft("123:456", "updated text").await;
    assert!(result.is_ok(), "update_draft() should succeed: {result:?}");
}

/// Test 8: add_reaction() sends POST to setMessageReaction with correct format
#[tokio::test]
async fn test_telegram_add_reaction_format() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/botTEST_TOKEN/setMessageReaction"))
        .and(body_partial_json(json!({
            "chat_id": "123",
            "message_id": 456
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "ok": true })))
        .expect(1)
        .mount(&server)
        .await;

    let ch = telegram_channel(&server.uri());
    let result = ch.add_reaction("123:456", "\u{1F44D}").await;
    assert!(result.is_ok(), "add_reaction() should succeed: {result:?}");
}

/// Test 9: add_reaction() with bad format (no colon) returns Validation error
#[tokio::test]
async fn test_telegram_add_reaction_bad_format() {
    let server = MockServer::start().await;
    // No mock mounted — should fail before making any HTTP request

    let ch = telegram_channel(&server.uri());
    let result = ch.add_reaction("no-colon", "\u{1F44D}").await;
    assert!(
        result.is_err(),
        "add_reaction() should fail on bad message_id format"
    );

    // Verify it's a Validation error by checking the error message
    let err_str = format!("{:?}", result.unwrap_err());
    assert!(
        err_str.contains("Validation")
            || err_str.contains("chat_id:message_id")
            || err_str.contains("format"),
        "error should indicate invalid format, got: {err_str}"
    );
}

/// Test 10: update_draft() with content >4096 bytes sends truncated text to Telegram
#[tokio::test]
async fn test_telegram_update_draft_truncates() {
    let server = MockServer::start().await;

    // Use a closure matcher to inspect the text field length after parsing the body
    let long_content = "A".repeat(5000); // well above the 4096-byte limit

    Mock::given(method("POST"))
        .and(path("/botTEST_TOKEN/editMessageText"))
        .and(|req: &wiremock::Request| {
            if let Ok(body) = serde_json::from_slice::<serde_json::Value>(&req.body) {
                if let Some(text) = body.get("text").and_then(|v| v.as_str()) {
                    return text.len() <= 4096;
                }
            }
            false
        })
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "ok": true })))
        .expect(1)
        .mount(&server)
        .await;

    let ch = telegram_channel(&server.uri());
    let result = ch.update_draft("123:456", &long_content).await;
    assert!(
        result.is_ok(),
        "update_draft() should succeed with truncation: {result:?}"
    );
}

// ===========================================================================
// TG-CH2: Slack Web API contract
// ===========================================================================

/// Test 11: send() posts to chat.postMessage with Bearer auth and returns Ok
#[tokio::test]
async fn test_slack_send_message_ok() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/chat.postMessage"))
        .and(header("Authorization", "Bearer xoxb-test"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "ok": true,
            "ts": "1234.5678",
            "channel": "C123"
        })))
        .mount(&server)
        .await;

    let ch = slack_channel(&server.uri());
    let result = ch.send(send_msg("C123", "hello world")).await;
    assert!(result.is_ok(), "Slack send() should succeed: {result:?}");
}

/// Test 12: send() with thread_ts includes thread_ts field in request body
#[tokio::test]
async fn test_slack_send_message_thread() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/chat.postMessage"))
        .and(body_partial_json(json!({
            "channel": "C123",
            "text": "threaded reply",
            "thread_ts": "1234.5678"
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "ok": true,
            "ts": "9999.0001",
            "channel": "C123"
        })))
        .expect(1)
        .mount(&server)
        .await;

    let ch = slack_channel(&server.uri());
    let result = ch
        .send(send_msg_with_thread("C123", "threaded reply", "1234.5678"))
        .await;
    assert!(result.is_ok(), "Slack threaded send() should succeed: {result:?}");
}

/// Test 13: health_check() posts to auth.test and returns Ok on success
#[tokio::test]
async fn test_slack_health_check_ok() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/auth.test"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "ok": true })))
        .expect(1)
        .mount(&server)
        .await;

    let ch = slack_channel(&server.uri());
    let result = ch.health_check().await;
    assert!(result.is_ok(), "Slack health_check() should succeed: {result:?}");
}

/// Test 14: health_check() returns error when API returns ok=false
#[tokio::test]
async fn test_slack_health_check_api_error() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/auth.test"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "ok": false,
            "error": "invalid_auth"
        })))
        .mount(&server)
        .await;

    let ch = slack_channel(&server.uri());
    let result = ch.health_check().await;
    assert!(result.is_err(), "Slack health_check() should fail on ok=false");

    let err_str = format!("{:?}", result.unwrap_err());
    assert!(
        err_str.contains("invalid_auth") || err_str.contains("error"),
        "error should mention the API error, got: {err_str}"
    );
}

/// Test 15: add_reaction() posts to reactions.add with correct channel:ts split
#[tokio::test]
async fn test_slack_add_reaction_ok() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/reactions.add"))
        .and(body_partial_json(json!({
            "channel": "C123",
            "timestamp": "1234.5678",
            "name": "thumbsup"
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "ok": true })))
        .expect(1)
        .mount(&server)
        .await;

    let ch = slack_channel(&server.uri());
    let result = ch.add_reaction("C123:1234.5678", "thumbsup").await;
    assert!(result.is_ok(), "Slack add_reaction() should succeed: {result:?}");
}

/// Test 16: remove_reaction() posts to reactions.remove with correct fields
#[tokio::test]
async fn test_slack_remove_reaction_ok() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/reactions.remove"))
        .and(body_partial_json(json!({
            "channel": "C123",
            "timestamp": "1234.5678",
            "name": "thumbsup"
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "ok": true })))
        .expect(1)
        .mount(&server)
        .await;

    let ch = slack_channel(&server.uri());
    let result = ch.remove_reaction("C123:1234.5678", "thumbsup").await;
    assert!(result.is_ok(), "Slack remove_reaction() should succeed: {result:?}");
}

/// Test 17: add_reaction() with bad format (no colon) returns Validation error
#[tokio::test]
async fn test_slack_reaction_bad_format() {
    let server = MockServer::start().await;
    // No mock needed — should fail before any HTTP call

    let ch = slack_channel(&server.uri());
    let result = ch.add_reaction("no-colon", "thumbsup").await;
    assert!(
        result.is_err(),
        "Slack add_reaction() should fail on bad message_id format"
    );

    let err_str = format!("{:?}", result.unwrap_err());
    assert!(
        err_str.contains("Validation")
            || err_str.contains("channel:ts")
            || err_str.contains("format"),
        "error should indicate invalid format, got: {err_str}"
    );
}

/// Test 18: send_draft() returns "channel:ts" as draft ID
#[tokio::test]
async fn test_slack_send_draft_returns_channel_ts() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/chat.postMessage"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "ok": true,
            "ts": "1234.5678",
            "channel": "C123"
        })))
        .mount(&server)
        .await;

    let ch = slack_channel(&server.uri());
    let draft_id = ch.send_draft(send_msg("C123", "draft content")).await;
    assert!(draft_id.is_ok(), "Slack send_draft() should succeed: {draft_id:?}");
    assert_eq!(
        draft_id.unwrap(),
        "C123:1234.5678",
        "draft_id should be 'channel:ts'"
    );
}

/// Test 19: update_draft() posts to chat.update with channel, ts, and text
#[tokio::test]
async fn test_slack_update_draft_calls_chat_update() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/chat.update"))
        .and(body_partial_json(json!({
            "channel": "C123",
            "ts": "1234.5678",
            "text": "new content"
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "ok": true })))
        .expect(1)
        .mount(&server)
        .await;

    let ch = slack_channel(&server.uri());
    let result = ch.update_draft("C123:1234.5678", "new content").await;
    assert!(result.is_ok(), "Slack update_draft() should succeed: {result:?}");
}

/// Test 20: send_approval_prompt() posts to chat.postMessage with a blocks array
#[tokio::test]
async fn test_slack_send_approval_prompt() {
    let server = MockServer::start().await;

    // Use a closure matcher to verify the request includes a non-empty "blocks" array
    Mock::given(method("POST"))
        .and(path("/chat.postMessage"))
        .and(|req: &wiremock::Request| {
            if let Ok(body) = serde_json::from_slice::<serde_json::Value>(&req.body) {
                body.get("blocks")
                    .and_then(|v| v.as_array())
                    .map(|arr| !arr.is_empty())
                    .unwrap_or(false)
            } else {
                false
            }
        })
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "ok": true })))
        .expect(1)
        .mount(&server)
        .await;

    let ch = slack_channel(&server.uri());
    let args = json!({ "command": "ls", "path": "/tmp" });
    let result = ch
        .send_approval_prompt("C123", "req-001", "shell", &args, None)
        .await;
    assert!(
        result.is_ok(),
        "Slack send_approval_prompt() should succeed: {result:?}"
    );
}

// ===========================================================================
// TG-CH3: Webhook HTTP contract
// ===========================================================================

/// Test 21: send() posts JSON body to the configured URL and returns Ok
#[tokio::test]
async fn test_webhook_send_posts_json() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/webhook"))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&server)
        .await;

    let url = format!("{}/webhook", server.uri());
    let ch = WebhookChannel::new("test-webhook".into(), url);
    let result = ch.send(send_msg("user-1", "test payload")).await;
    assert!(result.is_ok(), "Webhook send() should succeed: {result:?}");
}

/// Test 22: send() returns error when server returns HTTP 500
#[tokio::test]
async fn test_webhook_send_http_error() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/webhook"))
        .respond_with(ResponseTemplate::new(500).set_body_string("Internal Server Error"))
        .mount(&server)
        .await;

    let url = format!("{}/webhook", server.uri());
    let ch = WebhookChannel::new("test-webhook".into(), url);
    let result = ch.send(send_msg("user-1", "test payload")).await;
    assert!(result.is_err(), "Webhook send() should fail on HTTP 500");
}

/// Test 23: health_check() sends HEAD request to the configured URL
#[tokio::test]
async fn test_webhook_health_check() {
    let server = MockServer::start().await;

    Mock::given(method("HEAD"))
        .and(path("/webhook"))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&server)
        .await;

    let url = format!("{}/webhook", server.uri());
    let ch = WebhookChannel::new("test-webhook".into(), url);
    let result = ch.health_check().await;
    assert!(result.is_ok(), "Webhook health_check() should succeed: {result:?}");
}

/// Test 24: push_incoming() delivers a message to the listen() receiver in-process
#[tokio::test]
async fn test_webhook_push_incoming_delivers() {
    let server = MockServer::start().await;
    // No HTTP mocking needed — this test exercises the in-process push model

    let url = format!("{}/webhook", server.uri());
    let ch = std::sync::Arc::new(WebhookChannel::new("test-webhook".into(), url));

    // Create the mpsc channel that listen() will store internally
    let (tx, mut rx) = tokio::sync::mpsc::channel(10);

    // Spawn listen() — it stores tx internally and then blocks forever
    let ch_listen = ch.clone();
    tokio::spawn(async move {
        let _ = ch_listen.listen(tx).await;
    });

    // Give listen() time to store the sender before push_incoming is called
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;

    // Push an incoming message as if the HTTP handler received it
    let incoming = ChannelMessage {
        id: "msg-1".to_string(),
        sender: "external-user".to_string(),
        content: "hello from webhook".to_string(),
        channel: "test-webhook".to_string(),
        reply_target: None,
        timestamp: Utc::now(),
        thread_ts: None,
        metadata: json!({}),
        chat_type: ChatType::default(),
        bot_mentioned: false,
        group_id: None,
    };

    let push_result = ch.push_incoming(incoming).await;
    assert!(push_result.is_ok(), "push_incoming() should succeed: {push_result:?}");

    // Verify the message arrives on the receiver within 2 seconds
    let received =
        tokio::time::timeout(std::time::Duration::from_secs(2), rx.recv()).await;

    assert!(
        received.is_ok(),
        "should receive message within 2 seconds (timed out)"
    );
    let msg = received.unwrap().expect("channel should not be closed");
    assert_eq!(msg.content, "hello from webhook");
    assert_eq!(msg.sender, "external-user");
    assert_eq!(msg.channel, "test-webhook");
}

// ===========================================================================
// TG-CH4: Discord REST API contract
// ===========================================================================

fn discord_channel(mock_url: &str) -> DiscordChannel {
    DiscordChannel::new("test-bot-token".into()).with_api_base(mock_url.to_string())
}

#[tokio::test]
async fn test_discord_send_message_ok() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/channels/C001/messages"))
        .and(header("Authorization", "Bot test-bot-token"))
        .and(body_partial_json(json!({ "content": "hello" })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "msg-1", "channel_id": "C001", "content": "hello"
        })))
        .expect(1)
        .mount(&server)
        .await;

    let ch = discord_channel(&server.uri());
    let result = ch.send(send_msg("C001", "hello")).await;
    assert!(result.is_ok(), "Discord send() should succeed: {result:?}");
}

#[tokio::test]
async fn test_discord_health_check_ok() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/users/@me"))
        .and(header("Authorization", "Bot test-bot-token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "123", "username": "testbot"
        })))
        .expect(1)
        .mount(&server)
        .await;

    let ch = discord_channel(&server.uri());
    assert!(ch.health_check().await.is_ok());
}

#[tokio::test]
async fn test_discord_health_check_failure() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/users/@me"))
        .respond_with(ResponseTemplate::new(401))
        .mount(&server)
        .await;

    let ch = discord_channel(&server.uri());
    assert!(ch.health_check().await.is_err());
}

#[tokio::test]
async fn test_discord_send_draft_returns_id() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/channels/C001/messages"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "99", "channel_id": "C001"
        })))
        .mount(&server)
        .await;

    let ch = discord_channel(&server.uri());
    let draft = ch.send_draft(send_msg("C001", "draft")).await;
    assert!(draft.is_ok());
    assert_eq!(draft.unwrap(), "C001:99");
}

// ===========================================================================
// TG-CH5: DingTalk API contract
// ===========================================================================

fn dingtalk_channel(mock_url: &str) -> DingtalkChannel {
    DingtalkChannel::new("test-key".into(), "test-secret".into(), None)
        .with_api_base(mock_url.to_string())
}

#[tokio::test]
async fn test_dingtalk_health_check_ok() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/gettoken"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "errcode": 0,
            "access_token": "test-token-123",
            "expires_in": 7200
        })))
        .expect(1)
        .mount(&server)
        .await;

    let ch = dingtalk_channel(&server.uri());
    assert!(ch.health_check().await.is_ok());
}

#[tokio::test]
async fn test_dingtalk_health_check_bad_creds() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/gettoken"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "errcode": 40014,
            "errmsg": "invalid appkey"
        })))
        .mount(&server)
        .await;

    let ch = dingtalk_channel(&server.uri());
    assert!(ch.health_check().await.is_err());
}

#[tokio::test]
async fn test_dingtalk_send_via_api() {
    let server = MockServer::start().await;

    // Token endpoint
    Mock::given(method("GET"))
        .and(path("/gettoken"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "errcode": 0, "access_token": "tok-abc", "expires_in": 7200
        })))
        .mount(&server)
        .await;

    // Send endpoint
    Mock::given(method("POST"))
        .and(path("/v1.0/robot/oToMessages/batchSend"))
        .and(header("x-acs-dingtalk-access-token", "tok-abc"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"processQueryKey": "1"})))
        .expect(1)
        .mount(&server)
        .await;

    let ch = dingtalk_channel(&server.uri());
    let result = ch.send(send_msg("user-1", "hello dingtalk")).await;
    assert!(result.is_ok(), "DingTalk send() should succeed: {result:?}");
}

// ===========================================================================
// TG-CH6: Lark (Feishu) API contract
// ===========================================================================

fn lark_channel(mock_url: &str) -> LarkChannel {
    LarkChannel::new("test-app-id".into(), "test-app-secret".into())
        .with_api_base(mock_url.to_string())
}

#[tokio::test]
async fn test_lark_health_check_ok() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/auth/v3/tenant_access_token/internal"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "code": 0,
            "tenant_access_token": "t-test-token",
            "expire": 7200
        })))
        .expect(1)
        .mount(&server)
        .await;

    let ch = lark_channel(&server.uri());
    assert!(ch.health_check().await.is_ok());
}

#[tokio::test]
async fn test_lark_health_check_bad_creds() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/auth/v3/tenant_access_token/internal"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "code": 10003,
            "msg": "invalid app_id"
        })))
        .mount(&server)
        .await;

    let ch = lark_channel(&server.uri());
    assert!(ch.health_check().await.is_err());
}

#[tokio::test]
async fn test_lark_send_message_ok() {
    let server = MockServer::start().await;

    // Token
    Mock::given(method("POST"))
        .and(path("/auth/v3/tenant_access_token/internal"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "code": 0, "tenant_access_token": "t-abc", "expire": 7200
        })))
        .mount(&server)
        .await;

    // Send
    Mock::given(method("POST"))
        .and(path("/im/v1/messages"))
        .and(header("Authorization", "Bearer t-abc"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "code": 0, "data": { "message_id": "om_xxx" }
        })))
        .expect(1)
        .mount(&server)
        .await;

    let ch = lark_channel(&server.uri());
    let result = ch.send(send_msg("ou_user123", "hello lark")).await;
    assert!(result.is_ok(), "Lark send() should succeed: {result:?}");
}

// ===========================================================================
// TG-CH7: WhatsApp Cloud API contract
// ===========================================================================

fn whatsapp_channel(mock_url: &str) -> WhatsappChannel {
    WhatsappChannel::new("test-access-token".into(), "1234567890".into())
        .with_api_base(mock_url.to_string())
}

#[tokio::test]
async fn test_whatsapp_send_message_ok() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/1234567890/messages"))
        .and(header("Authorization", "Bearer test-access-token"))
        .and(body_partial_json(json!({
            "messaging_product": "whatsapp",
            "to": "+15551234567",
            "type": "text"
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "messages": [{"id": "wamid.abc"}]
        })))
        .expect(1)
        .mount(&server)
        .await;

    let ch = whatsapp_channel(&server.uri());
    let result = ch.send(send_msg("+15551234567", "hello whatsapp")).await;
    assert!(result.is_ok(), "WhatsApp send() should succeed: {result:?}");
}

#[tokio::test]
async fn test_whatsapp_health_check_ok() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/1234567890"))
        .and(header("Authorization", "Bearer test-access-token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "1234567890", "display_phone_number": "+1555"
        })))
        .expect(1)
        .mount(&server)
        .await;

    let ch = whatsapp_channel(&server.uri());
    assert!(ch.health_check().await.is_ok());
}

#[tokio::test]
async fn test_whatsapp_health_check_failure() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/1234567890"))
        .respond_with(ResponseTemplate::new(401))
        .mount(&server)
        .await;

    let ch = whatsapp_channel(&server.uri());
    assert!(ch.health_check().await.is_err());
}

// ===========================================================================
// TG-CH8: Mattermost REST API contract
// ===========================================================================

fn mattermost_channel(mock_url: &str) -> MattermostChannel {
    MattermostChannel::new(mock_url.to_string(), "test-mm-token".into())
}

#[tokio::test]
async fn test_mattermost_send_message_ok() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/api/v4/posts"))
        .and(header("Authorization", "Bearer test-mm-token"))
        .and(body_partial_json(json!({
            "channel_id": "ch-001",
            "message": "hello mattermost"
        })))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({
            "id": "post-1", "channel_id": "ch-001"
        })))
        .expect(1)
        .mount(&server)
        .await;

    let ch = mattermost_channel(&server.uri());
    let result = ch.send(send_msg("ch-001", "hello mattermost")).await;
    assert!(result.is_ok(), "Mattermost send() should succeed: {result:?}");
}

#[tokio::test]
async fn test_mattermost_health_check_ok() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/v4/users/me"))
        .and(header("Authorization", "Bearer test-mm-token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "bot-user-id", "username": "testbot"
        })))
        .expect(1)
        .mount(&server)
        .await;

    let ch = mattermost_channel(&server.uri());
    assert!(ch.health_check().await.is_ok());
}

#[tokio::test]
async fn test_mattermost_health_check_failure() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/v4/users/me"))
        .respond_with(ResponseTemplate::new(401))
        .mount(&server)
        .await;

    let ch = mattermost_channel(&server.uri());
    assert!(ch.health_check().await.is_err());
}

// ===========================================================================
// TG-CH9: Matrix Client-Server API contract
// ===========================================================================

fn matrix_channel(mock_url: &str) -> MatrixChannel {
    MatrixChannel::new(
        mock_url.to_string(),
        "test-mx-token".into(),
        "@bot:test.local".into(),
    )
}

#[tokio::test]
async fn test_matrix_health_check_ok() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/_matrix/client/v3/account/whoami"))
        .and(header("Authorization", "Bearer test-mx-token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "user_id": "@bot:test.local"
        })))
        .expect(1)
        .mount(&server)
        .await;

    let ch = matrix_channel(&server.uri());
    assert!(ch.health_check().await.is_ok());
}

#[tokio::test]
async fn test_matrix_health_check_failure() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/_matrix/client/v3/account/whoami"))
        .respond_with(ResponseTemplate::new(401))
        .mount(&server)
        .await;

    let ch = matrix_channel(&server.uri());
    assert!(ch.health_check().await.is_err());
}

#[tokio::test]
async fn test_matrix_send_message_ok() {
    let server = MockServer::start().await;

    // Matrix send uses PUT with a transaction ID (dynamic path)
    Mock::given(method("PUT"))
        .and(header("Authorization", "Bearer test-mx-token"))
        .and(body_partial_json(json!({
            "msgtype": "m.text",
            "body": "hello matrix"
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "event_id": "$evt-1"
        })))
        .expect(1)
        .mount(&server)
        .await;

    let ch = matrix_channel(&server.uri());
    let result = ch.send(send_msg("!room:test.local", "hello matrix")).await;
    assert!(result.is_ok(), "Matrix send() should succeed: {result:?}");
}

// ===========================================================================
// TG-CH10: Signal REST API contract
// ===========================================================================

fn signal_channel(mock_url: &str) -> SignalChannel {
    SignalChannel::new("+15551234567".into()).with_api_url(mock_url.to_string())
}

#[tokio::test]
async fn test_signal_send_message_ok() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/v2/send"))
        .and(body_partial_json(json!({
            "message": "hello signal",
            "number": "+15551234567",
            "recipients": ["+15559876543"]
        })))
        .respond_with(ResponseTemplate::new(201))
        .expect(1)
        .mount(&server)
        .await;

    let ch = signal_channel(&server.uri());
    let result = ch.send(send_msg("+15559876543", "hello signal")).await;
    assert!(result.is_ok(), "Signal send() should succeed: {result:?}");
}

#[tokio::test]
async fn test_signal_health_check_ok() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/v1/about"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "versions": ["v1", "v2"]
        })))
        .expect(1)
        .mount(&server)
        .await;

    let ch = signal_channel(&server.uri());
    assert!(ch.health_check().await.is_ok());
}

#[tokio::test]
async fn test_signal_health_check_failure() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/v1/about"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;

    let ch = signal_channel(&server.uri());
    assert!(ch.health_check().await.is_err());
}

// ===========================================================================
// TG-CH11: WATI API contract
// ===========================================================================

fn wati_channel(mock_url: &str) -> WatiChannel {
    WatiChannel::new(mock_url.to_string(), "test-wati-token".into())
}

#[tokio::test]
async fn test_wati_send_message_ok() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/api/v1/sendSessionMessage/15551234567"))
        .and(header("Authorization", "Bearer test-wati-token"))
        .and(body_partial_json(json!({ "messageText": "hello wati" })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "result": true, "info": "sent"
        })))
        .expect(1)
        .mount(&server)
        .await;

    let ch = wati_channel(&server.uri());
    let result = ch.send(send_msg("15551234567", "hello wati")).await;
    assert!(result.is_ok(), "WATI send() should succeed: {result:?}");
}

#[tokio::test]
async fn test_wati_health_check_ok() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/v1/getContacts"))
        .and(header("Authorization", "Bearer test-wati-token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "result": true, "contacts": []
        })))
        .expect(1)
        .mount(&server)
        .await;

    let ch = wati_channel(&server.uri());
    assert!(ch.health_check().await.is_ok());
}

#[tokio::test]
async fn test_wati_send_failure() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/api/v1/sendSessionMessage/123"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "result": false, "info": "session expired"
        })))
        .mount(&server)
        .await;

    let ch = wati_channel(&server.uri());
    let result = ch.send(send_msg("123", "test")).await;
    assert!(result.is_err(), "WATI send() should fail when result=false");
}

// ===========================================================================
// TG-CH12: Nextcloud Talk OCS API contract
// ===========================================================================

fn nextcloud_channel(mock_url: &str) -> NextcloudTalkChannel {
    NextcloudTalkChannel::new(
        mock_url.to_string(),
        "testuser".into(),
        "testpass".into(),
        "room-token-1".into(),
    )
}

#[tokio::test]
async fn test_nextcloud_send_message_ok() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/ocs/v2.php/apps/spreed/api/v1/chat/room-token-1"))
        .and(header("OCS-APIRequest", "true"))
        .and(body_partial_json(json!({ "message": "hello nextcloud" })))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({
            "ocs": { "data": { "id": 1 } }
        })))
        .expect(1)
        .mount(&server)
        .await;

    let ch = nextcloud_channel(&server.uri());
    let result = ch.send(send_msg("room-token-1", "hello nextcloud")).await;
    assert!(result.is_ok(), "Nextcloud send() should succeed: {result:?}");
}

#[tokio::test]
async fn test_nextcloud_health_check_ok() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/ocs/v2.php/apps/spreed/api/v1/room/room-token-1"))
        .and(header("OCS-APIRequest", "true"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "ocs": { "data": { "token": "room-token-1" } }
        })))
        .expect(1)
        .mount(&server)
        .await;

    let ch = nextcloud_channel(&server.uri());
    assert!(ch.health_check().await.is_ok());
}

#[tokio::test]
async fn test_nextcloud_health_check_failure() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/ocs/v2.php/apps/spreed/api/v1/room/room-token-1"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;

    let ch = nextcloud_channel(&server.uri());
    assert!(ch.health_check().await.is_err());
}
