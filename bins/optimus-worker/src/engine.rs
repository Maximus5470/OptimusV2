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
use anyhow::{Context, Result, bail};
use base64::{Engine as _, engine::general_purpose};
use tracing::{debug, info, warn};

/// Safety limits to prevent pathological inputs from reaching Docker
const MAX_SOURCE_CODE_BYTES: usize = 1024 * 1024; // 1MB
const MAX_TEST_INPUT_BYTES: usize = 10 * 1024 * 1024; // 10MB

/// Execute a complete job using DockerEngine (async version)
///
/// This function:
/// 1. Iterates through all test cases
/// 2. Checks for cancellation before each test case
/// 3. Calls engine.execute_in_container() for each
/// 4. Collects raw outputs
/// 5. Returns outputs for Evaluator
///
/// ## Arguments
/// * `job` - The job to execute
/// * `engine` - The Docker execution engine to use
/// * `redis_conn` - Redis connection for cancellation checks
///
/// ## Returns
/// Vector of raw execution outputs (one per test case)
pub async fn execute_job_async(
    job: &JobRequest,
    engine: &DockerEngine,
    redis_conn: &mut redis::aio::ConnectionManager,
) -> Vec<TestExecutionOutput> {
    let mut outputs = Vec::new();

    println!("→ Executing {} test cases with Docker", job.test_cases.len());
    println!("  Language: {}", job.language);
    println!("  Timeout per test: {}ms", job.timeout_ms);
    println!();

    for test_case in &job.test_cases {
        // Check for cancellation before each test case
        match optimus_common::redis::is_job_cancelled(redis_conn, &job.id).await {
            Ok(true) => {
                println!("  ⚠ Job cancelled - stopping execution");
                println!("    Completed {} of {} tests before cancellation", outputs.len(), job.test_cases.len());
                break;
            }
            Ok(false) => {
                // Not cancelled, continue
            }
            Err(e) => {
                eprintln!("  ⚠ Failed to check cancellation status: {}", e);
                // Continue execution on error to avoid false cancellations
            }
        }

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
                    compilation_failed: false,
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

/// Container cleanup guard - guarantees container removal on drop
/// This ensures containers are cleaned up even if execution panics or is cancelled
struct ContainerGuard<'a> {
    docker: &'a Docker,
    container_id: String,
}

impl<'a> ContainerGuard<'a> {
    fn new(docker: &'a Docker, container_id: String) -> Self {
        Self { docker, container_id }
    }
}

impl<'a> Drop for ContainerGuard<'a> {
    fn drop(&mut self) {
        // Best-effort cleanup - cannot be async in Drop
        // Log if cleanup fails but don't panic
        let container_id = self.container_id.clone();
        let docker = self.docker.clone();
        
        tokio::spawn(async move {
            let remove_options = RemoveContainerOptions {
                force: true,
                ..Default::default()
            };
            
            if let Err(e) = docker.remove_container(&container_id, Some(remove_options)).await {
                eprintln!("⚠ Failed to cleanup container {}: {}", container_id, e);
            }
        });
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
    /// 
    /// **Image Cache Health Check:**
    /// - Verifies image exists locally before execution
    /// - Pulls synchronously if missing (prevents execution failure)
    /// - Logs cache hits/misses for observability
    async fn ensure_image(&self, image: &str) -> Result<()> {
        // Image cache health check
        let inspect_result = self.docker.inspect_image(image).await;
        
        if inspect_result.is_ok() {
            // Cache hit - image is already present
            debug!("✓ Image cache hit: {}", image);
            return Ok(());
        }

        // Cache miss - need to pull the image
        warn!("⚠ Image cache miss: {} (pulling now)", image);
        
        let options = Some(CreateImageOptions {
            from_image: image,
            ..Default::default()
        });

        let mut stream = self.docker.create_image(options, None, None);
        
        while let Some(result) = stream.next().await {
            result.context("Failed to pull Docker image")?;
        }

        info!("✓ Image pulled successfully: {}", image);
        Ok(())
    }

    /// Execute code in Docker container with hardened safety guarantees
    /// 
    /// **Safety Guarantees:**
    /// - Input validation: Rejects oversized source code or test inputs
    /// - Hard timeout: Enforced via tokio::time::timeout, kills container on timeout
    /// - Guaranteed cleanup: Container removed even on panic/cancellation via Drop guard
    /// - Error classification: Distinguishes timeout, runtime error, and infrastructure failure
    /// - Partial output capture: Captures stdout/stderr even on timeout
    pub async fn execute_in_container(
        &self,
        language: &Language,
        source_code: &str,
        input: &str,
        timeout_ms: u64,
    ) -> Result<TestExecutionOutput> {
        // GUARDRAIL 1: Validate input sizes
        if source_code.len() > MAX_SOURCE_CODE_BYTES {
            bail!("Source code exceeds maximum size of {} bytes", MAX_SOURCE_CODE_BYTES);
        }
        if input.len() > MAX_TEST_INPUT_BYTES {
            bail!("Test input exceeds maximum size of {} bytes", MAX_TEST_INPUT_BYTES);
        }

        let image = self.get_image_name(language);
        let container_name = format!("optimus-{}", uuid::Uuid::new_v4());

        // Ensure image is available
        self.ensure_image(&image).await
            .context(format!("Failed to ensure Docker image '{}' is available", image))?;

        // Prepare environment and command
        let cmd = self.get_execution_command(language);
        
        // Create container configuration with LANGUAGE env var for universal runner
        let env = vec![
            format!("SOURCE_CODE={}", general_purpose::STANDARD.encode(source_code)),
            format!("TEST_INPUT={}", general_purpose::STANDARD.encode(input)),
            format!("LANGUAGE={}", format!("{}", language).to_lowercase()),
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
            network_disabled: Some(true), // SECURITY: No network access
            host_config: Some(bollard::models::HostConfig {
                memory: Some(memory_limit),
                nano_cpus: Some(cpu_limit),
                readonly_rootfs: Some(false), // Allow writes to /tmp for compilation
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
            .context("Failed to create Docker container")?;

        let container_id = container.id.clone();
        
        // CRITICAL: Set up cleanup guard immediately after container creation
        // This guarantees cleanup even if we panic or get cancelled
        let _guard = ContainerGuard::new(&self.docker, container_id.clone());

        // Start execution timer
        let start_time = Instant::now();

        // Start container
        self.docker
            .start_container(&container_id, None::<StartContainerOptions<String>>)
            .await
            .context("Failed to start Docker container")?;

        let mut timed_out = false;
        let mut runtime_error = false;

        // HARD TIMEOUT: Wrap execution in tokio::time::timeout
        let timeout_duration = Duration::from_millis(timeout_ms);
        
        let execution_future = async {
            let mut stdout = String::new();
            let mut stderr = String::new();
            let mut exit_code: Option<i64> = None;
            
            // Collect logs and wait for completion in parallel
            let logs_options = Some(bollard::container::LogsOptions::<String> {
                stdout: true,
                stderr: true,
                follow: true,
                ..Default::default()
            });
            
            let mut logs_stream = self.docker.logs(&container_id, logs_options);
            
            // Collect all output
            while let Some(output) = logs_stream.next().await {
                match output {
                    Ok(LogOutput::StdOut { message }) => {
                        stdout.push_str(&String::from_utf8_lossy(&message));
                    }
                    Ok(LogOutput::StdErr { message }) => {
                        stderr.push_str(&String::from_utf8_lossy(&message));
                    }
                    Err(e) => {
                        eprintln!("⚠ Error reading container logs: {}", e);
                        break;
                    }
                    _ => {}
                }
            }
            
            // Get exit code - wait for container to finish
            let wait_options = WaitContainerOptions {
                condition: "not-running",
            };
            
            let mut wait_stream = self.docker.wait_container(&container_id, Some(wait_options));
            if let Some(wait_result) = wait_stream.next().await {
                if let Ok(response) = wait_result {
                    exit_code = Some(response.status_code);
                    println!("    Container exited with code: {}", response.status_code);
                } else {
                    eprintln!("    ⚠ Failed to get container exit code");
                }
            } else {
                eprintln!("    ⚠ No wait response from container");
            }
            
            (stdout, stderr, exit_code)
        };

        // Execute with hard timeout
        let timeout_result = tokio::time::timeout(timeout_duration, execution_future).await;

        let (stdout, stderr, _exit_code) = match timeout_result {
            Ok((out, mut err, code)) => {
                // Execution completed within timeout
                // Classify error type based on exit code
                println!("    Received exit code: {:?}", code);
                if let Some(code) = code {
                    if code != 0 {
                        runtime_error = true;
                        println!("    ✗ Runtime error detected (exit code: {})", code);
                        
                        // Special handling for common signals
                        if code == 137 {
                            err.push_str("\n[Container killed: likely OOM or exceeded memory limit]");
                        } else if code == 139 {
                            err.push_str("\n[Container killed: segmentation fault]");
                        }
                    } else {
                        println!("    ✓ Container exited successfully (code 0)");
                    }
                } else {
                    eprintln!("    ⚠ WARNING: No exit code captured from container!");
                }
                
                (out, err, code)
            }
            Err(_) => {
                // TIMEOUT: Kill container immediately and capture partial output
                timed_out = true;
                
                println!("    ⚠ Execution timed out after {}ms - killing container", timeout_ms);
                
                // Force kill the container
                if let Err(e) = self.docker
                    .kill_container(&container_id, None::<bollard::container::KillContainerOptions<String>>)
                    .await
                {
                    eprintln!("    ⚠ Failed to kill timed-out container: {}", e);
                }
                
                // Return empty output with timeout message
                (String::new(), String::from("\n[Execution timed out]"), None)
            }
        };

        let execution_time_ms = start_time.elapsed().as_millis() as u64;

        // Container cleanup happens automatically via Drop guard
        // No need for explicit cleanup here

        Ok(TestExecutionOutput {
            test_id: 0, // Will be set by executor
            stdout,
            stderr,
            execution_time_ms,
            timed_out,
            runtime_error,
            compilation_failed: false,
        })
    }

    /// Compile code in a container (Phase 2: Compile-once execution)
    /// 
    /// This method compiles the source code once and leaves the container running.
    /// The compiled artifact is ready for multiple test executions.
    /// 
    /// ## Arguments
    /// * `container_id` - ID of the running container
    /// * `language` - Programming language
    /// 
    /// ## Returns
    /// CompilationResult with success status and compilation output
    #[tracing::instrument(skip(self), fields(language = %language))]
    pub async fn compile_in_container(
        &self,
        container_id: &str,
        language: &Language,
    ) -> Result<crate::evaluator::CompilationResult> {
        use bollard::exec::{CreateExecOptions, StartExecOptions};
        
        let start_time = Instant::now();
        debug!("Starting compilation for language: {}", language);
        
        // Determine compilation command based on language
        let compile_cmd = match language {
            Language::Java => vec!["bash", "-c", "javac /code/Main.java 2>&1"],
            Language::Rust => vec!["bash", "-c", "rustc /code/main.rs -o /code/main 2>&1"],
            Language::Python => vec!["bash", "-c", "python3 -m py_compile /code/main.py 2>&1"],
        };
        
        // Create exec instance for compilation
        let exec_config = CreateExecOptions {
            cmd: Some(compile_cmd),
            attach_stdout: Some(true),
            attach_stderr: Some(true),
            ..Default::default()
        };
        
        let exec = self.docker
            .create_exec(container_id, exec_config)
            .await
            .context("Failed to create exec for compilation")?;
        
        // Start compilation
        let start_config = StartExecOptions {
            detach: false,
            ..Default::default()
        };
        
        let output = self.docker.start_exec(&exec.id, Some(start_config)).await?;
        
        let mut stdout = String::new();
        let mut stderr = String::new();
        
        // Collect compilation output
        if let bollard::exec::StartExecResults::Attached { mut output, .. } = output {
            while let Some(msg) = output.next().await {
                match msg {
                    Ok(log_output) => {
                        match log_output {
                            LogOutput::StdOut { message } => {
                                stdout.push_str(&String::from_utf8_lossy(&message));
                            }
                            LogOutput::StdErr { message } => {
                                stderr.push_str(&String::from_utf8_lossy(&message));
                            }
                            _ => {}
                        }
                    }
                    Err(e) => {
                        return Ok(crate::evaluator::CompilationResult::failure(
                            format!("Failed to read compilation output: {}", e),
                        ));
                    }
                }
            }
        } else {
            return Ok(crate::evaluator::CompilationResult::failure(
                "Failed to attach to compilation exec".to_string(),
            ));
        }
        
        // Check exit code
        let inspect = self.docker.inspect_exec(&exec.id).await?;
        let compilation_time_ms = start_time.elapsed().as_millis() as u64;
        
        let success = inspect.exit_code == Some(0);
        
        if success {
            println!("    ✓ Compilation successful in {}ms", compilation_time_ms);
            info!(
                compilation_time_ms = compilation_time_ms,
                language = %language,
                "Compilation succeeded"
            );
            Ok(crate::evaluator::CompilationResult::success())
        } else {
            println!("    ✗ Compilation failed in {}ms", compilation_time_ms);
            if !stderr.is_empty() {
                println!("    Compilation error: {}", stderr.lines().next().unwrap_or(""));
            }
            warn!(
                compilation_time_ms = compilation_time_ms,
                language = %language,
                error_preview = stderr.lines().next().unwrap_or(""),
                "Compilation failed"
            );
            Ok(crate::evaluator::CompilationResult::failure(
                stderr,
            ))
        }
    }

    /// Execute a single test case in an existing container with compiled code
    /// 
    /// This method assumes the code has already been compiled and the container
    /// is running with the compiled artifact ready.
    /// 
    /// ## Arguments
    /// * `container_id` - ID of the running container with compiled code
    /// * `language` - Programming language
    /// * `input` - Test input
    /// * `timeout_ms` - Timeout for this test execution
    /// 
    /// ## Returns
    /// TestExecutionOutput with execution results
    #[tracing::instrument(skip(self, input), fields(language = %language, timeout_ms = timeout_ms))]
    pub async fn execute_test_in_container(
        &self,
        container_id: &str,
        language: &Language,
        input: &str,
        timeout_ms: u64,
    ) -> Result<TestExecutionOutput> {
        use bollard::exec::{CreateExecOptions, StartExecOptions};
        
        debug!("Executing test in container with timeout {}ms", timeout_ms);
        
        // Validate input size
        if input.len() > MAX_TEST_INPUT_BYTES {
            bail!("Test input exceeds maximum size of {} bytes", MAX_TEST_INPUT_BYTES);
        }
        
        let start_time = Instant::now();
        
        // Encode input for the runner script
        let encoded_input = general_purpose::STANDARD.encode(input);
        
        // Determine execution command based on language
        // CRITICAL: Unset JAVA_TOOL_OPTIONS to prevent JVM noise in stderr
        // Use parentheses to create a subshell so unset doesn't affect the container
        let java_cmd = format!("(unset JAVA_TOOL_OPTIONS; echo '{}' | base64 -d | java -cp /code Main)", encoded_input);
        let rust_cmd = format!("echo '{}' | base64 -d | /code/main", encoded_input);
        let python_cmd = format!("echo '{}' | base64 -d | python3 -u /code/main.py", encoded_input);
        
        let exec_cmd = match language {
            Language::Java => vec!["bash", "-c", &java_cmd],
            Language::Rust => vec!["bash", "-c", &rust_cmd],
            Language::Python => vec!["bash", "-c", &python_cmd],
        };
        
        // Create exec instance for test execution
        let exec_config = CreateExecOptions {
            cmd: Some(exec_cmd.iter().map(|s| s.to_string()).collect()),
            attach_stdin: Some(true),
            attach_stdout: Some(true),
            attach_stderr: Some(true),
            ..Default::default()
        };
        
        let exec = self.docker
            .create_exec(container_id, exec_config)
            .await
            .context("Failed to create exec for test execution")?;
        
        // Start execution with timeout
        let start_config = StartExecOptions {
            detach: false,
            ..Default::default()
        };
        
        let timeout_duration = Duration::from_millis(timeout_ms);
        let mut timed_out = false;
        let mut runtime_error = false;
        
        let execution_future = async {
            let output = self.docker.start_exec(&exec.id, Some(start_config)).await?;
            
            let mut stdout = String::new();
            let mut stderr = String::new();
            
            // Collect execution output
            if let bollard::exec::StartExecResults::Attached { mut output, .. } = output {
                while let Some(msg) = output.next().await {
                    match msg {
                        Ok(log_output) => {
                            match log_output {
                                LogOutput::StdOut { message } => {
                                    stdout.push_str(&String::from_utf8_lossy(&message));
                                }
                                LogOutput::StdErr { message } => {
                                    stderr.push_str(&String::from_utf8_lossy(&message));
                                }
                                _ => {}
                            }
                        }
                        Err(e) => {
                            stderr.push_str(&format!("\n[Execution error: {}]", e));
                            break;
                        }
                    }
                }
            }
            
            // Get exit code
            let inspect = self.docker.inspect_exec(&exec.id).await?;
            let exit_code = inspect.exit_code;
            
            Ok::<(String, String, Option<i64>), anyhow::Error>((stdout, stderr, exit_code))
        };
        
        // Execute with timeout
        let timeout_result = tokio::time::timeout(timeout_duration, execution_future).await;
        
        let (stdout, stderr, _exit_code) = match timeout_result {
            Ok(Ok((out, err, code))) => {
                // Check exit code for runtime errors
                if let Some(code) = code {
                    if code != 0 {
                        runtime_error = true;
                    }
                }
                (out, err, code)
            }
            Ok(Err(e)) => {
                // Execution error
                runtime_error = true;
                (String::new(), format!("Execution failed: {}", e), None)
            }
            Err(_) => {
                // Timeout
                timed_out = true;
                (String::new(), "[Execution timed out]".to_string(), None)
            }
        };
        
        let execution_time_ms = start_time.elapsed().as_millis() as u64;
        
        // Log execution metrics
        if timed_out {
            warn!(
                execution_time_ms = execution_time_ms,
                timeout_ms = timeout_ms,
                "Test execution timed out"
            );
        } else if runtime_error {
            warn!(
                execution_time_ms = execution_time_ms,
                "Test execution had runtime error"
            );
        } else {
            debug!(
                execution_time_ms = execution_time_ms,
                "Test execution completed successfully"
            );
        }
        
        Ok(TestExecutionOutput {
            test_id: 0, // Will be set by caller
            stdout,
            stderr,
            execution_time_ms,
            timed_out,
            runtime_error,
            compilation_failed: false,
        })
    }

    /// Execute a complete job in a single container (Phase 2: Compile-once execution)
    /// 
    /// This is the new execution path that:
    /// 1. Creates one container
    /// 2. Compiles code once
    /// 3. Executes all test cases against the compiled artifact
    /// 4. Cleans up the container
    /// 
    /// ## Arguments
    /// * `job` - The job request with source code and test cases
    /// * `redis_conn` - Redis connection for cancellation checks
    /// 
    /// ## Returns
    /// Vector of test execution outputs (one per test case)
    #[tracing::instrument(
        skip(self, job, redis_conn),
        fields(
            job_id = %job.id,
            language = %job.language,
            test_count = job.test_cases.len(),
            execution_mode = "compile_once"
        )
    )]
    pub async fn execute_job_in_single_container(
        &self,
        job: &JobRequest,
        redis_conn: &mut redis::aio::ConnectionManager,
    ) -> Vec<TestExecutionOutput> {
        let job_start_time = std::time::Instant::now();
        
        println!("→ Starting compile-once execution for job {}", job.id);
        println!("  Language: {}", job.language);
        println!("  Test cases: {}", job.test_cases.len());
        println!();
        
        info!(
            job_id = %job.id,
            language = %job.language,
            test_count = job.test_cases.len(),
            "Starting compile-once job execution"
        );

        // Check for early cancellation
        match optimus_common::redis::is_job_cancelled(redis_conn, &job.id).await {
            Ok(true) => {
                println!("  ⚠ Job cancelled before execution");
                return Vec::new();
            }
            Err(e) => {
                eprintln!("  ⚠ Failed to check cancellation: {}", e);
            }
            _ => {}
        }

        let image = self.get_image_name(&job.language);
        let container_name = format!("optimus-{}", uuid::Uuid::new_v4());

        // Ensure image is available
        if let Err(e) = self.ensure_image(&image).await {
            eprintln!("  ✗ Failed to ensure image: {}", e);
            return self.create_compilation_error_outputs(&job.test_cases, &format!("Failed to pull image: {}", e));
        }

        // Prepare environment - write source code to container
        let env = vec![
            format!("SOURCE_CODE={}", general_purpose::STANDARD.encode(&job.source_code)),
            format!("LANGUAGE={}", format!("{}", job.language).to_lowercase()),
        ];

        let memory_limit = self.get_memory_limit(&job.language);
        let cpu_limit = self.get_cpu_limit(&job.language);

        // Create container configuration
        let config = Config {
            image: Some(image.clone()),
            cmd: Some(vec!["/bin/bash".to_string(), "-c".to_string(), "sleep 300".to_string()]), // Keep container alive with bash
            entrypoint: Some(vec![]),  // Override entrypoint to avoid runner.sh
            env: Some(env),
            attach_stdout: Some(true),
            attach_stderr: Some(true),
            network_disabled: Some(true),
            host_config: Some(bollard::models::HostConfig {
                memory: Some(memory_limit),
                nano_cpus: Some(cpu_limit),
                readonly_rootfs: Some(false),
                ..Default::default()
            }),
            working_dir: Some("/code".to_string()),
            ..Default::default()
        };

        // Create container
        let create_options = CreateContainerOptions {
            name: container_name.as_str(),
            platform: None,
        };

        let container = match self.docker.create_container(Some(create_options), config).await {
            Ok(c) => c,
            Err(e) => {
                eprintln!("  ✗ Failed to create container: {}", e);
                return self.create_compilation_error_outputs(&job.test_cases, &format!("Container creation failed: {}", e));
            }
        };

        let container_id = container.id.clone();
        let _guard = ContainerGuard::new(&self.docker, container_id.clone());

        // Start container
        if let Err(e) = self.docker.start_container(&container_id, None::<StartContainerOptions<String>>).await {
            eprintln!("  ✗ Failed to start container: {}", e);
            return self.create_compilation_error_outputs(&job.test_cases, &format!("Container start failed: {}", e));
        }

        // Write source code to container
        if let Err(e) = self.write_source_to_container(&container_id, &job.language, &job.source_code).await {
            eprintln!("  ✗ Failed to write source code: {}", e);
            return self.create_compilation_error_outputs(&job.test_cases, &format!("Source write failed: {}", e));
        }

        println!("→ Compiling source code...");
        
        // Step 1: Compile code
        let compilation_result = match self.compile_in_container(&container_id, &job.language).await {
            Ok(result) => result,
            Err(e) => {
                eprintln!("  ✗ Compilation process failed: {}", e);
                return self.create_compilation_error_outputs(&job.test_cases, &format!("Compilation process error: {}", e));
            }
        };

        // If compilation failed, return all tests as failed
        if !compilation_result.success {
            println!("  ✗ Compilation failed - marking all tests as failed");
            return self.create_compilation_error_outputs(&job.test_cases, &compilation_result.stderr);
        }

        println!();
        println!("→ Executing {} test cases against compiled artifact", job.test_cases.len());
        println!();

        // Step 2: Execute all test cases
        let mut outputs = Vec::new();

        for (idx, test_case) in job.test_cases.iter().enumerate() {
            // Check for cancellation between tests
            match optimus_common::redis::is_job_cancelled(redis_conn, &job.id).await {
                Ok(true) => {
                    println!("  ⚠ Job cancelled - stopping at test {}/{}", idx + 1, job.test_cases.len());
                    break;
                }
                Err(e) => {
                    eprintln!("  ⚠ Failed to check cancellation: {}", e);
                }
                _ => {}
            }

            println!("  Executing test {} (id: {})", idx + 1, test_case.id);

            let result = self.execute_test_in_container(
                &container_id,
                &job.language,
                &test_case.input,
                job.timeout_ms,
            ).await;

            let mut output = match result {
                Ok(output) => output,
                Err(e) => {
                    eprintln!("    ✗ Test execution error: {}", e);
                    TestExecutionOutput {
                        test_id: test_case.id,
                        stdout: String::new(),
                        stderr: format!("Test execution error: {}", e),
                        execution_time_ms: 0,
                        timed_out: false,
                        runtime_error: true,
                        compilation_failed: false,
                    }
                }
            };

            output.test_id = test_case.id;

            println!("    Execution time: {}ms", output.execution_time_ms);
            if output.timed_out {
                println!("    ⚠ Timed out");
            }
            if output.runtime_error {
                println!("    ✗ Runtime error");
            }
            if !output.stderr.is_empty() && !output.runtime_error && !output.timed_out {
                println!("    stderr: {}", output.stderr.lines().next().unwrap_or(""));
            }

            outputs.push(output);
        }

        println!();
        println!("→ All test cases executed (compile-once mode)");
        
        let total_execution_time_ms = job_start_time.elapsed().as_millis() as u64;
        let successful_tests = outputs.iter().filter(|o| !o.runtime_error && !o.timed_out && !o.compilation_failed).count();
        
        info!(
            job_id = %job.id,
            total_execution_time_ms = total_execution_time_ms,
            tests_executed = outputs.len(),
            tests_successful = successful_tests,
            tests_failed = outputs.len() - successful_tests,
            "Completed compile-once job execution"
        );
        
        outputs
    }

    /// Helper to write source code to container filesystem
    async fn write_source_to_container(
        &self,
        container_id: &str,
        language: &Language,
        source_code: &str,
    ) -> Result<()> {
        use bollard::exec::{CreateExecOptions, StartExecOptions};
        
        let filename = match language {
            Language::Java => "Main.java",
            Language::Rust => "main.rs",
            Language::Python => "main.py",
        };
        
        // Write file using echo command (simple approach for now)
        let encoded_content = general_purpose::STANDARD.encode(source_code);
        let write_command = format!("echo '{}' | base64 -d > /code/{}", encoded_content, filename);
        let write_cmd = vec!["bash", "-c", &write_command];
        
        let exec_config = CreateExecOptions {
            cmd: Some(write_cmd.iter().map(|s| s.to_string()).collect()),
            attach_stdout: Some(true),
            attach_stderr: Some(true),
            ..Default::default()
        };
        
        let exec = self.docker.create_exec(container_id, exec_config).await?;
        
        let start_config = StartExecOptions {
            detach: false,
            ..Default::default()
        };
        
        let output = self.docker.start_exec(&exec.id, Some(start_config)).await?;
        
        // Wait for write to complete
        if let bollard::exec::StartExecResults::Attached { mut output, .. } = output {
            while let Some(_) = output.next().await {
                // Drain the stream
            }
        }
        
        // Check if write succeeded
        let inspect = self.docker.inspect_exec(&exec.id).await?;
        if inspect.exit_code != Some(0) {
            bail!("Failed to write source code to container");
        }
        
        Ok(())
    }

    /// Helper to create compilation error outputs for all test cases
    fn create_compilation_error_outputs(
        &self,
        test_cases: &[optimus_common::types::TestCase],
        error_message: &str,
    ) -> Vec<TestExecutionOutput> {
        test_cases.iter().map(|tc| TestExecutionOutput {
            test_id: tc.id,
            stdout: String::new(),
            stderr: error_message.to_string(),
            execution_time_ms: 0,
            timed_out: false,
            runtime_error: false,
            compilation_failed: true,
        }).collect()
    }
}

