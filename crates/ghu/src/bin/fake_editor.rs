//! `ghu-fake-editor`: the editor used by ghu's tests (ADR 0003 § 5), never shipped.
//!
//! Called as `ghu-fake-editor <mode> [arg] <path>`, the path last (as ghu appends it). Modes:
//! `write <text>` (replaces the content; a literal `\n` becomes a newline), `upper` (uppercases the content),
//! `fail <code>` (exits with that code, file untouched), `bytes` (writes bytes that are not UTF-8), `fill <n>`
//! (writes `n` bytes `a`), `keep` (exits at once with code 0, file untouched). Writes nothing on stdout or
//! stderr and reads no other file. Bad usage: exit code 64.

fn main() {
    todo!("ADR 0003, wave 4: dev[tui-editor]")
}
