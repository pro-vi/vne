# vne

`vne` is a local, private viewer/editor for environment files. It treats `.env` as structured configuration instead of plain text: files are detected, keys are typed, likely secrets are redacted, and actual files can be compared with example files before editing.

The project is intentionally small:

- Tauri 2 desktop shell
- Rust parser and key-shape registry
- Svelte/Vite interface
- No telemetry, accounts, sync, or hosted validation

## Why not just VS Code?

Most editors show a `.env` file as raw text. `vne` adds env-specific context:

- Detects common files such as `.env`, `.env.local`, `.env.production`, `.env.example`, `.env.sample`, `.envrc`, and `.flaskenv`
- Finds env files referenced by common project config, including `package.json` scripts using `--env-file`, `dotenv -e`, `env-cmd -f`, or `DOTENV_CONFIG_PATH`, plus Docker Compose `env_file`
- Compares `.env` with `.env.example` for missing, extra, shared, and duplicate keys
- Reports layered overrides across actual env files, naming the winning file without exposing secret values
- Uses Next.js and Vite env load-order knowledge as evidence on related findings
- Shows value-safe findings for missing keys, duplicate keys, placeholders, and layered conflicts, with previewed blank-key insertion for documented missing keys
- Lets targetable advisory findings jump to the relevant file and key occurrence without mutating the env file
- Infers common shapes such as credential URL/DSN, URL, secret, browser-exposed secret-looking variable, public frontend variable, bool, int, port, path, list, UUID, JSON, PEM, and provider-specific keys
- Redacts secret-like values and credential-bearing URLs by default
- Flags public-prefixed secret-looking names such as `NEXT_PUBLIC_API_KEY` as browser-exposed and sensitive-looking instead of treating them as safe public values
- Shows structured source context such as duplicate occurrence, export prefix, quote style, inline comment, and parser diagnostics
- Supports fast key scanning with filter focus and arrow-key row movement
- Edits one selected key occurrence while preserving comments, order, quote style, multiline values, and adjacent formatting

## Privacy

Env contents stay local. The current app has no network path for env data and no telemetry. Redaction is a presentation safeguard inside the trusted local app: secret-like values are hidden in normal UI until Reveal is active, but parsed values are still handled by the local Tauri/Svelte process.

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

Run checks:

```sh
npm run check
npm run build
npm run test
npm run test:rust
```

## Demo Fixture

`fixtures/demo` contains fake env files for exercising the first slice. `fixtures/holdouts` contains small fake Next.js and Vite projects for framework env profile checks:

```sh
npm run dev
```

Then open the browser preview, or run the Tauri app and enter:

```text
fixtures/demo
```

## Current Status

The current slice can scan a directory, pick a directory through the native desktop dialog, parse common env files, show a structured key table and source context, redact likely secrets, credential-bearing URLs, and public-prefixed secret-looking keys, compare actual vs example files, report layered override conflicts, discover env files referenced by common config, attach Next.js and Vite load-order evidence to related findings, show value-safe findings, add documented missing keys as blank entries, support keyboard scanning, and save one selected key occurrence while rescanning project diagnostics in the Tauri runtime. Release packaging is still intentionally deferred.
