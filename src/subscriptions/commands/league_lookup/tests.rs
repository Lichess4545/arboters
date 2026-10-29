use super::*;
use crate::heltour::league;
use crate::lichess::Variant;

fn league(tag: &str, name: &str, time_control: Option<(u32, u32)>) -> League {
    League {
        tag: league::Tag::try_new(tag).unwrap(),
        name: league::Name::try_new(name).unwrap(),
        time_control: time_control.map(|(minutes, increment)| Clock {
            initial_seconds: minutes * 60,
            increment_seconds: increment,
        }),
        variant: Variant::Standard,
        rated: true,
    }
}

fn active() -> ActiveLeagues {
    ActiveLeagues {
        leagues: vec![
            league("team4545", "Team 4545 League", Some((45, 45))),
            league("lonewolf", "Lone Wolf", Some((30, 30))),
            league("chess960", "Chess960 League", Some((45, 45))),
            league("blindfold", "Blindfold League", None),
        ],
        pairings: Vec::new(),
        team_pairings: Vec::new(),
    }
}

fn tags(found: &Found<'_>) -> Vec<String> {
    match found {
        Found::One(league) => vec![league.tag.to_string()],
        Found::Several(leagues) => leagues
            .iter()
            .map(|league| league.tag.to_string())
            .collect(),
        Found::Nothing => Vec::new(),
    }
}

#[test]
fn leagues_are_found_by_tag_name_time_control_or_a_typo_in_the_tag() {
    let active = active();
    for (written, expected) in [
        ("team4545", vec!["team4545"]),
        ("LoneWolf", vec!["lonewolf"]),
        ("lone   wolf", vec!["lonewolf"]),
        ("Team 4545 League", vec!["team4545"]),
        ("30+30", vec!["lonewolf"]),
        ("team4454", vec!["team4545"]),
        ("blindfld", vec!["blindfold"]),
        ("45+45", vec!["team4545", "chess960"]),
        ("15+10", vec![]),
        ("blitz", vec![]),
    ] {
        let found = find(&active, &WrittenLeague::try_new(written).unwrap());
        assert_eq!(tags(&found), expected, "{written}");
    }
}

#[test]
fn a_typo_as_close_to_two_tags_finds_both() {
    let active = ActiveLeagues {
        leagues: vec![
            league("team4545", "Team 4545 League", None),
            league("team4546", "Team 4546 League", None),
        ],
        pairings: Vec::new(),
        team_pairings: Vec::new(),
    };

    let found = find(&active, &WrittenLeague::try_new("team454").unwrap());

    assert_eq!(tags(&found), ["team4545", "team4546"]);
}
