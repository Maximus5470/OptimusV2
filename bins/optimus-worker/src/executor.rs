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
pub async fn execute_docker(
    job: &JobRequest,
    config_manager: &LanguageConfigManager,
    redis_conn: &mut redis::aio::ConnectionManager,
) -> Result<ExecutionResult> {
    println!("→ Starting job execution: {}", job.id);
    println!("  Using: DockerEngine + Evaluator");
    println!();

    // Step 1: Create Docker engine with config manager
    let engine = DockerEngine::new_with_config(config_manager)?;

    // Step 2: Execute with Docker engine (with cancellation support)
    let outputs = execute_job_async(job, &engine, redis_conn).await;

    // Cross-layer guard: Log failed executions before evaluation
    for output in &outputs {
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
