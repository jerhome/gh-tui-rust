//! `ghu-fake-editor`: the editor used by ghu's tests (ADR 0003 § 5), never shipped.
//!
//! Called as `ghu-fake-editor <mode> [arg] <path>`, the path last (as ghu appends it). Modes:
//! `write <text>` (replaces the content; a literal `\n` becomes a newline), `upper` (uppercases the content),
//! `fail <code>` (exits with that code, file untouched), `bytes` (writes bytes that are not UTF-8), `fill <n>`
//! (writes `n` bytes `a`), `keep` (exits at once with code 0, file untouched). Writes nothing on stdout or
//! stderr and reads no other file. Bad usage: exit code 64; I/O error: exit code 74.

use std::ffi::OsString;
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::PathBuf;
use std::process::ExitCode;

/// Bad usage (sysexits `EX_USAGE`).
const USAGE: u8 = 64;
/// I/O error on the file (sysexits `EX_IOERR`).
const IO_ERROR: u8 = 74;

enum Failure {
    Usage,
    Io,
}

impl From<io::Error> for Failure {
    fn from(_: io::Error) -> Self {
        Self::Io
    }
}

fn main() -> ExitCode {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    match run(&args) {
        Ok(code) => code,
        Err(Failure::Usage) => ExitCode::from(USAGE),
        Err(Failure::Io) => ExitCode::from(IO_ERROR),
    }
}

fn run(args: &[OsString]) -> Result<ExitCode, Failure> {
    let (path, rest) = args.split_last().ok_or(Failure::Usage)?;
    let path = PathBuf::from(path);
    let words: Vec<&str> = rest
        .iter()
        .map(|arg| arg.to_str().ok_or(Failure::Usage))
        .collect::<Result<_, _>>()?;
    match words.as_slice() {
        ["write", text] => fs::write(&path, text.replace("\\n", "\n"))?,
        ["upper"] => {
            let text = fs::read_to_string(&path)?;
            fs::write(&path, text.to_uppercase())?;
        }
        ["fail", code] => {
            return Ok(ExitCode::from(
                code.parse::<u8>().map_err(|_| Failure::Usage)?,
            ));
        }
        ["bytes"] => fs::write(&path, [b'a', 0xff, 0xfe, b'\n'])?,
        ["fill", count] => {
            let count = count.parse::<u64>().map_err(|_| Failure::Usage)?;
            // Streamed: no buffer sized by the argument.
            io::copy(&mut io::repeat(b'a').take(count), &mut File::create(&path)?)?;
        }
        ["keep"] => {}
        _ => return Err(Failure::Usage),
    }
    Ok(ExitCode::SUCCESS)
}
