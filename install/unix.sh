#!/bin/bash
set -euo pipefail

REPO_URL="https://github.com/Thunboo/newhatch"
INSTALL_DIR="${NEWHATCHCTL_INSTALL_DIR:-/usr/local/bin}"
BINARY="$INSTALL_DIR/newhatchctl"
tmp_dir=""
staged_binary=""
use_sudo=false

fail() {
  printf 'Error: %s\n' "$*" >&2
  exit 1
}

fetch() {
  curl --disable --fail --silent --show-error --location \
    --proto '=https' --proto-redir '=https' \
    --connect-timeout 15 --max-time 300 --retry 3 "$@" </dev/null
}

as_root() {
  if [ "$use_sudo" = true ]; then
    sudo "$@" </dev/null
  else
    "$@" </dev/null
  fi
}

cleanup() {
  if [ -n "$staged_binary" ]; then
    as_root rm -f -- "$staged_binary" || true
  fi
  if [ -n "$tmp_dir" ]; then
    rm -rf -- "$tmp_dir" || true
  fi
}

detect_system() {
  case "$(uname -s)" in
    Linux) os_name=linux ;;
    Darwin) os_name=macos ;;
    *) fail 'Supported operating systems: Linux and macOS.' ;;
  esac
  case "$(uname -m)" in
    x86_64|amd64) arch=amd64 ;;
    aarch64|arm64) arch=arm64 ;;
    *) fail 'Supported architectures: amd64 and arm64.' ;;
  esac
  [[ "$INSTALL_DIR" = /* ]] || fail 'NEWHATCHCTL_INSTALL_DIR must be an absolute path.'
  [ ! -d "$BINARY" ] || fail "Installation path is a directory: $BINARY"
}

get_latest_version() {
  local url
  printf 'Fetching latest release for %s-%s...\n' "$os_name" "$arch"
  url=$(fetch --output /dev/null --write-out '%{url_effective}' "$REPO_URL/releases/latest") || fail 'Could not find the latest release.'
  [[ "$url" = "$REPO_URL/releases/tag/"* ]] || fail 'GitHub did not return a published release.'
  tag="${url##*/}"
  [[ "$tag" =~ ^newhatchctl-v[0-9]+\.[0-9]+\.[0-9]+$ ]] || fail "Latest release is not a stable newhatchctl version: $tag"
  version="${tag#newhatchctl-v}"
}

check_existing() {
  local current
  [ -x "$BINARY" ] || return 0
  current=$("$BINARY" --version 2>/dev/null </dev/null) || current=""
  if [ "$current" = "newhatchctl $version" ]; then
    printf 'newhatchctl %s is already installed.\n' "$version"
    exit 0
  fi
}

download_binary() {
  local url="$REPO_URL/releases/download/$tag"
  tmp_dir=$(mktemp -d "${TMPDIR:-/tmp}/newhatchctl-install.XXXXXX")
  archive="newhatchctl-$os_name-$arch.tar.gz"
  printf 'Downloading newhatchctl %s...\n' "$version"
  fetch --output "$tmp_dir/$archive" "$url/$archive" || fail "Could not download $archive."
  fetch --output "$tmp_dir/SHA256SUMS" "$url/SHA256SUMS" || fail 'Could not download SHA256SUMS.'
}

verify_checksum() {
  local expected actual
  expected=$(awk -v archive="$archive" \
    '$2 == archive && NF == 2 { print $1; count++ } END { if (count != 1) exit 1 }' "$tmp_dir/SHA256SUMS") || fail "Missing or duplicate checksum for $archive."
  [[ "$expected" =~ ^[0-9a-f]{64}$ ]] || fail 'Invalid SHA256 checksum.'
  if command -v sha256sum >/dev/null 2>&1; then
    actual=$(sha256sum < "$tmp_dir/$archive")
  else
    actual=$(shasum -a 256 < "$tmp_dir/$archive")
  fi
  [ "${actual%% *}" = "$expected" ] || fail 'SHA256 verification failed.'
}

extract_binary() {
  local current
  [ "$(tar -tzf "$tmp_dir/$archive")" = newhatchctl ] || fail 'Archive must contain only newhatchctl.'
  [[ "$(LC_ALL=C tar -tvzf "$tmp_dir/$archive")" = -* ]] || fail 'Archive must contain a regular executable.'
  tar -xzf "$tmp_dir/$archive" -C "$tmp_dir" newhatchctl
  chmod 755 "$tmp_dir/newhatchctl"
  current=$("$tmp_dir/newhatchctl" --version </dev/null) || fail 'Downloaded executable cannot run on this host.'
  [ "$current" = "newhatchctl $version" ] || fail 'Executable version does not match the release tag.'
}

install_binary() {
  local parent="$INSTALL_DIR"
  while [ ! -d "$parent" ]; do
    parent=$(dirname "$parent")
  done
  if [ ! -w "$parent" ] && [ "$(id -u)" != 0 ]; then
    use_sudo=true
    printf 'Installing into %s requires sudo.\n' "$INSTALL_DIR"
  fi
  as_root mkdir -p -- "$INSTALL_DIR"
  staged_binary=$(as_root mktemp "$INSTALL_DIR/.newhatchctl.XXXXXX")
  as_root install -m 755 "$tmp_dir/newhatchctl" "$staged_binary" || fail 'Could not stage executable; existing installation was not changed.'
  as_root mv -f -- "$staged_binary" "$BINARY" || fail 'Could not replace executable; existing installation was not changed.'
  staged_binary=""
}

main() {
  detect_system
  get_latest_version
  check_existing
  download_binary
  verify_checksum
  extract_binary
  install_binary
  printf 'Installed newhatchctl %s at %s.\nRun: newhatchctl\n' "$version" "$BINARY"
  case ":${PATH:-}:" in
    *":$INSTALL_DIR:"*) ;;
    *) printf 'Add %s to PATH.\n' "$INSTALL_DIR" ;;
  esac
}

trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
main "$@"
