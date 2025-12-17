// HTTP route handlers for the Optimus API

use axum::{
    extract::{State, Path},
    http::StatusCode,
    response::{IntoResponse, Json},
};
use optimus_common::types::{JobRequest, Language};
use optimus_common::redis;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;
use tracing::{info, error};

use crate::AppState;

#[derive(Debug, Deserialize)]
pub struct SubmitRequest {
    pub language: Language,
    pub source_code: String,
    pub test_cases: Vec<TestCaseInput>,
    #[serde(default = "default_timeout")]
    pub timeout_ms: u64,
}

#[derive(Debug, Deserialize)]
pub struct TestCaseInput {
    pub input: String,
    pub expected_output: String,
    #[serde(default = "default_weight")]
    pub weight: u32,
}

fn default_timeout() -> u64 {
    5000
}

fn default_weight() -> u32 {
    10
}

#[derive(Debug, Serialize)]
pub struct SubmitResponse {
    pub job_id: String,
}

/// POST /execute - Submit a job for execution
pub async fn submit_job(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<SubmitRequest>,
) -> impl IntoResponse {
    // Generate job ID
    let job_id = Uuid::new_v4();

    // Convert test case inputs to internal format
    let test_cases: Vec<optimus_common::types::TestCase> = payload
        .test_cases
        .into_iter()
        .enumerate()
        .map(|(idx, tc)| optimus_common::types::TestCase {
            id: (idx + 1) as u32,
            input: tc.input,
            expected_output: tc.expected_output,
            weight: tc.weight,
        })
        .collect();

    // Create job request
    let job = JobRequest {
        id: job_id,
        language: payload.language,
        source_code: payload.source_code,
        test_cases,
        timeout_ms: payload.timeout_ms,
    };

    // Push to Redis queue
    let mut conn = state.redis.clone();
    match redis::push_job(&mut conn, &job).await {
        Ok(_) => {
            info!(
                job_id = %job_id,
                language = %job.language,
                test_cases = job.test_cases.len(),
                "Job queued"
            );
            
            (
                StatusCode::CREATED,
                Json(SubmitResponse {
                    job_id: job_id.to_string(),
                }),
            )
        }
        Err(e) => {
            error!(job_id = %job_id, error = %e, "Failed to queue job");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(SubmitResponse {
                    job_id: format!("error: {}", e),
                }),
            )
        }
    }
}

/// GET /status - Health check endpoint
pub async fn health_check() -> impl IntoResponse {
    (StatusCode::OK, "OK")
}

/// GET /job/{job_id} - Query execution result
pub async fn get_job_result(
    State(state): State<Arc<AppState>>,
    Path(job_id): Path<String>,
) -> impl IntoResponse {
    // Parse job ID
    let job_uuid = match Uuid::parse_str(&job_id) {
        Ok(id) => id,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "error": "Invalid job ID format"
                })),
            ).into_response();
        }
    };

    // Fetch result from Redis
    let mut conn = state.redis.clone();
    match redis::get_result(&mut conn, &job_uuid).await {
        Ok(Some(result)) => {
            info!(job_id = %job_id, status = ?result.overall_status, "Job result retrieved");
            // Result exists - return it
            (StatusCode::OK, Json(result)).into_response()
        }
        Ok(None) => {
            info!(job_id = %job_id, "Job still pending");
            // Result not found - job may still be queued/running
            (
                StatusCode::ACCEPTED,
                Json(serde_json::json!({
                    "job_id": job_id,
                    "status": "pending",
                    "message": "Job is queued or still executing"
                })),
            ).into_response()
        }
        Err(e) => {
            error!(job_id = %job_id, error = %e, "Failed to fetch job result");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "error": format!("Failed to query job status: {}", e)
                })),
            ).into_response()
        }
    }
}
