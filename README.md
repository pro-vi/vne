# vne

Let your coding agent work on `.env` files without reading them.

A coding agent that reads `.env` sends every value in it to the model. Block the file and the agent can't tell which keys exist, so it asks you to open the file and move keys around by hand.

vne does the env-file work for the agent. It answers with key names, file paths, line numbers and whether each value is set, and it starts commands with the keys loaded. When a step needs a secret typed in, the agent gives you one command to run.

```sh
vne where STRIPE_SECRET_KEY . ~/.config/keys   # which file holds it, and is it set
vne check .env --example .env.example         # which keys are missing
vne copy .env DATABASE_URL apps/web/.env      # move a value; prints a receipt, not the value
vne run .env -- npm test                      # start one command with the keys loaded
vne add .env OPENAI_API_KEY --prompt          # for you: type the secret, hidden
```

`vne .` opens a small macOS window for when you want to look yourself: hold ⌥ to see a value, ⌘C to copy one, ⏎ to fill the empty ones.

## Install

Requires Rust 1.90 or newer, Node matching `^20.19.0 || >=22.12.0` (the engine range for Vite 8.2.2), and the [Tauri 2 prerequisites](https://tauri.app/start/prerequisites/).

```sh
npm install
npm run install:local
```

Use `install:local` rather than plain `cargo install --path src-tauri`: Tauri release binaries need the `custom-protocol` feature to load the bundled interface, and the script sets it.

## What an agent does with it

The commands below print key names, paths, line numbers and states, not stored values. On a terminal the output is text; piped, which is what an agent sees, it is JSON.

### Find a key

```sh
vne where STRIPE_SECRET_KEY . ../shop-main ~/.config/keys
```

```text
STRIPE_SECRET_KEY
  ~/vne-demo/shop/.env.example:4  empty, git untracked
  ~/vne-demo/keys/stripe.env:1  set, outside git
```

Run it before telling the user a key is missing. Each match gives the file, the line, `set`, `empty` or `placeholder`, and git status. With no path it searches the current directory; otherwise only the paths you name. Directories are not searched recursively. Exit 0 means found, 1 not found, 2 a file could not be read.

### See what a project needs

```sh
vne check .env --example .env.example
```

```text
contract: 2 shared, 3 missing, 0 extra, 0 duplicate
  missing DATABASE_URL
  missing NEXTAUTH_SECRET
  missing STRIPE_SECRET_KEY
```

`check` compares one file with its example and exits 1 when keys are missing, extra or duplicated, so an agent or a script can branch on it. `vne inspect .` covers every env file in the project: the keys in each, overrides between layers, and whether git tracks each file.

### Change the files

| To | Run | Notes |
| --- | --- | --- |
| Start a `.env` | `vne create .env` | Empty and owner-private; never overwrites. |
| Add a key that is not secret | `vne add .env FEATURE_SEARCH=true` | Refuses a key that already exists. Warns when a secret-looking value arrives as an argument. |
| Change a value that is not secret | `printf '%s' 4000 \| vne set .env PORT --stdin` | `set` refuses values given as arguments. |
| Rename a key | `vne rename .env OPENAI_KEY OPENAI_API_KEY` | Value, quotes, comment and line position stay byte-identical. One file per run. |
| Remove a key | `vne rm .env STALE_FLAG --expect present` | A duplicated key needs `--line N` or `--all`. `--expect` turns a wrong belief about the key into exit 2. |
| Keep the example in step | `vne example .env` | Adds keys missing from `.env.example` as blanks, never values. |
| Copy a value between files | `vne copy .env DATABASE_URL apps/web/.env` | The value stays inside vne. Replacing a different value needs `--overwrite`. |

Comments, order, quoting and `export` prefixes around the change stay as they were. A write to a file git tracks prints a warning on stderr.

When its output is not a terminal, as with an agent or a script, `copy` writes a secret-looking value only into a file under the source file's folder, and refuses anything else. To bring a secret from another project, the agent hands you the command; run from your terminal, it copies between projects.

### Run commands with keys

```sh
vne run .env -- npm test
vne run .env -- npx prisma migrate dev
vne run .env .env.local -- npm run dev
vne run keys.env -- sh -c 'printf "Authorization: Bearer %s\n" "$API_KEY" | curl -H @- https://api.example.com'
```

`run` starts one command with the files' keys in its environment. The calling shell does not receive them, and the command can pass them to its own child processes. The last example hands a key to `curl` on stdin, so it never appears in a process list.

- A key defined in two of the files refuses the run rather than picking one. A key from a file replaces an inherited variable of the same name.
- Files are read as data, not shell code: nothing is expanded or executed. A line that is not an entry, a malformed or repeated entry, or a quoted value with an escape other than `\\` and an escaped quote refuses the run.
- `env`, `printenv`, and `sh -c` scripts that list the environment are refused. That catches a slip, not a command that wants to print what it receives.
- The exit status is the command's own. A refusal exits 2; a command that cannot start exits 127 or 126, as a shell would.

This replaces `set -a; source .env`, which puts every key into every later process, where `env`, `ps eww`, or a crash dump can print them. Reasoning: [ADR 0005](docs/adr/0005-run-loads-env-files-into-one-child.md).

### Hand a secret to you

When a step needs a value the agent should not see, it gives you a command for your own terminal:

```sh
vne add .env STRIPE_SECRET_KEY --prompt        # new key; what you type is hidden
vne set .env STRIPE_SECRET_KEY --prompt        # replace an existing value
vne copy ../shop-main/.env DATABASE_URL .env   # a secret from another folder
```

`--prompt` needs an interactive terminal. You can also fill values in the window (`vne .`).

## Pick how much your agent may do

| Level | The agent may run | Writes files | A command receives values |
| --- | --- | --- | --- |
| Look | `where`, `check`, `inspect` | no | no |
| Edit | Look, plus `create`, `add`, `set --stdin`, `rename`, `rm`, `example`, `copy` | yes | no |
| Run | Edit, plus `run` for the commands you name | yes | yes, the command `run` starts |

At every level these stay yours: `add --prompt`, `set --prompt`, copying a secret from another folder, `--values`, `format --dry-run`, and the window.

Put this in `AGENTS.md` or `CLAUDE.md`, and delete the levels you don't grant:

```markdown
## .env files

Use `vne` for env files. Don't open them with file tools, `cat` or `source`.

Look:
- Before saying a key is missing, run `vne where <KEY> . <other folders with env files>`.
- Use `vne check <file> --example <example>` and `vne inspect .` to see which keys exist or are missing. They print key names, line numbers and a `valueState` of set, empty or placeholder; `[value withheld by output policy]` is a marker, not a value.

Edit:
- When I ask for a change, run `create`, `rename`, `rm`, `example` and `copy` yourself, and `add` or `set --stdin` for a value that is not a secret.

Run:
- Start a command that needs keys with `vne run <file> -- <command>`, and only a command that doesn't print its environment.

Mine:
- For a secret, give me `vne add <file> <KEY> --prompt` or `vne set <file> <KEY> --prompt` to run in my terminal. Do the same for a `vne copy` of a secret from another folder; vne refuses that copy when you run it.
- Don't use `--values` or `format --dry-run`; they print values.
```

`vne help` opens with a shorter version of these rules, for an agent that reads help first.

In Claude Code, add matching rules to `.claude/settings.json` (rule syntax as of Claude Code 2.1.289). At every level, deny the file tools on env files:

```json
"deny": ["Read(.env)", "Read(.env.*)", "Read(!.env.example)"]
```

Then allow the commands of your level, so they run without asking:

| Level | Add to `allow` |
| --- | --- |
| Look | `"Bash(vne where *)", "Bash(vne check *)", "Bash(vne inspect *)"` |
| Edit | Look's rules, plus `"Bash(vne create *)", "Bash(vne add *)", "Bash(vne set *)", "Bash(vne rename *)", "Bash(vne rm *)", "Bash(vne example *)", "Bash(vne copy *)"` |
| Run | Edit's rules, plus one rule per command, such as `"Bash(vne run .env -- npm test)"` |

Don't allow `Bash(vne run *)`. Claude Code matches it against whatever follows `run`, so it would approve any command the agent starts through vne.

A `Read` deny rule covers Claude Code's file tools and the shell commands it recognizes, such as `cat` and `head`. It does not cover a program that opens the file itself. That is why `vne` keeps working, and why none of this is a sandbox.

## The window

`vne .` opens the project in a macOS window that does three things:

- **See a value.** Secrets are masked. Hold ⌥ to show the selected one; let go and it hides.
- **Copy a value.** ⌘C copies the selected value without it entering the window. A secret is kept to this Mac, marked concealed so clipboard history apps that check the mark skip it, and cleared after 30 seconds or when the app quits.
- **Fill values.** ⏎ edits a row and ⏎ saves it. After a save, the next empty, placeholder or missing row opens. ⌘N adds a key. A project with an example file and no `.env` gets a button that creates `.env` from the example.

Removing and renaming keys stay in the CLI. For a duplicated key, the window shows the `vne rm` command that keeps the selected line.

## Output and exit codes

- Text on a terminal; JSON when piped, unless `--text`. `--json` is compact, `--pretty` formatted.
- Each JSON entry's `valueState` is `set`, `empty` or `placeholder` (such as `changeme`). The strings `[value withheld by output policy]` and `[raw content withheld by output policy]` are markers, not values; read `valueState` rather than testing `value`, `displayValue` or `content`.
- `--values` adds the values the classifier judges non-secret. It is meant for a person at a terminal.
- Exit 0: done, or `check` / `inspect` found nothing to report. Exit 1: `check` or `inspect` found diagnostics or missing, extra or duplicate keys, or `where` did not find the key. Exit 2: an error or a refusal. `run` exits with its command's status.
- `vne --version` prints the version and the commit the binary was built from.

## What vne understands

- **Key meaning.** URL/DSN, secret, public frontend variable, bool, int, port, JSON, PEM, and common provider keys are labelled. A public-prefixed name that looks secret, such as `NEXT_PUBLIC_API_KEY`, is flagged as browser-exposed.
- **Faithful edits.** One occurrence changes; comments, order, quote style, `export` prefixes, and multiline values around it do not. Tests cover CRLF, UTF-8 BOM, empty values, inline comments, `#` inside values, and the [adversarial corpus](fixtures/adversarial-dotenv).
- **File discovery.** `.env`, `.env.local`, `.env.production`, `.env.example`, `.env.sample`, `.envrc`, `.flaskenv`, plus files named by `package.json` scripts (`--env-file`, `dotenv -e`, `env-cmd -f`, `DOTENV_CONFIG_PATH`) and Docker Compose `env_file`. A symlink or special file with one of these names in the project directory is not followed; the scan reports it as not read. A referenced file that is missing is reported as a notice and does not change the exit code.
- **Layers.** Reports overrides across real env files. It names a winner only when Next.js or Vite load-order rules explain it.
- **Git exposure.** `inspect` and `check` report each file, and `where` each match, as `tracked`, `untrackedIgnored`, `untrackedNotIgnored`, `outsideRepository`, or `unknown`.
- **Refuses what it cannot prove.** `rename` refuses when the old key is gone and the new one is present, because file state cannot show that a rename produced it. `rm --line N` never goes looking for another occurrence when line N does not hold the key.

Named file reads require a regular UTF-8 file of at most 10 MiB. An explicitly named symlink may point to a regular file. Writes refuse an update that would make the file exceed that limit.

## Privacy

| Surface | What it shows |
| --- | --- |
| `inspect`, `check` | No values, comments, or raw previews by default. `--values` adds values the classifier judges non-secret; detected secrets stay withheld. |
| `where` | Key name, paths, line numbers, git status, each value's state, and which discovered files it did not read. |
| `create`, `add`, `set`, `copy`, `rm`, `rename`, `example` | Receipts: disposition, paths, key names, line numbers. |
| `run` | Prints nothing of its own. The command it starts receives the values and can print them. |
| `format --dry-run` | Prints raw values. Refuses piped stdout and asks for an interactive `y`. |
| The window | Secret-like values, secret-like inline comments and credential-bearing URLs are masked. Holding ⌥ fetches the selected value into the window; letting go, switching away or moving the selection hides it. ⌘C copies in the Rust process, and the window never receives the value. |

Some facts about values do appear. A disposition such as `alreadyPresent` tells the caller that the value it supplied equals the stored one, and `valueState` tells whether a value is empty or a template such as `changeme`.

## What vne does not do

- **Encrypt.** Your `.env` files stay plain text, where your framework expects them.
- **Sandbox the agent.** Any program the agent runs can open `.env` directly. vne gives a well-behaved agent no reason to; enforcement belongs to your agent's sandbox or the operating system.
- **Classify perfectly.** Secret detection is a heuristic. Default CLI output withholds every value regardless; `--values` and the window rely on it.
- **Keep `run`'s command quiet.** The command receives the values and can print them.

Threat model and release checklist: [docs/THREAT_MODEL.md](docs/THREAT_MODEL.md). Design decisions: [docs/adr](docs/adr).

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

`fixtures/demo` holds fake env files for trying the window. `fixtures/holdouts` holds small fake Next.js and Vite projects for the load-order checks.

## Status

Version 0.1.0, pre-release. Source-only; no packaged installer yet. Native behavior is tested on macOS with Apple Silicon; other platforms are unverified.

## License

[MIT](LICENSE).
