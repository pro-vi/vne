use crate::{
    append_env_key_result, atomic_write_preserving_permissions, compare_env_files, parse_env_file,
    redact_env_file, redact_snapshot, EnvComparison, EnvFile, ProjectSnapshot,
};
use serde::Serialize;
use std::env;
use std::fmt;
use std::fs;
use std::io::{self, IsTerminal, Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;
use std::process::ExitCode;

const CLI_COMMANDS: &[&str] = &["check", "inspect", "format", "add", "help", "-h", "--help"];

#[derive(Debug, PartialEq, Eq)]
enum Command {
    Check {
        file: PathBuf,
        example: Option<PathBuf>,
        output: OutputFormat,
    },
    Inspect {
        dir: PathBuf,
        output: OutputFormat,
    },
    Format {
        file: PathBuf,
        dry_run: bool,
    },
    Add {
        file: PathBuf,
        key: String,
        value: AddValue,
        output: OutputFormat,
    },
    Help(HelpTopic),
}

#[derive(Debug, PartialEq, Eq)]
enum AddValue {
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
enum HelpTopic {
    General,
    Check,
    Inspect,
    Add,
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
        .is_some_and(|arg| CLI_COMMANDS.contains(&arg.as_str()))
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
        } => run_check(&file, example.as_deref(), output),
        Command::Inspect { dir, output } => run_inspect(&dir, output),
        Command::Format { file, dry_run } => run_format(&file, dry_run),
        Command::Add {
            file,
            key,
            value,
            output,
        } => {
            let value = read_add_value(&key, value)?;
            run_add(&file, &key, &value, output)
        }
        Command::Help(_) => Ok(false),
    }
}

fn read_add_value(key: &str, value: AddValue) -> Result<String, Box<dyn std::error::Error>> {
    match value {
        AddValue::Inline(value) => Ok(value),
        AddValue::Prompt => read_prompted_value(key),
        AddValue::Stdin => read_stdin_value(),
    }
}

fn read_prompted_value(key: &str) -> Result<String, Box<dyn std::error::Error>> {
    if !io::stdin().is_terminal() {
        return Err(
            "--prompt requires an interactive terminal; use --stdin for piped values".into(),
        );
    }

    stderr(format_args!("value for {key}: "))?;
    if !set_terminal_echo(false) {
        let _ = stderr_line(format_args!(""));
        return Err(
            "--prompt could not disable terminal echo; use --stdin or --value instead".into(),
        );
    }

    let mut value = String::new();
    let read_result = io::stdin().read_line(&mut value);
    let _ = set_terminal_echo(true);
    let _ = stderr_line(format_args!(""));
    read_result?;

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
        file: redact_env_file(file),
        example: example.map(redact_env_file),
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

fn run_inspect(dir: &Path, output: OutputFormat) -> Result<bool, Box<dyn std::error::Error>> {
    let snapshot = redact_snapshot(crate::snapshot_project(dir)?);
    let has_findings = has_snapshot_findings(&snapshot);

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
    stdout(format_args!("{content}"))?;
    Ok(false)
}

fn run_add(
    file: &Path,
    key: &str,
    value: &str,
    output: OutputFormat,
) -> Result<bool, Box<dyn std::error::Error>> {
    let content = read_to_string_with_path(file)?;
    let updated =
        append_env_key_result(&content, key, value).map_err(|error| error.message(key))?;

    atomic_write_preserving_permissions(file, &updated)?;

    if output.wants_json() {
        let file = redact_env_file(load_env_file(file)?);
        print_json(&file, output)?;
    } else {
        stdout_line(format_args!("added `{key}` to {}", file.display()))?;
    }

    Ok(false)
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

    match command.as_str() {
        "-h" | "--help" | "help" => Ok(Command::Help(HelpTopic::General)),
        "check" => parse_check_args(args),
        "inspect" => parse_inspect_args(args),
        "format" => parse_format_args(args),
        "add" => parse_add_args(args),
        _ => Err(ParseError::new(
            HelpTopic::General,
            format!("unknown command `{command}`"),
        )),
    }
}

fn parse_check_args(args: impl Iterator<Item = String>) -> Result<Command, ParseError> {
    let mut file = None;
    let mut example = None;
    let mut output = OutputFormat::Auto;
    let mut args = args.peekable();

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--text" => set_output_format(&mut output, OutputFormat::Text, HelpTopic::Check)?,
            "--json" => set_output_format(&mut output, OutputFormat::Json, HelpTopic::Check)?,
            "--pretty" => {
                set_output_format(&mut output, OutputFormat::PrettyJson, HelpTopic::Check)?
            }
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
    })
}

fn parse_inspect_args(args: impl Iterator<Item = String>) -> Result<Command, ParseError> {
    let mut dir = None;
    let mut output = OutputFormat::Auto;

    for arg in args {
        match arg.as_str() {
            "--text" => set_output_format(&mut output, OutputFormat::Text, HelpTopic::Inspect)?,
            "--json" => set_output_format(&mut output, OutputFormat::Json, HelpTopic::Inspect)?,
            "--pretty" => {
                set_output_format(&mut output, OutputFormat::PrettyJson, HelpTopic::Inspect)?
            }
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
    Ok(Command::Inspect { dir, output })
}

fn parse_format_args(args: impl Iterator<Item = String>) -> Result<Command, ParseError> {
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

fn parse_add_args(args: impl Iterator<Item = String>) -> Result<Command, ParseError> {
    let mut output = OutputFormat::Auto;
    let mut value = None;
    let mut positionals = Vec::new();
    let mut args = args.peekable();

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--text" => set_output_format(&mut output, OutputFormat::Text, HelpTopic::Add)?,
            "--json" => set_output_format(&mut output, OutputFormat::Json, HelpTopic::Add)?,
            "--pretty" => set_output_format(&mut output, OutputFormat::PrettyJson, HelpTopic::Add)?,
            "--prompt" => set_add_value_source(&mut value, AddValue::Prompt)?,
            "--stdin" => set_add_value_source(&mut value, AddValue::Stdin)?,
            "--value" => {
                let Some(inline_value) = args.next() else {
                    return Err(ParseError::new(HelpTopic::Add, "--value requires a value"));
                };
                set_add_value_source(&mut value, AddValue::Inline(inline_value))?;
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
                AddValue::Inline(inline_value.to_string()),
            )
        }
        ([file, key, inline_value], None) => (
            PathBuf::from(file),
            key.clone(),
            AddValue::Inline(inline_value.clone()),
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

fn set_add_value_source(target: &mut Option<AddValue>, value: AddValue) -> Result<(), ParseError> {
    if target.is_some() {
        return Err(ParseError::new(
            HelpTopic::Add,
            "add accepts only one of --prompt, --stdin, or --value",
        ));
    }
    *target = Some(value);
    Ok(())
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
            "vne - local private .env viewer and editor\n\nUSAGE:\n  vne [project-dir]\n  vne check <file> [--example <file>] [--text|--json|--pretty]\n  vne inspect <dir> [--text|--json|--pretty]\n  vne add <file> <KEY> <VALUE> [--text|--json|--pretty]\n  vne add <file> <KEY=VALUE> [--text|--json|--pretty]\n  vne add <file> <KEY> --value <VALUE> [--text|--json|--pretty]\n  vne add <file> <KEY> --stdin [--text|--json|--pretty]\n  vne add <file> <KEY> --prompt [--text|--json|--pretty]\n  vne format <file> --dry-run\n\nOUTPUT:\n  Human text is used on a terminal. Piped output is JSON unless --text is set.\n  --json emits compact one-line JSON; --pretty emits formatted JSON.\n\nEXAMPLES:\n  vne .\n  vne inspect fixtures/demo --json\n  vne check fixtures/demo/.env --example fixtures/demo/.env.example --json\n  vne add .env FEATURE_FLAG=true --json\n  vne add .env OPENAI_API_KEY --prompt\n"
        }
        HelpTopic::Check => {
            "vne check - inspect one env file and optionally compare it with an example contract\n\nUSAGE:\n  vne check <file> [--example <file>] [--text|--json|--pretty]\n\nOPTIONS:\n  --example <file>  Compare actual keys against an example env file\n  --text            Force human text output\n  --json            Emit compact one-line JSON\n  --pretty          Emit formatted JSON\n\nEXAMPLES:\n  vne check .env\n  vne check .env --example .env.example --json\n"
        }
        HelpTopic::Inspect => {
            "vne inspect - scan a project directory for env files and findings\n\nUSAGE:\n  vne inspect <dir> [--text|--json|--pretty]\n\nOPTIONS:\n  --text    Force human text output\n  --json    Emit compact one-line JSON\n  --pretty  Emit formatted JSON\n\nEXAMPLES:\n  vne inspect .\n  vne inspect fixtures/demo --json\n"
        }
        HelpTopic::Add => {
            "vne add - append one non-duplicate env key while preserving file formatting\n\nUSAGE:\n  vne add <file> <KEY> <VALUE> [--text|--json|--pretty]\n  vne add <file> <KEY=VALUE> [--text|--json|--pretty]\n  vne add <file> <KEY> --value <VALUE> [--text|--json|--pretty]\n  vne add <file> <KEY> --stdin [--text|--json|--pretty]\n  vne add <file> <KEY> --prompt [--text|--json|--pretty]\n\nOPTIONS:\n  --value <VALUE>  Read the new value from a flag\n  --stdin          Read the new value from stdin\n  --prompt         Read the new value from an interactive hidden prompt\n  --text           Force human text output\n  --json           Emit compact one-line JSON\n  --pretty         Emit formatted JSON\n\nEXAMPLES:\n  vne add .env FEATURE_FLAG=true --json\n  printf '%s' 'sk-local' | vne add .env OPENAI_API_KEY --stdin --json\n  vne add .env OPENAI_API_KEY --prompt\n"
        }
        HelpTopic::Format => {
            "vne format - print the parsed env file without rewriting it\n\nUSAGE:\n  vne format <file> --dry-run\n\nOPTIONS:\n  --dry-run  Required; print the current file content instead of writing\n\nEXAMPLE:\n  vne format .env --dry-run\n"
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
            })
        );
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
                value: AddValue::Inline("redis://localhost:6379".to_string()),
                output: OutputFormat::Json,
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
                value: AddValue::Inline("true".to_string()),
                output: OutputFormat::Auto,
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
                value: AddValue::Prompt,
                output: OutputFormat::Auto,
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
                value: AddValue::Stdin,
                output: OutputFormat::Json,
            })
        );
    }

    #[test]
    fn parses_command_specific_help() {
        assert_eq!(parse(&["add", "--help"]), Ok(Command::Help(HelpTopic::Add)));
        assert_eq!(
            parse(&["check", "--help"]),
            Ok(Command::Help(HelpTopic::Check))
        );
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
    fn rejects_add_with_inline_and_prompt_value_sources() {
        assert!(parse(&["add", ".env", "OPENAI_API_KEY", "sk-test", "--prompt"]).is_err());
        assert!(parse(&["add", ".env", "OPENAI_API_KEY", "--prompt", "--stdin"]).is_err());
    }

    #[test]
    fn add_rejects_duplicate_key_with_line_context() {
        let dir = tempfile::tempdir().unwrap();
        let env_path = dir.path().join(".env");
        fs::write(&env_path, "PORT=1420\nPORT=3000\n").unwrap();

        let error = run_add(&env_path, "PORT", "8080", OutputFormat::Text)
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
