{
  description = "arboters, the Lichess4545 Zulip bot, as a container image";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-26.05";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    dull-nix.url = "github:dull-ca/nix";
  };

  outputs = { self, nixpkgs, flake-utils, rust-overlay, dull-nix }:
    flake-utils.lib.eachSystem [ "x86_64-linux" ] (system:
      let
        pkgs = import nixpkgs {
          inherit system;
          overlays = [ rust-overlay.overlays.default dull-nix.overlays.default ];
        };
        inherit (pkgs) lib;

        toolchain = pkgs.rust-bin.stable.latest.default;
        rustPlatform = pkgs.makeRustPlatform {
          cargo = toolchain;
          rustc = toolchain;
        };
        version = (lib.importTOML ./Cargo.toml).workspace.package.version;
        httpPort = 8080;

        src = lib.fileset.toSource {
          root = ./.;
          fileset = lib.fileset.unions [ ./Cargo.toml ./Cargo.lock ./clippy.toml ./src ./.sqlx ./migrations ];
        };

        migrations = lib.fileset.toSource {
          root = ./migrations;
          fileset = ./migrations;
        };

        arboters = rustPlatform.buildRustPackage {
          pname = "arboters";
          inherit version src;
          cargoLock.lockFile = ./Cargo.lock;
          env.SQLX_OFFLINE = "true";
          doCheck = false;
          meta.mainProgram = "arboters";
        };

        migrate = pkgs.writeShellApplication {
          name = "arboters-migrate";
          runtimeInputs = [ pkgs.sqlx-cli ];
          text = ''
            DATABASE_URL="$ARBOTERS_DATABASE__URL" exec sqlx migrate run --source ${migrations}
          '';
        };

        container = pkgs.dockerTools.buildLayeredImage {
          name = "arboters";
          tag = "latest";
          contents = [ pkgs.dockerTools.fakeNss pkgs.dockerTools.caCertificates arboters migrate ];
          extraCommands = "mkdir -m 1777 tmp";
          config = {
            Cmd = [ "arboters" ];
            User = "65534:65534";
            ExposedPorts."${toString httpPort}/tcp" = { };
            Env = [
              "HOME=/tmp"
              "SSL_CERT_FILE=/etc/ssl/certs/ca-bundle.crt"
              "ARBOTERS_HTTP__HOST=0.0.0.0"
              "ARBOTERS_HTTP__PORT=${toString httpPort}"
            ];
            Labels = {
              "org.opencontainers.image.source" = "https://github.com/Lichess4545/arboters";
              "org.opencontainers.image.version" = version;
            };
          };
        };

        formatting = pkgs.runCommand "arboters-formatting" { nativeBuildInputs = [ toolchain ]; } ''
          rustfmt --check --edition 2024 ${src}/src/main.rs ${src}/src/lib.rs
          touch $out
        '';
      in
      {
        checks = {
          inherit arboters container formatting;
          release-guards-hold = pkgs.releaseGuardsTest;
        };

        packages = {
          inherit arboters container;
          default = container;
          release = pkgs.mkReleaseCommand {
            repositoryUrl = "https://github.com/Lichess4545/arboters";
            hooks = ./ci/release-hooks.sh;
            releaseWorkflow = "release.yml";
          };
          release-guards = pkgs.releaseGuards;
        };
      });
}
