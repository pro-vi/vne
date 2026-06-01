#!/usr/bin/env sh
set -eu

fail=0

check_no_matches() {
  label="$1"
  shift

  matches="$("$@" || true)"
  if [ -n "$matches" ]; then
    printf '%s\n' "security-check: failed $label" >&2
    printf '%s\n' "$matches" >&2
    fail=1
  fi
}

check_no_matches \
  "network client API scan" \
  rg -n \
  -e 'fetch\s*\(' \
  -e 'XMLHttpRequest' \
  -e 'WebSocket' \
  -e 'EventSource' \
  -e 'sendBeacon' \
  src src-tauri/src

check_no_matches \
  "telemetry SDK scan" \
  rg -n \
  -e '@sentry/' \
  -e 'sentry::' \
  -e 'Sentry::' \
  -e 'posthog' \
  -e 'datadog' \
  -e 'amplitude' \
  -e 'mixpanel' \
  -e 'analytics\.track' \
  package.json src src-tauri/Cargo.toml src-tauri/src

check_no_matches \
  "raw logging API scan" \
  rg -n \
  -e 'console\.(log|debug|info|warn|error)' \
  -e 'println!' \
  -e 'eprintln!' \
  -e 'dbg!' \
  -e 'tracing::' \
  -e '(^|[^[:alnum:]_])log::' \
  src src-tauri/src

check_no_matches \
  "updater surface scan" \
  rg -n \
  -e 'tauri-plugin-updater' \
  -e '"updater"' \
  package.json src-tauri/Cargo.toml src-tauri/tauri.conf.json

check_no_matches \
  "broad dialog permission scan" \
  rg -n \
  -e 'dialog:default' \
  -e 'dialog:allow-save' \
  -e 'dialog:allow-message' \
  -e 'dialog:allow-ask' \
  -e 'dialog:allow-confirm' \
  src-tauri/capabilities/default.json

dev_url="$(rg -o '"devUrl"\s*:\s*"[^"]+"' src-tauri/tauri.conf.json || true)"
case "$dev_url" in
  *127.0.0.1* | *localhost* | '')
    ;;
  *)
    printf '%s\n' "security-check: failed non-local Tauri devUrl" >&2
    printf '%s\n' "$dev_url" >&2
    fail=1
    ;;
esac

if rg -q '"csp"\s*:\s*null' src-tauri/tauri.conf.json; then
  printf '%s\n' "security-check: note csp is null; threat model tracks this release hardening item" >&2
fi

if [ "$fail" -ne 0 ]; then
  exit 1
fi

printf '%s\n' "security-check: passed"
