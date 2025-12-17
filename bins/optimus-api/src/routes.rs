// Route definitions for the Optimus API

use axum::{
    routing::{get, post},
    Router,
};
use std::sync::Arc;

use crate::{handlers, AppState};

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/execute", post(handlers::submit_job))
        .route("/health", get(handlers::health_check))
        .route("/ready", get(handlers::readiness_check))
        .route("/metrics", get(handlers::metrics_handler))
        .route("/job/:job_id", get(handlers::get_job_result))
        .route("/job/:job_id/debug", get(handlers::get_job_debug))
        .route("/job/:job_id/cancel", post(handlers::cancel_job))
}
