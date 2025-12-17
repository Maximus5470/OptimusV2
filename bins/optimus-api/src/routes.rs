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
        .route("/job/:job_id", get(handlers::get_job_result))
}
