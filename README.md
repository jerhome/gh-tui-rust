# ghu — a GitHub client in your terminal

`ghu` lists your GitHub repos and their pull requests, shows a PR (description, verdicts, conversation) and its
diff, and lets you publish a Comment or a Review (Approve / Request changes) on it. Nothing is published without a
confirmation screen.

Repository: `jerhome/gh-tui-rust` (this repo). Licence: MIT. Version 0.1.

## Install

Each release has one archive per platform and a `SHA256SUMS` file. The archive holds a folder
`ghu-<version>-<target>/` with the `ghu` executable, this README and the licence.

| Platform | Archive |
|---|---|
| Linux / WSL (x86_64) | `ghu-<version>-x86_64-unknown-linux-gnu.tar.gz` |
| Windows (x86_64) | `ghu-<version>-x86_64-pc-windows-msvc.zip` |
| macOS (Apple silicon) | `ghu-<version>-aarch64-apple-darwin.tar.gz` (built, never tried) |

Download the archive and `SHA256SUMS` into the same folder, check the archive, then unpack it.

Linux / WSL:

```bash
sha256sum --ignore-missing --check SHA256SUMS   # the line of your archive must say: OK
tar -xzf ghu-0.1.0-x86_64-unknown-linux-gnu.tar.gz
install -m 755 ghu-0.1.0-x86_64-unknown-linux-gnu/ghu ~/.local/bin/ghu   # any folder on your PATH
ghu --version
```

Windows (PowerShell):

```powershell
$archive = 'ghu-0.1.0-x86_64-pc-windows-msvc.zip'
(Get-FileHash $archive -Algorithm SHA256).Hash.ToLower()   # compare with its line in SHA256SUMS
Select-String -Path SHA256SUMS -Pattern $archive
Expand-Archive $archive -DestinationPath .
.\ghu-0.1.0-x86_64-pc-windows-msvc\ghu.exe --version
```

Then copy `ghu.exe` to a folder on your `Path`. Run it in Windows Terminal (or any console that supports
alternate screen and raw mode).

From source (Rust stable): `cargo install --locked --path crates/ghu --bin ghu`.

## Authentication

`ghu` uses the token of the GitHub CLI. At start-up it takes the first non-empty of:

1. `GH_TOKEN`;
2. `GITHUB_TOKEN`;
3. the output of `gh auth token` (run only when both variables are unset or empty).

With none of them, `ghu` prints `ghu: no GitHub token found.` with a hint (`gh auth login`) and exits with
code 1. The usual setup is simply `gh auth login` once; the classic token it creates (scope `repo`) covers
everything `ghu` does.

`ghu` stores no token and writes it nowhere: not on screen, not in logs, not in error messages. It is not passed
to your editor either (`GH_TOKEN` and `GITHUB_TOKEN` are removed from the editor's environment). A token GitHub
refuses shows up as an error inside the TUI.

## Screens and keys

Screens form a stack: Repo list → Pull requests → PR detail → Diff, Comment or Review. `?` shows the keys of the
current screen, `Esc` goes back, `q` quits, `Ctrl-C` quits from anywhere (also while typing a text, which is
then lost). The footer shows `? help · q quit` and the last notice.

Global keys:

| Key | Action |
|---|---|
| `?` | Toggle the help |
| `Esc` | Back / close the help |
| `q` | Quit |
| `Ctrl-C` | Quit |

### Repo list

Your repos (owner, collaborator, organization member) and your Pinned repos (pinned first). Markers: `[pinned]`,
`[fork]`, `[private]`, `[archived]`. Archived repos are hidden until you press `a`.

| Key | Action |
|---|---|
| `Enter` | Open the repo's PRs |
| `Up` / `k`, `Down` / `j` | Previous / next repo |
| `/` | Filter by name (`Enter` keeps the filter, `Esc` clears it) |
| `a` | Show / hide archived repos |
| `o` | Open `owner/name` (any repo you can read); it is pinned |
| `u` | Unpin the selected repo |
| `r` | Refresh |

Pinned repos are kept in `<config dir>/ghu/pinned.toml`: `~/.config/ghu/` (or `$XDG_CONFIG_HOME/ghu/`) on
Linux, `%APPDATA%\ghu\` on Windows, `~/Library/Application Support/ghu/` on macOS.

### Pull requests

| Key | Action |
|---|---|
| `Enter` | Open the PR |
| `Tab` | Open PRs ↔ closed and merged PRs |
| `r` | Refresh |
| `Up` / `k`, `Down` / `j` | Previous / next PR |

### PR detail

Title, state, branches, description (simple Markdown), current verdicts and the conversation. The line of actions
says what you may do: nothing on a closed or merged PR or an archived repo, Comment only on your own PR.

| Key | Action |
|---|---|
| `Up` / `k`, `Down` / `j` | Scroll |
| `PgUp`, `PgDn` | Page up / down |
| `r` | Refresh |
| `d` | Diff |
| `c` | Comment |
| `a` | Approve (text optional) |
| `x` | Request changes (text required) |

### Diff

Changed files on the left, the unified diff of the selected file on the right (no line wrap).

| Key | Action |
|---|---|
| `n`, `p` | Next / previous file |
| `Up` / `k`, `Down` / `j` | Scroll |
| `PgUp`, `PgDn` | Page up / down |
| `Left` / `h`, `Right` / `l` | Scroll left / right |
| `r` | Reload |

### Comment and Review

You type the text in the screen, then review what will be published, then confirm. Nothing is sent before the
confirmation. After a success, the PR detail comes back, reloaded.

While typing, every key is text (`q` and `?` too); arrows, `Home`, `End`, `Backspace`, `Delete` and
`Enter` (new line) edit the text, and pasting works.

| Key | Action |
|---|---|
| `Ctrl-S` | Review the text, then publish |
| `Ctrl-E` | Edit the text in `$VISUAL` / `$EDITOR` |
| `Esc` | Cancel (press twice if there is text) |

On the confirmation screen:

| Key | Action |
|---|---|
| `y` / `Enter` | Publish |
| `n` / `Esc` | Back to the text |

If GitHub gives no clear answer (network error, 5xx), the message says the text may have been published anyway:
press `Esc`, then `r` on the detail to check. A text that failed to publish is kept: open the Comment or the
Review again to get it back.

## External editor ($VISUAL / $EDITOR)

`Ctrl-E` writes the text to a temporary file `ghu-*.md`, hands the terminal to your editor and waits for it to
exit; the text of the file then replaces the text in `ghu`. The file is removed afterwards.

- Command: `VISUAL` if set, else `EDITOR`, else `vi` (`notepad` on Windows). It is split on spaces; double
  quotes group words; no shell is involved (no `~`, no `$VAR`, no pipes).
- The editor must **wait** until you close the file. Good values: `vim`, `nano`, `"code --wait"`; on Windows,
  `"code.cmd --wait"` (`code` alone is not found: only `.exe` is added to a bare name), or notepad.
- Exiting the editor with an error (`:cq` in vim) keeps the text as it was.
- An editor that returns within one second without changing the file gets the notice
  `The editor returned at once: the text is unchanged.` (for example Windows 11 Notepad handing the file to a
  window that was already open, or `code` without `--wait`).
- A missing program gives `Cannot open the editor: <program> was not found. Set VISUAL or EDITOR.`; a text over
  1 MiB or not in UTF-8 is refused, the text is kept.

Examples: `export VISUAL=nano` (Linux / WSL), `$env:VISUAL = 'code.cmd --wait'` (PowerShell).

## Other settings

| Variable | Effect |
|---|---|
| `GHU_LOG_FILE` | Append logs to this file (no logs otherwise; never on screen) |
| `GHU_LOG` | Log level or filter, `tracing` `EnvFilter` syntax (default `info`) |
| `GHU_API_URL` | API base URL (default `https://api.github.com`); meant for the test server `gh-fake` |

## Limits of version 0.1

- Comments on a line of the diff are not supported yet (planned for 0.2); Comment and Review apply to the whole PR.
- Pending reviews (started on the website) are ignored; one of yours on a PR makes GitHub refuse a new Review
  (422, explained on screen).
- No checks (CI), no commit list, no "my PRs" screen, no reply to discussion threads; no periodic refresh (`r`,
  and automatic after a publication).
- Text width: wide and combined characters (emoji ZWJ sequences, zero-width marks) are measured heuristically;
  the cursor may be off by a column on such text.
- An editor that returns after more than one second without waiting is not detected (the text stays unchanged,
  without a notice). If `ghu` is killed while a graphical editor is open, the temporary `ghu-*.md` file is left
  in the temporary folder.
- The message of a refused token (401) is worded differently from the other errors.
- macOS is built but has never been tried.

## Development

Workspace of three crates: `gh-client` (REST client), `ghu` (the TUI) and `gh-fake` (a fake GitHub server for
tests and manual tries, never shipped). Tests never call the real GitHub.

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Try it against the fake server: `cargo run -p gh-fake -- --port 8787` in one terminal, then
`GHU_API_URL=http://127.0.0.1:8787 GH_TOKEN=ghu-fake-token cargo run -p ghu` in another. Roadmap:
[docs/roadmap.md](docs/roadmap.md); decisions: [docs/adr/](docs/adr/).
