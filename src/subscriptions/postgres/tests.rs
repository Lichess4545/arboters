use super::*;
use crate::heltour::team;
use crate::lichess;

fn league(tag: &str) -> league::Tag {
    league::Tag::try_new(tag).unwrap()
}

fn player(name: &str) -> SubscriptionSource {
    SubscriptionSource::Player(lichess::UserId::try_new(name).unwrap())
}

fn team(name: &str) -> SubscriptionSource {
    SubscriptionSource::Team(team::Name::try_new(name).unwrap())
}

fn direct_message(user_id: u64) -> NotificationTarget {
    NotificationTarget::DirectMessage {
        user_id: ChatUserId(user_id),
    }
}

fn channel(channel: &str, topic: &str) -> NotificationTarget {
    NotificationTarget::Channel {
        channel: ChannelName::try_new(channel).unwrap(),
        topic: TopicName::try_new(topic).unwrap(),
    }
}

fn subscription(source: SubscriptionSource, target: NotificationTarget) -> NewSubscription {
    NewSubscription {
        requester: ChatUserId(1),
        league: league("team4545"),
        source,
        event: GameEventKind::GameStarted,
        target,
    }
}

async fn subscribed(
    pool: PgPool,
    new_subscriptions: Vec<NewSubscription>,
) -> PostgresSubscriptions {
    let subscriptions = PostgresSubscriptions::new(pool);
    for new_subscription in new_subscriptions {
        subscriptions.subscribe(new_subscription).await.unwrap();
    }
    subscriptions
}

async fn started_targets(
    subscriptions: &PostgresSubscriptions,
    sources: &[SubscriptionSource],
) -> BTreeSet<NotificationTarget> {
    subscriptions
        .targets_for(&league("team4545"), GameEventKind::GameStarted, sources)
        .await
        .unwrap()
}

#[sqlx::test]
async fn subscribing_to_the_same_intent_twice_returns_the_same_id(pool: PgPool) {
    let subscriptions = PostgresSubscriptions::new(pool);
    let intent = subscription(player("alice"), channel("4545", "games"));

    let first = subscriptions.subscribe(intent.clone()).await.unwrap();
    let second = subscriptions.subscribe(intent).await.unwrap();

    assert_eq!(first, second);
}

#[sqlx::test]
async fn targets_of_every_requested_source_are_returned(pool: PgPool) {
    let subscriptions = subscribed(
        pool,
        vec![
            subscription(player("alice"), direct_message(7)),
            subscription(team("knights"), channel("4545", "knights")),
        ],
    )
    .await;

    let targets = started_targets(&subscriptions, &[player("alice"), team("knights")]).await;

    assert_eq!(
        targets,
        BTreeSet::from([direct_message(7), channel("4545", "knights")])
    );
}

#[sqlx::test]
async fn a_player_and_a_team_with_the_same_name_are_different_sources(pool: PgPool) {
    let subscriptions =
        subscribed(pool, vec![subscription(team("alice"), direct_message(7))]).await;

    assert!(
        started_targets(&subscriptions, &[player("alice")])
            .await
            .is_empty()
    );
}

#[sqlx::test]
async fn subscriptions_in_another_league_are_not_targets(pool: PgPool) {
    let elsewhere = NewSubscription {
        league: league("lonewolf"),
        ..subscription(player("alice"), direct_message(7))
    };
    let subscriptions = subscribed(pool, vec![elsewhere]).await;

    assert!(
        started_targets(&subscriptions, &[player("alice")])
            .await
            .is_empty()
    );
}

#[sqlx::test]
async fn subscriptions_to_another_event_are_not_targets(pool: PgPool) {
    let finished = NewSubscription {
        event: GameEventKind::GameFinished,
        ..subscription(player("alice"), direct_message(7))
    };
    let subscriptions = subscribed(pool, vec![finished]).await;

    assert!(
        started_targets(&subscriptions, &[player("alice")])
            .await
            .is_empty()
    );
}

#[sqlx::test]
async fn team_names_match_regardless_of_case(pool: PgPool) {
    let subscriptions =
        subscribed(pool, vec![subscription(team("Knights"), direct_message(7))]).await;

    assert_eq!(
        started_targets(&subscriptions, &[team("knights")]).await,
        BTreeSet::from([direct_message(7)])
    );
}

#[sqlx::test]
async fn league_tags_match_regardless_of_case(pool: PgPool) {
    let shouted = NewSubscription {
        league: league("Team4545"),
        ..subscription(player("alice"), direct_message(7))
    };
    let subscriptions = subscribed(pool, vec![shouted]).await;

    assert_eq!(
        started_targets(&subscriptions, &[player("alice")]).await,
        BTreeSet::from([direct_message(7)])
    );
}

#[test]
fn user_ids_beyond_the_storable_range_are_rejected() {
    let result = StoredTarget::from_target(&direct_message(u64::MAX));

    assert!(result.is_err());
}

#[sqlx::test]
async fn league_subscriptions_are_targets_of_the_league_source(pool: PgPool) {
    let subscriptions = subscribed(
        pool,
        vec![subscription(
            SubscriptionSource::League,
            channel("4545", "games"),
        )],
    )
    .await;

    assert_eq!(
        started_targets(&subscriptions, &[SubscriptionSource::League]).await,
        BTreeSet::from([channel("4545", "games")])
    );
}

#[sqlx::test]
async fn league_subscriptions_of_another_league_are_not_targets(pool: PgPool) {
    let elsewhere = NewSubscription {
        league: league("lonewolf"),
        ..subscription(SubscriptionSource::League, direct_message(7))
    };
    let subscriptions = subscribed(pool, vec![elsewhere]).await;

    assert!(
        started_targets(&subscriptions, &[SubscriptionSource::League])
            .await
            .is_empty()
    );
}
