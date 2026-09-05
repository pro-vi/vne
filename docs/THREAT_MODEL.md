# vne Threat Model

`vne` is a local desktop env-file workbench. Its safety bar is simple: opening, inspecting, and editing env files must not move secret contents outside the user's machine.

## Assets

- Env file contents, including API keys, tokens, database URLs, private keys, and webhook secrets.
- File structure around env files, including project names and local paths.
- User edits before they are saved.

## Trust Boundaries

- Rust parser and writer: trusted local code that reads and writes files selected by the user.
- Svelte UI: trusted local code that renders parsed values and sends explicit commands to the Tauri backend. Default snapshots do not include raw secret-like values in parsed entries; Reveal fetches only one selected key occurrence into the local webview.
- Tauri command boundary: only local UI invokes project scan, root-level env-file creation, value save, and value-required missing-key insertion.
- CLI stdout and stderr: output may be retained by shells, CI, pipes, or agent transcripts. Structured JSON withholds env payloads by default; `--values` explicitly opts into classifier-approved entry values while comments and raw content remain withheld. `create` emits only a disposition and absolute path. `copy` emits only a disposition, normalized source and destination paths, and a key. `set`, `rm`, `rename`, and `example` emit only a disposition, normalized paths, key names, and line numbers.
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
- Env-file creation accepts one validated root-level filename in the desktop app. The shared Rust primitive canonicalizes the root, derives the target itself, and uses exclusive creation. It creates exactly zero bytes, uses owner-private permissions on Unix, never follows a final-component symlink, never truncates an existing regular file, and reports retries as `alreadyExists`.
- Creation and project refresh are separate operations. A refresh failure after successful creation is reported as “created, refresh failed” so retrying cannot accidentally overwrite or obscure the durable outcome.
- Env-key set is update-only and refuses every argument value form: the new value arrives from stdin or a hidden prompt, never from argv, where shell history and process lists would retain it. It refuses missing, duplicate, and malformed keys, and an identical parsed value reports `alreadyPresent` without writing.
- Env-key removal splices whole lines from one read snapshot in a single atomic write. `--line <N>` is a one-shot selector: when line N does not hold the named key it fails with the key it actually found and writes nothing, so a stale line number cannot delete whichever entry shifted into that line. `--expect present|absent` converts a wrong belief about the key's state into exit 2 before any write.
- Env-key rename changes only the key token; the value bytes, quoting, inline comment, `export` prefix, and line position stay identical. When the old key is absent and the new key is present it refuses rather than reporting a convergent success, because file state cannot prove that a rename produced that arrangement.
- Example sync is additive only. It appends the missing key names as valueless placeholders, never copies a value, never rewrites or deletes an existing example line, and drops an inline comment that itself trips the secret scan.
- `add` warns on stderr when a secret-looking value arrives as a command argument, naming the key only, and steers to `--stdin` or `--prompt`. The warning fires before any other precondition is checked, because the value is already in the process list and shell history by then.
- No CLI message echoes the value half of an argument. A rejected key, an unknown option, the secret-like argument warning, and the hidden prompt's label all render `KEY=value` as `KEY=…`; `set`, `rm`, and `rename` additionally refuse a `KEY=VALUE` argument in a key slot at parse time.
- `set` refuses an empty or whitespace value unless `--allow-empty` is passed, so a pipeline that produced nothing cannot silently overwrite a stored secret with an empty one.
- Every env file carries a git exposure state (`tracked`, `untrackedIgnored`, `untrackedNotIgnored`, `outsideRepository`, `unknown`) derived by read-only `git` plumbing commands that never read the file's contents. Mutating commands warn on stderr when a write landed in a tracked file. The warning never blocks the write.
- Env-key copy resolves two distinct existing regular-file targets, selects one diagnostic-free source occurrence, and transfers its exact value token without returning it to the CLI layer. It refuses missing, malformed, and duplicate source keys; refuses duplicate destination keys; and requires `--overwrite` for one different destination value. An identical token returns `alreadyPresent` without writing, so retry after an output failure converges.
- Atomic rewrites use an exclusively created same-directory temporary file with destination permissions, complete and sync its bytes before rename, and clean it up on pre-persist failure. This prevents partial destination bytes and predictable-temp symlink attacks; it does not lock out non-cooperating concurrent writers or sync the containing directory.
- Missing/new-key form protection derives from Rust-produced key-shape metadata. Unknown or conflicting keys default to password input until the local user explicitly chooses Show; the Svelte UI does not maintain a second secret-name registry.
- Write commands resolve the active project root and reject file paths outside that root, with regression tests for direct symlink escapes and nested symlink directory escapes.
- Env files referenced by Docker Compose are canonicalized and ignored when they resolve outside the active project root.
- Parser/write regression tests include CRLF, UTF-8 BOM, empty values, inline comments, hashes inside values, quoted values, multiline values, invalid keys, duplicates, quote-requiring replacements, and the public `fixtures/adversarial-dotenv` corpus.
- Browser preview uses sample data and cannot create files, save, or add missing keys.
- Tauri CSP is enabled with local-only defaults, IPC connect sources, and no `unsafe-inline` or `unsafe-eval` allowance.
- The app has no accounts, telemetry, sync, hosted validation, or cloud calls for env data.
- `scripts/security-check.sh` fails on obvious network client APIs, telemetry SDK imports, raw logging calls, updater surfaces, broad dialog permissions, non-local Tauri dev URLs, and missing, null, or unsafe Tauri CSP.

## Non-Goals

- `vne` is not a secrets manager.
- `vne` is not a cloud config platform.
- `vne` does not rotate, fetch, publish, or validate credentials against providers.
- `vne` does not sign releases or manage installers in this loop.

## Known Residual Risks

- Dispositions disclose value equality. `alreadyPresent` on `set` or `copy` tells the caller that the value it supplied equals the stored one, and exit codes carry the same fact. vne's guarantee is that values never appear in its output, not that no value-derived fact is inferable; any local process with file access can read env files directly.
- `rm --line <N>` protects against a stale line number that now holds a different entry, but not against two occurrences of the same key on adjacent lines: after the first is removed the second moves into line N, and an identical retry deletes it. File state cannot distinguish that retry from a fresh intent, which is the same limit that makes `rename` refuse to claim `alreadyRenamed`.
- Example sync never prunes. A key retired from the real env file stays in the example file until someone removes it by hand; `vne example` will not report it.
- Git exposure is a point-in-time read of git's index and ignore rules through a child process. It can be stale by the time a write happens, reports `unknown` when git is absent or a git invocation fails, and `tracked` means git holds the file now, not that a commit has already published it. A hung `git` would hang the command; there is no timeout.
- All mutating commands remain one read, one transform, one atomic write. Concurrent external editors are still last-write-wins: vne does not lock a file against a non-cooperating writer.

- Reveal intentionally serializes one selected raw value into the trusted local webview and shows it to the local user.
- Parsed raw values are present inside the trusted Rust process during scans, writes, and per-entry reveal. A selected raw value enters the Svelte process only after the explicit Reveal command; this is still a local trust boundary, not OS-level secret isolation.
- `copy` intentionally holds the selected source value token inside the trusted Rust process. Its atomic rewrite preserves `std::fs::Permissions`, but does not promise inode identity, ownership changes, ACLs, extended attributes, or protection from a concurrent external writer.
- Desktop snapshots and CLI `--values` entry values remain classifier-driven. An unknown sensitive value with no recognized provider, name, URL, or value signature can still be treated as ordinary; CLI comments and raw previews remain withheld, and default CLI JSON remains payload-free despite such a miss.
- `format --dry-run` is a human-terminal command: it refuses piped stdout (exit 2) and requires an interactive `y` confirmation on terminal stdin before printing raw values, so piped agents and transcripts cannot capture the file. A caller that allocates a pseudo-terminal can still reach the confirmation prompt; that spoofing surface belongs to the arbitrary-CLI attacker profile, not this residual-risk note.
- Raw preview remains disabled whenever the selected file has classified redacted entries, independent of whether one entry was explicitly revealed. Secret-like comments and malformed lines are withheld by heuristic; unknown sensitive text without recognizable markers can still appear in desktop raw preview for files with no classified sensitive entry.
- A malicious local project can use misleading key names or comments; `vne` treats env files as data and does not execute them.
- Active project root checks and root-plus-basename creation reduce accidental path escape, but they are not a substitute for operating-system file permissions or user caution when opening untrusted projects.
- Browser-preview screenshots and DOM checks cover responsive layout and modal behavior; native runtime checks remain necessary for filesystem mutation and platform-webview behavior.
- The security check is intentionally narrow. It is a regression harness for obvious source/config surfaces, not a dependency sandbox, runtime network monitor, CSP verifier, or proof that all transitive dependencies are inert.

## Release Checklist Before Public Distribution

- Re-run source scans for network clients, telemetry SDKs, and logging of raw values.
- Run `scripts/security-check.sh` and review any false-positive adjustment.
- Re-check path-scope, symlink, idempotency, and no-clobber behavior around write commands after any command-surface change.
- Re-review CSP after any asset, protocol, iframe, style, or network-surface change.
- Verify raw values are absent from structured logs and error paths.
- Verify `copy`, `set`, `rm`, `rename`, and `example` sentinel values are absent from stdout, stderr, errors, and child-process arguments on every success and failure path.
- Verify macOS signing/notarization choices with explicit user approval.
- Capture screenshots using a local browser-control surface.
