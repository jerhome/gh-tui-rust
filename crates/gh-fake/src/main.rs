//! `gh-fake`: runs the fake GitHub with demo data, for trying `ghu` by hand.
//!
//! Usage: `gh-fake [--port <N>] [--latency-ms <N>]`. Listens on `127.0.0.1` only; prints the two lines to use
//! (`GHU_API_URL=http://127.0.0.1:<port>` and `GH_TOKEN=ghu-fake-token`) on stdout, logs one line per request on
//! stderr, and stops on Ctrl-C.

use std::time::Duration;

use clap::Parser;
use gh_fake::{FakeGitHub, Method, State};

#[derive(Parser)]
#[command(
    version,
    about = "Fake GitHub REST server for trying ghu by hand (127.0.0.1 only)"
)]
struct Args {
    /// Port to listen on (0 = chosen by the OS).
    #[arg(long, default_value_t = 0)]
    port: u16,
    /// Delay added to every request, in milliseconds.
    #[arg(long, default_value_t = 0)]
    latency_ms: u64,
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let args = Args::parse();
    let mut state = State::demo();
    state.latency = Duration::from_millis(args.latency_ms);
    let token = state.token.clone();
    let fake = FakeGitHub::start_on(args.port, state).await?;
    println!("GHU_API_URL={}", fake.base_url());
    println!("GH_TOKEN={token}");

    let mut seen = 0;
    let mut tick = tokio::time::interval(Duration::from_millis(100));
    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => return Ok(()),
            _ = tick.tick() => {
                for req in fake.requests().into_iter().skip(seen) {
                    let method = match req.method { Method::Get => "GET", Method::Post => "POST" };
                    let query = req.query.map(|q| format!("?{q}")).unwrap_or_default();
                    eprintln!("{method} {}{query}", req.path);
                    seen += 1;
                }
            }
        }
    }
}
