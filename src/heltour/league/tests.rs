use super::*;

#[test]
fn tags_are_slugs() {
    assert!(Tag::try_new("lone-wolf_2").is_ok());
    assert!(Tag::try_new("45+45").is_err());
    assert!(Tag::try_new(" ").is_err());
}
