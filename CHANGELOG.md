# Changelog

Rendered by [git-cliff](https://git-cliff.org) from the conventional commits
behind each tag. `release` rewrites this file in full on every release, so an
edit made here is lost — edit the commit messages instead.

## v0.1.0 — 2026-10-29

### Features

- scaffold arboters with devenv, env config, and domain contracts ([8a76fa1](https://github.com/Lichess4545/arboters/commit/8a76fa1ff28a9a9a98d0770906172538d75f1bf2))
- add game monitor and notifier actors with kube probes and OTLP telemetry ([0adf764](https://github.com/Lichess4545/arboters/commit/0adf764f0e4203cb106dd5132a84bf57199d777d))

### Fixes

- **deploy**: keep render-manifests stdout to the manifests on a cold kcl cache ([#2](https://github.com/Lichess4545/arboters/pull/2)) ([970dfe4](https://github.com/Lichess4545/arboters/commit/970dfe45c6d28759d93915ba6eb1c7706b5aeef5))
- link games at lichess.org/game/<id> ([#3](https://github.com/Lichess4545/arboters/pull/3)) ([c53901a](https://github.com/Lichess4545/arboters/commit/c53901a2acae3ba80a1af7968ba98af6c17d6914))

### Documentation

- track open work in TODO.md ([6b752e7](https://github.com/Lichess4545/arboters/commit/6b752e78616fdc40a8f5a5dea8b7e6f092b8754c))
- add naming, newtype, and testing rules to AGENTS.md ([360f619](https://github.com/Lichess4545/arboters/commit/360f61971cb3a42fdf8cc3ee34ae1bc1cfc1cfc7))
- Add a README.md and an AI_POLICY.md ([#4](https://github.com/Lichess4545/arboters/pull/4)) ([f1852f7](https://github.com/Lichess4545/arboters/commit/f1852f7b0f1132f8a956678b461abd10d86edd59))

### Tooling

- build, release, and deploy arboters as a nix container on kubernetes ([2406460](https://github.com/Lichess4545/arboters/commit/24064605c207472e09fceced17e2335a55c9d11c))
- inject Zulip secrets with secretspec and add run-on-zulip and subscribe scripts ([fd3488a](https://github.com/Lichess4545/arboters/commit/fd3488a3d525d4e3e5c27104ae9661cf1bd3f570))
- drop the subscribe dev script ([#1](https://github.com/Lichess4545/arboters/pull/1)) ([cebca3f](https://github.com/Lichess4545/arboters/commit/cebca3fcde79fab7d7b2c449d176520faffbe365))

