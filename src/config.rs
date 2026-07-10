//! Runtime configuration, read from the environment at startup.
//!
//! The ingest tokens come from the environment (a k8s secret in deployment, a
//! `.env`-style shell locally). Nothing here is hard-coded.

use anyhow::{Context, Result};

#[derive(Clone, Debug)]
pub struct Config {
    /// MariaDB connection string, e.g. `mysql://fleetwatch:pw@host/fleetwatch`.
    pub database_url: String,
    /// Address to bind the HTTP server to.
    pub bind_addr: String,

    /// Directory of the built Angular bundle to serve (with SPA fallback). When
    /// unset the server is API-only — e.g. in dev, where `ng serve` proxies.
    pub static_dir: Option<String>,

    /// Ingest credentials: `(source, token)` pairs. A producer authenticates a
    /// POST /api/reports with `Authorization: Bearer <token>`; the matching
    /// `source` is stamped server-side (see `auth`), so a producer can only ever
    /// write as itself and never spoof another machine's status.
    pub tokens: Vec<(String, String)>,

    /// Raw report payloads are pruned after this many days (kept for
    /// schema-evolution replays + debugging). The derived `check` rows and the
    /// report summaries live far longer — see `report::retention`.
    pub raw_retention_days: i64,
    /// Per-check rows are pruned after this many days (a year of trends + margin).
    pub check_retention_days: i64,

    // --- Human auth (Nextcloud identity → cookie session). Distinct from the
    // ingest tokens above: producers write with bearer tokens, humans read the
    // dashboard behind an NC login. ---
    /// HMAC key for signing session cookies.
    pub session_secret: String,
    /// Nextcloud base URL, e.g. `https://dash.xinutec.org` (no trailing slash).
    pub nc_base_url: String,
    /// OAuth2 client id + secret registered in Nextcloud for this app.
    pub nc_client_id: String,
    pub nc_client_secret: String,
    /// Must match the redirect URI registered for the OAuth2 client.
    pub nc_redirect_uri: String,
    /// DEV ONLY. When set, `/dev-login` mints a session for this user id with no
    /// Nextcloud round-trip. Unset in production, so the route stays unmounted.
    pub dev_login_user: Option<String>,
}

fn env(key: &str) -> Result<String> {
    std::env::var(key).with_context(|| format!("missing required env var {key}"))
}

fn env_or(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}

fn env_i64(key: &str, default: i64) -> Result<i64> {
    match std::env::var(key) {
        Ok(v) => v
            .parse()
            .with_context(|| format!("{key} must be an integer, got {v:?}")),
        Err(_) => Ok(default),
    }
}

/// Parse `FLEETWATCH_TOKENS` — a comma-separated list of `source:token` pairs, e.g.
/// `mac-mini:abc123,odin:def456`. Whitespace around entries is trimmed; empty
/// entries are ignored. A malformed entry (no colon, empty source/token) is a
/// hard error so a broken secret fails at boot, not silently at request time.
pub fn parse_tokens(raw: &str) -> Result<Vec<(String, String)>> {
    let mut out = Vec::new();
    for entry in raw.split(',') {
        let entry = entry.trim();
        if entry.is_empty() {
            continue;
        }
        let (source, token) = entry
            .split_once(':')
            .with_context(|| format!("FLEETWATCH_TOKENS entry {entry:?} is not source:token"))?;
        let source = source.trim();
        let token = token.trim();
        if source.is_empty() || token.is_empty() {
            anyhow::bail!("FLEETWATCH_TOKENS entry {entry:?} has an empty source or token");
        }
        out.push((source.to_string(), token.to_string()));
    }
    Ok(out)
}

/// Minimum accepted `SESSION_SECRET` length. 32 chars ≈ 256 bits of key material.
pub const MIN_SESSION_SECRET_LEN: usize = 32;

/// The session secret is the HMAC key over the cookie. The id it signs is already
/// a 256-bit random DB-backed token, so a strong secret is defence-in-depth — but
/// there is no reason to accept a trivially weak key. Reject at boot.
pub fn check_session_secret(secret: &str) -> Result<()> {
    if secret.len() < MIN_SESSION_SECRET_LEN {
        anyhow::bail!(
            "SESSION_SECRET is too short ({} chars) — use at least {MIN_SESSION_SECRET_LEN} \
             (e.g. `openssl rand -hex 32`)",
            secret.len()
        );
    }
    Ok(())
}

/// Fail closed on the dev auth bypass. `/dev-login` mints a session with no
/// Nextcloud check, so it must never be reachable in a real deployment. Production
/// is the config that serves the static bundle (STATIC_DIR set); dev is API-only
/// with `ng serve` proxying /api. If the bypass is enabled alongside static
/// serving, that's a misconfiguration we refuse to boot on — a crashloop instead
/// of an open door.
pub fn check_dev_login_safe(dev_login_user: Option<&str>, static_dir: Option<&str>) -> Result<()> {
    if dev_login_user.is_some() && static_dir.is_some() {
        anyhow::bail!(
            "DEV_LOGIN_USER is set while serving a static bundle (STATIC_DIR) — the \
             /dev-login auth bypass must never be enabled in a production deployment"
        );
    }
    Ok(())
}

impl Config {
    pub fn from_env() -> Result<Self> {
        let tokens = parse_tokens(&env("FLEETWATCH_TOKENS")?)?;
        if tokens.is_empty() {
            anyhow::bail!("FLEETWATCH_TOKENS is empty — no producer could ever authenticate");
        }
        // Fail fast at boot if the NC base URL is malformed, rather than
        // panicking inside the /login handler at first request.
        let nc_base_url = env("NC_BASE_URL")?.trim_end_matches('/').to_string();
        let parsed = url::Url::parse(&nc_base_url)
            .with_context(|| format!("NC_BASE_URL is not a valid URL: {nc_base_url:?}"))?;
        if !matches!(parsed.scheme(), "http" | "https") || parsed.host().is_none() {
            anyhow::bail!("NC_BASE_URL must be an http(s) URL with a host: {nc_base_url:?}");
        }

        let session_secret = env("SESSION_SECRET")?;
        check_session_secret(&session_secret)?;

        let static_dir = std::env::var("STATIC_DIR").ok();
        let dev_login_user = std::env::var("DEV_LOGIN_USER").ok();
        check_dev_login_safe(dev_login_user.as_deref(), static_dir.as_deref())?;

        Ok(Self {
            database_url: env("DATABASE_URL")?,
            bind_addr: env_or("BIND_ADDR", "0.0.0.0:8080"),
            static_dir,
            tokens,
            raw_retention_days: env_i64("FLEETWATCH_RAW_RETENTION_DAYS", 30)?,
            check_retention_days: env_i64("FLEETWATCH_CHECK_RETENTION_DAYS", 400)?,
            session_secret,
            nc_base_url,
            nc_client_id: env("NC_CLIENT_ID")?,
            nc_client_secret: env("NC_CLIENT_SECRET")?,
            nc_redirect_uri: env("NC_REDIRECT_URI")?,
            dev_login_user,
        })
    }
}
