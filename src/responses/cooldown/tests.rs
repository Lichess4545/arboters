use super::*;

const WAIT: Duration = Duration::from_secs(5);

fn channel(name: &str) -> ChannelName {
    ChannelName::try_new(name).unwrap()
}

fn topic(name: &str) -> TopicName {
    TopicName::try_new(name).unwrap()
}

fn phrase(text: &str) -> Phrase {
    Phrase::try_new(text).unwrap()
}

#[test]
fn a_phrase_does_not_fire_again_in_a_topic_until_the_wait_has_passed() {
    let mut cooldown = Cooldown::new(WAIT);
    let start = Instant::now();
    let fires = |cooldown: &mut Cooldown, at| {
        cooldown.fires(&channel("chess"), &topic("games"), &phrase("gg"), at)
    };

    assert!(fires(&mut cooldown, start));
    assert!(!fires(&mut cooldown, start + Duration::from_secs(1)));
    assert!(!fires(
        &mut cooldown,
        start + WAIT - Duration::from_millis(1)
    ));
    assert!(fires(&mut cooldown, start + WAIT));
    assert!(!fires(&mut cooldown, start + WAIT + Duration::from_secs(1)));
}

#[test]
fn the_wait_is_kept_separately_for_each_topic_and_phrase() {
    let mut cooldown = Cooldown::new(WAIT);
    let now = Instant::now();

    assert!(cooldown.fires(&channel("chess"), &topic("games"), &phrase("gg"), now));
    assert!(cooldown.fires(&channel("chess"), &topic("games"), &phrase("rules"), now));
    assert!(cooldown.fires(&channel("chess"), &topic("rounds"), &phrase("gg"), now));
    assert!(cooldown.fires(&channel("4545"), &topic("games"), &phrase("gg"), now));
}

#[test]
fn phrases_that_differ_only_in_case_share_a_wait() {
    let mut cooldown = Cooldown::new(WAIT);
    let now = Instant::now();

    assert!(cooldown.fires(&channel("chess"), &topic("games"), &phrase("GG"), now));
    assert!(!cooldown.fires(&channel("chess"), &topic("games"), &phrase("gg"), now));
}

#[test]
fn without_a_wait_a_phrase_always_fires() {
    let mut cooldown = Cooldown::new(Duration::ZERO);
    let now = Instant::now();

    assert!(cooldown.fires(&channel("chess"), &topic("games"), &phrase("gg"), now));
    assert!(cooldown.fires(&channel("chess"), &topic("games"), &phrase("gg"), now));
}
