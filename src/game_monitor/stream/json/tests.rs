use super::*;

fn decoded(fields: &str) -> Result<GameObservation, GameJsonError> {
    decode(&format!(
        r#"{{"id":"g1","rated":true,"variant":"standard","speed":"blitz",{fields},"players":{{"white":{{"userId":"alice"}},"black":{{"userId":"bob"}}}}}}"#
    ))
}

fn phase_of(status_and_winner: &str) -> Result<GamePhase, GameJsonError> {
    decoded(status_and_winner).map(|observation| observation.phase)
}

const fn finished(outcome: GameOutcome, ending: Ending) -> GamePhase {
    GamePhase::Finished { outcome, ending }
}

#[test]
fn status_codes_and_winners_map_to_game_phases() {
    let cases = [
        (r#""status":10"#, GamePhase::Created),
        (r#""status":20"#, GamePhase::Started),
        (
            r#""status":25"#,
            finished(GameOutcome::Aborted, Ending::Aborted),
        ),
        (
            r#""status":30,"winner":"white""#,
            finished(GameOutcome::WhiteWon, Ending::Checkmate),
        ),
        (
            r#""status":31,"winner":"black""#,
            finished(GameOutcome::BlackWon, Ending::Resignation),
        ),
        (
            r#""status":32"#,
            finished(GameOutcome::Draw, Ending::Stalemate),
        ),
        (
            r#""status":33,"winner":"white""#,
            finished(GameOutcome::WhiteWon, Ending::VictoryClaimed),
        ),
        (r#""status":34"#, finished(GameOutcome::Draw, Ending::Draw)),
        (
            r#""status":35"#,
            finished(GameOutcome::Draw, Ending::OutOfTime),
        ),
        (
            r#""status":35,"winner":"black""#,
            finished(GameOutcome::BlackWon, Ending::OutOfTime),
        ),
        (
            r#""status":36,"winner":"black""#,
            finished(GameOutcome::BlackWon, Ending::CheatDetected),
        ),
        (
            r#""status":37"#,
            finished(GameOutcome::Aborted, Ending::NeverStarted),
        ),
        (
            r#""status":38"#,
            finished(GameOutcome::Undecided, Ending::Unknown),
        ),
        (
            r#""status":39"#,
            finished(GameOutcome::Draw, Ending::InsufficientMaterialClaimed),
        ),
        (
            r#""status":60,"winner":"white""#,
            finished(GameOutcome::WhiteWon, Ending::VariantEnd),
        ),
    ];
    for (status_and_winner, phase) in cases {
        assert_eq!(
            phase_of(status_and_winner).unwrap(),
            phase,
            "{status_and_winner}"
        );
    }
}
