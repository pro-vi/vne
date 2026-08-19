# vne

`vne` is an env editor built around the thing developers actually do: find the key, understand what kind of value it wants, add or change it, and get back to the app. It treats `.env` as structured configuration instead of plain text, with key-aware rows, duplicate-aware edits, typed values, and a command agents can suggest without touching secrets.

The project is intentionally small:

- Tauri 2 desktop shell
- Rust parser and key-shape registry
- Svelte/Vite interface
- No telemetry, accounts, sync, or hosted validation

## Why not just VS Code?

Most editors make `.env` editing feel like generic text editing. `vne` is meant to be faster for the env-only workflow:

- Adds or edits a key directly without making you scan the whole file
- Covers the whole key lifecycle from one CLI: add, set, rename, remove, copy between files, and sync an example file
- Tells you whether an env file is tracked by git, and warns when a write lands in one that is
- Creates an empty conventional env file from the desktop or `vne create <file>` without overwriting existing data
- Lets agents hand you a copyable `vne` command while you enter secret values locally
- Refuses duplicate adds with the existing line numbers instead of silently creating another occurrence
- Shows common key meaning inline, such as URL/DSN, secret, public frontend variable, bool, int, port, JSON, PEM, and provider-ish keys
- Edits one selected key occurrence while preserving comments, order, quote style, multiline values, and adjacent formatting
- Supports fast key scanning with filter focus and arrow-key row movement
- Detects common files such as `.env`, `.env.local`, `.env.production`, `.env.example`, `.env.sample`, `.envrc`, and `.flaskenv`
- Finds env files referenced by common project config, including `package.json` scripts using `--env-file`, `dotenv -e`, `env-cmd -f`, or `DOTENV_CONFIG_PATH`, plus Docker Compose `env_file`
- Compares `.env` with `.env.example` for missing, extra, shared, and duplicate keys
- Reports layered overrides across actual env files, avoiding generic winner claims unless framework evidence explains precedence
- Uses Next.js and Vite env load-order knowledge as evidence on related findings
- Separates value-required missing-key insertion from advisory duplicate, placeholder, extra-key, and layered-conflict findings
- Lets targetable advisory findings jump to the relevant file and key occurrence without mutating the env file
- Redacts secret-like values and credential-bearing URLs in desktop snapshots, while CLI JSON withholds every env payload by default
- Flags public-prefixed secret-looking names such as `NEXT_PUBLIC_API_KEY` as browser-exposed and sensitive-looking instead of treating them as safe public values
- Shows structured source context such as duplicate occurrence, export prefix, quote style, inline comment, and parser diagnostics
- Requires an explicit occurrence choice before saving duplicate keys
- Adds new env keys from the editor or CLI while refusing duplicate keys with line context
- Parser/write tests cover CRLF, UTF-8 BOM, empty values, inline comments, hashes inside values, quoted values, multiline values, invalid keys, duplicates, quote-requiring replacements, and the public `fixtures/adversarial-dotenv` corpus

## Agent-Friendly Commands

Agents should not read, write, or receive secret env values. They can produce a command for the user to run instead:

```sh
vne create .env
vne add .env OPENAI_API_KEY --prompt
vne set .env OPENAI_API_KEY --prompt
vne copy ../other-checkout/.env DATABASE_URL .env
```

For non-secret values, the command can be fully copyable:

```sh
vne add .env FEATURE_FLAG=true
vne add .env PORT --value 1420
```

Gestures that never touch a value at all are copyable whatever the key holds:

```sh
vne rm .env STALE_FLAG
vne rename .env OPENAI_KEY OPENAI_API_KEY
vne example .env
```

`create` is idempotent: it creates a zero-byte file once, leaves an existing regular file untouched, and rejects
symlinks, directories, unsupported filenames, and missing parent directories. When `add --prompt` targets a missing
env file, it offers to create the file before asking for the hidden value. Non-interactive flows can run `vne create`
first. `add` refuses duplicate keys and reports existing line numbers, so the copied command is still easy to recover
from, and it warns when a secret-looking value arrives as a command argument instead of through `--stdin`/`--prompt`.
`copy` keeps the selected value inside the Rust process. It adds a missing destination key, reports
`alreadyPresent` without writing when the exact value token is already there, and requires `--overwrite` for one
different existing value. Missing, malformed, and duplicate keys are refused.

`set` replaces one existing key's value and reads that value only from `--stdin` or `--prompt`; no argument form is
accepted. An identical value reports `alreadyPresent` and leaves the bytes alone. `rm` deletes a key, refuses a
duplicated one unless `--line <N>` or `--all` picks one, and reports `alreadyAbsent` instead of failing when the key
is already gone; `--line <N>` is a one-shot selector that fails when that line holds something else rather than
hunting for another occurrence, and `--expect present|absent` turns a wrong belief about the key into exit 2.
`rename` changes only the key token, leaving the value bytes, quoting, inline comment, `export` prefix, and line
position identical; when the old key is absent and the new one is present it refuses, because file state cannot prove
that a rename is what produced it. `example` appends the keys your real file has and the example file lacks, as
valueless placeholders, and never rewrites or deletes an existing example line.

`inspect` and `check` report each env file's exposure to git as `tracked`, `untrackedIgnored`, `untrackedNotIgnored`,
`outsideRepository`, or `unknown`, and every mutating command warns on stderr when its write landed in a file git
already tracks. The warning never blocks the write.
When stdout is piped, data-bearing commands emit compact JSON with every env value, display value, comment,
and raw preview withheld. Use `--text` for human output, `--json` for compact JSON explicitly, or `--pretty`
for formatted JSON. `--values` explicitly includes values the local classifier considers non-sensitive; detected
secrets remain redacted, while comments and raw previews stay withheld by output policy.

To open the desktop editor at a project, run:

```sh
vne .
```

Install or refresh the local desktop/CLI binary with the production asset protocol enabled:

```sh
npm run install:local
```

Use this project command instead of plain `cargo install --path src-tauri`: Tauri release binaries need the
`custom-protocol` feature to load the bundled interface when the Vite development server is not running.

## Privacy

Env contents stay local. The current app has no network path for env data and no telemetry. Tauri CSP is enabled for local assets and IPC. Default Tauri snapshots scrub classifier-detected entry values and secret-like inline comments, and withhold raw preview payloads for files with redacted values or secret-like comments/malformed lines. CLI JSON uses a stronger structural boundary: `inspect`, `check`, and `add` always withhold comments and raw previews, and withhold values unless `--values` is supplied; `create` returns only its disposition and absolute path; `copy` returns only its disposition, normalized paths, and key. Reveal fetches only the selected key occurrence into the local webview and clears it on hide, selection change, reload, or save. Newly created env files are empty and owner-private on Unix. `format --dry-run` is intentionally raw and warns when piped.

vne's guarantee is that env values never appear in its output, not that no fact derived from a value is inferable:
a disposition such as `alreadyPresent` tells the caller that the value it supplied equals the stored one, and any
local process with file access can read env files directly.

See [docs/THREAT_MODEL.md](docs/THREAT_MODEL.md) for the current local-only threat model and release safety checklist.

## Development

Install dependencies:

```sh
npm install
```

Run the browser preview with sample data:

```sh
npm run dev
```

Run the desktop app:

```sh
npm run tauri dev
```

Run the local `vne` binary:

```sh
cargo run --manifest-path src-tauri/Cargo.toml --bin vne -- inspect fixtures/demo
cargo run --manifest-path src-tauri/Cargo.toml --bin vne -- create /tmp/project/.env --json
cargo run --manifest-path src-tauri/Cargo.toml --bin vne -- check fixtures/demo/.env --example fixtures/demo/.env.example --json
cargo run --manifest-path src-tauri/Cargo.toml --bin vne -- inspect fixtures/demo --json --values
cargo run --manifest-path src-tauri/Cargo.toml --bin vne -- add fixtures/demo/.env FEATURE_FLAG true
cargo run --manifest-path src-tauri/Cargo.toml --bin vne -- add fixtures/demo/.env FEATURE_FLAG=true
cargo run --manifest-path src-tauri/Cargo.toml --bin vne -- add fixtures/demo/.env OPENAI_API_KEY --prompt
cargo run --manifest-path src-tauri/Cargo.toml --bin vne -- copy fixtures/demo/.env DATABASE_URL /tmp/project/.env --overwrite --json
cargo run --manifest-path src-tauri/Cargo.toml --bin vne -- example fixtures/demo/.env --json
cargo run --manifest-path src-tauri/Cargo.toml --bin vne -- format fixtures/demo/.env --dry-run
```

`inspect` and `check` return exit code `1` when diagnostics or contract drift are found. `create` accepts one env-file path whose parent already exists and returns exit code `0` for both `created` and retry-safe `alreadyExists` outcomes. `add` accepts `<KEY> <VALUE>`, `<KEY=VALUE>`, `--value`, `--stdin`, or `--prompt`; it refuses duplicate keys and reports the existing line numbers instead of appending another occurrence. `copy` accepts two distinct existing files and one key; it returns `added`, `overwritten`, or retry-safe `alreadyPresent`, and returns exit code `2` with empty stdout on conflicts and invalid input. `set`, `rm`, `rename`, and `example` return payload-free receipts too: `set` reports `updated` or `alreadyPresent`, `rm` reports `removed`, `removedAll`, or `alreadyAbsent` with the line numbers it spliced, `rename` reports the key pair, and `example` reports the key names it appended. JSON is payload-free by default; use `--values` only with `inspect`, `check`, or `add` when classifier-approved non-secret values are required. Use `vne <command> --help` for command-specific examples.

The compatibility binary still exists for local automation that already calls `--bin vne-cli`, but the product command and default Cargo binary are `vne`.

Run checks:

```sh
npm run check
npm run build
npm run test
npm run test:rust
scripts/security-check.sh
```

## Demo Fixture

`fixtures/demo` contains fake env files for exercising the first slice. `fixtures/holdouts` contains small fake Next.js and Vite projects for framework env profile checks:

```sh
npm run dev
```

The browser preview loads that fake project automatically. For the native app,
run `npm run tauri dev`, choose **Choose folder**, and select `fixtures/demo`.
An installed desktop binary can open it directly with `vne fixtures/demo`.

The current minimal UI direction is captured in [docs/assets/vne-minimal-design.png](docs/assets/vne-minimal-design.png).

## Current Status

The current slice can scan a directory, pick a directory through the native desktop dialog, create root-level env files, parse common env files, show a structured key table and source context, redact likely secrets, credential-bearing URLs, and public-prefixed secret-looking keys at the Tauri command boundary, reveal one selected key occurrence on demand, compare actual vs example files, report layered override conflicts, discover env files referenced by common project config, attach Next.js and Vite load-order evidence to related findings, separate value-required missing-key insertion from advisory findings, support keyboard scanning, add new duplicate-aware env keys, and save one selected key occurrence while rescanning project diagnostics in the Tauri runtime. The Rust CLI exposes the same local parser for `inspect`, `check --example`, duplicate-aware `add`, idempotent `create`, secret-safe same-key `copy`, update-only `set`, guarded `rm`, value-preserving `rename`, and additive `example` sync, and reports each env file's exposure to git; data-bearing JSON withholds all env payloads by default and requires `--values` to include classifier-approved values, while `create`, `copy`, `set`, `rm`, `rename`, and `example` return payload-incapable receipts. `format --dry-run` remains an explicitly raw local inspection command. Release packaging is still intentionally deferred.
