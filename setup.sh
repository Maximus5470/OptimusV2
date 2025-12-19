#!/bin/bash
# Optimus Setup Script for Linux/macOS
# This script sets up the complete Optimus development environment

set -e

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
CYAN='\033[0;36m'
NC='\033[0m' # No Color

# Function to print section headers
print_section() {
    echo ""
    echo -e "${BLUE}═══════════════════════════════════════════════════════════════${NC}"
    echo -e "${YELLOW} $1${NC}"
    echo -e "${BLUE}═══════════════════════════════════════════════════════════════${NC}"
}

# Function to check if a command exists
command_exists() {
    command -v "$1" >/dev/null 2>&1
}

echo -e "${CYAN}╔═══════════════════════════════════════════════════════════════╗${NC}"
echo -e "${CYAN}║                   OPTIMUS SETUP SCRIPT                        ║${NC}"
echo -e "${CYAN}║              Distributed Code Execution Platform              ║${NC}"
echo -e "${CYAN}╚═══════════════════════════════════════════════════════════════╝${NC}"
echo ""

# Step 1: Check Prerequisites
print_section "STEP 1: Checking Prerequisites"

echo -e "${CYAN}→ Checking for Docker...${NC}"
if ! command_exists docker; then
    echo -e "${RED}✗ Docker is not installed!${NC}"
    echo -e "${YELLOW}  Please install Docker from: https://docs.docker.com/get-docker/${NC}"
    exit 1
fi
echo -e "${GREEN}✓ Docker found${NC}"

echo -e "${CYAN}→ Checking if Docker daemon is running...${NC}"
if ! docker ps >/dev/null 2>&1; then
    echo -e "${RED}✗ Docker daemon is not running!${NC}"
    echo -e "${YELLOW}  Please start Docker daemon${NC}"
    exit 1
fi
echo -e "${GREEN}✓ Docker daemon is running${NC}"

echo -e "${CYAN}→ Checking for Rust/Cargo...${NC}"
if ! command_exists cargo; then
    echo -e "${RED}✗ Cargo is not installed!${NC}"
    echo -e "${YELLOW}  Please install Rust from: https://rustup.rs${NC}"
    exit 1
fi
echo -e "${GREEN}✓ Cargo found${NC}"
cargo --version

# Step 2: Build Workspace
print_section "STEP 2: Building Optimus Workspace"

echo -e "${CYAN}→ Building all binaries in release mode...${NC}"
echo -e "${YELLOW}  This may take a few minutes on first run...${NC}"
cargo build --workspace --release

echo -e "${GREEN}✓ Workspace built successfully${NC}"

# Step 3: Setup Redis Container
print_section "STEP 3: Setting up Redis Container"

echo -e "${CYAN}→ Checking for existing redis-optimus container...${NC}"
if docker ps -a --filter "name=redis-optimus" --format "{{.Names}}" | grep -q "redis-optimus"; then
    echo -e "${YELLOW}  Container 'redis-optimus' already exists${NC}"
    echo -e "${CYAN}→ Removing existing container...${NC}"
    docker rm -f redis-optimus >/dev/null 2>&1
fi

echo -e "${CYAN}→ Creating Redis container (redis:8-alpine)...${NC}"
docker run -d \
    --name redis-optimus \
    -p 6379:6379 \
    redis:8-alpine

echo -e "${GREEN}✓ Redis container 'redis-optimus' created and running on port 6379${NC}"

# Step 4: Configure Languages
print_section "STEP 4: Configuring Languages"

# Python
echo ""
echo -e "${CYAN}→ Adding Python language...${NC}"
./target/release/optimus-cli add-lang \
    --name python \
    --ext py \
    --version 3.11-slim \
    --memory 256 \
    --cpu 0.5

echo -e "${GREEN}✓ Python configured and Docker image built${NC}"

# Java
echo ""
echo -e "${CYAN}→ Adding Java language...${NC}"
./target/release/optimus-cli add-lang \
    --name java \
    --ext java \
    --version 17 \
    --memory 512 \
    --cpu 1.0

echo -e "${GREEN}✓ Java configured and Docker image built${NC}"

# Rust
echo ""
echo -e "${CYAN}→ Adding Rust language...${NC}"
./target/release/optimus-cli add-lang \
    --name rust \
    --ext rs \
    --version 1.75-slim \
    --memory 512 \
    --cpu 1.0

echo -e "${GREEN}✓ Rust configured and Docker image built${NC}"

# Step 5: Verify Setup
print_section "STEP 5: Verifying Setup"

echo -e "${CYAN}→ Listing configured languages...${NC}"
./target/release/optimus-cli list-langs

echo ""
echo -e "${CYAN}→ Checking Docker images...${NC}"
docker images | grep "optimus-"

echo ""
echo -e "${CYAN}→ Checking Redis container...${NC}"
docker ps --filter "name=redis-optimus" --format "table {{.Names}}\t{{.Status}}\t{{.Ports}}"

# Final Summary
echo ""
echo -e "${GREEN}╔═══════════════════════════════════════════════════════════════╗${NC}"
echo -e "${GREEN}║                    SETUP COMPLETED!                           ║${NC}"
echo -e "${GREEN}╚═══════════════════════════════════════════════════════════════╝${NC}"
echo ""
echo -e "${GREEN}✓ Workspace built successfully${NC}"
echo -e "${GREEN}✓ Redis container running on port 6379${NC}"
echo -e "${GREEN}✓ Languages configured: Python, Java, Rust${NC}"
echo -e "${GREEN}✓ Docker images created for all languages${NC}"
echo ""
echo -e "${YELLOW}Next Steps:${NC}"
echo -e "${NC}  1. Start the API server:${NC}"
echo -e "${CYAN}     ./target/release/optimus-api${NC}"
echo ""
echo -e "${NC}  2. Start workers (in separate terminals):${NC}"
echo -e "${CYAN}     ./target/release/optimus-worker --language python${NC}"
echo -e "${CYAN}     ./target/release/optimus-worker --language java${NC}"
echo -e "${CYAN}     ./target/release/optimus-worker --language rust${NC}"
echo ""
echo -e "${NC}  3. Submit a job:${NC}"
echo -e "${CYAN}     curl -X POST http://localhost:8080/jobs -H 'Content-Type: application/json' -d @test_job.json${NC}"
echo ""
echo -e "${YELLOW}For more information, see README.md${NC}"
echo ""
