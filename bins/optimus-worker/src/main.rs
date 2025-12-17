mod engine;
mod evaluator;
mod executor;
mod config;

use optimus_common::redis;
use optimus_common::types::Language;
use optimus_common::config::WorkerConfig;
use tokio::signal;
use tokio::sync::{Semaphore, RwLock};
use std::sync::Arc;
use config::LanguageConfigManager;
use tracing::{info, error, warn, debug, instrument};
use bollard::{Docker, image::CreateImageOptions};
use futures_util::stream::StreamExt;
use axum::{
    extract::State,
    response::{IntoResponse, Json},
    routing::get,
    Router,
    http::StatusCode,
};
use tokio::net::TcpListener;
use serde::Serialize;

/// Shared worker state for health checks
#[derive(Clone)]
struct WorkerState {
    redis_url: String,
    is_executing: Arc<RwLock<bool>>,
}

#[derive(Serialize)]
struct HealthResponse {
    status: String,
    executing_job: bool,
}

/// Liveness probe - simple process alive check
async fn health_handler(State(state): State<WorkerState>) -> impl IntoResponse {
    let executing = *state.is_executing.read().await;
    (
        StatusCode::OK,
        Json(HealthResponse {
            status: "healthy".to_string(),
            executing_job: executing,
        })
    )
}

/// Readiness probe - checks Redis connectivity and execution state
async fn ready_handler(State(state): State<WorkerState>) -> impl IntoResponse {
    // Check Redis connectivity
    let redis_ok = match ::redis::Client::open(state.redis_url.as_str()) {
        Ok(client) => {
            match client.get_async_connection().await {
                Ok(mut conn) => {
                    ::redis::cmd("PING")
                        .query_async::<_, String>(&mut conn)
                        .await
                        .is_ok()
                }
                Err(_) => false,
            }
        }
        Err(_) => false,
    };

    let executing = *state.is_executing.read().await;
    
    // Worker is ready if Redis is reachable AND not currently executing
    // This allows KEDA to scale down idle workers safely
    let is_ready = redis_ok && !executing;
    
    let status_code = if is_ready {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };

    (
        status_code,
        Json(HealthResponse {
            status: if is_ready { "ready".to_string() } else { "not_ready".to_string() },
            executing_job: executing,
        })
    )
}

/// Start health check HTTP server
async fn start_health_server(state: WorkerState) -> anyhow::Result<()> {
    let app = Router::new()
        .route("/health", get(health_handler))
        .route("/ready", get(ready_handler))
        .with_state(state);

    let port = std::env::var("HEALTH_PORT")
        .unwrap_or_else(|_| "8080".to_string());
    let addr = format!("0.0.0.0:{}", port);
    
    let listener = TcpListener::bind(&addr).await?;
    info!("Health server listening on {}", addr);
    
    axum::serve(listener, app).await?;
    Ok(())
}

/// Pre-pull a Docker image (best-effort)
/// Returns Ok(true) if image was pulled, Ok(false) if already present
async fn prepull_image(image: &str) -> anyhow::Result<bool> {
    let docker = Docker::connect_with_local_defaults()?;
    
    // Check if image exists locally
    if docker.inspect_image(image).await.is_ok() {
        return Ok(false); // Already present
    }
    
    // Pull the image
    let options = Some(CreateImageOptions {
        from_image: image,
        ..Default::default()
    });
    
    let mut stream = docker.create_image(options, None, None);
    while let Some(result) = stream.next().await {
        result?;
    }
    
    Ok(true) // Successfully pulled
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize tracing subscriber
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"))
        )
        .with_target(false)
        .with_thread_ids(true)
        .with_line_number(true)
        .init();

    info!("Optimus Worker booting...");

    // Load worker concurrency configuration
    let worker_config = WorkerConfig::from_env();
    info!(
        "Worker concurrency config: max_parallel_jobs={}, max_parallel_tests={}",
        worker_config.max_parallel_jobs,
        worker_config.max_parallel_tests
    );

    // Load language configurations
    let config_manager = LanguageConfigManager::load_default()
        .map_err(|e| {
            error!("Failed to load language configurations: {}", e);
            error!("Make sure config/languages.json exists");
            e
        })?;
    
    info!("Loaded language configurations for: {:?}", config_manager.list_languages());

    // Pre-pull all language images (best-effort, async, non-blocking)
    info!("Pre-pulling language images to warm cache...");
    let prepull_config_manager = config_manager.clone();
    tokio::spawn(async move {
        for lang_name in prepull_config_manager.list_languages() {
            if let Some(lang) = Language::from_str(&lang_name) {
                if let Ok(image) = prepull_config_manager.get_image(&lang) {
                    info!("Pre-pulling image: {}", image);
                    match prepull_image(&image).await {
                        Ok(true) => info!("✓ Image cached: {}", image),
                        Ok(false) => info!("✓ Image already present: {}", image),
                        Err(e) => warn!("⚠ Failed to pre-pull {}: {} (will retry during execution)", image, e),
                    }
                }
            }
        }
        info!("✓ Image pre-pull complete");
    });

    // ===== LANGUAGE BINDING ENFORCEMENT =====
    // Worker MUST be bound to exactly one language via environment variables
    // This is non-negotiable for proper scaling and isolation
    
    // 1. Validate OPTIMUS_LANGUAGE is set (REQUIRED)
    let language_str = std::env::var("OPTIMUS_LANGUAGE")
        .unwrap_or_else(|_| {
            error!("❌ FATAL: OPTIMUS_LANGUAGE environment variable not set");
            error!("Worker must be bound to a specific language (python, java, rust)");
            error!("This worker cannot start without language specification");
            std::process::exit(1);
        });
    
    let language = match Language::from_str(&language_str) {
        Some(lang) => lang,
        None => {
            error!("❌ FATAL: Invalid language: {}", language_str);
            let valid_languages: Vec<String> = Language::all_variants()
                .iter()
                .map(|l| l.to_string())
                .collect();
            error!("Valid options: {}", valid_languages.join(", "));
            std::process::exit(1);
        }
    };

    // 2. Validate language configuration exists
    if let Err(e) = config_manager.get_config(&language) {
        error!("❌ FATAL: Language '{}' is not configured: {}", language, e);
        error!("Available languages: {:?}", config_manager.list_languages());
        std::process::exit(1);
    }

    // 3. Validate OPTIMUS_QUEUE matches language (REQUIRED)
    let expected_queue = config_manager.get_queue_name(&language)?;
    let queue_name = std::env::var("OPTIMUS_QUEUE")
        .unwrap_or_else(|_| {
            error!("❌ FATAL: OPTIMUS_QUEUE environment variable not set");
            error!("Expected queue for {}: {}", language, expected_queue);
            error!("Worker cannot start without queue specification");
            std::process::exit(1);
        });
    
    if queue_name != expected_queue {
        error!("❌ FATAL: Queue mismatch detected");
        error!("  Configured language: {}", language);
        error!("  Expected queue: {}", expected_queue);
        error!("  Actual queue: {}", queue_name);
        error!("This configuration would cause routing bugs. Refusing to start.");
        std::process::exit(1);
    }

    // 4. Validate OPTIMUS_IMAGE matches language (REQUIRED)
    let expected_image = config_manager.get_image(&language)?;
    let image = std::env::var("OPTIMUS_IMAGE")
        .unwrap_or_else(|_| {
            error!("❌ FATAL: OPTIMUS_IMAGE environment variable not set");
            error!("Expected image for {}: {}", language, expected_image);
            error!("Worker cannot start without image specification");
            std::process::exit(1);
        });
    
    if image != expected_image {
        error!("❌ FATAL: Image mismatch detected");
        error!("  Configured language: {}", language);
        error!("  Expected image: {}", expected_image);
        error!("  Actual image: {}", image);
        error!("This configuration would cause execution bugs. Refusing to start.");
        std::process::exit(1);
    }

    // ===== ALL VALIDATIONS PASSED =====
    
    info!("Worker configured for language: {}", language);
    info!("Docker image: {}", image);
    info!("Queue: {}", queue_name);

    // Connect to Redis
    let redis_url = std::env::var("REDIS_URL")
        .unwrap_or_else(|_| "redis://127.0.0.1:6379".to_string());
    
    let client = ::redis::Client::open(redis_url.as_str())?;
    let mut redis_conn = ::redis::aio::ConnectionManager::new(client).await?;
    
    info!("Connected to Redis: {}", redis_url);
    info!("Worker is READY - waiting for jobs from queue: {}", queue_name);

    // Create semaphore for concurrency control
    // This guarantees at most max_parallel_jobs jobs execute simultaneously
    let semaphore = Arc::new(Semaphore::new(worker_config.max_parallel_jobs));
    info!("Concurrency semaphore initialized with {} permits", worker_config.max_parallel_jobs);

    // Create shared state for health checks
    let is_executing = Arc::new(RwLock::new(false));
    let health_state = WorkerState {
        redis_url: redis_url.clone(),
        is_executing: is_executing.clone(),
    };

    // Start health check server in background
    tokio::spawn(async move {
        if let Err(e) = start_health_server(health_state).await {
            error!("Health server error: {}", e);
        }
    });

    // Setup graceful shutdown - handles both SIGTERM (Kubernetes) and SIGINT (CTRL+C)
    let shutdown = async {
        #[cfg(unix)]
        {
            use tokio::signal::unix::{signal, SignalKind};
            let mut sigterm = signal(SignalKind::terminate())
                .expect("failed to install SIGTERM signal handler");
            let mut sigint = signal(SignalKind::interrupt())
                .expect("failed to install SIGINT signal handler");
            
            tokio::select! {
                _ = sigterm.recv() => {
                    warn!("⚠️  Received SIGTERM - initiating graceful shutdown");
                }
                _ = sigint.recv() => {
                    warn!("⚠️  Received SIGINT (CTRL+C) - initiating graceful shutdown");
                }
            }
        }
        #[cfg(not(unix))]
        {
            signal::ctrl_c().await.expect("failed to install CTRL+C signal handler");
            warn!("⚠️  Received CTRL+C - initiating graceful shutdown");
        }
        warn!("Worker will finish current job and exit cleanly");
    };

    tokio::select! {
        _ = worker_loop(&mut redis_conn, &language, &config_manager, semaphore, is_executing) => {},
        _ = shutdown => {},
    }

    info!("✓ Worker shutdown complete - all jobs processed");
    Ok(())
}

#[instrument(skip(redis_conn, config_manager, semaphore, is_executing), fields(language = %language))]
async fn worker_loop(
    redis_conn: &mut ::redis::aio::ConnectionManager,
    language: &Language,
    config_manager: &LanguageConfigManager,
    semaphore: Arc<Semaphore>,
    is_executing: Arc<RwLock<bool>>,
) -> anyhow::Result<()> {
    loop {
        // Log idle state (waiting for jobs)
        debug!("Worker IDLE - waiting for job from queue");
        
        // BLPOP with 5 second timeout for graceful shutdown
        // Consumes from both main queue and retry queue (main has priority)
        match redis::pop_job_with_retry(redis_conn, language, 5.0).await {
            Ok(Some(mut job)) => {
                let job_id = job.id;
                
                // ===== CRITICAL: Language Mismatch Check =====
                // Workers MUST only process jobs for their configured language
                // This prevents cross-language execution bugs
                if job.language != *language {
                    error!(
                        job_id = %job_id,
                        worker_language = %language,
                        job_language = %job.language,
                        phase = "language_mismatch",
                        "❌ FATAL: Job language mismatch - sending to DLQ"
                    );
                    error!(
                        job_id = %job_id,
                        "Worker bound to '{}' received '{}' job - this should never happen",
                        language, job.language
                    );
                    
                    // This is a routing bug - send directly to DLQ
                    job.metadata.last_failure_reason = Some(format!(
                        "Language routing error: worker bound to '{}' cannot execute '{}' job",
                        language, job.language
                    ));
                    
                    if let Err(dlq_err) = redis::push_to_dlq(redis_conn, &job).await {
                        error!(
                            job_id = %job_id,
                            error = %dlq_err,
                            "Failed to push misrouted job to DLQ"
                        );
                    } else {
                        warn!(job_id = %job_id, "Misrouted job sent to DLQ");
                    }
                    
                    continue;
                }
                // ===== End Language Validation =====
                
                // CRITICAL: Acquire semaphore permit before starting execution
                // This enforces max_parallel_jobs limit
                debug!(job_id = %job_id, "Acquiring concurrency permit");
                let permit = semaphore.clone().acquire_owned().await
                    .expect("Semaphore should never be closed");
                
                info!(
                    job_id = %job_id,
                    language = %job.language,
                    timeout_ms = job.timeout_ms,
                    test_cases = job.test_cases.len(),
                    source_size = job.source_code.len(),
                    phase = "dequeued",
                    available_permits = semaphore.available_permits(),
                    "Worker BUSY - processing job"
                );
                
                // Display language-specific configuration
                if let Ok(config) = config_manager.get_config(&job.language) {
                    debug!(
                        job_id = %job_id,
                        image = %config.image,
                        memory_mb = config.memory_limit_mb,
                        cpu_limit = config.cpu_limit,
                        "Job configuration"
                    );
                }
                
                // Check for cancellation before starting execution
                match redis::is_job_cancelled(redis_conn, &job_id).await {
                    Ok(true) => {
                        warn!(
                            job_id = %job_id,
                            phase = "cancelled_before_execution",
                            "Job was cancelled before execution started"
                        );
                        
                        // Store cancelled result
                        let cancelled_result = optimus_common::types::ExecutionResult {
                            job_id: job.id,
                            overall_status: optimus_common::types::JobStatus::Cancelled,
                            score: 0,
                            max_score: job.test_cases.iter().map(|tc| tc.weight).sum(),
                            results: vec![],
                        };
                        
                        if let Err(store_err) = redis::store_result_with_metrics(redis_conn, &cancelled_result, &job.language).await {
                            error!(
                                job_id = %job_id,
                                error = %store_err,
                                "Failed to store cancelled result"
                            );
                        } else {
                            info!(job_id = %job_id, "Cancelled result stored");
                        }
                        
                        // MARK: Worker as idle (job was cancelled)
                        *is_executing.write().await = false;
                        drop(permit);
                        
                        continue;
                    }
                    Ok(false) => {
                        // Not cancelled, proceed with execution
                    }
                    Err(e) => {
                        error!(
                            job_id = %job_id,
                            error = %e,
                            "Failed to check cancellation status, proceeding with execution"
                        );
                    }
                }
                
                // MARK: Worker as executing (for readiness probe)
                *is_executing.write().await = true;
                
                // Execute job with Docker executor
                info!(
                    job_id = %job_id, 
                    phase = "executing",
                    attempt = job.metadata.attempts + 1,
                    max_attempts = job.metadata.max_attempts,
                    "Starting execution"
                );
                let start = std::time::Instant::now();
                let result = match executor::execute_docker(&job, config_manager, redis_conn).await {
                    Ok(result) => result,
                    Err(e) => {
                        error!(
                            job_id = %job_id, 
                            phase = "execution_failed", 
                            error = %e,
                            attempts = job.metadata.attempts,
                            "Docker execution failed"
                        );
                        
                        // Increment attempts
                        job.metadata.attempts += 1;
                        job.metadata.last_failure_reason = Some(format!("Execution error: {}", e));
                        
                        // Retry logic
                        if job.metadata.attempts < job.metadata.max_attempts {
                            warn!(
                                job_id = %job_id,
                                attempt = job.metadata.attempts,
                                max_attempts = job.metadata.max_attempts,
                                "Job failed, sending to retry queue"
                            );
                            
                            if let Err(retry_err) = redis::push_to_retry_queue(redis_conn, &job).await {
                                error!(
                                    job_id = %job_id,
                                    error = %retry_err,
                                    "Failed to push job to retry queue"
                                );
                            } else {
                                info!(job_id = %job_id, "Job pushed to retry queue");
                            }
                        } else {
                            error!(
                                job_id = %job_id,
                                attempts = job.metadata.attempts,
                                "Job exceeded max attempts, sending to DLQ"
                            );
                            
                            if let Err(dlq_err) = redis::push_to_dlq(redis_conn, &job).await {
                                error!(
                                    job_id = %job_id,
                                    error = %dlq_err,
                                    "Failed to push job to DLQ"
                                );
                            } else {
                                info!(job_id = %job_id, "Job pushed to DLQ");
                            }
                            
                            // Store final failed result
                            let failed_result = optimus_common::types::ExecutionResult {
                                job_id: job.id,
                                overall_status: optimus_common::types::JobStatus::Failed,
                                score: 0,
                                max_score: job.test_cases.iter().map(|tc| tc.weight).sum(),
                                results: vec![],
                            };
                            
                            if let Err(store_err) = redis::store_result_with_metrics(redis_conn, &failed_result, &job.language).await {
                                error!(
                                    job_id = %job_id,
                                    error = %store_err,
                                    "Failed to store failed result"
                                );
                            }
                        }
                        
                        // MARK: Worker as idle (execution failed)
                        *is_executing.write().await = false;
                        drop(permit);
                        
                        continue;
                    }
                };
                let execution_time = start.elapsed();
                
                info!(
                    job_id = %job_id,
                    phase = "evaluated",
                    status = ?result.overall_status,
                    score = result.score,
                    max_score = result.max_score,
                    execution_ms = execution_time.as_millis(),
                    "Execution completed"
                );
                
                for (idx, test_result) in result.results.iter().enumerate() {
                    debug!(
                        job_id = %job_id,
                        test_num = idx + 1,
                        test_id = test_result.test_id,
                        status = ?test_result.status,
                        execution_ms = test_result.execution_time_ms,
                        "Test result"
                    );
                }
                
                // Persist result to Redis with metrics
                info!(job_id = %job_id, phase = "persisting", "Storing result to Redis");
                match redis::store_result_with_metrics(redis_conn, &result, &job.language).await {
                    Ok(_) => {
                        info!(job_id = %job_id, phase = "completed", "Result persisted to Redis");
                    }
                    Err(e) => {
                        error!(job_id = %job_id, phase = "persist_failed", error = %e, "Failed to persist result");
                        // Non-fatal - worker continues
                    }
                }
                
                info!(
                    job_id = %job_id, 
                    phase = "done", 
                    available_permits = semaphore.available_permits() + 1,
                    "Worker IDLE - job completed, permit released"
                );
                
                // MARK: Worker as idle (for readiness probe)
                *is_executing.write().await = false;
                
                // Permit is automatically released when dropped here
                drop(permit);
            }
            Ok(None) => {
                // Timeout - check for shutdown (idle continues)
                continue;
            }
            Err(e) => {
                error!(error = %e, "Redis error");
                tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
            }
        }
    }
}
