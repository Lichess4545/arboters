[![Build](https://github.com/Lichess4545/arboters/actions/workflows/build.yml/badge.svg?branch=main)](https://github.com/Lichess4545/arboters/actions/workflows/build.yml) [![Release](https://img.shields.io/github/v/release/Lichess4545/arboters?include_prereleases&sort=semver)](https://github.com/Lichess4545/arboters/releases) [![License: AGPL-3.0](https://img.shields.io/github/license/Lichess4545/arboters)](LICENSE)

# Arboters

Arboters is a communications bot for a chess league that runs on [heltour](https://github.com/Lichess4545/heltour). It works with Zulip now, and it may work with other chat apps later. Heltour is the league software behind lichess4545.com. Arboters will replace Chesster, the bot that Lichess4545 uses now.

## Features

- **Game watching.** Arboters watches the Lichess games of the players in the current heltour pairings.
- **Game announcements.** When a player's game starts or finishes, Arboters posts a message in each Zulip topic subscribed to that player.
- **Status messages.** Arboters posts in a status channel when it starts, connects to Lichess, and stops.

## Run it on your machine

You need [Nix](https://nixos.org/download/) and [devenv](https://devenv.sh/) 2.4 or later. devenv installs Rust, Postgres, and the project's own commands, such as `db-migrate` and `run-on-zulip`.

Open a shell in the project:

```sh
devenv shell
```

Start Postgres in the background, then set up the database tables:

```sh
devenv up -d
db-migrate
```

If `db-migrate` fails, wait a few seconds for Postgres to start, then run it again.

Run Arboters:

```sh
cargo run
```

When Arboters starts, the terminal shows `arboters started`. This run has no secrets, so Arboters writes its messages to the terminal instead of posting them to Zulip. Heltour does not send the pairings without a secret, so Arboters watches no games.

Every 30 seconds, the terminal shows the warning `keeping previous watchlist`. This is expected.

To post to a Zulip organization, run `run-on-zulip` instead of `cargo run`. On the first run, it asks you for the secrets that it needs. secretspec, a secrets tool that comes with devenv, saves them outside the repository.

If you use `run-on-zulip`, you can also set `ARBOTERS_STATUS__CHANNEL` to a Zulip channel. Then Arboters posts there when it starts, connects to Lichess, and stops.

To check the formatting and the lint rules, and to run the tests:

```sh
devenv test
```

## Work on it

Read [AGENTS.md](AGENTS.md) before you change the code. It has the rules for the code, all the project commands, and the release steps. If you want to change something, open a pull request against `main`. If you use AI tools, follow [AI_POLICY.md](AI_POLICY.md).

## License

Arboters is free software under the GNU Affero General Public License, version 3. See [LICENSE](LICENSE).
