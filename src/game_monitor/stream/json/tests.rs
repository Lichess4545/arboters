use super::*;

fn phase_of(status_and_winner: &str) -> Result<GamePhase, GameJsonError> {
    let line = format!(
        r#"{{"id":"g1","rated":true,"speed":"blitz",{status_and_winner},"players":{{"white":{{"userId":"alice"}},"black":{{"userId":"bob"}}}}}}"#
    );
    decode(&line).map(|observation| observation.phase)
}

#[test]
fn a_game_line_decodes_into_the_domain_game() {
    let line = r#"{"id":"g1","rated":true,"variant":"standard","speed":"classical","status":20,"statusName":"started","players":{"white":{"userId":"alice","rating":1850},"black":{"userId":"bob","rating":1790}}}"#;
    let player = |name: &str, rating| Player {
        user_id: UserId::try_new(name).unwrap(),
        rating: Some(rating),
    };
    let expected = Game {
        id: GameId::new("g1"),
        white: player("alice", 1850),
        black: player("bob", 1790),
        speed: Speed::Classical,
        rated: true,
    };
    assert_eq!(decode(line).unwrap().game, expected);
}

#[test]
fn status_codes_and_winners_map_to_game_phases() {
    let finished = GamePhase::Finished;
    let cases = [
        (r#""status":10"#, GamePhase::Created),
        (r#""status":20"#, GamePhase::Started),
        (r#""status":25"#, finished(GameOutcome::Aborted)),
        (r#""status":37"#, finished(GameOutcome::Aborted)),
        (
            r#""status":30,"winner":"white""#,
            finished(GameOutcome::WhiteWon),
        ),
        (
            r#""status":31,"winner":"black""#,
            finished(GameOutcome::BlackWon),
        ),
        (r#""status":34"#, finished(GameOutcome::Draw)),
        (r#""status":35"#, finished(GameOutcome::Draw)),
        (r#""status":38"#, finished(GameOutcome::Undecided)),
        (r#""status":39"#, finished(GameOutcome::Draw)),
    ];
    for (status_and_winner, phase) in cases {
        assert_eq!(
            phase_of(status_and_winner).unwrap(),
            phase,
            "{status_and_winner}"
        );
    }
}

#[test]
fn a_status_name_maps_like_its_code() {
    assert_eq!(
        phase_of(r#""status":"noStart""#).unwrap(),
        GamePhase::Finished(GameOutcome::Aborted)
    );
}

#[test]
fn an_unknown_status_code_is_rejected() {
    assert!(matches!(
        phase_of(r#""status":99"#),
        Err(GameJsonError::UnknownStatusCode(99))
    ));
}
