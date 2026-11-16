use kameo::actor::Spawn;
use tokio::sync::mpsc;

use super::*;
use crate::chat::{ChatUserId, CommandText, Conversation, MessageId, Revision, Role};

#[derive(Debug)]
struct Word(&'static str);

#[derive(Debug)]
struct Answerer {
    handling: Handling,
    heard: mpsc::UnboundedSender<&'static str>,
}

impl Actor for Answerer {
    type Args = Self;
    type Error = Infallible;

    fn on_start(
        answerer: Self,
        _: ActorRef<Self>,
    ) -> impl Future<Output = Result<Self, Self::Error>> + Send {
        std::future::ready(Ok(answerer))
    }
}

impl Message<Word> for Answerer {
    type Reply = Handling;

    fn handle(
        &mut self,
        Word(word): Word,
        _: &mut Context<Self, Self::Reply>,
    ) -> impl Future<Output = Handling> + Send {
        self.heard.send(word).unwrap();
        std::future::ready(self.handling.clone())
    }
}

fn answers(markdown: &str) -> Handling {
    Handling::Answered(OutgoingMessage::new(markdown.to_owned()))
}

fn message(text: &str) -> IncomingMessage {
    IncomingMessage {
        id: MessageId(1),
        revision: Revision::Original,
        sender: ChatUserId(10),
        role: Role::Member,
        reply_to: Conversation::Direct {
            user_id: ChatUserId(10),
        },
        text: CommandText::new(text),
    }
}

fn listening(
    word: &'static str,
    handling: Handling,
) -> (Listener, mpsc::UnboundedReceiver<&'static str>) {
    let (heard, received) = mpsc::unbounded_channel();
    let answerer = Answerer::spawn(Answerer { handling, heard });
    let listener = Listener::new(
        move |message: &IncomingMessage| {
            message
                .text
                .split_whitespace()
                .any(|each| each == word)
                .then_some(Word(word))
        },
        answerer.reply_recipient::<Word>(),
    );
    (listener, received)
}

#[tokio::test]
async fn only_listeners_whose_parser_matches_are_asked() {
    let (help, mut help_heard) = listening("help", answers("help"));
    let (tell, mut tell_heard) = listening("tell", answers("tell"));
    let commands = Commands::spawn(Commands::new(vec![help, tell]));

    let reply = commands.ask(message("tell me")).await.unwrap();

    assert_eq!(reply.markdown(), "tell");
    assert_eq!(tell_heard.recv().await, Some("tell"));
    assert!(help_heard.try_recv().is_err());
}

#[tokio::test]
async fn answers_are_joined_in_the_order_of_the_listeners() {
    let (subscriptions, _subscriptions_heard) = listening("help", answers("subscriptions"));
    let (responses, _responses_heard) = listening("help", answers("responses"));
    let commands = Commands::spawn(Commands::new(vec![subscriptions, responses]));

    let reply = commands.ask(message("help")).await.unwrap();

    assert_eq!(reply.markdown(), "subscriptions\n\nresponses");
}

#[tokio::test]
async fn a_message_no_parser_matches_is_unrecognized() {
    let (help, _help_heard) = listening("help", answers("help"));
    let commands = Commands::spawn(Commands::new(vec![help]));

    let reply = commands.ask(message("hello")).await.unwrap();

    assert_eq!(reply.markdown(), UNRECOGNIZED);
}

#[tokio::test]
async fn a_message_every_match_declines_is_unrecognized() {
    let (help, mut help_heard) = listening("help", Handling::Declined);
    let commands = Commands::spawn(Commands::new(vec![help]));

    let reply = commands.ask(message("help")).await.unwrap();

    assert_eq!(reply.markdown(), UNRECOGNIZED);
    assert_eq!(help_heard.recv().await, Some("help"));
}
