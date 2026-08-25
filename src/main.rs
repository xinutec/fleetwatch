//! fleetwatch — fleet monitoring platform backend. Entry point: load config, connect
//! the DB, run migrations, start the retention sweeper, serve. All logic lives
//! in the `fleetwatch` library crate.

use anyhow::Result;
use fleetwatch::{
    config::Config, db, report::repo, report::retention, routes, selfcheck, session,
    state::AppState,
};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let cfg = Config::from_env()?;
    let pool = db::connect(&cfg.database_url).await?;
    // Timed because it was invisible: migration 0006 took 47s against a stated
    // 0.54s and only this pod's log said so (#1069). `selfcheck` reports it.
    let migrate_started = std::time::Instant::now();
    db::migrate(&pool).await?;
    let migrate_ms = migrate_started.elapsed().as_millis() as u64;

    // Reconcile the latest-report pointer with the history it summarises. Cheap
    // (one pass over `report`), and it closes the one window this deployment
    // shape leaves open: during a rolling update the OLD pod keeps accepting
    // reports after the new one has migrated, and it does not know to maintain
    // the pointer. A report landing in those seconds would leave its producer's
    // tile pointing at the previous run — indistinguishable, to the staleness
    // rules, from a producer that had gone quiet.
    let reconcile_started = std::time::Instant::now();
    let reconcile_result = repo::rebuild_latest_report(&pool).await;
    let reconcile_ms = reconcile_started.elapsed().as_millis() as u64;
    match reconcile_result {
        Ok(n) => tracing::info!("latest_report: reconciled, {n} row(s) written"),
        // Not fatal: the pointer is maintained on ingest, so a failure here
        // costs freshness of the reconciliation, not the service.
        Err(e) => tracing::warn!("latest_report reconcile failed: {e:#}"),
    }

    // Daily retention sweep (the first tick fires immediately, so boot also
    // sweeps). Prunes raw payloads early and old checks; report summaries stay.
    let sweep_pool = pool.clone();
    let (raw_days, check_days) = (cfg.raw_retention_days, cfg.check_retention_days);
    // Shared with `selfcheck`, which reports whether a sweep has run at all —
    // the outcome used to go to stdout and nowhere durable, so a sweeper that
    // had stopped was invisible until the disk filled.
    let last_sweep: selfcheck::SweepState = std::sync::Arc::new(tokio::sync::RwLock::new(None));
    let sweep_state = last_sweep.clone();
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(std::time::Duration::from_secs(24 * 3600));
        loop {
            tick.tick().await;
            match retention::sweep(&sweep_pool, raw_days, check_days).await {
                Ok((raw, checks)) => {
                    if raw > 0 || checks > 0 {
                        tracing::info!(
                            "retention: cleared {raw} raw payload(s), {checks} old check(s)"
                        );
                    }
                    *sweep_state.write().await = Some(selfcheck::Sweep {
                        at: chrono::Utc::now(),
                        raw_cleared: raw,
                        checks_deleted: checks,
                    });
                }
                // Deliberately does NOT stamp on failure: the stamp means "a
                // sweep completed", and moving it on a failed one would make a
                // sweeper that errors every night look healthy.
                Err(e) => tracing::warn!("retention sweep failed: {e:#}"),
            }
        }
    });

    // Hourly sweep of expired login sessions (lazily-expired otherwise, so
    // abandoned sessions would accumulate). Auth artifacts, not user data.
    let session_pool = pool.clone();
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(std::time::Duration::from_secs(3600));
        loop {
            tick.tick().await;
            match session::sweep_expired(&session_pool).await {
                Ok(n) if n > 0 => tracing::info!("sessions: swept {n} expired"),
                Ok(_) => {}
                Err(e) => tracing::warn!("session sweep failed: {e:#}"),
            }
        }
    });

    // fleetwatch as a producer of its own internals (#1069). Its blind spot is
    // inherent — a monitor reporting on itself says nothing when it is dead —
    // and is covered from outside by the Mac's fleet_health probe.
    let self_pool = pool.clone();
    let boot = selfcheck::Boot {
        migrate_ms,
        reconcile_ms,
    };
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(selfcheck::INTERVAL);
        loop {
            tick.tick().await;
            let sweep = *last_sweep.read().await;
            if let Err(e) = selfcheck::run_once(&self_pool, boot, sweep).await {
                tracing::warn!("selfcheck failed: {e:#}");
            }
        }
    });

    let http = reqwest::Client::new();
    let bind_addr = cfg.bind_addr.clone();
    let app = routes::router(AppState::new(pool, cfg, http));

    let listener = tokio::net::TcpListener::bind(&bind_addr).await?;
    tracing::info!("fleetwatch listening on {bind_addr}");
    axum::serve(listener, app).await?;
    Ok(())
}
