//! The `ghu` binary: `ghu::prepare`, logging, then `ghu::run`.
//!
//! Logging (`tracing-subscriber`): off unless `GHU_LOG_FILE` names a file (appended); level from `GHU_LOG`
//! (`EnvFilter` syntax, default `info`). Never on stdout/stderr while the TUI runs. The token is never logged.
//! Exit code: the one of `Launch::Exit`, `0` after a normal quit, `1` on a terminal error (message on stderr after
//! the terminal is restored).

use std::process::ExitCode;

use ghu::{Launch, SystemEnvironment};

fn main() -> ExitCode {
    if let Err(message) = init_logging() {
        eprintln!("ghu: {message}");
        return ExitCode::from(1);
    }

    let ctx = match ghu::prepare(
        std::env::args_os(),
        &SystemEnvironment,
        &mut std::io::stdout(),
        &mut std::io::stderr(),
    ) {
        Launch::Exit(code) => return ExitCode::from(code),
        Launch::Tui(ctx) => ctx,
    };

    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("ghu: cannot start the async runtime: {error}");
            return ExitCode::from(1);
        }
    };
    match runtime.block_on(ghu::run(ctx)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            tracing::error!(%error, "terminal error");
            eprintln!("ghu: terminal error: {error}");
            ExitCode::from(1)
        }
    }
}

/// Logs to `GHU_LOG_FILE` (appended) at the `GHU_LOG` level, if the variable is set and non-empty.
fn init_logging() -> Result<(), String> {
    let Some(path) = std::env::var_os("GHU_LOG_FILE").filter(|p| !p.is_empty()) else {
        return Ok(());
    };
    let filter = std::env::var("GHU_LOG").unwrap_or_else(|_| "info".to_owned());
    let filter = tracing_subscriber::EnvFilter::try_new(&filter)
        .map_err(|error| format!("invalid GHU_LOG {filter:?}: {error}"))?;
    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|error| {
            format!(
                "cannot open GHU_LOG_FILE {}: {error}",
                path.to_string_lossy()
            )
        })?;
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_ansi(false)
        .with_writer(std::sync::Mutex::new(file))
        .init();
    Ok(())
}
