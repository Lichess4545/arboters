# AGENTS.md

Arboters is a Lichess4545 league bot for Zulip, written in Rust. It is the successor to Chesster.

## Rules

- **No comments.** Names and structure are the documentation.
- **No stringly typed values.** Model domain values as newtypes or enums that validate on construction.
- **Screaming, hexagonal architecture.** Top-level modules are named for domains (`games`, `subscriptions`, `notifications`, `game_monitor`), not for frameworks. Domains define ports as traits. Adapters (`lichess`, `zulip`, Postgres repositories) implement them. Dependencies point inward.
- **No ORMs.** A data-access layer is a small trait plus an implementation that uses `sqlx` directly with compile-time checked queries.
- **Build only what is needed.** No speculative APIs, endpoints, or trait methods.
- **All configuration comes from environment variables.** Application settings live under `ARBOTERS_`, with `__` separating nesting levels (for example `ARBOTERS_DATABASE__URL`). OpenTelemetry uses the standard `OTEL_*` variables.
- **Telemetry uses `tracing`.** It is exported through OpenTelemetry (OTLP) when `OTEL_EXPORTER_OTLP_ENDPOINT` is set. Log as structured fields, not interpolated strings.
- **Strict linting.** Clippy runs with `pedantic`, `nursery`, and restriction lints (no `unwrap`, `expect`, `panic`, or indexing outside tests), all with `-D warnings`. Never silence a lint broadly. If a lint must be suppressed, use a narrowly scoped `#[expect(...)]`.
- **Actors use `kameo`.** Each actor has a single responsibility, and actors talk to each other through `Recipient<M>` rather than concrete actor types.

## Layout

| Path | Responsibility |
| --- | --- |
| `src/games.rs` | Core chess domain: usernames, games, game events |
| `src/subscriptions/` | Who wants to be told what, plus its repository port and Postgres adapter |
| `src/notifications/` | Notifier actor and the `Messenger` port |
| `src/game_monitor/` | Actor that watches Lichess users and emits game events |
| `src/lichess/` | Lichess HTTP adapter (game stream) |
| `src/zulip/` | Zulip adapter implementing `Messenger`; without Zulip settings, notifications are only logged |
| `src/health.rs` | Liveness (in-process components) and readiness (liveness, database, not shutting down) |
| `src/http.rs` | `/livez` and `/readyz` probe endpoints |
| `src/telemetry.rs` | Console logs (JSON, or `pretty` for dev) plus OTLP export |
| `src/watchlist.rs` | Keeps the game monitor's watched users in sync with subscriptions |
| `src/app.rs` | Composition root: builds adapters, spawns actors, serves HTTP |
| `migrations/` | `sqlx` migrations, reversible |

## Probes

- `/livez` checks only in-process state: the game monitor, the notifier, and the watchlist sync. Never add external dependencies to it.
- `/readyz` checks liveness, a database ping bounded by `ARBOTERS_HTTP__READINESS_TIMEOUT`, and fails as soon as SIGTERM arrives.
- On shutdown, readiness flips first, then actors stop gracefully. The HTTP server stops last.

## Workflow

Run everything inside `devenv shell`.

- `devenv up` starts Postgres, runs migrations, and runs arboters under bacon on an allocated port.
- `bacon` runs `check`, `clippy`, `test`, or `run`. Switch jobs with `c`, `t`, and `r`.
- `db-migrate`, `db-new-migration <name>`, `db-reset`
- `db-prepare` regenerates `.sqlx/` after any query or schema change. Builds use `SQLX_OFFLINE=true`.
- `devenv test` runs fmt, clippy (`-D warnings`), nextest, and the doc tests.

Before handing work back, these must be clean: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo nextest run`, and `cargo test --doc`.
