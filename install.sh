#!/usr/bin/env sh
# freepi-bridge installer.
#
#   curl -fsSL https://raw.githubusercontent.com/ihsan-ramadhan/freepi-bridge/main/install.sh | sh
#   ./install.sh                        # from a clone: builds locally
#   ./install.sh --from-source          # force a cargo build
#   PREFIX=/usr/local ./install.sh      # custom prefix
#   ./install.sh --version v0.1.0       # pin a release
#
# Order of preference: release binary for this platform, then a local cargo
# build (clone, then a temporary clone). Nothing is written outside $PREFIX/bin.

set -eu

REPO="ihsan-ramadhan/freepi-bridge"
DOWNLOAD_BASE="${FREEPI_BRIDGE_DOWNLOAD_BASE:-https://github.com/$REPO/releases}"
PREFIX="${PREFIX:-$HOME/.local}"
VERSION=""
FROM_SOURCE=0

usage() {
  sed -n '2,12p' "$0" 2>/dev/null || echo "freepi-bridge installer: see README"
}

while [ $# -gt 0 ]; do
  case "$1" in
    --system) PREFIX="${SYSTEM_PREFIX:-/usr/local}" ;;
    --from-source) FROM_SOURCE=1 ;;
    --version)
      [ $# -ge 2 ] || { echo "--version needs a tag, e.g. v0.1.0" >&2; exit 1; }
      VERSION="$2"
      shift
      ;;
    -h|--help) usage; exit 0 ;;
    *) echo "unknown flag: $1" >&2; exit 1 ;;
  esac
  shift
done

say() { printf '%s\n' "$*" >&2; }
die() { say "error: $*"; exit 1; }

in_source_tree() {
  [ -f Cargo.toml ] && [ -d src ] && grep -q 'name = "freepi-bridge"' Cargo.toml 2>/dev/null
}

have() { command -v "$1" >/dev/null 2>&1; }

build_from_source() {
  dir="$1"
  have cargo || die "cargo not found; install Rust (https://rustup.rs) or drop --from-source"
  say "building release binary in $dir ..."
  ( cd "$dir" && cargo build --release --locked </dev/null >/dev/null 2>&1 ) \
    || die "cargo build failed in $dir"
  install -m 0755 "$dir/target/release/freepi-bridge" "$PREFIX/bin/freepi-bridge"
}

install_binary() {
  say "installed $PREFIX/bin/freepi-bridge"
  case ":$PATH:" in
    *":$PREFIX/bin:"*) ;;
    *) say "note: add $PREFIX/bin to PATH: export PATH=\"$PREFIX/bin:\$PATH\"" ;;
  esac
  "$PREFIX/bin/freepi-bridge" --version
}

platform_asset() {
  os=$(uname -s 2>/dev/null || echo unknown)
  arch=$(uname -m 2>/dev/null || echo unknown)
  case "$os" in
    Linux) os_part=linux ;;
    Darwin) os_part=macos ;;
    MINGW*|MSYS*|CYGWIN*)
      die "on Windows use install.ps1 instead: irm https://raw.githubusercontent.com/ihsan-ramadhan/freepi-bridge/main/install.ps1 | iex"
      ;;
    *) die "unsupported OS: $os (use --from-source)" ;;
  esac
  case "$arch" in
    x86_64|amd64) arch_part=x86_64 ;;
    arm64|aarch64) arch_part=aarch64 ;;
    *) die "unsupported architecture: $arch (use --from-source)" ;;
  esac
  printf 'freepi-bridge-%s-%s.tar.gz' "$os_part" "$arch_part"
}

download_release() {
  asset=$(platform_asset)
  if [ -n "$VERSION" ]; then
    url="$DOWNLOAD_BASE/download/$VERSION/$asset"
  else
    url="$DOWNLOAD_BASE/latest/download/$asset"
  fi

  tmp=$(mktemp -d 2>/dev/null || mktemp -d -t freepi-bridge)
  trap 'rm -rf "$tmp"' EXIT INT TERM

  say "downloading $asset ..."
  curl -fsSL --retry 2 --connect-timeout 10 "$url" -o "$tmp/$asset" 2>/dev/null || return 1

  if curl -fsSL --connect-timeout 10 "$url.sha256" -o "$tmp/checksum" 2>/dev/null; then
    expected=$(cut -d' ' -f1 <"$tmp/checksum")
    if have sha256sum; then
      actual=$(sha256sum "$tmp/$asset" | cut -d' ' -f1)
    elif have shasum; then
      actual=$(shasum -a 256 "$tmp/$asset" | cut -d' ' -f1)
    else
      actual=""
    fi
    if [ -n "$actual" ] && [ "$actual" != "$expected" ]; then
      die "checksum mismatch for $asset"
    fi
  fi

  tar -xzf "$tmp/$asset" -C "$tmp" || die "cannot extract $asset"
  [ -f "$tmp/freepi-bridge" ] || die "release archive has no freepi-bridge binary"
  install -d "$PREFIX/bin"
  install -m 0755 "$tmp/freepi-bridge" "$PREFIX/bin/freepi-bridge"
  return 0
}

main() {
  install -d "$PREFIX/bin"

  if in_source_tree; then
    say "source tree detected"
    build_from_source "$(pwd)"
    install_binary
    return 0
  fi

  if [ "$FROM_SOURCE" -eq 0 ] && have curl && download_release; then
    install_binary
    return 0
  fi

  say "no release binary installed; falling back to a source build"
  have git || die "git not found; install git or download a release manually from $DOWNLOAD_BASE"
  tmp=$(mktemp -d 2>/dev/null || mktemp -d -t freepi-bridge)
  trap 'rm -rf "$tmp"' EXIT INT TERM
  say "cloning $REPO ..."
  git clone --depth 1 "https://github.com/$REPO.git" "$tmp/src" </dev/null >/dev/null 2>&1 \
    || die "clone failed; push the repo first, or run this script from a clone"
  build_from_source "$tmp/src"
  install_binary
}

main "$@"
