use std::convert::Infallible;

use kameo::actor::Spawn;
use sqlx::PgPool;

use super::*;
use crate::chat::{ChannelName, CommandText, Role, TopicName};
use crate::heltour::league;
use crate::lichess::{self, Clock, Variant};
use crate::subscriptions::PostgresSubscriptions;

const MEMBER: ChatUserId = ChatUserId(10);
const MODERATOR: ChatUserId = ChatUserId(11);
const OTHER_MEMBER: ChatUserId = ChatUserId(12);

const ACTIVE_LEAGUES: &str = "The active leagues are `classical`, `rapid`, `blitz960`.";

#[derive(Debug)]
struct Inbox;

impl Actor for Inbox {
    type Args = Self;
    type Error = Infallible;

    fn on_start(
        inbox: Self,
        _: ActorRef<Self>,
    ) -> impl Future<Output = Result<Self, Self::Error>> + Send {
        std::future::ready(Ok(inbox))
    }
}

impl Message<Listen> for Inbox {
    type Reply = ();

    async fn handle(&mut self, _: Listen, _: &mut Context<Self, Self::Reply>) {}
}

type Commands = ActorRef<SubscriptionCommands<PostgresSubscriptions>>;

fn started(pool: PgPool) -> Commands {
    let league = |tag, name, minutes| League {
        tag: league::Tag::try_new(tag).unwrap(),
        name: league::Name::try_new(name).unwrap(),
        time_control: Some(Clock {
            initial_seconds: minutes * 60,
            increment_seconds: minutes,
        }),
        variant: Variant::Standard,
        rated: true,
    };
    let (_, active_leagues) = watch::channel(ActiveLeagues {
        leagues: vec![
            league("classical", "Classical League", 45),
            league("rapid", "Rapid Battle", 15),
            league("blitz960", "Blitz 960", 15),
        ],
        pairings: Vec::new(),
    });
    SubscriptionCommands::spawn((
        SubscriptionCommands::new(
            PostgresSubscriptions::new(pool),
            lichess::Config::default().base_url,
            active_leagues,
        ),
        Inbox::spawn(Inbox).recipient(),
    ))
}

async fn reply(commands: &Commands, message: IncomingMessage) -> String {
    match commands.ask(message).await.unwrap() {
        Handling::Answered(answer) => answer.markdown().to_owned(),
        Handling::Unrecognized => panic!("the commands left the message to others"),
    }
}

fn topic(topic: &str) -> Conversation {
    Conversation::Topic {
        channel: ChannelName::try_new("chess").unwrap(),
        topic: TopicName::try_new(topic).unwrap(),
    }
}

fn message(sender: ChatUserId, reply_to: Conversation, text: &str) -> IncomingMessage {
    IncomingMessage {
        id: MessageId(1),
        revision: Revision::Original,
        sender,
        role: if sender == MODERATOR {
            Role::Moderator
        } else {
            Role::Member
        },
        reply_to,
        text: CommandText::new(text),
    }
}

fn dm(sender: ChatUserId, text: &str) -> IncomingMessage {
    message(sender, Conversation::Direct { user_id: sender }, text)
}

fn in_games(sender: ChatUserId, text: &str) -> IncomingMessage {
    message(sender, topic("games"), text)
}

fn edited(original: &IncomingMessage, text: &str) -> IncomingMessage {
    IncomingMessage {
        revision: Revision::Edited,
        text: CommandText::new(text),
        ..original.clone()
    }
}

#[sqlx::test]
async fn tell_me_for_a_player_subscribes_the_sender_to_that_players_games(pool: PgPool) {
    let commands = started(pool);

    assert_eq!(
        reply(
            &commands,
            dm(MEMBER, "tell me when a-game-starts in classical for alice")
        )
        .await,
        "Subscription `1`: tell you when a-game-starts in classical for alice."
    );
}

#[sqlx::test]
async fn tell_me_without_a_player_subscribes_the_sender_to_the_whole_league(pool: PgPool) {
    let commands = started(pool);

    assert_eq!(
        reply(
            &commands,
            dm(MEMBER, "tell me when a-game-is-over in classical")
        )
        .await,
        "Subscription `1`: tell you when a-game-is-over in classical."
    );
}

#[sqlx::test]
async fn tell_me_from_a_topic_subscribes_the_sender(pool: PgPool) {
    let commands = started(pool);

    assert_eq!(
        reply(
            &commands,
            in_games(MEMBER, "tell me when a-game-is-over in classical")
        )
        .await,
        "Subscription `1`: tell you when a-game-is-over in classical."
    );
}

#[sqlx::test]
async fn tell_here_subscribes_the_topic(pool: PgPool) {
    let commands = started(pool);

    assert_eq!(
        reply(
            &commands,
            in_games(
                MEMBER,
                "tell here when a-game-starts in classical for alice"
            )
        )
        .await,
        "Subscription `1`: tell #**chess>games** when a-game-starts in classical for alice."
    );
}

#[sqlx::test]
async fn a_topic_link_subscribes_that_topic_from_a_direct_message(pool: PgPool) {
    let commands = started(pool);

    assert_eq!(
        reply(
            &commands,
            dm(
                MEMBER,
                "tell #**chess>other games** when a-game-starts in classical"
            )
        )
        .await,
        "Subscription `1`: tell #**chess>other games** when a-game-starts in classical."
    );
}

#[sqlx::test]
async fn a_moderator_can_subscribe_someone_else(pool: PgPool) {
    let commands = started(pool);

    assert_eq!(
        reply(
            &commands,
            in_games(
                MODERATOR,
                "tell @**Member|10** when a game starts in classical"
            )
        )
        .await,
        "Subscription `1`: tell @_**|10** when a-game-starts in classical."
    );
}

#[sqlx::test]
async fn a_member_cannot_subscribe_someone_else(pool: PgPool) {
    let commands = started(pool);

    assert_eq!(
        reply(
            &commands,
            dm(MEMBER, "tell @**Other|12** when a game starts in classical")
        )
        .await,
        "Only moderators can subscribe someone else. Use `tell me` to subscribe yourself."
    );
}

#[sqlx::test]
async fn a_member_who_mentions_themselves_is_subscribed(pool: PgPool) {
    let commands = started(pool);

    assert_eq!(
        reply(
            &commands,
            dm(MEMBER, "tell @**Member|10** when a game ends in classical")
        )
        .await,
        "Subscription `1`: tell you when a-game-is-over in classical."
    );
}

#[sqlx::test]
async fn a_league_named_by_its_name_time_control_or_a_typo_is_subscribed_by_its_tag(pool: PgPool) {
    let commands = started(pool);

    for (text, subscribed) in [
        ("tell me when a game starts in Rapid Battle", "rapid"),
        ("tell me when a game starts in 45+45", "classical"),
        ("tel me wen a game strats in blitz906", "blitz960"),
    ] {
        assert!(
            reply(&commands, dm(MEMBER, text))
                .await
                .ends_with(&format!(" in {subscribed}.")),
            "{text}"
        );
    }
}

#[sqlx::test]
async fn tell_here_in_a_direct_message_is_refused(pool: PgPool) {
    let commands = started(pool);

    assert_eq!(
        reply(
            &commands,
            dm(MEMBER, "tell here when a-game-starts in classical")
        )
        .await,
        "`tell here` only works in a channel topic. In a direct message, use `tell me`, or name a topic with `tell #**channel>topic**`."
    );
}

#[sqlx::test]
async fn a_league_heltour_does_not_list_as_active_is_refused(pool: PgPool) {
    let commands = started(pool);

    assert_eq!(
        reply(&commands, dm(MEMBER, "tell me when a-game-starts in blitz")).await,
        format!("`blitz` is not an active league. {ACTIVE_LEAGUES}")
    );
}

#[sqlx::test]
async fn a_time_control_of_several_leagues_is_answered_with_each_of_them(pool: PgPool) {
    let commands = started(pool);

    assert_eq!(
        reply(&commands, dm(MEMBER, "tell me when a game ends in 15+15")).await,
        "`15+15` could be `rapid` or `blitz960`.\n\
         Try `tell me when a-game-is-over in rapid` or `tell me when a-game-is-over in blitz960`."
    );
}

#[sqlx::test]
async fn a_mistake_is_answered_with_what_is_wrong_and_the_command_filled_in_from_the_rest(
    pool: PgPool,
) {
    let commands = started(pool);

    for (text, answer) in [
        (
            "tell me a-game-starts for alice",
            "Write it as `tell me when <event> in <league>`. Add `for <lichess-player>` to hear about one player's games only.\n\
             Try `tell me when a-game-starts in <league> for alice`.",
        ),
        (
            "tell me when a game is scheduled in Classical League",
            "`<event>` is `a-game-starts` or `a-game-is-over`.\n\
             Try `tell me when a-game-starts in classical`.",
        ),
        (
            "tell me when starts",
            "`<league>` is a league's tag, name, or time control. The active leagues are `classical`, `rapid`, `blitz960`.\n\
             Try `tell me when a-game-starts in <league>`.",
        ),
        (
            "tell me when a game starts in classical for a",
            "That is not a Lichess username.\n\
             Try `tell me when a-game-starts in classical for <lichess-player>`.",
        ),
        (
            "tell #**chess** when starts in rapid",
            "A topic link looks like `#**channel>topic**`. To write one, type `#` and pick the topic.\n\
             Try `tell me when a-game-starts in rapid`.",
        ),
        (
            "tell @**Nobody** when a game starts in classical",
            "I could not tell who you mentioned. Type `@` and pick them from the list.\n\
             Try `tell me when a-game-starts in classical`.",
        ),
        (
            "subscription show",
            "Use `tell me when <event> in <league>` to add a subscription, `subscription list` to see yours, or `subscription remove <id>` to remove one.",
        ),
        (
            "subscription remove twelve",
            "A subscription id is a number. Use `subscription list` to see yours.",
        ),
    ] {
        assert_eq!(reply(&commands, dm(MEMBER, text)).await, answer, "{text}");
    }
}

#[sqlx::test]
async fn a_wrong_listener_suggests_the_conversation_it_was_sent_in(pool: PgPool) {
    let commands = started(pool);
    let listeners = "You can tell `me`, `here`, or a topic, such as `#**channel>topic**`.";

    assert_eq!(
        reply(
            &commands,
            in_games(MEMBER, "tell this channel when ends in 45+45")
        )
        .await,
        format!("{listeners}\nTry `tell here when a-game-is-over in classical`.")
    );
    assert_eq!(
        reply(
            &commands,
            dm(MEMBER, "tell this channel when ends in 45+45")
        )
        .await,
        format!("{listeners}\nTry `tell me when a-game-is-over in classical`.")
    );
}

#[sqlx::test]
async fn subscription_list_shows_the_senders_subscriptions_with_their_ids(pool: PgPool) {
    let commands = started(pool);
    reply(
        &commands,
        dm(MEMBER, "tell me when a-game-starts in classical for alice"),
    )
    .await;
    reply(
        &commands,
        dm(
            OTHER_MEMBER,
            "tell me when a-game-starts in classical for bob",
        ),
    )
    .await;
    reply(
        &commands,
        in_games(MEMBER, "tell here when a-game-is-over in classical"),
    )
    .await;

    assert_eq!(
        reply(&commands, dm(MEMBER, "subscription list")).await,
        "Your subscriptions:\n\
         - `1`: tell you when a-game-starts in classical for alice\n\
         - `3`: tell #**chess>games** when a-game-is-over in classical"
    );
}

#[sqlx::test]
async fn subscription_list_says_so_when_there_are_none(pool: PgPool) {
    let commands = started(pool);

    assert_eq!(
        reply(&commands, dm(MEMBER, "subscription list")).await,
        "You have no subscriptions."
    );
}

#[sqlx::test]
async fn subscription_list_in_a_topic_shows_everyones_subscriptions_that_post_there(pool: PgPool) {
    let commands = started(pool);
    reply(
        &commands,
        in_games(OTHER_MEMBER, "tell here when a-game-starts in classical"),
    )
    .await;
    reply(
        &commands,
        in_games(MEMBER, "tell here when a game ends in classical"),
    )
    .await;
    reply(
        &commands,
        in_games(MEMBER, "tell me when a game ends in classical"),
    )
    .await;
    reply(
        &commands,
        message(
            MEMBER,
            topic("other games"),
            "tell here when a game ends in rapid",
        ),
    )
    .await;

    assert_eq!(
        reply(&commands, in_games(MEMBER, "subscription list")).await,
        "Subscriptions that post in this topic:\n\
         - `1`: tell #**chess>games** when a-game-starts in classical, made by @_**|12**\n\
         - `2`: tell #**chess>games** when a-game-is-over in classical"
    );
}

#[sqlx::test]
async fn subscription_list_in_a_topic_without_subscriptions_says_so(pool: PgPool) {
    let commands = started(pool);
    reply(
        &commands,
        dm(MEMBER, "tell me when a-game-starts in classical"),
    )
    .await;

    assert_eq!(
        reply(&commands, in_games(MEMBER, "subscription list")).await,
        "No subscription posts in this topic."
    );
}

#[sqlx::test]
async fn subscriptions_that_send_someone_direct_messages_are_theirs_to_list_and_remove(
    pool: PgPool,
) {
    let commands = started(pool);
    reply(
        &commands,
        dm(MEMBER, "tell me when a-game-starts in classical for alice"),
    )
    .await;
    reply(
        &commands,
        dm(
            MODERATOR,
            "tell @**Member|10** when a game ends in classical",
        ),
    )
    .await;

    assert_eq!(
        reply(&commands, dm(MEMBER, "subscription list")).await,
        "Your subscriptions:\n\
         - `1`: tell you when a-game-starts in classical for alice\n\
         - `2`: tell you when a-game-is-over in classical, made by @_**|11**"
    );
    assert_eq!(
        reply(&commands, dm(MEMBER, "subscription remove 2")).await,
        "Removed subscription `2`."
    );
}

#[sqlx::test]
async fn a_member_can_remove_their_own_subscription(pool: PgPool) {
    let commands = started(pool);
    reply(
        &commands,
        dm(MEMBER, "tell me when a-game-starts in classical"),
    )
    .await;

    assert_eq!(
        reply(&commands, dm(MEMBER, "subscription remove 1")).await,
        "Removed subscription `1`."
    );
    assert_eq!(
        reply(&commands, dm(MEMBER, "subscription list")).await,
        "You have no subscriptions."
    );
}

#[sqlx::test]
async fn a_member_cannot_remove_someone_elses_subscription(pool: PgPool) {
    let commands = started(pool);
    reply(
        &commands,
        dm(OTHER_MEMBER, "tell me when a-game-starts in classical"),
    )
    .await;

    assert_eq!(
        reply(&commands, dm(MEMBER, "subscription remove 1")).await,
        "You have no subscription `1`. Use `subscription list` to see yours."
    );
}

#[sqlx::test]
async fn a_moderator_can_remove_anyones_subscription(pool: PgPool) {
    let commands = started(pool);
    reply(
        &commands,
        dm(MEMBER, "tell me when a-game-starts in classical"),
    )
    .await;

    assert_eq!(
        reply(&commands, dm(MODERATOR, "subscription remove 1")).await,
        "Removed subscription `1`."
    );
}

#[sqlx::test]
async fn removing_a_subscription_that_does_not_exist_says_so(pool: PgPool) {
    let commands = started(pool);

    assert_eq!(
        reply(&commands, dm(MODERATOR, "subscription remove 4545")).await,
        "There is no subscription `4545`."
    );
}

#[sqlx::test]
async fn anyone_in_a_topic_can_remove_a_subscription_that_posts_there(pool: PgPool) {
    let commands = started(pool);
    reply(
        &commands,
        in_games(OTHER_MEMBER, "tell here when a-game-starts in classical"),
    )
    .await;

    assert_eq!(
        reply(&commands, in_games(MEMBER, "subscription remove 1")).await,
        "Removed subscription `1`."
    );
}

#[sqlx::test]
async fn a_subscription_that_posts_in_a_topic_cannot_be_removed_by_others_elsewhere(pool: PgPool) {
    let commands = started(pool);
    reply(
        &commands,
        in_games(OTHER_MEMBER, "tell here when a-game-starts in classical"),
    )
    .await;

    assert_eq!(
        reply(
            &commands,
            message(MEMBER, topic("other games"), "subscription remove 1")
        )
        .await,
        "No subscription `1` is yours or posts in this topic. Use `subscription list` to see the ones here."
    );
    assert_eq!(
        reply(&commands, dm(MEMBER, "subscription remove 1")).await,
        "You have no subscription `1`. Use `subscription list` to see yours."
    );
}

#[sqlx::test]
async fn a_requester_can_remove_a_topic_subscription_from_anywhere(pool: PgPool) {
    let commands = started(pool);
    reply(
        &commands,
        in_games(MEMBER, "tell here when a-game-starts in classical"),
    )
    .await;

    assert_eq!(
        reply(
            &commands,
            message(MEMBER, topic("other games"), "subscription remove 1")
        )
        .await,
        "Removed subscription `1`."
    );
}

#[sqlx::test]
async fn an_edit_into_the_same_subscription_keeps_it(pool: PgPool) {
    let commands = started(pool);
    let original = dm(MEMBER, "tell me when a-game-starts in classical for alice");
    reply(&commands, original.clone()).await;

    assert_eq!(
        reply(
            &commands,
            edited(
                &original,
                "tell me when a game starts in Classical League for @alice"
            )
        )
        .await,
        "Subscription `1`: tell you when a-game-starts in classical for alice."
    );
}

#[sqlx::test]
async fn an_edit_into_another_subscription_replaces_it(pool: PgPool) {
    let commands = started(pool);
    let original = dm(MEMBER, "tell me when a-game-starts in classical for alice");
    reply(&commands, original.clone()).await;

    assert_eq!(
        reply(
            &commands,
            edited(&original, "tell me when a game ends in classical for alice")
        )
        .await,
        "Subscription `2`: tell you when a-game-is-over in classical for alice. It replaces subscription `1`."
    );
    assert_eq!(
        reply(&commands, dm(MEMBER, "subscription list")).await,
        "Your subscriptions:\n- `2`: tell you when a-game-is-over in classical for alice"
    );
}

#[sqlx::test]
async fn an_edit_that_is_refused_or_not_a_command_keeps_the_earlier_subscription(pool: PgPool) {
    let commands = started(pool);
    let original = dm(MEMBER, "tell me when a-game-starts in classical");
    reply(&commands, original.clone()).await;

    assert_eq!(
        reply(
            &commands,
            edited(&original, "tell me when a game starts in blitz")
        )
        .await,
        format!(
            "`blitz` is not an active league. {ACTIVE_LEAGUES}\nSubscription `1` is unchanged."
        )
    );
    assert_eq!(
        reply(&commands, edited(&original, "never mind")).await,
        "Subscription `1` is unchanged. Send `help` to see the commands I know."
    );
}

#[sqlx::test]
async fn an_edit_of_a_refused_command_runs_it_again(pool: PgPool) {
    let commands = started(pool);
    let original = dm(MEMBER, "tell me when a game strts in 99+0");
    reply(&commands, original.clone()).await;

    assert_eq!(
        reply(
            &commands,
            edited(&original, "tell me when a game starts in rapid")
        )
        .await,
        "Subscription `1`: tell you when a-game-starts in rapid."
    );
}

#[sqlx::test]
async fn help_lists_the_commands_and_the_active_leagues(pool: PgPool) {
    let commands = started(pool);

    let help = reply(&commands, dm(MEMBER, "help")).await;

    for expected in [
        "`tell me when <event> in <league>`",
        "`tell here when <event> in <league>`",
        "`tell #**channel>topic** when <event> in <league>`",
        "`for <lichess-player>`",
        "`subscription list`",
        "`subscription remove <id>`",
        "`a-game-starts`",
        "`a-game-is-over`",
        ACTIVE_LEAGUES,
        "`tell me when a-game-starts in classical`",
    ] {
        assert!(
            help.contains(expected),
            "help is missing {expected}: {help}"
        );
    }
}

#[sqlx::test]
async fn help_ends_with_what_the_sender_can_do_where_they_asked(pool: PgPool) {
    let commands = started(pool);
    let moderating = "You are a moderator, so you can remove any subscription from anywhere. You can also subscribe someone else with `tell @**user** when <event> in <league>`.";

    for (asked, permissions) in [
        (
            in_games(MEMBER, "help"),
            "In this topic you can list and remove any subscription that posts here. You can remove the ones you made from anywhere.",
        ),
        (
            dm(MEMBER, "help"),
            "You can list and remove the subscriptions you made and the ones that send you direct messages.",
        ),
        (in_games(MODERATOR, "help"), moderating),
        (dm(MODERATOR, "help"), moderating),
    ] {
        let help = reply(&commands, asked).await;
        assert!(help.ends_with(&format!("\n{permissions}")), "{help}");
    }
}

#[sqlx::test]
async fn messages_that_are_not_subscription_commands_are_left_to_others(pool: PgPool) {
    let commands = started(pool);

    let handling = commands.ask(dm(MEMBER, "who plays today?")).await.unwrap();

    assert_eq!(handling, Handling::Unrecognized);
}
