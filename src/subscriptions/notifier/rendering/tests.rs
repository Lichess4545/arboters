use super::*;
use crate::game_monitor::{Draw, VoidReason, Win};
use crate::lichess;
use crate::testing::alice_vs_bob;

fn lichess() -> Url {
    lichess::Config::default().base_url
}

fn decided(decision: Decision) -> String {
    rendered(&GameChange::Decided {
        game: alice_vs_bob("abcd1234"),
        decision,
    })
}

fn void(reason: VoidReason) -> String {
    rendered(&GameChange::Void {
        game: alice_vs_bob("abcd1234"),
        reason,
    })
}

fn rendered(change: &GameChange) -> String {
    render(change, &lichess()).markdown().to_owned()
}

#[test]
fn a_started_game_announces_white_vs_black_and_the_game_link() {
    assert_eq!(
        render(&GameChange::Started(alice_vs_bob("abcd1234")), &lichess()).markdown(),
        "@_**alice** vs @_**bob** has started: https://lichess.org/game/abcd1234"
    );
}

#[test]
fn a_finished_game_with_a_result_announces_its_chess_score() {
    assert_eq!(
        decided(Decision::WhiteWon(Win::Checkmate)),
        "@_**alice** vs @_**bob** is over. The result is 1-0: https://lichess.org/game/abcd1234"
    );
    assert_eq!(
        decided(Decision::BlackWon(Win::Resignation)),
        "@_**alice** vs @_**bob** is over. The result is 0-1: https://lichess.org/game/abcd1234"
    );
    assert_eq!(
        decided(Decision::Drawn(Draw::AgreementOrRule)),
        "@_**alice** vs @_**bob** is over. The result is ½-½: https://lichess.org/game/abcd1234"
    );
}

#[test]
fn an_aborted_game_is_announced_as_aborted() {
    assert_eq!(
        void(VoidReason::Aborted),
        "@_**alice** vs @_**bob** was aborted: https://lichess.org/game/abcd1234"
    );
}

#[test]
fn the_game_link_points_at_the_configured_lichess_site() {
    let local = Url::parse("http://localhost:9663").unwrap();
    assert_eq!(
        render(&GameChange::Started(alice_vs_bob("abcd1234")), &local).markdown(),
        "@_**alice** vs @_**bob** has started: http://localhost:9663/game/abcd1234"
    );
}
