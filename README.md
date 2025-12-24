#  Optimus

**A high-performance distributed code execution platform** built with Rust, Redis, and Docker. Execute code in multiple programming languages with sandboxed environments, automatic resource management, and horizontal scalability.

##  Features

- **Multi-Language Support**: Python, Java, Rust (easily extensible)
- **Compile-Once Execution**:  Compile code once, run all tests (2-4x faster for compiled languages)
- **Universal Runner**: Single `runner.sh` script handles all languages
- **Docker Isolation**: Sandboxed execution with resource limits
- **Redis Queue**: Reliable job distribution and cancellation support
- **Horizontal Scaling**: Language-specific worker pools
- **CLI Management**: Easy language addition and image building
- **Type-Safe**: Built with Rust for performance and reliability

##  Architecture

```
┌─────────────┐         ┌──────────────┐         ┌────────────────┐
│  HTTP API   │────────▶│    Redis     │◀────────│  Workers Pool  │
│   (Axum)    │         │   Queues     │         │   (Bollard)    │
└─────────────┘         └──────────────┘         └────────────────┘
                                │                         │
                                │                         ▼
                                │                 ┌──────────────┐
                                │                 │    Docker    │
                                │                 │  Containers  │
                                └────────────────▶│  (Isolated)  │
                                                  └──────────────┘
```

### Components

- **optimus-api**: HTTP gateway for job submission and status queries (Axum framework)
- **optimus-worker**: Multi-threaded worker that processes jobs from Redis queues (Tokio + Bollard)
- **optimus-cli**: Language management CLI for adding languages and building Docker images
- **optimus-common**: Shared types, Redis client logic, and configuration utilities

##  Quick Start

### Automated Setup (Recommended)

**Windows (PowerShell):**
```powershell
.\setup.ps1
```

**Linux/macOS:**
```bash
chmod +x setup.sh
./setup.sh
```

The setup script will:
1. ✓ Check prerequisites (Docker, Rust)
2. ✓ Build all binaries in release mode
3. ✓ Create Redis container (`optimus-redis`)
4. ✓ Configure Python, Java, and Rust languages
5. ✓ Build Docker images for all languages

### Manual Setup

#### 1. Prerequisites

- **Rust**: Install from [rustup.rs](https://rustup.rs)
- **Docker**: Install from [docker.com](https://www.docker.com/get-docker)
- **Redis** (optional): For local testing

#### 2. Build the Workspace

```bash
# Build all binaries in release mode
cargo build --workspace --release
```

#### 3. Setup Redis

```bash
# Start Redis container
docker run -d --name optimus-redis -p 6379:6379 redis:8-alpine
```

#### 4. Configure Languages

```bash
# Add Python
./target/release/optimus-cli add-lang --name python --ext py --version latest --memory 512 --cpu 1.0

# Add Java
./target/release/optimus-cli add-lang --name java --ext java --version latest --memory 512 --cpu 1.0

# Add Rust
./target/release/optimus-cli add-lang --name rust --ext rs --version latest --memory 512 --cpu 1.0

# List configured languages
./target/release/optimus-cli list-langs
```

##  Usage

### Start the System

**1. Start API Server:**
```bash
./target/release/optimus-api
# Listens on http://localhost:8080
```

**2. Start Workers (separate terminals):**
```bash
# Python worker
./target/release/optimus-worker --language python

# Java worker
./target/release/optimus-worker --language java

# Rust worker
./target/release/optimus-worker --language rust
```

### Submit a Job

**Using curl:**
```bash
curl -X POST http://localhost:<PORT>/jobs \
  -H "Content-Type: application/json" \
  -d '{
    "language": "python",
    "source_code": "print(\"Hello, Optimus!\")",
    "test_cases": [
      {"id": 1, "input": "", "expected_output": "Hello, Optimus!\n"}
    ],
    "timeout_ms": 5000
  }'
```

**Using PowerShell:**
```powershell
$job = Get-Content test_job.json
Invoke-RestMethod -Method POST -Uri http://localhost:<PORT>/jobs -Body $job -ContentType 'application/json'
```

**Response:**
```json
{
  "job_id": "550e8400-e29b-41d4-a716-446655440000",
  "status": "queued"
}
```

### Check Job Status

```bash
curl http://localhost:<PORT>/jobs/{job_id}
```

**Response:**
```json
{
  "job_id": "550e8400-e29b-41d4-a716-446655440000",
  "status": "completed",
  "result": {
    "test_results": [
      {
        "test_id": 1,
        "passed": true,
        "stdout": "Hello, Optimus!\n",
        "stderr": "",
        "execution_time_ms": 125
      }
    ],
    "total_tests": 1,
    "passed_tests": 1,
    "overall_status": "success"
  }
}
```

### Cancel a Running Job

```bash
curl -X DELETE http://localhost:<PORT>/jobs/{job_id}
```

##  Project Structure

```
OptimusV2/
├── bins/
│   ├── optimus-api/          # HTTP API server
│   ├── optimus-worker/       # Worker execution engine
│   └── optimus-cli/          # CLI management tool
├── libs/
│   └── optimus-common/       # Shared types and utilities
├── config/
│   └── languages.json        # Language configurations
├── dockerfiles/
│   ├── runner.sh             # Universal runner script (all languages)
│   ├── python/
│   │   └── Dockerfile        # Python execution environment
│   ├── java/
│   │   └── Dockerfile        # Java execution environment
│   └── rust/
│       └── Dockerfile        # Rust execution environment
├── setup.ps1                 # Windows setup script
├── setup.sh                  # Linux/macOS setup script
└── Cargo.toml                # Workspace configuration
```

##  CLI Reference

### Add a Language

```bash
optimus-cli add-lang \
  --name <language> \
  --ext <extension> \
  --version <docker-tag> \
  [--memory <MB>] \
  [--cpu <cores>] \
  [--skip-docker]
```

**Example:**
```bash
optimus-cli add-lang --name python --ext py --version 3.11-slim
```

### Remove a Language

```bash
optimus-cli remove-lang --name <language> [--yes]
```

### List Languages

```bash
optimus-cli list-langs
```

### Build Docker Image

```bash
optimus-cli build-image --name <language> [--no-cache]
```

##  Universal Runner Architecture

Optimus uses a **single universal runner script** (`dockerfiles/runner.sh`) that handles all programming languages. This eliminates the need for language-specific runners and simplifies Docker image creation.

**How it works:**

1. Worker sets `LANGUAGE` environment variable (e.g., `python`, `java`, `rust`)
2. Worker encodes source code and test input as base64 in `SOURCE_CODE` and `TEST_INPUT`
3. Universal runner detects language and:
   - Decodes the inputs
   - Compiles code (if needed)
   - Executes with test input
   - Captures stdout/stderr

**Benefits:**
-  Single source of truth for execution logic
-  Easy to add new languages (just update `runner.sh`)
-  No language-specific runner maintenance
-  Consistent error handling across all languages

##  Configuration

### Language Configuration (`config/languages.json`)

```json
{
  "languages": [
    {
      "name": "python",
      "version": "latest",
      "image": "optimus-python:latest",
      "dockerfile_path": "dockerfiles/python/Dockerfile",
      "execution": {
        "command": "python",
        "args": [],
        "file_extension": ".py"
      },
      "queue_name": "optimus:queue:python",
      "memory_limit_mb": 256,
      "cpu_limit": 0.5,
      "resources": {
        "requests": { "memory": "512Mi", "cpu": "500m" },
        "limits": { "memory": "1Gi", "cpu": "2000m" }
      },
      "concurrency": {
        "max_parallel_jobs": 3,
        "max_parallel_tests": 5
      }
    }
  ]
}
```

### Environment Variables

```bash
# Redis connection
REDIS_URL=redis://localhost:6379

# API server
API_HOST=0.0.0.0
API_PORT=8080

# Worker configuration
WORKER_LANGUAGE=python
WORKER_CONCURRENCY=4
```

##  Monitoring

### View Logs

```bash
# API logs
./target/release/optimus-api

# Worker logs
./target/release/optimus-worker --language python
```

### Redis Queue Status

```bash
# Connect to Redis
docker exec -it optimus-redis redis-cli

# Check queue length
LLEN optimus:queue:python

# View pending jobs
LRANGE optimus:queue:python 0 -1
```

### Docker Container Metrics

```bash
# List running containers
docker ps

# View container logs
docker logs <container-id>

# Monitor resource usage
docker stats
```

##  Adding a New Language

### Example: Adding Go

1. **Add language via CLI:**
```bash
optimus-cli add-lang --name go --ext go --version 1.21
```

2. **Update `runner.sh`** (already supports Go):
```bash
# The universal runner already handles Go:
go)
    echo "$SOURCE_CODE" > /code/main.go
    echo "$TEST_INPUT" | go run /code/main.go
    ;;
```

3. **Start worker:**
```bash
./target/release/optimus-worker --language go
```

That's it! The CLI generates the Dockerfile, builds the image, and the universal runner handles execution.

##  Performance Optimization

### Compile-Once Execution (NEW)

Enable the compile-once execution model for significant performance improvements:

```bash
# Enable compile-once execution
export USE_COMPILE_ONCE=true

# Restart workers
kubectl rollout restart deployment/optimus-worker
```

**Performance Gains:**

| Language | 10 Tests (Legacy) | 10 Tests (Compile-Once) | Speedup |
|----------|-------------------|-------------------------|---------|
| Java     | 15 seconds        | 6 seconds               | **2.5x** |
| Rust     | 20 seconds        | 7 seconds               | **2.9x** |
| C++      | 12 seconds        | 5 seconds               | **2.4x** |
| Python   | 5 seconds         | 4.2 seconds             | **1.2x** |

**How It Works:**
- Code is compiled once per job (not per test case)
- All test cases execute against the same compiled artifact
- Massive improvement for jobs with 5+ test cases
- Container lifecycle is optimized (1 container per job)

**Documentation:**
-  [Complete Migration Guide](./COMPILE_ONCE_MIGRATION.md)
-  [Quick Reference Card](./COMPILE_ONCE_QUICKREF.md)
-  [Performance Benchmarks](./benchmark_compile_once.ps1)

##  API Reference

### POST /jobs
Submit a code execution job

**Request Body:**
```json
{
  "language": "python",
  "source_code": "code here",
  "test_cases": [
    {"id": 1, "input": "input data", "expected_output": "expected result"}
  ],
  "timeout_ms": 5000
}
```

### GET /jobs/:id
Get job status and results

### DELETE /jobs/:id
Cancel a running job

### GET /health
Health check endpoint

##  Contributing

Contributions are welcome! Please feel free to submit a Pull Request.

##  License

MIT License - see LICENSE file for details

##  Acknowledgments

Built with:
- [Rust](https://www.rust-lang.org/) - Systems programming language
- [Axum](https://github.com/tokio-rs/axum) - Web framework
- [Tokio](https://tokio.rs/) - Async runtime
- [Bollard](https://github.com/fussybeaver/bollard) - Docker API client
- [Redis](https://redis.io/) - Job queue
- [Docker](https://www.docker.com/) - Container runtime
