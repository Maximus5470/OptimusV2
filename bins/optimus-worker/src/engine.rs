/// Execution Engine - Abstraction for Code Execution
///
/// **Core Responsibility:**
/// Execute source code with test inputs and capture raw outputs.
///
/// **Critical Architectural Boundary:**
/// - Engine knows HOW to execute (Docker, local, sandbox, etc.)
/// - Engine does NOT know scoring rules
/// - Engine does NOT evaluate correctness
/// - Engine returns raw outputs for Evaluator to judge
///
/// **Why This Exists:**
/// Enables swappable execution backends without touching scoring logic.
/// DummyEngine → DockerEngine → K8s Engine → Lambda Engine (all compatible)

use crate::evaluator::TestExecutionOutput;
use optimus_common::types::{JobRequest, Language};
use bollard::{Docker, container::Config, image::CreateImageOptions, container::{CreateContainerOptions, StartContainerOptions, WaitContainerOptions, RemoveContainerOptions}};
use bollard::container::LogOutput;
use futures_util::stream::StreamExt;
use std::time::{Duration, Instant};
use anyhow::{Context, Result};
use base64::{Engine as _, engine::general_purpose};

/// Execution engine trait
/// 
/// Any implementation must guarantee:
/// 1. Execute source_code with given input
/// 2. Respect timeout_ms
/// 3. Capture stdout/stderr
/// 4. Report timing information
/// 5. Flag timeouts and runtime errors
pub trait ExecutionEngine {
    /// Execute code for a single test case
    ///
    /// ## Arguments
    /// * `source_code` - The source code to execute
    /// * `input` - The stdin input for this test case
    /// * `timeout_ms` - Maximum execution time
    ///
    /// ## Returns
    /// Raw execution output (stdout, stderr, timing, error flags)
    fn execute(
        &self,
        source_code: &str,
        input: &str,
        timeout_ms: u64,
    ) -> TestExecutionOutput;
}

/// Dummy execution engine for testing and validation
///
/// **Dummy Execution Rules:**
/// 1. Treats source_code as ignored
/// 2. stdout = input.trim() (echo semantics)
/// 3. Never times out
/// 4. Never has runtime errors
/// 5. Fixed execution time: 5ms
///
/// **Purpose:**
/// Validate architecture, scoring, and result aggregation
/// before introducing Docker complexity.
pub struct DummyEngine;

impl DummyEngine {
    pub fn new() -> Self {
        DummyEngine
    }
}

impl Default for DummyEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl ExecutionEngine for DummyEngine {
    fn execute(
        &self,
        _source_code: &str,
        input: &str,
        _timeout_ms: u64,
    ) -> TestExecutionOutput {
        // Dummy execution: stdout = input (trimmed)
        let stdout = input.trim().to_string();

        TestExecutionOutput {
            test_id: 0, // Will be set by executor
            stdout,
            stderr: String::new(),
            execution_time_ms: 5, // Fixed dummy time
            timed_out: false,
            runtime_error: false,
        }
    }
}

/// Execute a complete job using the provided execution engine
///
/// This function:
/// 1. Iterates through all test cases
/// 2. Calls engine.execute() for each
/// 3. Collects raw outputs
/// 4. Returns outputs for Evaluator
///
/// ## Arguments
/// * `job` - The job to execute
/// * `engine` - The execution engine to use
///
/// ## Returns
/// Vector of raw execution outputs (one per test case)
pub fn execute_job<E: ExecutionEngine>(
    job: &JobRequest,
    engine: &E,
) -> Vec<TestExecutionOutput> {
    let mut outputs = Vec::new();

    println!("→ Executing {} test cases", job.test_cases.len());
    println!("  Timeout per test: {}ms", job.timeout_ms);
    println!();

    for test_case in &job.test_cases {
        println!("  Executing test {} (id: {})", outputs.len() + 1, test_case.id);

        // Execute with engine
        let mut output = engine.execute(
            &job.source_code,
            &test_case.input,
            job.timeout_ms,
        );

        // Set correct test_id
        output.test_id = test_case.id;

        println!("    Execution time: {}ms", output.execution_time_ms);
        if output.timed_out {
            println!("    ⚠ Timed out");
        }
        if output.runtime_error {
            println!("    ✗ Runtime error");
        }

        outputs.push(output);
    }

    println!();
    println!("→ All test cases executed");

    outputs
}

/// Execute a complete job using DockerEngine (async version)
///
/// This function:
/// 1. Iterates through all test cases
/// 2. Calls engine.execute_async() for each
/// 3. Collects raw outputs
/// 4. Returns outputs for Evaluator
///
/// ## Arguments
/// * `job` - The job to execute
/// * `engine` - The Docker execution engine to use
///
/// ## Returns
/// Vector of raw execution outputs (one per test case)
pub async fn execute_job_async(
    job: &JobRequest,
    engine: &DockerEngine,
) -> Vec<TestExecutionOutput> {
    let mut outputs = Vec::new();

    println!("→ Executing {} test cases with Docker", job.test_cases.len());
    println!("  Language: {}", job.language);
    println!("  Timeout per test: {}ms", job.timeout_ms);
    println!();

    for test_case in &job.test_cases {
        println!("  Executing test {} (id: {})", outputs.len() + 1, test_case.id);

        // Execute with Docker engine
        let result = engine.execute_in_container(
            &job.language,
            &job.source_code,
            &test_case.input,
            job.timeout_ms,
        ).await;

        let mut output = match result {
            Ok(output) => output,
            Err(e) => {
                eprintln!("    ✗ Docker execution error: {}", e);
                TestExecutionOutput {
                    test_id: test_case.id,
                    stdout: String::new(),
                    stderr: format!("Docker execution error: {}", e),
                    execution_time_ms: 0,
                    timed_out: false,
                    runtime_error: true,
                }
            }
        };

        // Set correct test_id
        output.test_id = test_case.id;

        println!("    Execution time: {}ms", output.execution_time_ms);
        if output.timed_out {
            println!("    ⚠ Timed out");
        }
        if output.runtime_error {
            println!("    ✗ Runtime error");
        }
        if !output.stderr.is_empty() {
            println!("    stderr: {}", output.stderr.lines().next().unwrap_or(""));
        }

        outputs.push(output);
    }

    println!();
    println!("→ All test cases executed");

    outputs
}

#[cfg(test)]
mod tests {
    use super::*;
    use optimus_common::types::{Language, TestCase};
    use uuid::Uuid;

    #[test]
    fn test_dummy_engine_echo() {
        let engine = DummyEngine::new();

        let output = engine.execute("ignored code", "hello world", 5000);

        assert_eq!(output.stdout, "hello world");
        assert_eq!(output.stderr, "");
        assert_eq!(output.execution_time_ms, 5);
        assert!(!output.timed_out);
        assert!(!output.runtime_error);
    }

    #[test]
    fn test_dummy_engine_trims_input() {
        let engine = DummyEngine::new();

        let output = engine.execute("code", "  test  \n", 1000);

        assert_eq!(output.stdout, "test");
    }

    #[test]
    fn test_execute_job_multiple_tests() {
        let job = JobRequest {
            id: Uuid::new_v4(),
            language: Language::Python,
            source_code: "# dummy".to_string(),
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
                    weight: 15,
                },
            ],
            timeout_ms: 5000,
        };

        let engine = DummyEngine::new();
        let outputs = execute_job(&job, &engine);

        assert_eq!(outputs.len(), 2);
        assert_eq!(outputs[0].test_id, 1);
        assert_eq!(outputs[0].stdout, "input1");
        assert_eq!(outputs[1].test_id, 2);
        assert_eq!(outputs[1].stdout, "input2");
    }

    #[test]
    fn test_execute_job_preserves_test_order() {
        let job = JobRequest {
            id: Uuid::new_v4(),
            language: Language::Java,
            source_code: String::new(),
            test_cases: vec![
                TestCase {
                    id: 5,
                    input: "a".to_string(),
                    expected_output: "x".to_string(),
                    weight: 1,
                },
                TestCase {
                    id: 3,
                    input: "b".to_string(),
                    expected_output: "y".to_string(),
                    weight: 1,
                },
                TestCase {
                    id: 7,
                    input: "c".to_string(),
                    expected_output: "z".to_string(),
                    weight: 1,
                },
            ],
            timeout_ms: 1000,
        };

        let engine = DummyEngine::new();
        let outputs = execute_job(&job, &engine);

        assert_eq!(outputs[0].test_id, 5);
        assert_eq!(outputs[1].test_id, 3);
        assert_eq!(outputs[2].test_id, 7);
    }
}

/// Docker-based execution engine for real sandboxed code execution
///
/// **Docker Execution Rules:**
/// 1. Pulls language-specific Docker image if not present
/// 2. Creates container with security constraints:
///    - Network disabled
///    - CPU/memory limits enforced
///    - Read-only filesystem (where possible)
/// 3. Injects source code and test input
/// 4. Captures stdout/stderr streams
/// 5. Measures execution time
/// 6. Handles timeouts and runtime errors
/// 7. Cleans up container after execution
///
/// **Purpose:**
/// Production-grade sandboxed execution with resource isolation
pub struct DockerEngine {
    docker: Docker,
}

impl DockerEngine {
    /// Create a new Docker engine instance
    pub fn new() -> Result<Self> {
        let docker = Docker::connect_with_local_defaults()
            .context("Failed to connect to Docker daemon")?;
        Ok(DockerEngine { docker })
    }

    /// Get the Docker image name for a language
    fn get_image_name(language: &Language) -> &'static str {
        match language {
            Language::Python => "optimus-python:latest",
            Language::Java => "optimus-java:latest",
            Language::Rust => "optimus-rust:latest",
        }
    }

    /// Get the execution command for a language
    fn get_execution_command(language: &Language) -> Vec<String> {
        match language {
            Language::Python => vec!["python".to_string(), "-u".to_string(), "/code/main.py".to_string()],
            Language::Java => vec!["java".to_string(), "Main".to_string()],
            Language::Rust => vec!["/code/main".to_string()],
        }
    }

    /// Get the file name for source code
    fn get_source_filename(language: &Language) -> &'static str {
        match language {
            Language::Python => "main.py",
            Language::Java => "Main.java",
            Language::Rust => "main.rs",
        }
    }

    /// Ensure Docker image is available (pull if needed)
    async fn ensure_image(&self, image: &str) -> Result<()> {
        let options = Some(CreateImageOptions {
            from_image: image,
            ..Default::default()
        });

        let mut stream = self.docker.create_image(options, None, None);
        
        while let Some(result) = stream.next().await {
            result.context("Failed to pull Docker image")?;
        }

        Ok(())
    }

    /// Execute code in Docker container
    pub async fn execute_in_container(
        &self,
        language: &Language,
        source_code: &str,
        input: &str,
        timeout_ms: u64,
    ) -> Result<TestExecutionOutput> {
        let image = Self::get_image_name(language);
        let container_name = format!("optimus-{}", uuid::Uuid::new_v4());

        // Ensure image is available
        self.ensure_image(image).await?;

        // Prepare environment and command
        let cmd = Self::get_execution_command(language);
        
        // Create container configuration
        let env = vec![
            format!("SOURCE_CODE={}", general_purpose::STANDARD.encode(source_code)),
            format!("TEST_INPUT={}", general_purpose::STANDARD.encode(input)),
        ];

        let config = Config {
            image: Some(image.to_string()),
            cmd: Some(cmd),
            env: Some(env),
            attach_stdout: Some(true),
            attach_stderr: Some(true),
            network_disabled: Some(true),
            host_config: Some(bollard::models::HostConfig {
                memory: Some(256 * 1024 * 1024), // 256MB
                nano_cpus: Some(500_000_000), // 0.5 CPU
                readonly_rootfs: Some(false), // Allow writes to /tmp
                ..Default::default()
            }),
            ..Default::default()
        };

        // Create container
        let create_options = CreateContainerOptions {
            name: container_name.as_str(),
            platform: None,
        };

        let container = self.docker
            .create_container(Some(create_options), config)
            .await
            .context("Failed to create container")?;

        let container_id = container.id;

        // Start execution timer
        let start_time = Instant::now();

        // Start container
        self.docker
            .start_container(&container_id, None::<StartContainerOptions<String>>)
            .await
            .context("Failed to start container")?;

        let mut stdout = String::new();
        let mut stderr = String::new();
        let mut timed_out = false;
        let mut runtime_error = false;

        // Collect output with timeout using wait API
        let timeout_duration = Duration::from_millis(timeout_ms);
        let output_future = async {
            // Wait for container to complete and get logs
            let logs_options = Some(bollard::container::LogsOptions::<String> {
                stdout: true,
                stderr: true,
                follow: true,
                ..Default::default()
            });
            
            let mut logs_stream = self.docker.logs(&container_id, logs_options);
            while let Some(output) = logs_stream.next().await {
                match output {
                    Ok(LogOutput::StdOut { message }) => {
                        stdout.push_str(&String::from_utf8_lossy(&message));
                    }
                    Ok(LogOutput::StdErr { message }) => {
                        stderr.push_str(&String::from_utf8_lossy(&message));
                    }
                    Err(_) => break,
                    _ => {}
                }
            }
        };

        // Wait for container with timeout
        let wait_result = tokio::time::timeout(timeout_duration, output_future).await;

        if wait_result.is_err() {
            timed_out = true;
            // Kill container if timeout
            let _ = self.docker
                .kill_container(&container_id, None::<bollard::container::KillContainerOptions<String>>)
                .await;
        }

        // Check container exit code
        let wait_options = WaitContainerOptions {
            condition: "not-running",
        };
        
        let mut wait_stream = self.docker.wait_container(&container_id, Some(wait_options));
        if let Some(wait_result) = wait_stream.next().await {
            if let Ok(response) = wait_result {
                if response.status_code != 0 && !timed_out {
                    runtime_error = true;
                }
            }
        }

        let execution_time_ms = start_time.elapsed().as_millis() as u64;

        // Clean up container
        let remove_options = RemoveContainerOptions {
            force: true,
            ..Default::default()
        };
        
        let _ = self.docker
            .remove_container(&container_id, Some(remove_options))
            .await;

        Ok(TestExecutionOutput {
            test_id: 0, // Will be set by executor
            stdout,
            stderr,
            execution_time_ms,
            timed_out,
            runtime_error,
        })
    }
}

impl ExecutionEngine for DockerEngine {
    fn execute(
        &self,
        source_code: &str,
        input: &str,
        timeout_ms: u64,
    ) -> TestExecutionOutput {
        // Create a runtime for async execution
        // Note: In production, this should be handled by the worker's main runtime
        let rt = tokio::runtime::Runtime::new().unwrap();
        
        rt.block_on(async {
            // For now, we'll assume Python. In production, language should be passed
            self.execute_in_container(&Language::Python, source_code, input, timeout_ms)
                .await
                .unwrap_or_else(|e| {
                    eprintln!("Docker execution error: {}", e);
                    TestExecutionOutput {
                        test_id: 0,
                        stdout: String::new(),
                        stderr: format!("Docker execution error: {}", e),
                        execution_time_ms: 0,
                        timed_out: false,
                        runtime_error: true,
                    }
                })
        })
    }
}

