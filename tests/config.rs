//! Boot-time config guards. These lock two security invariants that `Config::from_env`
//! enforces, tested through the crate's public validators (env-var-driven `from_env`
//! itself is racy + unsafe to exercise under edition 2024's `set_var`).

use fleetwatch::config::{MIN_SESSION_SECRET_LEN, check_dev_login_safe, check_session_secret};

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
