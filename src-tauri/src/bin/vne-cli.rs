use std::process::ExitCode;

fn main() -> ExitCode {
    vne_lib::cli::run_from_env()
}
