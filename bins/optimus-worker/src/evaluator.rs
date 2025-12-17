/// Test Evaluator - Language-Agnostic Scoring Logic
///
/// **Core Responsibility:**
/// Compare raw execution outputs against expected outputs and assign scores.
///
/// **Critical Properties:**
/// - Knows nothing about Docker
/// - Knows nothing about language runtimes
/// - Knows nothing about Redis
/// - Pure function: (execution outputs, expected outputs) → scores
///
/// **Scoring Rules:**
/// - Each test case has a weight
/// - score = sum of weights for Passed tests
/// - max_score = sum of all test case weights
/// - overall_status: Completed if any test passed, Failed if all failed
///
/// **Normalization Rules (Applied to All Languages):**
/// - Trim trailing whitespace: YES
/// - Trim leading whitespace: YES
/// - Ignore newline differences (\n vs \r\n): YES (via trim)
/// - Case sensitivity: YES (exact match required)
/// - Floating-point tolerance: NO (future enhancement)
///
/// **Why This Exists:**
/// Separates correctness evaluation from execution mechanism.
/// Guarantees deterministic scoring regardless of execution engine.

use optimus_common::types::{
    ExecutionResult, JobRequest, JobStatus, TestCase, TestResult, TestStatus,
};

/// Raw execution output for a single test case
/// Produced by ExecutionEngine, consumed by Evaluator
#[derive(Debug, Clone)]
pub struct TestExecutionOutput {
    pub test_id: u32,
    pub stdout: String,
    pub stderr: String,
    pub execution_time_ms: u64,
    pub timed_out: bool,
    pub runtime_error: bool,
}

/// Normalize output string for comparison
///
/// **Normalization Rules:**
/// - Trim leading whitespace
/// - Trim trailing whitespace
/// - Removes differences in line endings (\r\n vs \n)
///
/// **Preserves:**
/// - Internal whitespace
/// - Case sensitivity
/// - Empty lines within content
fn normalize_output(output: &str) -> &str {
    output.trim()
}

/// Evaluate a single test case execution output
///
/// This function determines the TestStatus based on:
/// 1. Runtime errors (highest priority)
/// 2. Timeouts (second priority)
/// 3. Output comparison (if execution succeeded)
///
/// ## Arguments
/// * `output` - Raw execution output from the engine
/// * `test_case` - Expected test case definition
///
/// ## Returns
/// TestResult with status and execution details
pub fn evaluate_test(output: &TestExecutionOutput, test_case: &TestCase) -> TestResult {
    let status = if output.runtime_error {
        TestStatus::RuntimeError
    } else if output.timed_out {
        TestStatus::TimeLimitExceeded
    } else {
        // Compare normalized outputs
        let actual = normalize_output(&output.stdout);
        let expected = normalize_output(&test_case.expected_output);

        if actual == expected {
            TestStatus::Passed
        } else {
            TestStatus::Failed
        }
    };

    TestResult {
        test_id: output.test_id,
        status,
        stdout: output.stdout.clone(),
        stderr: output.stderr.clone(),
        execution_time_ms: output.execution_time_ms,
    }
}

/// Aggregate multiple test results into final execution result
///
/// This function:
/// 1. Calculates total score (sum of passed test weights)
/// 2. Calculates max possible score (sum of all weights)
/// 3. Determines overall status (Completed if any passed, Failed otherwise)
///
/// ## Arguments
/// * `outputs` - Raw execution outputs from engine
/// * `job` - Original job request with test cases
///
/// ## Returns
/// Complete ExecutionResult with aggregated scores and status
pub fn aggregate_results(
    outputs: &[TestExecutionOutput],
    job: &JobRequest,
) -> ExecutionResult {
    let mut test_results = Vec::new();
    let mut total_score = 0u32;
    let max_score: u32 = job.test_cases.iter().map(|tc| tc.weight).sum();

    println!("→ Evaluating {} test outputs", outputs.len());
    println!("  Max possible score: {}", max_score);
    println!();

    for output in outputs {
        // Find corresponding test case
        let test_case = job
            .test_cases
            .iter()
            .find(|tc| tc.id == output.test_id)
            .expect("Test case not found for output");

        // Evaluate single test
        let test_result = evaluate_test(output, test_case);

        // Update score if passed
        if test_result.status == TestStatus::Passed {
            total_score += test_case.weight;
        }

        // Log evaluation result
        println!(
            "  Test {} (id: {}, weight: {}) → {:?}",
            test_results.len() + 1,
            test_case.id,
            test_case.weight,
            test_result.status
        );

        match test_result.status {
            TestStatus::Passed => println!("    ✓ Output matched"),
            TestStatus::RuntimeError => println!("    ✗ Runtime error"),
            TestStatus::TimeLimitExceeded => println!("    ✗ Timeout"),
            TestStatus::Failed => {
                println!("    ✗ Output mismatch");
                println!("    Expected: \"{}\"", normalize_output(&test_case.expected_output));
                println!("    Got:      \"{}\"", normalize_output(&output.stdout));
            }
        }

        test_results.push(test_result);
    }

    // Determine overall status
    let overall_status = if total_score > 0 {
        JobStatus::Completed
    } else {
        JobStatus::Failed
    };

    println!();
    println!("→ Evaluation complete");
    println!("  Score: {} / {}", total_score, max_score);
    println!("  Status: {:?}", overall_status);

    ExecutionResult {
        job_id: job.id,
        overall_status,
        score: total_score,
        max_score,
        results: test_results,
    }
}

/// Evaluate all test cases and produce final execution result
///
/// This is the main entry point for evaluation. It delegates to:
/// - `evaluate_test` for individual test evaluation
/// - `aggregate_results` for score calculation and status determination
///
/// ## Arguments
/// * `job` - The original job request (for test cases and expected outputs)
/// * `outputs` - Raw execution outputs from the execution engine
///
/// ## Returns
/// Complete ExecutionResult with scores and aggregated status
pub fn evaluate(job: &JobRequest, outputs: Vec<TestExecutionOutput>) -> ExecutionResult {
    aggregate_results(&outputs, job)
}

#[cfg(test)]
mod tests {
    use super::*;
    use optimus_common::types::{Language, TestCase};
    use uuid::Uuid;

    /// Helper to create a test case
    fn make_test_case(id: u32, expected_output: &str, weight: u32) -> TestCase {
        TestCase {
            id,
            input: "input".to_string(),
            expected_output: expected_output.to_string(),
            weight,
        }
    }

    /// Helper to create a passing output
    fn make_output(test_id: u32, stdout: &str, exec_time: u64) -> TestExecutionOutput {
        TestExecutionOutput {
            test_id,
            stdout: stdout.to_string(),
            stderr: String::new(),
            execution_time_ms: exec_time,
            timed_out: false,
            runtime_error: false,
        }
    }

    #[test]
    fn test_normalize_output() {
        assert_eq!(normalize_output("hello"), "hello");
        assert_eq!(normalize_output("  hello  "), "hello");
        assert_eq!(normalize_output("hello\n"), "hello");
        assert_eq!(normalize_output("\nhello\n"), "hello");
        assert_eq!(normalize_output("  hello world  \n"), "hello world");
        assert_eq!(normalize_output(""), "");
        assert_eq!(normalize_output("   "), "");
    }

    #[test]
    fn test_evaluate_test_exact_match() {
        let test_case = make_test_case(1, "120", 10);
        let output = make_output(1, "120", 42);

        let result = evaluate_test(&output, &test_case);

        assert_eq!(result.status, TestStatus::Passed);
        assert_eq!(result.test_id, 1);
        assert_eq!(result.execution_time_ms, 42);
    }

    #[test]
    fn test_evaluate_test_with_whitespace() {
        let test_case = make_test_case(1, "hello", 10);
        let output = make_output(1, "  hello  \n", 5);

        let result = evaluate_test(&output, &test_case);

        assert_eq!(result.status, TestStatus::Passed);
    }

    #[test]
    fn test_evaluate_test_mismatch() {
        let test_case = make_test_case(1, "expected", 10);
        let output = make_output(1, "actual", 5);

        let result = evaluate_test(&output, &test_case);

        assert_eq!(result.status, TestStatus::Failed);
    }

    #[test]
    fn test_evaluate_test_runtime_error() {
        let test_case = make_test_case(1, "output", 10);
        let output = TestExecutionOutput {
            test_id: 1,
            stdout: String::new(),
            stderr: "RuntimeError: crash".to_string(),
            execution_time_ms: 5,
            timed_out: false,
            runtime_error: true,
        };

        let result = evaluate_test(&output, &test_case);

        assert_eq!(result.status, TestStatus::RuntimeError);
    }

    #[test]
    fn test_evaluate_test_timeout() {
        let test_case = make_test_case(1, "output", 10);
        let output = TestExecutionOutput {
            test_id: 1,
            stdout: String::new(),
            stderr: String::new(),
            execution_time_ms: 1001,
            timed_out: true,
            runtime_error: false,
        };

        let result = evaluate_test(&output, &test_case);

        assert_eq!(result.status, TestStatus::TimeLimitExceeded);
    }

    #[test]
    fn test_all_pass() {
        let job = JobRequest {
            id: Uuid::new_v4(),
            language: Language::Python,
            source_code: String::new(),
            test_cases: vec![
                TestCase {
                    id: 1,
                    input: "5".to_string(),
                    expected_output: "120".to_string(),
                    weight: 10,
                },
                TestCase {
                    id: 2,
                    input: "3".to_string(),
                    expected_output: "6".to_string(),
                    weight: 15,
                },
            ],
            timeout_ms: 5000,
            metadata: optimus_common::types::JobMetadata::default(),
        };

        let outputs = vec![
            TestExecutionOutput {
                test_id: 1,
                stdout: "120".to_string(),
                stderr: String::new(),
                execution_time_ms: 42,
                timed_out: false,
                runtime_error: false,
            },
            TestExecutionOutput {
                test_id: 2,
                stdout: "6".to_string(),
                stderr: String::new(),
                execution_time_ms: 38,
                timed_out: false,
                runtime_error: false,
            },
        ];

        let result = evaluate(&job, outputs);

        assert_eq!(result.overall_status, JobStatus::Completed);
        assert_eq!(result.score, 25);
        assert_eq!(result.max_score, 25);
        assert_eq!(result.results[0].status, TestStatus::Passed);
        assert_eq!(result.results[1].status, TestStatus::Passed);
    }

    #[test]
    fn test_partial_pass() {
        let job = JobRequest {
            id: Uuid::new_v4(),
            language: Language::Java,
            source_code: String::new(),
            test_cases: vec![
                TestCase {
                    id: 1,
                    input: "input".to_string(),
                    expected_output: "correct".to_string(),
                    weight: 20,
                },
                TestCase {
                    id: 2,
                    input: "input".to_string(),
                    expected_output: "wrong".to_string(),
                    weight: 30,
                },
            ],
            timeout_ms: 5000,
            metadata: optimus_common::types::JobMetadata::default(),
        };

        let outputs = vec![
            TestExecutionOutput {
                test_id: 1,
                stdout: "correct".to_string(),
                stderr: String::new(),
                execution_time_ms: 10,
                timed_out: false,
                runtime_error: false,
            },
            TestExecutionOutput {
                test_id: 2,
                stdout: "incorrect".to_string(),
                stderr: String::new(),
                execution_time_ms: 10,
                timed_out: false,
                runtime_error: false,
            },
        ];

        let result = evaluate(&job, outputs);

        assert_eq!(result.overall_status, JobStatus::Completed);
        assert_eq!(result.score, 20);
        assert_eq!(result.max_score, 50);
        assert_eq!(result.results[0].status, TestStatus::Passed);
        assert_eq!(result.results[1].status, TestStatus::Failed);
    }

    #[test]
    fn test_all_fail() {
        let job = JobRequest {
            id: Uuid::new_v4(),
            language: Language::Python,
            source_code: String::new(),
            test_cases: vec![
                make_test_case(1, "expected1", 10),
                make_test_case(2, "expected2", 10),
            ],
            timeout_ms: 5000,
            metadata: optimus_common::types::JobMetadata::default(),
        };

        let outputs = vec![
            make_output(1, "wrong1", 10),
            make_output(2, "wrong2", 10),
        ];

        let result = evaluate(&job, outputs);

        assert_eq!(result.overall_status, JobStatus::Failed);
        assert_eq!(result.score, 0);
        assert_eq!(result.max_score, 20);
        assert_eq!(result.results[0].status, TestStatus::Failed);
        assert_eq!(result.results[1].status, TestStatus::Failed);
    }

    #[test]
    fn test_runtime_error() {
        let job = JobRequest {
            id: Uuid::new_v4(),
            language: Language::Python,
            source_code: String::new(),
            test_cases: vec![TestCase {
                id: 1,
                input: "input".to_string(),
                expected_output: "output".to_string(),
                weight: 10,
            }],
            timeout_ms: 5000,
            metadata: optimus_common::types::JobMetadata::default(),
        };

        let outputs = vec![TestExecutionOutput {
            test_id: 1,
            stdout: String::new(),
            stderr: "RuntimeError: division by zero".to_string(),
            execution_time_ms: 5,
            timed_out: false,
            runtime_error: true,
        }];

        let result = evaluate(&job, outputs);

        assert_eq!(result.overall_status, JobStatus::Failed);
        assert_eq!(result.score, 0);
        assert_eq!(result.results[0].status, TestStatus::RuntimeError);
    }

    #[test]
    fn test_timeout() {
        let job = JobRequest {
            id: Uuid::new_v4(),
            language: Language::Rust,
            source_code: String::new(),
            test_cases: vec![TestCase {
                id: 1,
                input: "input".to_string(),
                expected_output: "output".to_string(),
                weight: 5,
            }],
            timeout_ms: 1000,
            metadata: optimus_common::types::JobMetadata::default(),
        };

        let outputs = vec![TestExecutionOutput {
            test_id: 1,
            stdout: String::new(),
            stderr: String::new(),
            execution_time_ms: 1001,
            timed_out: true,
            runtime_error: false,
        }];

        let result = evaluate(&job, outputs);

        assert_eq!(result.overall_status, JobStatus::Failed);
        assert_eq!(result.score, 0);
        assert_eq!(result.results[0].status, TestStatus::TimeLimitExceeded);
    }

    #[test]
    fn test_whitespace_trimming() {
        let job = JobRequest {
            id: Uuid::new_v4(),
            language: Language::Python,
            source_code: String::new(),
            test_cases: vec![TestCase {
                id: 1,
                input: "input".to_string(),
                expected_output: "hello".to_string(),
                weight: 10,
            }],
            timeout_ms: 5000,
            metadata: optimus_common::types::JobMetadata::default(),
        };

        let outputs = vec![TestExecutionOutput {
            test_id: 1,
            stdout: "  hello  \n".to_string(),
            stderr: String::new(),
            execution_time_ms: 5,
            timed_out: false,
            runtime_error: false,
        }];

        let result = evaluate(&job, outputs);

        assert_eq!(result.overall_status, JobStatus::Completed);
        assert_eq!(result.score, 10);
        assert_eq!(result.results[0].status, TestStatus::Passed);
    }

    #[test]
    fn test_newline_handling() {
        let job = JobRequest {
            id: Uuid::new_v4(),
            language: Language::Java,
            source_code: String::new(),
            test_cases: vec![make_test_case(1, "line1\nline2\nline3", 10)],
            timeout_ms: 5000,
            metadata: optimus_common::types::JobMetadata::default(),
        };

        // Different newline styles should match after normalization
        let outputs = vec![make_output(1, "line1\nline2\nline3\n", 10)];

        let result = evaluate(&job, outputs);

        assert_eq!(result.results[0].status, TestStatus::Passed);
        assert_eq!(result.score, 10);
    }

    #[test]
    fn test_empty_output() {
        let job = JobRequest {
            id: Uuid::new_v4(),
            language: Language::Python,
            source_code: String::new(),
            test_cases: vec![make_test_case(1, "", 5)],
            timeout_ms: 5000,
            metadata: optimus_common::types::JobMetadata::default(),
        };

        let outputs = vec![make_output(1, "   \n", 5)];

        let result = evaluate(&job, outputs);

        assert_eq!(result.results[0].status, TestStatus::Passed);
        assert_eq!(result.score, 5);
    }

    #[test]
    fn test_case_sensitivity() {
        let job = JobRequest {
            id: Uuid::new_v4(),
            language: Language::Python,
            source_code: String::new(),
            test_cases: vec![make_test_case(1, "Hello", 10)],
            timeout_ms: 5000,
            metadata: optimus_common::types::JobMetadata::default(),
        };

        let outputs = vec![make_output(1, "hello", 10)];

        let result = evaluate(&job, outputs);

        // Case should matter - this should fail
        assert_eq!(result.results[0].status, TestStatus::Failed);
        assert_eq!(result.score, 0);
    }

    #[test]
    fn test_mixed_statuses() {
        let job = JobRequest {
            id: Uuid::new_v4(),
            language: Language::Rust,
            source_code: String::new(),
            test_cases: vec![
                make_test_case(1, "pass", 10),
                make_test_case(2, "fail", 10),
                make_test_case(3, "timeout", 10),
                make_test_case(4, "error", 10),
            ],
            timeout_ms: 1000,
            metadata: optimus_common::types::JobMetadata::default(),
        };

        let outputs = vec![
            make_output(1, "pass", 100),
            make_output(2, "wrong", 100),
            TestExecutionOutput {
                test_id: 3,
                stdout: String::new(),
                stderr: String::new(),
                execution_time_ms: 1001,
                timed_out: true,
                runtime_error: false,
            },
            TestExecutionOutput {
                test_id: 4,
                stdout: String::new(),
                stderr: "Error".to_string(),
                execution_time_ms: 50,
                timed_out: false,
                runtime_error: true,
            },
        ];

        let result = evaluate(&job, outputs);

        assert_eq!(result.overall_status, JobStatus::Completed); // At least one passed
        assert_eq!(result.score, 10); // Only first test passed
        assert_eq!(result.max_score, 40);
        assert_eq!(result.results[0].status, TestStatus::Passed);
        assert_eq!(result.results[1].status, TestStatus::Failed);
        assert_eq!(result.results[2].status, TestStatus::TimeLimitExceeded);
        assert_eq!(result.results[3].status, TestStatus::RuntimeError);
    }

    #[test]
    fn test_zero_weight_tests() {
        let job = JobRequest {
            id: Uuid::new_v4(),
            language: Language::Python,
            source_code: String::new(),
            test_cases: vec![
                TestCase {
                    id: 1,
                    input: "input".to_string(),
                    expected_output: "output".to_string(),
                    weight: 0,
                },
            ],
            timeout_ms: 5000,
            metadata: optimus_common::types::JobMetadata::default(),
        };

        let outputs = vec![make_output(1, "output", 10)];

        let result = evaluate(&job, outputs);

        // Even though test passed, score is 0
        assert_eq!(result.score, 0);
        assert_eq!(result.max_score, 0);
        // Status is Failed because total_score is 0 (no points earned)
        assert_eq!(result.overall_status, JobStatus::Failed);
    }

    #[test]
    fn test_aggregate_results_directly() {
        let job = JobRequest {
            id: Uuid::new_v4(),
            language: Language::Python,
            source_code: String::new(),
            test_cases: vec![
                make_test_case(1, "hello", 15),
                make_test_case(2, "world", 25),
            ],
            timeout_ms: 5000,
            metadata: optimus_common::types::JobMetadata::default(),
        };

        let outputs = vec![
            make_output(1, "hello", 50),
            make_output(2, "world", 75),
        ];

        let result = aggregate_results(&outputs, &job);

        assert_eq!(result.score, 40);
        assert_eq!(result.max_score, 40);
        assert_eq!(result.overall_status, JobStatus::Completed);
        assert_eq!(result.job_id, job.id);
    }
}
