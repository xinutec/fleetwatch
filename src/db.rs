//! MariaDB connection pool + migration runner.

use anyhow::{Context, Result};
use sqlx::MySqlPool;
use sqlx::mysql::MySqlPoolOptions;

/// The pool ceiling. Named rather than inline because `selfcheck` reports how
/// close the pool is to it, and a denominator that lives in one place cannot
/// disagree with the number it is measured against.
pub const MAX_CONNECTIONS: u32 = 8;

pub async fn connect(database_url: &str) -> Result<MySqlPool> {
    let pool = MySqlPoolOptions::new()
        .max_connections(MAX_CONNECTIONS)
        .connect(database_url)
        .await
        .context("connecting to MariaDB")?;
    Ok(pool)
}

/// Apply embedded migrations from `migrations/`. Idempotent; safe on every boot.
pub async fn migrate(pool: &MySqlPool) -> Result<()> {
    sqlx::migrate!()
        .run(pool)
        .await
        .context("running migrations")?;
    Ok(())
}
