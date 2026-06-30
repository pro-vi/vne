use std::env;
use std::fmt;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

fn main() -> ExitCode {
    let args = env::args().skip(1).collect::<Vec<_>>();

    if vne_lib::cli::is_cli_invocation(&args) {
        return vne_lib::cli::run_from_args(args);
    }

    match desktop_project_path(&args) {
        Ok(initial_project_path) => {
            vne_lib::run_with_initial_project_path(initial_project_path);
            ExitCode::SUCCESS
        }
        Err(error) => {
            let _ = print_desktop_usage_error(&error);
            ExitCode::from(2)
        }
    }
}

fn print_desktop_usage_error(error: &str) -> io::Result<()> {
    stderr_line(format_args!("vne: {error}"))?;
    stderr_line(format_args!(""))?;
    stderr_line(format_args!("USAGE:"))?;
    stderr_line(format_args!("  vne [project-dir]"))?;
    stderr_line(format_args!("  vne add <file> <KEY> --prompt [--json]"))?;
    stderr_line(format_args!("  vne inspect <dir> [--text|--json|--pretty]"))?;
    stderr_line(format_args!("  vne help"))
}

fn stderr_line(args: fmt::Arguments<'_>) -> io::Result<()> {
    let mut stderr = io::stderr().lock();
    stderr.write_fmt(args)?;
    stderr.write_all(b"\n")
}

fn desktop_project_path(args: &[String]) -> Result<Option<String>, String> {
    match args {
        [] => Ok(None),
        [path] if is_desktop_path_argument(path) => Ok(Some(project_path_for(path))),
        [arg] if arg.starts_with('-') => Err(format!("unknown option `{arg}`")),
        [arg] => Err(format!("unknown command or project path `{arg}`")),
        _ => Err("desktop launch accepts at most one project path".to_string()),
    }
}

fn is_desktop_path_argument(arg: &str) -> bool {
    arg == "." || arg == ".." || arg.contains(std::path::MAIN_SEPARATOR) || Path::new(arg).exists()
}

fn project_path_for(arg: &str) -> String {
    let path = PathBuf::from(arg);
    if path.is_file() {
        return path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .to_string_lossy()
            .to_string();
    }

    path.to_string_lossy().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn desktop_launch_without_args_uses_app_default() {
        assert_eq!(desktop_project_path(&args(&[])), Ok(None));
    }

    #[test]
    fn desktop_launch_accepts_current_directory() {
        assert_eq!(
            desktop_project_path(&args(&["."])),
            Ok(Some(".".to_string()))
        );
    }

    #[test]
    fn desktop_launch_rejects_unknown_options() {
        assert!(desktop_project_path(&args(&["--json"])).is_err());
    }

    #[test]
    fn desktop_launch_rejects_extra_args() {
        assert!(desktop_project_path(&args(&[".", "--json"])).is_err());
    }
}
