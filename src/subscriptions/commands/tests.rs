use std::collections::BTreeSet;
use std::convert::Infallible;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use kameo::actor::{ReplyRecipient, Spawn};
use sqlx::PgPool;

use super::*;
use crate::chat::{ChannelName, CommandText, Role, TopicName};
use crate::heltour::League;
use crate::lichess::{self, Clock, Variant};
use crate::subscriptions::{PostgresSubscriptions, Subscription};

const MEMBER: ChatUserId = ChatUserId(10);
const OTHER_MEMBER: ChatUserId = ChatUserId(12);
const MODERATOR: ChatUserId = ChatUserId(11);

#[derive(Debug, Default)]
struct FakeInbox {
    listener: Option<ReplyRecipient<IncomingMessage, Handling>>,
}

impl Actor for FakeInbox {
    type Args = Self;
    type Error = Infallible;

    fn on_start(
        inbox: Self,
        _: ActorRef<Self>,
    ) -> impl Future<Output = Result<Self, Self::Error>> + Send {
        std::future::ready(Ok(inbox))
    }
}

impl Message<Listen> for FakeInbox {
    type Reply = ();

    fn handle(
        &mut self,
        Listen(listener): Listen,
        _: &mut Context<Self, Self::Reply>,
    ) -> impl Future<Output = ()> + Send {
        self.listener = Some(listener);
        std::future::ready(())
    }
}

struct RegisteredListener;

impl Message<RegisteredListener> for FakeInbox {
    type Reply = Option<ReplyRecipient<IncomingMessage, Handling>>;

    fn handle(
        &mut self,
        _: RegisteredListener,
        _: &mut Context<Self, Self::Reply>,
    ) -> impl Future<Output = Self::Reply> + Send {
        std::future::ready(self.listener.clone())
    }
}

struct Bot {
    commands: ReplyRecipient<IncomingMessage, Handling>,
    subscriptions: PostgresSubscriptions,
    replies: Mutex<Vec<(NotificationTarget, String)>>,
    next_id: AtomicU64,
}

fn league() -> league::Tag {
    league::Tag::try_new("classical").unwrap()
}

fn active_leagues() -> ActiveLeagues {
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
    ActiveLeagues {
        leagues: vec![
            league("classical", "Classical League", 45),
            league("rapid", "Rapid Battle", 15),
            league("blitz960", "Blitz 960", 15),
        ],
        pairings: Vec::new(),
        team_pairings: Vec::new(),
    }
}

fn dm_to(user_id: ChatUserId) -> NotificationTarget {
    NotificationTarget::DirectMessage { user_id }
}

fn games_topic() -> NotificationTarget {
    NotificationTarget::Channel {
        channel: ChannelName::try_new("chess").unwrap(),
        topic: TopicName::try_new("games").unwrap(),
    }
}

fn other_topic() -> NotificationTarget {
    NotificationTarget::Channel {
        channel: ChannelName::try_new("chess").unwrap(),
        topic: TopicName::try_new("other games").unwrap(),
    }
}

fn player(name: &str) -> SubscriptionSource {
    SubscriptionSource::Player(lichess::UserId::try_new(name).unwrap())
}

impl Bot {
    async fn start(pool: PgPool) -> Self {
        let subscriptions = PostgresSubscriptions::new(pool);
        let inbox = FakeInbox::spawn(FakeInbox::default());
        let commands = SubscriptionCommands::spawn((
            SubscriptionCommands::new(subscriptions.clone()),
            inbox.clone().recipient(),
        ));
        commands.ask(active_leagues()).await.unwrap();
        Self {
            commands: inbox.ask(RegisteredListener).await.unwrap().unwrap(),
            subscriptions,
            replies: Mutex::default(),
            next_id: AtomicU64::new(1),
        }
    }

    async fn hear(
        &self,
        sender: ChatUserId,
        role: Role,
        reply_to: NotificationTarget,
        text: &str,
    ) -> Handling {
        let id = MessageId(self.next_id.fetch_add(1, Ordering::Relaxed));
        self.hear_as(id, Revision::Original, sender, role, reply_to, text)
            .await
    }

    async fn hear_as(
        &self,
        id: MessageId,
        revision: Revision,
        sender: ChatUserId,
        role: Role,
        reply_to: NotificationTarget,
        text: &str,
    ) -> Handling {
        let handling = self
            .commands
            .ask(IncomingMessage {
                id,
                revision,
                sender,
                role,
                reply_to: reply_to.clone(),
                text: CommandText::new(text),
            })
            .await
            .unwrap();
        if let Handling::Answered(answer) = &handling {
            self.replies
                .lock()
                .unwrap()
                .push((reply_to, answer.markdown().to_owned()));
        }
        handling
    }

    fn last_id(&self) -> MessageId {
        MessageId(self.next_id.load(Ordering::Relaxed) - 1)
    }

    async fn edit_last(&self, sender: ChatUserId, role: Role, text: &str) -> Handling {
        let reply_to = self.replies.lock().unwrap().last().unwrap().0.clone();
        self.hear_as(
            self.last_id(),
            Revision::Edited,
            sender,
            role,
            reply_to,
            text,
        )
        .await
    }

    async fn direct(&self, sender: ChatUserId, role: Role, text: &str) -> Handling {
        self.hear(sender, role, dm_to(sender), text).await
    }

    async fn in_topic(&self, sender: ChatUserId, role: Role, text: &str) -> Handling {
        self.hear(sender, role, games_topic(), text).await
    }

    fn replies(&self) -> Vec<(NotificationTarget, String)> {
        self.replies.lock().unwrap().clone()
    }

    fn last_reply(&self) -> (NotificationTarget, String) {
        self.replies().last().unwrap().clone()
    }

    async fn targets(
        &self,
        event: GameEventKind,
        source: SubscriptionSource,
    ) -> BTreeSet<NotificationTarget> {
        self.subscriptions
            .targets_for(&league(), event, &[source])
            .await
            .unwrap()
    }

    async fn subscriptions_of(&self, requester: ChatUserId) -> Vec<Subscription> {
        self.subscriptions.requested_by(requester).await.unwrap()
    }
}

#[sqlx::test]
async fn tell_me_for_a_player_subscribes_the_sender_by_direct_message(pool: PgPool) {
    let bot = Bot::start(pool).await;

    let handling = bot
        .direct(
            MEMBER,
            Role::Member,
            "tell me when a-game-starts in classical for alice",
        )
        .await;

    assert!(matches!(handling, Handling::Answered(_)));
    assert_eq!(
        bot.targets(GameEventKind::GameStarted, player("alice"))
            .await,
        BTreeSet::from([dm_to(MEMBER)])
    );
    let id = bot.subscriptions_of(MEMBER).await[0].id;
    assert_eq!(
        bot.last_reply(),
        (
            dm_to(MEMBER),
            format!("Subscription `{id}`: tell you when a-game-starts in classical for alice.")
        )
    );
}

#[sqlx::test]
async fn tell_me_from_a_topic_subscribes_the_sender_and_replies_in_the_topic(pool: PgPool) {
    let bot = Bot::start(pool).await;

    bot.in_topic(
        MEMBER,
        Role::Member,
        "tell me when a-game-is-over in classical for alice",
    )
    .await;

    assert_eq!(
        bot.targets(GameEventKind::GameFinished, player("alice"))
            .await,
        BTreeSet::from([dm_to(MEMBER)])
    );
    assert_eq!(bot.last_reply().0, games_topic());
}

#[sqlx::test]
async fn tell_me_without_a_player_subscribes_the_sender_to_the_whole_league(pool: PgPool) {
    let bot = Bot::start(pool).await;

    bot.direct(
        MEMBER,
        Role::Member,
        "tell me when a-game-is-over in classical",
    )
    .await;

    assert_eq!(
        bot.targets(GameEventKind::GameFinished, SubscriptionSource::League)
            .await,
        BTreeSet::from([dm_to(MEMBER)])
    );
}

#[sqlx::test]
async fn a_member_can_subscribe_the_topic_to_a_player(pool: PgPool) {
    let bot = Bot::start(pool).await;

    bot.in_topic(
        MEMBER,
        Role::Member,
        "tell here when a-game-starts in classical for alice",
    )
    .await;

    assert_eq!(
        bot.targets(GameEventKind::GameStarted, player("alice"))
            .await,
        BTreeSet::from([games_topic()])
    );
    let id = bot.subscriptions_of(MEMBER).await[0].id;
    assert_eq!(
        bot.last_reply(),
        (
            games_topic(),
            format!(
                "Subscription `{id}`: tell #**chess>games** when a-game-starts in classical for alice."
            )
        )
    );
}

#[sqlx::test]
async fn a_member_can_subscribe_a_topic_named_by_its_link_from_a_direct_message(pool: PgPool) {
    let bot = Bot::start(pool).await;

    bot.direct(
        MEMBER,
        Role::Member,
        "tell #**chess>games** when a-game-starts in classical",
    )
    .await;

    assert_eq!(
        bot.targets(GameEventKind::GameStarted, SubscriptionSource::League)
            .await,
        BTreeSet::from([games_topic()])
    );
    assert_eq!(bot.last_reply().0, dm_to(MEMBER));
}

#[sqlx::test]
async fn tell_here_in_a_direct_message_is_refused(pool: PgPool) {
    let bot = Bot::start(pool).await;

    bot.direct(
        MODERATOR,
        Role::Owner,
        "tell here when a-game-starts in classical",
    )
    .await;

    assert_eq!(bot.subscriptions_of(MODERATOR).await, []);
    assert_eq!(
        bot.last_reply(),
        (
            dm_to(MODERATOR),
            "`tell here` only works in a channel topic. In a direct message, use `tell me`, or name a topic with `tell #**channel>topic**`."
                .to_owned()
        )
    );
}

#[sqlx::test]
async fn a_league_heltour_does_not_list_as_active_is_refused(pool: PgPool) {
    let bot = Bot::start(pool).await;

    bot.direct(
        MEMBER,
        Role::Member,
        "tell me when a-game-starts in blitz for alice",
    )
    .await;

    assert_eq!(bot.subscriptions_of(MEMBER).await, []);
    assert_eq!(
        bot.last_reply(),
        (
            dm_to(MEMBER),
            "`blitz` is not an active league. The active leagues are `classical`, `rapid`, `blitz960`."
                .to_owned()
        )
    );
}

#[sqlx::test]
async fn a_malformed_tell_is_answered_with_its_usage_and_what_it_could_have_been(pool: PgPool) {
    let bot = Bot::start(pool).await;

    let handling = bot
        .direct(MEMBER, Role::Member, "tell me a-game-starts for alice")
        .await;

    assert!(matches!(handling, Handling::Answered(_)));
    assert_eq!(
        bot.last_reply().1,
        "Write it as `tell me when <event> in <league>`. Add `for <lichess-player>` to hear about one player's games only.\n\
         Try `tell me when a-game-starts in <league> for alice`."
    );
}

#[sqlx::test]
async fn a_wrong_event_is_answered_with_the_command_filled_in_from_the_rest(pool: PgPool) {
    let bot = Bot::start(pool).await;

    bot.in_topic(
        MODERATOR,
        Role::Moderator,
        "tell here when a game is scheduled in Classical League",
    )
    .await;

    assert_eq!(bot.subscriptions_of(MODERATOR).await, []);
    assert_eq!(
        bot.last_reply(),
        (
            games_topic(),
            "`<event>` is `a-game-starts` or `a-game-is-over`.\n\
             Try `tell here when a-game-starts in classical`."
                .to_owned()
        )
    );
}

#[sqlx::test]
async fn a_wrong_listener_suggests_the_conversation_it_was_sent_in(pool: PgPool) {
    let bot = Bot::start(pool).await;

    bot.in_topic(MEMBER, Role::Member, "tell this channel when ends in 45+45")
        .await;
    bot.direct(MEMBER, Role::Member, "tell this channel when ends in 45+45")
        .await;

    let replies: Vec<String> = bot.replies().into_iter().map(|(_, reply)| reply).collect();
    let listeners = "You can tell `me`, `here`, or a topic, such as `#**channel>topic**`.";
    assert_eq!(
        replies,
        [
            format!("{listeners}\nTry `tell here when a-game-is-over in classical`."),
            format!("{listeners}\nTry `tell me when a-game-is-over in classical`."),
        ]
    );
}

#[sqlx::test]
async fn a_league_named_by_its_name_time_control_or_a_typo_is_subscribed_by_its_tag(pool: PgPool) {
    let bot = Bot::start(pool).await;

    for text in [
        "tell me when a game starts in rapid battle for alice",
        "tell me when a game ends in 45+45 for alice",
        "tel me wen a game strats in blitz906 for alice",
    ] {
        bot.direct(MEMBER, Role::Member, text).await;
    }

    let described: Vec<String> = bot
        .subscriptions_of(MEMBER)
        .await
        .iter()
        .map(|subscription| format!("{} {:?}", subscription.league, subscription.event))
        .collect();
    assert_eq!(
        described,
        [
            "rapid GameStarted",
            "classical GameFinished",
            "blitz960 GameStarted"
        ]
    );
    let id = bot.subscriptions_of(MEMBER).await[2].id;
    assert_eq!(
        bot.last_reply().1,
        format!("Subscription `{id}`: tell you when a-game-starts in blitz960 for alice.")
    );
}

#[sqlx::test]
async fn a_time_control_of_several_leagues_is_answered_with_each_of_them(pool: PgPool) {
    let bot = Bot::start(pool).await;

    bot.direct(MEMBER, Role::Member, "tell me when a game ends in 15+15")
        .await;

    assert_eq!(bot.subscriptions_of(MEMBER).await, []);
    assert_eq!(
        bot.last_reply().1,
        "`15+15` could be `rapid` or `blitz960`.\n\
         Try `tell me when a-game-is-over in rapid` or `tell me when a-game-is-over in blitz960`."
    );
}

#[sqlx::test]
async fn messages_that_are_not_subscription_commands_are_left_to_others(pool: PgPool) {
    let bot = Bot::start(pool).await;

    let handling = bot.direct(MEMBER, Role::Member, "who plays today?").await;

    assert_eq!(handling, Handling::Unrecognized);
    assert_eq!(bot.replies(), []);
}

#[sqlx::test]
async fn help_lists_the_commands(pool: PgPool) {
    let bot = Bot::start(pool).await;

    let handling = bot.direct(MEMBER, Role::Member, "help").await;

    assert!(matches!(handling, Handling::Answered(_)));
    let (target, help) = bot.last_reply();
    assert_eq!(target, dm_to(MEMBER));
    for command in [
        "`tell me when <event> in <league>`",
        "`tell here when <event> in <league>`",
        "`tell #**channel>topic** when <event> in <league>`",
        "`for <lichess-player>`",
        "`subscription list`",
        "`subscription remove <id>`",
        "`a-game-starts`",
        "`a-game-is-over`",
        "The active leagues are `classical`, `rapid`, `blitz960`.",
        "`tell me when a-game-starts in classical`",
    ] {
        assert!(help.contains(command), "help is missing {command}: {help}");
    }
}

#[sqlx::test]
async fn subscription_list_shows_the_senders_subscriptions_with_their_ids(pool: PgPool) {
    let bot = Bot::start(pool).await;
    bot.direct(
        MEMBER,
        Role::Member,
        "tell me when a-game-starts in classical for alice",
    )
    .await;
    bot.direct(
        OTHER_MEMBER,
        Role::Member,
        "tell me when a-game-starts in classical for bob",
    )
    .await;
    bot.in_topic(
        MEMBER,
        Role::Moderator,
        "tell here when a-game-is-over in classical",
    )
    .await;
    let ids: Vec<_> = bot
        .subscriptions_of(MEMBER)
        .await
        .iter()
        .map(|subscription| subscription.id)
        .collect();

    bot.direct(MEMBER, Role::Member, "subscription list").await;

    assert_eq!(
        bot.last_reply(),
        (
            dm_to(MEMBER),
            format!(
                "Your subscriptions:\n\
                 - `{}`: tell you when a-game-starts in classical for alice\n\
                 - `{}`: tell #**chess>games** when a-game-is-over in classical",
                ids[0], ids[1]
            )
        )
    );
}

#[sqlx::test]
async fn subscription_list_says_so_when_there_are_none(pool: PgPool) {
    let bot = Bot::start(pool).await;

    bot.direct(MEMBER, Role::Member, "subscription list").await;

    assert_eq!(
        bot.last_reply(),
        (dm_to(MEMBER), "You have no subscriptions.".to_owned())
    );
}

async fn subscribed(bot: &Bot, requester: ChatUserId) -> SubscriptionId {
    bot.direct(
        requester,
        Role::Member,
        "tell me when a-game-starts in classical for alice",
    )
    .await;
    bot.subscriptions_of(requester).await[0].id
}

#[sqlx::test]
async fn a_member_can_remove_their_own_subscription(pool: PgPool) {
    let bot = Bot::start(pool).await;
    let id = subscribed(&bot, MEMBER).await;

    bot.direct(MEMBER, Role::Member, &format!("subscription remove {id}"))
        .await;

    assert_eq!(bot.subscriptions_of(MEMBER).await, []);
    assert_eq!(
        bot.last_reply(),
        (dm_to(MEMBER), format!("Removed subscription `{id}`."))
    );
}

#[sqlx::test]
async fn a_member_cannot_remove_someone_elses_subscription(pool: PgPool) {
    let bot = Bot::start(pool).await;
    let id = subscribed(&bot, OTHER_MEMBER).await;

    bot.direct(MEMBER, Role::Member, &format!("subscription remove {id}"))
        .await;

    assert_eq!(bot.subscriptions_of(OTHER_MEMBER).await.len(), 1);
    assert_eq!(
        bot.last_reply(),
        (
            dm_to(MEMBER),
            format!("You have no subscription `{id}`. Use `subscription list` to see yours.")
        )
    );
}

#[sqlx::test]
async fn a_moderator_can_remove_anyones_subscription(pool: PgPool) {
    let bot = Bot::start(pool).await;
    let id = subscribed(&bot, MEMBER).await;

    bot.direct(
        MODERATOR,
        Role::Moderator,
        &format!("subscription remove {id}"),
    )
    .await;

    assert_eq!(bot.subscriptions_of(MEMBER).await, []);
}

#[sqlx::test]
async fn removing_a_subscription_that_does_not_exist_says_so(pool: PgPool) {
    let bot = Bot::start(pool).await;

    bot.direct(MODERATOR, Role::Moderator, "subscription remove 4545")
        .await;

    assert_eq!(
        bot.last_reply(),
        (
            dm_to(MODERATOR),
            "There is no subscription `4545`.".to_owned()
        )
    );
}

#[sqlx::test]
async fn an_edit_into_the_same_subscription_keeps_it(pool: PgPool) {
    let bot = Bot::start(pool).await;
    let id = subscribed(&bot, MEMBER).await;

    bot.edit_last(
        MEMBER,
        Role::Member,
        "tell me when a game starts in Classical League for @alice",
    )
    .await;

    assert_eq!(
        bot.subscriptions_of(MEMBER)
            .await
            .iter()
            .map(|subscription| subscription.id)
            .collect::<Vec<_>>(),
        [id]
    );
    assert_eq!(
        bot.last_reply().1,
        format!("Subscription `{id}`: tell you when a-game-starts in classical for alice.")
    );
}

#[sqlx::test]
async fn an_edit_into_another_subscription_replaces_it(pool: PgPool) {
    let bot = Bot::start(pool).await;
    let earlier = subscribed(&bot, MEMBER).await;

    bot.edit_last(
        MEMBER,
        Role::Member,
        "tell me when a game ends in classical for alice",
    )
    .await;

    let subscriptions = bot.subscriptions_of(MEMBER).await;
    assert_eq!(subscriptions.len(), 1);
    assert_eq!(subscriptions[0].event, GameEventKind::GameFinished);
    assert_eq!(
        bot.last_reply().1,
        format!(
            "Subscription `{}`: tell you when a-game-is-over in classical for alice. It replaces subscription `{earlier}`.",
            subscriptions[0].id
        )
    );
}

#[sqlx::test]
async fn an_edit_that_is_refused_or_not_a_command_keeps_the_earlier_subscription(pool: PgPool) {
    let bot = Bot::start(pool).await;
    let id = subscribed(&bot, MEMBER).await;

    bot.edit_last(MEMBER, Role::Member, "tell me when a game starts in blitz")
        .await;
    let refused = bot.last_reply().1;
    let handling = bot.edit_last(MEMBER, Role::Member, "never mind").await;

    assert_eq!(bot.subscriptions_of(MEMBER).await.len(), 1);
    assert!(
        refused.ends_with(&format!("\nSubscription `{id}` is unchanged.")),
        "{refused}"
    );
    assert_eq!(
        handling,
        Handling::Answered(Notification::new(format!(
            "Subscription `{id}` is unchanged. Send `help` to see the commands I know."
        )))
    );
}

#[sqlx::test]
async fn an_edit_of_a_refused_command_runs_it_again(pool: PgPool) {
    let bot = Bot::start(pool).await;
    bot.direct(MEMBER, Role::Member, "tell me when a game strts in 99+0")
        .await;

    bot.edit_last(MEMBER, Role::Member, "tell me when a game starts in rapid")
        .await;

    let id = bot.subscriptions_of(MEMBER).await[0].id;
    assert_eq!(
        bot.last_reply().1,
        format!("Subscription `{id}`: tell you when a-game-starts in rapid.")
    );
}

async fn subscribed_in_topic(bot: &Bot, requester: ChatUserId) -> SubscriptionId {
    bot.in_topic(
        requester,
        Role::Member,
        "tell here when a-game-starts in classical for alice",
    )
    .await;
    bot.subscriptions_of(requester).await[0].id
}

#[sqlx::test]
async fn anyone_in_a_topic_can_remove_a_subscription_that_posts_there(pool: PgPool) {
    let bot = Bot::start(pool).await;
    let id = subscribed_in_topic(&bot, OTHER_MEMBER).await;

    bot.in_topic(MEMBER, Role::Member, &format!("subscription remove {id}"))
        .await;

    assert_eq!(bot.subscriptions_of(OTHER_MEMBER).await, []);
    assert_eq!(
        bot.last_reply(),
        (games_topic(), format!("Removed subscription `{id}`."))
    );
}

#[sqlx::test]
async fn a_subscription_that_posts_in_a_topic_cannot_be_removed_by_others_elsewhere(pool: PgPool) {
    let bot = Bot::start(pool).await;
    let id = subscribed_in_topic(&bot, OTHER_MEMBER).await;

    bot.hear(
        MEMBER,
        Role::Member,
        other_topic(),
        &format!("subscription remove {id}"),
    )
    .await;
    bot.direct(MEMBER, Role::Member, &format!("subscription remove {id}"))
        .await;

    assert_eq!(bot.subscriptions_of(OTHER_MEMBER).await.len(), 1);
    assert_eq!(
        bot.replies()[1..],
        [
            (
                other_topic(),
                format!(
                    "No subscription `{id}` is yours or posts in this topic. Use `subscription list` to see the ones here."
                )
            ),
            (
                dm_to(MEMBER),
                format!("You have no subscription `{id}`. Use `subscription list` to see yours.")
            ),
        ]
    );
}

#[sqlx::test]
async fn a_requester_can_remove_a_topic_subscription_from_anywhere(pool: PgPool) {
    let bot = Bot::start(pool).await;
    let id = subscribed_in_topic(&bot, MEMBER).await;

    bot.hear(
        MEMBER,
        Role::Member,
        other_topic(),
        &format!("subscription remove {id}"),
    )
    .await;

    assert_eq!(bot.subscriptions_of(MEMBER).await, []);
}

#[sqlx::test]
async fn subscription_list_in_a_topic_shows_everyone_s_subscriptions_that_post_there(pool: PgPool) {
    let bot = Bot::start(pool).await;
    let theirs = subscribed_in_topic(&bot, OTHER_MEMBER).await;
    bot.in_topic(
        MEMBER,
        Role::Member,
        "tell here when a game ends in classical",
    )
    .await;
    let mine = bot.subscriptions_of(MEMBER).await[0].id;
    bot.in_topic(
        MEMBER,
        Role::Member,
        "tell me when a game ends in classical",
    )
    .await;
    bot.hear(
        MEMBER,
        Role::Member,
        other_topic(),
        "tell here when a game ends in rapid",
    )
    .await;

    bot.in_topic(MEMBER, Role::Member, "subscription list")
        .await;

    assert_eq!(
        bot.last_reply(),
        (
            games_topic(),
            format!(
                "Subscriptions that post in this topic:\n\
                 - `{theirs}`: tell #**chess>games** when a-game-starts in classical for alice, made by @_**|12**\n\
                 - `{mine}`: tell #**chess>games** when a-game-is-over in classical"
            )
        )
    );
}

#[sqlx::test]
async fn subscription_list_in_a_topic_without_subscriptions_says_so(pool: PgPool) {
    let bot = Bot::start(pool).await;
    subscribed(&bot, MEMBER).await;

    bot.in_topic(MEMBER, Role::Member, "subscription list")
        .await;

    assert_eq!(bot.last_reply().1, "No subscription posts in this topic.");
}

#[sqlx::test]
async fn help_ends_with_what_the_sender_can_do_where_they_asked(pool: PgPool) {
    let bot = Bot::start(pool).await;

    for (role, reply_to, permissions) in [
        (
            Role::Member,
            games_topic(),
            "In this topic you can list and remove any subscription that posts here. You can remove the ones you made from anywhere.",
        ),
        (
            Role::Member,
            dm_to(MEMBER),
            "You can list and remove the subscriptions you made and the ones that send you direct messages.",
        ),
        (
            Role::Moderator,
            games_topic(),
            "You are a moderator, so you can remove any subscription from anywhere. You can also subscribe someone else with `tell @**user** when <event> in <league>`.",
        ),
        (
            Role::Moderator,
            dm_to(MEMBER),
            "You are a moderator, so you can remove any subscription from anywhere. You can also subscribe someone else with `tell @**user** when <event> in <league>`.",
        ),
    ] {
        bot.hear(MEMBER, role, reply_to, "help").await;
        let help = bot.last_reply().1;
        assert!(
            help.ends_with(&format!("\n{permissions}")),
            "{role:?}: {help}"
        );
    }
}

#[sqlx::test]
async fn a_moderator_can_subscribe_someone_else_by_direct_message(pool: PgPool) {
    let bot = Bot::start(pool).await;

    bot.in_topic(
        MODERATOR,
        Role::Moderator,
        "tell @**Member|10** when a game starts in classical for alice",
    )
    .await;

    assert_eq!(
        bot.targets(GameEventKind::GameStarted, player("alice"))
            .await,
        BTreeSet::from([dm_to(MEMBER)])
    );
    let made = bot.subscriptions_of(MODERATOR).await;
    assert_eq!(made[0].target, dm_to(MEMBER));
    assert_eq!(
        bot.last_reply(),
        (
            games_topic(),
            format!(
                "Subscription `{}`: tell @_**|10** when a-game-starts in classical for alice.",
                made[0].id
            )
        )
    );
}

#[sqlx::test]
async fn a_member_cannot_subscribe_someone_else(pool: PgPool) {
    let bot = Bot::start(pool).await;

    bot.direct(
        MEMBER,
        Role::Member,
        "tell @**Other|12** when a game starts in classical",
    )
    .await;
    bot.direct(
        MEMBER,
        Role::Member,
        "tell @**Member|10** when a game ends in classical",
    )
    .await;

    let replies = bot.replies();
    assert_eq!(
        replies[0].1,
        "Only moderators can subscribe someone else. Use `tell me` to subscribe yourself."
    );
    let made = bot.subscriptions_of(MEMBER).await;
    assert_eq!(made.len(), 1);
    assert_eq!(made[0].target, dm_to(MEMBER));
}

#[sqlx::test]
async fn a_mention_without_a_user_id_is_answered_with_how_to_pick_the_user(pool: PgPool) {
    let bot = Bot::start(pool).await;

    bot.direct(
        MODERATOR,
        Role::Moderator,
        "tell @**Nobody** when a game starts in classical",
    )
    .await;

    assert_eq!(
        bot.last_reply().1,
        "I could not tell who you mentioned. Type `@` and pick them from the list.\n\
         Try `tell me when a-game-starts in classical`."
    );
}

#[sqlx::test]
async fn subscriptions_that_send_someone_direct_messages_are_theirs_to_list_and_remove(
    pool: PgPool,
) {
    let bot = Bot::start(pool).await;
    subscribed(&bot, MEMBER).await;
    bot.direct(
        MODERATOR,
        Role::Moderator,
        "tell @**Member|10** when a game ends in classical",
    )
    .await;
    let theirs = bot.subscriptions_of(MODERATOR).await[0].id;
    let mine = bot.subscriptions_of(MEMBER).await[0].id;

    bot.direct(MEMBER, Role::Member, "subscription list").await;
    let listed = bot.last_reply().1;
    bot.direct(
        MEMBER,
        Role::Member,
        &format!("subscription remove {theirs}"),
    )
    .await;

    assert_eq!(
        listed,
        format!(
            "Your subscriptions:\n\
             - `{mine}`: tell you when a-game-starts in classical for alice\n\
             - `{theirs}`: tell you when a-game-is-over in classical, made by @_**|11**"
        )
    );
    assert_eq!(bot.subscriptions_of(MODERATOR).await, []);
    assert_eq!(
        bot.last_reply().1,
        format!("Removed subscription `{theirs}`.")
    );
}
