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

fn requested_by(requester: u64, subscription: NewSubscription) -> NewSubscription {
    NewSubscription {
        requester: ChatUserId(requester),
        ..subscription
    }
}

#[sqlx::test]
async fn a_requesters_subscriptions_are_listed_in_the_order_they_were_made(pool: PgPool) {
    let subscriptions = PostgresSubscriptions::new(pool);
    let alice = subscriptions
        .subscribe(requested_by(
            1,
            subscription(player("alice"), direct_message(1)),
        ))
        .await
        .unwrap();
    subscriptions
        .subscribe(requested_by(
            2,
            subscription(player("bob"), direct_message(2)),
        ))
        .await
        .unwrap();
    let league_wide = subscriptions
        .subscribe(NewSubscription {
            event: GameEventKind::GameFinished,
            ..requested_by(
                1,
                subscription(SubscriptionSource::League, channel("4545", "games")),
            )
        })
        .await
        .unwrap();
    let knights = subscriptions
        .subscribe(requested_by(
            1,
            subscription(team("Knights"), direct_message(1)),
        ))
        .await
        .unwrap();

    assert_eq!(
        subscriptions.requested_by(ChatUserId(1)).await.unwrap(),
        vec![
            Subscription {
                id: alice,
                requester: ChatUserId(1),
                league: league("team4545"),
                source: player("alice"),
                event: GameEventKind::GameStarted,
                target: direct_message(1),
            },
            Subscription {
                id: league_wide,
                requester: ChatUserId(1),
                league: league("team4545"),
                source: SubscriptionSource::League,
                event: GameEventKind::GameFinished,
                target: channel("4545", "games"),
            },
            Subscription {
                id: knights,
                requester: ChatUserId(1),
                league: league("team4545"),
                source: team("knights"),
                event: GameEventKind::GameStarted,
                target: direct_message(1),
            },
        ]
    );
}

#[sqlx::test]
async fn someone_without_subscriptions_has_none_listed(pool: PgPool) {
    let subscriptions = subscribed(
        pool,
        vec![requested_by(
            1,
            subscription(player("alice"), direct_message(1)),
        )],
    )
    .await;

    assert_eq!(subscriptions.requested_by(ChatUserId(2)).await.unwrap(), []);
}

fn owner(user: u64, at: NotificationTarget) -> Remover {
    Remover::Owner {
        user: ChatUserId(user),
        at,
    }
}

#[sqlx::test]
async fn the_subscriptions_posting_to_a_target_are_listed_whoever_made_them(pool: PgPool) {
    let subscriptions = PostgresSubscriptions::new(pool);
    let games = subscriptions
        .subscribe(requested_by(
            1,
            subscription(player("alice"), channel("4545", "games")),
        ))
        .await
        .unwrap();
    subscriptions
        .subscribe(requested_by(
            2,
            subscription(player("bob"), channel("4545", "other games")),
        ))
        .await
        .unwrap();
    subscriptions
        .subscribe(requested_by(
            2,
            subscription(player("bob"), direct_message(2)),
        ))
        .await
        .unwrap();
    let more_games = subscriptions
        .subscribe(requested_by(
            2,
            subscription(SubscriptionSource::League, channel("4545", "games")),
        ))
        .await
        .unwrap();

    let posting: Vec<(SubscriptionId, ChatUserId)> = subscriptions
        .posting_to(&channel("4545", "games"))
        .await
        .unwrap()
        .iter()
        .map(|subscription| (subscription.id, subscription.requester))
        .collect();

    assert_eq!(
        posting,
        [(games, ChatUserId(1)), (more_games, ChatUserId(2))]
    );
    assert_eq!(
        subscriptions
            .posting_to(&direct_message(2))
            .await
            .unwrap()
            .len(),
        1
    );
}

#[sqlx::test]
async fn anyone_at_a_subscriptions_target_can_remove_it(pool: PgPool) {
    let subscriptions = PostgresSubscriptions::new(pool);
    let id = subscriptions
        .subscribe(requested_by(
            1,
            subscription(player("alice"), channel("4545", "games")),
        ))
        .await
        .unwrap();

    let elsewhere = subscriptions
        .remove(id, owner(2, channel("4545", "other games")))
        .await
        .unwrap();
    let there = subscriptions
        .remove(id, owner(2, channel("4545", "games")))
        .await
        .unwrap();

    assert!(!elsewhere);
    assert!(there);
    assert_eq!(subscriptions.requested_by(ChatUserId(1)).await.unwrap(), []);
}

#[sqlx::test]
async fn a_requester_can_remove_their_own_subscription(pool: PgPool) {
    let subscriptions = PostgresSubscriptions::new(pool);
    let id = subscriptions
        .subscribe(requested_by(
            1,
            subscription(player("alice"), direct_message(1)),
        ))
        .await
        .unwrap();

    let removed = subscriptions
        .remove(id, owner(1, direct_message(1)))
        .await
        .unwrap();

    assert!(removed);
    assert_eq!(subscriptions.requested_by(ChatUserId(1)).await.unwrap(), []);
}

#[sqlx::test]
async fn a_requester_cannot_remove_someone_elses_subscription(pool: PgPool) {
    let subscriptions = PostgresSubscriptions::new(pool);
    let id = subscriptions
        .subscribe(requested_by(
            1,
            subscription(player("alice"), direct_message(1)),
        ))
        .await
        .unwrap();

    let removed = subscriptions
        .remove(id, owner(2, direct_message(2)))
        .await
        .unwrap();

    assert!(!removed);
    assert_eq!(
        subscriptions
            .requested_by(ChatUserId(1))
            .await
            .unwrap()
            .len(),
        1
    );
}

#[sqlx::test]
async fn a_moderator_can_remove_anyones_subscription(pool: PgPool) {
    let subscriptions = PostgresSubscriptions::new(pool);
    let id = subscriptions
        .subscribe(requested_by(
            1,
            subscription(player("alice"), direct_message(1)),
        ))
        .await
        .unwrap();

    let removed = subscriptions.remove(id, Remover::Moderator).await.unwrap();

    assert!(removed);
    assert_eq!(subscriptions.requested_by(ChatUserId(1)).await.unwrap(), []);
}

#[sqlx::test]
async fn removing_a_subscription_that_does_not_exist_removes_nothing(pool: PgPool) {
    let subscriptions = PostgresSubscriptions::new(pool);

    let removed = subscriptions
        .remove(SubscriptionId(4545), Remover::Moderator)
        .await
        .unwrap();

    assert!(!removed);
}
