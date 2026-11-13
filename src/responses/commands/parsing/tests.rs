use super::*;
use crate::chat::MessageId;

const LINK: &str =
    "https://chess.zulipchat.example.com/#narrow/channel/7-chess/topic/games/near/1234";

fn site() -> Url {
    Url::parse("https://chess.zulipchat.example.com").unwrap()
}

fn command(text: &str) -> Action {
    parse(text, &site()).unwrap_or_else(|| panic!("{text:?} is not a response command"))
}

fn phrase(text: &str) -> Phrase {
    Phrase::try_new(text).unwrap()
}

#[test]
fn only_help_and_response_commands_are_recognized() {
    assert_eq!(parse("help", &site()), Some(Action::Help));
    assert_eq!(
        parse("tell me when a-game-starts in classical", &site()),
        None
    );
    assert_eq!(parse("subscription list", &site()), None);
}

fn add(phrase_text: &str, matching: Matching) -> Action {
    Action::Add {
        phrase: phrase(phrase_text),
        matching,
        source: MessageId(1234),
    }
}

#[test]
fn add_takes_a_quoted_phrase_and_a_message_link() {
    assert_eq!(
        command(&format!(r#"response add "Good Game" {LINK}"#)),
        add("Good Game", Matching::AnyCase)
    );
    assert_eq!(
        command(&format!(r#"Response ADD "good   game"   {LINK} "#)),
        add("good game", Matching::AnyCase)
    );
    assert_eq!(
        command(&format!("responses add \u{201c}good game\u{201d} {LINK}")),
        add("good game", Matching::AnyCase)
    );
}

#[test]
fn add_with_exact_before_the_phrase_matches_only_its_case() {
    assert_eq!(
        command(&format!(r#"response add exact "GG" {LINK}"#)),
        add("GG", Matching::ExactCase)
    );
    assert_eq!(
        command(&format!(r#"response add Exact   "GG" {LINK}"#)),
        add("GG", Matching::ExactCase)
    );
    assert_eq!(
        command(&format!(r#"response add "exact" {LINK}"#)),
        add("exact", Matching::AnyCase)
    );
    assert_eq!(
        command(&format!("response add exact gg {LINK}")),
        Action::Mistake(Mistake::Add)
    );
    assert_eq!(
        command(&format!(r#"response add exactly "GG" {LINK}"#)),
        Action::Mistake(Mistake::Add)
    );
}

#[test]
fn add_without_quotes_or_a_link_is_a_mistake() {
    assert_eq!(
        command(&format!("response add gg {LINK}")),
        Action::Mistake(Mistake::Add)
    );
    assert_eq!(
        command(r#"response add "gg""#),
        Action::Mistake(Mistake::Add)
    );
    assert_eq!(
        command(&format!(r#"response add "gg {LINK}"#)),
        Action::Mistake(Mistake::Add)
    );
    assert_eq!(
        command(&format!(r#"response add "  " {LINK}"#)),
        Action::Mistake(Mistake::EmptyPhrase)
    );
}

#[test]
fn add_reports_what_is_wrong_with_the_link() {
    assert_eq!(
        command(r#"response add "gg" https://other.zulipchat.example.com/#narrow/near/1"#),
        Action::Mistake(Mistake::Link(LinkMistake::OtherSite))
    );
    assert_eq!(
        command(r#"response add "gg" lichess.org"#),
        Action::Mistake(Mistake::Link(LinkMistake::NotAMessageLink))
    );
}

#[test]
fn list_takes_an_optional_quoted_phrase() {
    assert_eq!(command("response list"), Action::List(None));
    assert_eq!(
        command(r#"response list "GG""#),
        Action::List(Some(phrase("GG")))
    );
    assert_eq!(command("response list gg"), Action::Mistake(Mistake::List));
    assert_eq!(
        command(r#"response list "gg" extra"#),
        Action::Mistake(Mistake::List)
    );
}

#[test]
fn remove_takes_an_id() {
    assert_eq!(command("response remove 7"), Action::Remove(ResponseId(7)));
    assert_eq!(command("response remove #7"), Action::Remove(ResponseId(7)));
    assert_eq!(
        command("response remove gg"),
        Action::Mistake(Mistake::Remove)
    );
    assert_eq!(command("response remove"), Action::Mistake(Mistake::Remove));
}

#[test]
fn response_alone_or_with_help_asks_for_help() {
    assert_eq!(command("response"), Action::ResponseHelp);
    assert_eq!(command("response help"), Action::ResponseHelp);
    assert_eq!(
        command("response forget 7"),
        Action::Mistake(Mistake::Unknown)
    );
}
