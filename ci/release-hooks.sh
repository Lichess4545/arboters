#!/usr/bin/env bash
set -euo pipefail

readonly published_image=ghcr.io/lichess4545/arboters
readonly pin_pattern='^(\s*image_version = ")([^"]*)(")$'
readonly prod_manifest=deploy/prod/main.k

refuse() {
  if [[ ${GITHUB_ACTIONS-} == true ]]; then
    printf '::error::%s\n' "$*" >&2
  else
    printf 'refusing to release: %s\n' "$*" >&2
  fi
  return 1
}

assert_ready() {
  release-guards cargo-workspace-version <Cargo.toml >/dev/null
  command -v cargo >/dev/null 2>&1 \
    || refuse 'cargo is missing; enter the devenv shell'
  assert_pinned
}

assert_pinned() {
  (($(grep -cE "$pin_pattern" "$prod_manifest") == 1)) \
    || refuse "$prod_manifest must have exactly one 'image_version = \"...\"' line"
}

is_published() {
  local reference="docker://$published_image:${1#v}"
  local -a skopeo=(skopeo) authfile=()
  command -v skopeo >/dev/null 2>&1 || skopeo=(nix run nixpkgs#skopeo --)
  if [[ -n ${GHCR_AUTHFILE-} ]]; then authfile=(--authfile "$GHCR_AUTHFILE"); fi
  "${skopeo[@]}" inspect --no-tags "${authfile[@]}" "$reference" >/dev/null 2>&1
}

assert_unpublished() {
  local version=${1-}
  is_published "$version" || return 0
  refuse "$published_image:${version#v} is already published"
}

assert_published() {
  local version=${1-}
  is_published "$version" && return 0
  refuse "$published_image:${version#v} is not published"
}

deployed_version() {
  assert_pinned
  sed -n -E "s|$pin_pattern|\\2|p" "$prod_manifest"
}

set_prod_image() {
  local version=${1#v}
  [[ $(deployed_version) == "$version" ]] && return 0
  sed -i -E "s|$pin_pattern|\\1$version\\3|" "$prod_manifest"
  printf '%s\n' "$prod_manifest"
}

describe() {
  local version=v${1#v} latest
  if release-guards is-stable "$version"; then
    latest="moves to $version"
  else
    latest="unchanged -- $version is a prerelease"
  fi
  printf '%-9s %s:%s\n' image "$published_image" "${version#v}"
  printf '%-9s %s\n' ':latest' "$latest"
  if release-guards is-stable "$version"; then
    printf '%-9s %s\n' prod "deploy.yml pins $prod_manifest to ${version#v} after publishing"
  else
    printf '%-9s %s\n' prod "$prod_manifest unchanged"
  fi
}

set_version() {
  local version=${1#v} updated
  assert_ready
  updated=$(release-guards set-cargo-workspace-version "$version" <Cargo.toml) || return 1
  printf '%s\n' "$updated" >Cargo.toml
  cargo update --workspace --offline --quiet
  printf 'Cargo.toml\nCargo.lock\n'
}

case ${1-} in
  assert-ready) assert_ready ;;
  assert-unpublished) assert_unpublished "${2-}" ;;
  assert-published) assert_published "${2-}" ;;
  deployed-version) deployed_version ;;
  set-prod-image) set_prod_image "${2-}" ;;
  describe) describe "${2-}" ;;
  set-version) set_version "${2-}" ;;
  image) printf '%s\n' "$published_image" ;;
  *)
    printf 'usage: release-hooks {assert-ready|assert-unpublished|assert-published|describe|set-version|deployed-version|set-prod-image|image} ARGUMENT\n' >&2
    exit 2
    ;;
esac
