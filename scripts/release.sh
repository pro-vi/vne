#!/usr/bin/env sh
# Builds the macOS arm64 release archive from a clean checkout of tag
# v<version> and prints the Homebrew formula that installs it.
set -eu
cd "$(dirname "$0")/.."

fail() {
  printf 'release: %s\n' "$1" >&2
  exit 1
}

version=$(sed -n 's/^version = "\(.*\)"$/\1/p' src-tauri/Cargo.toml | head -n 1)
tag="v$version"
commit=$(git rev-parse HEAD)

[ "$(uname -sm)" = "Darwin arm64" ] || fail "build on an Apple Silicon Mac"
tree_status=$(git status --porcelain)
[ -z "$tree_status" ] || fail "the working tree has changes"
[ "$(git rev-parse -q --verify "$tag^{commit}" || true)" = "$commit" ] || fail "HEAD is not tag $tag"

export CARGO_BUILD_JOBS=1
scripts/validate.sh
cargo build --release --locked --manifest-path src-tauri/Cargo.toml --bin vne --features custom-protocol

name="vne-$version-aarch64-apple-darwin"
out=src-tauri/target/release-dist
mkdir -p "$out"
work=$(mktemp -d "$out/.release.XXXXXX")
trap 'rm -rf "$work"' EXIT
trap 'exit 129' HUP
trap 'exit 130' INT
trap 'exit 143' TERM
mkdir -p "$work/$name" "$work/extracted"
cp src-tauri/target/release/vne LICENSE "$work/$name/"

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

printf 'archive: %s\nsha256:  %s\n\n' "$out/$name.tar.gz" "$sha"
cat <<FORMULA
require "json"
require "shellwords"

class Vne < Formula
  desc "Let your coding agent work on .env files it is blocked from reading"
  homepage "https://github.com/pro-vi/vne"
  url "https://github.com/pro-vi/vne/releases/download/$tag/$name.tar.gz"
  sha256 "$sha"
  license "MIT"

  depends_on arch: :arm64
  depends_on :macos

  def install
    bin.install "vne"
  end

  test do
    executable = (bin/"vne").to_s.shellescape
    assert_match "vne $version", shell_output("#{executable} --version")
    payload = "release-synthetic-value"
    (testpath/".env").write("RELEASE_TEST_KEY=#{payload}\\n")
    output = shell_output("#{executable} check .env --json")
    refute_includes output, payload
    parsed = JSON.parse(output)
    refute_includes parsed.inspect, payload
    entry = parsed.fetch("file").fetch("entries").find { |item| item.fetch("key") == "RELEASE_TEST_KEY" }
    assert_equal "set", entry.fetch("valueState")
  end
end
FORMULA
