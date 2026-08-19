use crate::{
    append_env_key_result, atomic_write_preserving_permissions, compare_env_files, copy_env_key,
    ensure_project_env_file, parse_env_file, redact_env_file_values_only,
    redact_snapshot_values_only, remove_env_key, rename_env_key, set_env_key,
    withhold_all_env_file_payloads, withhold_all_snapshot_payloads, EnsureEnvFileOutcome,
    EnvComparison, EnvFile, EnvFileCreationDisposition, EnvKeyCopyDisposition, EnvKeyExpectation,
    EnvKeyRemoveDisposition, EnvKeyRemoveSelector, EnvKeySetDisposition, ProjectSnapshot,
};
use serde::Serialize;
use std::env;
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
        name: "copy",
        parse: parse_copy_args,
    },
    CliCommand {
        name: "create",
        parse: parse_create_args,
    },
];

const CLI_HELP_ALIASES: &[&str] = &["help", "-h", "--help"];

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
    Help(HelpTopic),
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
    Copy,
    Create,
    Format,
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

pub fn is_cli_invocation(args: &[String]) -> bool {
    args.first()
        .is_some_and(|arg| cli_command(arg).is_some() || CLI_HELP_ALIASES.contains(&arg.as_str()))
}

pub fn run_from_env() -> ExitCode {
    run_from_args(env::args().skip(1))
}

pub fn run_from_args(args: impl IntoIterator<Item = String>) -> ExitCode {
    match parse_args(args) {
        Ok(Command::Help(topic)) => {
            if print_usage(topic).is_ok() {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(2)
            }
        }
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
            prepare_add_target(&file, matches!(&value, ValueSource::Prompt))?;
            let value = resolve_value_source(&key, value)?;
            run_add(&file, &key, &value, output, values)
        }
        Command::Set {
            file,
            key,
            value,
            output,
        } => {
            prepare_set_target(&file)?;
            let value = resolve_value_source(&key, value)?;
            run_set(&file, &key, &value, output)
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
        Command::Copy {
            source_file,
            key,
            destination_file,
            overwrite,
            output,
        } => run_copy(&source_file, &key, &destination_file, overwrite, output),
        Command::Create { file, output } => run_create(&file, output),
        Command::Help(_) => Ok(false),
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
        stderr(format_args!("value for {key}: "))?;
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
    let file = load_env_file(file_path)?;
    let example = example_path.map(load_env_file).transpose()?;
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
        if let Some(comparison) = &snapshot.comparison {
            print_comparison(comparison)?;
        }
    }

    Ok(has_findings)
}

fn run_format(file: &Path, dry_run: bool) -> Result<bool, Box<dyn std::error::Error>> {
    if !dry_run {
        return Err(
            "format currently requires --dry-run; vne does not rewrite files from the CLI yet"
                .into(),
        );
    }

    let content = read_to_string_with_path(file)?;
    let _ = parse_env_file(file, content.clone());
    if !io::stdout().is_terminal() {
        stderr_line(format_args!(
            "warning: format --dry-run emits raw env content to stdout"
        ))?;
    }
    stdout(format_args!("{content}"))?;
    Ok(false)
}

fn run_add(
    file: &Path,
    key: &str,
    value: &str,
    output: OutputFormat,
    value_output: ValueOutput,
) -> Result<bool, Box<dyn std::error::Error>> {
    let content = read_to_string_with_path(file)?;
    let updated =
        append_env_key_result(&content, key, value).map_err(|error| error.message(key))?;

    atomic_write_preserving_permissions(file, &updated)?;

    if output.wants_json() {
        let file = project_env_file_for_output(load_env_file(file)?, value_output);
        print_json(&file, output)?;
    } else {
        stdout_line(format_args!("added `{key}` to {}", file.display()))?;
    }

    Ok(false)
}

fn run_set(
    file: &Path,
    key: &str,
    value: &str,
    output: OutputFormat,
) -> Result<bool, Box<dyn std::error::Error>> {
    let outcome = set_env_key(file, key, value).map_err(|error| error.message(key))?;

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

    if output.wants_json() {
        print_json(&outcome, output)?;
    } else {
        match outcome.disposition {
            EnvKeyRemoveDisposition::Removed | EnvKeyRemoveDisposition::RemovedAll => {
                stdout_line(format_args!(
                    "removed `{}` from {} at {}",
                    outcome.key,
                    outcome.path,
                    line_label(&outcome.removed_lines)
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

fn line_label(line_numbers: &[usize]) -> String {
    let lines = line_numbers
        .iter()
        .map(|line| line.to_string())
        .collect::<Vec<_>>()
        .join(", ");
    if line_numbers.len() == 1 {
        format!("line {lines}")
    } else {
        format!("lines {lines}")
    }
}

fn run_rename(
    file: &Path,
    from: &str,
    to: &str,
    output: OutputFormat,
) -> Result<bool, Box<dyn std::error::Error>> {
    let outcome = rename_env_key(file, from, to).map_err(|error| error.message(from, to))?;

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
    let outcome = copy_env_key(source_file, key, destination_file, overwrite)
        .map_err(|error| error.message(key))?;

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

fn load_env_file(path: &Path) -> Result<EnvFile, Box<dyn std::error::Error>> {
    let content = read_to_string_with_path(path)?;
    let display_path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    Ok(parse_env_file(&display_path, content))
}

fn read_to_string_with_path(path: &Path) -> io::Result<String> {
    fs::read_to_string(path).map_err(|error| {
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
        "{}: {} keys, {} diagnostics, {} redacted",
        file.path,
        file.entries.len(),
        file.diagnostics.len(),
        redacted
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
    !snapshot.findings.is_empty() || snapshot.files.iter().any(has_file_findings)
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

    let Some(entry) = cli_command(&command) else {
        return Err(ParseError::new(
            HelpTopic::General,
            format!("unknown command `{command}`"),
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
                    format!("unknown check option `{arg}`"),
                ));
            }
            _ if file.is_none() => file = Some(PathBuf::from(arg)),
            _ => {
                return Err(ParseError::new(
                    HelpTopic::Check,
                    format!("unexpected check argument `{arg}`"),
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
                    format!("unknown inspect option `{arg}`"),
                ));
            }
            _ if dir.is_none() => dir = Some(PathBuf::from(arg)),
            _ => {
                return Err(ParseError::new(
                    HelpTopic::Inspect,
                    format!("unexpected inspect argument `{arg}`"),
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
                    format!("unknown format option `{arg}`"),
                ));
            }
            _ if file.is_none() => file = Some(PathBuf::from(arg)),
            _ => {
                return Err(ParseError::new(
                    HelpTopic::Format,
                    format!("unexpected format argument `{arg}`"),
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
    let mut positionals = Vec::new();

    for arg in args {
        match arg.as_str() {
            "--text" => set_output_format(&mut output, OutputFormat::Text, HelpTopic::Set)?,
            "--json" => set_output_format(&mut output, OutputFormat::Json, HelpTopic::Set)?,
            "--pretty" => set_output_format(&mut output, OutputFormat::PrettyJson, HelpTopic::Set)?,
            "--prompt" => set_value_source(&mut value, ValueSource::Prompt, HelpTopic::Set)?,
            "--stdin" => set_value_source(&mut value, ValueSource::Stdin, HelpTopic::Set)?,
            "-h" | "--help" => return Ok(Command::Help(HelpTopic::Set)),
            _ if arg.starts_with('-') => {
                return Err(ParseError::new(
                    HelpTopic::Set,
                    format!("unknown set option `{arg}`; {SET_VALUE_SOURCE_RULE}"),
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
                        format!("--line requires a positive line number, not `{value}`"),
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
                            format!("--expect accepts `present` or `absent`, not `{value}`"),
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
                    format!("unknown rm option `{arg}`"),
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
                    format!("unknown rename option `{arg}`"),
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

    Ok(Command::Rename {
        file: PathBuf::from(file),
        from: from.clone(),
        to: to.clone(),
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
                    format!("unknown create option `{arg}`"),
                ));
            }
            _ if file.is_none() => file = Some(PathBuf::from(arg)),
            _ => {
                return Err(ParseError::new(
                    HelpTopic::Create,
                    format!("unexpected create argument `{arg}`"),
                ));
            }
        }
    }

    let file =
        file.ok_or_else(|| ParseError::new(HelpTopic::Create, "create requires an env file path"))?;
    Ok(Command::Create { file, output })
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
                    format!("unknown copy option `{arg}`"),
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
            "vne - local private .env viewer and editor\n\nUSAGE:\n  vne [project-dir]\n  vne create <file> [--text|--json|--pretty]\n  vne check <file> [--example <file>] [--text|--json|--pretty] [--values]\n  vne inspect <dir> [--text|--json|--pretty] [--values]\n  vne add <file> <KEY> <VALUE> [--text|--json|--pretty] [--values]\n  vne add <file> <KEY=VALUE> [--text|--json|--pretty] [--values]\n  vne add <file> <KEY> --value <VALUE> [--text|--json|--pretty] [--values]\n  vne add <file> <KEY> --stdin [--text|--json|--pretty] [--values]\n  vne add <file> <KEY> --prompt [--text|--json|--pretty] [--values]\n  vne set <file> <KEY> --stdin [--text|--json|--pretty]\n  vne set <file> <KEY> --prompt [--text|--json|--pretty]\n  vne rm <file> <KEY> [--all|--line <N>] [--expect present|absent] [--text|--json|--pretty]\n  vne rename <file> <OLD_KEY> <NEW_KEY> [--text|--json|--pretty]\n  vne copy <source-file> <KEY> <destination-file> [--overwrite] [--text|--json|--pretty]\n  vne format <file> --dry-run\n\nOUTPUT:\n  Human text is used on a terminal. Piped output is JSON unless --text is set.\n  JSON withholds all env values, comments, and raw content by default.\n  copy, set, rm, and rename output contains only state, normalized paths, keys, and lines.\n  --values includes classifier-approved values; comments and raw content stay withheld.\n  --json emits compact JSON; --pretty emits formatted JSON.\n  format --dry-run is intentionally raw and warns when piped.\n\nEXAMPLES:\n  vne .\n  vne create .env --json\n  vne inspect fixtures/demo --json\n  vne inspect fixtures/demo --json --values\n  vne check fixtures/demo/.env --example fixtures/demo/.env.example --json\n  vne add .env FEATURE_FLAG=true --json\n  vne add .env OPENAI_API_KEY --prompt\n  vne set .env OPENAI_API_KEY --prompt\n  vne rm .env STALE_FLAG\n  vne rename .env OLD_NAME NEW_NAME\n  vne copy ../other/.env DATABASE_URL .env\n"
        }
        HelpTopic::Check => {
            "vne check - inspect one env file and optionally compare it with an example contract\n\nUSAGE:\n  vne check <file> [--example <file>] [--text|--json|--pretty] [--values]\n\nOPTIONS:\n  --example <file>  Compare actual keys against an example env file\n  --text            Force human text output\n  --json            Emit compact one-line JSON; payloads are withheld by default\n  --pretty          Emit formatted JSON; payloads are withheld by default\n  --values          Include classifier-approved values; comments and raw content stay withheld\n\nEXAMPLES:\n  vne check .env\n  vne check .env --example .env.example --json\n  vne check .env --json --values\n"
        }
        HelpTopic::Inspect => {
            "vne inspect - scan a project directory for env files and findings\n\nUSAGE:\n  vne inspect <dir> [--text|--json|--pretty] [--values]\n\nOPTIONS:\n  --text    Force human text output\n  --json    Emit compact one-line JSON; payloads are withheld by default\n  --pretty  Emit formatted JSON; payloads are withheld by default\n  --values  Include classifier-approved values; comments and raw content stay withheld\n\nEXAMPLES:\n  vne inspect .\n  vne inspect fixtures/demo --json\n  vne inspect fixtures/demo --json --values\n"
        }
        HelpTopic::Add => {
            "vne add - append one non-duplicate env key while preserving file formatting\n\nUSAGE:\n  vne add <file> <KEY> <VALUE> [--text|--json|--pretty] [--values]\n  vne add <file> <KEY=VALUE> [--text|--json|--pretty] [--values]\n  vne add <file> <KEY> --value <VALUE> [--text|--json|--pretty] [--values]\n  vne add <file> <KEY> --stdin [--text|--json|--pretty] [--values]\n  vne add <file> <KEY> --prompt [--text|--json|--pretty] [--values]\n\nOPTIONS:\n  --value <VALUE>  Read the new value from a flag\n  --stdin          Read the new value from stdin\n  --prompt         Offer to create a missing env file, then read a hidden value\n  --text           Force human text output\n  --json           Emit compact one-line JSON; payloads are withheld by default\n  --pretty         Emit formatted JSON; payloads are withheld by default\n  --values         Include classifier-approved values; comments and raw content stay withheld\n\nEXAMPLES:\n  vne add .env FEATURE_FLAG=true --json\n  printf '%s' 'sk-local' | vne add .env OPENAI_API_KEY --stdin --json\n  vne add .env OPENAI_API_KEY --prompt\n"
        }
        HelpTopic::Create => {
            "vne create - ensure one empty env file exists without overwriting it\n\nUSAGE:\n  vne create <file> [--text|--json|--pretty]\n\nBEHAVIOR:\n  The parent directory must already exist.\n  The filename must look like .env, .env.local, .envrc, .flaskenv, or worker.env.\n  A new file is empty and private to its owner on Unix.\n  An existing regular file is left unchanged and reported as alreadyExists.\n  Symlinks, directories, missing parent directories, and overwrite flags are rejected.\n\nOPTIONS:\n  --text    Force human text output\n  --json    Emit a compact disposition and absolute path\n  --pretty  Emit a formatted disposition and absolute path\n\nEXAMPLES:\n  vne create .env\n  vne create config.env --json\n  vne create /path/to/project/.env.local --pretty\n"
        }
        HelpTopic::Set => {
            "vne set - replace one existing env key value without exposing it\n\nUSAGE:\n  vne set <file> <KEY> --stdin [--text|--json|--pretty]\n  vne set <file> <KEY> --prompt [--text|--json|--pretty]\n\nBEHAVIOR:\n  The target must be an existing regular file holding exactly one occurrence of KEY.\n  The new value is read from stdin or a hidden prompt; argument value forms are refused.\n  An identical stored value reports alreadyPresent without writing.\n  Missing, duplicate, and malformed keys are refused with line numbers only.\n  An empty value clears the stored value; use `vne add` to create a new key.\n\nOPTIONS:\n  --stdin   Read the new value from stdin\n  --prompt  Read the new value from a hidden terminal prompt\n  --text    Force human text output\n  --json    Emit a compact payload-free set receipt\n  --pretty  Emit a formatted payload-free set receipt\n\nEXAMPLES:\n  vne set .env OPENAI_API_KEY --prompt\n  printf '%s' '1421' | vne set .env PORT --stdin --json\n"
        }
        HelpTopic::Remove => {
            "vne rm - delete one env key without reading its value\n\nUSAGE:\n  vne rm <file> <KEY> [--all|--line <N>] [--expect present|absent] [--text|--json|--pretty]\n\nBEHAVIOR:\n  A unique occurrence is removed and its whole line spliced out.\n  A duplicated key is refused with its line numbers unless --line or --all selects one.\n  --line <N> is a one-shot selector: if line N does not hold KEY it fails and never\n  hunts for another occurrence, so a stale line number cannot delete the wrong entry.\n  An absent key reports alreadyAbsent without writing, so a retry converges.\n  --expect states the key state you believe in and turns a wrong belief into exit 2.\n\nOPTIONS:\n  --all             Remove every occurrence in one write\n  --line <N>        Remove exactly the occurrence at line N\n  --expect present  Fail instead of reporting alreadyAbsent\n  --expect absent   Fail instead of removing anything\n  --text            Force human text output\n  --json            Emit a compact payload-free removal receipt\n  --pretty          Emit a formatted payload-free removal receipt\n\nEXAMPLES:\n  vne rm .env STALE_FLAG\n  vne rm .env DUPLICATED --line 12 --json\n  vne rm .env OPENAI_API_KEY --expect present\n"
        }
        HelpTopic::Rename => {
            "vne rename - rename one env key while leaving its value untouched\n\nUSAGE:\n  vne rename <file> <OLD_KEY> <NEW_KEY> [--text|--json|--pretty]\n\nBEHAVIOR:\n  Only the key token changes. The value bytes, quoting, inline comment, export\n  prefix, spacing, and line position stay byte-identical.\n  The old key must occur exactly once and be well formed.\n  An existing new key is refused; vne never merges two keys.\n  When the old key is absent and the new key is present, vne refuses instead of\n  reporting success: file state cannot prove that this rename is what happened.\n\nOPTIONS:\n  --text    Force human text output\n  --json    Emit a compact payload-free rename receipt\n  --pretty  Emit a formatted payload-free rename receipt\n\nEXAMPLES:\n  vne rename .env OLD_NAME NEW_NAME\n  vne rename .env OPENAI_KEY OPENAI_API_KEY --json\n"
        }
        HelpTopic::Copy => {
            "vne copy - copy one env key between existing files without exposing its value\n\nUSAGE:\n  vne copy <source-file> <KEY> <destination-file> [--overwrite] [--text|--json|--pretty]\n\nBEHAVIOR:\n  Source and destination must be distinct existing regular files.\n  The source key must occur exactly once and be well formed.\n  An absent destination key is added. An identical value reports alreadyPresent without writing.\n  A different destination value requires --overwrite. Duplicate destination keys are refused.\n  The value stays inside vne; output contains only state, normalized paths, and key.\n\nOPTIONS:\n  --overwrite  Replace one different existing destination value\n  --text       Force human text output\n  --json       Emit a compact payload-free copy receipt\n  --pretty     Emit a formatted payload-free copy receipt\n\nEXAMPLE:\n  vne copy ../other/.env DATABASE_URL .env\n  vne copy ../other/.env DATABASE_URL .env --overwrite --json\n"
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
                Err(format!("unknown command `{name}`")),
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
    fn parses_set_with_hidden_value_sources_only() {
        assert_eq!(
            parse(&["set", ".env", "OPENAI_API_KEY", "--stdin", "--json"]),
            Ok(Command::Set {
                file: PathBuf::from(".env"),
                key: "OPENAI_API_KEY".to_string(),
                value: ValueSource::Stdin,
                output: OutputFormat::Json,
            })
        );
        assert_eq!(
            parse(&["set", ".env", "OPENAI_API_KEY", "--prompt"]),
            Ok(Command::Set {
                file: PathBuf::from(".env"),
                key: "OPENAI_API_KEY".to_string(),
                value: ValueSource::Prompt,
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
}
