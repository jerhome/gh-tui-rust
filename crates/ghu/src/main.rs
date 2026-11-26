//! The `ghu` binary: `ghu::prepare`, logging, then `ghu::run`.
//!
//! Logging (`tracing-subscriber`): off unless `GHU_LOG_FILE` names a file (appended); level from `GHU_LOG`
//! (`EnvFilter` syntax, default `info`). Never on stdout/stderr while the TUI runs. The token is never logged.
//! Exit code: the one of `Launch::Exit`, `0` after a normal quit, `1` on a terminal error (message on stderr after
//! the terminal is restored).

fn main() -> std::process::ExitCode {
    todo!()
}
