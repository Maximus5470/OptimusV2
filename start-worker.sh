#!/bin/bash
# Optimus Worker Launcher
# Automatically sets environment variables and starts a worker for the specified language

set -e

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
NC='\033[0m' # No Color

# Load .env file if it exists
if [ -f .env ]; then
    export $(grep -v '^#' .env | xargs)
fi

# Show usage
usage() {
    echo -e "${CYAN}Optimus Worker Launcher${NC}"
    echo ""
    echo "Usage: $0 <language> [--all]"
    echo ""
    echo "Languages:"
    echo "  python    Start Python worker"
    echo "  java      Start Java worker"
    echo "  rust      Start Rust worker"
    echo ""
    echo "Options:"
    echo "  --all     Start workers for ALL configured languages (in background)"
    echo ""
    echo "Examples:"
    echo "  $0 python          # Start a Python worker"
    echo "  $0 java            # Start a Java worker"
    echo "  $0 --all           # Start all workers in background"
    exit 1
}

# Function to get language config
get_language_config() {
    local lang=$1
    case $lang in
        python)
            export OPTIMUS_LANGUAGE="python"
            export OPTIMUS_QUEUE="optimus:queue:python"
            export OPTIMUS_IMAGE="optimus-python:3.11-slim"
            ;;
        java)
            export OPTIMUS_LANGUAGE="java"
            export OPTIMUS_QUEUE="optimus:queue:java"
            export OPTIMUS_IMAGE="optimus-java:17"
            ;;
        rust)
            export OPTIMUS_LANGUAGE="rust"
            export OPTIMUS_QUEUE="optimus:queue:rust"
            export OPTIMUS_IMAGE="optimus-rust:1.75-slim"
            ;;
        *)
            echo -e "${RED}Error: Unknown language '$lang'${NC}"
            echo "Supported languages: python, java, rust"
            exit 1
            ;;
    esac
}

# Function to start a single worker
start_worker() {
    local lang=$1
    local background=$2
    
    get_language_config "$lang"
    
    echo -e "${GREEN}Starting $lang worker...${NC}"
    echo -e "  ${CYAN}OPTIMUS_LANGUAGE${NC}=$OPTIMUS_LANGUAGE"
    echo -e "  ${CYAN}OPTIMUS_QUEUE${NC}=$OPTIMUS_QUEUE"
    echo -e "  ${CYAN}OPTIMUS_IMAGE${NC}=$OPTIMUS_IMAGE"
    echo ""
    
    if [ "$background" = "true" ]; then
        ./target/release/optimus-worker &
        echo -e "${GREEN}✓ $lang worker started (PID: $!)${NC}"
    else
        ./target/release/optimus-worker
    fi
}

# Function to start all workers
start_all_workers() {
    echo -e "${CYAN}╔═══════════════════════════════════════════════════════════════╗${NC}"
    echo -e "${CYAN}║              STARTING ALL OPTIMUS WORKERS                     ║${NC}"
    echo -e "${CYAN}╚═══════════════════════════════════════════════════════════════╝${NC}"
    echo ""
    
    for lang in python java rust; do
        start_worker "$lang" "true"
        sleep 1  # Brief delay between worker starts
    done
    
    echo ""
    echo -e "${GREEN}All workers started!${NC}"
    echo -e "${YELLOW}Use 'jobs' to see running workers, 'fg' to bring one to foreground${NC}"
    echo -e "${YELLOW}Use 'pkill optimus-worker' to stop all workers${NC}"
    
    # Wait for all background jobs
    wait
}

# Check if worker binary exists
if [ ! -f "./target/release/optimus-worker" ]; then
    echo -e "${RED}Error: optimus-worker binary not found!${NC}"
    echo "Please run 'cargo build --workspace --release' first or run setup.sh"
    exit 1
fi

# Parse arguments
if [ $# -eq 0 ]; then
    usage
fi

case $1 in
    --all)
        start_all_workers
        ;;
    --help|-h)
        usage
        ;;
    python|java|rust)
        start_worker "$1" "false"
        ;;
    *)
        echo -e "${RED}Error: Unknown argument '$1'${NC}"
        usage
        ;;
esac
