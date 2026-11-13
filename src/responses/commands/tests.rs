use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};

use kameo::actor::Spawn;
use sqlx::PgPool;

use super::*;
use crate::chat::{
    ChannelName, ChatUserId, CommandText, Conversation, MessageText, Revision, Role, TopicName,
};
use crate::commands::{Commands, UNRECOGNIZED};
use crate::responses::PostgresResponses;

const MEMBER: ChatUserId = ChatUserId(10);
const MODERATOR: ChatUserId = ChatUserId(11);
const SITE: &str = "https://chess.zulipchat.example.com";
const GOOD_GAME: MessageId = MessageId(1234);
const WELL_PLAYED: MessageId = MessageId(1235);
const HIDDEN: MessageId = MessageId(999);

#[derive(Debug)]
struct KnownMessages(BTreeMap<MessageId, &'static str>);

impl MessageReader for KnownMessages {
    fn read(&self, id: MessageId) -> impl Future<Output = Result<MessageText, ReadError>> + Send {
        std::future::ready(
            self.0
                .get(&id)
                .map(|text| MessageText::new((*text).to_owned()))
                .ok_or(ReadError::Unreadable),
        )
    }
}

struct Bot {
    commands: ActorRef<Commands>,
    responses: PostgresResponses,
    next_id: AtomicU64,
}

fn link_to(id: MessageId) -> String {
    format!("{SITE}/#narrow/channel/7-chess/topic/games/near/{}", id.0)
}

fn dm_to(user_id: ChatUserId) -> Conversation {
    Conversation::Direct { user_id }
}

fn games_topic() -> Conversation {
    Conversation::Topic {
        channel: ChannelName::try_new("chess").unwrap(),
        topic: TopicName::try_new("games").unwrap(),
    }
}

impl Bot {
    fn start(pool: PgPool) -> Self {
        let responses = PostgresResponses::new(pool);
        let messages = KnownMessages(BTreeMap::from([
            (GOOD_GAME, "Good game!"),
            (WELL_PLAYED, "Well played!\n\nSee you next round."),
        ]));
        let (_, listener) = ResponseCommands::new(
            responses.clone(),
            messages,
            Url::parse(SITE).unwrap(),
            &Config::default(),
        )
        .listen();
        Self {
            commands: Commands::spawn(Commands::new(vec![listener])),
            responses,
            next_id: AtomicU64::new(1),
        }
    }

    async fn hear(
        &self,
        sender: ChatUserId,
        role: Role,
        reply_to: Conversation,
        text: &str,
    ) -> String {
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
        reply_to: Conversation,
        text: &str,
    ) -> String {
        self.commands
            .ask(IncomingMessage {
                id,
                revision,
                sender,
                role,
                reply_to,
                text: CommandText::new(text),
            })
            .await
            .unwrap()
            .markdown()
            .to_owned()
    }

    async fn moderator_says(&self, text: &str) -> String {
        self.hear(MODERATOR, Role::Moderator, dm_to(MODERATOR), text)
            .await
    }

    async fn moderator_edits_last(&self, text: &str) -> String {
        let id = MessageId(self.next_id.load(Ordering::Relaxed) - 1);
        self.hear_as(
            id,
            Revision::Edited,
            MODERATOR,
            Role::Moderator,
            dm_to(MODERATOR),
            text,
        )
        .await
    }

    async fn saved(&self) -> Vec<Response> {
        self.responses.list(None).await.unwrap()
    }
}

#[sqlx::test]
async fn a_moderator_adds_a_response_from_a_message_link(pool: PgPool) {
    let bot = Bot::start(pool);

    let reply = bot
        .moderator_says(&format!(r#"response add "GG" {}"#, link_to(GOOD_GAME)))
        .await;

    let saved = bot.saved().await;
    assert_eq!(saved.len(), 1);
    assert_eq!(saved[0].phrase.as_str(), "GG");
    assert_eq!(saved[0].matching, Matching::AnyCase);
    assert_eq!(saved[0].markdown.as_str(), "Good game!");
    assert_eq!(
        reply,
        format!("Response `{}` answers `GG` with `Good game!`.", saved[0].id)
    );
}

#[sqlx::test]
async fn a_moderator_adds_a_response_to_a_phrase_in_its_exact_case(pool: PgPool) {
    let bot = Bot::start(pool);

    let reply = bot
        .moderator_says(&format!(
            r#"response add exact "GG" {}"#,
            link_to(GOOD_GAME)
        ))
        .await;

    let saved = bot.saved().await;
    assert_eq!(saved.len(), 1);
    assert_eq!(saved[0].phrase.as_str(), "GG");
    assert_eq!(saved[0].matching, Matching::ExactCase);
    assert_eq!(
        reply,
        format!(
            "Response `{}` answers `GG` (exact case) with `Good game!`.",
            saved[0].id
        )
    );
}

#[sqlx::test]
async fn an_add_edited_to_exact_keeps_its_response_and_makes_it_exact(pool: PgPool) {
    let bot = Bot::start(pool);
    bot.moderator_says(&format!(r#"response add "GG" {}"#, link_to(GOOD_GAME)))
        .await;
    let earlier = bot.saved().await[0].id;

    let reply = bot
        .moderator_edits_last(&format!(
            r#"response add exact "GG" {}"#,
            link_to(GOOD_GAME)
        ))
        .await;

    let saved = bot.saved().await;
    assert_eq!(saved.len(), 1);
    assert_eq!(saved[0].id, earlier);
    assert_eq!(saved[0].matching, Matching::ExactCase);
    assert_eq!(
        reply,
        format!("Response `{earlier}` answers `GG` (exact case) with `Good game!`.")
    );
}

#[sqlx::test]
async fn a_moderator_adds_a_response_from_a_link_zulip_formatted_while_pasting(pool: PgPool) {
    let bot = Bot::start(pool);

    bot.hear(
        MODERATOR,
        Role::Administrator,
        games_topic(),
        &format!(r#"response add "gg" #**chess>games@{}**"#, GOOD_GAME.0),
    )
    .await;

    assert_eq!(bot.saved().await[0].markdown.as_str(), "Good game!");
}

#[sqlx::test]
async fn an_edited_add_replaces_the_response_its_earlier_version_added(pool: PgPool) {
    let bot = Bot::start(pool);
    bot.moderator_says(&format!(r#"response add "gp" {}"#, link_to(GOOD_GAME)))
        .await;
    let earlier = bot.saved().await[0].id;

    let reply = bot
        .moderator_edits_last(&format!(r#"response add "gg" {}"#, link_to(GOOD_GAME)))
        .await;

    let saved = bot.saved().await;
    assert_eq!(saved.len(), 1);
    assert_eq!(saved[0].phrase.as_str(), "gg");
    assert_eq!(
        reply,
        format!(
            "Response `{}` answers `gg` with `Good game!`. It replaces response `{earlier}`.",
            saved[0].id
        )
    );
}

#[sqlx::test]
async fn a_moderator_lists_responses_with_their_ids_phrases_and_previews(pool: PgPool) {
    let bot = Bot::start(pool);
    bot.moderator_says(&format!(r#"response add "gg" {}"#, link_to(GOOD_GAME)))
        .await;
    bot.moderator_says(&format!(
        r#"response add exact "GG" {}"#,
        link_to(WELL_PLAYED)
    ))
    .await;
    bot.moderator_says(&format!(r#"response add "wp" {}"#, link_to(WELL_PLAYED)))
        .await;
    let ids: Vec<ResponseId> = bot
        .saved()
        .await
        .iter()
        .map(|response| response.id)
        .collect();

    let everything = bot.moderator_says("response list").await;
    let one_phrase = bot.moderator_says(r#"response list "Gg""#).await;
    let nothing = bot.moderator_says(r#"response list "rules""#).await;

    assert_eq!(
        everything,
        format!(
            "Saved responses:\n- `{}` `gg`: `Good game!`\n- `{}` `GG` (exact case): `Well played!\u{2026}`\n- `{}` `wp`: `Well played!\u{2026}`",
            ids[0], ids[1], ids[2]
        )
    );
    assert_eq!(
        one_phrase,
        format!(
            "Saved responses:\n- `{}` `gg`: `Good game!`\n- `{}` `GG` (exact case): `Well played!\u{2026}`",
            ids[0], ids[1]
        )
    );
    assert_eq!(nothing, "There are no saved responses to `rules`.");
}

#[sqlx::test]
async fn a_moderator_removes_a_response(pool: PgPool) {
    let bot = Bot::start(pool);
    bot.moderator_says(&format!(r#"response add "gg" {}"#, link_to(GOOD_GAME)))
        .await;
    let id = bot.saved().await[0].id;

    let removed = bot.moderator_says(&format!("response remove {id}")).await;
    let again = bot.moderator_says(&format!("response remove {id}")).await;

    assert_eq!(bot.saved().await, []);
    assert_eq!(removed, format!("Removed response `{id}`."));
    assert_eq!(
        again,
        format!("There is no response `{id}`. Use `response list` to see the ids.")
    );
}

#[sqlx::test]
async fn a_member_is_refused_every_response_command(pool: PgPool) {
    let bot = Bot::start(pool);

    for command in [
        format!(r#"response add "gg" {}"#, link_to(GOOD_GAME)),
        "response list".to_owned(),
        "response remove 1".to_owned(),
        "response help".to_owned(),
    ] {
        let answer = bot
            .hear(MEMBER, Role::Member, dm_to(MEMBER), &command)
            .await;

        assert_eq!(answer, REFUSAL);
    }
    assert_eq!(bot.saved().await, []);
}

#[sqlx::test]
async fn help_shows_the_response_commands_to_moderators_only(pool: PgPool) {
    let bot = Bot::start(pool);

    let member = bot.hear(MEMBER, Role::Member, dm_to(MEMBER), "help").await;
    let moderator = bot.moderator_says("help").await;

    assert_eq!(member, UNRECOGNIZED);
    assert!(
        moderator.starts_with("As a moderator, you can also save responses"),
        "{moderator}"
    );
    assert!(moderator.contains("I wait 5s"), "{moderator}");
}

#[sqlx::test]
async fn a_link_to_another_site_is_refused(pool: PgPool) {
    let bot = Bot::start(pool);

    let reply = bot
        .moderator_says(&format!(
            r#"response add "gg" https://other.zulipchat.example.com/#narrow/channel/7-chess/topic/games/near/{}"#,
            GOOD_GAME.0
        ))
        .await;

    assert_eq!(bot.saved().await, []);
    assert_eq!(
        reply,
        "That link is to another site. Link to a message on https://chess.zulipchat.example.com/."
    );
}

#[sqlx::test]
async fn a_message_the_bot_cannot_read_is_reported_plainly(pool: PgPool) {
    let bot = Bot::start(pool);

    let reply = bot
        .moderator_says(&format!(r#"response add "gg" {}"#, link_to(HIDDEN)))
        .await;

    assert_eq!(bot.saved().await, []);
    assert_eq!(
        reply,
        "I can't read that message. It may have been deleted, or it may be in a private channel that I am not subscribed to."
    );
}

#[sqlx::test]
async fn other_commands_are_unrecognized(pool: PgPool) {
    let bot = Bot::start(pool);

    let answer = bot
        .hear(
            MODERATOR,
            Role::Moderator,
            dm_to(MODERATOR),
            "subscription list",
        )
        .await;

    assert_eq!(answer, UNRECOGNIZED);
}
