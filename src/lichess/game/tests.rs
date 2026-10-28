use super::*;

#[test]
fn speeds_are_read_in_the_lichess_camel_case_spelling() {
    let speed: Speed = serde_json::from_str("\"ultraBullet\"").unwrap();
    assert_eq!(speed, Speed::UltraBullet);
}
