//! Attow Network's installed command-line entry point.
use attow_cli::{CliError, args::Args, commands, display::Output};
use std::{ffi::OsString, process::ExitCode};
#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() -> ExitCode {
    let words: Vec<OsString> = std::env::args_os().skip(1).collect();
    let json = words.iter().any(|arg| arg == "--json");
    let command = words
        .first()
        .and_then(|arg| arg.to_str())
        .filter(|arg| {
            matches!(
                *arg,
                "init"
                    | "run"
                    | "branch"
                    | "compare"
                    | "promote"
                    | "replay"
                    | "verify"
                    | "doctor"
                    | "peer"
                    | "export"
                    | "import"
            )
        })
        .unwrap_or("help");
    let result = match Args::parse(words.clone()) {
        Ok(args) => match std::env::current_dir() {
            Ok(cwd) => commands::execute(args, &cwd).await,
            Err(error) => Err(CliError::infrastructure(error)),
        },
        Err(error) => Err(error),
    };
    let output = result.unwrap_or_else(|error| Output::error(command, error));
    match output.render(json) {
        Ok(text) => println!("{text}"),
        Err(_) => {
            eprintln!("output serialization failed");
            return ExitCode::from(5);
        }
    }
    ExitCode::from(output.exit_code)
}
