use secrecy::SecretString;

use super::*;

fn config_with_token(token: Option<&str>) -> Config {
    Config {
        token: token.map(SecretString::from),
        ..Config::default()
    }
}

#[test]
fn without_a_token_a_connection_watches_up_to_300_users() {
    let config = config_with_token(None);
    assert_eq!(config.users_per_connection(None).get(), 300);
}

#[test]
fn with_a_token_not_known_to_be_lichess4545s_a_connection_watches_up_to_500_users() {
    let config = config_with_token(Some("lip_secret"));
    let arbiter = UserId::try_new("arbiter").unwrap();
    assert_eq!(
        [None, Some(&arbiter)].map(|owner| config.users_per_connection(owner).get()),
        [500; 2]
    );
}

#[test]
fn with_lichess4545s_token_a_connection_watches_up_to_900_users() {
    let config = config_with_token(Some("lip_secret"));
    let lichess4545 = UserId::try_new("lichess4545").unwrap();
    assert_eq!(config.users_per_connection(Some(&lichess4545)).get(), 900);
}
