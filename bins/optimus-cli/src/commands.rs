// CLI commands for managing Optimus
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LanguageExecution {
    pub command: String,
    pub args: Vec<String>,
    pub file_extension: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LanguageConfig {
    pub name: String,
    pub version: String,
    pub image: String,
    pub dockerfile_path: String,
    pub execution: LanguageExecution,
    pub queue_name: String,
    pub memory_limit_mb: u32,
    pub cpu_limit: f32,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct LanguagesJson {
    pub languages: Vec<LanguageConfig>,
}

/// Add a new language to Optimus
///
/// This command:
/// 1. Updates config/languages.json with new language metadata
/// 2. Generates Dockerfile in dockerfiles/<language>/Dockerfile
/// 3. Generates KEDA ScaledObject YAML in k8s/keda/scaled-object-<language>.yaml
/// 4. Creates runner script if needed
pub async fn add_language(
    name: &str,
    ext: &str,
    version: &str,
    base_image: Option<&str>,
    command: Option<&str>,
    queue: Option<&str>,
    memory: u32,
    cpu: f32,
    build_docker: bool,
) -> Result<()> {
    println!("🚀 Adding language: {}", name);

    // Validate inputs
    if name.is_empty() || ext.is_empty() {
        bail!("Language name and extension cannot be empty");
    }

    // Determine paths
    let config_path = Path::new("config/languages.json");
    let dockerfile_dir = PathBuf::from(format!("dockerfiles/{}", name));
    let dockerfile_path = dockerfile_dir.join("Dockerfile");
    let keda_path = PathBuf::from(format!("k8s/keda/scaled-object-{}.yaml", name));

    // Step 1: Update languages.json
    println!("📝 Updating config/languages.json...");
    update_languages_config(
        config_path,
        name,
        ext,
        version,
        command,
        queue,
        memory,
        cpu,
    )?;

    // Step 2: Generate Dockerfile
    println!("🐳 Generating Dockerfile...");
    generate_dockerfile(&dockerfile_path, name, version, base_image)?;

    // Step 3: Generate KEDA ScaledObject
    println!("📊 Generating KEDA ScaledObject...");
    generate_keda_scaledobject(&keda_path, name, queue)?;

    // Step 4: Generate runner script if needed
    if matches!(name, "python" | "java" | "rust" | "cpp" | "go") {
        println!("📜 Generating runner script...");
        generate_runner_script(&dockerfile_dir, name)?;
    }

    println!("✅ Language '{}' added successfully!", name);

    // Step 5: Build Docker image if requested
    if build_docker {
        println!("\n🔨 Building Docker image...");
        build_docker_image(name, false).await?;
    } else {
        println!("\n⏭️  Skipping Docker build (use --build-docker=true to build)");
    }

    println!("\n📋 Next steps:");
    if !build_docker {
        println!("  1. Build Docker image: optimus-cli build-image --name {}", name);
    }
    println!("  {}. Apply KEDA ScaledObject: kubectl apply -f {}", if build_docker { 1 } else { 2 }, keda_path.display());
    println!("  {}. Deploy worker for {}: Update worker-deployment.yaml with language filter", if build_docker { 2 } else { 3 }, name);

    Ok(())
}

/// Update languages.json with new language
fn update_languages_config(
    config_path: &Path,
    name: &str,
    ext: &str,
    version: &str,
    command: Option<&str>,
    queue: Option<&str>,
    memory: u32,
    cpu: f32,
) -> Result<()> {
    // Read existing config
    let mut languages_json: LanguagesJson = if config_path.exists() {
        let content = fs::read_to_string(config_path)
            .context("Failed to read languages.json")?;
        serde_json::from_str(&content)
            .context("Failed to parse languages.json")?
    } else {
        LanguagesJson { languages: vec![] }
    };

    // Check if language already exists
    if languages_json.languages.iter().any(|l| l.name == name) {
        bail!("Language '{}' already exists in config", name);
    }

    // Determine defaults
    let exec_command = command.unwrap_or(name).to_string();
    let queue_name = queue.map(|q| q.to_string()).unwrap_or_else(|| format!("jobs:{}", name));
    let file_extension = if ext.starts_with('.') {
        ext.to_string()
    } else {
        format!(".{}", ext)
    };

    // Create new language config
    let new_lang = LanguageConfig {
        name: name.to_string(),
        version: version.to_string(),
        image: format!("optimus-{}:latest", name),
        dockerfile_path: format!("dockerfiles/{}/Dockerfile", name),
        execution: LanguageExecution {
            command: exec_command,
            args: vec![],
            file_extension,
        },
        queue_name,
        memory_limit_mb: memory,
        cpu_limit: cpu,
    };

    // Add to languages
    languages_json.languages.push(new_lang);

    // Write back to file
    let json_content = serde_json::to_string_pretty(&languages_json)
        .context("Failed to serialize languages.json")?;
    
    // Ensure config directory exists
    if let Some(parent) = config_path.parent() {
        fs::create_dir_all(parent)?;
    }
    
    fs::write(config_path, json_content)
        .context("Failed to write languages.json")?;

    Ok(())
}

/// Generate Dockerfile for the language
fn generate_dockerfile(
    dockerfile_path: &Path,
    name: &str,
    version: &str,
    base_image: Option<&str>,
) -> Result<()> {
    // Create directory
    if let Some(parent) = dockerfile_path.parent() {
        fs::create_dir_all(parent)?;
    }

    let dockerfile_content = match name {
        "python" => generate_python_dockerfile(version),
        "java" => generate_java_dockerfile(version),
        "rust" => generate_rust_dockerfile(version),
        "cpp" => generate_cpp_dockerfile(version),
        "go" => generate_go_dockerfile(version),
        "javascript" | "node" => generate_node_dockerfile(version),
        _ => {
            // Generic Dockerfile
            let default_base = format!("{}:{}", name, version);
            let base = base_image.unwrap_or(&default_base);
            format!(
                r#"FROM {}

WORKDIR /app

# Copy runner script (if exists)
COPY runner.* /app/

# Set execution command
CMD ["{}"]
"#,
                base, name
            )
        }
    };

    fs::write(dockerfile_path, dockerfile_content)
        .context("Failed to write Dockerfile")?;

    Ok(())
}

/// Generate Python Dockerfile
fn generate_python_dockerfile(version: &str) -> String {
    format!(
        r#"FROM python:{}

WORKDIR /app

# Copy runner script
COPY runner.py /app/runner.py

# Make runner executable
RUN chmod +x /app/runner.py

# Set Python to run in unbuffered mode
ENV PYTHONUNBUFFERED=1

CMD ["python", "/app/runner.py"]
"#,
        version
    )
}

/// Generate Java Dockerfile
fn generate_java_dockerfile(version: &str) -> String {
    format!(
        r#"FROM openjdk:{}

WORKDIR /app

# Install necessary tools
RUN apt-get update && apt-get install -y --no-install-recommends \
    && rm -rf /var/lib/apt/lists/*

# Copy runner script (if needed)
# COPY runner.sh /app/runner.sh
# RUN chmod +x /app/runner.sh

CMD ["java"]
"#,
        version
    )
}

/// Generate C++ Dockerfile
fn generate_cpp_dockerfile(version: &str) -> String {
    format!(
        r#"FROM gcc:{}

WORKDIR /app

# Install necessary build tools
RUN apt-get update && apt-get install -y --no-install-recommends \
    build-essential \
    && rm -rf /var/lib/apt/lists/*

CMD ["g++"]
"#,
        version
    )
}

/// Generate Go Dockerfile
fn generate_go_dockerfile(version: &str) -> String {
    format!(
        r#"FROM golang:{}

WORKDIR /app

# Set Go environment
ENV GO111MODULE=on
ENV CGO_ENABLED=0

CMD ["go"]
"#,
        version
    )
}

/// Generate Node.js Dockerfile
fn generate_node_dockerfile(version: &str) -> String {
    format!(
        r#"FROM node:{}

WORKDIR /app

# Install necessary tools
RUN npm install -g typescript ts-node

CMD ["node"]
"#,
        version
    )
}

/// Generate Rust Dockerfile
fn generate_rust_dockerfile(version: &str) -> String {
    format!(
        r#"# Rust Execution Environment - Optimized for Code Execution
FROM rust:{}-slim

# Set environment variables for performance
ENV CARGO_HOME=/usr/local/cargo \
    RUSTUP_HOME=/usr/local/rustup \
    PATH=/usr/local/cargo/bin:$PATH \
    RUSTFLAGS="-C opt-level=2 -C debuginfo=0"

WORKDIR /code

# Install required packages
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# Copy runner script
COPY runner.sh /code/runner.sh
RUN chmod +x /code/runner.sh

# Create non-root user for security
RUN useradd -m -u 1000 optimus && \
    chown -R optimus:optimus /code

USER optimus

# Set entrypoint to runner script
ENTRYPOINT ["/code/runner.sh"]
"#,
        version
    )
}

/// Generate KEDA ScaledObject YAML
fn generate_keda_scaledobject(
    keda_path: &Path,
    name: &str,
    queue: Option<&str>,
) -> Result<()> {
    // Create directory
    if let Some(parent) = keda_path.parent() {
        fs::create_dir_all(parent)?;
    }

    let default_queue = format!("jobs:{}", name);
    let queue_name = queue.unwrap_or(&default_queue);
    let deployment_name = format!("optimus-worker-{}", name);

    let keda_content = format!(
        r#"apiVersion: keda.sh/v1alpha1
kind: ScaledObject
metadata:
  name: worker-{name}
  namespace: optimus
spec:
  scaleTargetRef:
    name: {deployment_name}
  minReplicaCount: 0
  maxReplicaCount: 10
  pollingInterval: 5
  cooldownPeriod: 30
  triggers:
    - type: redis
      metadata:
        address: redis-master:6379
        listName: {queue_name}
        listLength: "1"
        enableTLS: "false"
"#,
        name = name,
        deployment_name = deployment_name,
        queue_name = queue_name
    );

    fs::write(keda_path, keda_content)
        .context("Failed to write KEDA ScaledObject")?;

    Ok(())
}

/// Generate language-specific runner script
fn generate_runner_script(dockerfile_dir: &Path, name: &str) -> Result<()> {
    match name {
        "rust" => {
            let runner_path = dockerfile_dir.join("runner.sh");
            let runner_content = r#"#!/bin/bash
# Optimus Rust Runner
# Executes Rust code with given input and captures output

set -e

# Read source code from environment (base64 encoded)
SOURCE_CODE_B64="${SOURCE_CODE:-}"
TEST_INPUT_B64="${TEST_INPUT:-}"

if [ -z "$SOURCE_CODE_B64" ]; then
    echo "Error: SOURCE_CODE environment variable not set" >&2
    exit 1
fi

# Decode source code and input
SOURCE_CODE=$(echo "$SOURCE_CODE_B64" | base64 -d)
TEST_INPUT=$(echo "$TEST_INPUT_B64" | base64 -d)

# Write source code to file
echo "$SOURCE_CODE" > /code/main.rs

# Compile the Rust code
rustc /code/main.rs -o /code/main 2>&1

if [ $? -ne 0 ]; then
    echo "Compilation failed" >&2
    exit 1
fi

# Execute with test input
echo "$TEST_INPUT" | /code/main
"#;
            fs::write(runner_path, runner_content)?;
        }
        "python" => {
            let runner_path = dockerfile_dir.join("runner.py");
            let runner_content = r#"#!/usr/bin/env python3
"""
Python Runner for Optimus
Executes Python code with given input and captures output
"""

import sys
import subprocess
import tempfile
import os

def main():
    # Read source code from environment or stdin
    source_code = os.environ.get('SOURCE_CODE', '')
    if not source_code:
        source_code = sys.stdin.read()
    
    # Read input
    test_input = os.environ.get('TEST_INPUT', '')
    
    # Create temporary file
    with tempfile.NamedTemporaryFile(mode='w', suffix='.py', delete=False) as f:
        f.write(source_code)
        temp_file = f.name
    
    try:
        # Execute Python code
        result = subprocess.run(
            ['python', '-u', temp_file],
            input=test_input,
            capture_output=True,
            text=True,
            timeout=60
        )
        
        # Output results
        print(result.stdout, end='')
        if result.stderr:
            print(result.stderr, file=sys.stderr, end='')
        
        sys.exit(result.returncode)
    finally:
        # Cleanup
        if os.path.exists(temp_file):
            os.remove(temp_file)

if __name__ == '__main__':
    main()
"#;
            fs::write(runner_path, runner_content)?;
        }
        _ => {
            // For other languages, create a placeholder
            println!("  ⚠️  No default runner for {}. You may need to create one manually.", name);
        }
    }

    Ok(())
}

/// Initialize a new Optimus project
pub async fn init_project(path: &str) -> Result<()> {
    println!("🚀 Initializing Optimus project at: {}", path);
    
    let project_path = Path::new(path);
    
    // Create directories
    let dirs = [
        "config",
        "dockerfiles",
        "k8s",
        "k8s/keda",
        "examples",
    ];
    
    for dir in &dirs {
        let dir_path = project_path.join(dir);
        fs::create_dir_all(&dir_path)
            .with_context(|| format!("Failed to create directory: {}", dir))?;
        println!("  ✅ Created: {}", dir);
    }
    
    // Create default languages.json
    let languages_json_path = project_path.join("config/languages.json");
    if !languages_json_path.exists() {
        let default_config = LanguagesJson {
            languages: vec![],
        };
        let json_content = serde_json::to_string_pretty(&default_config)?;
        fs::write(languages_json_path, json_content)?;
        println!("  ✅ Created: config/languages.json");
    }
    
    println!("✅ Project initialized successfully!");
    println!("\n📋 Next steps:");
    println!("  1. Add a language: optimus-cli add-lang --name python --ext py");
    println!("  2. Configure Redis and API settings");
    println!("  3. Deploy to Kubernetes");
    
    Ok(())
}

/// Build Docker image for a language
pub async fn build_docker_image(name: &str, no_cache: bool) -> Result<()> {
    println!("🐳 Building Docker image for: {}", name);
    
    // Read languages.json to get version info
    let config_path = Path::new("config/languages.json");
    if !config_path.exists() {
        bail!("config/languages.json not found. Have you added the language yet?");
    }
    
    let content = fs::read_to_string(config_path)
        .context("Failed to read languages.json")?;
    let languages_json: LanguagesJson = serde_json::from_str(&content)
        .context("Failed to parse languages.json")?;
    
    let lang_config = languages_json.languages.iter()
        .find(|l| l.name == name)
        .ok_or_else(|| anyhow::anyhow!("Language '{}' not found in config", name))?;
    
    let dockerfile_dir = PathBuf::from(format!("dockerfiles/{}", name));
    let dockerfile_path = dockerfile_dir.join("Dockerfile");
    
    if !dockerfile_path.exists() {
        bail!("Dockerfile not found at {}. Generate it first with add-lang command.", dockerfile_path.display());
    }
    
    // Build image tags
    let image_versioned = format!("optimus-{}:{}-v1", name, lang_config.version);
    let image_latest = format!("optimus-{}:latest", name);
    
    println!("📦 Building tags:");
    println!("  - {}", image_versioned);
    println!("  - {}", image_latest);
    println!("📂 Context: {}", dockerfile_dir.display());
    println!("📄 Dockerfile: {}", dockerfile_path.display());
    
    // Build docker command
    let mut docker_args = vec![
        "build".to_string(),
        "-t".to_string(),
        image_versioned.clone(),
        "-t".to_string(),
        image_latest.clone(),
        "-f".to_string(),
        dockerfile_path.to_string_lossy().to_string(),
    ];
    
    if no_cache {
        docker_args.push("--no-cache".to_string());
    }
    
    docker_args.push(dockerfile_dir.to_string_lossy().to_string());
    
    println!("\n🔨 Running: docker {}", docker_args.join(" "));
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n");
    
    // Execute docker build
    let status = Command::new("docker")
        .args(&docker_args)
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .context("Failed to execute docker build. Is Docker installed and running?")?;
    
    if !status.success() {
        bail!("Docker build failed with exit code: {:?}", status.code());
    }
    
    println!("\n━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("✅ Docker image built successfully!");
    println!("\n📦 Available images:");
    println!("  - {}", image_versioned);
    println!("  - {}", image_latest);
    
    // Verify images exist
    println!("\n🔍 Verifying images...");
    let verify_status = Command::new("docker")
        .args(&["images", &image_latest, "--format", "{{.Repository}}:{{.Tag}}"])
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status();
    
    if verify_status.is_ok() {
        println!("✅ Image verification complete!");
    }
    
    Ok(())
}
