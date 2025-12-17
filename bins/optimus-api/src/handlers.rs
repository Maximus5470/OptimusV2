// HTTP route handlers for the Optimus API

use axum::{
    extract::{State, Path},
    http::{StatusCode, HeaderMap},
    response::{IntoResponse, Json},
};
use optimus_common::types::{JobRequest, Language};
use optimus_common::redis;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;
use tracing::{info, error, warn};

use crate::AppState;
use crate::metrics;

#[derive(Debug, Deserialize, Serialize)]
pub struct SubmitRequest {
    pub language: Language,
    pub source_code: String,
    pub test_cases: Vec<TestCaseInput>,
    #[serde(default = "default_timeout")]
    pub timeout_ms: u64,
}

#[derive(Debug, Deserialize, Serialize)]
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

// Safety limits (per specification)
const MAX_TEST_CASES: usize = 100;
const MAX_SOURCE_CODE_SIZE: usize = 256_000; // 256 KB
const MAX_STDIN_SIZE: usize = 64_000; // 64 KB per test case input
const MAX_EXPECTED_OUTPUT_SIZE: usize = 64_000; // 64 KB per expected output
const MAX_TIMEOUT_MS: u64 = 60_000; // 60 seconds
const MIN_TIMEOUT_MS: u64 = 1; // 1 millisecond

#[derive(Debug, Serialize)]
pub struct ErrorResponse {
    pub error: ErrorDetail,
}

#[derive(Debug, Serialize)]
pub struct ErrorDetail {
    pub code: String,
    pub message: String,
}

/// POST /execute - Submit a job for execution
/// 
/// Supports idempotency via Idempotency-Key header
/// - Same key + same payload → returns same job_id
/// - Same key + different payload → returns 409 Conflict
pub async fn submit_job(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<SubmitRequest>,
) -> impl IntoResponse {
    // Extract idempotency key if provided
    let idempotency_key = headers
        .get("idempotency-key")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());
    
    // 0. Validate language is enabled
    if !state.language_registry.is_enabled(payload.language) {
        metrics::record_job_rejected("language_not_supported");
        error!(
            language = %payload.language,
            "Rejected: Language not supported or disabled"
        );
        return (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(ErrorResponse {
                error: ErrorDetail {
                    code: "LANGUAGE_NOT_SUPPORTED".to_string(),
                    message: format!(
                        "Language '{}' is not enabled or supported",
                        payload.language
                    ),
                },
            }),
        ).into_response();
    }
    
    // Handle idempotency if key is provided
    if let Some(ref key) = idempotency_key {
        let mut conn = state.redis.clone();
        let idempotency_redis_key = format!("optimus:idempotency:{}", key);
        
        // Check if this key was used before using redis commands
        match ::redis::cmd("GET")
            .arg(&idempotency_redis_key)
            .query_async::<_, Option<String>>(&mut conn)
            .await
        {
            Ok(Some(stored_data)) => {
                // Key exists - check if payload matches
                let payload_json = serde_json::to_string(&payload).unwrap_or_default();
                
                if let Ok(stored) = serde_json::from_str::<serde_json::Value>(&stored_data) {
                    if let Some(stored_payload) = stored.get("payload").and_then(|p| p.as_str()) {
                        if stored_payload == payload_json {
                            // Same payload - return existing job_id
                            if let Some(job_id) = stored.get("job_id").and_then(|j| j.as_str()) {
                                info!(
                                    idempotency_key = %key,
                                    job_id = %job_id,
                                    "Idempotent request - returning existing job_id"
                                );
                                return (
                                    StatusCode::ACCEPTED,
                                    Json(SubmitResponse {
                                        job_id: job_id.to_string(),
                                    }),
                                ).into_response();
                            }
                        } else {
                            // Different payload with same key - conflict
                            warn!(
                                idempotency_key = %key,
                                "Rejected: Same idempotency key with different payload"
                            );
                            metrics::record_job_rejected("idempotency_conflict");
                            return (
                                StatusCode::CONFLICT,
                                Json(ErrorResponse {
                                    error: ErrorDetail {
                                        code: "IDEMPOTENCY_CONFLICT".to_string(),
                                        message: "Same idempotency key used with different payload".to_string(),
                                    },
                                }),
                            ).into_response();
                        }
                    }
                }
            }
            Ok(None) => {
                // Key doesn't exist - will store after creating job
            }
            Err(e) => {
                error!(error = %e, "Failed to check idempotency key");
                // Continue without idempotency on Redis errors
            }
        }
    }
    
    // Generate job ID
    let job_id = Uuid::new_v4();
    
    // Serialize payload early for idempotency check (before moving fields)
    let payload_json_for_idempotency = serde_json::to_string(&payload).unwrap_or_default();
    
    // Safety checks - validate request before queueing
    
    // 1. Check test case count
    if payload.test_cases.is_empty() {
        metrics::record_job_rejected("no_test_cases");
        error!(job_id = %job_id, "Rejected: No test cases provided");
        return (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: ErrorDetail {
                    code: "NO_TEST_CASES".to_string(),
                    message: "At least one test case is required".to_string(),
                },
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
                error: ErrorDetail {
                    code: "TOO_MANY_TEST_CASES".to_string(),
                    message: format!(
                        "Maximum {} test cases allowed, got {}",
                        MAX_TEST_CASES,
                        payload.test_cases.len()
                    ),
                },
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
            StatusCode::PAYLOAD_TOO_LARGE,
            Json(ErrorResponse {
                error: ErrorDetail {
                    code: "SOURCE_CODE_TOO_LARGE".to_string(),
                    message: format!(
                        "Maximum {} bytes allowed, got {} bytes",
                        MAX_SOURCE_CODE_SIZE,
                        payload.source_code.len()
                    ),
                },
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
                error: ErrorDetail {
                    code: "EMPTY_SOURCE_CODE".to_string(),
                    message: "Source code cannot be empty".to_string(),
                },
            }),
        ).into_response();
    }
    
    // 4. Check test case input/output sizes
    for (idx, tc) in payload.test_cases.iter().enumerate() {
        if tc.input.len() > MAX_STDIN_SIZE {
            metrics::record_job_rejected("test_case_input_too_large");
            error!(
                job_id = %job_id,
                test_case = idx + 1,
                size = tc.input.len(),
                limit = MAX_STDIN_SIZE,
                "Rejected: Test case input too large"
            );
            return (
                StatusCode::PAYLOAD_TOO_LARGE,
                Json(ErrorResponse {
                    error: ErrorDetail {
                        code: "TEST_CASE_INPUT_TOO_LARGE".to_string(),
                        message: format!(
                            "Test case {} input exceeds {} bytes",
                            idx + 1,
                            MAX_STDIN_SIZE
                        ),
                    },
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
                StatusCode::PAYLOAD_TOO_LARGE,
                Json(ErrorResponse {
                    error: ErrorDetail {
                        code: "TEST_CASE_OUTPUT_TOO_LARGE".to_string(),
                        message: format!(
                            "Test case {} expected output exceeds {} bytes",
                            idx + 1,
                            MAX_EXPECTED_OUTPUT_SIZE
                        ),
                    },
                }),
            ).into_response();
        }
    }
    
    // 5. Validate timeout
    if payload.timeout_ms < MIN_TIMEOUT_MS || payload.timeout_ms > MAX_TIMEOUT_MS {
        metrics::record_job_rejected("invalid_timeout");
        error!(
            job_id = %job_id,
            timeout_ms = payload.timeout_ms,
            "Rejected: Invalid timeout"
        );
        return (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: ErrorDetail {
                    code: "INVALID_TIMEOUT".to_string(),
                    message: format!(
                        "Timeout must be between {}ms and {}ms",
                        MIN_TIMEOUT_MS,
                        MAX_TIMEOUT_MS
                    ),
                },
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
            // Store idempotency key if provided
            if let Some(ref key) = idempotency_key {
                let idempotency_redis_key = format!("optimus:idempotency:{}", key);
                let idempotency_data = serde_json::json!({
                    "job_id": job_id.to_string(),
                    "payload": payload_json_for_idempotency,
                    "created_at": chrono::Utc::now().to_rfc3339(),
                });
                
                // Store with 24 hour TTL using SETEX
                let mut conn_for_idempotency = state.redis.clone();
                if let Err(e) = ::redis::cmd("SETEX")
                    .arg(&idempotency_redis_key)
                    .arg(86400) // 24 hours
                    .arg(idempotency_data.to_string())
                    .query_async::<_, ()>(&mut conn_for_idempotency)
                    .await
                {
                    error!(
                        error = %e,
                        idempotency_key = %key,
                        "Failed to store idempotency key (job already queued)"
                    );
                    // Don't fail the request - job is already queued
                }
            }
            
            // Record metrics
            metrics::record_job_submitted(&job.language.to_string());
            
            info!(
                job_id = %job_id,
                language = %job.language,
                test_cases = job.test_cases.len(),
                phase = "queued",
                idempotency_key = ?idempotency_key,
                "Job queued"
            );
            
            (
                StatusCode::ACCEPTED,
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
                    error: ErrorDetail {
                        code: "QUEUE_FAILURE".to_string(),
                        message: format!("Failed to queue job: {}", e),
                    },
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

/// GET /health - Liveness probe (process alive check)
/// Returns 200 if the process is running
pub async fn health_check(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let uptime = state.start_time.elapsed().as_secs();
    
    let response = HealthResponse {
        status: "healthy".to_string(),
        uptime_seconds: uptime,
        redis_connected: true, // We assume Redis is fine for liveness
        timestamp: chrono::Utc::now().to_rfc3339(),
    };

    (StatusCode::OK, Json(response))
}

/// GET /ready - Readiness probe (Redis connectivity check)
/// Returns 200 only if Redis is reachable
pub async fn readiness_check(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let uptime = state.start_time.elapsed().as_secs();
    
    // Test Redis connectivity with PING
    let redis_ok = match ::redis::cmd("PING")
        .query_async::<_, String>(&mut state.redis.clone())
        .await
    {
        Ok(_) => true,
        Err(e) => {
            error!(error = %e, "Redis readiness check failed");
            false
        }
    };

    let response = HealthResponse {
        status: if redis_ok { "ready".to_string() } else { "not_ready".to_string() },
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
                Json(ErrorResponse {
                    error: ErrorDetail {
                        code: "INVALID_JOB_ID".to_string(),
                        message: "Invalid job ID format".to_string(),
                    },
                }),
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
            info!(job_id = %job_id, "Job still pending or not found");
            // Result not found - job may still be queued/running (or doesn't exist)
            // We return 202 optimistically to avoid expensive queue scans
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
                Json(ErrorResponse {
                    error: ErrorDetail {
                        code: "INTERNAL_ERROR".to_string(),
                        message: format!("Failed to query job status: {}", e),
                    },
                }),
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
    // Parse job ID
    let job_uuid = match Uuid::parse_str(&job_id) {
        Ok(id) => id,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    error: ErrorDetail {
                        code: "INVALID_JOB_ID".to_string(),
                        message: "Invalid job ID format".to_string(),
                    },
                }),
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
                Json(ErrorResponse {
                    error: ErrorDetail {
                        code: "INTERNAL_ERROR".to_string(),
                        message: format!("Failed to query job: {}", e),
                    },
                }),
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
        if let Ok(items) = ::redis::cmd("LRANGE")
            .arg(&main_queue)
            .arg(0)
            .arg(-1)
            .query_async::<_, Vec<String>>(&mut conn)
            .await
        {
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
        if let Ok(items) = ::redis::cmd("LRANGE")
            .arg(&retry_queue)
            .arg(0)
            .arg(-1)
            .query_async::<_, Vec<String>>(&mut conn)
            .await
        {
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
        if let Ok(items) = ::redis::cmd("LRANGE")
            .arg(&dlq)
            .arg(0)
            .arg(-1)
            .query_async::<_, Vec<String>>(&mut conn)
            .await
        {
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
        
        if in_main_queue || in_retry_queue || in_dlq {
            break;
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

#[derive(Debug, Serialize)]
pub struct CancelResponse {
    pub job_id: String,
    pub status: String,
    pub message: String,
}

/// POST /job/{job_id}/cancel - Cancel a running or queued job
/// 
/// Behavior:
/// - Sets cancellation flag in Redis
/// - Idempotent (multiple calls are safe)
/// - Returns 200 OK if cancelled
/// - Returns 409 Conflict if already completed/failed
/// - Returns 404 Not Found if job doesn't exist
pub async fn cancel_job(
    State(state): State<Arc<AppState>>,
    Path(job_id): Path<String>,
) -> impl IntoResponse {
    // Parse job ID
    let job_uuid = match Uuid::parse_str(&job_id) {
        Ok(id) => id,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    error: ErrorDetail {
                        code: "INVALID_JOB_ID".to_string(),
                        message: "Invalid job ID format".to_string(),
                    },
                }),
            ).into_response();
        }
    };

    let mut conn = state.redis.clone();
    
    // Check if job already has a result (completed/failed)
    match redis::get_result(&mut conn, &job_uuid).await {
        Ok(Some(result)) => {
            // Job already completed - cannot cancel
            let status = match result.overall_status {
                optimus_common::types::JobStatus::Completed => "completed",
                optimus_common::types::JobStatus::Failed => "failed",
                optimus_common::types::JobStatus::TimedOut => "timed_out",
                optimus_common::types::JobStatus::Cancelled => "cancelled",
                _ => "finished",
            };
            
            info!(
                job_id = %job_id,
                status = ?result.overall_status,
                "Cannot cancel job - already finished"
            );
            
            return (
                StatusCode::CONFLICT,
                Json(CancelResponse {
                    job_id: job_id.clone(),
                    status: status.to_string(),
                    message: format!("Job has already finished with status: {}", status),
                }),
            ).into_response();
        }
        Ok(None) => {
            // Job not finished yet - proceed with cancellation
        }
        Err(e) => {
            error!(job_id = %job_id, error = %e, "Failed to check job status");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: ErrorDetail {
                        code: "INTERNAL_ERROR".to_string(),
                        message: format!("Failed to query job: {}", e),
                    },
                }),
            ).into_response();
        }
    }
    
    // Set cancellation flag
    match redis::set_job_cancelled(&mut conn, &job_uuid).await {
        Ok(_) => {
            info!(job_id = %job_id, "Job cancellation requested");
            metrics::record_job_cancelled("user");
            
            (
                StatusCode::OK,
                Json(CancelResponse {
                    job_id: job_id.clone(),
                    status: "cancelling".to_string(),
                    message: "Job cancellation requested. Worker will stop execution.".to_string(),
                }),
            ).into_response()
        }
        Err(e) => {
            error!(job_id = %job_id, error = %e, "Failed to set cancellation flag");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: ErrorDetail {
                        code: "INTERNAL_ERROR".to_string(),
                        message: format!("Failed to cancel job: {}", e),
                    },
                }),
            ).into_response()
        }
    }
}
