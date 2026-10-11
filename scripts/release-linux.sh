#!/usr/bin/env sh
# Builds a Linux CLI release archive from a clean checkout of tag v<version>:
# a static musl binary without the window (`--no-default-features`), so it runs
# on any Linux distribution and needs no GTK or WebKit. Run it on Linux. It
# builds for this machine's architecture, or pass `aarch64` or `x86_64`. The
# other architecture needs its gcc as the linker (gcc-x86-64-linux-gnu or
# gcc-aarch64-linux-gnu) and a way to run its binaries (qemu-user-binfmt),
# because the extracted binary is checked by running it.
# Needs the target installed: rustup target add <arch>-unknown-linux-musl
set -eu
cd "$(dirname "$0")/.."

fail() {
  printf 'release-linux: %s\n' "$1" >&2
  exit 1
}

version=$(sed -n 's/^version = "\(.*\)"$/\1/p' src-tauri/Cargo.toml | head -n 1)
tag="v$version"
commit=$(git rev-parse HEAD)

[ "$(uname -s)" = "Linux" ] || fail "build on Linux"
host=$(uname -m)
arch=${1:-$host}
case "$arch" in
  aarch64 | x86_64) ;;
  *) fail "architecture must be aarch64 or x86_64" ;;
esac
target="$arch-unknown-linux-musl"
tree_status=$(git status --porcelain)
[ -z "$tree_status" ] || fail "the working tree has changes"
[ "$(git rev-parse -q --verify "$tag^{commit}" || true)" = "$commit" ] || fail "HEAD is not tag $tag"

export CARGO_BUILD_JOBS=1
export RUST_TEST_THREADS=1
if [ "$arch" != "$host" ]; then
  linker="$arch-linux-gnu-gcc"
  command -v "$linker" >/dev/null 2>&1 || fail "a $arch build on a $host machine needs $linker"
  upper=$(printf '%s' "$target" | tr 'a-z-' 'A-Z_')
  export "CARGO_TARGET_${upper}_LINKER=$linker"
fi

scripts/check-cli-only.sh
cargo test --locked --no-default-features --manifest-path src-tauri/Cargo.toml
cargo build --release --locked --no-default-features --manifest-path src-tauri/Cargo.toml --bin vne --target "$target"

name="vne-$version-$target"
out=src-tauri/target/release-dist
mkdir -p "$out"
work=$(mktemp -d "$out/.release.XXXXXX")
trap 'rm -rf "$work"' EXIT
trap 'exit 129' HUP
trap 'exit 130' INT
trap 'exit 143' TERM
mkdir -p "$work/$name" "$work/extracted"
cp "src-tauri/target/$target/release/vne" LICENSE "$work/$name/"

tar -C "$work" -czf "$work/$name.tar.gz" "$name"
tar -xzf "$work/$name.tar.gz" -C "$work/extracted"
node scripts/check-release.mjs "$work/extracted/$name/vne" "vne $version ($commit)"

checksum=$(shasum -a 256 "$work/$name.tar.gz")
sha=${checksum%% *}
[ "$(git rev-parse HEAD)" = "$commit" ] || fail "HEAD changed during release preparation"
tree_status=$(git status --porcelain)
[ -z "$tree_status" ] || fail "the working tree changed during release preparation"
[ "$(git rev-parse -q --verify "$tag^{commit}" || true)" = "$commit" ] || fail "tag $tag changed during release preparation"
node --input-type=module -e 'import { renameSync } from "node:fs"; renameSync(process.argv[1], process.argv[2]);' \
  "$work/$name.tar.gz" "$out/$name.tar.gz"

printf 'archive: %s\nsha256:  %s\n' "$out/$name.tar.gz" "$sha"
