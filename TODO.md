# TODO

## League attribution for notifications

Subscriptions are league-scoped and their source is a player, a team, or the whole league. The notifier looks up the two players and the league as sources in every league whose current pairings pair the game's two players, but not team sources. Any game between two paired players counts, including a casual one.

- Look up the players' teams as sources too.

## Pairings split across stream connections

`/api/stream/games-by-users` includes a game only when both players are on the list, as the Lichess API spec says. The watchlist is both players of every current heltour pairing, so that holds, but the game monitor splits the watchlist into connections of up to 900 users with a token of lichess4545, 500 with a token of any other account, or 300 without one (the limits lila allows), and a pairing whose players land in different connections reaches neither. The watchlist covers every active league, so it often needs more than one connection.

- Chunk by pairing, so both players of a pairing share a connection, or stream the pairings' game ids through `/api/stream/games/{streamId}` (1000 ids authenticated, more can be added while open).

## Heltour pairing and roster adapter

- The watchlist reads heltour's `find_pairing` for every active league. Still missing: which team a player is on, and who is on a team. That unblocks team sources for notifications.

## Subscription commands

- There is no way to create a subscription yet. Port Chesster's `tell <listener> when <event> in <league> for <source>`, `subscription list`, and `subscription remove <id>` as Zulip commands.

## Deployment

- `Deploy` pins `deploy/prod/main.k`, but nothing applies it to the cluster yet. Choose Argo CD with a KCL plugin, Flux on rendered YAML, or `kubectl apply` from CI.
- The pod expects a Secret named `arboters` holding `ARBOTERS_DATABASE__URL`, `ARBOTERS_LICHESS__TOKEN`, `ARBOTERS_HELTOUR__TOKEN`, `ARBOTERS_STATUS__CHANNEL`, the `ARBOTERS_ZULIP__*` settings, and optionally the `OTEL_*` variables.
