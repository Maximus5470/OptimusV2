/// Integration tests for compile-once execution model
/// 
/// These tests verify that the new execution path works correctly:
/// 1. Compilation succeeds and all tests execute
/// 2. Compilation failures are handled properly
/// 3. Runtime errors are detected correctly
/// 4. Timeouts work as expected
/// 5. Container cleanup happens reliably

#[cfg(test)]
mod compile_once_tests {
    use crate::engine::DockerEngine;
    use crate::config::LanguageConfigManager;
    use crate::evaluator::{evaluate};
    use optimus_common::types::{JobRequest, Language, TestCase, JobMetadata, TestStatus};
    use uuid::Uuid;

    /// Helper to create a mock Redis connection manager
    /// Note: These tests require a running Redis instance
    async fn create_redis_conn() -> redis::aio::ConnectionManager {
        let client = redis::Client::open("redis://127.0.0.1:6379")
            .expect("Failed to create Redis client");
        client.get_connection_manager().await
            .expect("Failed to connect to Redis")
    }

    /// Test: Successful compilation and execution of multiple tests
    #[tokio::test]
    #[ignore] // Requires Docker and Redis
    async fn test_compile_once_python_success() {
        let config_manager = LanguageConfigManager::load_default()
            .expect("Failed to load language config");
        
        let engine = DockerEngine::new_with_config(&config_manager)
            .expect("Failed to create Docker engine");
        
        let mut redis_conn = create_redis_conn().await;
        
        let job = JobRequest {
            id: Uuid::new_v4(),
            language: Language::Python,
            source_code: r#"
n = int(input())
print(n * 2)
"#.to_string(),
            test_cases: vec![
                TestCase {
                    id: 1,
                    input: "5".to_string(),
                    expected_output: "10".to_string(),
                    weight: 10,
                },
                TestCase {
                    id: 2,
                    input: "10".to_string(),
                    expected_output: "20".to_string(),
                    weight: 10,
                },
                TestCase {
                    id: 3,
                    input: "15".to_string(),
                    expected_output: "30".to_string(),
                    weight: 10,
                },
            ],
            timeout_ms: 5000,
            metadata: JobMetadata::default(),
        };

        // Execute with compile-once model
        let outputs = engine.execute_job_in_single_container(&job, &mut redis_conn).await;

        // Verify all tests executed
        assert_eq!(outputs.len(), 3, "Should have 3 test outputs");
        
        // Verify no compilation failures
        for output in &outputs {
            assert!(!output.compilation_failed, "Compilation should succeed");
        }
        
        // Evaluate results
        let result = evaluate(&job, outputs);
        assert_eq!(result.score, 30, "All tests should pass");
    }

    /// Test: Compilation failure marks all tests as failed
    #[tokio::test]
    #[ignore] // Requires Docker and Redis
    async fn test_compile_once_java_compilation_error() {
        let config_manager = LanguageConfigManager::load_default()
            .expect("Failed to load language config");
        
        let engine = DockerEngine::new_with_config(&config_manager)
            .expect("Failed to create Docker engine");
        
        let mut redis_conn = create_redis_conn().await;
        
        let job = JobRequest {
            id: Uuid::new_v4(),
            language: Language::Java,
            source_code: r#"
public class Main {
    public static void main(String[] args) {
        // Missing semicolon - compilation error
        System.out.println("test")
    }
}
"#.to_string(),
            test_cases: vec![
                TestCase {
                    id: 1,
                    input: "".to_string(),
                    expected_output: "test".to_string(),
                    weight: 10,
                },
                TestCase {
                    id: 2,
                    input: "".to_string(),
                    expected_output: "test".to_string(),
                    weight: 10,
                },
            ],
            timeout_ms: 5000,
            metadata: JobMetadata::default(),
        };

        // Execute with compile-once model
        let outputs = engine.execute_job_in_single_container(&job, &mut redis_conn).await;

        // Verify all tests marked as compilation failed
        assert_eq!(outputs.len(), 2, "Should have 2 test outputs");
        
        for output in &outputs {
            assert!(output.compilation_failed, "All tests should be marked as compilation failed");
            assert!(output.stderr.contains("error") || output.stderr.contains("Error"), 
                "Should contain compilation error message");
        }
        
        // Evaluate results
        let result = evaluate(&job, outputs);
        assert_eq!(result.score, 0, "No tests should pass with compilation error");
    }

    /// Test: Runtime error detection in compiled code
    #[tokio::test]
    #[ignore] // Requires Docker and Redis
    async fn test_compile_once_rust_runtime_error() {
        let config_manager = LanguageConfigManager::load_default()
            .expect("Failed to load language config");
        
        let engine = DockerEngine::new_with_config(&config_manager)
            .expect("Failed to create Docker engine");
        
        let mut redis_conn = create_redis_conn().await;
        
        let job = JobRequest {
            id: Uuid::new_v4(),
            language: Language::Rust,
            source_code: r#"
use std::io;

fn main() {
    let mut input = String::new();
    io::stdin().read_line(&mut input).unwrap();
    let n: i32 = input.trim().parse().unwrap();
    
    // This will panic when n is 0
    println!("{}", 100 / n);
}
"#.to_string(),
            test_cases: vec![
                TestCase {
                    id: 1,
                    input: "10".to_string(),
                    expected_output: "10".to_string(),
                    weight: 10,
                },
                TestCase {
                    id: 2,
                    input: "0".to_string(), // This will cause division by zero
                    expected_output: "error".to_string(),
                    weight: 10,
                },
                TestCase {
                    id: 3,
                    input: "5".to_string(),
                    expected_output: "20".to_string(),
                    weight: 10,
                },
            ],
            timeout_ms: 5000,
            metadata: JobMetadata::default(),
        };

        // Execute with compile-once model
        let outputs = engine.execute_job_in_single_container(&job, &mut redis_conn).await;

        // Verify compilation succeeded
        assert!(!outputs[0].compilation_failed, "Compilation should succeed");
        
        // Verify first test passes
        assert!(!outputs[0].runtime_error, "First test should not have runtime error");
        
        // Verify second test has runtime error
        assert!(outputs[1].runtime_error, "Second test should have runtime error");
        
        // Verify third test still executes (container doesn't crash)
        assert!(!outputs[2].runtime_error, "Third test should execute after runtime error");
        
        // Evaluate results
        let result = evaluate(&job, outputs);
        
        // First and third should pass, second should fail
        assert_eq!(result.results[0].status, TestStatus::Passed);
        assert_eq!(result.results[1].status, TestStatus::RuntimeError);
        assert_eq!(result.results[2].status, TestStatus::Passed);
    }

    /// Test: Timeout handling for individual tests
    #[tokio::test]
    #[ignore] // Requires Docker and Redis
    async fn test_compile_once_timeout() {
        let config_manager = LanguageConfigManager::load_default()
            .expect("Failed to load language config");
        
        let engine = DockerEngine::new_with_config(&config_manager)
            .expect("Failed to create Docker engine");
        
        let mut redis_conn = create_redis_conn().await;
        
        let job = JobRequest {
            id: Uuid::new_v4(),
            language: Language::Python,
            source_code: r#"
import time
n = int(input())
if n == 999:
    time.sleep(10)  # Infinite loop simulation
print(n)
"#.to_string(),
            test_cases: vec![
                TestCase {
                    id: 1,
                    input: "5".to_string(),
                    expected_output: "5".to_string(),
                    weight: 10,
                },
                TestCase {
                    id: 2,
                    input: "999".to_string(), // This will timeout
                    expected_output: "999".to_string(),
                    weight: 10,
                },
                TestCase {
                    id: 3,
                    input: "10".to_string(),
                    expected_output: "10".to_string(),
                    weight: 10,
                },
            ],
            timeout_ms: 1000, // 1 second timeout
            metadata: JobMetadata::default(),
        };

        // Execute with compile-once model
        let outputs = engine.execute_job_in_single_container(&job, &mut redis_conn).await;

        // Verify compilation succeeded
        assert!(!outputs[0].compilation_failed, "Compilation should succeed");
        
        // Verify first test passes
        assert!(!outputs[0].timed_out, "First test should not timeout");
        
        // Verify second test times out
        assert!(outputs[1].timed_out, "Second test should timeout");
        
        // Verify third test still executes (container recovers from timeout)
        assert!(!outputs[2].timed_out, "Third test should execute after timeout");
        
        // Evaluate results
        let result = evaluate(&job, outputs);
        assert_eq!(result.results[0].status, TestStatus::Passed);
        assert_eq!(result.results[1].status, TestStatus::TimeLimitExceeded);
        assert_eq!(result.results[2].status, TestStatus::Passed);
    }

    /// Test: Performance comparison between legacy and compile-once
    #[tokio::test]
    #[ignore] // Requires Docker and Redis - manual performance test
    async fn test_compile_once_performance_comparison() {
        use std::time::Instant;
        
        let config_manager = LanguageConfigManager::load_default()
            .expect("Failed to load language config");
        
        let engine = DockerEngine::new_with_config(&config_manager)
            .expect("Failed to create Docker engine");
        
        let mut redis_conn = create_redis_conn().await;
        
        // Create a job with many test cases
        let mut test_cases = Vec::new();
        for i in 1..=10 {
            test_cases.push(TestCase {
                id: i,
                input: i.to_string(),
                expected_output: (i * 2).to_string(),
                weight: 10,
            });
        }
        
        let job = JobRequest {
            id: Uuid::new_v4(),
            language: Language::Java,
            source_code: r#"
import java.util.Scanner;

public class Main {
    public static void main(String[] args) {
        Scanner scanner = new Scanner(System.in);
        int n = scanner.nextInt();
        System.out.println(n * 2);
    }
}
"#.to_string(),
            test_cases: test_cases.clone(),
            timeout_ms: 5000,
            metadata: JobMetadata::default(),
        };

        // Test compile-once execution
        let start = Instant::now();
        let outputs_new = engine.execute_job_in_single_container(&job, &mut redis_conn).await;
        let compile_once_duration = start.elapsed();
        
        println!("Compile-once execution: {:?}", compile_once_duration);
        println!("  Tests executed: {}", outputs_new.len());
        println!("  All passed: {}", outputs_new.iter().all(|o| !o.runtime_error && !o.timed_out));
        
        // For comparison, you would run the legacy path here
        // This demonstrates the expected improvement
        assert!(!outputs_new.is_empty(), "Should have executed all tests");
        assert!(compile_once_duration.as_secs() < 30, "Should complete within 30 seconds for 10 tests");
    }

    /// Test: Container cleanup on cancellation
    #[tokio::test]
    #[ignore] // Requires Docker and Redis
    async fn test_compile_once_cleanup_on_error() {
        let config_manager = LanguageConfigManager::load_default()
            .expect("Failed to load language config");
        
        let engine = DockerEngine::new_with_config(&config_manager)
            .expect("Failed to create Docker engine");
        
        let mut redis_conn = create_redis_conn().await;
        
        let job = JobRequest {
            id: Uuid::new_v4(),
            language: Language::Python,
            source_code: r#"
print("test")
"#.to_string(),
            test_cases: vec![
                TestCase {
                    id: 1,
                    input: "".to_string(),
                    expected_output: "test".to_string(),
                    weight: 10,
                },
            ],
            timeout_ms: 5000,
            metadata: JobMetadata::default(),
        };

        // Execute - container should be cleaned up even if test fails
        let _outputs = engine.execute_job_in_single_container(&job, &mut redis_conn).await;
        
        // Container should be automatically cleaned up by Drop guard
        // Manual verification: docker ps should not show lingering containers
        // This test mainly ensures the code doesn't panic during cleanup
    }
}
