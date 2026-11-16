use clap::Parser;
use tracing_subscriber::EnvFilter;

use crate::cli::{Cli, dispatch, render::emit_json};
use crate::error::ExitCode;

pub fn init_logging(verbose: u8, quiet: bool, no_color: bool) {
    let level = if quiet {
        "error"
    } else {
        match verbose {
            0 => "warn",
            1 => "info",
            2 => "debug",
            _ => "trace",
        }
    };
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(level));
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .with_ansi(!no_color)
        .without_time()
        .try_init();
}

pub async fn run() -> ExitCode {
    let json_requested = std::env::args_os().any(|arg| arg == "--json");
    let cli = match Cli::try_parse() {
        Ok(parsed) => parsed,
        Err(err) => {
            use clap::error::ErrorKind;
            match err.kind() {
                ErrorKind::DisplayHelp | ErrorKind::DisplayVersion => {
                    let _ = err.print();
                    return ExitCode::Success;
                }
                _ => {
                    if json_requested {
                        let env = crate::error::ErrorEnvelope {
                            ok: false,
                            exit_code: ExitCode::ConfigOrArgs.code(),
                            category: ExitCode::ConfigOrArgs.category().to_string(),
                            message: err.to_string().trim().to_string(),
                            detail: None,
                        };
                        emit_json(&env);
                    } else {
                        let _ = err.print();
                    }
                    return ExitCode::ConfigOrArgs;
                }
            }
        }
    };

    init_logging(cli.verbose, cli.quiet, cli.no_color);
    let emit_json_errors = cli.json;

    match dispatch(cli).await {
        Ok(()) => ExitCode::Success,
        Err(err) => {
            if emit_json_errors {
                emit_json(&err.to_envelope());
            } else {
                let msg = crate::domain::media_manifest::redact_urls_in_text(&err.to_string());
                eprintln!("error: {msg}");
            }
            err.exit_code()
        }
    }
}
