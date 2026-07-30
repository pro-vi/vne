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
```

For non-secret values, the command can be fully copyable:

```sh
vne add .env FEATURE_FLAG=true
vne add .env PORT --value 1420
```

`create` is idempotent: it creates a zero-byte file once, leaves an existing regular file untouched, and rejects
symlinks, directories, unsupported filenames, and missing parent directories. `add` refuses duplicate keys and
reports existing line numbers, so the copied command is still easy to recover from.
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

Env contents stay local. The current app has no network path for env data and no telemetry. Tauri CSP is enabled for local assets and IPC. Default Tauri snapshots scrub classifier-detected entry values and secret-like inline comments, and withhold raw preview payloads for files with redacted values or secret-like comments/malformed lines. CLI JSON uses a stronger structural boundary: `inspect`, `check`, and `add` always withhold comments and raw previews, and withhold values unless `--values` is supplied; `create` returns only its disposition and absolute path. Reveal fetches only the selected key occurrence into the local webview and clears it on hide, selection change, reload, or save. Newly created env files are empty and owner-private on Unix. `format --dry-run` is intentionally raw and warns when piped.

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
cargo run --manifest-path src-tauri/Cargo.toml --bin vne -- format fixtures/demo/.env --dry-run
```

`inspect` and `check` return exit code `1` when diagnostics or contract drift are found. `create` accepts one env-file path whose parent already exists and returns exit code `0` for both `created` and retry-safe `alreadyExists` outcomes. `add` accepts `<KEY> <VALUE>`, `<KEY=VALUE>`, `--value`, `--stdin`, or `--prompt`; it refuses duplicate keys and reports the existing line numbers instead of appending another occurrence. JSON is payload-free by default; use `--values` only when classifier-approved non-secret values are required. Use `vne <command> --help` for command-specific examples.

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

The current slice can scan a directory, pick a directory through the native desktop dialog, create root-level env files, parse common env files, show a structured key table and source context, redact likely secrets, credential-bearing URLs, and public-prefixed secret-looking keys at the Tauri command boundary, reveal one selected key occurrence on demand, compare actual vs example files, report layered override conflicts, discover env files referenced by common project config, attach Next.js and Vite load-order evidence to related findings, separate value-required missing-key insertion from advisory findings, support keyboard scanning, add new duplicate-aware env keys, and save one selected key occurrence while rescanning project diagnostics in the Tauri runtime. The Rust CLI exposes the same local parser for `inspect`, `check --example`, duplicate-aware `add`, and idempotent `create`; data-bearing JSON withholds all env payloads by default and requires `--values` to include classifier-approved values, while `create` returns only disposition and path. `format --dry-run` remains an explicitly raw local inspection command. Release packaging is still intentionally deferred.
