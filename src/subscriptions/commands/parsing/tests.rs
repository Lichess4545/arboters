use super::*;

fn told(text: &str) -> Tell {
    match parse(text) {
        Some(Ok(Command::Tell(tell))) => tell,
        other => panic!("{text} parsed as {other:?}"),
    }
}

fn tell(listener: Listener, event: GameEventKind, player: Option<&str>) -> Tell {
    Tell {
        listener,
        event,
        league: league::Tag::try_new("classical").unwrap(),
        player: player.map(|name| lichess::UserId::try_new(name).unwrap()),
    }
}

#[test]
fn tell_me_for_a_player_subscribes_the_sender_to_that_players_games() {
    assert_eq!(
        told("tell me when a-game-starts in classical for alice"),
        tell(Listener::Me, GameEventKind::GameStarted, Some("alice"))
    );
}

#[test]
fn tell_here_for_a_player_subscribes_the_topic() {
    assert_eq!(
        told("tell here when a-game-is-over in classical for alice"),
        tell(Listener::Here, GameEventKind::GameFinished, Some("alice"))
    );
}

#[test]
fn without_a_player_the_subscription_covers_the_whole_league() {
    assert_eq!(
        told("tell here when a-game-starts in classical"),
        tell(Listener::Here, GameEventKind::GameStarted, None)
    );
    assert_eq!(
        told("tell me when a-game-is-over in classical"),
        tell(Listener::Me, GameEventKind::GameFinished, None)
    );
}

#[test]
fn words_match_regardless_of_case_and_spacing() {
    assert_eq!(
        told("  Tell  Me when A-Game-Starts in Classical\nfor Alice "),
        tell(Listener::Me, GameEventKind::GameStarted, Some("alice"))
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
            None
        )
    );
}

#[test]
fn a_topic_link_without_a_topic_is_a_topic_link_mistake() {
    for text in [
        "tell #**chess** when a-game-starts in classical",
        "tell #**chess>** when a-game-starts in classical",
        "tell #**chess>games when a-game-starts in classical",
    ] {
        assert_eq!(parse(text), Some(Err(Mistake::TopicLink)), "{text}");
    }
}

#[test]
fn an_unknown_listener_is_a_listener_mistake() {
    assert_eq!(
        parse("tell my-team-channel when a-game-starts in classical for alice"),
        Some(Err(Mistake::Listener))
    );
    assert_eq!(
        parse("tell this channel when a-game-starts in classical"),
        Some(Err(Mistake::Listener))
    );
}

#[test]
fn an_unknown_event_is_an_event_mistake() {
    assert_eq!(
        parse("tell me when a-game-is-scheduled in classical for alice"),
        Some(Err(Mistake::Event))
    );
}

#[test]
fn a_league_that_is_not_a_tag_is_a_league_mistake() {
    assert_eq!(
        parse("tell me when a-game-starts in lone/wolf"),
        Some(Err(Mistake::League))
    );
}

#[test]
fn a_player_that_is_not_a_lichess_user_id_is_a_player_mistake() {
    assert_eq!(
        parse("tell me when a-game-starts in classical for a"),
        Some(Err(Mistake::Player))
    );
}

#[test]
fn a_tell_with_missing_or_extra_words_is_a_tell_mistake() {
    for text in [
        "tell",
        "tell me",
        "tell me a-game-starts in classical",
        "tell me when a-game-starts",
        "tell me when a-game-starts classical",
        "tell me when a-game-starts in",
        "tell me when a-game-starts in classical for",
        "tell me when a-game-starts in classical about alice",
        "tell me when a-game-starts in classical for alice and bob",
    ] {
        assert_eq!(parse(text), Some(Err(Mistake::Tell)), "{text}");
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
    for text in ["", "   ", "hello", "who is playing today?", "telling me"] {
        assert_eq!(parse(text), None, "{text}");
    }
}
