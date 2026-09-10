#!/usr/bin/env sh
set -eu

# Fail closed when the scanner is missing: every check_no_matches scan below
# silently passes if rg cannot run, so absence of rg must stop the gate before
# any check is skipped, not after two direct scans happen to notice.
if ! command -v rg >/dev/null 2>&1; then
  printf '%s\n' "security-check: failed - rg (ripgrep) not found on PATH; no scan can run (install ripgrep)" >&2
  exit 1
fi

fail=0

check_no_matches() {
  label="$1"
  shift

  # Gate FM8 (2026-09-10): a scanner ERROR is not a clean scan — an rg
  # failure (bad target, wrong cwd) must fail the gate, not pass it
  # having scanned nothing.
  # rg: 0 = matches found, 1 = no matches (clean), >=2 = scanner ERROR
  # (gate FM8: an error is not a clean scan). `|| rc=$` keeps set -e calm.
  rc=0
  matches="$("$@" 2>/dev/null)" || rc=$?
  if [ "$rc" -ge 2 ]; then
    printf '%s
' "security-check: failed - scanner errored (rc=$rc) on: $*" >&2
    fail=1
    continue
  fi
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

csp_line="$(rg -n '"csp"\s*:' src-tauri/tauri.conf.json || true)"
if [ -z "$csp_line" ]; then
  printf '%s\n' "security-check: failed missing Tauri CSP" >&2
  fail=1
elif printf '%s\n' "$csp_line" | rg -q '"csp"\s*:\s*null'; then
  printf '%s\n' "security-check: failed null Tauri CSP" >&2
  fail=1
elif ! printf '%s\n' "$csp_line" | rg -q 'connect-src[^"]*ipc:[^"]*http://ipc\.localhost'; then
  printf '%s\n' "security-check: failed Tauri CSP missing IPC connect source" >&2
  printf '%s\n' "$csp_line" >&2
  fail=1
elif printf '%s\n' "$csp_line" | rg -q "'unsafe-(inline|eval)'"; then
  printf '%s\n' "security-check: failed Tauri CSP allows unsafe inline/eval" >&2
  printf '%s\n' "$csp_line" >&2
  fail=1
fi

if [ "$fail" -ne 0 ]; then
  exit 1
fi

printf '%s\n' "security-check: passed"
