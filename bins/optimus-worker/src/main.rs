mod engine;
mod evaluator;
mod executor;
mod config;

use optimus_common::redis;
use optimus_common::types::Language;
use tokio::signal;
use config::LanguageConfigManager;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    println!("Optimus Worker booting...");

    // Load language configurations
    let config_manager = LanguageConfigManager::load_default()
        .map_err(|e| {
            eprintln!("✗ Failed to load language configurations: {}", e);
            eprintln!("  Make sure config/languages.json exists");
            e
        })?;
    
    println!("✓ Loaded language configurations for: {:?}", config_manager.list_languages());

    // Get language from environment
    let language_str = std::env::var("WORKER_LANGUAGE")
        .unwrap_or_else(|_| "python".to_string());
    
    let language = match language_str.to_lowercase().as_str() {
        "python" => Language::Python,
        "java" => Language::Java,
        "rust" => Language::Rust,
        _ => {
            eprintln!("✗ Invalid language: {}", language_str);
            eprintln!("  Valid options: python, java, rust");
            std::process::exit(1);
        }
    };

    // Validate language is configured
    if let Err(e) = config_manager.get_config(&language) {
        eprintln!("✗ Language '{}' is not configured: {}", language, e);
        eprintln!("  Available languages: {:?}", config_manager.list_languages());
        std::process::exit(1);
    }

    // Get language-specific settings
    let queue_name = config_manager.get_queue_name(&language)?;
    let image = config_manager.get_image(&language)?;
    
    println!("✓ Worker configured for language: {}", language);
    println!("  Docker image: {}", image);
    println!("  Queue: {}", queue_name);

    // Connect to Redis
    let redis_url = std::env::var("REDIS_URL")
        .unwrap_or_else(|_| "redis://127.0.0.1:6379".to_string());
    
    let client = ::redis::Client::open(redis_url.as_str())?;
    let mut redis_conn = ::redis::aio::ConnectionManager::new(client).await?;
    
    println!("✓ Connected to Redis: {}", redis_url);
    println!();

    // Setup graceful shutdown
    let shutdown = async {
        signal::ctrl_c().await.expect("failed to install CTRL+C signal handler");
        println!("\n✓ Received shutdown signal, draining queue...");
    };

    tokio::select! {
        _ = worker_loop(&mut redis_conn, &language, &config_manager) => {},
        _ = shutdown => {},
    }

    println!("✓ Worker shutdown complete");
    Ok(())
}

async fn worker_loop(
    redis_conn: &mut ::redis::aio::ConnectionManager,
    language: &Language,
    config_manager: &LanguageConfigManager,
) -> anyhow::Result<()> {
    loop {
        // BLPOP with 5 second timeout for graceful shutdown
        match redis::pop_job(redis_conn, language, 5.0).await {
            Ok(Some(job)) => {
                println!("═══════════════════════════════════════════");
                println!("Received job: {}", job.id);
                println!("Language: {}", job.language);
                println!("Timeout: {}ms", job.timeout_ms);
                println!("Test cases: {}", job.test_cases.len());
                println!("Source code size: {} bytes", job.source_code.len());
                
                // Display language-specific configuration
                if let Ok(config) = config_manager.get_config(&job.language) {
                    println!("Docker image: {}", config.image);
                    println!("Memory limit: {}MB", config.memory_limit_mb);
                    println!("CPU limit: {}", config.cpu_limit);
                }
                
                println!("═══════════════════════════════════════════");
                println!();
                
                // Execute job with Docker executor (now using config)
                let result = match executor::execute_docker(&job, config_manager).await {
                    Ok(result) => result,
                    Err(e) => {
                        eprintln!("✗ Docker execution failed: {}", e);
                        continue;
                    }
                };
                
                println!();
                println!("═══════════════════════════════════════════");
                println!("EXECUTION RESULT");
                println!("═══════════════════════════════════════════");
                println!("Job ID: {}", result.job_id);
                println!("Overall Status: {:?}", result.overall_status);
                println!("Score: {} / {}", result.score, result.max_score);
                println!("───────────────────────────────────────────");
                
                for (idx, test_result) in result.results.iter().enumerate() {
                    println!(
                        "Test {} (id: {}) → {:?}",
                        idx + 1,
                        test_result.test_id,
                        test_result.status
                    );
                    println!("  Execution time: {}ms", test_result.execution_time_ms);
                    if !test_result.stdout.is_empty() {
                        println!("  Stdout: \"{}\"", test_result.stdout);
                    }
                    if !test_result.stderr.is_empty() {
                        println!("  Stderr: \"{}\"", test_result.stderr);
                    }
                }
                
                println!("═══════════════════════════════════════════");
                println!();
            }
            Ok(None) => {
                // Timeout - check for shutdown
                continue;
            }
            Err(e) => {
                eprintln!("✗ Redis error: {}", e);
                tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
            }
        }
    }
}
