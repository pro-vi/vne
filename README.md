# vne

A local `.env` editor and CLI for finding keys and editing configuration. `inspect`, `check`, and `where` return metadata without stored values by default.

`vne` treats an env file as structured configuration, not text. You find a key, see what kind of value it wants, change it, and go back to the app. Comments, order, quoting, and multiline values stay as they were. An agent can hand you a `vne` command to run, and you type the secret yourself.

- Desktop app: Tauri 2 shell, Svelte interface, Rust parser.
- CLI: the same parser, one binary, `vne`.
- Local only: no telemetry, no accounts, no sync, no network path for env data.

![vne design sketch](docs/assets/vne-minimal-design.png)

## Install

Requires Rust 1.90 or newer, Node matching `^20.19.0 || >=22.12.0` (the engine range for Vite 8.2.2), and the [Tauri 2 prerequisites](https://tauri.app/start/prerequisites/).

```sh
npm install
npm run install:local
```

Use `install:local` rather than plain `cargo install --path src-tauri`. Tauri release binaries need the `custom-protocol` feature to load the bundled interface, and the script sets it. Release packaging is not done yet.

## Use

Open the desktop editor on a project:

```sh
vne .
```

Or work from the CLI. Commands that read or write one key:

```sh
vne create .env                          # empty, owner-private; never overwrites
vne add .env FEATURE_FLAG=true           # non-secret value: fine as an argument
vne add .env OPENAI_API_KEY --prompt     # secret: typed locally, hidden
vne set .env OPENAI_API_KEY --prompt     # replace one existing value
vne rename .env OPENAI_KEY OPENAI_API_KEY
vne rm .env STALE_FLAG
vne copy ../other/.env DATABASE_URL .env # value never leaves the Rust process
vne example .env                         # add missing keys to .env.example as blanks
```

Commands that look at files:

```sh
vne inspect .                              # every env file in a project
vne check .env --example .env.example      # missing, extra, shared, duplicate keys
vne where OPENAI_API_KEY . ~/.config/keys  # which file holds a key, and whether it is set
```

`where` searches the current directory when given no path, and only the paths you name otherwise.

`vne help` lists every option; `vne <command> --help` has examples.

### Run a command with keys

```sh
vne run ~/.config/keys/fal.env -- falgen image "a lighthouse"
vne run .env .env.local -- npm run dev
```

`run` loads the files into that one command's environment. Your calling shell does not receive the loaded keys. The command can pass them to its own child processes. This replaces `set -a; source keys.env`, which puts every key into every process, where `env`, `ps eww`, or a crash dump can print them.

Files are read as data, not shell code. `run` refuses to start if a line is not an entry, an entry is malformed or repeated, a key is defined in two files, or a quoted value uses an escape other than `\\` and an escaped quote. It also refuses `env`, `printenv`, and `sh -c` scripts that list the environment. That check catches a slip, not a caller who wants the value: any program you run can print what it receives. Reasoning: [ADR 0005](docs/adr/0005-run-loads-env-files-into-one-child.md).

## For agents

Project scans report missing package-script and Compose env references as informational notices. They do not change exit codes. Project-contained unreadable or non-regular references, and unresolved reference locations, produce incomplete records. References resolved outside the project are excluded. Missing-path notices retain up to 10,000 paths and disclose when additional notices were omitted.

Start with `vne where <KEY> [path...]` before saying a key is unavailable. Name each directory to search; the default is the current project only. Use `inspect`, `check`, and `where` without `--values` to inspect metadata.

Read `valueState` (`set`, `empty`, or `placeholder`) for the stored value's state. The strings `[value withheld by output policy]` and `[raw content withheld by output policy]` are markers, not stored values; do not test `value`, `displayValue`, or `content` for presence.

Run mutations only when the user requests them. Known non-secret values can be added as arguments or set through stdin. For secret entry, give the user `vne add <file> <KEY> --prompt` or `vne set <file> <KEY> --prompt` to run locally. Agents must not obtain or pass real secret values as arguments or stdin. `copy` transfers an existing value internally and returns only a receipt.

`run` gives values to the program it starts. Use it only for authorized commands whose output does not print the environment; any program can print what it receives. Use the desktop, `--values`, and `format --dry-run` for local human inspection, rather than putting real env contents into an agent's context.

Named file reads require a regular UTF-8 file of at most 10 MiB. An explicitly named symlink may point to a regular file. Mutations refuse an update that would make the file exceed that read limit.

## What the editor and CLI understand

- **Key meaning.** URL/DSN, secret, public frontend variable, bool, int, port, JSON, PEM, and common provider keys are labelled inline. A public-prefixed name that looks secret, such as `NEXT_PUBLIC_API_KEY`, is flagged as browser-exposed.
- **Duplicates.** `add` refuses a duplicate and prints the existing line numbers. The editor asks which occurrence to change. `rm` needs `--line <N>` or `--all` for a duplicated key.
- **Faithful edits.** One occurrence changes; comments, order, quote style, `export` prefixes, and multiline values around it do not. Tests cover CRLF, UTF-8 BOM, empty values, inline comments, `#` inside values, and the [adversarial corpus](fixtures/adversarial-dotenv).
- **File discovery.** `.env`, `.env.local`, `.env.production`, `.env.example`, `.env.sample`, `.envrc`, `.flaskenv`, plus files named by `package.json` scripts (`--env-file`, `dotenv -e`, `env-cmd -f`, `DOTENV_CONFIG_PATH`) and Docker Compose `env_file`. A symlink or special file with one of these names in the project directory is not followed; the scan reports it as not read.
- **Layers.** Reports overrides across real env files. It names a winner only when Next.js or Vite load-order rules explain it.
- **Git exposure.** `inspect` and `check` report each file, and `where` each match, as `tracked`, `untrackedIgnored`, `untrackedNotIgnored`, `outsideRepository`, or `unknown`. Every write warns on stderr if it landed in a tracked file. The warning never blocks the write.
- **Refuses what it cannot prove.** `rename` refuses when the old key is gone and the new one is present, because file state cannot show that a rename produced it. `--expect present|absent` turns a wrong belief about a key into exit 2.

## Privacy

The CLI withholds stored values from default inspection output. The desktop and explicit value modes use the output policy shown below.

| Surface | What it shows |
| --- | --- |
| Desktop snapshot | Secret-like values, secret-like inline comments, and credential-bearing URLs redacted. Reveal fetches one selected occurrence into the local webview and clears it on hide, selection change, reload, or save. |
| CLI JSON (piped output) | No values, comments, or raw previews. Each entry's `valueState` says `set`, `empty`, or `placeholder`. `--values` adds values the classifier judges non-secret; detected secrets stay redacted. |
| `create`, `copy`, `set`, `rm`, `rename`, `example` | Receipts only: disposition, paths, key names, line numbers. |
| `where` | Key name, paths, line numbers, git status, each value's state, and which discovered files it did not read. |
| `run` | Does not print loaded env values. The command it starts receives them and can print them. |
| `format --dry-run` | Prints raw values. Refuses piped stdout and asks for an interactive `y`. |

Classification is heuristic: a sensitive value that matches an allowed format can appear in the desktop or under `--values`. Default CLI inspection output withholds values regardless of classification.

Some facts about values do appear: a disposition such as `alreadyPresent` tells the caller that the value it supplied equals the stored one, and `valueState` tells whether a value is empty or a template such as `changeme`. And any local process with file access can read env files directly. Threat model and release checklist: [docs/THREAT_MODEL.md](docs/THREAT_MODEL.md).

## Development

```sh
npm run dev          # browser preview on fake data (127.0.0.1:1420)
npm run tauri dev    # desktop app
npm run check        # svelte-check
npm run build
npm run test         # vitest
npm run test:rust    # cargo test
scripts/security-check.sh
```

Run the CLI from source:

```sh
cargo run --manifest-path src-tauri/Cargo.toml --bin vne -- inspect fixtures/demo
```

`fixtures/demo` holds fake env files for trying the editor; in the native app, choose **Choose folder** and select it. `fixtures/holdouts` holds small fake Next.js and Vite projects for the load-order checks.

Design decisions are recorded in [docs/adr](docs/adr).

## Status

The first public distribution is source-only. Native behavior is tested on macOS with Apple Silicon; other platforms are unverified.

Version 0.1.0, pre-release. No packaged installer yet; install from source as above.

## License

[MIT](LICENSE).
