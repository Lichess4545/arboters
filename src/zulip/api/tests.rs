use super::*;

#[test]
fn roles_follow_the_zulip_role_codes_and_unknown_codes_are_guests() {
    assert_eq!(role_from(100), Role::Owner);
    assert_eq!(role_from(200), Role::Administrator);
    assert_eq!(role_from(300), Role::Moderator);
    assert_eq!(role_from(400), Role::Member);
    assert_eq!(role_from(500), Role::Guest);
    assert_eq!(role_from(600), Role::Guest);
}
