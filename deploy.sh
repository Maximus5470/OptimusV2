#!/bin/bash
# OptimusV2 - Kubernetes Deployment Script
# Deploys Optimus to Kubernetes (Docker Desktop, kind, or minikube)

set -e

# Default values
SKIP_BUILD=false
SKIP_KEDA=false
SKIP_IMAGES=false
UNINSTALL=false
CONTEXT="docker-desktop"

# Parse arguments
while [[ $# -gt 0 ]]; do
    case $1 in
        --skip-build)
            SKIP_BUILD=true
            shift
            ;;
        --skip-keda)
            SKIP_KEDA=true
            shift
            ;;
        --skip-images)
            SKIP_IMAGES=true
            shift
            ;;
        --uninstall)
            UNINSTALL=true
            shift
            ;;
        --context)
            CONTEXT="$2"
            shift 2
            ;;
        *)
            echo "Unknown option: $1"
            echo "Usage: $0 [--skip-build] [--skip-keda] [--skip-images] [--uninstall] [--context CONTEXT]"
            exit 1
            ;;
    esac
done

# Colors for output
CYAN='\033[0;36m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
BLUE='\033[0;34m'
MAGENTA='\033[0;35m'
NC='\033[0m' # No Color

function write_step { echo -e "\n${CYAN}[*] $1${NC}"; }
function write_success { echo -e "${GREEN}[+] $1${NC}"; }
function write_warning { echo -e "${YELLOW}[!] $1${NC}"; }
function write_error { echo -e "${RED}[X] $1${NC}"; }
function write_info { echo -e "${BLUE}[i] $1${NC}"; }

# Banner
echo -e "${MAGENTA}"
cat << "EOF"
=======================================================
   OPTIMUS - Kubernetes Deployment with Autoscaling
=======================================================
EOF
echo -e "${NC}"

# Uninstall mode
if [ "$UNINSTALL" = true ]; then
    write_step "Uninstalling Optimus from Kubernetes"
    
    echo "Deleting ScaledObjects..."
    kubectl delete scaledobjects -n optimus --all 2>/dev/null || true
    
    echo "Deleting worker deployments..."
    kubectl delete -f k8s/workers/ --ignore-not-found=true 2>/dev/null || true
    
    echo "Deleting API deployment..."
    kubectl delete -f k8s/api-deployment.yaml --ignore-not-found=true 2>/dev/null || true
    
    echo "Deleting Redis..."
    kubectl delete -f k8s/redis.yaml --ignore-not-found=true 2>/dev/null || true
    
    echo "Deleting namespace..."
    kubectl delete -f k8s/namespace.yaml --ignore-not-found=true 2>/dev/null || true
    
    echo "Deleting KEDA..."
    kubectl delete -f https://github.com/kedacore/keda/releases/download/v2.16.1/keda-2.16.1.yaml --ignore-not-found=true 2>/dev/null || true
    
    write_success "Uninstall complete!"
    exit 0
fi

# Step 1: Check prerequisites
write_step "Checking prerequisites..."

# Check kubectl
if command -v kubectl &> /dev/null; then
    KUBECTL_VERSION=$(kubectl version --client --short 2>/dev/null || kubectl version --client 2>/dev/null | head -1)
    write_success "kubectl is installed: $KUBECTL_VERSION"
else
    write_error "kubectl not found. Please install kubectl first."
    exit 1
fi

# Check Docker
if command -v docker &> /dev/null; then
    DOCKER_VERSION=$(docker version --format '{{.Client.Version}}' 2>/dev/null)
    write_success "Docker is installed: $DOCKER_VERSION"
else
    write_error "Docker not found. Please install Docker."
    exit 1
fi

# Check Rust/Cargo
if [ "$SKIP_BUILD" = false ]; then
    if command -v cargo &> /dev/null; then
        CARGO_VERSION=$(cargo --version 2>/dev/null)
        write_success "Cargo is installed: $CARGO_VERSION"
    else
        write_error "Cargo not found. Please install Rust toolchain."
        exit 1
    fi
fi

# Step 2: Set kubectl context
write_step "Setting kubectl context to: $CONTEXT"
kubectl config use-context "$CONTEXT" 2>/dev/null || write_warning "Failed to set context. Using current context."
CURRENT_CONTEXT=$(kubectl config current-context)
write_success "Using context: $CURRENT_CONTEXT"

# Verify cluster connection
write_info "Testing cluster connection..."
if kubectl cluster-info | head -1 &> /dev/null; then
    write_success "Cluster is reachable"
else
    write_error "Cannot connect to Kubernetes cluster. Is it running?"
    exit 1
fi

# Step 3: Build binaries
if [ "$SKIP_BUILD" = false ]; then
    write_step "Building Optimus binaries..."
    cargo build --workspace --release
    write_success "Binaries built successfully"
fi

# Step 4: Build and load Docker images
if [ "$SKIP_IMAGES" = false ]; then
    write_step "Building and loading Docker images..."
    
    # Build API image
    write_info "Building optimus-api image..."
    docker build --no-cache -t optimus-api:latest -f bins/optimus-api/Dockerfile .
    
    # Build Worker image (single image used for all languages)
    write_info "Building optimus-worker image..."
    docker build --no-cache -t optimus-worker:latest -f bins/optimus-worker/Dockerfile .
    
    write_success "Docker images built (API + Worker)"
    write_info "Note: Language runtime images (python/java/rust) are used by workers to spawn execution containers"
    
    # For kind clusters, load images
    if [[ "$CONTEXT" == *"kind"* ]]; then
        write_info "Detected kind cluster - loading images..."
        kind load docker-image optimus-api:latest
        kind load docker-image optimus-worker:latest
        write_success "Images loaded into kind cluster"
    # For k3s clusters, import images into containerd
    elif [[ "$CONTEXT" == *"k3s"* ]] || [[ "$CURRENT_CONTEXT" == *"k3s"* ]]; then
        write_info "Detected k3s cluster - importing images into containerd..."
        write_info "This requires sudo access for k3s ctr..."
        
        # Save images to tar
        write_info "Saving optimus-api image..."
        docker save optimus-api:latest -o /tmp/optimus-api.tar
        
        write_info "Saving optimus-worker image..."
        docker save optimus-worker:latest -o /tmp/optimus-worker.tar
        
        # Import into k3s containerd
        write_info "Importing optimus-api into k3s..."
        sudo k3s ctr images import /tmp/optimus-api.tar
        
        write_info "Importing optimus-worker into k3s..."
        sudo k3s ctr images import /tmp/optimus-worker.tar
        
        # Cleanup tar files
        rm -f /tmp/optimus-api.tar /tmp/optimus-worker.tar
        
        write_success "Images imported into k3s containerd"
        write_info "Verifying images in k3s..."
        sudo k3s ctr images ls | grep optimus || write_warning "Could not verify images"
    fi
fi

# Step 5: Render Kubernetes manifests
write_step "Rendering Kubernetes manifests..."
cargo run --bin optimus-cli --release -- render-k8s
write_success "Manifests rendered"

# Step 6: Install KEDA
if [ "$SKIP_KEDA" = false ]; then
    write_step "Installing KEDA..."
    
    # Check if KEDA is already installed
    if kubectl get namespace keda &> /dev/null; then
        write_info "KEDA namespace already exists, checking pods..."
        if kubectl get pods -n keda &> /dev/null && [ $(kubectl get pods -n keda --no-headers 2>/dev/null | wc -l) -gt 0 ]; then
            write_success "KEDA already installed and running"
        else
            write_info "KEDA namespace exists but pods not found, reinstalling..."
            kubectl apply -f https://github.com/kedacore/keda/releases/download/v2.16.1/keda-2.16.1.yaml
            sleep 5
        fi
    else
        write_info "Installing KEDA for the first time..."
        kubectl apply -f https://github.com/kedacore/keda/releases/download/v2.16.1/keda-2.16.1.yaml
        sleep 10
    fi
    
    # Quick check without blocking wait
    write_info "Verifying KEDA pods..."
    MAX_ATTEMPTS=6
    ATTEMPT=0
    KEDA_READY=false
    
    while [ $ATTEMPT -lt $MAX_ATTEMPTS ] && [ "$KEDA_READY" = false ]; do
        ATTEMPT=$((ATTEMPT + 1))
        sleep 5
        READY_PODS=$(kubectl get pods -n keda --field-selector=status.phase=Running 2>/dev/null | tail -n +2 | wc -l)
        if [ "$READY_PODS" -ge 2 ]; then
            KEDA_READY=true
            write_success "KEDA is ready ($READY_PODS pods running)"
        else
            write_info "Waiting for KEDA pods... (attempt $ATTEMPT/$MAX_ATTEMPTS)"
        fi
    done
    
    if [ "$KEDA_READY" = false ]; then
        write_warning "KEDA pods not ready yet, but continuing deployment..."
        write_info "You can check KEDA status later with: kubectl get pods -n keda"
    fi
fi

# Step 7: Create namespace
write_step "Creating Optimus namespace..."
kubectl apply -f k8s/namespace.yaml
write_success "Namespace created"

# Step 8: Deploy Redis
write_step "Deploying Redis..."
kubectl apply -f k8s/redis.yaml
write_info "Waiting for Redis to be ready..."
sleep 5
if ! kubectl wait --for=condition=ready pod -l app=redis -n optimus --timeout=60s 2>/dev/null; then
    write_warning "Redis wait timed out, checking status..."
    kubectl get pods -n optimus -l app=redis
fi
write_success "Redis deployed"

# Step 9: Deploy API
write_step "Deploying Optimus API..."
kubectl apply -f k8s/api-deployment.yaml
write_info "Waiting for API to be ready..."
sleep 10
if ! kubectl wait --for=condition=ready pod -l app=optimus-api -n optimus --timeout=90s 2>/dev/null; then
    write_warning "API wait timed out, checking status..."
    kubectl get pods -n optimus -l app=optimus-api
    write_info "Check logs with: kubectl logs -n optimus -l app=optimus-api"
fi
write_success "API deployed"

# Step 10: Deploy workers
write_step "Deploying language-specific worker deployments..."
write_info "Each deployment uses the same optimus-worker image with different env vars"
kubectl apply -f k8s/workers/
write_success "Worker deployments created (scaled to 0, waiting for jobs)"

# Step 11: Deploy KEDA scalers
write_step "Deploying KEDA ScaledObjects..."
for f in k8s/keda/scaled-object-*.yaml; do
    kubectl apply -f "$f"
done
write_success "KEDA scalers deployed"

# Step 12: Verify deployment
write_step "Verifying deployment..."

echo -e "\n[*] Pods in optimus namespace:"
kubectl get pods -n optimus -o wide

echo -e "\n[*] Services:"
kubectl get svc -n optimus

echo -e "\n[*] ScaledObjects:"
kubectl get scaledobjects -n optimus

echo -e "\n[*] Deployments:"
kubectl get deployments -n optimus

# Get API endpoint
write_step "Getting API endpoint..."
API_PORT=$(kubectl get svc optimus-api -n optimus -o jsonpath='{.spec.ports[0].port}')

if [ "$CONTEXT" = "docker-desktop" ]; then
    API_URL="http://localhost:$API_PORT"
elif [[ "$CONTEXT" == *"minikube"* ]]; then
    API_URL=$(minikube service optimus-api -n optimus --url)
else
    API_URL="http://localhost:$API_PORT (or use kubectl port-forward)"
fi

echo -e "${GREEN}"
cat << EOF

=======================================================================
                    DEPLOYMENT SUCCESSFUL!
=======================================================================

API Endpoint: $API_URL

Test the deployment:

   # Port forward (if needed):
   kubectl port-forward -n optimus svc/optimus-api 8080:80

   # Submit a test job:
   curl -X POST "http://localhost:8080/jobs" \\
     -H "Content-Type: application/json" \\
     -d '{"language":"python","source_code":"print(\"Hello from K8s!\")","test_cases":[{"id":1,"input":"","expected_output":"Hello from K8s!\\n"}],"timeout_ms":5000}'

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

   ./deploy.sh --uninstall

EOF
echo -e "${NC}"

write_success "Deployment complete! Happy coding!"
