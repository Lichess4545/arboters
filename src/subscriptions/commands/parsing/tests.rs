use super::*;
use crate::game_monitor::LichessSettings;

fn parse(text: &str) -> Option<Result<Command, Mistake>> {
    super::parse(text, &LichessSettings::default().base_url)
}

fn told(text: &str) -> Tell {
    match parse(text) {
        Some(Ok(Command::Tell(tell))) => tell,
        other => panic!("{text} parsed as {other:?}"),
    }
}

fn written(league: &str) -> WrittenLeague {
    WrittenLeague::try_new(league).unwrap()
}

fn user(name: &str) -> lichess::UserId {
    lichess::UserId::try_new(name).unwrap()
}

fn tell(listener: Listener, event: GameEventKind, league: &str, player: Option<&str>) -> Tell {
    Tell {
        listener,
        event,
        league: written(league),
        player: player.map(user),
    }
}

fn mistake(text: &str) -> (Problem, Draft) {
    match parse(text) {
        Some(Err(Mistake::Tell(problem, draft))) => (problem, draft),
        other => panic!("{text} parsed as {other:?}"),
    }
}

#[test]
fn tell_me_for_a_player_subscribes_the_sender_to_that_players_games() {
    assert_eq!(
        told("tell me when a-game-starts in classical for alice"),
        tell(
            Listener::Me,
            GameEventKind::GameStarted,
            "classical",
            Some("alice")
        )
    );
}

#[test]
fn without_a_player_the_subscription_covers_the_whole_league() {
    assert_eq!(
        told("tell here when a-game-is-over in classical"),
        tell(
            Listener::Here,
            GameEventKind::GameFinished,
            "classical",
            None
        )
    );
}

#[test]
fn words_match_regardless_of_case_and_spacing() {
    assert_eq!(
        told("  Tell  Me when A-Game-Starts in Classical\nfor Alice "),
        tell(
            Listener::Me,
            GameEventKind::GameStarted,
            "classical",
            Some("alice")
        )
    );
}

#[test]
fn events_can_be_written_in_plain_words() {
    for (event, kind) in [
        ("a-game-starts", GameEventKind::GameStarted),
        ("a game starts", GameEventKind::GameStarted),
        ("the game starts", GameEventKind::GameStarted),
        ("game starts", GameEventKind::GameStarted),
        ("starts", GameEventKind::GameStarted),
        ("a game begins", GameEventKind::GameStarted),
        ("begins", GameEventKind::GameStarted),
        ("a-game-is-over", GameEventKind::GameFinished),
        ("a game is over", GameEventKind::GameFinished),
        ("A Game Ends", GameEventKind::GameFinished),
        ("a-game-ends", GameEventKind::GameFinished),
        ("ends", GameEventKind::GameFinished),
        ("finishes", GameEventKind::GameFinished),
        ("a game finishes", GameEventKind::GameFinished),
        ("is over", GameEventKind::GameFinished),
        ("game over", GameEventKind::GameFinished),
    ] {
        assert_eq!(
            told(&format!("tell me when {event} in classical")).event,
            kind,
            "{event}"
        );
    }
}

#[test]
fn a_mention_with_a_user_id_names_that_user() {
    for mention in ["@**Alice Smith|23**", "@_**Alice Smith|23**", "@_**|23**"] {
        assert_eq!(
            told(&format!("tell {mention} when ends in classical")).listener,
            Listener::User(ChatUserId(23)),
            "{mention}"
        );
    }
    assert_eq!(
        mistake("tell @**Alice Smith** when ends in classical").0,
        Problem::Person
    );
}

#[test]
fn here_can_also_be_written_this_topic_or_us() {
    for listener in ["here", "this topic", "us", "This Topic"] {
        assert_eq!(
            told(&format!("tell {listener} when a-game-starts in classical")).listener,
            Listener::Here,
            "{listener}"
        );
    }
}

#[test]
fn a_league_is_everything_between_in_and_for() {
    assert_eq!(
        told("tell me when a game ends in Team 4545 League for alice"),
        tell(
            Listener::Me,
            GameEventKind::GameFinished,
            "team 4545 league",
            Some("alice")
        )
    );
    assert_eq!(
        told("tell here when starts in 45+45").league,
        written("45+45")
    );
}

#[test]
fn a_player_can_be_a_mention_or_a_lichess_profile_link() {
    for player in [
        "alice",
        "@alice",
        "https://lichess.org/@/Alice",
        "lichess.org/@/alice/",
        "https://www.lichess.org/@/alice",
    ] {
        assert_eq!(
            told(&format!("tell me when ends in classical for {player}")).player,
            Some(user("alice")),
            "{player}"
        );
    }
}

#[test]
fn a_profile_link_is_read_on_the_configured_lichess_site_only() {
    let local = Url::parse("http://localhost:9663").unwrap();
    let player = |link: &str| match super::parse(
        &format!("tell me when ends in classical for {link}"),
        &local,
    ) {
        Some(Ok(Command::Tell(tell))) => tell.player,
        _ => None,
    };
    assert_eq!(player("http://localhost:9663/@/alice"), Some(user("alice")));
    assert_eq!(player("https://lichess.org/@/alice"), None);
}

#[test]
fn typos_in_keywords_events_and_here_are_corrected() {
    for (text, expected) in [
        (
            "tel here when a-game-starts in classical",
            tell(
                Listener::Here,
                GameEventKind::GameStarted,
                "classical",
                None,
            ),
        ),
        (
            "tell hre wen a-game-strats in classical",
            tell(
                Listener::Here,
                GameEventKind::GameStarted,
                "classical",
                None,
            ),
        ),
        (
            "tell me when a gmae is ovr in classical fro alice",
            tell(
                Listener::Me,
                GameEventKind::GameFinished,
                "classical",
                Some("alice"),
            ),
        ),
    ] {
        assert_eq!(told(text), expected, "{text}");
    }
    assert_eq!(parse("hlep"), Some(Ok(Command::Help)));
    assert_eq!(parse("subscripton lsit"), Some(Ok(Command::List)));
    assert_eq!(
        parse("subscription remvoe 3"),
        Some(Ok(Command::Remove(SubscriptionId(3))))
    );
}

#[test]
fn player_names_and_short_words_are_never_corrected() {
    assert_eq!(
        told("tell me when ends in classical for fro").player,
        Some(user("fro"))
    );
    assert_eq!(
        mistake("tell mi when a-game-starts in classical").0,
        Problem::Listener
    );
}

#[test]
fn a_topic_link_keeps_the_case_and_spaces_of_its_channel_and_topic() {
    assert_eq!(
        told("Tell #**Chess Club>Round 1 Games** when A-Game-Starts in Classical"),
        tell(
            Listener::Topic {
                channel: ChannelName::try_new("Chess Club").unwrap(),
                topic: TopicName::try_new("Round 1 Games").unwrap(),
            },
            GameEventKind::GameStarted,
            "classical",
            None
        )
    );
}

#[test]
fn mistakes_name_the_first_wrong_part_and_keep_the_parts_that_were_right() {
    let draft = |listener, event: Option<GameEventKind>, league: Option<&str>| Draft {
        listener,
        event,
        league: league.map(written),
        player: None,
    };
    for (text, problem, expected) in [
        (
            "tell #**chess** when a-game-starts in classical",
            Problem::TopicLink,
            draft(None, Some(GameEventKind::GameStarted), Some("classical")),
        ),
        (
            "tell #**chess>games when a-game-starts in classical",
            Problem::TopicLink,
            draft(None, Some(GameEventKind::GameStarted), Some("classical")),
        ),
        (
            "tell my-team-channel when a-game-starts in classical",
            Problem::Listener,
            draft(None, Some(GameEventKind::GameStarted), Some("classical")),
        ),
        (
            "tell this channel when a-game-starts in classical",
            Problem::Listener,
            draft(None, Some(GameEventKind::GameStarted), Some("classical")),
        ),
        (
            "tell here when a-game-is-scheduled in classical",
            Problem::Event,
            draft(Some(Listener::Here), None, Some("classical")),
        ),
        (
            "tell here when a game starts soon in classical",
            Problem::Event,
            draft(
                Some(Listener::Here),
                Some(GameEventKind::GameStarted),
                Some("classical"),
            ),
        ),
        (
            "tell me when a-game-starts",
            Problem::League,
            draft(Some(Listener::Me), Some(GameEventKind::GameStarted), None),
        ),
        (
            "tell me when a-game-starts in",
            Problem::League,
            draft(Some(Listener::Me), Some(GameEventKind::GameStarted), None),
        ),
        (
            "tell me a-game-starts in classical",
            Problem::Wording,
            draft(
                Some(Listener::Me),
                Some(GameEventKind::GameStarted),
                Some("classical"),
            ),
        ),
        (
            "tell me when a-game-starts classical",
            Problem::Wording,
            draft(
                Some(Listener::Me),
                Some(GameEventKind::GameStarted),
                Some("classical"),
            ),
        ),
        (
            "tell me",
            Problem::Event,
            draft(Some(Listener::Me), None, None),
        ),
        ("tell", Problem::Wording, Draft::default()),
    ] {
        assert_eq!(mistake(text), (problem, expected), "{text}");
    }
}

#[test]
fn a_player_that_is_not_a_lichess_user_id_is_a_player_mistake() {
    for text in [
        "tell me when a-game-starts in classical for a",
        "tell me when a-game-starts in classical for",
        "tell me when a-game-starts in classical for alice and bob",
        "tell me when a-game-starts in classical for https://lichess.org/abcd1234",
    ] {
        assert_eq!(mistake(text).0, Problem::Player, "{text}");
    }
}

#[test]
fn subscription_list_and_remove_are_parsed() {
    assert_eq!(parse("subscription list"), Some(Ok(Command::List)));
    assert_eq!(
        parse("subscription remove 12"),
        Some(Ok(Command::Remove(SubscriptionId(12))))
    );
    assert_eq!(
        parse("subscription remove #12"),
        Some(Ok(Command::Remove(SubscriptionId(12))))
    );
}

#[test]
fn a_subscription_id_that_is_not_a_number_is_an_id_mistake() {
    assert_eq!(parse("subscription remove twelve"), Some(Err(Mistake::Id)));
}

#[test]
fn any_other_subscription_command_is_a_subscription_mistake() {
    for text in [
        "subscription",
        "subscription show",
        "subscription remove",
        "subscription list all",
        "subscription remove 1 2",
    ] {
        assert_eq!(parse(text), Some(Err(Mistake::Subscription)), "{text}");
    }
}

#[test]
fn help_is_parsed() {
    assert_eq!(parse("help"), Some(Ok(Command::Help)));
    assert_eq!(parse("Help me"), Some(Ok(Command::Help)));
}

#[test]
fn other_messages_are_not_subscription_commands() {
    for text in [
        "",
        "   ",
        "hello",
        "who is playing today?",
        "telling me",
        "hell",
    ] {
        assert_eq!(parse(text), None, "{text}");
    }
}
