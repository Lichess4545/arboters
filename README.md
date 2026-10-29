[![Build](https://github.com/Lichess4545/arboters/actions/workflows/build.yml/badge.svg?branch=main)](https://github.com/Lichess4545/arboters/actions/workflows/build.yml) [![Release](https://img.shields.io/github/v/release/Lichess4545/arboters?include_prereleases&sort=semver)](https://github.com/Lichess4545/arboters/releases) [![License: AGPL-3.0](https://img.shields.io/github/license/Lichess4545/arboters)](LICENSE)

# Arboters

Arboters is a communications bot for a chess league that runs on [heltour](https://github.com/Lichess4545/heltour). It works with Zulip now, and it may work with other chat apps later. Heltour is the league software behind lichess4545.com. Arboters will replace Chesster, the bot that Lichess4545 uses now.

## Features

- **Game watching.** Arboters watches the Lichess games of the players in the current heltour pairings.
- **Game announcements.** When a player's game starts or finishes, Arboters posts a message to each Zulip user and topic subscribed to that player or to the league of the game. For a game in a team match, Arboters also includes the match score, the other boards of the match that are being played, and whether the game can decide the match. When a game finishes before heltour has its result, Arboters adds the result to the score itself.
- **Subscriptions from Zulip.** To manage subscriptions, send Arboters a direct message, or mention it in a Zulip message. `tell me when ...` subscribes you to one player's games, or to every game in a league. Anyone can subscribe a topic: `tell here when ...` subscribes the topic you send it in, and `tell #**channel>topic** when ...` subscribes the topic you name. Moderators can subscribe another Zulip user: `tell @**user** when ...` sends that user direct messages. Subscriptions that post in a topic belong to everyone in that topic: anyone can list or remove them by sending the command there. In the same way, subscriptions that send you direct messages are yours to list and remove, whoever made them. In a topic, `subscription list` shows the subscriptions that post there and who made them. In a direct message, it shows the subscriptions you made and the ones that send you direct messages. `subscription remove <id>` removes a subscription that you made or that posts in the topic where you send it, and moderators can remove any subscription. `help` lists the commands and says what you can do where you ask. If you edit a command, Arboters runs it again and edits its reply, and a subscription from the earlier version is replaced by the new one. Plain wording such as `tell me when a game ends in <league>` works too, and so do small typos. You can name a league by its tag, its name, or its time control, such as `45+45`, if only one active league has that time control.
- **Saved responses.** Moderators can save responses to phrases. When a message in a channel contains a saved phrase as whole words, in any case, Arboters answers in the same topic with one of the phrase's responses, chosen at random. To save a response, a moderator sends Arboters `response add "<phrase>" <message link>`, or `response add exact "<phrase>" <message link>` for a response to the phrase only in exactly that case. Arboters keeps a copy of the linked message, so later edits to that message do not change the response. `response list` shows the saved responses and their ids, marking the exact-case ones, and `response remove <id>` removes one. Only moderators can use these commands, and only moderators see them in `help`. If a moderator edits a `response add` command, the response from the earlier version is replaced by the new one. After Arboters answers a phrase in a topic, it waits 5 seconds before it answers that phrase in that topic again, in any case. To change the wait, set `ARBOTERS_RESPONSES__COOLDOWN`, for example to `30s`. Arboters sees every public channel. To use responses in a private channel, subscribe Arboters to it.
- **Game reports to heltour.** When a game between two paired players starts, Arboters checks its clock, variant, and rated setting against the league's rules. If the game fits them, Arboters sends the game link to heltour, and it sends the result when the game finishes. Heltour decides whether the pairing still needs them.
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
