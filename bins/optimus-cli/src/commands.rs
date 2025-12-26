// CLI commands for managing Optimus
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use handlebars::Handlebars;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LanguageExecution {
    pub command: String,
    pub args: Vec<String>,
    pub file_extension: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceRequests {
    pub memory: String,
    pub cpu: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceLimits {
    pub memory: String,
    pub cpu: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Resources {
    pub requests: ResourceRequests,
    pub limits: ResourceLimits,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Concurrency {
    pub max_parallel_jobs: u32,
    pub max_parallel_tests: u32,
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
    pub resources: Resources,
    pub concurrency: Concurrency,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct LanguagesJson {
    pub languages: Vec<LanguageConfig>,
}

/// Load languages configuration
fn load_languages_config() -> Result<LanguagesJson> {
    let config_path = Path::new("config/languages.json");
    if !config_path.exists() {
        return Ok(LanguagesJson { languages: vec![] });
    }

    let content = fs::read_to_string(config_path)
        .context("Failed to read languages.json")?;
    serde_json::from_str(&content)
        .context("Failed to parse languages.json")
}

/// Save languages configuration
fn save_languages_config(config: &LanguagesJson) -> Result<()> {
    let config_path = Path::new("config/languages.json");
    
    // Ensure config directory exists
    if let Some(parent) = config_path.parent() {
        fs::create_dir_all(parent)?;
    }
    
    let json_content = serde_json::to_string_pretty(&config)
        .context("Failed to serialize languages.json")?;
    
    fs::write(config_path, json_content)
        .context("Failed to write languages.json")?;
    
    Ok(())
}

/// Add a new language to Optimus
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

    // Load existing config
    let mut languages_json = load_languages_config()?;

    // Check if language already exists
    if languages_json.languages.iter().any(|l| l.name == name) {
        bail!("Language '{}' already exists in config", name);
    }

    // Determine defaults
    let exec_command = command.unwrap_or(name).to_string();
    let queue_name = queue.map(|q| q.to_string())
        .unwrap_or_else(|| format!("optimus:queue:{}", name));
    let file_extension = if ext.starts_with('.') {
        ext.to_string()
    } else {
        format!(".{}", ext)
    };

    // Calculate resource allocations
    let (resources, concurrency) = calculate_resources(memory, cpu);

    // Create new language config
    let new_lang = LanguageConfig {
        name: name.to_string(),
        version: version.to_string(),
        image: format!("optimus-{}:{}", name, version),
        dockerfile_path: format!("dockerfiles/{}/Dockerfile", name),
        execution: LanguageExecution {
            command: exec_command,
            args: vec![],
            file_extension,
        },
        queue_name,
        memory_limit_mb: memory,
        cpu_limit: cpu,
        resources,
        concurrency,
    };

    // Add to languages
    languages_json.languages.push(new_lang);

    // Save config
    println!("📝 Updating config/languages.json...");
    save_languages_config(&languages_json)?;

    // Generate Dockerfile
    let dockerfile_dir = PathBuf::from(format!("dockerfiles/{}", name));
    let dockerfile_path = dockerfile_dir.join("Dockerfile");
    println!("🐳 Generating Dockerfile...");
    generate_dockerfile(&dockerfile_path, name, version, base_image)?;

    // Note: No need to generate language-specific runner scripts
    // All languages use the universal runner.sh from dockerfiles/runner.sh

    println!("✅ Language '{}' added successfully!", name);

    // Build Docker image if requested
    if build_docker {
        println!("\n🔨 Building Docker image...");
        build_docker_image(name, false).await?;
        
        println!("\n📋 Next steps:");
        println!("  1. Render K8s manifests: optimus-cli render-k8s");
        println!("  2. Deploy to cluster: kubectl apply -f k8s/worker-deployment-{}.yaml", name);
    } else {
        println!("\n⚠️  Docker image not built - the language won't work until you build it!");
        println!("\n📋 Next steps:");
        println!("  1. Build Docker image: optimus-cli build-image --name {}", name);
        println!("  2. Render K8s manifests: optimus-cli render-k8s");
        println!("  3. Deploy to cluster: kubectl apply -f k8s/");
    }

    Ok(())
}

/// Calculate resource allocations based on memory and CPU
fn calculate_resources(memory_mb: u32, cpu: f32) -> (Resources, Concurrency) {
    // Resource requests are 50% of limits
    let memory_request = format!("{}Mi", memory_mb * 2);
    let memory_limit = format!("{}Gi", (memory_mb as f32 * 4.0 / 1024.0).ceil() as u32);
    let cpu_request = format!("{}m", (cpu * 1000.0) as u32);
    let cpu_limit = format!("{}m", (cpu * 4000.0) as u32);

    let resources = Resources {
        requests: ResourceRequests {
            memory: memory_request,
            cpu: cpu_request,
        },
        limits: ResourceLimits {
            memory: memory_limit,
            cpu: cpu_limit,
        },
    };

    // Concurrency based on memory
    let concurrency = if memory_mb >= 512 {
        Concurrency {
            max_parallel_jobs: 2,
            max_parallel_tests: 3,
        }
    } else {
        Concurrency {
            max_parallel_jobs: 3,
            max_parallel_tests: 5,
        }
    };

    (resources, concurrency)
}

/// Remove a language from Optimus
pub async fn remove_language(name: &str, yes: bool) -> Result<()> {
    println!("🗑️  Removing language: {}", name);

    // Load existing config
    let mut languages_json = load_languages_config()?;

    // Find language
    let lang_index = languages_json.languages.iter()
        .position(|l| l.name == name)
        .ok_or_else(|| anyhow::anyhow!("Language '{}' not found in config", name))?;

    let lang_version = languages_json.languages[lang_index].version.clone();
    let lang_dockerfile_path = languages_json.languages[lang_index].dockerfile_path.clone();

    // Confirm deletion
    if !yes {
        print!("⚠️  This will remove:\n");
        print!("  - Config entry in languages.json\n");
        print!("  - Dockerfile at {}\n", lang_dockerfile_path);
        print!("  - K8s manifests (worker-deployment-{}.yaml, KEDA ScaledObjects)\n", name);
        print!("\nContinue? (y/N): ");
        io::stdout().flush()?;

        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        
        if !input.trim().eq_ignore_ascii_case("y") {
            println!("❌ Aborted");
            return Ok(());
        }
    }

    // Remove from config
    languages_json.languages.remove(lang_index);
    println!("📝 Removing from config/languages.json...");
    save_languages_config(&languages_json)?;

    // Remove Dockerfile directory
    let dockerfile_dir = PathBuf::from(format!("dockerfiles/{}", name));
    if dockerfile_dir.exists() {
        println!("🐳 Removing {}...", dockerfile_dir.display());
        fs::remove_dir_all(&dockerfile_dir)
            .with_context(|| format!("Failed to remove {}", dockerfile_dir.display()))?;
    }

    // Remove K8s manifests
    let manifests = vec![
        format!("k8s/worker-deployment-{}.yaml", name),
        format!("k8s/keda/scaled-object-{}.yaml", name),
        format!("k8s/keda/scaled-object-{}-retry.yaml", name),
    ];

    for manifest_path in manifests {
        let path = Path::new(&manifest_path);
        if path.exists() {
            println!("📊 Removing {}...", manifest_path);
            fs::remove_file(path)
                .with_context(|| format!("Failed to remove {}", manifest_path))?;
        }
    }

    // Remove Docker image
    let image_name = format!("optimus-{}:{}", name, lang_version);
    println!("🐳 Removing Docker image: {}...", image_name);
    
    let docker_result = Command::new("docker")
        .args(["rmi", "-f", &image_name])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output();
    
    match docker_result {
        Ok(output) => {
            if output.status.success() {
                println!("✅ Docker image removed successfully");
            } else {
                let stderr = String::from_utf8_lossy(&output.stderr);
                // Don't fail if image doesn't exist
                if stderr.contains("No such image") {
                    println!("ℹ️  Docker image not found (may already be removed)");
                } else {
                    eprintln!("⚠️  Failed to remove Docker image: {}", stderr.trim());
                    eprintln!("   You can manually remove it with: docker rmi {}", image_name);
                }
            }
        }
        Err(e) => {
            eprintln!("⚠️  Docker command failed: {}", e);
            eprintln!("   You can manually remove the image with: docker rmi {}", image_name);
        }
    }

    println!("\n✅ Language '{}' removed successfully!", name);
    println!("\n📋 Next steps:");
    println!("  1. Apply changes to K8s cluster if deployed");

    Ok(())
}

/// List all configured languages
pub async fn list_languages() -> Result<()> {
    let languages_json = load_languages_config()?;

    if languages_json.languages.is_empty() {
        println!("No languages configured.");
        println!("\n💡 Add a language with: optimus-cli add-lang --name <name> --ext <ext>");
        return Ok(());
    }

    println!("📋 Configured Languages:\n");
    println!("{:<12} {:<10} {:<30} {:<20} {:<10}",
             "Name", "Version", "Image", "Queue", "CPU/Mem");
    println!("{}", "─".repeat(100));

    for lang in &languages_json.languages {
        println!("{:<12} {:<10} {:<30} {:<20} {:.1}/{} MB",
                 lang.name,
                 lang.version,
                 lang.image,
                 lang.queue_name,
                 lang.cpu_limit,
                 lang.memory_limit_mb);
    }

    println!("\n✅ Total: {} language(s)", languages_json.languages.len());

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
                r#"# GENERATED BY optimus-cli — DO NOT EDIT
FROM {}

WORKDIR /app

# Copy runner script (if exists) from dockerfiles/{1}/ (build context is repo root)
COPY dockerfiles/{1}/runner.* /app/

# Set execution command
CMD ["{1}"]
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
        r#"# GENERATED BY optimus-cli — DO NOT EDIT
# Python Execution Environment - Optimized for Cold Start
FROM python:{}

# Set environment variables for performance
ENV PYTHONUNBUFFERED=1 \
    PYTHONDONTWRITEBYTECODE=1 \
    PIP_NO_CACHE_DIR=1 \
    PIP_DISABLE_PIP_VERSION_CHECK=1 \
    LANGUAGE=python

WORKDIR /code

# Install common packages (pre-installed at build time, not runtime)
RUN pip install --no-cache-dir \
    pytest==7.4.3 \
    numpy==1.26.2 \
    requests==2.31.0

# Copy universal runner script (build context is repo root)
COPY dockerfiles/runner.sh /runner.sh
RUN chmod +x /runner.sh

# Create non-root user for security
RUN useradd -m -u 1000 optimus && \
    chown -R optimus:optimus /code

USER optimus

# Use universal runner
ENTRYPOINT ["/runner.sh"]
"#,
        version
    )
}

/// Generate Java Dockerfile
fn generate_java_dockerfile(version: &str) -> String {
    format!(
        r#"# GENERATED BY optimus-cli — DO NOT EDIT
# Java Execution Environment - Optimized for Cold Start
FROM eclipse-temurin:{}-jdk-alpine

# Set environment variables for performance
ENV JAVA_TOOL_OPTIONS="-XX:+UseContainerSupport -XX:MaxRAMPercentage=75.0 -XX:+TieredCompilation -XX:TieredStopAtLevel=1" \
    LANGUAGE=java

WORKDIR /code

# Install JUnit and bash at build time (bash needed for runner.sh)
RUN apk add --no-cache wget bash && \
    wget -q https://repo1.maven.org/maven2/junit/junit/4.13.2/junit-4.13.2.jar -P /opt/ && \
    wget -q https://repo1.maven.org/maven2/org/hamcrest/hamcrest-core/1.3/hamcrest-core-1.3.jar -P /opt/ && \
    apk del wget && \
    rm -rf /var/cache/apk/*

ENV CLASSPATH=/opt/junit-4.13.2.jar:/opt/hamcrest-core-1.3.jar

# Copy universal runner script (build context is repo root)
COPY dockerfiles/runner.sh /runner.sh
RUN chmod +x /runner.sh

# Create non-root user for security
RUN adduser -D -u 1000 optimus && \
    chown -R optimus:optimus /code

USER optimus

# Use universal runner
ENTRYPOINT ["/runner.sh"]
"#,
        version
    )
}

/// Generate C++ Dockerfile
fn generate_cpp_dockerfile(version: &str) -> String {
    format!(
        r#"# GENERATED BY optimus-cli — DO NOT EDIT
# C++ Execution Environment
FROM gcc:{}

# Set environment variables
ENV LANGUAGE=cpp

WORKDIR /code

# Install necessary build tools
RUN apt-get update && apt-get install -y --no-install-recommends \
    build-essential \
    && rm -rf /var/lib/apt/lists/*

# Copy universal runner script (build context is repo root)
COPY dockerfiles/runner.sh /runner.sh
RUN chmod +x /runner.sh

# Create non-root user for security
RUN useradd -m -u 1000 optimus && \
    chown -R optimus:optimus /code

USER optimus

# Use universal runner
ENTRYPOINT ["/runner.sh"]
"#,
        version
    )
}

/// Generate Go Dockerfile
fn generate_go_dockerfile(version: &str) -> String {
    format!(
        r#"# GENERATED BY optimus-cli — DO NOT EDIT
# Go Execution Environment
FROM golang:{}

# Set environment variables
ENV GO111MODULE=on \
    CGO_ENABLED=0 \
    LANGUAGE=go

WORKDIR /code

# Copy universal runner script (build context is repo root)
COPY dockerfiles/runner.sh /runner.sh
RUN chmod +x /runner.sh

# Create non-root user for security
RUN useradd -m -u 1000 optimus && \
    chown -R optimus:optimus /code

USER optimus

# Use universal runner
ENTRYPOINT ["/runner.sh"]
"#,
        version
    )
}

/// Generate Node.js Dockerfile
fn generate_node_dockerfile(version: &str) -> String {
    format!(
        r#"# GENERATED BY optimus-cli — DO NOT EDIT
# Node.js Execution Environment
FROM node:{}

# Set environment variables
ENV LANGUAGE=javascript

WORKDIR /code

# Install necessary tools
RUN npm install -g typescript ts-node

# Copy universal runner script (build context is repo root)
COPY dockerfiles/runner.sh /runner.sh
RUN chmod +x /runner.sh

# Create non-root user for security
RUN useradd -m -u 1000 optimus && \
    chown -R optimus:optimus /code

USER optimus

# Use universal runner
ENTRYPOINT ["/runner.sh"]
"#,
        version
    )
}

/// Generate Rust Dockerfile
fn generate_rust_dockerfile(version: &str) -> String {
    format!(
        r#"# GENERATED BY optimus-cli — DO NOT EDIT
# Rust Execution Environment - Optimized for Code Execution
FROM rust:{}

# Set environment variables for performance
ENV CARGO_HOME=/usr/local/cargo \
    RUSTUP_HOME=/usr/local/rustup \
    PATH=/usr/local/cargo/bin:$PATH \
    RUSTFLAGS="-C opt-level=2 -C debuginfo=0" \
    LANGUAGE=rust

WORKDIR /code

# Install required packages
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# Copy universal runner script (build context is repo root)
COPY dockerfiles/runner.sh /runner.sh
RUN chmod +x /runner.sh

# Create non-root user for security
RUN useradd -m -u 1000 optimus && \
    chown -R optimus:optimus /code

USER optimus

# Use universal runner
ENTRYPOINT ["/runner.sh"]
"#,
        version
    )
}

/// Build Docker image for a language
pub async fn build_docker_image(name: &str, no_cache: bool) -> Result<()> {
    println!("🐳 Building Docker image for: {}", name);
    
    // Read languages.json to get version info
    let languages_json = load_languages_config()?;
    
    let lang_config = languages_json.languages.iter()
        .find(|l| l.name == name)
        .ok_or_else(|| anyhow::anyhow!("Language '{}' not found in config", name))?;
    
    let dockerfile_dir = PathBuf::from(format!("dockerfiles/{}", name));
    let dockerfile_path = dockerfile_dir.join("Dockerfile");
    
    if !dockerfile_path.exists() {
        bail!("Dockerfile not found at {}. Generate it first with add-lang command.", dockerfile_path.display());
    }
    
    // Build image tag
    let image_tag = format!("optimus-{}:{}", name, lang_config.version);
    
    println!("📦 Building tag: {}", image_tag);
    
    // Use current directory (.) as build context to support both:
    // - COPY dockerfiles/{lang}/file.ext (for manually created Dockerfiles)
    // - COPY file.ext (for generated Dockerfiles in subdirectory)
    let build_context = ".";
    println!("📂 Build context: {}", build_context);
    println!("📄 Dockerfile: {}", dockerfile_path.display());
    
    // Build docker command
    let mut docker_args = vec![
        "build".to_string(),
        "-t".to_string(),
        image_tag.clone(),
        "-f".to_string(),
        dockerfile_path.to_string_lossy().to_string(),
    ];
    
    if no_cache {
        docker_args.push("--no-cache".to_string());
    }
    
    // Add build context as the final argument
    docker_args.push(build_context.to_string());
    
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
    println!("\n📦 Available image: {}", image_tag);
    
    // Verify image exists
    println!("\n🔍 Verifying image...");
    let verify_status = Command::new("docker")
        .args(&["images", &image_tag, "--format", "{{.Repository}}:{{.Tag}}"])
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status();
    
    if verify_status.is_ok() {
        println!("✅ Image verification complete!");
    }
    
    Ok(())
}

/// Render Kubernetes manifests from templates for all configured languages
pub async fn render_k8s_manifests() -> Result<()> {
    println!("📊 Rendering Kubernetes manifests from templates...\n");
    
    // Load languages config
    let languages_json = load_languages_config()?;
    
    if languages_json.languages.is_empty() {
        bail!("No languages configured. Add a language first with: optimus-cli add-lang");
    }
    
    // Create handlebars registry
    let mut handlebars = Handlebars::new();
    handlebars.set_strict_mode(true);
    
    // Read template files
    let worker_template = fs::read_to_string("config/templates/worker-deployment.yaml.tmpl")
        .context("Failed to read worker-deployment.yaml.tmpl")?;
    let scaled_object_template = fs::read_to_string("config/templates/scaled-object.yaml.tmpl")
        .context("Failed to read scaled-object.yaml.tmpl")?;
    let scaled_object_retry_template = fs::read_to_string("config/templates/scaled-object-retry.yaml.tmpl")
        .context("Failed to read scaled-object-retry.yaml.tmpl")?;
    
    // Register templates
    handlebars.register_template_string("worker", &worker_template)?;
    handlebars.register_template_string("scaled_object", &scaled_object_template)?;
    handlebars.register_template_string("scaled_object_retry", &scaled_object_retry_template)?;
    
    // Ensure output directories exist
    fs::create_dir_all("k8s/workers")
        .context("Failed to create k8s/workers directory")?;
    fs::create_dir_all("k8s/keda")
        .context("Failed to create k8s/keda directory")?;
    
    let mut generated_files = Vec::new();
    
    // Render manifests for each language
    for lang in &languages_json.languages {
        println!("🔧 Rendering manifests for: {}", lang.name);
        
        // Prepare template data
        let data = json!({
            "language": lang.name,
            "queue_name": lang.queue_name,
            "image": lang.image,
            "memory_request": lang.resources.requests.memory,
            "memory_limit": lang.resources.limits.memory,
            "cpu_request": lang.resources.requests.cpu,
            "cpu_limit": lang.resources.limits.cpu,
            "max_parallel_jobs": lang.concurrency.max_parallel_jobs,
            "max_parallel_tests": lang.concurrency.max_parallel_tests,
        });
        
        // Render worker deployment
        let worker_yaml = handlebars.render("worker", &data)
            .context(format!("Failed to render worker deployment for {}", lang.name))?;
        let worker_path = format!("k8s/workers/worker-deployment-{}.yaml", lang.name);
        fs::write(&worker_path, worker_yaml)
            .context(format!("Failed to write {}", worker_path))?;
        
        // Render main queue ScaledObject
        let scaled_object_yaml = handlebars.render("scaled_object", &data)
            .context(format!("Failed to render ScaledObject for {}", lang.name))?;
        let scaled_object_path = format!("k8s/keda/scaled-object-{}.yaml", lang.name);
        fs::write(&scaled_object_path, scaled_object_yaml)
            .context(format!("Failed to write {}", scaled_object_path))?;
        
        // Render retry queue ScaledObject
        let scaled_object_retry_yaml = handlebars.render("scaled_object_retry", &data)
            .context(format!("Failed to render retry ScaledObject for {}", lang.name))?;
        let scaled_object_retry_path = format!("k8s/keda/scaled-object-{}-retry.yaml", lang.name);
        fs::write(&scaled_object_retry_path, scaled_object_retry_yaml)
            .context(format!("Failed to write {}", scaled_object_retry_path))?;
        
        println!("  ✅ {}", worker_path);
        println!("  ✅ {}", scaled_object_path);
        println!("  ✅ {}", scaled_object_retry_path);
        
        generated_files.push(worker_path);
        generated_files.push(scaled_object_path);
        generated_files.push(scaled_object_retry_path);
    }
    
    println!("\n━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("✅ Successfully rendered {} manifest files!", generated_files.len());
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n");
    
    println!("📋 Next steps:");
    println!("  1. Install KEDA (if not already):");
    println!("     kubectl apply --server-side -f k8s/keda/keda-install.yaml");
    println!();
    println!("  2. Create namespace:");
    println!("     kubectl apply -f k8s/namespace.yaml");
    println!();
    println!("  3. Deploy Redis:");
    println!("     kubectl apply -f k8s/redis.yaml");
    println!();
    println!("  4. Deploy API:");
    println!("     kubectl apply -f k8s/api-deployment.yaml");
    println!();
    println!("  5. Deploy workers:");
    println!("     kubectl apply -f k8s/workers/");
    println!();
    println!("  6. Deploy KEDA scalers:");
    println!("     kubectl apply -f k8s/keda/scaled-object-*.yaml");
    println!();
    println!("  7. Verify deployment:");
    println!("     kubectl get pods -n optimus");
    println!("     kubectl get scaledobjects -n optimus");
    
    Ok(())
}

