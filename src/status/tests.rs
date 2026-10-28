use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use kameo::actor::Spawn;

use super::*;
use crate::notifications::DeliveryError;

#[derive(Clone, Debug, Default)]
struct RecordingMessenger {
    deliveries: Arc<Mutex<Vec<(NotificationTarget, Notification)>>>,
    unreachable: Arc<AtomicBool>,
}

#[derive(Debug, thiserror::Error)]
#[error("unreachable")]
struct Unreachable;

impl RecordingMessenger {
    fn deliveries(&self) -> Vec<(NotificationTarget, Notification)> {
        self.deliveries.lock().unwrap().clone()
    }

    fn posted(&self) -> Vec<String> {
        self.deliveries()
            .into_iter()
            .map(|(_, notification)| notification.markdown().to_owned())
            .collect()
    }

    fn set_unreachable(&self, unreachable: bool) {
        self.unreachable.store(unreachable, Ordering::SeqCst);
    }
}

impl Messenger for RecordingMessenger {
    fn send(
        &self,
        target: &NotificationTarget,
        notification: &Notification,
    ) -> impl Future<Output = Result<(), DeliveryError>> + Send {
        let outcome = if self.unreachable.load(Ordering::SeqCst) {
            Err(DeliveryError {
                target: target.clone(),
                cause: Box::new(Unreachable),
            })
        } else {
            self.deliveries
                .lock()
                .unwrap()
                .push((target.clone(), notification.clone()));
            Ok(())
        };
        std::future::ready(outcome)
    }
}

fn channel() -> ChannelName {
    ChannelName::try_new("arboters-status").unwrap()
}

fn configured(messenger: &RecordingMessenger) -> Status<RecordingMessenger> {
    Status::new(
        messenger.clone(),
        Some(StatusSettings { channel: channel() }),
    )
    .unwrap()
}

fn topic(status: &Status<RecordingMessenger>) -> String {
    match &status.target {
        Some(NotificationTarget::Channel { topic, .. }) => topic.as_str().to_owned(),
        other => panic!("status posts to {other:?}"),
    }
}

async fn started(status: Status<RecordingMessenger>) -> ActorRef<Status<RecordingMessenger>> {
    let status = Status::spawn(status);
    status.wait_for_startup().await;
    status
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
            NotificationTarget::Channel {
                channel: channel(),
                topic,
            },
            Notification::new(format!(
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
