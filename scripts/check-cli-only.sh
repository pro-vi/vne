#!/usr/bin/env sh
# The CLI-only build (`--no-default-features`) must not contain the window's
# stack. It is what builds on Linux, where Tauri would need GTK and WebKit
# development packages, and it keeps a tool that handles secrets at about 44
# packages instead of about 600. Extra arguments go to `cargo tree`, so
# `scripts/check-cli-only.sh --features desktop` must fail.
set -eu
cd "$(dirname "$0")/.."

tree=$(cargo tree --manifest-path src-tauri/Cargo.toml --locked --no-default-features "$@" \
  --edges normal,build --prefix none --target all)
found=$(printf '%s\n' "$tree" | grep -E '^(tauri|wry|tao|webkit2gtk|javascriptcore|soup|gtk|gdk|glib|gio|gobject|pango|cairo|atk)[a-z0-9_-]* ' || true)
if [ -n "$found" ]; then
  printf 'check-cli-only: the CLI-only build depends on the window stack:\n%s\n' "$found" >&2
  exit 1
fi
printf 'check-cli-only: no window dependencies (%s packages)\n' "$(printf '%s\n' "$tree" | sort -u | wc -l | tr -d ' ')"
