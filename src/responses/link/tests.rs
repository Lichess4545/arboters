use super::*;

fn site() -> Url {
    Url::parse("https://chess.zulipchat.com").unwrap()
}

fn linked(text: &str) -> Result<MessageId, LinkMistake> {
    linked_message(text, &site())
}

#[test]
fn a_near_link_names_its_message() {
    assert_eq!(
        linked("https://chess.zulipchat.com/#narrow/channel/7-chess/topic/games/near/1234"),
        Ok(MessageId(1234))
    );
}

#[test]
fn a_with_link_names_its_message() {
    assert_eq!(
        linked("https://chess.zulipchat.com/#narrow/channel/7-chess/topic/games/with/1234"),
        Ok(MessageId(1234))
    );
}

#[test]
fn links_from_older_zulip_versions_and_direct_messages_name_their_message() {
    assert_eq!(
        linked("https://chess.zulipchat.com/#narrow/stream/7-chess/topic/games/near/55"),
        Ok(MessageId(55))
    );
    assert_eq!(
        linked("https://chess.zulipchat.com/#narrow/dm/10-alice/near/56"),
        Ok(MessageId(56))
    );
}

#[test]
fn a_message_link_that_zulip_formatted_while_pasting_names_its_message() {
    assert_eq!(linked("#**chess>games@1234**"), Ok(MessageId(1234)));
    assert_eq!(linked("#**chess>who @ where@77**"), Ok(MessageId(77)));
}

#[test]
fn a_link_to_another_site_is_refused() {
    assert_eq!(
        linked("https://other.zulipchat.com/#narrow/channel/7-chess/topic/games/near/1234"),
        Err(LinkMistake::OtherSite)
    );
    assert_eq!(
        linked("http://chess.zulipchat.com/#narrow/channel/7-chess/topic/games/near/1234"),
        Err(LinkMistake::OtherSite)
    );
}

#[test]
fn links_that_do_not_name_a_message_are_refused() {
    for text in [
        "gg",
        "https://chess.zulipchat.com/",
        "https://chess.zulipchat.com/#narrow/channel/7-chess/topic/games",
        "https://chess.zulipchat.com/#narrow/channel/7-chess/topic/games/near/latest",
        "#**chess>games**",
        "#**chess**",
    ] {
        assert_eq!(linked(text), Err(LinkMistake::NotAMessageLink), "{text}");
    }
}
