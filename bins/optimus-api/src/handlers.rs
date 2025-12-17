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
use crate::metrics;

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

// Safety limits
const MAX_TEST_CASES: usize = 100;
const MAX_SOURCE_CODE_SIZE: usize = 100_000; // 100KB
const MAX_INPUT_SIZE: usize = 10_000; // 10KB per test case input
const MAX_EXPECTED_OUTPUT_SIZE: usize = 10_000; // 10KB per expected output

#[derive(Debug, Serialize)]
pub struct ErrorResponse {
    pub error: String,
    pub details: Option<String>,
}

/// POST /execute - Submit a job for execution
pub async fn submit_job(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<SubmitRequest>,
) -> impl IntoResponse {
    // Generate job ID
    let job_id = Uuid::new_v4();
    
    // Safety checks - validate request before queueing
    
    // 1. Check test case count
    if payload.test_cases.is_empty() {
        metrics::record_job_rejected("no_test_cases");
        error!(job_id = %job_id, "Rejected: No test cases provided");
        return (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "Invalid request".to_string(),
                details: Some("At least one test case is required".to_string()),
            }),
        ).into_response();
    }
    
    if payload.test_cases.len() > MAX_TEST_CASES {
        metrics::record_job_rejected("too_many_test_cases");
        error!(
            job_id = %job_id,
            test_cases = payload.test_cases.len(),
            limit = MAX_TEST_CASES,
            "Rejected: Too many test cases"
        );
        return (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "Too many test cases".to_string(),
                details: Some(format!(
                    "Maximum {} test cases allowed, got {}",
                    MAX_TEST_CASES,
                    payload.test_cases.len()
                )),
            }),
        ).into_response();
    }
    
    // 2. Check source code size
    if payload.source_code.len() > MAX_SOURCE_CODE_SIZE {
        metrics::record_job_rejected("source_code_too_large");
        error!(
            job_id = %job_id,
            size = payload.source_code.len(),
            limit = MAX_SOURCE_CODE_SIZE,
            "Rejected: Source code too large"
        );
        return (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "Source code too large".to_string(),
                details: Some(format!(
                    "Maximum {} bytes allowed, got {} bytes",
                    MAX_SOURCE_CODE_SIZE,
                    payload.source_code.len()
                )),
            }),
        ).into_response();
    }
    
    // 3. Validate source code is not empty
    if payload.source_code.trim().is_empty() {
        metrics::record_job_rejected("empty_source_code");
        error!(job_id = %job_id, "Rejected: Empty source code");
        return (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "Invalid request".to_string(),
                details: Some("Source code cannot be empty".to_string()),
            }),
        ).into_response();
    }
    
    // 4. Check test case input/output sizes
    for (idx, tc) in payload.test_cases.iter().enumerate() {
        if tc.input.len() > MAX_INPUT_SIZE {
            metrics::record_job_rejected("test_case_input_too_large");
            error!(
                job_id = %job_id,
                test_case = idx + 1,
                size = tc.input.len(),
                limit = MAX_INPUT_SIZE,
                "Rejected: Test case input too large"
            );
            return (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    error: "Test case input too large".to_string(),
                    details: Some(format!(
                        "Test case {} input exceeds {} bytes",
                        idx + 1,
                        MAX_INPUT_SIZE
                    )),
                }),
            ).into_response();
        }
        
        if tc.expected_output.len() > MAX_EXPECTED_OUTPUT_SIZE {
            metrics::record_job_rejected("test_case_output_too_large");
            error!(
                job_id = %job_id,
                test_case = idx + 1,
                size = tc.expected_output.len(),
                limit = MAX_EXPECTED_OUTPUT_SIZE,
                "Rejected: Test case expected output too large"
            );
            return (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    error: "Test case expected output too large".to_string(),
                    details: Some(format!(
                        "Test case {} expected output exceeds {} bytes",
                        idx + 1,
                        MAX_EXPECTED_OUTPUT_SIZE
                    )),
                }),
            ).into_response();
        }
    }
    
    // 5. Validate timeout
    if payload.timeout_ms == 0 || payload.timeout_ms > 60_000 {
        metrics::record_job_rejected("invalid_timeout");
        error!(
            job_id = %job_id,
            timeout_ms = payload.timeout_ms,
            "Rejected: Invalid timeout"
        );
        return (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "Invalid timeout".to_string(),
                details: Some("Timeout must be between 1ms and 60000ms".to_string()),
            }),
        ).into_response();
    }

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
        metadata: optimus_common::types::JobMetadata::default(),
    };

    // Push to Redis queue
    let mut conn = state.redis.clone();
    match redis::push_job(&mut conn, &job).await {
        Ok(_) => {
            // Record metrics
            metrics::record_job_submitted(&job.language.to_string());
            
            info!(
                job_id = %job_id,
                language = %job.language,
                test_cases = job.test_cases.len(),
                phase = "queued",
                "Job queued"
            );
            
            (
                StatusCode::CREATED,
                Json(SubmitResponse {
                    job_id: job_id.to_string(),
                }),
            ).into_response()
        }
        Err(e) => {
            error!(job_id = %job_id, error = %e, "Failed to queue job");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: "Failed to queue job".to_string(),
                    details: Some(e.to_string()),
                }),
            ).into_response()
        }
    }
}

#[derive(Debug, Serialize)]
pub struct HealthResponse {
    pub status: String,
    pub uptime_seconds: u64,
    pub redis_connected: bool,
    pub timestamp: String,
}

/// GET /metrics - Prometheus metrics endpoint
pub async fn metrics_handler(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    // Update queue depth metrics before rendering
    let mut conn = state.redis.clone();
    metrics::update_queue_depths(&mut conn).await;
    
    let metrics_text = metrics::render_metrics();
    (
        StatusCode::OK,
        [("content-type", "text/plain; version=0.0.4")],
        metrics_text,
    )
}

/// GET /health - Enhanced health check endpoint
pub async fn health_check(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let uptime = state.start_time.elapsed().as_secs();
    
    // Test Redis connectivity with PING
    let redis_ok = match ::redis::cmd("PING")
        .query_async::<_, String>(&mut state.redis.clone())
        .await
    {
        Ok(_) => true,
        Err(e) => {
            error!(error = %e, "Redis health check failed");
            false
        }
    };

    let response = HealthResponse {
        status: if redis_ok { "healthy".to_string() } else { "degraded".to_string() },
        uptime_seconds: uptime,
        redis_connected: redis_ok,
        timestamp: chrono::Utc::now().to_rfc3339(),
    };

    if redis_ok {
        (StatusCode::OK, Json(response))
    } else {
        (StatusCode::SERVICE_UNAVAILABLE, Json(response))
    }
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

#[derive(Debug, Serialize)]
pub struct JobDebugInfo {
    pub job_id: String,
    pub status: String,
    pub attempts: u8,
    pub max_attempts: u8,
    pub last_failure_reason: Option<String>,
    pub in_main_queue: bool,
    pub in_retry_queue: bool,
    pub in_dlq: bool,
    pub result: Option<optimus_common::types::ExecutionResult>,
}

/// GET /job/{job_id}/debug - Detailed debugging information for job
/// Shows retry attempts, queue status, and failure reasons
pub async fn get_job_debug(
    State(state): State<Arc<AppState>>,
    Path(job_id): Path<String>,
) -> impl IntoResponse {
    use ::redis::AsyncCommands;
    
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

    let mut conn = state.redis.clone();
    
    // Fetch result from Redis
    let result = match redis::get_result(&mut conn, &job_uuid).await {
        Ok(result) => result,
        Err(e) => {
            error!(job_id = %job_id, error = %e, "Failed to fetch job result");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "error": format!("Failed to query job: {}", e)
                })),
            ).into_response();
        }
    };
    
    // Check all queues for this job (search all languages)
    let mut in_main_queue = false;
    let mut in_retry_queue = false;
    let mut in_dlq = false;
    let mut job_metadata = None;
    
    for language in Language::all_variants() {
        let lang = language.to_string();
        // Check main queue
        let main_queue = format!("optimus:queue:{}", lang);
        if let Ok(items) = conn.lrange::<_, Vec<String>>(&main_queue, 0, -1).await {
            for item in items {
                if let Ok(job) = serde_json::from_str::<optimus_common::types::JobRequest>(&item) {
                    if job.id == job_uuid {
                        in_main_queue = true;
                        job_metadata = Some(job.metadata);
                        break;
                    }
                }
            }
        }
        
        // Check retry queue
        let retry_queue = format!("optimus:queue:{}:retry", lang);
        if let Ok(items) = conn.lrange::<_, Vec<String>>(&retry_queue, 0, -1).await {
            for item in items {
                if let Ok(job) = serde_json::from_str::<optimus_common::types::JobRequest>(&item) {
                    if job.id == job_uuid {
                        in_retry_queue = true;
                        job_metadata = Some(job.metadata);
                        break;
                    }
                }
            }
        }
        
        // Check DLQ
        let dlq = format!("optimus:queue:{}:dlq", lang);
        if let Ok(items) = conn.lrange::<_, Vec<String>>(&dlq, 0, -1).await {
            for item in items {
                if let Ok(job) = serde_json::from_str::<optimus_common::types::JobRequest>(&item) {
                    if job.id == job_uuid {
                        in_dlq = true;
                        job_metadata = Some(job.metadata);
                        break;
                    }
                }
            }
        }
    }
    
    let debug_info = JobDebugInfo {
        job_id: job_id.clone(),
        status: if result.is_some() {
            "completed".to_string()
        } else if in_dlq {
            "dead_letter_queue".to_string()
        } else if in_retry_queue {
            "retrying".to_string()
        } else if in_main_queue {
            "queued".to_string()
        } else {
            "unknown".to_string()
        },
        attempts: job_metadata.as_ref().map(|m| m.attempts).unwrap_or(0),
        max_attempts: job_metadata.as_ref().map(|m| m.max_attempts).unwrap_or(3),
        last_failure_reason: job_metadata.and_then(|m| m.last_failure_reason),
        in_main_queue,
        in_retry_queue,
        in_dlq,
        result,
    };
    
    info!(job_id = %job_id, status = %debug_info.status, "Debug info retrieved");
    (StatusCode::OK, Json(debug_info)).into_response()
}
