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
/// Production uses DockerEngine with language-aware configuration.

use crate::evaluator::TestExecutionOutput;
use crate::config::LanguageConfigManager;
use optimus_common::types::{JobRequest, Language};
use bollard::{Docker, container::Config, image::CreateImageOptions, container::{CreateContainerOptions, StartContainerOptions, WaitContainerOptions, RemoveContainerOptions}};
use bollard::container::LogOutput;
use futures_util::stream::StreamExt;
use std::time::{Duration, Instant};
use anyhow::{Context, Result};
use base64::{Engine as _, engine::general_purpose};

/// Execute a complete job using DockerEngine (async version)
///
/// This function:
/// 1. Iterates through all test cases
/// 2. Calls engine.execute_in_container() for each
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
    config_manager: Option<LanguageConfigManager>,
}

impl DockerEngine {
    /// Create a new Docker engine with language config manager
    pub fn new_with_config(config_manager: &LanguageConfigManager) -> Result<Self> {
        let docker = Docker::connect_with_local_defaults()
            .context("Failed to connect to Docker daemon")?;
        
        // Clone the config manager for use in this engine
        Ok(DockerEngine { 
            docker,
            config_manager: Some(config_manager.clone()),
        })
    }

    /// Get the Docker image name for a language
    fn get_image_name(&self, language: &Language) -> String {
        // Try config manager first, fallback to hardcoded values
        if let Some(ref config) = self.config_manager {
            if let Ok(image) = config.get_image(language) {
                return image;
            }
        }
        
        // Fallback to hardcoded defaults
        match language {
            Language::Python => "optimus-python:latest".to_string(),
            Language::Java => "optimus-java:latest".to_string(),
            Language::Rust => "optimus-rust:latest".to_string(),
        }
    }

    /// Get the execution command for a language
    fn get_execution_command(&self, language: &Language) -> Vec<String> {
        // Use the runner script from the Docker image
        // The runner handles decoding SOURCE_CODE and TEST_INPUT env vars
        match language {
            Language::Python => vec!["python".to_string(), "/runner.py".to_string()],
            Language::Java => vec!["java".to_string(), "-cp".to_string(), "/".to_string(), "Runner".to_string()],
            Language::Rust => vec!["rust".to_string(), "/runner.sh".to_string()],
        }
    }

    /// Old implementation - kept for reference
    fn _get_execution_command_old(&self, language: &Language) -> Vec<String> {
        // Try config manager first, fallback to hardcoded values
        if let Some(ref config) = self.config_manager {
            if let Ok(cmd) = config.get_command(language) {
                // Add file path to command
                let file_ext = config.get_file_extension(language)
                    .unwrap_or_else(|_| {
                        // Fallback file extensions
                        match language {
                            Language::Python => ".py".to_string(),
                            Language::Java => ".java".to_string(),
                            Language::Rust => ".rs".to_string(),
                        }
                    });
                let filename = format!("/code/main{}", file_ext);
                
                let mut full_cmd = cmd;
                // For Python, add -u and filename
                if matches!(language, Language::Python) {
                    full_cmd.push(filename);
                } else {
                    // For other languages, append filename if needed
                    full_cmd.push(filename);
                }
                return full_cmd;
            }
        }
        
        // Fallback to hardcoded defaults
        match language {
            Language::Python => vec!["python".to_string(), "-u".to_string(), "/code/main.py".to_string()],
            Language::Java => vec!["java".to_string(), "Main".to_string()],
            Language::Rust => vec!["/code/main".to_string()],
        }
    }

    /// Get memory limit for a language
    fn get_memory_limit(&self, language: &Language) -> i64 {
        if let Some(ref config) = self.config_manager {
            if let Ok(limit_mb) = config.get_memory_limit_mb(language) {
                return (limit_mb as i64) * 1024 * 1024;
            }
        }
        256 * 1024 * 1024 // Default: 256MB
    }

    /// Get CPU limit for a language
    fn get_cpu_limit(&self, language: &Language) -> i64 {
        if let Some(ref config) = self.config_manager {
            if let Ok(limit) = config.get_cpu_limit(language) {
                return (limit * 1_000_000_000.0) as i64;
            }
        }
        500_000_000 // Default: 0.5 CPU
    }

    /// Ensure Docker image is available (pull if needed)
    async fn ensure_image(&self, image: &str) -> Result<()> {
        // First check if image exists locally
        let inspect_result = self.docker.inspect_image(image).await;
        
        if inspect_result.is_ok() {
            // Image exists locally, no need to pull
            return Ok(());
        }

        // Image doesn't exist, try to pull it
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
        let image = self.get_image_name(language);
        let container_name = format!("optimus-{}", uuid::Uuid::new_v4());

        // Ensure image is available
        self.ensure_image(&image).await?;

        // Prepare environment and command
        let cmd = self.get_execution_command(language);
        
        // Create container configuration
        let env = vec![
            format!("SOURCE_CODE={}", general_purpose::STANDARD.encode(source_code)),
            format!("TEST_INPUT={}", general_purpose::STANDARD.encode(input)),
        ];

        // Get resource limits from config
        let memory_limit = self.get_memory_limit(language);
        let cpu_limit = self.get_cpu_limit(language);

        let config = Config {
            image: Some(image.clone()),
            cmd: Some(cmd),
            env: Some(env),
            attach_stdout: Some(true),
            attach_stderr: Some(true),
            network_disabled: Some(true),
            host_config: Some(bollard::models::HostConfig {
                memory: Some(memory_limit),
                nano_cpus: Some(cpu_limit),
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

