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
    pkgs.openssl
    pkgs.pkg-config
    pkgs.sqlx-cli
  ];

  services.postgres = {
    enable = true;
    listen_addresses = "127.0.0.1";
    initialDatabases = [ database ];
    initialScript = "ALTER ROLE ${database.user} CREATEDB;";
  };

  dotenv.enable = true;

  env.DATABASE_URL = databaseUrl;
  env.ARBOTERS_DATABASE__URL = databaseUrl;
  env.SQLX_OFFLINE = "true";
  env.ARBOTERS_TELEMETRY__LOG_FORMAT = "pretty";

  tasks."arboters:migrate" = {
    exec = "sqlx migrate run --source \"$DEVENV_ROOT/migrations\"";
    after = [ "devenv:processes:postgres" ];
    before = [ "devenv:processes:arboters" ];
  };

  processes.arboters = {
    exec = "bacon --headless run";
    ports.http.allocate = 4545;
    env.ARBOTERS_HTTP__PORT = toString config.processes.arboters.ports.http.value;
  };

  scripts.db-migrate.exec = ''sqlx migrate run --source "$DEVENV_ROOT/migrations"'';
  scripts.db-new-migration.exec = ''sqlx migrate add --source "$DEVENV_ROOT/migrations" -r "$@"'';
  scripts.db-prepare.exec = ''cd "$DEVENV_ROOT" && SQLX_OFFLINE=false cargo sqlx prepare -- --all-targets'';
  scripts.db-reset.exec = ''sqlx database reset -y --source "$DEVENV_ROOT/migrations"'';

  enterTest = ''
    cargo fmt --check
    cargo clippy --all-targets -- -D warnings
    cargo nextest run
    cargo test --doc
  '';
}
