# ghu 0.1

First release of `ghu`, a terminal client for GitHub: browse your repositories and their pull requests, read
diffs and conversations, and review (comment, approve, request changes).

Authentication uses the token of [gh](https://cli.github.com/) (`GH_TOKEN`, `GITHUB_TOKEN` or `gh auth token`).
Nothing is stored by the application. Every action that publishes on GitHub (comment, review) asks for a
confirmation first.

## Assets

- `ghu-0.1.0-x86_64-unknown-linux-gnu.tar.gz`
- `ghu-0.1.0-x86_64-pc-windows-msvc.zip`
- `ghu-0.1.0-aarch64-apple-darwin.tar.gz`
- `SHA256SUMS`: check with `sha256sum --check SHA256SUMS`.

The final release publishes the very same archives as the release candidate that was tried by hand: nothing is
rebuilt between the two.

## Tested platforms

- Linux (WSL) and Windows: full manual trial on the release-candidate assets before the final release.
- macOS (Apple Silicon): **built by CI but never tried by hand.** Tests run in CI on macOS; expect rough edges.

## Known limits of 0.1

None is blocking.

- Text width uses heuristic grapheme clusters (ZWJ, zero width), not full Unicode segmentation: the cursor may be
  off with wide characters.
- The message of a 401 error does not start with "Cannot ..." like the other errors.
