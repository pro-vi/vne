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

    if !desktop_assets_available(cfg!(debug_assertions), tauri::is_dev()) {
        let _ = print_desktop_usage_error(
            "this release was installed without bundled desktop assets; run `npm run install:local` from the vne project",
        );
        return ExitCode::from(2);
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
    stderr_line(format_args!("  vne create <file> [--text|--json|--pretty]"))?;
    stderr_line(format_args!(
        "  vne add <file> <KEY> --prompt [--json] [--values]"
    ))?;
    stderr_line(format_args!(
        "  vne inspect <dir> [--text|--json|--pretty] [--values]"
    ))?;
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
        [path] if is_desktop_path_argument(path) => project_path_for(path).map(Some),
        [arg] if arg.starts_with('-') => Err(format!("unknown option `{arg}`")),
        [arg] => Err(format!("unknown command or project path `{arg}`")),
        _ => Err("desktop launch accepts at most one project path".to_string()),
    }
}

fn is_desktop_path_argument(arg: &str) -> bool {
    arg == "." || arg == ".." || arg.contains(std::path::MAIN_SEPARATOR) || Path::new(arg).exists()
}

fn project_path_for(arg: &str) -> Result<String, String> {
    let path = PathBuf::from(arg)
        .canonicalize()
        .map_err(|error| format!("could not open project path `{arg}`: {error}"))?;
    let project_path = if path.is_file() {
        path.parent()
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf()
    } else if path.is_dir() {
        path
    } else {
        return Err(format!("project path `{arg}` is not a file or directory"));
    };

    Ok(project_path.to_string_lossy().to_string())
}

fn desktop_assets_available(debug_build: bool, tauri_dev_mode: bool) -> bool {
    debug_build || !tauri_dev_mode
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
        let expected = std::env::current_dir()
            .unwrap()
            .canonicalize()
            .unwrap()
            .to_string_lossy()
            .to_string();
        assert_eq!(desktop_project_path(&args(&["."])), Ok(Some(expected)));
    }

    #[test]
    fn desktop_launch_anchors_file_paths_to_their_project_directory() {
        let temp = tempfile::tempdir().unwrap();
        let env_path = temp.path().join(".env");
        std::fs::write(&env_path, "PORT=1420\n").unwrap();

        assert_eq!(
            desktop_project_path(&args(&[env_path.to_str().unwrap()])),
            Ok(Some(
                temp.path()
                    .canonicalize()
                    .unwrap()
                    .to_string_lossy()
                    .to_string()
            ))
        );
    }

    #[test]
    fn desktop_release_requires_embedded_assets() {
        assert!(!desktop_assets_available(false, true));
        assert!(desktop_assets_available(true, true));
        assert!(desktop_assets_available(false, false));
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
