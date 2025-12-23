/// Job Executor - High-Level Orchestration
///
/// **Responsibility:**
/// Coordinate execution engine and evaluator to produce final results.
///
/// **Architecture:**
/// 1. Use DockerEngine to run code in sandboxed containers (engine.rs)
/// 2. Use Evaluator to score outputs (evaluator.rs)
/// 3. Return aggregated ExecutionResult
///
/// This module is the glue layer - it knows nothing about:
/// - How code executes (engine's job)
/// - How scoring works (evaluator's job)

use crate::engine::{execute_job_async, DockerEngine};
use crate::evaluator;
use crate::config::LanguageConfigManager;
use optimus_common::types::{ExecutionResult, JobRequest};
use anyhow::Result;

/// Execute a job using Docker engine + evaluator
///
/// This is the production execution path:
/// - DockerEngine runs code in sandboxed containers with language-specific configs
/// - Evaluator scores outputs
/// - Results are aggregated
/// - Cooperative cancellation is checked between test cases
/// 
/// ## Feature Flag: USE_COMPILE_ONCE
/// Set environment variable `USE_COMPILE_ONCE=true` to enable the new compile-once execution model
pub async fn execute_docker(
    job: &JobRequest,
    config_manager: &LanguageConfigManager,
    redis_conn: &mut redis::aio::ConnectionManager,
) -> Result<ExecutionResult> {
    println!("→ Starting job execution: {}", job.id);
    
    // Check feature flag for compile-once execution
    let use_compile_once = std::env::var("USE_COMPILE_ONCE")
        .unwrap_or_else(|_| "false".to_string())
        .to_lowercase() == "true";
    
    let execution_mode = if use_compile_once { "compile_once" } else { "legacy" };
    
    if use_compile_once {
        println!("  Using: Compile-Once Execution (NEW)");
    } else {
        println!("  Using: Per-Test Compilation (LEGACY)");
    }
    println!();
    
    tracing::info!(
        job_id = %job.id,
        language = %job.language,
        test_count = job.test_cases.len(),
        execution_mode = execution_mode,
        "Starting job execution"
    );

    // Step 1: Create Docker engine with config manager
    let engine = DockerEngine::new_with_config(config_manager)?;

    // Step 2: Execute with Docker engine (with cancellation support)
    let outputs = if use_compile_once {
        // NEW PATH: Compile once, run all tests
        engine.execute_job_in_single_container(job, redis_conn).await
    } else {
        // LEGACY PATH: Compile per test (current behavior)
        execute_job_async(job, &engine, redis_conn).await
    };

    // Cross-layer guard: Log failed executions before evaluation
    for output in &outputs {
        if output.compilation_failed {
            tracing::warn!(
                test_id = output.test_id,
                "Compilation failed; all tests marked as failed"
            );
        }
        if output.runtime_error {
            tracing::warn!(
                test_id = output.test_id,
                execution_time_ms = output.execution_time_ms,
                "Execution failed with runtime error; test cannot pass"
            );
        }
        if output.timed_out {
            tracing::warn!(
                test_id = output.test_id,
                execution_time_ms = output.execution_time_ms,
                "Execution timed out; test cannot pass"
            );
        }
    }

    // Step 3: Evaluate outputs
    let result = evaluator::evaluate(job, outputs);

    Ok(result)
}
