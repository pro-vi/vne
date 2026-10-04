use crate::{
    append_env_key_result, compare_env_files, copy_env_key, ensure_project_env_file,
    infer_key_shape, load_runtime_env, parse_env_file, redact_env_file_values_only,
    redact_snapshot_values_only, remove_env_key, rename_env_key, set_env_key, sync_example_file,
    withhold_all_env_file_payloads, withhold_all_snapshot_payloads, EnsureEnvFileOutcome,
    EnvComparison, EnvFile, EnvFileCreationDisposition, EnvFileGitStatus, EnvKeyCopyDisposition,
    EnvKeyExpectation, EnvKeyRemoveDisposition, EnvKeyRemoveSelector, EnvKeySetDisposition,
    EnvValueState, ExampleSyncDisposition, IncompleteEnvFile, ProjectSnapshot,
};
use serde::Serialize;
use std::env;
use std::ffi::OsStr;
use std::fmt;
use std::fs;
use std::io::{self, BufRead, IsTerminal, Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;
use std::process::ExitCode;

type ArgIter = std::vec::IntoIter<String>;

struct CliCommand {
    name: &'static str,
    parse: fn(ArgIter) -> Result<Command, ParseError>,
}

const CLI_COMMANDS: &[CliCommand] = &[
    CliCommand {
        name: "check",
        parse: parse_check_args,
    },
    CliCommand {
        name: "inspect",
        parse: parse_inspect_args,
    },
    CliCommand {
        name: "format",
        parse: parse_format_args,
    },
    CliCommand {
        name: "add",
        parse: parse_add_args,
    },
    CliCommand {
        name: "set",
        parse: parse_set_args,
    },
    CliCommand {
        name: "rm",
        parse: parse_rm_args,
    },
    CliCommand {
        name: "rename",
        parse: parse_rename_args,
    },
    CliCommand {
        name: "example",
        parse: parse_example_args,
    },
    CliCommand {
        name: "copy",
        parse: parse_copy_args,
    },
    CliCommand {
        name: "create",
        parse: parse_create_args,
    },
    CliCommand {
        name: "run",
        parse: parse_run_args,
    },
    CliCommand {
        name: "where",
        parse: parse_where_args,
    },
];

const CLI_HELP_ALIASES: &[&str] = &["help", "-h", "--help"];
const CLI_VERSION_ALIASES: &[&str] = &["--version", "-V"];

fn cli_command(name: &str) -> Option<&'static CliCommand> {
    CLI_COMMANDS.iter().find(|command| command.name == name)
}

#[derive(Debug, PartialEq, Eq)]
enum Command {
    Check {
        file: PathBuf,
        example: Option<PathBuf>,
        output: OutputFormat,
        values: ValueOutput,
    },
    Inspect {
        dir: PathBuf,
        output: OutputFormat,
        values: ValueOutput,
    },
    Format {
        file: PathBuf,
        dry_run: bool,
    },
    Add {
        file: PathBuf,
        key: String,
        value: ValueSource,
        output: OutputFormat,
        values: ValueOutput,
    },
    Set {
        file: PathBuf,
        key: String,
        value: ValueSource,
        allow_empty: bool,
        output: OutputFormat,
    },
    Remove {
        file: PathBuf,
        key: String,
        selector: EnvKeyRemoveSelector,
        expectation: Option<EnvKeyExpectation>,
        output: OutputFormat,
    },
    Rename {
        file: PathBuf,
        from: String,
        to: String,
        output: OutputFormat,
    },
    Example {
        file: PathBuf,
        example: Option<PathBuf>,
        output: OutputFormat,
    },
    Copy {
        source_file: PathBuf,
        key: String,
        destination_file: PathBuf,
        overwrite: bool,
        output: OutputFormat,
    },
    Create {
        file: PathBuf,
        output: OutputFormat,
    },
    Run {
        files: Vec<PathBuf>,
        program: String,
        args: Vec<String>,
    },
    Where {
        key: String,
        paths: Vec<PathBuf>,
        output: OutputFormat,
    },
    Help(HelpTopic),
    Version,
}

#[derive(Debug, PartialEq, Eq)]
enum ValueSource {
    Inline(String),
    Prompt,
    Stdin,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OutputFormat {
    Auto,
    Text,
    Json,
    PrettyJson,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ValueOutput {
    Hidden,
    Classified,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HelpTopic {
    General,
    Check,
    Inspect,
    Add,
    Set,
    Remove,
    Rename,
    Example,
    Copy,
    Create,
    Format,
    Run,
    Where,
}

#[derive(Debug, PartialEq, Eq)]
struct ParseError {
    message: String,
    topic: HelpTopic,
}

impl ParseError {
    fn new(topic: HelpTopic, message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            topic,
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CheckOutput {
    file: EnvFile,
    example: Option<EnvFile>,
    comparison: Option<EnvComparison>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct WhereOutput {
    key: String,
    found: bool,
    matches: Vec<WhereMatch>,
    searched: Vec<WhereSearch>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct WhereMatch {
    path: String,
    line_number: usize,
    value_state: EnvValueState,
    git_status: EnvFileGitStatus,
}

/// What one path argument covered, so a miss shows its own scope: a directory
/// with no env files reads differently from a key absent from ten files.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct WhereSearch {
    path: String,
    files: Vec<String>,
    incomplete: Vec<IncompleteEnvFile>,
    missing_references: Vec<crate::EnvFinding>,
}

pub fn is_cli_invocation(args: &[String]) -> bool {
    args.first().is_some_and(|arg| {
        cli_command(arg).is_some()
            || CLI_HELP_ALIASES.contains(&arg.as_str())
            || CLI_VERSION_ALIASES.contains(&arg.as_str())
    })
}

pub fn run_from_env() -> ExitCode {
    run_from_args(env::args().skip(1))
}

pub fn run_from_args(args: impl IntoIterator<Item = String>) -> ExitCode {
    match parse_args(args) {
        Ok(Command::Help(topic)) => exit_after_print(print_usage(topic)),
        Ok(Command::Version) => {
            exit_after_print(stdout_line(format_args!("{}", version_text())))
        }
        Ok(Command::Run {
            files,
            program,
            args,
        }) => run_with_env(&files, &program, &args),
        Ok(command) => match run(command) {
            Ok(has_findings) => {
                if has_findings {
                    ExitCode::from(1)
                } else {
                    ExitCode::SUCCESS
                }
            }
            Err(error) => {
                let _ = stderr_line(format_args!("vne: {error}"));
                ExitCode::from(2)
            }
        },
        Err(error) => {
            let _ = stderr_line(format_args!("vne: {}", error.message));
            let _ = stderr_line(format_args!(""));
            let _ = print_usage_to_stderr(error.topic);
            ExitCode::from(2)
        }
    }
}

fn exit_after_print(printed: io::Result<()>) -> ExitCode {
    if printed.is_ok() {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(2)
    }
}

fn run(command: Command) -> Result<bool, Box<dyn std::error::Error>> {
    match command {
        Command::Check {
            file,
            example,
            output,
            values,
        } => run_check(&file, example.as_deref(), output, values),
        Command::Inspect {
            dir,
            output,
            values,
        } => run_inspect(&dir, output, values),
        Command::Format { file, dry_run } => run_format(&file, dry_run),
        Command::Add {
            file,
            key,
            value,
            output,
            values,
        } => {
            // Warn first: the value is already in the process list and the
            // shell history, so a later failure does not un-expose it.
            if let ValueSource::Inline(value) = &value {
                warn_on_secret_like_argument(&key, value)?;
            }
            prepare_add_target(&file, matches!(&value, ValueSource::Prompt))?;
            let value = resolve_value_source(&key, value)?;
            run_add(&file, &key, &value, output, values)
        }
        Command::Set {
            file,
            key,
            value,
            allow_empty,
            output,
        } => {
            prepare_set_target(&file)?;
            let value = resolve_value_source(&key, value)?;
            run_set(&file, &key, &value, allow_empty, output)
        }
        Command::Remove {
            file,
            key,
            selector,
            expectation,
            output,
        } => run_rm(&file, &key, selector, expectation, output),
        Command::Rename {
            file,
            from,
            to,
            output,
        } => run_rename(&file, &from, &to, output),
        Command::Example {
            file,
            example,
            output,
        } => run_example(&file, example.as_deref(), output),
        Command::Copy {
            source_file,
            key,
            destination_file,
            overwrite,
            output,
        } => run_copy(&source_file, &key, &destination_file, overwrite, output),
        Command::Create { file, output } => run_create(&file, output),
        Command::Where { key, paths, output } => run_where(&key, &paths, output),
        Command::Run { .. } => unreachable!("run_from_args starts `run` itself"),
        Command::Help(_) | Command::Version => Ok(false),
    }
}

fn resolve_value_source(
    key: &str,
    source: ValueSource,
) -> Result<String, Box<dyn std::error::Error>> {
    match source {
        ValueSource::Inline(value) => Ok(value),
        ValueSource::Prompt => read_prompted_value(key),
        ValueSource::Stdin => read_stdin_value(),
    }
}

/// Warns when a value handed in as a command argument classifies secret-like.
/// The warning names the key only: process lists and shell history already hold
/// the value, and vne must not add a third copy of it.
fn warn_on_secret_like_argument(key: &str, value: &str) -> io::Result<()> {
    if !infer_key_shape(key, value).redacted_by_default {
        return Ok(());
    }

    // The key is caller-supplied and may itself be a `KEY=VALUE` assignment, so
    // it goes through the same elision as any other echoed argument.
    stderr_line(format_args!(
        "warning: `{}` looks secret-like and its value came from a command argument, which shell history and process lists can retain; use --stdin or --prompt instead",
        argument_label(key)
    ))
}

/// Withheld-value egress guard (loop-002 campaign t1c6, 2026-09-05): copy
/// is the one verb that moves a value the tool HOLDS but will not print —
/// the display redaction never sees file contents, so without this guard
/// `vne create /tmp/dest.env` + `vne copy .env SESSION_TOKEN /tmp/dest.env`
/// hands an agent the raw KEY=value line at a world-readable path. Policy:
/// a redacted-by-default value may only be copied within its own project
/// directory, OR when stdout is a real terminal — an interactive human
/// copying between projects is the tool's job; the terminal check is the
/// one gate no wrapper-mediated attacker could satisfy in the loop-002
/// campaigns (five pty/spoof attempts across four model families; the
/// wrapper re-pipes the subject's stdout). add/set are deliberately
/// unguarded: their values arrive from the caller, who already has them —
/// there is nothing to exfiltrate.
fn ensure_copy_target_within_source_project(
    source_file: &Path,
    key: &str,
    destination_file: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let source_dir = canonical_dir_of(source_file)?;
    let destination = fs::canonicalize(destination_file).map_err(|error| {
        io::Error::new(
            error.kind(),
            format!("could not resolve {}: {error}", destination_file.display()),
        )
    })?;
    if destination.starts_with(&source_dir) {
        return Ok(());
    }
    if io::stdout().is_terminal() {
        return Ok(()); // interactive use: cross-project copy is a feature
    }

    let source = parse_env_file(source_file, read_to_string_with_path(source_file)?);
    let Some(entry) = source.entries.iter().find(|entry| entry.key == key) else {
        return Ok(()); // copy_env_key reports the missing key itself
    };
    if !entry.shape.redacted_by_default {
        return Ok(());
    }
    Err(format!(
        "refusing to copy `{}` out of {}: its value is withheld by the output policy, \
         so it may only be copied within the project directory (or from a terminal)",
        key,
        source_dir.display()
    )
    .into())
}

fn canonical_dir_of(path: &Path) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let canonical = fs::canonicalize(path).map_err(|error| {
        io::Error::new(
            error.kind(),
            format!("could not resolve {}: {error}", path.display()),
        )
    })?;
    let parent = canonical
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("/"));
    Ok(parent)
}

fn prepare_set_target(file: &Path) -> Result<(), Box<dyn std::error::Error>> {
    match fs::metadata(file) {
        Ok(_) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Err(format!(
            "env file {} does not exist; run `vne create <file>` and `vne add <file> <KEY>` first",
            file.display()
        )
        .into()),
        Err(error) => Err(io::Error::new(
            error.kind(),
            format!("could not inspect env file {}: {error}", file.display()),
        )
        .into()),
    }
}

fn prepare_add_target(
    file: &Path,
    prompt_if_missing: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    match fs::symlink_metadata(file) {
        Ok(_) => return Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(io::Error::new(
                error.kind(),
                format!("could not inspect env file {}: {error}", file.display()),
            )
            .into());
        }
    }

    if !prompt_if_missing {
        return Err(format!(
            "env file {} does not exist; run `vne create <file>` first",
            file.display()
        )
        .into());
    }

    if !io::stdin().is_terminal() {
        return Err(
            "--prompt requires an interactive terminal; use `vne create <file>` first and --stdin for piped values"
                .into(),
        );
    }

    let stdin = io::stdin();
    let stderr = io::stderr();
    confirm_missing_env_file_creation(file, &mut stdin.lock(), &mut stderr.lock())
}

fn confirm_missing_env_file_creation(
    file: &Path,
    input: &mut impl BufRead,
    diagnostics: &mut impl Write,
) -> Result<(), Box<dyn std::error::Error>> {
    write!(
        diagnostics,
        "env file {} does not exist. Create it? [y/N] ",
        file.display()
    )?;
    diagnostics.flush()?;

    let mut response = String::new();
    input.read_line(&mut response)?;
    if !is_affirmative_confirmation(&response) {
        return Err(format!("add cancelled; {} was not created", file.display()).into());
    }

    let outcome = ensure_env_file_path(file)?;
    let action = match outcome.disposition {
        EnvFileCreationDisposition::Created => "created",
        EnvFileCreationDisposition::AlreadyExists => "using existing",
    };
    writeln!(diagnostics, "{action} {}", outcome.path)?;
    Ok(())
}

fn is_affirmative_confirmation(response: &str) -> bool {
    let response = response.trim();
    response.eq_ignore_ascii_case("y") || response.eq_ignore_ascii_case("yes")
}

fn read_prompted_value(key: &str) -> Result<String, Box<dyn std::error::Error>> {
    if !io::stdin().is_terminal() {
        return Err(
            "--prompt requires an interactive terminal; use --stdin for piped values".into(),
        );
    }

    if !set_terminal_echo(false) {
        return Err(
            "--prompt could not disable terminal echo; use --stdin or --value instead".into(),
        );
    }

    let read_result = (|| -> io::Result<String> {
        stderr(format_args!("value for {}: ", argument_label(key)))?;
        let mut value = String::new();
        io::stdin().read_line(&mut value)?;
        Ok(value)
    })();
    let echo_restored = set_terminal_echo(true);
    let _ = stderr_line(format_args!(""));
    if !echo_restored {
        return Err("--prompt could not restore terminal echo; run `stty echo`".into());
    }
    let value = read_result?;

    Ok(strip_one_trailing_line_ending(value))
}

fn read_stdin_value() -> Result<String, Box<dyn std::error::Error>> {
    let mut value = String::new();
    io::stdin().read_to_string(&mut value)?;
    Ok(strip_one_trailing_line_ending(value))
}

fn strip_one_trailing_line_ending(mut value: String) -> String {
    if value.ends_with('\n') {
        value.pop();
        if value.ends_with('\r') {
            value.pop();
        }
    }
    value
}

fn set_terminal_echo(enabled: bool) -> bool {
    set_terminal_echo_for_platform(enabled)
}

#[cfg(unix)]
fn set_terminal_echo_for_platform(enabled: bool) -> bool {
    ProcessCommand::new("stty")
        .arg(if enabled { "echo" } else { "-echo" })
        .status()
        .is_ok_and(|status| status.success())
}

#[cfg(not(unix))]
fn set_terminal_echo_for_platform(_enabled: bool) -> bool {
    false
}

fn run_check(
    file_path: &Path,
    example_path: Option<&Path>,
    output_format: OutputFormat,
    value_output: ValueOutput,
) -> Result<bool, Box<dyn std::error::Error>> {
    let mut paths = vec![file_path];
    paths.extend(example_path);
    let mut loaded = load_env_files(&paths, crate::GitStatusScope::AllFiles)?;
    let example = example_path.and_then(|_| loaded.pop());
    let file = loaded.remove(0);
    let comparison = example
        .as_ref()
        .map(|example| compare_env_files(&file, example));
    let has_findings = has_file_findings(&file)
        || example.as_ref().is_some_and(has_file_findings)
        || comparison.as_ref().is_some_and(has_comparison_findings);
    let output = CheckOutput {
        file: project_env_file_for_output(file, value_output),
        example: example.map(|file| project_env_file_for_output(file, value_output)),
        comparison,
    };

    if output_format.wants_json() {
        print_json(&output, output_format)?;
    } else {
        print_file_summary(&output.file)?;
        if let Some(example) = &output.example {
            print_file_summary(example)?;
        }
        if let Some(comparison) = &output.comparison {
            print_comparison(comparison)?;
        }
    }

    Ok(has_findings)
}

fn run_inspect(
    dir: &Path,
    output: OutputFormat,
    value_output: ValueOutput,
) -> Result<bool, Box<dyn std::error::Error>> {
    let snapshot = crate::snapshot_project(dir)?;
    let has_findings = has_snapshot_findings(&snapshot);
    let snapshot = project_snapshot_for_output(snapshot, value_output);

    if output.wants_json() {
        print_json(&snapshot, output)?;
    } else {
        stdout_line(format_args!(
            "{}: {} env files, {} findings",
            snapshot.root,
            snapshot.files.len(),
            snapshot.findings.len()
        ))?;
        for file in &snapshot.files {
            print_file_summary(file)?;
        }
        for notice in snapshot.findings.iter().filter(|finding| finding.action_kind == crate::MISSING_REFERENCE_ACTION) {
            stdout_line(format_args!("  {}", notice.title))?;
        }
        if let Some(comparison) = &snapshot.comparison {
            print_comparison(comparison)?;
        }
    }

    Ok(has_findings)
}

/// Reports every occurrence of `key` in the env files each path names or
/// discovers. Returns true, for exit 1, only when every discovered file was
/// read and none holds the key; a miss with files left unread is an error,
/// because one of them may hold the key or name an env file that does.
fn run_where(
    key: &str,
    paths: &[PathBuf],
    output_format: OutputFormat,
) -> Result<bool, Box<dyn std::error::Error>> {
    let mut matches = Vec::new();
    let mut searched = Vec::new();

    let git_scope = crate::GitStatusScope::FilesWithKey(key);
    for path in paths {
        let (root, files, incomplete, missing_references) = if path.is_dir() {
            let snapshot = crate::scan_project(path, git_scope).map_err(|error| {
                io::Error::new(
                    error.kind(),
                    format!("could not scan {}: {error}", path.display()),
                )
            })?;
            let missing = snapshot.findings.into_iter()
                .filter(|finding| finding.action_kind == crate::MISSING_REFERENCE_ACTION)
                .collect();
            (snapshot.root, snapshot.files, snapshot.incomplete, missing)
        } else {
            let file = load_env_files(&[path], git_scope)?.remove(0);
            (file.path.clone(), vec![file], Vec::new(), Vec::new())
        };

        for file in &files {
            matches.extend(
                file.entries
                    .iter()
                    .filter(|entry| entry.key == key)
                    .map(|entry| WhereMatch {
                        path: file.path.clone(),
                        line_number: entry.line_number,
                        value_state: entry.value_state,
                        git_status: file.git_status,
                    }),
            );
        }
        searched.push(WhereSearch {
            path: root,
            files: files.into_iter().map(|file| file.path).collect(),
            incomplete,
            missing_references,
        });
    }

    let found = !matches.is_empty();
    let unread = searched
        .iter()
        .map(|search| search.incomplete.len())
        .sum::<usize>();
    let output = WhereOutput {
        key: key.to_string(),
        found,
        matches,
        searched,
    };

    if output_format.wants_json() {
        print_json(&output, output_format)?;
    } else {
        print_where_text(&output)?;
    }

    if !found && unread > 0 {
        return Err(format!(
            "`{key}` was not found, but {unread} discovered file(s) were not read"
        )
        .into());
    }
    Ok(!found)
}

fn print_where_text(output: &WhereOutput) -> io::Result<()> {
    if output.found {
        stdout_line(format_args!("{}", output.key))?;
        for found in &output.matches {
            stdout_line(format_args!(
                "  {}:{}  {}, {}",
                found.path,
                found.line_number,
                found.value_state.label(),
                found.git_status.label()
            ))?;
        }
    } else {
        stdout_line(format_args!("{} not found", output.key))?;
    }

    for search in &output.searched {
        if !output.found {
            match search.files.len() {
                0 => stdout_line(format_args!("  {}: no env files", search.path))?,
                count => stdout_line(format_args!(
                    "  {}: {count} env file(s) searched",
                    search.path
                ))?,
            }
        }
        for skipped in &search.incomplete {
            stdout_line(format_args!(
                "  {}: not read: {} ({})",
                search.path, skipped.name, skipped.reason
            ))?;
        }
        for notice in &search.missing_references {
            stdout_line(format_args!("  {}: {}", search.path, notice.title))?;
        }
    }

    Ok(())
}

fn version_text() -> String {
    match option_env!("VNE_GIT_COMMIT") {
        Some(commit) => format!("vne {} ({commit})", env!("CARGO_PKG_VERSION")),
        None => format!("vne {}", env!("CARGO_PKG_VERSION")),
    }
}

fn run_format(file: &Path, dry_run: bool) -> Result<bool, Box<dyn std::error::Error>> {
    if !dry_run {
        return Err(
            "format currently requires --dry-run; vne does not rewrite files from the CLI yet"
                .into(),
        );
    }
    // Raw values may reach a human at a terminal, never a pipe: a piped
    // stdout is how agents and transcripts capture output (VNE-SEC-003).
    if !io::stdout().is_terminal() {
        return Err(
            "format --dry-run prints raw values and refuses piped stdout; run it from a terminal"
                .into(),
        );
    }
    if !io::stdin().is_terminal() {
        return Err(
            "format --dry-run prints raw values and requires an interactive terminal for confirmation"
                .into(),
        );
    }
    if !confirm_raw_format_output()? {
        stderr_line(format_args!("format --dry-run aborted"))?;
        // Aborted by choice reads as a finding (exit 1), not an error: the
        // command ran, examined its precondition, and declined to print.
        return Ok(true);
    }

    let content = read_to_string_with_path(file)?;
    let _ = parse_env_file(file, content.clone());
    stdout(format_args!("{content}"))?;
    Ok(false)
}

fn confirm_raw_format_output() -> Result<bool, Box<dyn std::error::Error>> {
    // The prompt goes to stderr so stdout stays exactly the raw file bytes
    // the human consented to see.
    stderr_line(format_args!(
        "format --dry-run will print raw values to this terminal. Continue? [y/N]"
    ))?;
    let mut answer = String::new();
    io::stdin().read_line(&mut answer)?;
    Ok(answer.trim().eq_ignore_ascii_case("y"))
}

fn run_add(
    file: &Path,
    key: &str,
    value: &str,
    output: OutputFormat,
    value_output: ValueOutput,
) -> Result<bool, Box<dyn std::error::Error>> {
    crate::ensure_mutation_target(file, "target").map_err(|e| format!("vne: {}", e.message()))?;
    let content = read_to_string_with_path(file)?;
    let updated =
        append_env_key_result(&content, key, value).map_err(|error| error.message(key))?;

    // Harden F0011 (2026-09-05): add is not exempt from the VNE-SEC-013
    // stale-overwrite precondition every other mutation honors — persist
    // only if the file still holds the bytes the append was computed from.
    crate::atomic_write_metadata_if_unchanged(file, &updated, Some(&content))
        .map_err(|e| format!("vne: {}", e))?;
    warn_when_tracked_by_git(file)?;

    if output.wants_json() {
        let loaded = load_env_files(&[file], crate::GitStatusScope::AllFiles)?.remove(0);
        print_json(&project_env_file_for_output(loaded, value_output), output)?;
    } else {
        stdout_line(format_args!("added `{key}` to {}", file.display()))?;
    }

    Ok(false)
}

fn run_set(
    file: &Path,
    key: &str,
    value: &str,
    allow_empty: bool,
    output: OutputFormat,
) -> Result<bool, Box<dyn std::error::Error>> {
    let outcome = set_env_key(file, key, value, allow_empty).map_err(|error| error.message(key))?;
    if outcome.disposition == EnvKeySetDisposition::Updated {
        warn_when_tracked_by_git(Path::new(&outcome.path))?;
    }

    if output.wants_json() {
        print_json(&outcome, output)?;
    } else {
        let state = match outcome.disposition {
            EnvKeySetDisposition::Updated => "updated",
            EnvKeySetDisposition::AlreadyPresent => "already present",
        };
        stdout_line(format_args!(
            "{state} `{}` in {}",
            outcome.key, outcome.path
        ))?;
    }

    Ok(false)
}

fn run_rm(
    file: &Path,
    key: &str,
    selector: EnvKeyRemoveSelector,
    expectation: Option<EnvKeyExpectation>,
    output: OutputFormat,
) -> Result<bool, Box<dyn std::error::Error>> {
    let outcome =
        remove_env_key(file, key, selector, expectation).map_err(|error| error.message(key))?;
    if outcome.disposition != EnvKeyRemoveDisposition::AlreadyAbsent {
        warn_when_tracked_by_git(Path::new(&outcome.path))?;
    }

    if output.wants_json() {
        print_json(&outcome, output)?;
    } else {
        match outcome.disposition {
            EnvKeyRemoveDisposition::Removed | EnvKeyRemoveDisposition::RemovedAll => {
                stdout_line(format_args!(
                    "removed `{}` from {} at {}",
                    outcome.key,
                    outcome.path,
                    crate::line_number_label(&outcome.removed_lines)
                ))?;
            }
            EnvKeyRemoveDisposition::AlreadyAbsent => {
                stdout_line(format_args!(
                    "already absent `{}` in {}",
                    outcome.key, outcome.path
                ))?;
            }
        }
    }

    Ok(false)
}

fn run_rename(
    file: &Path,
    from: &str,
    to: &str,
    output: OutputFormat,
) -> Result<bool, Box<dyn std::error::Error>> {
    let outcome = rename_env_key(file, from, to).map_err(|error| error.message(from, to))?;
    warn_when_tracked_by_git(Path::new(&outcome.path))?;

    if output.wants_json() {
        print_json(&outcome, output)?;
    } else {
        stdout_line(format_args!(
            "renamed `{}` to `{}` in {}",
            outcome.keys.from, outcome.keys.to, outcome.path
        ))?;
    }

    Ok(false)
}

fn run_example(
    file: &Path,
    example: Option<&Path>,
    output: OutputFormat,
) -> Result<bool, Box<dyn std::error::Error>> {
    let outcome = sync_example_file(file, example).map_err(|error| error.message())?;

    if output.wants_json() {
        print_json(&outcome, output)?;
    } else {
        let state = match outcome.disposition {
            ExampleSyncDisposition::Created => "created",
            ExampleSyncDisposition::Updated => "updated",
            ExampleSyncDisposition::AlreadyCurrent => "already current",
        };
        if outcome.added_keys.is_empty() {
            stdout_line(format_args!(
                "{state} {} from {}",
                outcome.example_path, outcome.source_path
            ))?;
        } else {
            stdout_line(format_args!(
                "{state} {} from {}, added {}",
                outcome.example_path,
                outcome.source_path,
                outcome.added_keys.join(", ")
            ))?;
        }
    }

    Ok(false)
}

fn run_create(file: &Path, output: OutputFormat) -> Result<bool, Box<dyn std::error::Error>> {
    let outcome = ensure_env_file_path(file)?;

    if output.wants_json() {
        print_json(&outcome, output)?;
    } else {
        let action = match outcome.disposition {
            EnvFileCreationDisposition::Created => "created",
            EnvFileCreationDisposition::AlreadyExists => "already exists",
        };
        stdout_line(format_args!("{action} {}", outcome.path))?;
    }

    Ok(false)
}

fn run_copy(
    source_file: &Path,
    key: &str,
    destination_file: &Path,
    overwrite: bool,
    output: OutputFormat,
) -> Result<bool, Box<dyn std::error::Error>> {
    ensure_copy_target_within_source_project(source_file, key, destination_file)?;
    let outcome = copy_env_key(source_file, key, destination_file, overwrite)
        .map_err(|error| error.message(key))?;
    if outcome.disposition != EnvKeyCopyDisposition::AlreadyPresent {
        warn_when_tracked_by_git(Path::new(&outcome.destination_path))?;
    }

    if output.wants_json() {
        print_json(&outcome, output)?;
    } else {
        let state = match outcome.disposition {
            EnvKeyCopyDisposition::Added => "added",
            EnvKeyCopyDisposition::Overwritten => "overwritten",
            EnvKeyCopyDisposition::AlreadyPresent => "already present",
        };
        stdout_line(format_args!(
            "{state} `{}` from {} to {}",
            outcome.key, outcome.source_path, outcome.destination_path
        ))?;
    }

    Ok(false)
}

/// `vne run` (ADR 0005): loads env files into one command's environment and
/// starts it. vne writes nothing on the way in, so the command's own output is
/// the only output; refusals exit 2 before the command starts.
fn run_with_env(files: &[PathBuf], program: &str, args: &[String]) -> ExitCode {
    if let Some(reason) = environment_printer_refusal(program, args) {
        let _ = stderr_line(format_args!("vne: {reason}"));
        return ExitCode::from(2);
    }
    let vars = match load_runtime_env(files) {
        Ok(vars) => vars,
        Err(error) => {
            let _ = stderr_line(format_args!("vne: {}", error.message()));
            return ExitCode::from(2);
        }
    };

    let mut command = ProcessCommand::new(program);
    command.args(args);
    for var in &vars {
        command.env(&var.key, &var.value);
    }
    start_command(command, program)
}

/// Replaces vne with the command, so signals, stdio and the exit status belong
/// to the command itself. Returns only when the command could not start.
#[cfg(unix)]
fn start_command(mut command: ProcessCommand, program: &str) -> ExitCode {
    use std::os::unix::process::CommandExt;

    let error = command.exec();
    command_start_failure(program, &error)
}

#[cfg(not(unix))]
fn start_command(mut command: ProcessCommand, program: &str) -> ExitCode {
    match command.status() {
        Ok(status) => ExitCode::from(
            status
                .code()
                .and_then(|code| u8::try_from(code).ok())
                .unwrap_or(1),
        ),
        Err(error) => command_start_failure(program, &error),
    }
}

/// Exits the way a shell does when a command cannot start: 127 when the
/// program is not found, 126 otherwise.
fn command_start_failure(program: &str, error: &io::Error) -> ExitCode {
    let _ = stderr_line(format_args!(
        "vne: could not start `{}`: {error}",
        argument_label(program)
    ));
    if error.kind() == io::ErrorKind::NotFound {
        ExitCode::from(127)
    } else {
        ExitCode::from(126)
    }
}

const ENVIRONMENT_PRINTERS: &[&str] = &["env", "printenv"];
const SCRIPT_SHELLS: &[&str] = &["sh", "bash", "zsh", "dash", "ksh"];

/// Refuses a command whose purpose is to print the environment `run` is about
/// to load. This narrows a slip; it is not a boundary, because any program the
/// caller picks can print what it receives.
fn environment_printer_refusal(program: &str, args: &[String]) -> Option<String> {
    let name = Path::new(program)
        .file_name()
        .and_then(OsStr::to_str)
        .unwrap_or(program);
    if ENVIRONMENT_PRINTERS.contains(&name) {
        return Some(format!(
            "run refuses `{name}` as the command: it would print the keys run loads; run the program that needs them instead"
        ));
    }
    if !SCRIPT_SHELLS.contains(&name) {
        return None;
    }
    let script = shell_script_operand(args)?;
    script_prints_environment(script)
        .then(|| format!("run refuses this `{name}` script: it would print the keys run loads"))
}

/// The script of `sh -c script`: the argument after the first short-option
/// cluster that contains `c`, such as `-c`, `-ec` or `-lc`.
fn shell_script_operand(args: &[String]) -> Option<&str> {
    let flag = args.iter().position(|arg| {
        arg.len() > 1
            && arg.starts_with('-')
            && !arg.starts_with("--")
            && arg[1..].chars().all(|c| c.is_ascii_alphabetic())
            && arg.contains('c')
    })?;
    args.get(flag + 1).map(String::as_str)
}

/// Whether any simple command in the script lists the environment: `env` or
/// `printenv`, a bare `set`, or `export`, `declare` or `typeset` with no
/// operands except the `-p`/`-x` listing flags. The split on shell operators is
/// deliberately coarse; quoting is not parsed.
fn script_prints_environment(script: &str) -> bool {
    script
        .split([';', '|', '&', '\n', '(', ')', '`', '{', '}'])
        .any(|segment| {
            let mut words = segment.split_whitespace().skip_while(|word| {
                matches!(*word, "exec" | "command" | "builtin") || word.contains('=')
            });
            let Some(first) = words.next() else {
                return false;
            };
            let first = first.rsplit('/').next().unwrap_or(first);
            match first {
                "env" | "printenv" => true,
                "set" => words.next().is_none(),
                "export" | "declare" | "typeset" => words.all(|word| {
                    word.len() > 1
                        && word.starts_with('-')
                        && word[1..].chars().all(|c| matches!(c, 'p' | 'x'))
                }),
                _ => false,
            }
        })
}

fn ensure_env_file_path(file: &Path) -> io::Result<EnsureEnvFileOutcome> {
    let raw_path = file.to_string_lossy();
    let terminal_component = raw_path.rsplit(['/', '\\']).next().unwrap_or_default();
    if terminal_component.is_empty() || matches!(terminal_component, "." | "..") {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "env file path must end in an env filename",
        ));
    }

    let name = file
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "env file path requires a UTF-8 filename",
            )
        })?;
    let parent = file
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    ensure_project_env_file(parent, name)
}

fn project_snapshot_for_output(
    snapshot: ProjectSnapshot,
    value_output: ValueOutput,
) -> ProjectSnapshot {
    match value_output {
        ValueOutput::Hidden => withhold_all_snapshot_payloads(snapshot),
        ValueOutput::Classified => redact_snapshot_values_only(snapshot),
    }
}

fn project_env_file_for_output(file: EnvFile, value_output: ValueOutput) -> EnvFile {
    match value_output {
        ValueOutput::Hidden => withhold_all_env_file_payloads(file),
        ValueOutput::Classified => redact_env_file_values_only(file),
    }
}

/// Loads env files in order, then asks git about the ones `git_scope` names in
/// one batch, so files sharing a directory share its git processes.
fn load_env_files(
    paths: &[&Path],
    git_scope: crate::GitStatusScope,
) -> Result<Vec<EnvFile>, Box<dyn std::error::Error>> {
    let mut files = Vec::with_capacity(paths.len());
    let mut real_paths = Vec::with_capacity(paths.len());
    for path in paths {
        let content = read_to_string_with_path(path)?;
        let real_path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        files.push(parse_env_file(&real_path, content));
        real_paths.push(real_path);
    }
    crate::fill_git_statuses(&mut files, &real_paths, git_scope, None);
    Ok(files)
}

/// One stderr line when a mutation just landed in a file git already tracks.
/// It never blocks the write and never names a value.
fn warn_when_tracked_by_git(path: &Path) -> io::Result<()> {
    if !crate::git::env_file_is_tracked(path) {
        return Ok(());
    }

    stderr_line(format_args!(
        "warning: {} is tracked by git; committing an env file can publish its secrets",
        path.display()
    ))
}

fn read_to_string_with_path(path: &Path) -> io::Result<String> {
    crate::read_regular_file_bounded(path).map_err(|error| {
        io::Error::new(
            error.kind(),
            format!("could not read {}: {error}", path.display()),
        )
    })
}

fn print_json<T: Serialize>(
    value: &T,
    output: OutputFormat,
) -> Result<(), Box<dyn std::error::Error>> {
    let serialized = if output == OutputFormat::PrettyJson {
        serde_json::to_string_pretty(value)?
    } else {
        serde_json::to_string(value)?
    };
    stdout_line(format_args!("{serialized}"))?;
    Ok(())
}

impl OutputFormat {
    fn wants_json(self) -> bool {
        match self {
            Self::Auto => !io::stdout().is_terminal(),
            Self::Text => false,
            Self::Json | Self::PrettyJson => true,
        }
    }
}

fn print_file_summary(file: &EnvFile) -> io::Result<()> {
    let redacted = file
        .entries
        .iter()
        .filter(|entry| entry.shape.redacted_by_default)
        .count();
    stdout_line(format_args!(
        "{}: {} keys, {} diagnostics, {} redacted, {}",
        file.path,
        file.entries.len(),
        file.diagnostics.len(),
        redacted,
        file.git_status.label()
    ))?;

    for diagnostic in &file.diagnostics {
        stdout_line(format_args!("  warning {diagnostic}"))?;
    }

    Ok(())
}

fn print_comparison(comparison: &EnvComparison) -> io::Result<()> {
    stdout_line(format_args!(
        "contract: {} shared, {} missing, {} extra, {} duplicate",
        comparison.shared_keys.len(),
        comparison.missing_keys.len(),
        comparison.extra_keys.len(),
        comparison.duplicate_keys.len()
    ))?;
    for key in &comparison.missing_keys {
        stdout_line(format_args!("  missing {key}"))?;
    }
    for key in &comparison.extra_keys {
        stdout_line(format_args!("  extra {key}"))?;
    }
    for key in &comparison.duplicate_keys {
        stdout_line(format_args!("  duplicate {key}"))?;
    }

    Ok(())
}

fn has_snapshot_findings(snapshot: &ProjectSnapshot) -> bool {
    snapshot.findings.iter().any(|finding| finding.action_kind != crate::MISSING_REFERENCE_ACTION)
        || snapshot.files.iter().any(has_file_findings)
}

fn has_file_findings(file: &EnvFile) -> bool {
    !file.diagnostics.is_empty()
}

fn has_comparison_findings(comparison: &EnvComparison) -> bool {
    !comparison.missing_keys.is_empty()
        || !comparison.extra_keys.is_empty()
        || !comparison.duplicate_keys.is_empty()
}

fn parse_args(args: impl IntoIterator<Item = String>) -> Result<Command, ParseError> {
    let mut args = args.into_iter();
    let Some(command) = args.next() else {
        return Ok(Command::Help(HelpTopic::General));
    };

    if CLI_HELP_ALIASES.contains(&command.as_str()) {
        return Ok(Command::Help(HelpTopic::General));
    }
    if CLI_VERSION_ALIASES.contains(&command.as_str()) {
        return Ok(Command::Version);
    }

    let Some(entry) = cli_command(&command) else {
        return Err(ParseError::new(
            HelpTopic::General,
            "unknown command",
        ));
    };

    (entry.parse)(args.collect::<Vec<_>>().into_iter())
}

fn parse_check_args(args: ArgIter) -> Result<Command, ParseError> {
    let mut file = None;
    let mut example = None;
    let mut output = OutputFormat::Auto;
    let mut values = ValueOutput::Hidden;
    let mut args = args.peekable();

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--text" => set_output_format(&mut output, OutputFormat::Text, HelpTopic::Check)?,
            "--json" => set_output_format(&mut output, OutputFormat::Json, HelpTopic::Check)?,
            "--pretty" => {
                set_output_format(&mut output, OutputFormat::PrettyJson, HelpTopic::Check)?
            }
            "--values" => values = ValueOutput::Classified,
            "--example" => {
                let Some(path) = args.next() else {
                    return Err(ParseError::new(
                        HelpTopic::Check,
                        "--example requires a path",
                    ));
                };
                example = Some(PathBuf::from(path));
            }
            "-h" | "--help" => return Ok(Command::Help(HelpTopic::Check)),
            _ if arg.starts_with('-') => {
                return Err(ParseError::new(
                    HelpTopic::Check,
                    "unknown check option",
                ));
            }
            _ if file.is_none() => file = Some(PathBuf::from(arg)),
            _ => {
                return Err(ParseError::new(
                    HelpTopic::Check,
                    "unexpected check argument",
                ));
            }
        }
    }

    let file =
        file.ok_or_else(|| ParseError::new(HelpTopic::Check, "check requires an env file path"))?;
    Ok(Command::Check {
        file,
        example,
        output,
        values,
    })
}

fn parse_inspect_args(args: ArgIter) -> Result<Command, ParseError> {
    let mut dir = None;
    let mut output = OutputFormat::Auto;
    let mut values = ValueOutput::Hidden;

    for arg in args {
        match arg.as_str() {
            "--text" => set_output_format(&mut output, OutputFormat::Text, HelpTopic::Inspect)?,
            "--json" => set_output_format(&mut output, OutputFormat::Json, HelpTopic::Inspect)?,
            "--pretty" => {
                set_output_format(&mut output, OutputFormat::PrettyJson, HelpTopic::Inspect)?
            }
            "--values" => values = ValueOutput::Classified,
            "-h" | "--help" => return Ok(Command::Help(HelpTopic::Inspect)),
            _ if arg.starts_with('-') => {
                return Err(ParseError::new(
                    HelpTopic::Inspect,
                    "unknown inspect option",
                ));
            }
            _ if dir.is_none() => dir = Some(PathBuf::from(arg)),
            _ => {
                return Err(ParseError::new(
                    HelpTopic::Inspect,
                    "unexpected inspect argument",
                ));
            }
        }
    }

    let dir = dir.ok_or_else(|| {
        ParseError::new(HelpTopic::Inspect, "inspect requires a project directory")
    })?;
    Ok(Command::Inspect {
        dir,
        output,
        values,
    })
}

fn parse_format_args(args: ArgIter) -> Result<Command, ParseError> {
    let mut file = None;
    let mut dry_run = false;

    for arg in args {
        match arg.as_str() {
            "--dry-run" => dry_run = true,
            "-h" | "--help" => return Ok(Command::Help(HelpTopic::Format)),
            _ if arg.starts_with('-') => {
                return Err(ParseError::new(
                    HelpTopic::Format,
                    "unknown format option",
                ));
            }
            _ if file.is_none() => file = Some(PathBuf::from(arg)),
            _ => {
                return Err(ParseError::new(
                    HelpTopic::Format,
                    "unexpected format argument",
                ));
            }
        }
    }

    let file =
        file.ok_or_else(|| ParseError::new(HelpTopic::Format, "format requires an env file path"))?;
    Ok(Command::Format { file, dry_run })
}

fn parse_add_args(args: ArgIter) -> Result<Command, ParseError> {
    let mut output = OutputFormat::Auto;
    let mut values = ValueOutput::Hidden;
    let mut value = None;
    let mut positionals = Vec::new();
    let mut args = args.peekable();

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--text" => set_output_format(&mut output, OutputFormat::Text, HelpTopic::Add)?,
            "--json" => set_output_format(&mut output, OutputFormat::Json, HelpTopic::Add)?,
            "--pretty" => set_output_format(&mut output, OutputFormat::PrettyJson, HelpTopic::Add)?,
            "--values" => values = ValueOutput::Classified,
            "--prompt" => set_value_source(&mut value, ValueSource::Prompt, HelpTopic::Add)?,
            "--stdin" => set_value_source(&mut value, ValueSource::Stdin, HelpTopic::Add)?,
            "--value" => {
                let Some(inline_value) = args.next() else {
                    return Err(ParseError::new(HelpTopic::Add, "--value requires a value"));
                };
                set_value_source(
                    &mut value,
                    ValueSource::Inline(inline_value),
                    HelpTopic::Add,
                )?;
            }
            "-h" | "--help" => return Ok(Command::Help(HelpTopic::Add)),
            _ => positionals.push(arg),
        }
    }

    let (file, key, value) = match (positionals.as_slice(), value) {
        ([file, key], Some(value)) => (PathBuf::from(file), key.clone(), value),
        ([file, assignment], None) => {
            let Some((key, inline_value)) = assignment.split_once('=') else {
                return Err(add_usage_error());
            };
            (
                PathBuf::from(file),
                key.to_string(),
                ValueSource::Inline(inline_value.to_string()),
            )
        }
        ([file, key, inline_value], None) => (
            PathBuf::from(file),
            key.clone(),
            ValueSource::Inline(inline_value.clone()),
        ),
        ([_, _, _], Some(_)) => {
            return Err(ParseError::new(
                HelpTopic::Add,
                "add accepts either <VALUE> or --prompt/--stdin/--value, not both",
            ));
        }
        _ => {
            return Err(add_usage_error());
        }
    };

    if key.is_empty() {
        return Err(ParseError::new(
            HelpTopic::Add,
            "add requires a non-empty key",
        ));
    }

    Ok(Command::Add {
        file,
        key,
        value,
        output,
        values,
    })
}

fn parse_set_args(args: ArgIter) -> Result<Command, ParseError> {
    let mut output = OutputFormat::Auto;
    let mut value = None;
    let mut allow_empty = false;
    let mut positionals = Vec::new();

    for arg in args {
        match arg.as_str() {
            "--allow-empty" => allow_empty = true,
            "--text" => set_output_format(&mut output, OutputFormat::Text, HelpTopic::Set)?,
            "--json" => set_output_format(&mut output, OutputFormat::Json, HelpTopic::Set)?,
            "--pretty" => set_output_format(&mut output, OutputFormat::PrettyJson, HelpTopic::Set)?,
            "--prompt" => set_value_source(&mut value, ValueSource::Prompt, HelpTopic::Set)?,
            "--stdin" => set_value_source(&mut value, ValueSource::Stdin, HelpTopic::Set)?,
            "-h" | "--help" => return Ok(Command::Help(HelpTopic::Set)),
            _ if arg.starts_with('-') => {
                return Err(ParseError::new(
                    HelpTopic::Set,
                    format!("unknown set option; {SET_VALUE_SOURCE_RULE}"),
                ));
            }
            _ => positionals.push(arg),
        }
    }

    let [file, key] = positionals.as_slice() else {
        return Err(set_usage_error());
    };
    if key.is_empty() {
        return Err(ParseError::new(
            HelpTopic::Set,
            "set requires a non-empty key",
        ));
    }
    if key.contains('=') {
        return Err(set_usage_error());
    }
    let Some(value) = value else {
        return Err(set_usage_error());
    };

    Ok(Command::Set {
        file: PathBuf::from(file),
        key: key.clone(),
        value,
        allow_empty,
        output,
    })
}

fn parse_rm_args(args: ArgIter) -> Result<Command, ParseError> {
    let mut output = OutputFormat::Auto;
    let mut selector = None;
    let mut expectation = None;
    let mut positionals = Vec::new();
    let mut args = args.peekable();

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--text" => set_output_format(&mut output, OutputFormat::Text, HelpTopic::Remove)?,
            "--json" => set_output_format(&mut output, OutputFormat::Json, HelpTopic::Remove)?,
            "--pretty" => {
                set_output_format(&mut output, OutputFormat::PrettyJson, HelpTopic::Remove)?
            }
            "--all" => set_remove_selector(&mut selector, EnvKeyRemoveSelector::All)?,
            "--line" => {
                let Some(value) = args.next() else {
                    return Err(ParseError::new(
                        HelpTopic::Remove,
                        "--line requires a line number",
                    ));
                };
                let Some(line_number) = value.parse::<usize>().ok().filter(|line| *line > 0) else {
                    return Err(ParseError::new(
                        HelpTopic::Remove,
                        "--line requires a positive line number",
                    ));
                };
                set_remove_selector(&mut selector, EnvKeyRemoveSelector::Line(line_number))?;
            }
            "--expect" => {
                let Some(value) = args.next() else {
                    return Err(ParseError::new(
                        HelpTopic::Remove,
                        "--expect requires `present` or `absent`",
                    ));
                };
                let parsed = match value.as_str() {
                    "present" => EnvKeyExpectation::Present,
                    "absent" => EnvKeyExpectation::Absent,
                    _ => {
                        return Err(ParseError::new(
                            HelpTopic::Remove,
                            "--expect accepts `present` or `absent`",
                        ))
                    }
                };
                if expectation.is_some() {
                    return Err(ParseError::new(
                        HelpTopic::Remove,
                        "rm accepts only one --expect",
                    ));
                }
                expectation = Some(parsed);
            }
            "-h" | "--help" => return Ok(Command::Help(HelpTopic::Remove)),
            _ if arg.starts_with('-') => {
                return Err(ParseError::new(
                    HelpTopic::Remove,
                    "unknown rm option",
                ));
            }
            _ => positionals.push(arg),
        }
    }

    let [file, key] = positionals.as_slice() else {
        return Err(ParseError::new(
            HelpTopic::Remove,
            "rm requires <file> <KEY>",
        ));
    };
    if key.is_empty() {
        return Err(ParseError::new(
            HelpTopic::Remove,
            "rm requires a non-empty key",
        ));
    }
    if key.contains('=') {
        return Err(assignment_in_key_slot(HelpTopic::Remove, "rm"));
    }

    Ok(Command::Remove {
        file: PathBuf::from(file),
        key: key.clone(),
        selector: selector.unwrap_or(EnvKeyRemoveSelector::Only),
        expectation,
        output,
    })
}

fn set_remove_selector(
    target: &mut Option<EnvKeyRemoveSelector>,
    selector: EnvKeyRemoveSelector,
) -> Result<(), ParseError> {
    if target.is_some() {
        return Err(ParseError::new(
            HelpTopic::Remove,
            "rm accepts only one of --line <N> or --all",
        ));
    }
    *target = Some(selector);
    Ok(())
}

fn parse_rename_args(args: ArgIter) -> Result<Command, ParseError> {
    let mut output = OutputFormat::Auto;
    let mut positionals = Vec::new();

    for arg in args {
        match arg.as_str() {
            "--text" => set_output_format(&mut output, OutputFormat::Text, HelpTopic::Rename)?,
            "--json" => set_output_format(&mut output, OutputFormat::Json, HelpTopic::Rename)?,
            "--pretty" => {
                set_output_format(&mut output, OutputFormat::PrettyJson, HelpTopic::Rename)?
            }
            "-h" | "--help" => return Ok(Command::Help(HelpTopic::Rename)),
            _ if arg.starts_with('-') => {
                return Err(ParseError::new(
                    HelpTopic::Rename,
                    "unknown rename option",
                ));
            }
            _ => positionals.push(arg),
        }
    }

    let [file, from, to] = positionals.as_slice() else {
        return Err(ParseError::new(
            HelpTopic::Rename,
            "rename requires <file> <OLD_KEY> <NEW_KEY>",
        ));
    };
    if from.is_empty() || to.is_empty() {
        return Err(ParseError::new(
            HelpTopic::Rename,
            "rename requires two non-empty keys",
        ));
    }
    if from.contains('=') || to.contains('=') {
        return Err(assignment_in_key_slot(HelpTopic::Rename, "rename"));
    }

    Ok(Command::Rename {
        file: PathBuf::from(file),
        from: from.clone(),
        to: to.clone(),
        output,
    })
}

fn parse_example_args(args: ArgIter) -> Result<Command, ParseError> {
    let mut file = None;
    let mut example = None;
    let mut output = OutputFormat::Auto;
    let mut args = args.peekable();

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--text" => set_output_format(&mut output, OutputFormat::Text, HelpTopic::Example)?,
            "--json" => set_output_format(&mut output, OutputFormat::Json, HelpTopic::Example)?,
            "--pretty" => {
                set_output_format(&mut output, OutputFormat::PrettyJson, HelpTopic::Example)?
            }
            "--example" => {
                let Some(path) = args.next() else {
                    return Err(ParseError::new(
                        HelpTopic::Example,
                        "--example requires a path",
                    ));
                };
                if example.is_some() {
                    return Err(ParseError::new(
                        HelpTopic::Example,
                        "example accepts only one --example path",
                    ));
                }
                example = Some(PathBuf::from(path));
            }
            "-h" | "--help" => return Ok(Command::Help(HelpTopic::Example)),
            _ if arg.starts_with('-') => {
                return Err(ParseError::new(
                    HelpTopic::Example,
                    "unknown example option",
                ));
            }
            _ if file.is_none() => file = Some(PathBuf::from(arg)),
            _ => {
                return Err(ParseError::new(
                    HelpTopic::Example,
                    "unexpected example argument",
                ));
            }
        }
    }

    let file = file
        .ok_or_else(|| ParseError::new(HelpTopic::Example, "example requires an env file path"))?;
    Ok(Command::Example {
        file,
        example,
        output,
    })
}

fn parse_create_args(args: ArgIter) -> Result<Command, ParseError> {
    let mut file = None;
    let mut output = OutputFormat::Auto;

    for arg in args {
        match arg.as_str() {
            "--text" => set_output_format(&mut output, OutputFormat::Text, HelpTopic::Create)?,
            "--json" => set_output_format(&mut output, OutputFormat::Json, HelpTopic::Create)?,
            "--pretty" => {
                set_output_format(&mut output, OutputFormat::PrettyJson, HelpTopic::Create)?
            }
            "-h" | "--help" => return Ok(Command::Help(HelpTopic::Create)),
            _ if arg.starts_with('-') => {
                return Err(ParseError::new(
                    HelpTopic::Create,
                    "unknown create option",
                ));
            }
            _ if file.is_none() => file = Some(PathBuf::from(arg)),
            _ => {
                return Err(ParseError::new(
                    HelpTopic::Create,
                    "unexpected create argument",
                ));
            }
        }
    }

    let file =
        file.ok_or_else(|| ParseError::new(HelpTopic::Create, "create requires an env file path"))?;
    Ok(Command::Create { file, output })
}

fn parse_run_args(mut args: ArgIter) -> Result<Command, ParseError> {
    let mut files = Vec::new();
    loop {
        let Some(arg) = args.next() else {
            return Err(ParseError::new(
                HelpTopic::Run,
                "run requires `--` between the env files and the command",
            ));
        };
        match arg.as_str() {
            "--" => break,
            "-h" | "--help" => return Ok(Command::Help(HelpTopic::Run)),
            _ if arg.starts_with('-') => {
                return Err(ParseError::new(
                    HelpTopic::Run,
                    "unknown run option",
                ));
            }
            _ => files.push(PathBuf::from(arg)),
        }
    }

    if files.is_empty() {
        return Err(ParseError::new(
            HelpTopic::Run,
            "run requires at least one env file before `--`",
        ));
    }
    let program = args
        .next()
        .ok_or_else(|| ParseError::new(HelpTopic::Run, "run requires a command after `--`"))?;
    Ok(Command::Run {
        files,
        program,
        args: args.collect(),
    })
}

fn parse_where_args(args: ArgIter) -> Result<Command, ParseError> {
    let mut output = OutputFormat::Auto;
    let mut positionals = Vec::new();

    for arg in args {
        match arg.as_str() {
            "--text" => set_output_format(&mut output, OutputFormat::Text, HelpTopic::Where)?,
            "--json" => set_output_format(&mut output, OutputFormat::Json, HelpTopic::Where)?,
            "--pretty" => {
                set_output_format(&mut output, OutputFormat::PrettyJson, HelpTopic::Where)?
            }
            "-h" | "--help" => return Ok(Command::Help(HelpTopic::Where)),
            _ if arg.starts_with('-') => {
                return Err(ParseError::new(
                    HelpTopic::Where,
                    "unknown where option",
                ));
            }
            _ => positionals.push(arg),
        }
    }

    let mut positionals = positionals.into_iter();
    let Some(key) = positionals.next() else {
        return Err(ParseError::new(HelpTopic::Where, "where requires a <KEY>"));
    };
    if key.is_empty() {
        return Err(ParseError::new(
            HelpTopic::Where,
            "where requires a non-empty key",
        ));
    }
    if key.contains('=') {
        return Err(assignment_in_key_slot(HelpTopic::Where, "where"));
    }
    let mut paths = positionals.map(PathBuf::from).collect::<Vec<_>>();
    if paths.is_empty() {
        paths.push(PathBuf::from("."));
    }

    Ok(Command::Where { key, paths, output })
}

fn parse_copy_args(args: ArgIter) -> Result<Command, ParseError> {
    let mut output = OutputFormat::Auto;
    let mut overwrite = false;
    let mut positionals = Vec::new();

    for arg in args {
        match arg.as_str() {
            "--overwrite" => overwrite = true,
            "--text" => set_output_format(&mut output, OutputFormat::Text, HelpTopic::Copy)?,
            "--json" => set_output_format(&mut output, OutputFormat::Json, HelpTopic::Copy)?,
            "--pretty" => {
                set_output_format(&mut output, OutputFormat::PrettyJson, HelpTopic::Copy)?
            }
            "-h" | "--help" => return Ok(Command::Help(HelpTopic::Copy)),
            _ if arg.starts_with('-') => {
                return Err(ParseError::new(
                    HelpTopic::Copy,
                    "unknown copy option",
                ));
            }
            _ => positionals.push(arg),
        }
    }

    let [source_file, key, destination_file] = positionals.as_slice() else {
        return Err(ParseError::new(
            HelpTopic::Copy,
            "copy requires <source-file> <KEY> <destination-file>",
        ));
    };
    if key.is_empty() {
        return Err(ParseError::new(
            HelpTopic::Copy,
            "copy requires a non-empty key",
        ));
    }

    Ok(Command::Copy {
        source_file: PathBuf::from(source_file),
        key: key.clone(),
        destination_file: PathBuf::from(destination_file),
        overwrite,
        output,
    })
}

/// Renders an argument for an error message without echoing an `=` value.
/// `--value=sk-live-…` becomes `--value=…`, so a mistyped flag cannot copy a
/// secret into stderr.
fn argument_label(argument: &str) -> String {
    match argument.split_once('=') {
        Some((name, _)) => format!("{name}=…"),
        None => argument.to_string(),
    }
}

fn set_output_format(
    target: &mut OutputFormat,
    value: OutputFormat,
    topic: HelpTopic,
) -> Result<(), ParseError> {
    if *target != OutputFormat::Auto && *target != value {
        return Err(ParseError::new(
            topic,
            "choose only one output format: --text, --json, or --pretty",
        ));
    }
    *target = value;
    Ok(())
}

const SET_VALUE_SOURCE_RULE: &str =
    "set reads the new value from --stdin or --prompt only, never from an argument";

fn set_value_source(
    target: &mut Option<ValueSource>,
    value: ValueSource,
    topic: HelpTopic,
) -> Result<(), ParseError> {
    if target.is_some() {
        let message = match topic {
            HelpTopic::Set => "set accepts only one of --stdin or --prompt",
            _ => "add accepts only one of --prompt, --stdin, or --value",
        };
        return Err(ParseError::new(topic, message));
    }
    *target = Some(value);
    Ok(())
}

/// Refuses a `KEY=VALUE` argument where a key name belongs, without repeating
/// the argument back — the value half is exactly what must not reach stderr.
fn assignment_in_key_slot(topic: HelpTopic, verb: &'static str) -> ParseError {
    ParseError::new(
        topic,
        format!(
            "{verb} takes a key name, not a KEY=VALUE assignment; pass the key alone (the value is not echoed)"
        ),
    )
}

fn set_usage_error() -> ParseError {
    ParseError::new(
        HelpTopic::Set,
        format!(
            "set requires <file> <KEY> --stdin or <file> <KEY> --prompt; {SET_VALUE_SOURCE_RULE}"
        ),
    )
}

fn add_usage_error() -> ParseError {
    ParseError::new(
        HelpTopic::Add,
        "add requires <file> <KEY> <VALUE>, <file> <KEY=VALUE>, <file> <KEY> --value <VALUE>, <file> <KEY> --stdin, or <file> <KEY> --prompt",
    )
}

fn print_usage(topic: HelpTopic) -> io::Result<()> {
    stdout(format_args!("{}", usage_text(topic)))
}

fn print_usage_to_stderr(topic: HelpTopic) -> io::Result<()> {
    stderr(format_args!("{}", usage_text(topic)))
}

fn usage_text(topic: HelpTopic) -> &'static str {
    match topic {
        HelpTopic::General => {
            "vne - local private .env viewer and editor\n\nAGENTS:\n  Start with where <KEY> [path...] before saying a key is unavailable.\n  Use inspect, check and where without --values for metadata without stored values.\n  Read valueState for set/empty/placeholder; value, displayValue and content\n  may contain withheld markers instead of data.\n  Hand secret entry to the user with add --prompt or set --prompt.\n  Only pass known non-secret values as arguments or agent-provided stdin.\n  Run create, add, set, rm, rename, example and copy when the user requests\n  the mutation; copy transfers values internally without returning them.\n  run gives values to the chosen command, whose output can expose them;\n  use it only for authorized commands that do not print their environment.\n  The desktop, --values and format --dry-run are local human surfaces.\n\nUSAGE:\n  vne [project-dir]\n  vne create <file> [--text|--json|--pretty]\n  vne check <file> [--example <file>] [--text|--json|--pretty] [--values]\n  vne inspect <dir> [--text|--json|--pretty] [--values]\n  vne add <file> <KEY> <VALUE> [--text|--json|--pretty] [--values]\n  vne add <file> <KEY=VALUE> [--text|--json|--pretty] [--values]\n  vne add <file> <KEY> --value <VALUE> [--text|--json|--pretty] [--values]\n  vne add <file> <KEY> --stdin [--text|--json|--pretty] [--values]\n  vne add <file> <KEY> --prompt [--text|--json|--pretty] [--values]\n  vne set <file> <KEY> --stdin [--allow-empty] [--text|--json|--pretty]\n  vne set <file> <KEY> --prompt [--allow-empty] [--text|--json|--pretty]\n  vne rm <file> <KEY> [--all|--line <N>] [--expect present|absent] [--text|--json|--pretty]\n  vne rename <file> <OLD_KEY> <NEW_KEY> [--text|--json|--pretty]\n  vne example <file> [--example <file>] [--text|--json|--pretty]\n  vne copy <source-file> <KEY> <destination-file> [--overwrite] [--text|--json|--pretty]\n  vne format <file> --dry-run\n  vne run <file>... -- <command> [args...]\n  vne where <KEY> [path...] [--text|--json|--pretty]\n  vne --version\n\nOUTPUT:\n  Human text is used on a terminal. Piped output is JSON unless --text is set.\n  JSON withholds all env values, comments, and raw content by default.\n  Each entry's valueState says whether the stored value is set, empty, or a\n  placeholder such as changeme, with or without --values.\n  copy, set, rm, rename, and example output contains only state, paths, keys, and lines.
  example additions are key-only placeholders; comments never travel from real files.\n  where output contains only the key, paths, lines, value states, git status,\n  and the name and reason of each discovered file it did not read.\n  --values includes classifier-approved values; comments and raw content stay withheld.\n  --json emits compact JSON; --pretty emits formatted JSON.\n  format --dry-run is a human-terminal command: it refuses piped stdout and
  asks for confirmation before printing raw values.\n  A disposition can reveal whether a value you supplied equals the stored one,\n  and valueState whether a value is empty or a template. Default CLI inspection\n  withholds stored values, not every fact derived from them.\n  run does not print loaded env values; the command it starts receives them.\n\nEXAMPLES:\n  vne .\n  vne create .env --json\n  vne inspect fixtures/demo --json\n  vne inspect fixtures/demo --json --values\n  vne where OPENAI_API_KEY . ~/.config/keys\n  vne check fixtures/demo/.env --example fixtures/demo/.env.example --json\n  vne add .env FEATURE_FLAG=true --json\n  vne add .env OPENAI_API_KEY --prompt\n  vne set .env OPENAI_API_KEY --prompt\n  vne rm .env STALE_FLAG\n  vne rename .env OLD_NAME NEW_NAME\n  vne example .env\n  vne copy ../other/.env DATABASE_URL .env\n  vne run ~/.config/keys/fal.env -- falgen image \"a lighthouse\"\n"
        }
        HelpTopic::Check => {
            "vne check - inspect one env file and optionally compare it with an example contract\n\nUSAGE:\n  vne check <file> [--example <file>] [--text|--json|--pretty] [--values]\n\nBEHAVIOR:\n  Named files must be regular UTF-8 files within the 10 MiB read bound.\n  An explicitly named symlink may point to a regular file.\n\nOPTIONS:\n  --example <file>  Compare actual keys against an example env file\n  --text            Force human text output\n  --json            Emit compact one-line JSON; payloads are withheld by default\n  --pretty          Emit formatted JSON; payloads are withheld by default\n  --values          Include classifier-approved values; comments and raw content stay withheld\n\nEXAMPLES:\n  vne check .env\n  vne check .env --example .env.example --json\n  vne check .env --json --values\n"
        }
        HelpTopic::Inspect => {
            "vne inspect - scan a project directory for env files and findings\n\nBEHAVIOR:\n  Missing package-script and Compose env references are informational and do\n  not change exit codes. Unreadable project references are incomplete;\n  references resolved outside the project are excluded.\n  Missing-reference notices retain at most 10,000 paths; omissions are reported.\n\nUSAGE:\n  vne inspect <dir> [--text|--json|--pretty] [--values]\n\nOPTIONS:\n  --text    Force human text output\n  --json    Emit compact one-line JSON; payloads are withheld by default\n  --pretty  Emit formatted JSON; payloads are withheld by default\n  --values  Include classifier-approved values; comments and raw content stay withheld\n\nEXAMPLES:\n  vne inspect .\n  vne inspect fixtures/demo --json\n  vne inspect fixtures/demo --json --values\n"
        }
        HelpTopic::Add => {
            "vne add - append one non-duplicate env key while preserving file formatting\n\nUSAGE:\n  vne add <file> <KEY> <VALUE> [--text|--json|--pretty] [--values]\n  vne add <file> <KEY=VALUE> [--text|--json|--pretty] [--values]\n  vne add <file> <KEY> --value <VALUE> [--text|--json|--pretty] [--values]\n  vne add <file> <KEY> --stdin [--text|--json|--pretty] [--values]\n  vne add <file> <KEY> --prompt [--text|--json|--pretty] [--values]\n\nOPTIONS:\n  --value <VALUE>  Read the new value from a flag\n  --stdin          Read the new value from stdin\n  --prompt         Offer to create a missing env file, then read a hidden value\n  --text           Force human text output\n  --json           Emit compact one-line JSON; payloads are withheld by default\n  --pretty         Emit formatted JSON; payloads are withheld by default\n  --values         Include classifier-approved values; comments and raw content stay withheld\n\nEXAMPLES:\n  vne add .env FEATURE_FLAG=true --json\n  printf '%s' 'sk-local' | vne add .env OPENAI_API_KEY --stdin --json\n  vne add .env OPENAI_API_KEY --prompt\n"
        }
        HelpTopic::Create => {
            "vne create - ensure one empty env file exists without overwriting it\n\nUSAGE:\n  vne create <file> [--text|--json|--pretty]\n\nBEHAVIOR:\n  The parent directory must already exist.\n  The filename must look like .env, .env.local, .envrc, .flaskenv, or worker.env.\n  A new file is empty and private to its owner on Unix.\n  An existing regular file is left unchanged and reported as alreadyExists.\n  Symlinks, directories, missing parent directories, and overwrite flags are rejected.\n\nOPTIONS:\n  --text    Force human text output\n  --json    Emit a compact disposition and absolute path\n  --pretty  Emit a formatted disposition and absolute path\n\nEXAMPLES:\n  vne create .env\n  vne create config.env --json\n  vne create /path/to/project/.env.local --pretty\n"
        }
        HelpTopic::Set => {
            "vne set - replace one existing env key value without exposing it\n\nUSAGE:\n  vne set <file> <KEY> --stdin [--allow-empty] [--text|--json|--pretty]\n  vne set <file> <KEY> --prompt [--allow-empty] [--text|--json|--pretty]\n\nBEHAVIOR:\n  The target must be an existing regular file holding exactly one occurrence of KEY.\n  The new value is read from stdin or a hidden prompt; argument value forms are refused.\n  An identical stored value reports alreadyPresent without writing.\n  That disposition tells the caller the value it supplied is already stored.\n  Missing, duplicate, and malformed keys are refused with line numbers only.\n  An empty value is refused unless --allow-empty; use `vne rm` to remove a key.\n\nOPTIONS:\n  --stdin        Read the new value from stdin\n  --prompt       Read the new value from a hidden terminal prompt\n  --allow-empty  Store an empty value instead of refusing one\n  --text    Force human text output\n  --json    Emit a compact payload-free set receipt\n  --pretty  Emit a formatted payload-free set receipt\n\nEXAMPLES:\n  vne set .env OPENAI_API_KEY --prompt\n  printf '%s' '1421' | vne set .env PORT --stdin --json\n"
        }
        HelpTopic::Remove => {
            "vne rm - delete one env key without reading its value\n\nUSAGE:\n  vne rm <file> <KEY> [--all|--line <N>] [--expect present|absent] [--text|--json|--pretty]\n\nBEHAVIOR:\n  A unique occurrence is removed and its whole line spliced out.\n  A duplicated key is refused with its line numbers unless --line or --all selects one.\n  --line <N> is a one-shot selector: if line N does not hold KEY it fails and never\n  hunts for another occurrence, so a stale line number cannot delete the wrong entry.\n  An absent key reports alreadyAbsent without writing, so a retry converges.\n  --expect states the key state you believe in and turns a wrong belief into exit 2.\n\nOPTIONS:\n  --all             Remove every occurrence in one write\n  --line <N>        Remove exactly the occurrence at line N\n  --expect present  Fail instead of reporting alreadyAbsent\n  --expect absent   Fail instead of removing anything\n  --text            Force human text output\n  --json            Emit a compact payload-free removal receipt\n  --pretty          Emit a formatted payload-free removal receipt\n\nEXAMPLES:\n  vne rm .env STALE_FLAG\n  vne rm .env DUPLICATED --line 12 --json\n  vne rm .env OPENAI_API_KEY --expect present\n"
        }
        HelpTopic::Rename => {
            "vne rename - rename one env key while leaving its value untouched\n\nUSAGE:\n  vne rename <file> <OLD_KEY> <NEW_KEY> [--text|--json|--pretty]\n\nBEHAVIOR:\n  Only the key token changes. The value bytes, quoting, inline comment, export\n  prefix, spacing, and line position stay byte-identical.\n  The old key must occur exactly once and be well formed.\n  An existing new key is refused; vne never merges two keys.\n  When the old key is absent and the new key is present, vne refuses instead of\n  reporting success: file state cannot prove that this rename is what happened.\n\nOPTIONS:\n  --text    Force human text output\n  --json    Emit a compact payload-free rename receipt\n  --pretty  Emit a formatted payload-free rename receipt\n\nEXAMPLES:\n  vne rename .env OLD_NAME NEW_NAME\n  vne rename .env OPENAI_KEY OPENAI_API_KEY --json\n"
        }
        HelpTopic::Example => {
            "vne example - add missing keys to an example env file, never their values\n\nUSAGE:\n  vne example <file> [--example <file>] [--text|--json|--pretty]\n\nBEHAVIOR:\n  Keys present in the real file and missing from the example are appended as\n  valueless placeholders, in the order the real file lists them.\n  Existing example lines are never rewritten and never deleted.\n  An inline comment travels only when it does not itself look secret-like.\n  A missing example file is created; the default is a sibling .env.example.\n  Nothing to add reports alreadyCurrent without writing.\n\nOPTIONS:\n  --example <file>  Sync into this example file instead of the sibling default\n  --text            Force human text output\n  --json            Emit a compact payload-free sync receipt\n  --pretty          Emit a formatted payload-free sync receipt\n\nEXAMPLES:\n  vne example .env\n  vne example .env --example .env.sample --json\n"
        }
        HelpTopic::Copy => {
            "vne copy - copy one env key between existing files without exposing its value\n\nUSAGE:\n  vne copy <source-file> <KEY> <destination-file> [--overwrite] [--text|--json|--pretty]\n\nBEHAVIOR:\n  Source and destination must be distinct existing regular files.\n  The source key must occur exactly once and be well formed.\n  An absent destination key is added. An identical value reports alreadyPresent without writing.\n  That disposition tells the caller the two values are equal.\n  A different destination value requires --overwrite. Duplicate destination keys are refused.\n  The value stays inside vne; output contains only state, normalized paths, and key.\n\nOPTIONS:\n  --overwrite  Replace one different existing destination value\n  --text       Force human text output\n  --json       Emit a compact payload-free copy receipt\n  --pretty     Emit a formatted payload-free copy receipt\n\nEXAMPLE:\n  vne copy ../other/.env DATABASE_URL .env\n  vne copy ../other/.env DATABASE_URL .env --overwrite --json\n"
        }
        HelpTopic::Run => {
            "vne run - start one command with env files loaded into its environment only\n\nUSAGE:\n  vne run <file>... -- <command> [args...]\n\nBEHAVIOR:\n  Keys reach the command, which can pass them to its child processes.\n  The calling shell does not receive them. A key from a file replaces an inherited variable of the same name.\n  Files are read as data. Values are literal: nothing is expanded or executed.\n  Inside quotes, only \\\\ and an escaped quote are read; any other escape refuses.\n  A line that is not an entry, a comment, or blank refuses the run, as do a\n  malformed entry, a key repeated in one file, and a key defined in two files.\n  `env`, `printenv`, and shell -c scripts that list the environment are refused.\n  That check catches a slip; any command you choose can still print what it gets.\n  vne prints nothing when the command starts. Refusals exit 2; a command that\n  cannot start exits 127 (not found) or 126, as a shell would; otherwise the\n  exit status is the command's own.\n\nEXAMPLES:\n  vne run ~/.config/keys/fal.env -- falgen image \"a lighthouse\"\n  vne run .env .env.local -- npm run dev\n  vne run keys.env -- sh -c 'printf \"Authorization: Bearer %s\\n\" \"$API_KEY\" | curl -H @- https://api.example.com'\n"
        }
        HelpTopic::Where => {
            "vne where - find which env files hold a key, without printing its value\n\nUSAGE:\n  vne where <KEY> [path...] [--text|--json|--pretty]\n\nBEHAVIOR:\n  Each path is a project directory, scanned the way `vne inspect` scans one, or a\n  single env file. Named files must be regular UTF-8 files within the\n  10 MiB read bound. With no path, the current directory is scanned. Directories\n  are not searched recursively, and no other location is searched unless named.\n  A symlink with an env filename is not followed and is reported as not read;\n  name the symlink as a path to read the file it points to.\n  Missing package-script and Compose references appear in missingReferences\n  as nonfatal notices. Unreadable project references remain incomplete;\n  references resolved outside the project are excluded.\n  Each match reports its file, line, value state, and git status. Values never\n  appear. A value of only whitespace or quote characters counts as empty; a\n  template such as changeme, todo, tbd, your_..., or <...> (any case) counts as\n  placeholder.\n  Exit 2 when a named path cannot be read, even if another path holds the key,\n  or when the key is not found and a discovered file was not read. Otherwise\n  exit 0 when the key is found and 1 when it is not.\n\nOPTIONS:\n  --text    Force human text output\n  --json    Emit compact one-line JSON\n  --pretty  Emit formatted JSON\n\nEXAMPLES:\n  vne where OPENAI_API_KEY\n  vne where FAL_KEY . ~/.config/keys\n  vne where DATABASE_URL .env.local --json\n"
        }
        HelpTopic::Format => {
            "vne format - print the parsed env file without rewriting it\n\nWARNING:\n  This command emits raw env content. It does not apply JSON payload withholding or classified redaction.\n\nUSAGE:\n  vne format <file> --dry-run\n\nOPTIONS:\n  --dry-run  Required; print the current raw file content instead of writing\n\nEXAMPLE:\n  vne format .env --dry-run\n"
        }
    }
}

fn stdout(args: fmt::Arguments<'_>) -> io::Result<()> {
    io::stdout().lock().write_fmt(args)
}

fn stderr(args: fmt::Arguments<'_>) -> io::Result<()> {
    io::stderr().lock().write_fmt(args)
}

fn stdout_line(args: fmt::Arguments<'_>) -> io::Result<()> {
    let mut stdout = io::stdout().lock();
    stdout.write_fmt(args)?;
    stdout.write_all(b"\n")
}

fn stderr_line(args: fmt::Arguments<'_>) -> io::Result<()> {
    let mut stderr = io::stderr().lock();
    stderr.write_fmt(args)?;
    stderr.write_all(b"\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(input: &[&str]) -> Result<Command, String> {
        parse_args(input.iter().map(|arg| arg.to_string())).map_err(|error| error.message)
    }

    #[test]
    fn identifies_cli_invocations_by_explicit_command() {
        assert!(is_cli_invocation(&["add".to_string(), ".env".to_string()]));
        assert!(is_cli_invocation(&[
            "copy".to_string(),
            "source.env".to_string()
        ]));
        assert!(is_cli_invocation(&[
            "create".to_string(),
            ".env".to_string()
        ]));
        assert!(is_cli_invocation(&["--help".to_string()]));
        assert!(!is_cli_invocation(&[".".to_string()]));
        assert!(!is_cli_invocation(&["fixtures/demo".to_string()]));
    }

    #[test]
    fn parses_check_with_example_and_json() {
        assert_eq!(
            parse(&["check", ".env", "--example", ".env.example", "--json"]),
            Ok(Command::Check {
                file: PathBuf::from(".env"),
                example: Some(PathBuf::from(".env.example")),
                output: OutputFormat::Json,
                values: ValueOutput::Hidden,
            })
        );
    }

    #[test]
    fn parses_inspect_with_default_auto_output() {
        assert_eq!(
            parse(&["inspect", "fixtures/demo"]),
            Ok(Command::Inspect {
                dir: PathBuf::from("fixtures/demo"),
                output: OutputFormat::Auto,
                values: ValueOutput::Hidden,
            })
        );
    }

    #[test]
    fn parses_inspect_with_pretty_json() {
        assert_eq!(
            parse(&["inspect", "fixtures/demo", "--pretty"]),
            Ok(Command::Inspect {
                dir: PathBuf::from("fixtures/demo"),
                output: OutputFormat::PrettyJson,
                values: ValueOutput::Hidden,
            })
        );
    }

    #[test]
    fn parses_create_with_stable_output_modes() {
        assert_eq!(
            parse(&["create", ".env"]),
            Ok(Command::Create {
                file: PathBuf::from(".env"),
                output: OutputFormat::Auto,
            })
        );
        assert_eq!(
            parse(&["create", "project/.env.local", "--json"]),
            Ok(Command::Create {
                file: PathBuf::from("project/.env.local"),
                output: OutputFormat::Json,
            })
        );
        assert_eq!(
            parse(&["create", "worker.env", "--pretty"]),
            Ok(Command::Create {
                file: PathBuf::from("worker.env"),
                output: OutputFormat::PrettyJson,
            })
        );
    }

    #[test]
    fn parses_copy_with_explicit_overwrite_and_output() {
        assert_eq!(
            parse(&[
                "copy",
                "../source/.env",
                "DATABASE_URL",
                ".env",
                "--overwrite",
                "--json"
            ]),
            Ok(Command::Copy {
                source_file: PathBuf::from("../source/.env"),
                key: "DATABASE_URL".to_string(),
                destination_file: PathBuf::from(".env"),
                overwrite: true,
                output: OutputFormat::Json,
            })
        );
    }

    #[test]
    fn copy_parser_rejects_missing_extra_and_value_output_arguments() {
        assert!(parse(&["copy"]).is_err());
        assert!(parse(&["copy", "source.env", "TOKEN"]).is_err());
        assert!(parse(&["copy", "source.env", "TOKEN", "dest.env", "extra"]).is_err());
        assert!(parse(&["copy", "source.env", "TOKEN", "dest.env", "--values"]).is_err());
        assert!(parse(&[
            "copy",
            "source.env",
            "TOKEN",
            "dest.env",
            "--json",
            "--text"
        ])
        .is_err());
    }

    #[test]
    fn create_parser_rejects_missing_extra_and_conflicting_arguments() {
        assert!(parse(&["create"]).is_err());
        assert!(parse(&["create", ".env", ".env.local"]).is_err());
        assert!(parse(&["create", ".env", "--force"]).is_err());
        assert!(parse(&["create", ".env", "--json", "--text"]).is_err());
    }

    #[test]
    fn rejects_unknown_options() {
        assert!(parse(&["check", ".env", "--reveal"]).is_err());
    }

    #[test]
    fn parses_add_with_json() {
        assert_eq!(
            parse(&[
                "add",
                ".env",
                "REDIS_URL",
                "redis://localhost:6379",
                "--json"
            ]),
            Ok(Command::Add {
                file: PathBuf::from(".env"),
                key: "REDIS_URL".to_string(),
                value: ValueSource::Inline("redis://localhost:6379".to_string()),
                output: OutputFormat::Json,
                values: ValueOutput::Hidden,
            })
        );
    }

    #[test]
    fn parses_add_assignment_for_agent_copyable_commands() {
        assert_eq!(
            parse(&["add", ".env", "FEATURE_FLAG=true"]),
            Ok(Command::Add {
                file: PathBuf::from(".env"),
                key: "FEATURE_FLAG".to_string(),
                value: ValueSource::Inline("true".to_string()),
                output: OutputFormat::Auto,
                values: ValueOutput::Hidden,
            })
        );
    }

    #[test]
    fn parses_add_prompt_for_local_secret_entry() {
        assert_eq!(
            parse(&["add", ".env", "OPENAI_API_KEY", "--prompt"]),
            Ok(Command::Add {
                file: PathBuf::from(".env"),
                key: "OPENAI_API_KEY".to_string(),
                value: ValueSource::Prompt,
                output: OutputFormat::Auto,
                values: ValueOutput::Hidden,
            })
        );
    }

    #[test]
    fn parses_add_stdin_for_piped_secret_entry() {
        assert_eq!(
            parse(&["add", ".env", "OPENAI_API_KEY", "--stdin", "--json"]),
            Ok(Command::Add {
                file: PathBuf::from(".env"),
                key: "OPENAI_API_KEY".to_string(),
                value: ValueSource::Stdin,
                output: OutputFormat::Json,
                values: ValueOutput::Hidden,
            })
        );
    }

    #[test]
    fn parses_values_as_explicit_classified_output() {
        assert_eq!(
            parse(&["inspect", "fixtures/demo", "--values", "--json"]),
            Ok(Command::Inspect {
                dir: PathBuf::from("fixtures/demo"),
                output: OutputFormat::Json,
                values: ValueOutput::Classified,
            })
        );
        assert_eq!(
            parse(&["check", ".env", "--values"]),
            Ok(Command::Check {
                file: PathBuf::from(".env"),
                example: None,
                output: OutputFormat::Auto,
                values: ValueOutput::Classified,
            })
        );
    }

    #[test]
    fn parses_command_specific_help() {
        assert_eq!(parse(&["add", "--help"]), Ok(Command::Help(HelpTopic::Add)));
        assert_eq!(
            parse(&["copy", "--help"]),
            Ok(Command::Help(HelpTopic::Copy))
        );
        assert_eq!(
            parse(&["create", "--help"]),
            Ok(Command::Help(HelpTopic::Create))
        );
        assert_eq!(
            parse(&["check", "--help"]),
            Ok(Command::Help(HelpTopic::Check))
        );
    }

    #[test]
    fn help_documents_safe_json_defaults_and_raw_format() {
        for topic in [HelpTopic::Check, HelpTopic::Inspect, HelpTopic::Add] {
            let help = usage_text(topic);
            assert!(help.contains("--values"));
            assert!(help.contains("payloads are withheld by default"));
            assert!(help.contains("comments and raw content stay withheld"));
        }
        let general = usage_text(HelpTopic::General);
        assert!(general.contains("JSON withholds all env values"));
        assert!(general.contains("comments and raw content stay withheld"));
        assert!(usage_text(HelpTopic::Format).contains("emits raw env content"));
        let create = usage_text(HelpTopic::Create);
        assert!(create.contains("without overwriting"));
        assert!(create.contains("existing regular file is left unchanged"));
        assert!(create.contains("Symlinks"));
        assert!(usage_text(HelpTopic::Add).contains("Offer to create a missing env file"));
        let copy = usage_text(HelpTopic::Copy);
        assert!(copy.contains("without exposing its value"));
        assert!(copy.contains("alreadyPresent"));
        assert!(copy.contains("--overwrite"));
        assert!(!copy.contains("--values"));
    }

    #[test]
    fn strips_one_trailing_line_ending_from_prompted_values() {
        assert_eq!(
            strip_one_trailing_line_ending("sk-test\n".to_string()),
            "sk-test"
        );
        assert_eq!(
            strip_one_trailing_line_ending("sk-test\r\n".to_string()),
            "sk-test"
        );
        assert_eq!(
            strip_one_trailing_line_ending("line1\nline2\n".to_string()),
            "line1\nline2"
        );
    }

    #[test]
    fn missing_prompt_target_is_created_after_affirmative_confirmation() {
        let dir = tempfile::tempdir().unwrap();
        let env_path = dir.path().join(".env");
        let mut input = io::Cursor::new(b"yes\n");
        let mut diagnostics = Vec::new();

        confirm_missing_env_file_creation(&env_path, &mut input, &mut diagnostics).unwrap();

        assert_eq!(fs::read(&env_path).unwrap(), b"");
        let diagnostics = String::from_utf8(diagnostics).unwrap();
        assert!(diagnostics.contains("does not exist. Create it? [y/N]"));
        assert!(diagnostics.contains("created"));
    }

    #[test]
    fn missing_prompt_target_decline_is_side_effect_free() {
        let dir = tempfile::tempdir().unwrap();
        let env_path = dir.path().join(".env");
        let mut input = io::Cursor::new(b"no\n");
        let mut diagnostics = Vec::new();

        let error = confirm_missing_env_file_creation(&env_path, &mut input, &mut diagnostics)
            .expect_err("declining creation should cancel add");

        assert!(error.to_string().contains("add cancelled"));
        assert!(!env_path.exists());
    }

    #[test]
    fn prompt_creation_race_uses_existing_file_without_overwriting() {
        let dir = tempfile::tempdir().unwrap();
        let env_path = dir.path().join(".env");
        fs::write(&env_path, "EXISTING=keep\n").unwrap();
        let mut input = io::Cursor::new(b"y\n");
        let mut diagnostics = Vec::new();

        confirm_missing_env_file_creation(&env_path, &mut input, &mut diagnostics).unwrap();

        assert_eq!(fs::read_to_string(env_path).unwrap(), "EXISTING=keep\n");
        assert!(String::from_utf8(diagnostics)
            .unwrap()
            .contains("using existing"));
    }

    #[test]
    fn confirmation_accepts_only_explicit_yes_answers() {
        for response in ["y", "Y\n", "yes", "YES\r\n", " yes "] {
            assert!(is_affirmative_confirmation(response), "{response:?}");
        }
        for response in ["", "n", "no", "true", "1", "yep"] {
            assert!(!is_affirmative_confirmation(response), "{response:?}");
        }
    }

    #[test]
    fn rejects_add_with_inline_and_prompt_value_sources() {
        assert!(parse(&["add", ".env", "OPENAI_API_KEY", "sk-test", "--prompt"]).is_err());
        assert!(parse(&["add", ".env", "OPENAI_API_KEY", "--prompt", "--stdin"]).is_err());
    }

    #[test]
    fn every_registered_command_dispatches_and_documents_itself() {
        let general = usage_text(HelpTopic::General);
        for command in CLI_COMMANDS {
            let name = command.name;
            assert!(
                is_cli_invocation(&[name.to_string()]),
                "`{name}` is not recognised as a CLI invocation"
            );

            let parsed = parse(&[name]);
            assert_ne!(
                parsed,
                Err("unknown command".to_string()),
                "`{name}` is registered but not dispatched"
            );

            let Ok(Command::Help(topic)) = parse(&[name, "--help"]) else {
                panic!("`{name} --help` should resolve to its own help topic");
            };
            assert_ne!(topic, HelpTopic::General, "`{name}` reuses general help");
            assert!(
                usage_text(topic).starts_with(&format!("vne {name} ")),
                "`{name}` help topic documents a different command"
            );
            assert!(
                general.contains(&format!("vne {name} ")),
                "`{name}` is missing from the general usage text"
            );
        }
    }

    #[test]
    fn help_aliases_resolve_to_general_usage() {
        for alias in CLI_HELP_ALIASES {
            assert!(is_cli_invocation(&[alias.to_string()]));
            assert_eq!(parse(&[alias]), Ok(Command::Help(HelpTopic::General)));
        }
    }

    #[test]
    fn version_aliases_resolve_to_version() {
        for alias in CLI_VERSION_ALIASES {
            assert!(is_cli_invocation(&[alias.to_string()]));
            assert_eq!(parse(&[alias]), Ok(Command::Version));
        }
    }

    #[test]
    fn parses_where_with_current_directory_default_and_named_paths() {
        assert_eq!(
            parse(&["where", "OPENAI_API_KEY"]),
            Ok(Command::Where {
                key: "OPENAI_API_KEY".to_string(),
                paths: vec![PathBuf::from(".")],
                output: OutputFormat::Auto,
            })
        );
        assert_eq!(
            parse(&["where", "FAL_KEY", ".", "/keys", "--json"]),
            Ok(Command::Where {
                key: "FAL_KEY".to_string(),
                paths: vec![PathBuf::from("."), PathBuf::from("/keys")],
                output: OutputFormat::Json,
            })
        );
    }

    #[test]
    fn where_parser_rejects_missing_and_empty_keys() {
        assert!(parse(&["where"]).is_err());
        assert!(parse(&["where", ""]).is_err());
    }

    #[test]
    fn parses_set_with_hidden_value_sources_only() {
        assert_eq!(
            parse(&["set", ".env", "OPENAI_API_KEY", "--stdin", "--json"]),
            Ok(Command::Set {
                file: PathBuf::from(".env"),
                key: "OPENAI_API_KEY".to_string(),
                value: ValueSource::Stdin,
                allow_empty: false,
                output: OutputFormat::Json,
            })
        );
        assert_eq!(
            parse(&["set", ".env", "OPENAI_API_KEY", "--prompt"]),
            Ok(Command::Set {
                file: PathBuf::from(".env"),
                key: "OPENAI_API_KEY".to_string(),
                value: ValueSource::Prompt,
                allow_empty: false,
                output: OutputFormat::Auto,
            })
        );
    }

    #[test]
    fn set_parser_refuses_every_argument_value_form() {
        for arguments in [
            vec!["set", ".env", "OPENAI_API_KEY", "sk-inline"],
            vec!["set", ".env", "OPENAI_API_KEY=sk-inline", "--stdin"],
            vec!["set", ".env", "OPENAI_API_KEY", "--value", "sk-inline"],
            vec!["set", ".env", "OPENAI_API_KEY"],
            vec!["set", ".env"],
        ] {
            let error = parse(&arguments).expect_err(&format!("{arguments:?} should be refused"));
            assert!(
                error.contains("--stdin") && error.contains("--prompt"),
                "{arguments:?} error omits the allowed value sources: {error}"
            );
            assert!(
                !error.contains("sk-inline"),
                "{arguments:?} error echoed the value: {error}"
            );
        }
        assert!(parse(&["set", ".env", "KEY", "--stdin", "--prompt"]).is_err());
        assert!(parse(&["set", ".env", "KEY", "--stdin", "--values"]).is_err());
        assert!(parse(&["set", ".env", "KEY", "--stdin", "--json", "--text"]).is_err());
    }

    #[test]
    fn set_help_documents_hidden_value_sources_and_convergence() {
        let help = usage_text(HelpTopic::Set);
        assert!(help.contains("--stdin"));
        assert!(help.contains("--prompt"));
        assert!(help.contains("alreadyPresent"));
        assert!(help.contains("argument value forms are refused"));
        assert!(!help.contains("--values"));
    }

    #[test]
    fn parses_rm_with_selectors_and_expectations() {
        assert_eq!(
            parse(&["rm", ".env", "STALE_FLAG"]),
            Ok(Command::Remove {
                file: PathBuf::from(".env"),
                key: "STALE_FLAG".to_string(),
                selector: EnvKeyRemoveSelector::Only,
                expectation: None,
                output: OutputFormat::Auto,
            })
        );
        assert_eq!(
            parse(&["rm", ".env", "TOKEN", "--line", "12", "--json"]),
            Ok(Command::Remove {
                file: PathBuf::from(".env"),
                key: "TOKEN".to_string(),
                selector: EnvKeyRemoveSelector::Line(12),
                expectation: None,
                output: OutputFormat::Json,
            })
        );
        assert_eq!(
            parse(&["rm", ".env", "TOKEN", "--all", "--expect", "present"]),
            Ok(Command::Remove {
                file: PathBuf::from(".env"),
                key: "TOKEN".to_string(),
                selector: EnvKeyRemoveSelector::All,
                expectation: Some(EnvKeyExpectation::Present),
                output: OutputFormat::Auto,
            })
        );
        assert_eq!(
            parse(&["rm", ".env", "TOKEN", "--expect", "absent"]),
            Ok(Command::Remove {
                file: PathBuf::from(".env"),
                key: "TOKEN".to_string(),
                selector: EnvKeyRemoveSelector::Only,
                expectation: Some(EnvKeyExpectation::Absent),
                output: OutputFormat::Auto,
            })
        );
    }

    #[test]
    fn rm_parser_rejects_conflicting_and_malformed_selectors() {
        for arguments in [
            vec!["rm"],
            vec!["rm", ".env"],
            vec!["rm", ".env", "TOKEN", "extra"],
            vec!["rm", ".env", "TOKEN", "--all", "--line", "3"],
            vec!["rm", ".env", "TOKEN", "--line"],
            vec!["rm", ".env", "TOKEN", "--line", "0"],
            vec!["rm", ".env", "TOKEN", "--line", "second"],
            vec!["rm", ".env", "TOKEN", "--expect"],
            vec!["rm", ".env", "TOKEN", "--expect", "maybe"],
            vec![
                "rm", ".env", "TOKEN", "--expect", "present", "--expect", "absent",
            ],
            vec!["rm", ".env", "TOKEN", "--values"],
        ] {
            assert!(
                parse(&arguments).is_err(),
                "{arguments:?} should be refused"
            );
        }
    }

    #[test]
    fn rm_help_documents_the_one_shot_line_selector() {
        let help = usage_text(HelpTopic::Remove);
        assert!(help.contains("--line"));
        assert!(help.contains("never"));
        assert!(help.contains("hunts for another occurrence"));
        assert!(help.contains("alreadyAbsent"));
        assert!(help.contains("--expect"));
    }

    #[test]
    fn parses_rename_with_two_keys() {
        assert_eq!(
            parse(&["rename", ".env", "OPENAI_KEY", "OPENAI_API_KEY", "--json"]),
            Ok(Command::Rename {
                file: PathBuf::from(".env"),
                from: "OPENAI_KEY".to_string(),
                to: "OPENAI_API_KEY".to_string(),
                output: OutputFormat::Json,
            })
        );
    }

    #[test]
    fn rename_parser_rejects_missing_extra_and_unknown_arguments() {
        for arguments in [
            vec!["rename"],
            vec!["rename", ".env"],
            vec!["rename", ".env", "OLD_KEY"],
            vec!["rename", ".env", "OLD_KEY", "NEW_KEY", "extra"],
            vec!["rename", ".env", "OLD_KEY", "NEW_KEY", "--values"],
            vec!["rename", ".env", "OLD_KEY", "NEW_KEY", "--json", "--text"],
        ] {
            assert!(
                parse(&arguments).is_err(),
                "{arguments:?} should be refused"
            );
        }
    }

    #[test]
    fn rename_help_documents_what_it_preserves_and_what_it_refuses() {
        let help = usage_text(HelpTopic::Rename);
        assert!(help.contains("byte-identical"));
        assert!(help.contains("inline comment"));
        assert!(help.contains("never merges"));
        assert!(help.contains("cannot prove"));
    }

    #[test]
    fn parses_example_with_an_optional_override_path() {
        assert_eq!(
            parse(&["example", ".env"]),
            Ok(Command::Example {
                file: PathBuf::from(".env"),
                example: None,
                output: OutputFormat::Auto,
            })
        );
        assert_eq!(
            parse(&["example", ".env", "--example", ".env.sample", "--json"]),
            Ok(Command::Example {
                file: PathBuf::from(".env"),
                example: Some(PathBuf::from(".env.sample")),
                output: OutputFormat::Json,
            })
        );
    }

    #[test]
    fn example_parser_rejects_missing_extra_and_unknown_arguments() {
        for arguments in [
            vec!["example"],
            vec!["example", ".env", "extra"],
            vec!["example", ".env", "--example"],
            vec!["example", ".env", "--values"],
            vec!["example", ".env", "--example", "a", "--example", "b"],
        ] {
            assert!(
                parse(&arguments).is_err(),
                "{arguments:?} should be refused"
            );
        }
    }

    #[test]
    fn example_help_documents_the_additive_contract() {
        let help = usage_text(HelpTopic::Example);
        assert!(help.contains("never rewritten and never deleted"));
        assert!(help.contains("valueless placeholders"));
        assert!(help.contains("alreadyCurrent"));
    }

    #[test]
    fn add_rejects_duplicate_key_with_line_context() {
        let dir = tempfile::tempdir().unwrap();
        let env_path = dir.path().join(".env");
        fs::write(&env_path, "PORT=1420\nPORT=3000\n").unwrap();

        let error = run_add(
            &env_path,
            "PORT",
            "8080",
            OutputFormat::Text,
            ValueOutput::Hidden,
        )
        .expect_err("duplicate add should fail");

        assert_eq!(
            error.to_string(),
            "`PORT` already exists at lines 1, 2; edit the existing occurrence instead"
        );
        assert_eq!(
            fs::read_to_string(env_path).unwrap(),
            "PORT=1420\nPORT=3000\n"
        );
    }

    #[test]
    fn parses_run_files_before_the_separator_and_the_command_after_it() {
        assert_eq!(
            parse(&["run", "a.env", "b.env", "--", "node", "--help", "--", "x"]).unwrap(),
            Command::Run {
                files: vec![PathBuf::from("a.env"), PathBuf::from("b.env")],
                program: "node".to_string(),
                args: vec!["--help".to_string(), "--".to_string(), "x".to_string()],
            }
        );
        assert_eq!(
            parse(&["run", "--help"]).unwrap(),
            Command::Help(HelpTopic::Run)
        );
    }

    #[test]
    fn rejects_run_without_a_separator_files_or_command() {
        for (input, expected) in [
            (&["run", "a.env", "node"][..], "run requires `--`"),
            (&["run", "--", "node"][..], "at least one env file"),
            (&["run", "a.env", "--"][..], "a command after `--`"),
            (
                &["run", "--values", "a.env", "--", "node"][..],
                "unknown run option",
            ),
        ] {
            let message = parse(input).unwrap_err();
            assert!(message.contains(expected), "{input:?} gave {message}");
        }
    }

    #[test]
    fn run_refuses_commands_that_print_the_environment() {
        let refused = [
            ("env", vec![]),
            ("/usr/bin/env", vec!["-0"]),
            ("printenv", vec![]),
            ("sh", vec!["-c", "env"]),
            (
                "bash",
                vec!["-o", "pipefail", "-lc", "echo start; printenv | sort"],
            ),
            ("/bin/zsh", vec!["-c", "x=$(env)"]),
            ("sh", vec!["-ec", "FOO=1 exec env"]),
            ("bash", vec!["-c", "declare -px"]),
            ("sh", vec!["-c", "true && export"]),
            ("sh", vec!["-c", "set"]),
        ];
        for (program, args) in refused {
            let args = args.into_iter().map(String::from).collect::<Vec<_>>();
            assert!(
                environment_printer_refusal(program, &args).is_some(),
                "{program} {args:?} should be refused"
            );
        }
    }

    #[test]
    fn run_allows_ordinary_commands_and_scripts() {
        let allowed = [
            ("node", vec!["server.js"]),
            ("envoy", vec![]),
            ("sh", vec!["./deploy.sh", "env"]),
            ("sh", vec!["-c", "set -euo pipefail; node x.js"]),
            ("bash", vec!["-c", "export PATH=/opt/bin:$PATH; make"]),
            ("sh", vec!["-c", "grep env README.md"]),
            ("python3", vec!["-c", "import os; print(os.environ)"]),
        ];
        for (program, args) in allowed {
            let args = args.into_iter().map(String::from).collect::<Vec<_>>();
            assert!(
                environment_printer_refusal(program, &args).is_none(),
                "{program} {args:?} should be allowed"
            );
        }
    }
}
