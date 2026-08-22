//! Liveness and readiness — two questions, deliberately not the same one.
//!
//! Both k8s probes used to point at `/healthz`, which returned the literal
//! `"ok"`. That is the right answer for liveness and the wrong one for
//! readiness: it proves the process is listening and nothing else, so a pod that
//! could not reach its database — or had exhausted its pool, which is what
//! #1053 was — reported Ready and kept taking traffic it answered with 500s. A
//! probe that cannot fail does not just miss the fault; it asserts the opposite
//! of it.
//!
//! ⚠ **Liveness stays shallow, and that is not an oversight.** Liveness failing
//! kills the container. If it depended on the database, a database outage would
//! restart the app in a loop for the duration — the restart cannot fix a
//! dependency, and it throws away the one thing still working. "I cannot serve"
//! belongs in readiness, which withdraws the pod from the Service and puts it
//! back by itself when the answer changes.

use std::time::Duration;

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;

use crate::state::AppState;

/// How long readiness will wait for the database before calling it unready.
///
/// This is a budget, not a latency target. It has to be comfortably under the
/// probe's own `timeoutSeconds` so a failure arrives as a 503 we logged rather
/// than as a probe timeout, which says only that nothing answered and names no
/// cause. It also has to be well ABOVE a normal `SELECT 1` (sub-millisecond
/// here) so ordinary load never trips it: readiness flapping on a slow query
/// turns a slow dashboard into no dashboard, which is worse than the fault.
const READY_BUDGET: Duration = Duration::from_secs(3);

/// Liveness: the process is running and its executor is answering.
pub async fn healthz() -> &'static str {
    "ok"
}

/// Readiness: this pod can reach the database, so a request routed here can be
/// served.
///
/// `SELECT 1` through the shared pool, which is the whole point — it asks for a
/// pooled connection the same way a real handler does, so an exhausted pool
/// reads as unready even while MariaDB itself is perfectly healthy.
pub async fn readyz(State(state): State<AppState>) -> impl IntoResponse {
    let ping = sqlx::query("SELECT 1").execute(&state.pool);
    match tokio::time::timeout(READY_BUDGET, ping).await {
        Ok(Ok(_)) => (StatusCode::OK, "ready"),
        Ok(Err(e)) => {
            tracing::warn!(error = %e, "readiness: database unreachable");
            (StatusCode::SERVICE_UNAVAILABLE, "database unreachable")
        }
        Err(_) => {
            tracing::warn!(
                budget_s = READY_BUDGET.as_secs(),
                "readiness: database did not answer within the budget"
            );
            (StatusCode::SERVICE_UNAVAILABLE, "database did not answer")
        }
    }
}
