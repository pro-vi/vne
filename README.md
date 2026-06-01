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
- Shows framework env profiles for Next.js and Vite projects, including effective load order and missing expected files
- Builds a value-safe repair queue for missing keys, duplicate keys, placeholders, and layered conflicts
- Infers common shapes such as URL/DSN, secret, public frontend variable, bool, int, port, path, list, UUID, JSON, PEM, and provider-specific keys
- Redacts secret-like values by default
- Edits one value while preserving comments, order, quote style, multiline values, and adjacent formatting

## Privacy

Env contents stay local. The current app has no network path for env data and no telemetry. Secret-like values are redacted in the UI and should not be logged.

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

The current slice can scan a directory, pick a directory through the native desktop dialog, parse common env files, show a structured key table, redact likely secrets, compare actual vs example files, report layered override conflicts, discover env files referenced by common config, model Next.js and Vite env load-order profiles, suggest value-safe repair actions, and save a single edited value in the Tauri runtime. Release packaging is still intentionally deferred.
