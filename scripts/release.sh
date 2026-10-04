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
[ -z "$(git status --porcelain)" ] || fail "the working tree has changes"
[ "$(git rev-parse -q --verify "$tag^{commit}" || true)" = "$commit" ] || fail "HEAD is not tag $tag"

npm run build
cargo build --release --locked --manifest-path src-tauri/Cargo.toml --bin vne --features custom-protocol

name="vne-$version-aarch64-apple-darwin"
out=src-tauri/target/release-dist
rm -rf "$out/$name" "$out/$name.tar.gz"
mkdir -p "$out/$name"
cp src-tauri/target/release/vne LICENSE "$out/$name/"

# The binary names the commit it was built from; a mismatch means a stale build.
[ "$("$out/$name/vne" --version)" = "vne $version ($commit)" ] || fail "the built binary does not name $commit"

tar -C "$out" -czf "$out/$name.tar.gz" "$name"
sha=$(shasum -a 256 "$out/$name.tar.gz" | cut -d ' ' -f 1)

printf 'archive: %s\nsha256:  %s\n\n' "$out/$name.tar.gz" "$sha"
cat <<FORMULA
class Vne < Formula
  desc "Let your coding agent work on .env files without reading them"
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
    assert_match "vne $version", shell_output("#{bin}/vne --version")
  end
end
FORMULA
