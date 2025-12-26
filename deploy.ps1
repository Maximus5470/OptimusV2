#!/usr/bin/env pwsh
# OptimusV2 - Kubernetes Deployment Script
# Deploys Optimus to Kubernetes (Docker Desktop, kind, or minikube)

# Ensure UTF-8 output for emojis
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8

param(
    [switch]$SkipBuild,
    [switch]$SkipKeda,
    [switch]$SkipImages,
    [switch]$Uninstall,
    [string]$Context = "docker-desktop"
)

$ErrorActionPreference = "Stop"

# Colors for output
function Write-Step { param($Message) Write-Host "`n[*] $Message" -ForegroundColor Cyan }
function Write-Success { param($Message) Write-Host "[+] $Message" -ForegroundColor Green }
function Write-Warning { param($Message) Write-Host "[!] $Message" -ForegroundColor Yellow }
function Write-Error { param($Message) Write-Host "[X] $Message" -ForegroundColor Red }
function Write-Info { param($Message) Write-Host "[i] $Message" -ForegroundColor Blue }

# Banner
Write-Host @"
=======================================================
   OPTIMUS - Kubernetes Deployment with Autoscaling
=======================================================
"@ -ForegroundColor Magenta

# Uninstall mode
if ($Uninstall) {
    Write-Step "Uninstalling Optimus from Kubernetes"
    
    Write-Host "Deleting ScaledObjects..."
    kubectl delete scaledobjects -n optimus --all 2>$null
    
    Write-Host "Deleting worker deployments..."
    kubectl delete -f k8s/workers/ --ignore-not-found=true 2>$null
    
    Write-Host "Deleting API deployment..."
    kubectl delete -f k8s/api-deployment.yaml --ignore-not-found=true 2>$null
    
    Write-Host "Deleting Redis..."
    kubectl delete -f k8s/redis.yaml --ignore-not-found=true 2>$null
    
    Write-Host "Deleting namespace..."
    kubectl delete -f k8s/namespace.yaml --ignore-not-found=true 2>$null
    
    Write-Host "Deleting KEDA..."
    kubectl delete -f k8s/keda/keda-install.yaml --ignore-not-found=true 2>$null
    
    Write-Success "Uninstall complete!"
    exit 0
}

# Step 1: Check prerequisites
Write-Step "Checking prerequisites..."

# Check kubectl
try {
    $kubectlVersion = kubectl version --client 2>$null
    Write-Success "kubectl is installed: $kubectlVersion"
} catch {
    Write-Error "kubectl not found. Please install kubectl first."
    exit 1
}

# Check Docker
try {
    $dockerVersion = docker version --format '{{.Client.Version}}' 2>$null
    Write-Success "Docker is installed: $dockerVersion"
} catch {
    Write-Error "Docker not found. Please install Docker Desktop."
    exit 1
}

# Check Rust/Cargo
if (-not $SkipBuild) {
    try {
        $cargoVersion = cargo --version 2>$null
        Write-Success "Cargo is installed: $cargoVersion"
    } catch {
        Write-Error "Cargo not found. Please install Rust toolchain."
        exit 1
    }
}

# Step 2: Set kubectl context
Write-Step "Setting kubectl context to: $Context"
try {
    kubectl config use-context $Context
    $currentContext = kubectl config current-context
    Write-Success "Using context: $currentContext"
} catch {
    Write-Warning "Failed to set context. Using current context."
}

# Verify cluster connection
Write-Info "Testing cluster connection..."
try {
    kubectl cluster-info | Select-Object -First 1
    Write-Success "Cluster is reachable"
} catch {
    Write-Error "Cannot connect to Kubernetes cluster. Is it running?"
    exit 1
}

# Step 3: Build binaries
if (-not $SkipBuild) {
    Write-Step "Building Optimus binaries..."
    cargo build --workspace --release
    if ($LASTEXITCODE -ne 0) {
        Write-Error "Build failed!"
        exit 1
    }
    Write-Success "Binaries built successfully"
}

# Step 4: Build and load Docker images
if (-not $SkipImages) {
    Write-Step "Building and loading Docker images..."
    
    # Build API image
    Write-Info "Building optimus-api image..."
    docker build --no-cache -t optimus-api:latest -f bins/optimus-api/Dockerfile .
    if ($LASTEXITCODE -ne 0) {
        Write-Error "Failed to build optimus-api image"
        exit 1
    }
    
    # Build Worker image (single image used for all languages)
    Write-Info "Building optimus-worker image..."
    docker build --no-cache -t optimus-worker:latest -f bins/optimus-worker/Dockerfile .
    if ($LASTEXITCODE -ne 0) {
        Write-Error "Failed to build optimus-worker image"
        exit 1
    }
    
    Write-Success "Docker images built (API + Worker)"
    Write-Info "Note: Language runtime images (python/java/rust) are used by workers to spawn execution containers"
    
    # For kind clusters, load images
    if ($Context -like "*kind*") {
        Write-Info "Detected kind cluster - loading images..."
        kind load docker-image optimus-api:latest
        kind load docker-image optimus-worker:latest
        Write-Success "Images loaded into kind cluster"
    }
}

# Step 5: Render Kubernetes manifests
Write-Step "Rendering Kubernetes manifests..."
cargo run --bin optimus-cli --release -- render-k8s
if ($LASTEXITCODE -ne 0) {
    Write-Error "Failed to render manifests"
    exit 1
}
Write-Success "Manifests rendered"

# Step 6: Install KEDA
if (-not $SkipKeda) {
    Write-Step "Installing KEDA..."
    
    # Check if KEDA is already installed
    $ErrorActionPreference = 'SilentlyContinue'
    $kedaExists = kubectl get namespace keda 2>$null
    $ErrorActionPreference = 'Continue'
    
    if ($LASTEXITCODE -eq 0 -and $kedaExists) {
        Write-Info "KEDA namespace already exists, checking pods..."
        $kedaPods = kubectl get pods -n keda 2>$null
        if ($LASTEXITCODE -eq 0 -and $kedaPods) {
            Write-Success "KEDA already installed and running"
        } else {
            Write-Info "KEDA namespace exists but pods not found, reinstalling..."
            kubectl apply -f https://github.com/kedacore/keda/releases/download/v2.16.1/keda-2.16.1.yaml
            Start-Sleep -Seconds 5
        }
    } else {
        Write-Info "Installing KEDA for the first time..."
        kubectl apply -f https://github.com/kedacore/keda/releases/download/v2.16.1/keda-2.16.1.yaml
        Start-Sleep -Seconds 10
    }
    
    # Quick check without blocking wait
    Write-Info "Verifying KEDA pods..."
    $maxAttempts = 6
    $attempt = 0
    $kedaReady = $false
    
    while ($attempt -lt $maxAttempts -and -not $kedaReady) {
        $attempt++
        Start-Sleep -Seconds 5
        $readyPods = (kubectl get pods -n keda --field-selector=status.phase=Running 2>$null | Measure-Object -Line).Lines - 1
        if ($readyPods -ge 2) {
            $kedaReady = $true
            Write-Success "KEDA is ready ($readyPods pods running)"
        } else {
            Write-Info "Waiting for KEDA pods... (attempt $attempt/$maxAttempts)"
        }
    }
    
    if (-not $kedaReady) {
        Write-Warning "KEDA pods not ready yet, but continuing deployment..."
        Write-Info "You can check KEDA status later with: kubectl get pods -n keda"
    }
}

# Step 7: Create namespace
Write-Step "Creating Optimus namespace..."
kubectl apply -f k8s/namespace.yaml
Write-Success "Namespace created"

# Step 8: Deploy Redis
Write-Step "Deploying Redis..."
kubectl apply -f k8s/redis.yaml
Write-Info "Waiting for Redis to be ready..."
Start-Sleep -Seconds 5
$redisReady = kubectl wait --for=condition=ready pod -l app=redis -n optimus --timeout=60s 2>$null
if (-not $redisReady) {
    Write-Warning "Redis wait timed out, checking status..."
    kubectl get pods -n optimus -l app=redis
}
Write-Success "Redis deployed"

# Step 9: Deploy API
Write-Step "Deploying Optimus API..."
kubectl apply -f k8s/api-deployment.yaml
Write-Info "Waiting for API to be ready..."
Start-Sleep -Seconds 10
$apiReady = kubectl wait --for=condition=ready pod -l app=optimus-api -n optimus --timeout=90s 2>$null
if (-not $apiReady) {
    Write-Warning "API wait timed out, checking status..."
    kubectl get pods -n optimus -l app=optimus-api
    Write-Info "Check logs with: kubectl logs -n optimus -l app=optimus-api"
}
Write-Success "API deployed"

# Step 10: Deploy workers
Write-Step "Deploying language-specific worker deployments..."
Write-Info "Each deployment uses the same optimus-worker image with different env vars"
kubectl apply -f k8s/workers/
Write-Success "Worker deployments created (scaled to 0, waiting for jobs)"

# Step 11: Deploy KEDA scalers
Write-Step "Deploying KEDA ScaledObjects..."
kubectl apply -f k8s/keda/scaled-object-*.yaml
Write-Success "KEDA scalers deployed"

# Step 12: Verify deployment
Write-Step "Verifying deployment..."

Write-Host "`n[*] Pods in optimus namespace:"
kubectl get pods -n optimus -o wide

Write-Host "`n[*] Services:"
kubectl get svc -n optimus

Write-Host "`n[*] ScaledObjects:"
kubectl get scaledobjects -n optimus

Write-Host "`n[*] Deployments:"
kubectl get deployments -n optimus

# Get API endpoint
Write-Step "Getting API endpoint..."
$apiService = kubectl get svc optimus-api -n optimus -o json | ConvertFrom-Json
$apiPort = $apiService.spec.ports[0].port

if ($Context -eq "docker-desktop") {
    $apiUrl = "http://localhost:$apiPort"
} elseif ($Context -like "*minikube*") {
    $apiUrl = minikube service optimus-api -n optimus --url
} else {
    $apiUrl = "http://localhost:$apiPort (or use kubectl port-forward)"
}

Write-Host @"

=======================================================================
                    DEPLOYMENT SUCCESSFUL!
=======================================================================

API Endpoint: $apiUrl

Test the deployment:

   # Port forward (if needed):
   kubectl port-forward -n optimus svc/optimus-api 8080:80

   # Submit a test job:
   Invoke-RestMethod -Method POST -Uri "http://localhost:8080/jobs" `
     -ContentType "application/json" `
     -Body '{"language":"python","source_code":"print(\"Hello from K8s!\")","test_cases":[{"id":1,"input":"","expected_output":"Hello from K8s!\n"}],"timeout_ms":5000}'

Monitor autoscaling:

   # Watch pods scale up/down:
   kubectl get pods -n optimus -w

   # Check queue lengths:
   kubectl exec -n optimus deployment/redis -- redis-cli LLEN optimus:queue:python

   # View KEDA scaler events:
   kubectl describe scaledobject -n optimus

Logs:

   # API logs:
   kubectl logs -n optimus -l app=optimus-api -f

   # Worker logs:
   kubectl logs -n optimus -l app=optimus-worker-python -f

Uninstall:

   .\deploy.ps1 -Uninstall

"@ -ForegroundColor Green

Write-Success "Deployment complete! Happy coding!"
