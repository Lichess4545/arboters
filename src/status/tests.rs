use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use kameo::actor::Spawn;

use super::*;
use crate::chat::{ChannelName, DeliveryError};

#[derive(Clone, Debug, Default)]
struct RecordingMessenger {
    deliveries: Arc<Mutex<Vec<(Conversation, OutgoingMessage)>>>,
    unreachable: Arc<AtomicBool>,
}

#[derive(Debug, thiserror::Error)]
#[error("unreachable")]
struct Unreachable;

impl RecordingMessenger {
    fn deliveries(&self) -> Vec<(Conversation, OutgoingMessage)> {
        self.deliveries.lock().unwrap().clone()
    }

    fn posted(&self) -> Vec<String> {
        self.deliveries()
            .into_iter()
            .map(|(_, message)| message.markdown().to_owned())
            .collect()
    }

    fn set_unreachable(&self, unreachable: bool) {
        self.unreachable.store(unreachable, Ordering::SeqCst);
    }
}

impl Messenger for RecordingMessenger {
    fn send(
        &self,
        conversation: &Conversation,
        message: &OutgoingMessage,
    ) -> impl Future<Output = Result<(), DeliveryError>> + Send {
        let outcome = if self.unreachable.load(Ordering::SeqCst) {
            Err(DeliveryError {
                conversation: conversation.clone(),
                cause: Box::new(Unreachable),
            })
        } else {
            self.deliveries
                .lock()
                .unwrap()
                .push((conversation.clone(), message.clone()));
            Ok(())
        };
        std::future::ready(outcome)
    }
}

fn everything_available() -> Health {
    Health {
        postgres: Availability::Available(()),
        heltour: Availability::Available(()),
        lichess: None,
        zulip: Availability::Available(()),
    }
}

fn unavailable_for_now() -> Availability {
    Availability::Unavailable(Unavailable {
        recovery: Recovery::Recoverable,
        error: "answered 503 Service Unavailable".to_owned(),
    })
}

fn channel() -> ChannelName {
    ChannelName::try_new("arboters-status").unwrap()
}

fn configured(messenger: &RecordingMessenger) -> Status<RecordingMessenger> {
    Status::new(messenger.clone(), Some(Config { channel: channel() })).unwrap()
}

fn topic(status: &Status<RecordingMessenger>) -> String {
    match &status.conversation {
        Some(Conversation::Topic { topic, .. }) => topic.as_str().to_owned(),
        other => panic!("status posts to {other:?}"),
    }
}

async fn started(status: Status<RecordingMessenger>) -> ActorRef<Status<RecordingMessenger>> {
    following(status, watch::channel(None).1).await
}

async fn following(
    status: Status<RecordingMessenger>,
    health: watch::Receiver<Option<Health>>,
) -> ActorRef<Status<RecordingMessenger>> {
    let status = Status::spawn((status, health));
    status.wait_for_startup().await;
    status
}

async fn until_posted(messenger: &RecordingMessenger, markdown: &str) {
    tokio::time::timeout(Duration::from_secs(5), async {
        while !messenger.posted().iter().any(|posted| posted == markdown) {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
}

fn today() -> String {
    let now = humantime::format_rfc3339_seconds(SystemTime::now()).to_string();
    now.split_once('T').unwrap().0.to_owned()
}

#[tokio::test]
async fn startup_posts_the_version_and_changelog_to_the_instance_topic_in_the_configured_channel() {
    let messenger = RecordingMessenger::default();
    let status = configured(&messenger);
    let topic = TopicName::try_new(topic(&status)).unwrap();

    started(status).await;

    let version = env!("CARGO_PKG_VERSION");
    assert_eq!(
        messenger.deliveries(),
        [(
            Conversation::Topic {
                channel: channel(),
                topic,
            },
            OutgoingMessage::new(format!(
                ":rocket: arboters v{version} started — [changelog](https://github.com/Lichess4545/arboters/releases/tag/v{version})"
            )),
        )]
    );
}

#[tokio::test]
async fn the_topic_names_the_version_a_fresh_instance_id_and_the_startup_date() {
    let topic = topic(&configured(&RecordingMessenger::default()));

    let parts: Vec<&str> = topic.split(' ').collect();
    let version = format!("v{}", env!("CARGO_PKG_VERSION"));
    assert_eq!(parts.len(), 3, "{topic}");
    assert_eq!(parts[0], version);
    assert_eq!(Uuid::parse_str(parts[1]).unwrap().get_version_num(), 4);
    assert_eq!(parts[2], today());
}

#[tokio::test]
async fn the_topic_fits_within_the_zulip_topic_length_limit() {
    let topic = topic(&configured(&RecordingMessenger::default()));

    assert!(topic.chars().count() <= 60, "{topic} is too long");
}

#[tokio::test]
async fn every_instance_gets_its_own_topic() {
    let messenger = RecordingMessenger::default();

    assert_ne!(
        topic(&configured(&messenger)),
        topic(&configured(&messenger))
    );
}

#[tokio::test]
async fn a_stream_connection_is_posted_with_its_player_count_in_the_instance_topic() {
    let messenger = RecordingMessenger::default();
    let status = started(configured(&messenger)).await;

    status.ask(StreamConnected { players: 42 }).await.unwrap();

    let deliveries = messenger.deliveries();
    assert_eq!(deliveries.len(), 2);
    assert_eq!(deliveries[1].0, deliveries[0].0);
    assert_eq!(
        deliveries[1].1.markdown(),
        ":chess_pawn: Connected to the Lichess game stream, watching 42 players"
    );
}

#[tokio::test]
async fn when_zulip_and_heltour_stop_working_then_only_heltour_is_posted() {
    let messenger = RecordingMessenger::default();
    let health = watch::Sender::new(Some(everything_available()));
    following(configured(&messenger), health.subscribe()).await;
    until_posted(&messenger, ":check: Postgres works\n:check: Heltour works").await;
    let before = messenger.posted().len();

    health.send_replace(Some(Health {
        heltour: unavailable_for_now(),
        zulip: unavailable_for_now(),
        ..everything_available()
    }));

    let warning =
        ":warning: Cannot use Heltour, will keep trying: answered 503 Service Unavailable";
    until_posted(&messenger, warning).await;
    assert_eq!(messenger.posted()[before..], [warning]);
}

#[tokio::test]
async fn when_a_service_works_again_then_it_is_posted_and_a_first_snapshot_posts_what_works() {
    let messenger = RecordingMessenger::default();
    let health = watch::Sender::new(Some(everything_available()));
    following(configured(&messenger), health.subscribe()).await;

    until_posted(&messenger, ":check: Postgres works\n:check: Heltour works").await;
    health.send_replace(Some(Health {
        heltour: unavailable_for_now(),
        ..everything_available()
    }));
    until_posted(
        &messenger,
        ":warning: Cannot use Heltour, will keep trying: answered 503 Service Unavailable",
    )
    .await;
    health.send_replace(Some(everything_available()));
    until_posted(&messenger, ":check: Heltour works again").await;

    assert_eq!(
        messenger.posted()[1..],
        [
            ":check: Postgres works\n:check: Heltour works",
            ":warning: Cannot use Heltour, will keep trying: answered 503 Service Unavailable",
            ":check: Heltour works again",
        ]
    );
}

#[tokio::test(start_paused = true)]
async fn a_requested_shutdown_names_the_signal_and_the_whole_seconds_of_uptime() {
    let messenger = RecordingMessenger::default();
    let status = started(configured(&messenger)).await;
    tokio::time::advance(Duration::from_millis(3_723_400)).await;

    status
        .ask(Shutdown::Requested(Signal::Terminate))
        .await
        .unwrap();

    assert_eq!(
        messenger.posted().last().unwrap(),
        ":octagonal_sign: Shutting down (SIGTERM) after 1h 2m 3s"
    );
}

#[tokio::test(start_paused = true)]
async fn a_failed_shutdown_names_the_error() {
    let messenger = RecordingMessenger::default();
    let status = started(configured(&messenger)).await;
    tokio::time::advance(Duration::from_secs(90)).await;

    status
        .ask(Shutdown::Failed(
            "game monitor stopped unexpectedly".to_owned(),
        ))
        .await
        .unwrap();

    assert_eq!(
        messenger.posted().last().unwrap(),
        ":octagonal_sign: Shutting down after 1m 30s: game monitor stopped unexpectedly"
    );
}

#[tokio::test]
async fn without_a_status_channel_nothing_is_sent() {
    let messenger = RecordingMessenger::default();
    let status = started(Status::new(messenger.clone(), None).unwrap()).await;

    status.ask(StreamConnected { players: 2 }).await.unwrap();
    status
        .ask(Shutdown::Requested(Signal::Interrupt))
        .await
        .unwrap();

    assert_eq!(messenger.deliveries(), []);
}

#[tokio::test]
async fn a_failed_delivery_does_not_stop_the_status() {
    let messenger = RecordingMessenger::default();
    messenger.set_unreachable(true);
    let status = started(configured(&messenger)).await;
    let during_the_outage = status.ask(StreamConnected { players: 1 }).await;
    messenger.set_unreachable(false);

    let after_the_outage = status.ask(StreamConnected { players: 2 }).await;

    assert!(
        during_the_outage.is_ok() && after_the_outage.is_ok(),
        "status stopped: {during_the_outage:?} then {after_the_outage:?}"
    );
    assert_eq!(
        messenger.posted(),
        [":chess_pawn: Connected to the Lichess game stream, watching 2 players"]
    );
}
