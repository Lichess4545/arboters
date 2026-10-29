{ pkgs, config, ... }:

let
  database = {
    name = "arboters";
    user = "arboters";
    pass = "arboters";
  };
  databaseUrl = "postgres://${database.user}:${database.pass}@127.0.0.1:${toString config.env.PGPORT}/${database.name}";
in
{
  languages.rust = {
    enable = true;
    channel = "stable";
    components = [ "rustc" "cargo" "clippy" "rustfmt" "rust-analyzer" "rust-src" ];
  };

  packages = [
    pkgs.bacon
    pkgs.cargo-edit
    pkgs.cargo-nextest
    pkgs.gh
    pkgs.git-cliff
    pkgs.jq
    pkgs.kcl
    pkgs.kubeconform
    pkgs.skopeo
    pkgs.sqlx-cli
  ];

  services.postgres = {
    enable = true;
    package = pkgs.postgresql_18;
    listen_addresses = "127.0.0.1";
    initialDatabases = [ database ];
    initialScript = "ALTER ROLE ${database.user} CREATEDB;";
  };

  env.DATABASE_URL = databaseUrl;
  env.ARBOTERS_DATABASE__URL = databaseUrl;
  env.ARBOTERS_LEAGUE = "team4545";
  env.SQLX_OFFLINE = "true";
  env.ARBOTERS_TELEMETRY__LOG_FORMAT = "pretty";
  env.ARBOTERS_WATCHLIST__REFRESH_INTERVAL = "30s";

  scripts.db-migrate.exec = ''sqlx migrate run --source "$DEVENV_ROOT/migrations"'';
  scripts.db-new-migration.exec = ''sqlx migrate add --source "$DEVENV_ROOT/migrations" -r "$@"'';
  scripts.db-prepare.exec = ''cd "$DEVENV_ROOT" && SQLX_OFFLINE=false cargo sqlx prepare -- --all-targets'';
  scripts.db-reset.exec = ''sqlx database reset -y --source "$DEVENV_ROOT/migrations"'';

  scripts.run-on-zulip.exec = ''
    set -eu
    cd "$DEVENV_ROOT"
    if ! pg_isready -q -h 127.0.0.1 -p "$PGPORT"; then
      echo "postgres is not running; start it with: devenv up -d" >&2
      exit 1
    fi
    sqlx migrate run --source migrations
    exec secretspec run -- cargo run
  '';

  git-hooks.hooks.nixpkgs-pin = {
    enable = true;
    entry = "ci/nixpkgs-pin.sh";
    files = "^(flake|devenv)\\.lock$";
    pass_filenames = false;
  };

  scripts.render-manifests.exec = ''cd "$DEVENV_ROOT/deploy" && exec kcl run --quiet prod/main.k'';

  scripts.release.exec = ''cd "$DEVENV_ROOT" && exec nix run "$DEVENV_ROOT#release" -- "$@"'';

  scripts.deploy.exec = ''
    set -eu
    if [ $# -ne 1 ]; then
      echo "usage: deploy <version>" >&2
      exit 2
    fi
    version=''${1#v}
    gh workflow run deploy.yml --repo Lichess4545/arboters -f version="$version"
    echo "Requested a production deploy of $version. Follow it with:"
    echo "  gh run list --repo Lichess4545/arboters --workflow deploy.yml"
    echo "  gh run watch --repo Lichess4545/arboters"
  '';

  enterTest = ''
    cargo fmt --check
    cargo clippy --all-targets -- -D warnings
    cargo nextest run
    cargo test --doc
  '';
}
