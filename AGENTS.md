# AGENTS.md

Arboters is a Lichess4545 league bot for Zulip, written in Rust. It is the successor to Chesster.

## Rules

- **No LLM generated comments.** Names and structure are the documentation.
- **Human Comments sparingly.** Names and structure are the documentation.
- **No stringly typed values.** Model domain values as newtypes or enums that validate on construction. Define string and number newtypes with `nutype`.
- **Screaming, hexagonal architecture.** Top-level modules are named for domains (`subscriptions`, `notifications`, `game_monitor`), not for frameworks. Domains define ports as traits. Adapters (`zulip`, the Postgres implementations) implement them. Dependencies point inward.
- **No ORMs.** A data-access layer is a small trait plus an implementation that uses `sqlx` directly with compile-time checked queries.
- **Plain names, no technical jargon.** Name things for what they do in this system (`Subscriptions`, `GameMonitor`, `Notifier`), not for the pattern or layer behind them. Do not add words like `Repository`, `Manager`, `Service`, `Handler`, `Helper`, `Impl`, `Wrapper`, `Factory`, or `Wire` for no reason. A technical word belongs in a name only when it is the most direct description of the thing, as with `postgres`, `http`, or `json`.
- **Tests live beside the code, not inside it.** Public API gets doc tests. Every other test goes in `foo/tests.rs`, declared in `foo.rs` with `#[cfg(test)] mod tests;`. Never write an inline `mod tests { ... }` block.
- **Modules are `foo.rs` beside `foo/`, never `foo/mod.rs`.** `clippy::mod_module_files` enforces it.
- **Prefer integration tests.** Test DAL code against the real database with `#[sqlx::test]`, and test code that uses the DAL against the real DAL too. Never write an in-memory reimplementation of a DAL: it can drift from the SQL. A test double is acceptable only for a failure that cannot be produced for real, and it must not reimplement behavior. Test adapters for external services we cannot run locally (Lichess, Zulip) against a local HTTP server that speaks their protocol.
- **Build only what is needed.** No speculative APIs, endpoints, or trait methods.
- **All configuration comes from environment variables.** Application settings live under `ARBOTERS_`, with `__` separating nesting levels (for example `ARBOTERS_DATABASE__URL`). OpenTelemetry uses the standard `OTEL_*` variables. Secrets are declared in `secretspec.toml` and injected only into the process that needs them with `secretspec run`. devenv's secretspec integration stays off in `devenv.yaml`, so secrets never enter the shell. Never put secrets in `.env` files or commit them.
- **Requests to Lichess and heltour go through `game_monitor::http_client()`**, so every one carries our User-Agent, `arboters/<version> (+https://github.com/Lichess4545/arboters)`.
- **Telemetry uses `tracing`.** It is exported through OpenTelemetry (OTLP) when `OTEL_EXPORTER_OTLP_ENDPOINT` is set. Log as structured fields, not interpolated strings.
- **Strict linting.** Clippy runs with `pedantic`, `nursery`, and restriction lints (no `unwrap`, `expect`, `panic`, or indexing outside tests), all with `-D warnings`. Never silence a lint broadly. If a lint must be suppressed, use a narrowly scoped `#[expect(...)]`.
- **Actors use `kameo`.** Each actor has a single responsibility, and actors talk to each other through `Recipient<M>` rather than concrete actor types.

## Layout

| Path                 | Responsibility                                                                                                                                                               |
| -------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `src/heltour/`       | Lichess4545 league concepts as heltour defines them: `league::Tag`, `league::Name`, `team::Name`, `Pairing`                                                                  |
| `src/lichess/`       | Plain data types Lichess deals in (`lichess::UserId`, `lichess::Game`); depends on nothing else in the crate                                                                 |
| `src/subscriptions/` | Who wants to be told what: the `Subscriptions` trait and its Postgres implementation, which filter game events into Zulip targets                                            |
| `src/notifications/` | Notifier actor and the `Messenger` port                                                                                                                                      |
| `src/game_monitor/`  | Actor that watches the players of heltour's current pairings through the Lichess game stream and emits game events (`GameEvent`, `GameOutcome`)                              |
| `src/status/`        | Status actor that posts startup, Lichess connection, and shutdown messages to a per-instance topic in `ARBOTERS_STATUS__CHANNEL`; without that channel, they are only logged |
| `src/zulip/`         | Zulip adapter implementing `Messenger`; without Zulip settings, notifications are only logged                                                                                |
| `src/app/`           | Wires everything together: config, database pool, `/livez` and `/readyz` probes, telemetry, and the actors                                                                   |
| `migrations/`        | `sqlx` migrations, reversible                                                                                                                                                |
| `deploy/`            | Kubernetes resources in KCL: `arboters/` is the typed workload, `prod/main.k` pins the released version                                                                      |
| `flake.nix`          | The release build and the layered container image (`arboters`, `arboters-migrate`)                                                                                           |
| `ci/`                | Release hooks and the nixpkgs/rust-overlay pin check                                                                                                                         |

## Probes

- Arboters is crash-only. If an actor or the watchlist sync stops, the process exits with an error and Kubernetes restarts it.
- `/livez` answers 200 unless Lichess reports the configured `ARBOTERS_LICHESS__TOKEN` invalid or owned by an account other than lichess4545. `TokenCheck` tests the token at startup and every `ARBOTERS_LICHESS__TOKEN_CHECK_INTERVAL`. A test that fails to reach Lichess keeps the previous answer, so a Lichess outage never fails liveness. Never add other dependencies to it.
- `/readyz` pings the database, bounded by `ARBOTERS_HTTP__READINESS_TIMEOUT`.

## Workflow

Run everything inside `devenv shell`.

- `devenv up` starts the surrounding services (Postgres). It never starts arboters; run that yourself.
- `bacon` runs `check`, `clippy`, `test`, or `run`. Switch jobs with `c`, `t`, and `r`.
- `db-migrate`, `db-new-migration <name>`, `db-reset`
- `subscribe <lichess-user> <zulip-channel> <zulip-topic>` subscribes a Zulip topic to a player's started and finished games in the local database.
- `run-on-zulip` runs migrations, then arboters with its secrets from secretspec. Start the services first with `devenv up -d`. The first run prompts for missing secrets and saves them through your secretspec provider.
- `db-prepare` regenerates `.sqlx/` after any query or schema change. Builds use `SQLX_OFFLINE=true`.
- `devenv test` runs fmt, clippy (`-D warnings`), nextest, and the doc tests.

- `render-manifests` prints the production Kubernetes YAML from KCL. Never hand-write YAML; change the KCL.

Before handing work back, these must be clean: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo nextest run`, and `cargo test --doc`.

## Release and deploy

- Commit subjects on `main` are conventional commits. `feat` asks for a minor bump, other types ask for a patch, and `!` or `BREAKING CHANGE:` asks for a major. Releases read the version from them.
- `release` runs the shared `dull-nix` release command. It sets `[workspace.package]` version, renders `CHANGELOG.md`, tags `v*`, and pushes. Never edit the version or the changelog by hand.
- The `Release` workflow publishes `ghcr.io/lichess4545/arboters:<version>` (plus `latest` for stable releases) and creates the GitHub release. For stable releases, `Deploy` then pins `deploy/prod/main.k` to the new version.
- `deploy <version>` re-pins production to a version that is already published.
- `ci/nixpkgs-pin.sh` requires `flake.lock` and `devenv.lock` to agree on nixpkgs and rust-overlay, so dev, CI, and the container share a toolchain. Update both together.
- Postgres is pinned to major version 18 in `devenv.nix`. Change it only together with production.
