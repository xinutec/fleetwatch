//! Boot-time config guards. These lock two security invariants that `Config::from_env`
//! enforces, tested through the crate's public validators (env-var-driven `from_env`
//! itself is racy + unsafe to exercise under edition 2024's `set_var`).

use fleetwatch::config::{
    MIN_READ_TOKEN_LEN, MIN_SESSION_SECRET_LEN, check_dev_login_safe, check_session_secret,
    parse_read_tokens,
};

#[test]
fn session_secret_rejects_short_keys() {
    assert!(check_session_secret("").is_err());
    assert!(check_session_secret("short").is_err());
    // One below the boundary fails; exactly the boundary passes.
    assert!(check_session_secret(&"x".repeat(MIN_SESSION_SECRET_LEN - 1)).is_err());
    assert!(check_session_secret(&"x".repeat(MIN_SESSION_SECRET_LEN)).is_ok());
    assert!(check_session_secret(&"x".repeat(64)).is_ok());
}

#[test]
fn dev_login_bypass_refused_only_with_static_serving() {
    // Dev: bypass on, no static bundle → allowed.
    assert!(check_dev_login_safe(Some("pippijn"), None).is_ok());
    // Prod: bypass on WHILE serving the bundle → refuse to boot.
    assert!(check_dev_login_safe(Some("pippijn"), Some("/srv/www")).is_err());
    // Prod with the bypass unset is fine; so is bare API-only dev.
    assert!(check_dev_login_safe(None, Some("/srv/www")).is_ok());
    assert!(check_dev_login_safe(None, None).is_ok());
}

#[test]
fn read_tokens_default_to_none() {
    // Unset/empty means "no unattended readers" — the safe default. An empty list can
    // never match a presented token, so /api/problems stays session-only until a token
    // is deliberately configured.
    assert!(parse_read_tokens("").expect("empty is valid").is_empty());
    assert!(
        parse_read_tokens("  ,  ")
            .expect("blanks skipped")
            .is_empty()
    );
}

#[test]
fn read_tokens_reject_weak_entries() {
    // A short token is a misconfiguration, not a choice: fail at boot, not at request
    // time when a guessable credential is already live.
    assert!(parse_read_tokens("short").is_err());
    assert!(parse_read_tokens(&"x".repeat(MIN_READ_TOKEN_LEN - 1)).is_err());
    assert!(parse_read_tokens(&"x".repeat(MIN_READ_TOKEN_LEN)).is_ok());
}

#[test]
fn read_tokens_parse_a_list() {
    let a = "a".repeat(MIN_READ_TOKEN_LEN);
    let b = "b".repeat(MIN_READ_TOKEN_LEN);
    let parsed = parse_read_tokens(&format!(" {a} , {b} ")).expect("valid");
    assert_eq!(parsed, vec![a, b]);
}

#[test]
fn one_weak_token_fails_the_whole_list() {
    // Not "skip the bad one and carry on": a half-applied secret is how a weak
    // credential survives unnoticed.
    let good = "g".repeat(MIN_READ_TOKEN_LEN);
    assert!(parse_read_tokens(&format!("{good},oops")).is_err());
}
