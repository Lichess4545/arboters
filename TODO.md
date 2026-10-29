# TODO

## League attribution for notifications

Subscriptions are league-scoped and their source is a player, a team, or the whole league. The notifier looks up the two players and the league as sources in every league whose current pairings pair the game's two players, but not team sources. For notifications, any game between two paired players counts, including a casual one. Reporting to heltour does not work that way: the reporter reports only a game that fits the time control, rated setting, and variant of a league that pairs its players.

- Look up the players' teams as sources too.

## Pairings split across stream connections

`/api/stream/games-by-users` includes a game only when both players are on the list, as the Lichess API spec says. The watchlist is both players of every current heltour pairing, so that holds, but the game monitor splits the watchlist into connections of up to 900 users with a token of lichess4545, 500 with a token of any other account, or 300 without one (the limits lila allows), and a pairing whose players land in different connections reaches neither. The watchlist covers every active league, so it often needs more than one connection.

- Chunk by pairing, so both players of a pairing share a connection, or stream the pairings' game ids through `/api/stream/games/{streamId}` (1000 ids authenticated, more can be added while open).

## Heltour pairing and roster adapter

- The watchlist reads heltour's `get_leagues` and `find_pairing`, and the reporter writes `update_pairing`. Still missing: which team a player is on, and who is on a team. That unblocks team sources for notifications.

## Reporting games to heltour

- Not yet ported from Chesster: the warnings about invalid games (the Slack messages and heltour's `game_warning`), and the announcements of game links and results in the league's channels.
- A valid game's link is reported when it starts. If the game then ends in a way that does not count, such as an abort, the link stays on the pairing, as with Chesster.

## Subscription commands

`tell me when ...`, `tell here when ...`, `tell #**channel>topic** when ...`, `subscription list`, `subscription remove <id>`, and `help` work over Zulip direct messages and mentions. The bot must be a Zulip **Generic** bot: an incoming webhook bot can only send messages, so it cannot register the event queue the inbox reads.

- Chesster's `my-team-channel` listener, `my-team` and team-name sources, and the `a-game-is-scheduled` and `a-pairing-is-released` events are still missing. They need the heltour roster adapter and pairing events.
- `tell` accepts any well-formed Lichess username without checking that the player exists or plays in the league.
- Small typos are corrected in the command words, the event, `here`, and league tags, but not in league names, which must match apart from case and spacing.
- Zulip flags a message as mentioning the bot when it mentions a user group the bot belongs to, so such a message that is not a command gets the "send `help`" reply.

## Deployment

- `Deploy` pins `deploy/prod/main.k`, but nothing applies it to the cluster yet. Choose Argo CD with a KCL plugin, Flux on rendered YAML, or `kubectl apply` from CI.
- The pod expects a Secret named `arboters` holding `ARBOTERS_DATABASE__URL`, `ARBOTERS_LICHESS__TOKEN`, `ARBOTERS_HELTOUR__TOKEN`, `ARBOTERS_STATUS__CHANNEL`, the `ARBOTERS_ZULIP__*` settings, and optionally the `OTEL_*` variables.
- The reporter always runs, so production reports games to heltour as soon as this is deployed, alongside Chesster. heltour's strict `update_pairing` refuses a second game or result for a pairing, so whichever reports first wins.
