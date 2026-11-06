use kameo::Actor;
use kameo::actor::ActorRef;
use kameo::message::{Context, Message};
use tokio::sync::mpsc;

use crate::game_monitor::{self, Fixture, League};
use crate::heltour::league;
use crate::lichess::{
    Clock, Color, Game, GameId, GameState, GameStatus, Player, Speed, UserId, Variant,
};

pub const CLASSICAL_CLOCK: Clock = Clock {
    initial_seconds: 2700,
    increment_seconds: 45,
};

pub fn user_id(name: &str) -> UserId {
    UserId::try_new(name).unwrap()
}

pub fn user_ids(names: &[&str]) -> Vec<UserId> {
    names.iter().map(|name| user_id(name)).collect()
}

pub fn tag(league: &str) -> league::Tag {
    league::Tag::try_new(league).unwrap()
}

pub fn league(source: &str, key: &str) -> League {
    League {
        source: game_monitor::league::Source::new(source),
        key: game_monitor::league::Key::new(key),
    }
}

pub fn player(name: &str) -> Player {
    Player {
        user_id: user_id(name),
        rating: None,
    }
}

pub fn alice_vs_bob(id: &str) -> Game {
    Game {
        id: GameId::new(id),
        white: player("alice"),
        black: player("bob"),
        speed: Speed::Classical,
        rated: true,
        variant: Variant::Standard,
        clock: None,
    }
}

pub fn alice_beat_bob() -> GameState {
    let rated = |name, rating| Player {
        rating: Some(rating),
        ..player(name)
    };
    GameState {
        game: Game {
            white: rated("alice", 1900),
            black: rated("bob", 1850),
            clock: Some(CLASSICAL_CLOCK),
            ..alice_vs_bob("g1")
        },
        status: GameStatus::Mate {
            winner: Color::White,
        },
    }
}

pub fn fixture(white: &str, black: &str) -> Fixture {
    Fixture {
        white: user_id(white),
        black: user_id(black),
        time_control: Some(CLASSICAL_CLOCK),
        variant: Variant::Standard,
        rated: true,
    }
}

pub struct Collector<T>(pub mpsc::UnboundedSender<T>);

impl<T: Send + 'static> Actor for Collector<T> {
    type Args = Self;
    type Error = std::convert::Infallible;

    fn on_start(
        collector: Self,
        _: ActorRef<Self>,
    ) -> impl Future<Output = Result<Self, Self::Error>> + Send {
        std::future::ready(Ok(collector))
    }
}

impl<T: Send + 'static> Message<T> for Collector<T> {
    type Reply = ();

    async fn handle(&mut self, collected: T, _: &mut Context<Self, Self::Reply>) {
        self.0.send(collected).unwrap();
    }
}
