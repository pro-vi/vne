# ADR 0005: `vne run` Loads Env Files Into One Child Process

Output scope: default CLI inspection withholds stored values. Human value-display modes and the program launched by `run` can expose values; that program can also pass its environment to its children. See [the current threat model](../THREAT_MODEL.md#known-residual-risks).

- **Status:** Accepted
- **Date:** 2026-09-23
- **Deciders:** Provi, Claude
- **Changes:** vne's runtime role, recorded as "none — edit-time only" in
  `docs/research/dopbase-2026-09-15.md`

## Context

A developer who keeps API keys in env files still has to get them into the
programs that use them. The common way is a shell startup file that runs
`set -a` and then `source`s each file. That exports every key into every shell
and every process those shells start, whether or not the process needs a key.

That broad exposure is a problem once coding agents run shells. An agent's
shell carries every key, so any command that prints an environment writes them
into the agent's transcript: a bare `env`, `printenv`, `ps` with an environment
flag, a crash dump. On the author's machine that happened twice in one week.
Both times the cause was a slip in a command that had nothing to do with the
keys. The goal, in the author's words: agents should "be able to load …
these keys, but without it being … always visible."

The fix is to load keys at the moment a program starts, into that program only.
dopbase calls this env-at-exec and builds its `run` command on it. vne already
owns the other half of the job: it parses env files losslessly and never prints
a value. `source` is the wrong loader for this: it runs the file as shell code.
`$` expands, command substitution runs, and any line that is not an assignment
is executed as a command.

## Decision

`vne run <file>... -- <command> [args...]` reads each env file, adds its keys
to the environment of `<command>` only, and starts it. vne prints nothing when
the command starts. Its only output is refusals and errors, and those name
files, keys and line numbers, never a value.

The loader treats the files as data and refuses whatever it cannot read
unambiguously. When it refuses, the command is not started:

- Every line must be blank, a comment, or a well-formed entry. A line that is
  none of these refuses the run. `source` would have executed that line;
  skipping it silently would hide the mismatch.
- An entry with a parser diagnostic refuses the run. So does a key that occurs
  twice in one file.
- A key defined in two of the given files refuses the run, naming both files.
  File order cannot prove which value the caller meant.
- Values are literal. Nothing is expanded, substituted or executed. Inside
  quotes, the only escapes read are the two vne's own writer produces: a
  backslash before a backslash, and a backslash before the value's own quote
  character. Any other backslash escape refuses the run, because shells and
  dotenv libraries read it three different ways. A NUL byte in a value also
  refuses the run.
- A key from a file replaces a variable of the same name inherited from the
  caller. Naming the file is the request for its value.

`vne run` refuses a command whose purpose is to print an environment:
`env` and `printenv` as the command, and a `-c` script for `sh`, `bash`, `zsh`,
`dash` or `ksh` in which `env` or `printenv` starts a command, or `export`,
`declare`, `typeset` or `set` starts a command with no operands other than
flags. This check only narrows the risk. Any program the caller chooses can
still print what it received. It exists to catch the slip that caused both
incidents, not a caller who means to read the value.

On Unix, vne replaces its own process with the command (`exec`), so signals,
stdio and the exit status belong to the command itself. On other platforms it
starts the command as a child, waits, and exits with the child's status. vne's
own refusals exit 2, like every other vne error. When the command cannot be
started, vne exits the way a shell would: 127 when the program is not found,
126 when it cannot be run.

## Rationale

- **Load per process, not per shell.** A key that exists only in the process
  that needs it cannot be printed by a command that does not need it. No output
  filter can promise that, because commands that print an environment take too
  many forms to list.
- **One named command instead of a shell idiom.** `( set -a; . file; exec cmd )`
  works, but only when every caller writes it correctly, and it keeps the
  `source` semantics described above. A named command gives an agent one rule
  to follow, and gives a hook one pattern to allow.
- **Refuse rather than guess.** This is ADR 0003's rule applied to reading.
  When the file cannot prove what value the caller meant, vne starts nothing.
  A tool that fails with "key missing" is safer than one that runs with a
  wrong or partly expanded key.
- **No server, no store.** Keys stay in the files the developer already keeps,
  edited with `vne set --prompt`. dopbase's encrypted store, roles and audit
  log answer a team's problem, not a single machine's.

## Consequences

- vne gains a runtime role. The dopbase research note's "edit-time only" no
  longer describes it.
- vne's invariant still holds: no vne output path emits a value. But `run`
  hands values to a program the caller picks, so a caller can print them on
  purpose, for example `vne run keys.env -- node -e 'console.log(process.env)'`.
  The research note described the same asymmetry in dopbase's `run`. vne now
  has it too, and `docs/THREAT_MODEL.md` lists it as a residual risk.
- While the command runs, its environment is readable by same-user tools that
  read process environments.
- `vne run` is CLI-only. No Tauri command is added.

## Alternatives Considered

- **Keep exporting from shell startup, and block environment-printing commands
  in agent hooks.** This keeps every key in every shell and relies on a pattern
  list to catch every form of the slip. It is still worth having as a backup;
  it does not work as the main control.
- **OS keychain.** It protects the file at rest. That was not the exposure, and
  a caller still has to fetch the key to use it.
- **A local secrets server (dopbase's model).** It solves team sharing and
  audit, which this problem does not need, at the cost of a daemon and a store.

## Revisit Triggers

- A caller needs `$` expansion or references between keys. That is a
  templating feature and needs its own decision.
- An agent runtime sandboxes processes so that the agent cannot read the env
  files. `run` then becomes the only path to a key, and the environment-printing
  check becomes a real boundary worth hardening.

## References

- `docs/research/dopbase-2026-09-15.md` (runtime role, `run` asymmetry)
- ADR 0003 (refuse what file state cannot prove)
