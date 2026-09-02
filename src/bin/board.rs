//! `board` — read fleetwatch's problems from a terminal (#1312).
//!
//! The read half of the auth split: `GET /api/problems` with the read token,
//! which grants exactly this endpoint and nothing else (`src/auth.rs`). The
//! token comes from `FLEETWATCH_READ_TOKEN`, or on macOS the Keychain item
//! `fleetwatch-read-token` — the same out-of-band placement the ingest token
//! uses, so no credential lives in a repo.
//!
//! Exit codes are the interface for scripts: 0 nothing notifiable, 1 the phone
//! would have notified, 2 the board could not be read. "Could not be read" is
//! its own state on purpose — a fetch failure must never render as a clean
//! board.

use std::process::ExitCode;

use fleetwatch::board;
use fleetwatch::report::types::Problems;

const KEYCHAIN_ITEM: &str = "fleetwatch-read-token";

fn usage() -> String {
    format!(
        "usage: {bin} [--json]\n\
         \n\
         Prints fleetwatch's current problems (GET /api/problems).\n\
         --json prints the service's response verbatim instead.\n\
         \n\
         Auth: $FLEETWATCH_READ_TOKEN, else Keychain item `{KEYCHAIN_ITEM}`.\n\
         URL:  $FLEETWATCH_BASE_URL, else https://fleetwatch.xinutec.org\n\
         Exit: 0 nothing notifiable, 1 notifiable problems, 2 read failed.\n",
        bin = env!("CARGO_BIN_NAME"),
    )
}

fn token() -> Result<String, String> {
    if let Ok(t) = std::env::var("FLEETWATCH_READ_TOKEN")
        && !t.trim().is_empty()
    {
        return Ok(t.trim().to_string());
    }
    let out = std::process::Command::new("security")
        .args(["find-generic-password", "-s", KEYCHAIN_ITEM, "-w"])
        .output()
        .map_err(|e| format!("no $FLEETWATCH_READ_TOKEN and `security` failed to run: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "no $FLEETWATCH_READ_TOKEN and no Keychain item `{KEYCHAIN_ITEM}` — \
             add one with: security add-generic-password -s {KEYCHAIN_ITEM} -a fleetwatch -w"
        ));
    }
    let t = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if t.is_empty() {
        return Err(format!("Keychain item `{KEYCHAIN_ITEM}` is empty"));
    }
    Ok(t)
}

async fn fetch(json: bool) -> Result<ExitCode, String> {
    let base = std::env::var("FLEETWATCH_BASE_URL")
        .unwrap_or_else(|_| "https://fleetwatch.xinutec.org".to_string());
    let url = format!("{}/api/problems", base.trim_end_matches('/'));
    let resp = reqwest::Client::new()
        .get(&url)
        .bearer_auth(token()?)
        .send()
        .await
        .map_err(|e| format!("GET {url}: {e}"))?;
    let status = resp.status();
    let body = resp
        .text()
        .await
        .map_err(|e| format!("GET {url}: reading body: {e}"))?;
    if status == reqwest::StatusCode::UNAUTHORIZED {
        return Err(format!(
            "{url} answered 401 — the token was rejected. It is the READ token \
             (Keychain `{KEYCHAIN_ITEM}`), not the ingest one; if it was rotated, \
             update the Keychain item."
        ));
    }
    if !status.is_success() {
        return Err(format!("{url} answered {status}: {}", body.trim()));
    }
    if json {
        // The service's JSON, reprinted — not rebuilt here.
        println!("{}", body.trim_end());
    }
    let problems: Problems = serde_json::from_str(&body)
        .map_err(|e| format!("{url}: response did not parse as Problems: {e}"))?;
    if !json {
        print!("{}", board::render(&problems));
    }
    Ok(if board::notifiable_count(&problems) == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    })
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let json = match args.iter().map(String::as_str).collect::<Vec<_>>()[..] {
        [] => false,
        ["--json"] => true,
        ["--help" | "-h"] => {
            print!("{}", usage());
            return ExitCode::SUCCESS;
        }
        _ => {
            eprint!("{}", usage());
            return ExitCode::from(2);
        }
    };
    match fetch(json).await {
        Ok(code) => code,
        Err(e) => {
            eprintln!("{}: {e}", env!("CARGO_BIN_NAME"));
            ExitCode::from(2)
        }
    }
}
