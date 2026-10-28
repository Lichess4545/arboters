#!/usr/bin/env bash
set -euo pipefail

cd "$(git rev-parse --show-toplevel)"

rev() {
  jq -r --arg input "$2" '.nodes[.nodes.root.inputs[$input]].locked.rev' "$1"
}

status=0
for input in nixpkgs rust-overlay; do
  flake=$(rev flake.lock "$input")
  devenv=$(rev devenv.lock "$input")
  if [[ $flake != "$devenv" ]]; then
    printf '%s pins differ: flake.lock %s, devenv.lock %s\n' "$input" "$flake" "$devenv" >&2
    status=1
  fi
done
if ((status != 0)); then
  printf 'Run: nix flake update nixpkgs rust-overlay, then devenv update nixpkgs rust-overlay, until both match.\n' >&2
fi
exit "$status"
