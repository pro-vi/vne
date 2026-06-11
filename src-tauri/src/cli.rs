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
        json: bool,
    },
    Inspect {
        dir: PathBuf,
        json: bool,
    },
    Format {
        file: PathBuf,
        dry_run: bool,
    },
    Add {
        file: PathBuf,
        key: String,
        value: AddValue,
        json: bool,
    },
    Help,
}

#[derive(Debug, PartialEq, Eq)]
enum AddValue {
    Inline(String),
    Prompt,
    Stdin,
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
        Ok(Command::Help) => {
            if print_usage().is_ok() {
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
            let _ = stderr_line(format_args!("vne: {error}"));
            let _ = stderr_line(format_args!(""));
            let _ = print_usage();
            ExitCode::from(2)
        }
    }
}

fn run(command: Command) -> Result<bool, Box<dyn std::error::Error>> {
    match command {
        Command::Check {
            file,
            example,
            json,
        } => run_check(&file, example.as_deref(), json),
        Command::Inspect { dir, json } => run_inspect(&dir, json),
        Command::Format { file, dry_run } => run_format(&file, dry_run),
        Command::Add {
            file,
            key,
            value,
            json,
        } => {
            let value = read_add_value(&key, value)?;
            run_add(&file, &key, &value, json)
        }
        Command::Help => Ok(false),
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
    json: bool,
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

    if json {
        stdout_line(format_args!("{}", serde_json::to_string_pretty(&output)?))?;
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

fn run_inspect(dir: &Path, json: bool) -> Result<bool, Box<dyn std::error::Error>> {
    let snapshot = redact_snapshot(crate::snapshot_project(dir)?);
    let has_findings = has_snapshot_findings(&snapshot);

    if json {
        stdout_line(format_args!("{}", serde_json::to_string_pretty(&snapshot)?))?;
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

    let content = fs::read_to_string(file)?;
    let _ = parse_env_file(file, content.clone());
    stdout(format_args!("{content}"))?;
    Ok(false)
}

fn run_add(
    file: &Path,
    key: &str,
    value: &str,
    json: bool,
) -> Result<bool, Box<dyn std::error::Error>> {
    let content = fs::read_to_string(file)?;
    let updated =
        append_env_key_result(&content, key, value).map_err(|error| error.message(key))?;

    atomic_write_preserving_permissions(file, &updated)?;

    if json {
        let file = redact_env_file(load_env_file(file)?);
        stdout_line(format_args!("{}", serde_json::to_string_pretty(&file)?))?;
    } else {
        stdout_line(format_args!("added `{key}` to {}", file.display()))?;
    }

    Ok(false)
}

fn load_env_file(path: &Path) -> Result<EnvFile, Box<dyn std::error::Error>> {
    let content = fs::read_to_string(path)?;
    let display_path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    Ok(parse_env_file(&display_path, content))
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

fn parse_args(args: impl IntoIterator<Item = String>) -> Result<Command, String> {
    let mut args = args.into_iter();
    let Some(command) = args.next() else {
        return Ok(Command::Help);
    };

    match command.as_str() {
        "-h" | "--help" | "help" => Ok(Command::Help),
        "check" => parse_check_args(args),
        "inspect" => parse_inspect_args(args),
        "format" => parse_format_args(args),
        "add" => parse_add_args(args),
        _ => Err(format!("unknown command `{command}`")),
    }
}

fn parse_check_args(args: impl Iterator<Item = String>) -> Result<Command, String> {
    let mut file = None;
    let mut example = None;
    let mut json = false;
    let mut args = args.peekable();

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--json" => json = true,
            "--example" => {
                let Some(path) = args.next() else {
                    return Err("--example requires a path".to_string());
                };
                example = Some(PathBuf::from(path));
            }
            "-h" | "--help" => return Ok(Command::Help),
            _ if arg.starts_with('-') => return Err(format!("unknown check option `{arg}`")),
            _ if file.is_none() => file = Some(PathBuf::from(arg)),
            _ => return Err(format!("unexpected check argument `{arg}`")),
        }
    }

    let file = file.ok_or_else(|| "check requires an env file path".to_string())?;
    Ok(Command::Check {
        file,
        example,
        json,
    })
}

fn parse_inspect_args(args: impl Iterator<Item = String>) -> Result<Command, String> {
    let mut dir = None;
    let mut json = false;

    for arg in args {
        match arg.as_str() {
            "--json" => json = true,
            "-h" | "--help" => return Ok(Command::Help),
            _ if arg.starts_with('-') => return Err(format!("unknown inspect option `{arg}`")),
            _ if dir.is_none() => dir = Some(PathBuf::from(arg)),
            _ => return Err(format!("unexpected inspect argument `{arg}`")),
        }
    }

    let dir = dir.ok_or_else(|| "inspect requires a project directory".to_string())?;
    Ok(Command::Inspect { dir, json })
}

fn parse_format_args(args: impl Iterator<Item = String>) -> Result<Command, String> {
    let mut file = None;
    let mut dry_run = false;

    for arg in args {
        match arg.as_str() {
            "--dry-run" => dry_run = true,
            "-h" | "--help" => return Ok(Command::Help),
            _ if arg.starts_with('-') => return Err(format!("unknown format option `{arg}`")),
            _ if file.is_none() => file = Some(PathBuf::from(arg)),
            _ => return Err(format!("unexpected format argument `{arg}`")),
        }
    }

    let file = file.ok_or_else(|| "format requires an env file path".to_string())?;
    Ok(Command::Format { file, dry_run })
}

fn parse_add_args(args: impl Iterator<Item = String>) -> Result<Command, String> {
    let mut json = false;
    let mut value = None;
    let mut positionals = Vec::new();
    let mut args = args.peekable();

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--json" => json = true,
            "--prompt" => set_add_value_source(&mut value, AddValue::Prompt)?,
            "--stdin" => set_add_value_source(&mut value, AddValue::Stdin)?,
            "--value" => {
                let Some(inline_value) = args.next() else {
                    return Err("--value requires a value".to_string());
                };
                set_add_value_source(&mut value, AddValue::Inline(inline_value))?;
            }
            "-h" | "--help" => return Ok(Command::Help),
            _ => positionals.push(arg),
        }
    }

    let (file, key, value) = match (positionals.as_slice(), value) {
        ([file, key], Some(value)) => (PathBuf::from(file), key.clone(), value),
        ([file, assignment], None) => {
            let Some((key, inline_value)) = assignment.split_once('=') else {
                return Err("add requires <file> <KEY> <VALUE>, <file> <KEY=VALUE>, or <file> <KEY> --prompt".to_string());
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
            return Err(
                "add accepts either <VALUE> or --prompt/--stdin/--value, not both".to_string(),
            );
        }
        _ => {
            return Err(
                "add requires <file> <KEY> <VALUE>, <file> <KEY=VALUE>, or <file> <KEY> --prompt"
                    .to_string(),
            );
        }
    };

    if key.is_empty() {
        return Err("add requires a non-empty key".to_string());
    }

    Ok(Command::Add {
        file,
        key,
        value,
        json,
    })
}

fn set_add_value_source(target: &mut Option<AddValue>, value: AddValue) -> Result<(), String> {
    if target.is_some() {
        return Err("add accepts only one of --prompt, --stdin, or --value".to_string());
    }
    *target = Some(value);
    Ok(())
}

fn print_usage() -> io::Result<()> {
    stdout_line(format_args!(
        "vne\n\nUSAGE:\n  vne [project-dir]\n  vne check <file> [--example <file>] [--json]\n  vne inspect <dir> [--json]\n  vne add <file> <KEY> <VALUE> [--json]\n  vne add <file> <KEY=VALUE> [--json]\n  vne add <file> <KEY> --prompt [--json]\n  vne add <file> <KEY> --stdin [--json]\n  vne format <file> --dry-run"
    ))
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
        parse_args(input.iter().map(|arg| arg.to_string()))
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
                json: true,
            })
        );
    }

    #[test]
    fn parses_inspect_with_json() {
        assert_eq!(
            parse(&["inspect", "fixtures/demo", "--json"]),
            Ok(Command::Inspect {
                dir: PathBuf::from("fixtures/demo"),
                json: true,
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
                json: true,
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
                json: false,
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
                json: false,
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
                json: true,
            })
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

        let error =
            run_add(&env_path, "PORT", "8080", false).expect_err("duplicate add should fail");

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
