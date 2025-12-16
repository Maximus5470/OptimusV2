# Optimus Development Guide

## Repository Structure

This repository follows a **Cargo workspace** architecture with separate binary and library crates.

```
optimus/
├── Cargo.toml                      # Workspace root
├── Cargo.lock                      # Shared dependency lock
├── README.md
├── .gitignore
├── .dockerignore
│
├── bins/                           # Binary crates
│   ├── optimus-api/                # HTTP Gateway (Axum)
│   │   ├── Cargo.toml
│   │   ├── Dockerfile
│   │   └── src/
│   │       ├── main.rs
│   │       ├── handlers.rs         # Route handlers
│   │       └── routes.rs           # Router config
│   │
│   ├── optimus-worker/             # Execution Engine (Bollard + Docker)
│   │   ├── Cargo.toml
│   │   ├── Dockerfile
│   │   └── src/
│   │       ├── main.rs
│   │       ├── docker.rs           # Container management
│   │       └── runner.rs           # Test orchestration
│   │
│   └── optimus-cli/                # Management Tool (Clap)
│       ├── Cargo.toml
│       └── src/
│           ├── main.rs
│           ├── commands.rs
│           └── generator.rs
│
├── libs/                           # Shared library crates
│   └── optimus-common/
│       ├── Cargo.toml
│       └── src/
│           ├── lib.rs
│           ├── types.rs            # Shared types
│           ├── redis.rs            # Redis logic
│           └── config.rs           # Configuration
│
├── config/                         # Configuration files
│   ├── optimus.toml                # Main config
│   └── languages.json              # Language definitions
│
├── k8s/                            # Kubernetes manifests
│   ├── namespace.yaml
│   ├── redis.yaml
│   ├── api-deployment.yaml
│   ├── worker-deployment.yaml
│   └── keda/
│       ├── scaled-object-python.yaml
│       └── scaled-object-java.yaml
│
└── dockerfiles/                    # Language execution environments
    ├── Dockerfile.base
    ├── python/
    │   └── Dockerfile
    └── java/
        └── Dockerfile
```

## Quick Start

### 1. Verify Workspace Integrity

```bash
cargo check
```

All crates should compile successfully.

### 2. Build All Crates

```bash
cargo build --release
```

### 3. Run Individual Binaries

```bash
# API Server
cargo run -p optimus-api

# Worker
cargo run -p optimus-worker

# CLI
cargo run -p optimus-cli
```

## Development Workflow

### Adding Dependencies

Dependencies are managed per-crate in their respective `Cargo.toml` files.

**For shared dependencies** (optimus-common):
```bash
cd libs/optimus-common
cargo add <dependency>
```

**For API**:
```bash
cd bins/optimus-api
cargo add axum tokio
```

**For Worker**:
```bash
cd bins/optimus-worker
cargo add bollard tokio
```

### Building Docker Images

**Build language execution images:**
```bash
docker build -t optimus-python:latest -f dockerfiles/python/Dockerfile .
docker build -t optimus-java:latest -f dockerfiles/java/Dockerfile .
```

**Build service images:**
```bash
docker build -t optimus-api:latest -f bins/optimus-api/Dockerfile .
docker build -t optimus-worker:latest -f bins/optimus-worker/Dockerfile .
```

### Deploying to Kubernetes

**1. Install KEDA (if not already installed):**
```bash
kubectl apply -f https://github.com/kedacore/keda/releases/download/v2.13.0/keda-2.13.0.yaml
```

**2. Deploy Optimus:**
```bash
# Create namespace
kubectl apply -f k8s/namespace.yaml

# Deploy Redis
kubectl apply -f k8s/redis.yaml

# Deploy API
kubectl apply -f k8s/api-deployment.yaml

# Deploy Worker
kubectl apply -f k8s/worker-deployment.yaml

# Apply KEDA autoscaling
kubectl apply -f k8s/keda/scaled-object-python.yaml
kubectl apply -f k8s/keda/scaled-object-java.yaml
```

## Next Steps (Implementation Roadmap)

### Phase 1: Core Infrastructure
- [ ] Implement Redis client in `optimus-common/src/redis.rs`
- [ ] Implement config loading from TOML/JSON
- [ ] Add error handling types

### Phase 2: API Implementation
- [ ] Add Axum and Tokio dependencies
- [ ] Implement `/submit` endpoint
- [ ] Implement `/status/:id` endpoint
- [ ] Add health/readiness probes

### Phase 3: Worker Implementation
- [ ] Add Bollard dependency
- [ ] Implement Docker container spawning
- [ ] Implement test case runner
- [ ] Add Redis queue polling

### Phase 4: CLI Implementation
- [ ] Add Clap dependency
- [ ] Implement `add-lang` command
- [ ] Implement template generation (Tera/Handlebars)
- [ ] Generate KEDA manifests dynamically

### Phase 5: Production Hardening
- [ ] Add comprehensive error handling
- [ ] Implement structured logging (tracing)
- [ ] Add metrics (Prometheus)
- [ ] Security: sandboxing, resource limits
- [ ] Add integration tests

## Architecture Notes

### Why This Structure?

1. **Workspace-first approach**: Prevents nested workspace issues
2. **Separation of concerns**: Each binary has a clear responsibility
3. **Shared library**: Common types/logic avoid duplication
4. **Docker-ready**: Dockerfiles are co-located with binaries
5. **K8s-native**: Manifests are version-controlled and templatable

### Key Design Decisions

- **Redis as queue**: Simple, fast, KEDA-compatible
- **Bollard for Docker**: Native Rust Docker client
- **KEDA for autoscaling**: Industry-standard event-driven scaling
- **Minimal initial deps**: Easy to understand, fast to build

## Troubleshooting

### Workspace Issues

If you see "invalid workspace configuration":
```bash
cargo check
```

Ensure all crates referenced in workspace `Cargo.toml` exist.

### Docker Build Failures

Ensure you're building from the **repository root**:
```bash
docker build -f bins/optimus-api/Dockerfile .
```

Not from inside the crate directory.

### KEDA Not Scaling

Check KEDA logs:
```bash
kubectl logs -n keda -l app=keda-operator
```

Verify Redis is accessible from worker pods.

## Contributing

1. Create a feature branch
2. Make focused commits (one concern per commit)
3. Run `cargo check` and `cargo clippy` before committing
4. Update this guide if adding new components

---

**Status**: ✅ Workspace skeleton complete and validated
**Next**: Implement Redis client and API endpoints
