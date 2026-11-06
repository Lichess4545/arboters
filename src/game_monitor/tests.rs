use futures::StreamExt;

use super::*;
use crate::testing::alice_vs_bob;

fn started(id: &str) -> GameEvent {
    GameEvent {
        change: GameChange::Started(alice_vs_bob(id)),
        matched: Vec::new(),
    }
}

#[tokio::test]
async fn a_lagging_subscriber_skips_missed_events_and_carries_on() {
    let (publisher, subscriber) = broadcast::channel(2);
    for id in ["g1", "g2", "g3"] {
        publisher.send(started(id)).unwrap();
    }
    drop(publisher);

    let followed: Vec<GameEvent> = event_stream(subscriber).collect().await;

    assert_eq!(followed, [started("g2"), started("g3")]);
}
