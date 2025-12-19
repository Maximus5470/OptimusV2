#!/usr/bin/env pwsh
# Optimus Setup Script for Windows (PowerShell)
# This script sets up the complete Optimus development environment

$ErrorActionPreference = "Stop"

Write-Host "=================================================================" -ForegroundColor Cyan
Write-Host "                   OPTIMUS SETUP SCRIPT                        " -ForegroundColor Cyan
Write-Host "              Distributed Code Execution Platform              " -ForegroundColor Cyan
Write-Host "=================================================================" -ForegroundColor Cyan
Write-Host ""

# Function to check if a command exists
function Test-Command {
    param($Command)
    $null = Get-Command $Command -ErrorAction SilentlyContinue
    return $?
}

# Function to print section headers
function Write-Section {
    param($Title)
    Write-Host ""
    Write-Host "=================================================================" -ForegroundColor Blue
    Write-Host " $Title" -ForegroundColor Yellow
    Write-Host "=================================================================" -ForegroundColor Blue
}

# Step 1: Check Prerequisites
Write-Section "STEP 1: Checking Prerequisites"

Write-Host "[*] Checking for Docker..." -ForegroundColor Cyan
if (-not (Test-Command "docker")) {
    Write-Host "[X] Docker is not installed!" -ForegroundColor Red
    Write-Host "  Please install Docker Desktop from: https://www.docker.com/products/docker-desktop" -ForegroundColor Yellow
    exit 1
}
Write-Host "[+] Docker found" -ForegroundColor Green

Write-Host "[*] Checking if Docker daemon is running..." -ForegroundColor Cyan
try {
    docker ps | Out-Null
    Write-Host "[+] Docker daemon is running" -ForegroundColor Green
} catch {
    Write-Host "[X] Docker daemon is not running!" -ForegroundColor Red
    Write-Host "  Please start Docker Desktop" -ForegroundColor Yellow
    exit 1
}

Write-Host "[*] Checking for Rust/Cargo..." -ForegroundColor Cyan
if (-not (Test-Command "cargo")) {
    Write-Host "[X] Cargo is not installed!" -ForegroundColor Red
    Write-Host "  Please install Rust from: https://rustup.rs" -ForegroundColor Yellow
    exit 1
}
Write-Host "[+] Cargo found" -ForegroundColor Green
cargo --version

# Step 2: Build Workspace
Write-Section "STEP 2: Building Optimus Workspace"

Write-Host "[*] Building all binaries in release mode..." -ForegroundColor Cyan
Write-Host "  This may take a few minutes on first run..." -ForegroundColor Yellow
cargo build --workspace --release

if ($LASTEXITCODE -ne 0) {
    Write-Host "[X] Build failed!" -ForegroundColor Red
    exit 1
}
Write-Host "[+] Workspace built successfully" -ForegroundColor Green

# Step 3: Setup Redis Container
Write-Section "STEP 3: Setting up Redis Container"

Write-Host "[*] Checking for existing redis-optimus container..." -ForegroundColor Cyan
$existingContainer = docker ps -a --filter "name=redis-optimus" --format "{{.Names}}"

if ($existingContainer -eq "redis-optimus") {
    Write-Host "  Container 'redis-optimus' already exists" -ForegroundColor Yellow
    Write-Host "[*] Removing existing container..." -ForegroundColor Cyan
    docker rm -f redis-optimus | Out-Null
}

Write-Host "[*] Creating Redis container (redis:8-alpine)..." -ForegroundColor Cyan
docker run -d `
    --name redis-optimus `
    -p 6379:6379 `
    redis:8-alpine

if ($LASTEXITCODE -ne 0) {
    Write-Host "[X] Failed to create Redis container!" -ForegroundColor Red
    exit 1
}

Write-Host "[+] Redis container 'redis-optimus' created and running on port 6379" -ForegroundColor Green

# Step 4: Configure Languages
Write-Section "STEP 4: Configuring Languages"

$languages = @(
    @{Name="python"; Ext="py"; Version="3.11-slim"; Memory=256; CPU=0.5},
    @{Name="java"; Ext="java"; Version="17"; Memory=512; CPU=1.0},
    @{Name="rust"; Ext="rs"; Version="1.75-slim"; Memory=512; CPU=1.0}
)

foreach ($lang in $languages) {
    Write-Host ""
    Write-Host "[*] Processing $($lang.Name) language..." -ForegroundColor Cyan
    
    # Check if language exists in config
    $configPath = "config\languages.json"
    $langExists = $false
    
    if (Test-Path $configPath) {
        $config = Get-Content $configPath -Raw | ConvertFrom-Json
        $langExists = $config.languages | Where-Object { $_.name -eq $lang.Name }
    }
    
    $imageName = "optimus-$($lang.Name):$($lang.Version)"
    
    if ($langExists) {
        Write-Host "  Language '$($lang.Name)' already configured" -ForegroundColor Yellow
        
        # Check if Docker image exists
        $imageExists = docker images -q $imageName
        
        if ($imageExists) {
            Write-Host "  Docker image '$imageName' already exists" -ForegroundColor Yellow
            Write-Host "[+] Skipping $($lang.Name) - already configured" -ForegroundColor Green
            continue
        } else {
            Write-Host "  Docker image '$imageName' not found" -ForegroundColor Yellow
            Write-Host "[*] Building Docker image for $($lang.Name)..." -ForegroundColor Cyan
            
            & .\target\release\optimus-cli.exe build-image --name $lang.Name
            
            if ($LASTEXITCODE -ne 0) {
                Write-Host "[X] Failed to build Docker image for $($lang.Name)!" -ForegroundColor Red
                exit 1
            }
            
            Write-Host "[+] Docker image built for $($lang.Name)" -ForegroundColor Green
        }
    } else {
        Write-Host "[*] Adding new language: $($lang.Name)..." -ForegroundColor Cyan
        
        & .\target\release\optimus-cli.exe add-lang `
            --name $lang.Name `
            --ext $lang.Ext `
            --version $lang.Version `
            --memory $lang.Memory `
            --cpu $lang.CPU
        
        if ($LASTEXITCODE -ne 0) {
            Write-Host "[X] Failed to add $($lang.Name)!" -ForegroundColor Red
            exit 1
        }
        
        Write-Host "[+] $($lang.Name) configured and Docker image built" -ForegroundColor Green
    }
}

# Step 5: Verify Setup
Write-Section "STEP 5: Verifying Setup"

Write-Host "[*] Listing configured languages..." -ForegroundColor Cyan
& .\target\release\optimus-cli.exe list-langs

Write-Host ""
Write-Host "[*] Checking Docker images..." -ForegroundColor Cyan
docker images | Select-String "optimus-"

Write-Host ""
Write-Host "[*] Checking Redis container..." -ForegroundColor Cyan
docker ps --filter "name=redis-optimus" --format "table {{.Names}}`t{{.Status}}`t{{.Ports}}"

# Final Summary
Write-Host ""
Write-Host "=================================================================" -ForegroundColor Green
Write-Host "                    SETUP COMPLETED!                           " -ForegroundColor Green
Write-Host "=================================================================" -ForegroundColor Green
Write-Host ""
Write-Host "[+] Workspace built successfully" -ForegroundColor Green
Write-Host "[+] Redis container running on port 6379" -ForegroundColor Green
Write-Host "[+] Languages configured: Python, Java, Rust" -ForegroundColor Green
Write-Host "[+] Docker images created for all languages" -ForegroundColor Green
Write-Host ""
Write-Host "Next Steps:" -ForegroundColor Yellow
Write-Host "  1. Start the API server:" -ForegroundColor White
Write-Host "     .\target\release\optimus-api.exe" -ForegroundColor Cyan
Write-Host ""
Write-Host "  2. Start workers (in separate terminals):" -ForegroundColor White
Write-Host "     .\target\release\optimus-worker.exe --language python" -ForegroundColor Cyan
Write-Host "     .\target\release\optimus-worker.exe --language java" -ForegroundColor Cyan
Write-Host "     .\target\release\optimus-worker.exe --language rust" -ForegroundColor Cyan
Write-Host ""
Write-Host "  3. Submit a job:" -ForegroundColor White
Write-Host "     Invoke-RestMethod -Method POST -Uri http://localhost:8080/jobs -Body (Get-Content test_job.json) -ContentType 'application/json'" -ForegroundColor Cyan
Write-Host ""
Write-Host "For more information, see README.md" -ForegroundColor Yellow
Write-Host ""
