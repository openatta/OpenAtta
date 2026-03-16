//! Testcontainers-based integration tests for channels that need a real server.
//!
//! Run with:
//!   cargo test --test channel_testcontainers -p atta-channel --features mattermost
//!
//! Requires Docker to be running.
//!
//! These tests are marked `#[ignore]` by default because they pull and start
//! Docker images, which is slow and requires Docker. Run with `--ignored`:
//!   cargo test --test channel_testcontainers --features mattermost -- --ignored

use atta_channel::impls::mattermost::MattermostChannel;
use atta_channel::Channel;
use serde_json::json;
use testcontainers::{runners::AsyncRunner, GenericImage};

/// Spin up a Mattermost preview container and exercise the REST API.
///
/// The `mattermost/mattermost-preview` image bundles Postgres + Mattermost
/// in a single container and exposes port 8065.
#[tokio::test]
#[ignore = "requires Docker"]
async fn test_mattermost_docker_send_and_health() {
    // Start Mattermost preview container
    let container = GenericImage::new("mattermost/mattermost-preview", "latest")
        .with_exposed_port(8065.into())
        .with_wait_for(testcontainers::core::WaitFor::message_on_stdout(
            "Server is listening on",
        ))
        .start()
        .await
        .expect("Failed to start Mattermost container");

    let host_port = container
        .get_host_port_ipv4(8065)
        .await
        .expect("Failed to get mapped port");
    let base_url = format!("http://127.0.0.1:{}", host_port);

    let http = reqwest::Client::new();

    // Wait a bit for the server to fully initialize
    tokio::time::sleep(std::time::Duration::from_secs(5)).await;

    // --- Step 1: Create initial admin user ---
    let create_user_resp = http
        .post(format!("{}/api/v4/users", base_url))
        .json(&json!({
            "email": "admin@test.local",
            "username": "admin",
            "password": "Admin1234!",
        }))
        .send()
        .await
        .expect("create user request failed");
    assert!(
        create_user_resp.status().is_success(),
        "Failed to create admin user: {}",
        create_user_resp.text().await.unwrap_or_default()
    );

    // --- Step 2: Login to get a token ---
    let login_resp = http
        .post(format!("{}/api/v4/users/login", base_url))
        .json(&json!({
            "login_id": "admin",
            "password": "Admin1234!",
        }))
        .send()
        .await
        .expect("login request failed");
    assert!(
        login_resp.status().is_success(),
        "Login failed: {}",
        login_resp.status()
    );
    let token = login_resp
        .headers()
        .get("Token")
        .expect("no Token header in login response")
        .to_str()
        .unwrap()
        .to_string();

    // --- Step 3: Create a test team ---
    let team_resp = http
        .post(format!("{}/api/v4/teams", base_url))
        .header("Authorization", format!("Bearer {}", token))
        .json(&json!({
            "name": "testteam",
            "display_name": "Test Team",
            "type": "O",
        }))
        .send()
        .await
        .expect("create team request failed");
    let team: serde_json::Value = team_resp.json().await.unwrap();
    let team_id = team["id"].as_str().expect("missing team id");

    // --- Step 4: Create a test channel ---
    let channel_resp = http
        .post(format!("{}/api/v4/channels", base_url))
        .header("Authorization", format!("Bearer {}", token))
        .json(&json!({
            "team_id": team_id,
            "name": "testchannel",
            "display_name": "Test Channel",
            "type": "O",
        }))
        .send()
        .await
        .expect("create channel request failed");
    let channel: serde_json::Value = channel_resp.json().await.unwrap();
    let channel_id = channel["id"].as_str().expect("missing channel id");

    // --- Step 5: Test MattermostChannel ---
    let mm_channel = MattermostChannel::new(base_url.clone(), token.clone());

    // Health check should succeed
    let health = mm_channel.health_check().await;
    assert!(
        health.is_ok(),
        "Mattermost health_check failed: {:?}",
        health.err()
    );

    // Send a message
    let send_result = mm_channel
        .send(atta_channel::SendMessage {
            recipient: channel_id.to_string(),
            content: "Hello from testcontainers!".to_string(),
            subject: None,
            thread_ts: None,
            metadata: json!({}),
        })
        .await;
    assert!(
        send_result.is_ok(),
        "Mattermost send failed: {:?}",
        send_result.err()
    );

    // Verify the message was posted by fetching posts
    let posts_resp = http
        .get(format!("{}/api/v4/channels/{}/posts", base_url, channel_id))
        .header("Authorization", format!("Bearer {}", token))
        .send()
        .await
        .expect("get posts request failed");
    let posts: serde_json::Value = posts_resp.json().await.unwrap();
    let order = posts["order"].as_array().expect("missing order");
    assert!(!order.is_empty(), "No posts found after send");

    let first_post_id = order[0].as_str().unwrap();
    let first_post = &posts["posts"][first_post_id];
    assert_eq!(
        first_post["message"].as_str().unwrap(),
        "Hello from testcontainers!"
    );
}
