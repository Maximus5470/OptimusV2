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
    
    let language = match language_str.to_lowercase().as_str() {
        "python" => Language::Python,
        "java" => Language::Java,
        "rust" => Language::Rust,
        _ => {
            error!("Invalid language: {}", language_str);
            error!("Valid options: python, java, rust");
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

    // Setup graceful shutdown
    let shutdown = async {
        signal::ctrl_c().await.expect("failed to install CTRL+C signal handler");
        warn!("Received shutdown signal, draining queue...");
    };

    tokio::select! {
        _ = worker_loop(&mut redis_conn, &language, &config_manager) => {},
        _ = shutdown => {},
    }

    info!("Worker shutdown complete");
    Ok(())
}

#[instrument(skip(redis_conn, config_manager), fields(language = %language))]
async fn worker_loop(
    redis_conn: &mut ::redis::aio::ConnectionManager,
    language: &Language,
    config_manager: &LanguageConfigManager,
) -> anyhow::Result<()> {
    loop {
        // BLPOP with 5 second timeout for graceful shutdown
        match redis::pop_job(redis_conn, language, 5.0).await {
            Ok(Some(job)) => {
                let job_id = job.id;
                info!(
                    job_id = %job_id,
                    language = %job.language,
                    timeout_ms = job.timeout_ms,
                    test_cases = job.test_cases.len(),
                    source_size = job.source_code.len(),
                    "Received job"
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
                let start = std::time::Instant::now();
                let result = match executor::execute_docker(&job, config_manager).await {
                    Ok(result) => result,
                    Err(e) => {
                        error!(job_id = %job_id, error = %e, "Docker execution failed");
                        continue;
                    }
                };
                let execution_time = start.elapsed();
                
                info!(
                    job_id = %job_id,
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
                
                // Persist result to Redis
                match redis::store_result(redis_conn, &result).await {
                    Ok(_) => {
                        info!(job_id = %job_id, "Result persisted to Redis");
                    }
                    Err(e) => {
                        error!(job_id = %job_id, error = %e, "Failed to persist result");
                        // Non-fatal - worker continues
                    }
                }
            }
            Ok(None) => {
                // Timeout - check for shutdown
                continue;
            }
            Err(e) => {
                error!(error = %e, "Redis error");
                tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
            }
        }
    }
}
