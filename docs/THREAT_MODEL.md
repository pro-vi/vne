# vne Threat Model

`vne` is a local desktop env-file workbench. Its safety bar is simple: opening, inspecting, and editing env files must not move secret contents outside the user's machine.

## Assets

- Env file contents, including API keys, tokens, database URLs, private keys, and webhook secrets.
- File structure around env files, including project names and local paths.
- User edits before they are saved.

## Trust Boundaries

- Rust parser and writer: trusted local code that reads and writes files selected by the user.
- Svelte UI: trusted local code that renders parsed values and sends explicit commands to the Tauri backend. Default snapshots do not include raw secret-like values in parsed entries; Reveal fetches only one selected key occurrence into the local webview.
- Tauri command boundary: only local UI invokes project scan, value save, and value-required missing-key insertion.
- CLI stdout and stderr: output may be retained by shells, CI, pipes, or agent transcripts. Structured JSON withholds env payloads by default; `--values` explicitly opts into classifier-approved entry values while comments and raw content remain withheld.
- Native dialog: used only to pick a local directory.
- Network: not part of the product path for env contents.

## Current Controls

- Exact provider profiles, shared secret-name evidence, boundary-aware credential value signatures, credential-bearing URLs/DSNs, and public-prefixed secret-looking names drive classified redaction before normal Tauri snapshots reach the webview. Strong secret evidence overrides a known-public key profile unless the value validates as that provider's public representation.
- Default Tauri snapshots clear `entry.value`, mask `entry.displayValue`, withhold comments attached to redacted entries or containing secret-like text, and withhold raw preview payloads for files containing redacted entries or secret-like comments/malformed lines.
- `inspect`, `check`, and `add` JSON rebuild the serialized model with every env value, display value, comment, and raw preview withheld by default. `--values` restores classifier-approved entry values and display values while comments and raw previews remain policy-withheld, without changing findings, writes, or exit status.
- The explicit Reveal command fetches one selected key occurrence by file, key, and line number. Frontend operation generations bind the response to that captured occurrence, and plaintext is retained as one target/value object only while it still matches the active selection. Hide, selection changes, reloads, and saves clear it.
- Key-shape metadata separates sensitive-looking values from browser-exposed public prefixes, so names such as `NEXT_PUBLIC_API_KEY` are not treated as safe public values.
- Findings include key names, file names, and framework load-order evidence, not raw values.
- Source context shows structural metadata such as quote style, export prefix, safe comments, and diagnostics without adding network or logging paths; secret-like inline comments are replaced as a whole.
- Missing-key insertion previews include only key names, file names, and the value-required edit shape.
- Advisory finding inspection changes only UI selection state; targetable findings carry entry id and line number where an exact occurrence exists, and inspection does not mutate files.
- Layer conflict summaries avoid generic winner claims; framework load-order evidence is attached to findings when available without showing competing values.
- Save operations update one selected key occurrence at a time, use atomic write with permission preservation where practical, and return a fresh project snapshot so diagnostics do not stay stale after writes.
- Duplicate-key saves require an explicit "this occurrence only" choice in the UI before the save button is enabled.
- Missing-key insertion requires an explicit non-empty value; it refuses duplicate, invalid, or empty keys and returns a fresh project snapshot.
- Missing/new-key form protection derives from Rust-produced key-shape metadata. Unknown or conflicting keys default to password input until the local user explicitly chooses Show; the Svelte UI does not maintain a second secret-name registry.
- Write commands resolve the active project root and reject file paths outside that root, with regression tests for direct symlink escapes and nested symlink directory escapes.
- Env files referenced by Docker Compose are canonicalized and ignored when they resolve outside the active project root.
- Parser/write regression tests include CRLF, UTF-8 BOM, empty values, inline comments, hashes inside values, quoted values, multiline values, invalid keys, duplicates, quote-requiring replacements, and the public `fixtures/adversarial-dotenv` corpus.
- Browser preview uses sample data and cannot save or add missing keys.
- Tauri CSP is enabled with local-only defaults, IPC connect sources, and no `unsafe-inline` or `unsafe-eval` allowance.
- The app has no accounts, telemetry, sync, hosted validation, or cloud calls for env data.
- `scripts/security-check.sh` fails on obvious network client APIs, telemetry SDK imports, raw logging calls, updater surfaces, broad dialog permissions, non-local Tauri dev URLs, and missing, null, or unsafe Tauri CSP.

## Non-Goals

- `vne` is not a secrets manager.
- `vne` is not a cloud config platform.
- `vne` does not rotate, fetch, publish, or validate credentials against providers.
- `vne` does not sign releases or manage installers in this loop.

## Known Residual Risks

- Reveal intentionally serializes one selected raw value into the trusted local webview and shows it to the local user.
- Parsed raw values are present inside the trusted Rust process during scans, writes, and per-entry reveal. A selected raw value enters the Svelte process only after the explicit Reveal command; this is still a local trust boundary, not OS-level secret isolation.
- Desktop snapshots and CLI `--values` entry values remain classifier-driven. An unknown sensitive value with no recognized provider, name, URL, or value signature can still be treated as ordinary; CLI comments and raw previews remain withheld, and default CLI JSON remains payload-free despite such a miss.
- `format --dry-run` intentionally writes the complete raw file to stdout and warns when piped. It is for explicit local inspection, not agent or transcript-safe diagnostics.
- Raw preview remains disabled whenever the selected file has classified redacted entries, independent of whether one entry was explicitly revealed. Secret-like comments and malformed lines are withheld by heuristic; unknown sensitive text without recognizable markers can still appear in desktop raw preview for files with no classified sensitive entry.
- A malicious local project can use misleading key names or comments; `vne` treats env files as data and does not execute them.
- Active project root checks reduce accidental path escape, but they are not a substitute for operating-system file permissions or user caution when opening untrusted projects.
- Screenshot or browser-inspection proof is still unavailable in this session because only navigation, not screenshot or DOM inspection, was exposed.
- The security check is intentionally narrow. It is a regression harness for obvious source/config surfaces, not a dependency sandbox, runtime network monitor, CSP verifier, or proof that all transitive dependencies are inert.

## Release Checklist Before Public Distribution

- Re-run source scans for network clients, telemetry SDKs, and logging of raw values.
- Run `scripts/security-check.sh` and review any false-positive adjustment.
- Re-check path-scope behavior around write commands after any command-surface change.
- Re-review CSP after any asset, protocol, iframe, style, or network-surface change.
- Verify raw values are absent from structured logs and error paths.
- Verify macOS signing/notarization choices with explicit user approval.
- Capture screenshots using a local browser-control surface.
