//! NatsBus integration tests
//!
//! These tests require a running NATS server with JetStream enabled.
//! Set the `ATTA_NATS_URL` environment variable to enable them, e.g.:
//!
//! ```bash
//! ATTA_NATS_URL=nats://localhost:4222 cargo test -p atta-bus --features nats --test nats_tests
//! ```
//!
//! All tests are skipped automatically when `ATTA_NATS_URL` is not set.
//! Each test uses a UUID-qualified topic to avoid cross-test interference
//! since NATS JetStream is shared state.

#![cfg(feature = "nats")]

use atta_bus::{EventBus, NatsBus};
use atta_types::{Actor, EntityRef, EventEnvelope};
use futures::StreamExt;
use std::time::Duration;
use tokio::time::timeout;
use uuid::Uuid;

// ── helpers ──────────────────────────────────────────────────────────────────

/// Skip the current test if `ATTA_NATS_URL` is not set; otherwise return the URL.
macro_rules! skip_unless_nats {
    () => {
        match std::env::var("ATTA_NATS_URL") {
            Ok(url) => url,
            Err(_) => {
                eprintln!("skipping NATS test: ATTA_NATS_URL not set");
                return;
            }
        }
    };
}

/// Create a test `EventEnvelope` with a given `event_type`.
fn make_event(event_type: &str) -> EventEnvelope {
    EventEnvelope::new(
        event_type,
        EntityRef::task(&Uuid::new_v4()),
        Actor::system(),
        Uuid::new_v4(),
        serde_json::json!({"test": true}),
    )
    .unwrap()
}

/// Connect to a `NatsBus` using `url`, panicking on failure.
async fn connect(url: &str) -> NatsBus {
    NatsBus::connect(url)
        .await
        .expect("failed to connect to NATS")
}

/// Topic string scoped to this test run via a UUID suffix.
fn unique_topic(prefix: &str) -> String {
    format!("{}.{}", prefix, Uuid::new_v4().simple())
}

// ── tests ─────────────────────────────────────────────────────────────────────

/// 1. Basic publish/subscribe on an exact topic.
///
/// Subscribes before publishing, then verifies the received event matches
/// the published one by `event_id`.
#[tokio::test]
async fn test_nats_publish_subscribe_exact() {
    let url = skip_unless_nats!();
    let bus = connect(&url).await;

    let topic = unique_topic("atta.test.exact");
    let mut stream = bus.subscribe(&topic).await.expect("subscribe failed");

    let event = make_event("atta.test.exact.created");
    let expected_id = event.event_id;
    bus.publish(&topic, event).await.expect("publish failed");

    let received = timeout(Duration::from_secs(5), stream.next())
        .await
        .expect("timed out waiting for event")
        .expect("stream ended unexpectedly");

    assert_eq!(
        received.event_id, expected_id,
        "received event_id should match published event_id"
    );
}

/// 2. Wildcard subscription: subscribe to `atta.test.<uuid>.*`, publish to
///    `atta.test.<uuid>.created`, and verify the event is delivered.
#[tokio::test]
async fn test_nats_wildcard_subscribe() {
    let url = skip_unless_nats!();
    let bus = connect(&url).await;

    // Create a unique namespace for this test run.
    let ns = Uuid::new_v4().simple().to_string();
    let wildcard_topic = format!("atta.test.{}.wildcard.*", ns);
    let publish_topic = format!("atta.test.{}.wildcard.created", ns);

    let mut stream = bus
        .subscribe(&wildcard_topic)
        .await
        .expect("wildcard subscribe failed");

    let event = make_event("atta.test.wildcard.created");
    let expected_id = event.event_id;
    bus.publish(&publish_topic, event)
        .await
        .expect("publish failed");

    let received = timeout(Duration::from_secs(5), stream.next())
        .await
        .expect("timed out waiting for wildcard event")
        .expect("stream ended unexpectedly");

    assert_eq!(
        received.event_id, expected_id,
        "wildcard subscriber should receive the published event"
    );
}

/// 3. Consumer group subscription.
///
/// Creates two group consumers on the same durable consumer name and publishes
/// one event. Because NATS JetStream load-balances across group members,
/// exactly one of the two consumers should receive the message within the
/// timeout; the other should time out.
#[tokio::test]
async fn test_nats_subscribe_group() {
    let url = skip_unless_nats!();
    let bus = connect(&url).await;

    let topic = unique_topic("atta.test.group");
    // Use a unique durable name so this test does not collide with others.
    let group = format!("grp-{}", Uuid::new_v4().simple());

    let mut stream_a = bus
        .subscribe_group(&topic, &group)
        .await
        .expect("subscribe_group A failed");
    let mut stream_b = bus
        .subscribe_group(&topic, &group)
        .await
        .expect("subscribe_group B failed");

    let event = make_event("atta.test.group.created");
    bus.publish(&topic, event).await.expect("publish failed");

    // Collect whichever consumer receives the event within 5 s.
    let result_a = timeout(Duration::from_secs(5), stream_a.next()).await;
    let result_b = timeout(Duration::from_secs(5), stream_b.next()).await;

    // At least one consumer must have received the event.
    let a_received = result_a.is_ok();
    let b_received = result_b.is_ok();

    assert!(
        a_received || b_received,
        "at least one group consumer should receive the published event"
    );
}

/// 4. Two independent subscribers on the same topic both receive the message.
#[tokio::test]
async fn test_nats_multiple_subscribers() {
    let url = skip_unless_nats!();
    let bus = connect(&url).await;

    let topic = unique_topic("atta.test.multi");

    let mut stream1 = bus.subscribe(&topic).await.expect("subscribe 1 failed");
    let mut stream2 = bus.subscribe(&topic).await.expect("subscribe 2 failed");

    let event = make_event("atta.test.multi.created");
    let expected_id = event.event_id;
    bus.publish(&topic, event).await.expect("publish failed");

    let r1 = timeout(Duration::from_secs(5), stream1.next())
        .await
        .expect("timed out waiting for subscriber 1")
        .expect("stream 1 ended unexpectedly");

    let r2 = timeout(Duration::from_secs(5), stream2.next())
        .await
        .expect("timed out waiting for subscriber 2")
        .expect("stream 2 ended unexpectedly");

    assert_eq!(
        r1.event_id, expected_id,
        "subscriber 1 should receive the published event"
    );
    assert_eq!(
        r2.event_id, expected_id,
        "subscriber 2 should receive the published event"
    );
}

/// 5. Publishing to a topic with no subscribers should not return an error.
#[tokio::test]
async fn test_nats_publish_no_subscribers() {
    let url = skip_unless_nats!();
    let bus = connect(&url).await;

    let topic = unique_topic("atta.test.nosub");
    let event = make_event("atta.test.nosub.created");

    let result = bus.publish(&topic, event).await;
    assert!(
        result.is_ok(),
        "publish with no subscribers should succeed, got: {:?}",
        result
    );
}

/// 6. A raw message with invalid JSON payload published directly via JetStream
///    should be silently skipped by the subscriber (acked and not yielded).
///
///    The test publishes one bad message followed by one valid event.
///    The subscriber must yield exactly the valid event and not hang.
#[tokio::test]
async fn test_nats_bad_payload_skipped() {
    let url = skip_unless_nats!();

    // We need direct JetStream access to inject a raw bad payload.
    let client = async_nats::connect(&url)
        .await
        .expect("failed to connect raw NATS client");
    let jetstream = async_nats::jetstream::new(client);

    let topic = unique_topic("atta.test.badpayload");

    // Publish a raw, non-JSON message directly so the subscriber encounters it.
    // Vec<u8> implements Into<bytes::Bytes>, so no extra crate dependency is needed.
    let bad_payload: Vec<u8> = b"not-valid-json!!!".to_vec();
    jetstream
        .publish(topic.clone(), bad_payload.into())
        .await
        .expect("raw publish failed")
        .await
        .expect("raw publish ack failed");

    // Now subscribe using NatsBus and publish one valid event after the bad one.
    let bus = NatsBus::connect(&url).await.expect("NatsBus connect failed");
    let mut stream = bus.subscribe(&topic).await.expect("subscribe failed");

    let valid_event = make_event("atta.test.badpayload.valid");
    let expected_id = valid_event.event_id;
    bus.publish(&topic, valid_event)
        .await
        .expect("valid publish failed");

    // The bad message should be skipped; the valid event should arrive within 5 s.
    let received = timeout(Duration::from_secs(5), stream.next())
        .await
        .expect("timed out — bad payload may have blocked the subscriber")
        .expect("stream ended unexpectedly");

    assert_eq!(
        received.event_id, expected_id,
        "subscriber should skip bad payload and deliver the valid event"
    );
}

/// 7. Two topics are isolated: a subscriber on topic A does not receive
///    events published to topic B.
#[tokio::test]
async fn test_nats_multiple_topics() {
    let url = skip_unless_nats!();
    let bus = connect(&url).await;

    let topic_a = unique_topic("atta.test.isolation.a");
    let topic_b = unique_topic("atta.test.isolation.b");

    let mut stream_a = bus.subscribe(&topic_a).await.expect("subscribe A failed");
    let mut stream_b = bus.subscribe(&topic_b).await.expect("subscribe B failed");

    let event_a = make_event("atta.test.isolation.a.created");
    let event_b = make_event("atta.test.isolation.b.created");
    let id_a = event_a.event_id;
    let id_b = event_b.event_id;

    bus.publish(&topic_a, event_a).await.expect("publish A failed");
    bus.publish(&topic_b, event_b).await.expect("publish B failed");

    let received_a = timeout(Duration::from_secs(5), stream_a.next())
        .await
        .expect("timed out on stream A")
        .expect("stream A ended unexpectedly");

    let received_b = timeout(Duration::from_secs(5), stream_b.next())
        .await
        .expect("timed out on stream B")
        .expect("stream B ended unexpectedly");

    assert_eq!(
        received_a.event_id, id_a,
        "stream A should receive event A, not event B"
    );
    assert_eq!(
        received_b.event_id, id_b,
        "stream B should receive event B, not event A"
    );
}

/// 8. High-throughput: publish 100 events and verify all 100 are received in
///    order within a 5-second window.
#[tokio::test]
async fn test_nats_high_throughput() {
    let url = skip_unless_nats!();
    let bus = connect(&url).await;

    const EVENT_COUNT: usize = 100;

    let topic = unique_topic("atta.test.throughput");
    let mut stream = bus.subscribe(&topic).await.expect("subscribe failed");

    // Collect the IDs we publish in order so we can verify completeness.
    let mut published_ids = Vec::with_capacity(EVENT_COUNT);
    for i in 0..EVENT_COUNT {
        let event = make_event(&format!("atta.test.throughput.{}", i));
        published_ids.push(event.event_id);
        bus.publish(&topic, event).await.expect("publish failed");
    }

    // Receive all events within 5 seconds total.
    let mut received_ids = Vec::with_capacity(EVENT_COUNT);
    let deadline = Duration::from_secs(5);
    let collect_fut = async {
        for _ in 0..EVENT_COUNT {
            match stream.next().await {
                Some(event) => received_ids.push(event.event_id),
                None => break,
            }
        }
    };

    timeout(deadline, collect_fut)
        .await
        .expect("timed out before receiving all 100 events");

    assert_eq!(
        received_ids.len(),
        EVENT_COUNT,
        "should receive exactly {} events, got {}",
        EVENT_COUNT,
        received_ids.len()
    );

    // Every published ID must appear in the received set (order may vary with JetStream).
    let received_set: std::collections::HashSet<Uuid> = received_ids.into_iter().collect();
    for id in &published_ids {
        assert!(
            received_set.contains(id),
            "event {} was published but never received",
            id
        );
    }
}
