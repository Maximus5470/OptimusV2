/// Job Executor - High-Level Orchestration
///
/// **Responsibility:**
/// Coordinate execution engine and evaluator to produce final results.
///
/// **Architecture:**
/// 1. Use ExecutionEngine to run code (engine.rs)
/// 2. Use Evaluator to score outputs (evaluator.rs)
/// 3. Return aggregated ExecutionResult
///
/// This module is the glue layer - it knows nothing about:
/// - How code executes (engine's job)
/// - How scoring works (evaluator's job)

use crate::engine::{execute_job, execute_job_async, DummyEngine, DockerEngine};
use crate::evaluator;
use optimus_common::types::{ExecutionResult, JobRequest};
use anyhow::Result;

/// Execute a job using dummy engine + evaluator
///
/// This validates the complete separated architecture:
/// - Engine produces raw outputs
/// - Evaluator scores outputs
/// - Results are aggregated
pub fn execute_dummy(job: &JobRequest) -> ExecutionResult {
    println!("→ Starting job execution: {}", job.id);
    println!("  Using: DummyEngine + Evaluator");
    println!();

    // Step 1: Execute with engine
    let engine = DummyEngine::new();
    let outputs = execute_job(job, &engine);

    // Step 2: Evaluate outputs
    let result = evaluator::evaluate(job, outputs);

    result
}

/// Execute a job using Docker engine + evaluator
///
/// This is the production execution path:
/// - DockerEngine runs code in sandboxed containers
/// - Evaluator scores outputs (same as dummy)
/// - Results are aggregated (same as dummy)
pub async fn execute_docker(job: &JobRequest) -> Result<ExecutionResult> {
    println!("→ Starting job execution: {}", job.id);
    println!("  Using: DockerEngine + Evaluator");
    println!();

    // Step 1: Create Docker engine
    let engine = DockerEngine::new()?;

    // Step 2: Execute with Docker engine
    let outputs = execute_job_async(job, &engine).await;

    // Step 3: Evaluate outputs (same evaluator as dummy)
    let result = evaluator::evaluate(job, outputs);

    Ok(result)
}


#[cfg(test)]
mod tests {
    use super::*;
    use optimus_common::types::{JobStatus, Language, TestCase, TestStatus};
    use uuid::Uuid;

    #[test]
    fn test_all_tests_pass() {
        let job = JobRequest {
            id: Uuid::new_v4(),
            language: Language::Python,
            source_code: "# dummy code".to_string(),
            test_cases: vec![
                TestCase {
                    id: 1,
                    input: "hello".to_string(),
                    expected_output: "hello".to_string(),
                    weight: 10,
                },
                TestCase {
                    id: 2,
                    input: "world".to_string(),
                    expected_output: "world".to_string(),
                    weight: 15,
                },
            ],
            timeout_ms: 5000,
        };

        let result = execute_dummy(&job);

        assert_eq!(result.overall_status, JobStatus::Completed);
        assert_eq!(result.score, 25); // 10 + 15
        assert_eq!(result.max_score, 25);
        assert_eq!(result.results.len(), 2);
        assert_eq!(result.results[0].status, TestStatus::Passed);
        assert_eq!(result.results[1].status, TestStatus::Passed);
    }

    #[test]
    fn test_partial_pass() {
        let job = JobRequest {
            id: Uuid::new_v4(),
            language: Language::Java,
            source_code: "// dummy".to_string(),
            test_cases: vec![
                TestCase {
                    id: 1,
                    input: "42".to_string(),
                    expected_output: "42".to_string(),
                    weight: 20,
                },
                TestCase {
                    id: 2,
                    input: "wrong".to_string(),
                    expected_output: "different".to_string(),
                    weight: 30,
                },
            ],
            timeout_ms: 5000,
        };

        let result = execute_dummy(&job);

        assert_eq!(result.overall_status, JobStatus::Completed);
        assert_eq!(result.score, 20); // only first test passed
        assert_eq!(result.max_score, 50);
        assert_eq!(result.results[0].status, TestStatus::Passed);
        assert_eq!(result.results[1].status, TestStatus::Failed);
    }

    #[test]
    fn test_all_tests_fail() {
        let job = JobRequest {
            id: Uuid::new_v4(),
            language: Language::Rust,
            source_code: "fn main() {}".to_string(),
            test_cases: vec![
                TestCase {
                    id: 1,
                    input: "input1".to_string(),
                    expected_output: "output1".to_string(),
                    weight: 10,
                },
                TestCase {
                    id: 2,
                    input: "input2".to_string(),
                    expected_output: "output2".to_string(),
                    weight: 10,
                },
            ],
            timeout_ms: 5000,
        };

        let result = execute_dummy(&job);

        assert_eq!(result.overall_status, JobStatus::Failed);
        assert_eq!(result.score, 0);
        assert_eq!(result.max_score, 20);
        assert_eq!(result.results[0].status, TestStatus::Failed);
        assert_eq!(result.results[1].status, TestStatus::Failed);
    }

    #[test]
    fn test_whitespace_trimming() {
        let job = JobRequest {
            id: Uuid::new_v4(),
            language: Language::Python,
            source_code: "print('test')".to_string(),
            test_cases: vec![TestCase {
                id: 1,
                input: "  hello  \n".to_string(),
                expected_output: "hello".to_string(),
                weight: 5,
            }],
            timeout_ms: 5000,
        };

        let result = execute_dummy(&job);

        assert_eq!(result.overall_status, JobStatus::Completed);
        assert_eq!(result.score, 5);
        assert_eq!(result.results[0].status, TestStatus::Passed);
    }

    #[test]
    fn test_empty_test_cases() {
        let job = JobRequest {
            id: Uuid::new_v4(),
            language: Language::Python,
            source_code: "# no tests".to_string(),
            test_cases: vec![],
            timeout_ms: 5000,
        };

        let result = execute_dummy(&job);

        assert_eq!(result.overall_status, JobStatus::Failed);
        assert_eq!(result.score, 0);
        assert_eq!(result.max_score, 0);
        assert_eq!(result.results.len(), 0);
    }

    #[test]
    fn test_execution_time_is_fixed() {
        let job = JobRequest {
            id: Uuid::new_v4(),
            language: Language::Python,
            source_code: "".to_string(),
            test_cases: vec![TestCase {
                id: 1,
                input: "test".to_string(),
                expected_output: "test".to_string(),
                weight: 1,
            }],
            timeout_ms: 5000,
        };

        let result = execute_dummy(&job);

        assert_eq!(result.results[0].execution_time_ms, 5);
    }
}
