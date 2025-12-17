mod engine;
mod evaluator;
mod executor;
mod config;

use optimus_common::redis;
use optimus_common::types::Language;
use tokio::signal;
use config::LanguageConfigManager;
use tracing::{info, error, warn, debug, instrument};

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

    // Load language configurations
    let config_manager = LanguageConfigManager::load_default()
        .map_err(|e| {
            error!("Failed to load language configurations: {}", e);
            error!("Make sure config/languages.json exists");
            e
        })?;
    
    info!("Loaded language configurations for: {:?}", config_manager.list_languages());

    // Get language from environment
    let language_str = std::env::var("WORKER_LANGUAGE")
        .unwrap_or_else(|_| "python".to_string());
    
    let language = match Language::from_str(&language_str) {
        Some(lang) => lang,
        None => {
            error!("Invalid language: {}", language_str);
            let valid_languages: Vec<String> = Language::all_variants()
                .iter()
                .map(|l| l.to_string())
                .collect();
            error!("Valid options: {}", valid_languages.join(", "));
            std::process::exit(1);
        }
    };

    // Validate language is configured
    if let Err(e) = config_manager.get_config(&language) {
        error!("Language '{}' is not configured: {}", language, e);
        error!("Available languages: {:?}", config_manager.list_languages());
        std::process::exit(1);
    }

    // Get language-specific settings
    let queue_name = config_manager.get_queue_name(&language)?;
    let image = config_manager.get_image(&language)?;
    
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

    // Setup graceful shutdown
    let shutdown = async {
        signal::ctrl_c().await.expect("failed to install CTRL+C signal handler");
        warn!("⚠️  Received SIGTERM/CTRL+C - initiating graceful shutdown");
        warn!("Worker will finish current job and exit");
    };

    tokio::select! {
        _ = worker_loop(&mut redis_conn, &language, &config_manager) => {},
        _ = shutdown => {},
    }

    info!("✓ Worker shutdown complete - all jobs processed");
    Ok(())
}

#[instrument(skip(redis_conn, config_manager), fields(language = %language))]
async fn worker_loop(
    redis_conn: &mut ::redis::aio::ConnectionManager,
    language: &Language,
    config_manager: &LanguageConfigManager,
) -> anyhow::Result<()> {
    loop {
        // Log idle state (waiting for jobs)
        debug!("Worker IDLE - waiting for job from queue");
        
        // BLPOP with 5 second timeout for graceful shutdown
        // Consumes from both main queue and retry queue (main has priority)
        match redis::pop_job_with_retry(redis_conn, language, 5.0).await {
            Ok(Some(mut job)) => {
                let job_id = job.id;
                info!(
                    job_id = %job_id,
                    language = %job.language,
                    timeout_ms = job.timeout_ms,
                    test_cases = job.test_cases.len(),
                    source_size = job.source_code.len(),
                    phase = "dequeued",
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
                
                // Execute job with Docker executor
                info!(
                    job_id = %job_id, 
                    phase = "executing",
                    attempt = job.metadata.attempts + 1,
                    max_attempts = job.metadata.max_attempts,
                    "Starting execution"
                );
                let start = std::time::Instant::now();
                let result = match executor::execute_docker(&job, config_manager).await {
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
                
                info!(job_id = %job_id, phase = "done", "Worker IDLE - job completed");
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
