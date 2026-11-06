mod config;

use kameo::actor::Recipient;
use time::OffsetDateTime;
use tokio::sync::watch;
use tokio::time::MissedTickBehavior;

use crate::lichess::{self, GameState, GamesUserError, GamesUserQuery};

use super::fixtures::{Fixture, Fixtures};

pub use config::Config;

pub async fn keep_rechecking(
    lichess: lichess::Api,
    active_fixtures: watch::Receiver<Vec<Fixtures>>,
    games: Recipient<GameState>,
    config: Config,
) {
    let mut rechecks = tokio::time::interval(config.interval);
    rechecks.set_missed_tick_behavior(MissedTickBehavior::Skip);
    loop {
        rechecks.tick().await;
        let fixtures = active_fixtures.borrow().clone();
        recheck(&lichess, &fixtures, &games, config).await;
    }
}

async fn recheck(
    lichess: &lichess::Api,
    active: &[Fixtures],
    games: &Recipient<GameState>,
    config: Config,
) {
    for fixtures in active {
        for fixture in &fixtures.games {
            recheck_fixture(lichess, fixture, fixtures.played_since(), games, config).await;
        }
    }
}

async fn recheck_fixture(
    lichess: &lichess::Api,
    fixture: &Fixture,
    since: OffsetDateTime,
    games: &Recipient<GameState>,
    config: Config,
) {
    match lichess
        .games_user(
            &fixture.white,
            &finished_games_against(&fixture.black, since),
            config.interval,
        )
        .await
    {
        Ok(finished) => {
            for game in finished {
                if let Err(error) = games.tell(game).await {
                    tracing::warn!(error = ?error, "could not hand a rechecked game to the game monitor");
                }
            }
        }
        Err(GamesUserError::RateLimited) => {
            tracing::warn!(retry_in = ?config.rate_limit_backoff, white = %fixture.white, black = %fixture.black, "rate limited by lichess while rechecking finished games");
            tokio::time::sleep(config.rate_limit_backoff).await;
        }
        Err(error @ GamesUserError::Undecodable { .. }) => {
            tracing::error!(error = ?error, white = %fixture.white, black = %fixture.black, "could not read the finished games");
        }
        Err(error) => {
            tracing::warn!(error = ?error, white = %fixture.white, black = %fixture.black, "could not recheck finished games");
        }
    }
}

fn finished_games_against(black: &lichess::UserId, since: OffsetDateTime) -> GamesUserQuery {
    GamesUserQuery {
        vs: black.clone(),
        since,
    }
}

#[cfg(test)]
mod tests;
