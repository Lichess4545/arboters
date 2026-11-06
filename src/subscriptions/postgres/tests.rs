use super::*;
use crate::heltour::team;
use crate::testing::{tag, user_id};

fn player(name: &str) -> SubscriptionSource {
    SubscriptionSource::Player(user_id(name))
}

fn team(name: &str) -> SubscriptionSource {
    SubscriptionSource::Team(team::Name::try_new(name).unwrap())
}

fn direct_message(user_id: u64) -> Conversation {
    Conversation::Direct {
        user_id: ChatUserId(user_id),
    }
}

fn topic(topic: &str) -> Conversation {
    Conversation::Topic {
        channel: ChannelName::try_new("chess").unwrap(),
        topic: TopicName::try_new(topic).unwrap(),
    }
}

fn subscription(
    requester: u64,
    source: SubscriptionSource,
    target: Conversation,
) -> NewSubscription {
    NewSubscription {
        requester: ChatUserId(requester),
        league: tag("team4545"),
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

async fn targets_of_alice_vs_bob(subscriptions: &PostgresSubscriptions) -> BTreeSet<Conversation> {
    let [alice, bob] = ["alice", "bob"].map(user_id);
    subscriptions
        .targets_for(
            &[&tag("team4545")],
            GameEventKind::GameStarted,
            [&alice, &bob],
        )
        .await
        .unwrap()
}

async fn listed_ids(subscriptions: &PostgresSubscriptions, conversation: Conversation) -> Vec<i64> {
    subscriptions
        .listed_in(&conversation)
        .await
        .unwrap()
        .iter()
        .map(|subscription| subscription.id.0)
        .collect()
}

fn owner(user: u64, at: Conversation) -> Remover {
    Remover::Owner {
        user: ChatUserId(user),
        at,
    }
}

#[sqlx::test]
async fn subscribing_to_the_same_intent_twice_returns_the_same_id(pool: PgPool) {
    let subscriptions = PostgresSubscriptions::new(pool);
    let intent = subscription(1, player("alice"), topic("games"));

    let first = subscriptions.subscribe(intent.clone()).await.unwrap();
    let second = subscriptions.subscribe(intent).await.unwrap();

    assert_eq!(first, second);
}

#[sqlx::test]
async fn a_games_targets_are_its_players_and_its_leagues_subscribers(pool: PgPool) {
    let subscriptions = subscribed(
        pool,
        vec![
            subscription(1, player("alice"), direct_message(1)),
            subscription(2, player("bob"), topic("bob")),
            subscription(3, SubscriptionSource::League, topic("games")),
            subscription(4, player("carol"), direct_message(4)),
        ],
    )
    .await;

    assert_eq!(
        targets_of_alice_vs_bob(&subscriptions).await,
        BTreeSet::from([direct_message(1), topic("bob"), topic("games")])
    );
}

#[sqlx::test]
async fn a_player_and_a_team_with_the_same_name_are_different_sources(pool: PgPool) {
    let subscriptions = subscribed(
        pool,
        vec![subscription(1, team("alice"), direct_message(1))],
    )
    .await;

    assert!(targets_of_alice_vs_bob(&subscriptions).await.is_empty());
}

#[sqlx::test]
async fn subscriptions_in_another_league_are_not_targets(pool: PgPool) {
    let elsewhere = |source| NewSubscription {
        league: tag("lonewolf"),
        ..subscription(1, source, direct_message(1))
    };
    let subscriptions = subscribed(
        pool,
        vec![
            elsewhere(player("alice")),
            elsewhere(SubscriptionSource::League),
        ],
    )
    .await;

    assert!(targets_of_alice_vs_bob(&subscriptions).await.is_empty());
}

#[sqlx::test]
async fn subscriptions_to_another_event_are_not_targets(pool: PgPool) {
    let finished = NewSubscription {
        event: GameEventKind::GameFinished,
        ..subscription(1, player("alice"), direct_message(1))
    };
    let subscriptions = subscribed(pool, vec![finished]).await;

    assert!(targets_of_alice_vs_bob(&subscriptions).await.is_empty());
}

#[sqlx::test]
async fn league_tags_match_regardless_of_case(pool: PgPool) {
    let shouted = NewSubscription {
        league: tag("Team4545"),
        ..subscription(1, player("alice"), direct_message(1))
    };
    let subscriptions = subscribed(pool, vec![shouted]).await;

    assert_eq!(
        targets_of_alice_vs_bob(&subscriptions).await,
        BTreeSet::from([direct_message(1)])
    );
}

#[test]
fn user_ids_beyond_the_storable_range_are_rejected() {
    let result = StoredTarget::from_target(&direct_message(u64::MAX));

    assert!(result.is_err());
}

#[sqlx::test]
async fn a_listed_subscription_reads_back_as_it_was_made(pool: PgPool) {
    let subscriptions = PostgresSubscriptions::new(pool);
    let made = NewSubscription {
        event: GameEventKind::GameFinished,
        ..subscription(1, team("knights"), topic("games"))
    };
    let id = subscriptions.subscribe(made).await.unwrap();

    assert_eq!(
        subscriptions.listed_in(&topic("games")).await.unwrap(),
        [Subscription {
            id,
            requester: ChatUserId(1),
            league: tag("team4545"),
            source: team("knights"),
            event: GameEventKind::GameFinished,
            target: topic("games"),
        }]
    );
}

#[sqlx::test]
async fn a_topic_lists_the_subscriptions_that_post_there_whoever_made_them(pool: PgPool) {
    let subscriptions = subscribed(
        pool,
        vec![
            subscription(1, player("alice"), topic("games")),
            subscription(2, player("bob"), topic("other games")),
            subscription(2, player("bob"), direct_message(2)),
            subscription(2, SubscriptionSource::League, topic("games")),
        ],
    )
    .await;

    assert_eq!(listed_ids(&subscriptions, topic("games")).await, [1, 4]);
}

#[sqlx::test]
async fn a_direct_message_lists_what_its_person_made_and_what_messages_them(pool: PgPool) {
    let subscriptions = subscribed(
        pool,
        vec![
            subscription(1, player("alice"), direct_message(1)),
            subscription(2, player("bob"), direct_message(1)),
            subscription(1, player("bob"), topic("games")),
            subscription(2, player("carol"), topic("games")),
        ],
    )
    .await;

    assert_eq!(
        listed_ids(&subscriptions, direct_message(1)).await,
        [1, 2, 3]
    );
    assert_eq!(
        listed_ids(&subscriptions, direct_message(3)).await,
        Vec::<i64>::new()
    );
}

#[sqlx::test]
async fn anyone_at_a_subscriptions_target_can_remove_it(pool: PgPool) {
    let subscriptions = PostgresSubscriptions::new(pool);
    let id = subscriptions
        .subscribe(subscription(1, player("alice"), topic("games")))
        .await
        .unwrap();

    assert!(
        !subscriptions
            .remove(id, owner(2, topic("other games")))
            .await
            .unwrap()
    );
    assert!(
        subscriptions
            .remove(id, owner(2, topic("games")))
            .await
            .unwrap()
    );
}

#[sqlx::test]
async fn a_requester_can_remove_their_own_subscription(pool: PgPool) {
    let subscriptions = PostgresSubscriptions::new(pool);
    let id = subscriptions
        .subscribe(subscription(1, player("alice"), topic("games")))
        .await
        .unwrap();

    assert!(
        subscriptions
            .remove(id, owner(1, direct_message(1)))
            .await
            .unwrap()
    );
}

#[sqlx::test]
async fn a_requester_cannot_remove_someone_elses_subscription(pool: PgPool) {
    let subscriptions = PostgresSubscriptions::new(pool);
    let id = subscriptions
        .subscribe(subscription(1, player("alice"), direct_message(1)))
        .await
        .unwrap();

    assert!(
        !subscriptions
            .remove(id, owner(2, direct_message(2)))
            .await
            .unwrap()
    );
}

#[sqlx::test]
async fn a_moderator_can_remove_anyones_subscription(pool: PgPool) {
    let subscriptions = PostgresSubscriptions::new(pool);
    let id = subscriptions
        .subscribe(subscription(1, player("alice"), direct_message(1)))
        .await
        .unwrap();

    assert!(subscriptions.remove(id, Remover::Moderator).await.unwrap());
}

#[sqlx::test]
async fn removing_a_subscription_that_does_not_exist_removes_nothing(pool: PgPool) {
    let subscriptions = PostgresSubscriptions::new(pool);

    assert!(
        !subscriptions
            .remove(SubscriptionId(4545), Remover::Moderator)
            .await
            .unwrap()
    );
}
