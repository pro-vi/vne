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
- CLI stdout and stderr: output may be retained by shells, CI, pipes, or agent transcripts. Structured JSON withholds env payloads by default; `--values` explicitly opts into classifier-approved entry values while comments and raw content remain withheld. `create` emits only a disposition and absolute path. `copy` emits only a disposition, normalized source and destination paths, and a key. `set`, `rm`, `rename`, and `example` emit only a disposition, normalized paths, key names, and line numbers. `where` emits only the key name, paths, line numbers, git status, value states, and the name and reason of each discovered file it did not read. Every JSON entry carries a `valueState` of `set`, `empty`, or `placeholder`.
- Native dialog: used only to pick a local directory.
- Network: not part of the product path for env contents.

## Current Controls

- Exact provider profiles, shared secret-name evidence, boundary-aware credential value signatures, credential-bearing URLs/DSNs, and public-prefixed secret-looking names drive classified redaction before normal Tauri snapshots reach the webview. Strong secret evidence overrides a known-public key profile unless the value validates as that provider's public representation.
- Default Tauri snapshots clear classified-redacted entries' `entry.value` and mask their `entry.displayValue`, leave `entry.valueState` as parsed, withhold comments attached to redacted entries or containing secret-like text, and withhold raw preview payloads for files containing redacted entries or secret-like comments/malformed lines.
- `inspect`, `check`, and `add` JSON rebuild the serialized model with every env value, display value, comment, and raw preview withheld by default. `--values` restores classifier-approved entry values and display values while comments and raw previews remain policy-withheld, without changing findings, writes, or exit status.
- The explicit Reveal command fetches one selected key occurrence by file, key, and line number. Frontend operation generations bind the response to that captured occurrence, and plaintext is retained as one target/value object only while it still matches the active selection. Releasing Option, the window losing focus, selection changes, reloads, and saves clear it.
- Key-shape metadata separates sensitive-looking values from browser-exposed public prefixes, so names such as `NEXT_PUBLIC_API_KEY` are not treated as safe public values.
- Findings include key names, file names, and framework load-order evidence, not raw values.
- Source context shows structural metadata such as quote style, export prefix, safe comments, and diagnostics without adding network or logging paths; secret-like inline comments are replaced as a whole.
- Missing-key insertion previews include only key names, file names, and the value-required edit shape.
- Layer conflict summaries avoid generic winner claims; framework load-order evidence is attached to findings when available without showing competing values.
- Save operations update one selected key occurrence at a time, use atomic write with permission preservation where practical, and return a fresh project snapshot so diagnostics do not stay stale after writes.
- Each desktop row is one occurrence, and a save targets that row's key and line. The window never removes lines: a duplicated key shows the `vne rm` command that keeps the selected line.
- Copy reads the selected occurrence in Rust and writes it to the macOS general pasteboard; the value never enters the webview. A sensitive or classifier-redacted value is written for this Mac only (no Universal Clipboard) with the `org.nspasteboard.ConcealedType` and `TransientType` markers, and is cleared after 30 s, on app exit, and on SIGINT, SIGTERM or SIGHUP, while the pasteboard still holds it: the same change count, or text matching a per-process keyed fingerprint taken at copy time. The value itself is not retained (ADR 0006).
- Missing-key insertion requires an explicit non-empty value; it refuses duplicate, invalid, or empty keys and returns a fresh project snapshot.
- The desktop app creates only a root `.env`. An empty one goes through the shared Rust primitive, which canonicalizes the root, derives the target itself, and uses exclusive creation; it creates exactly zero bytes, uses owner-private permissions on Unix, never follows a final-component symlink, never truncates an existing regular file, and reports retries as `alreadyExists`. A `.env` started from the example file is a byte copy of the first root `.env.example`, `.env.sample`, `.env.template` or `.env.defaults`, written to a temporary file and persisted without clobbering, owner-private on Unix; an existing `.env` is never replaced.
- Creating an empty `.env` and refreshing the project are separate operations, so a refresh failure cannot obscure that the file now exists. Creating `.env` from the example rescans in the same call; a rescan failure is reported as that error after the file was written, and the next reload shows it.
- Env-key set is update-only and refuses every argument value form: the new value arrives from stdin or a hidden prompt, never from argv, where shell history and process lists would retain it. It refuses missing, duplicate, and malformed keys, and an identical parsed value reports `alreadyPresent` without writing.
- Env-key removal splices whole lines from one read snapshot in a single atomic write. `--line <N>` is a one-shot selector: when line N does not hold the named key it fails with the key it actually found and writes nothing, so a stale line number cannot delete whichever entry shifted into that line. `--expect present|absent` converts a wrong belief about the key's state into exit 2 before any write.
- Env-key rename changes only the key token; the value bytes, quoting, inline comment, `export` prefix, and line position stay identical. When the old key is absent and the new key is present it refuses rather than reporting a convergent success, because file state cannot prove that a rename produced that arrangement.
- Example sync is additive only. It appends the missing key names as valueless placeholders, never copies a value, never rewrites or deletes an existing example line, and drops an inline comment that itself trips the secret scan.
- `run` loads env files into one command's environment and starts it; the calling shell never holds the keys, and vne writes nothing when the command starts. It reads files as data, never as shell code: values are literal, and a non-entry line, a malformed or repeated entry, a key defined in two of the files, a quoted escape other than a doubled backslash or an escaped quote, or a NUL byte refuses the run before the command starts, with a message naming only files, keys and line numbers. It refuses `env`, `printenv`, and `sh -c`-style scripts that list the environment.
- `add` warns on stderr when a secret-looking value arrives as a command argument, naming the key only, and steers to `--stdin` or `--prompt`. The warning fires before any other precondition is checked, because the value is already in the process list and shell history by then.
- Rejected options, extra arguments, and invalid `rm` selector values are omitted from errors. Key labels, secret-like argument warnings, and hidden prompt labels render `KEY=value` as `KEY=…`; `set`, `rm`, and `rename` additionally refuse a `KEY=VALUE` argument in a key slot at parse time.
- `set` refuses an empty or whitespace value unless `--allow-empty` is passed, so a pipeline that produced nothing cannot silently overwrite a stored secret with an empty one.
- Every env file that `inspect` and `check` report, and every `where` match, carries a git exposure state (`tracked`, `untrackedIgnored`, `untrackedNotIgnored`, `outsideRepository`, `unknown`) derived by read-only `git` plumbing commands that never read the file's contents. Mutating commands warn on stderr when a write landed in a tracked file. The warning never blocks the write.
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

- Dispositions and value states disclose value-derived facts. `alreadyPresent` on `set` or `copy` tells the caller that the value it supplied equals the stored one, and exit codes carry the same fact. Every JSON entry's `valueState` tells whether a stored value is empty or a template such as `changeme`; `inspect` already listed layer-file keys holding either in `layerReport.placeholderKeys`, without telling the two apart. Default CLI inspection withholds stored values while exposing these value-derived facts; any local process with file access can read env files directly.
- `rm --line <N>` protects against a stale line number that now holds a different entry, but not against two occurrences of the same key on adjacent lines: after the first is removed the second moves into line N, and an identical retry deletes it. File state cannot distinguish that retry from a fresh intent, which is the same limit that makes `rename` refuse to claim `alreadyRenamed`.
- Example sync never prunes. A key retired from the real env file stays in the example file until someone removes it by hand; `vne example` will not report it.
- Git exposure is a point-in-time read of git's index and ignore rules through a child process. It can be stale by the time a write happens, reports `unknown` when git is absent, a git invocation fails or refuses to open the repository, a scan's deadline passes before git reaches the file, or git's listings leave the file out (as after a case-only rename or inside an uninitialized submodule; git 2.50.1), and `tracked` means git holds the file now, not that a commit has already published it. A hung `git` is bounded: every invocation runs under a 5-second deadline and is killed to `unknown` (VNE-SEC-014).
- `run` hands values to a program the caller chooses, so a caller can print them on purpose, for example with `node -e 'console.log(process.env)'`. The refusal of `env`, `printenv` and environment-listing shell scripts catches a slip; it is not a boundary. While the command runs, same-user tools that read process environments (`ps` with an environment flag) can see its keys. This is the same asymmetry `docs/research/dopbase-2026-09-15.md` found in dopbase's `run` (ADR 0005).
- All mutating commands remain one read, one transform, one atomic write. Concurrent external editors are still last-write-wins: vne does not lock a file against a non-cooperating writer.

- Reveal intentionally serializes one selected raw value into the trusted local webview and shows it to the local user. Authority boundary (harden F0010, 2026-09-05): reveal is an explicit per-entry grant in the app UI, bounded to the backend-owned AuthorizedRoot, auto-hidden on selection change; the pre-branch version accepted a frontend-supplied root. The only programmatic driver is the webdriver automation server, which now initializes in debug builds only — release builds embed no server, so no local process can drive reveal without the user's own session.
- Parsed raw values are present inside the trusted Rust process during scans, writes, and per-entry reveal. Classifier-approved values enter normal desktop snapshots; Reveal fetches one withheld value explicitly. This is a local trust boundary, not OS-level secret isolation.
- `copy` intentionally holds the selected source value token inside the trusted Rust process. Its atomic rewrite preserves `std::fs::Permissions` and the extended attributes captured before the write — a captured attribute disappearing or erroring mid-write refuses the write; a captured attribute's VALUE changing mid-write is carried, and attributes added by a concurrent writer after the capture are dropped. On macOS it also copies the original file's ACL onto the replacement before rename and refuses the write if that copy fails. It does not preserve inode identity or ownership changes, promise ACL preservation on other platforms, or lock out a concurrent external writer.
- Desktop snapshots and CLI `--values` entry values remain classifier-driven. An unknown sensitive value with no recognized provider, name, URL, or value signature can still be treated as ordinary; CLI comments and raw previews remain withheld, and default CLI JSON remains payload-free despite such a miss.
- `format --dry-run` is a human-terminal command: it refuses piped stdout (exit 2) and requires an interactive `y` confirmation on terminal stdin before printing raw values, so piped agents and transcripts cannot capture the file. A caller that allocates a pseudo-terminal can still reach the confirmation prompt; that spoofing surface belongs to the arbitrary-CLI attacker profile, not this residual-risk note.
- A copied secret sits on the general pasteboard for up to 30 s, where any same-user process can read it. The concealed and transient markers are a convention: a clipboard tool that ignores them can record the value, and a tool that re-posts clipboard items drops them (the clear still matches the value by fingerprint). A crash or SIGKILL skips the clear, leaving the value until something else is copied.
- A malicious local project can use misleading key names or comments; `vne` treats env files as data and does not execute them.
- Active project root checks and root-plus-basename creation reduce accidental path escape, but they are not a substitute for operating-system file permissions or user caution when opening untrusted projects.
- Browser-preview screenshots and DOM checks cover layout and key handling; native runtime checks remain necessary for filesystem mutation and platform-webview behavior.
- The security check is intentionally narrow. It is a regression harness for obvious source/config surfaces, not a dependency sandbox, runtime network monitor, CSP verifier, or proof that all transitive dependencies are inert.

## Release Checklist Before Public Distribution

- Re-run source scans for network clients, telemetry SDKs, and logging of raw values.
- Run `scripts/security-check.sh` and review any false-positive adjustment.
- Re-check path-scope, symlink, idempotency, and no-clobber behavior around write commands after any command-surface change.
- Re-review CSP after any asset, protocol, iframe, style, or network-surface change.
- Verify raw values are absent from structured logs and error paths.
- Verify `copy`, `set`, `rm`, `rename`, `example`, and `where` sentinel values are absent from stdout, stderr, errors, and child-process arguments on every success and failure path.
- Verify macOS signing/notarization choices with explicit user approval.
- Capture screenshots using a local browser-control surface.
