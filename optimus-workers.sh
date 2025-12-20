#!/bin/bash
# Optimus Worker Manager
# Manage multiple worker instances with dynamic scaling

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
GRAY='\033[0;90m'
BOLD='\033[1m'
NC='\033[0m'

# Configuration
PID_DIR="/tmp/optimus-workers"
WORKER_BINARY="./target/release/optimus-worker"

# Load .env file
if [ -f .env ]; then
    set -a
    source .env
    set +a
fi

# Ensure PID directory exists
mkdir -p "$PID_DIR"

# Language configurations
get_config() {
    local lang="$1"
    case "$lang" in
        python)
            LANG_QUEUE="optimus:queue:python"
            LANG_IMAGE="optimus-python:3.11-slim"
            PORT_BASE=8080
            ;;
        java)
            LANG_QUEUE="optimus:queue:java"
            LANG_IMAGE="optimus-java:17"
            PORT_BASE=8090
            ;;
        rust)
            LANG_QUEUE="optimus:queue:rust"
            LANG_IMAGE="optimus-rust:1.75-slim"
            PORT_BASE=8100
            ;;
        *)
            echo -e "${RED}Error: Unknown language '$lang'${NC}"
            echo "Supported: python, java, rust"
            return 1
            ;;
    esac
    return 0
}

# Check if a worker is running by PID file
is_running() {
    local pid_file="$1"
    if [ -f "$pid_file" ]; then
        local pid
        pid=$(cat "$pid_file" 2>/dev/null)
        if [ -n "$pid" ] && kill -0 "$pid" 2>/dev/null; then
            return 0
        fi
    fi
    return 1
}

# Count running workers for a language
count_workers() {
    local lang="$1"
    local count=0
    for f in "$PID_DIR/${lang}_"*.pid; do
        [ -f "$f" ] || continue
        if is_running "$f"; then
            count=$((count + 1))
        fi
    done
    echo "$count"
}

# Get next available instance number
get_next_instance() {
    local lang="$1"
    local i=1
    while [ -f "$PID_DIR/${lang}_${i}.pid" ]; do
        if ! is_running "$PID_DIR/${lang}_${i}.pid"; then
            rm -f "$PID_DIR/${lang}_${i}.pid"
            break
        fi
        i=$((i + 1))
    done
    echo "$i"
}

# Start a single worker instance
start_one() {
    local lang="$1"
    local inst="$2"
    
    get_config "$lang" || return 1
    
    local port=$((PORT_BASE + inst))
    local pid_file="$PID_DIR/${lang}_${inst}.pid"
    local log_file="$PID_DIR/${lang}_${inst}.log"
    
    if is_running "$pid_file"; then
        echo -e "${YELLOW}Worker $lang #$inst already running${NC}"
        return 0
    fi
    
    # Export all required environment variables
    export OPTIMUS_LANGUAGE="$lang"
    export OPTIMUS_QUEUE="$LANG_QUEUE"
    export OPTIMUS_IMAGE="$LANG_IMAGE"
    export HEALTH_PORT="$port"
    # REDIS_URL should already be set from .env, but ensure it's exported
    export REDIS_URL="${REDIS_URL:-redis://127.0.0.1:6380}"
    
    # Start worker in background with full environment
    nohup "$WORKER_BINARY" > "$log_file" 2>&1 &
    
    local pid=$!
    echo "$pid" > "$pid_file"
    
    echo -e "${GREEN}✓ Started $lang worker #$inst (PID: $pid, Port: $port)${NC}"
}

# Stop a single worker instance
stop_one() {
    local lang="$1"
    local inst="$2"
    local pid_file="$PID_DIR/${lang}_${inst}.pid"
    
    [ -f "$pid_file" ] || return 0
    
    local pid
    pid=$(cat "$pid_file" 2>/dev/null)
    
    if [ -n "$pid" ] && kill -0 "$pid" 2>/dev/null; then
        echo -e "${YELLOW}Stopping $lang worker #$inst (PID: $pid)...${NC}"
        kill -TERM "$pid" 2>/dev/null || true
        sleep 2
        kill -0 "$pid" 2>/dev/null && kill -9 "$pid" 2>/dev/null
        echo -e "${GREEN}✓ Stopped $lang worker #$inst${NC}"
    fi
    
    rm -f "$pid_file"
}

# Command: start
cmd_start() {
    local arg="$1"
    
    if [ "$arg" = "--all" ]; then
        echo -e "${CYAN}Starting all workers...${NC}"
        start_single "python" 1
        start_single "java" 1
        start_single "rust" 1
        return
    fi
    
    # Check if using lang:count format (multi-language mode)
    if echo "$arg" | grep -q ':'; then
        # Multi-language mode: python:3 java:2 rust:1
        for pair in "$@"; do
            local lang count
            lang=$(echo "$pair" | cut -d: -f1)
            count=$(echo "$pair" | cut -d: -f2)
            [ -z "$count" ] && count=1
            start_single "$lang" "$count"
        done
    else
        # Single language mode: python 3
        local lang="$1"
        local count="${2:-1}"
        start_single "$lang" "$count"
    fi
}

# Start workers for a single language
start_single() {
    local lang="$1"
    local count="${2:-1}"
    
    get_config "$lang" || return 1
    
    echo -e "${CYAN}Starting $count $lang worker(s)...${NC}"
    
    local i=1
    while [ "$i" -le "$count" ]; do
        local inst
        inst=$(get_next_instance "$lang")
        start_one "$lang" "$inst"
        sleep 0.5
        i=$((i + 1))
    done
    
    echo -e "${GREEN}✓ Started $count $lang worker(s)${NC}"
}

# Command: stop
cmd_stop() {
    local lang="$1"
    
    if [ "$lang" = "--all" ] || [ -z "$lang" ]; then
        echo -e "${CYAN}Stopping all workers...${NC}"
        for l in python java rust; do
            cmd_stop "$l"
        done
        return
    fi
    
    get_config "$lang" || exit 1
    
    echo -e "${CYAN}Stopping all $lang workers...${NC}"
    
    for f in "$PID_DIR/${lang}_"*.pid; do
        [ -f "$f" ] || continue
        local inst
        inst=$(basename "$f" | sed "s/${lang}_//" | sed 's/.pid//')
        stop_one "$lang" "$inst"
    done
    
    echo -e "${GREEN}✓ All $lang workers stopped${NC}"
}

# Command: scale
cmd_scale() {
    local lang="$1"
    local target="$2"
    
    get_config "$lang" || exit 1
    
    if [ -z "$target" ]; then
        echo -e "${RED}Error: Please specify target count${NC}"
        echo "Usage: $0 scale <language> <count>"
        exit 1
    fi
    
    local current
    current=$(count_workers "$lang")
    
    echo -e "${CYAN}Scaling $lang workers: $current → $target${NC}"
    
    if [ "$target" -gt "$current" ]; then
        local to_add=$((target - current))
        echo -e "${GREEN}Adding $to_add worker(s)...${NC}"
        local i=1
        while [ "$i" -le "$to_add" ]; do
            local inst
            inst=$(get_next_instance "$lang")
            start_one "$lang" "$inst"
            sleep 0.5
            i=$((i + 1))
        done
    elif [ "$target" -lt "$current" ]; then
        local to_remove=$((current - target))
        echo -e "${YELLOW}Removing $to_remove worker(s)...${NC}"
        
        # Get instances sorted descending
        local instances=""
        for f in "$PID_DIR/${lang}_"*.pid; do
            [ -f "$f" ] && is_running "$f" || continue
            local inst
            inst=$(basename "$f" | sed "s/${lang}_//" | sed 's/.pid//')
            instances="$instances $inst"
        done
        
        # Sort descending and stop
        local removed=0
        for inst in $(echo "$instances" | tr ' ' '\n' | sort -rn); do
            [ "$removed" -ge "$to_remove" ] && break
            stop_one "$lang" "$inst"
            removed=$((removed + 1))
        done
    else
        echo -e "${GREEN}Already at target ($current workers)${NC}"
    fi
    
    echo -e "${GREEN}✓ Scaling complete. $lang workers: $(count_workers "$lang")${NC}"
}

# Command: status
cmd_status() {
    echo -e "${CYAN}╔═══════════════════════════════════════════════════════════════╗${NC}"
    echo -e "${CYAN}║                  OPTIMUS WORKER STATUS                        ║${NC}"
    echo -e "${CYAN}╚═══════════════════════════════════════════════════════════════╝${NC}"
    echo ""
    
    printf "${BOLD}%-10s %-10s %-8s %-8s %-20s${NC}\n" "LANGUAGE" "INSTANCE" "PORT" "PID" "STATUS"
    echo "────────────────────────────────────────────────────────────"
    
    local total=0
    
    for lang in python java rust; do
        get_config "$lang"
        local found=0
        
        for f in "$PID_DIR/${lang}_"*.pid; do
            [ -f "$f" ] || continue
            found=1
            
            local inst
            inst=$(basename "$f" | sed "s/${lang}_//" | sed 's/.pid//')
            local port=$((PORT_BASE + inst))
            local pid
            pid=$(cat "$f" 2>/dev/null)
            
            if is_running "$f"; then
                printf "${GREEN}%-10s %-10s %-8s %-8s %-20s${NC}\n" "$lang" "#$inst" "$port" "$pid" "● Running"
                total=$((total + 1))
            else
                printf "${GRAY}%-10s %-10s %-8s %-8s %-20s${NC}\n" "$lang" "#$inst" "$port" "$pid" "○ Stale"
                rm -f "$f"
            fi
        done
        
        if [ "$found" -eq 0 ]; then
            printf "${GRAY}%-10s %-10s %-8s %-8s %-20s${NC}\n" "$lang" "-" "-" "-" "No workers"
        fi
    done
    
    echo ""
    echo -e "${BOLD}Total running workers: $total${NC}"
    echo ""
    echo -e "${CYAN}Quick commands:${NC}"
    echo "  Start:  $0 start python 3"
    echo "  Scale:  $0 scale python 5"
    echo "  Stop:   $0 stop python"
    echo "  Logs:   tail -f $PID_DIR/<lang>_<n>.log"
}

# Command: logs
cmd_logs() {
    local lang="${1:-python}"
    local inst="${2:-1}"
    local log_file="$PID_DIR/${lang}_${inst}.log"
    
    if [ ! -f "$log_file" ]; then
        echo -e "${RED}No log file found for $lang worker #$inst${NC}"
        exit 1
    fi
    
    echo -e "${CYAN}Logs for $lang worker #$inst ($log_file)${NC}"
    tail -f "$log_file"
}

# Show usage
usage() {
    echo -e "${CYAN}╔═══════════════════════════════════════════════════════════════╗${NC}"
    echo -e "${CYAN}║                  OPTIMUS WORKER MANAGER                       ║${NC}"
    echo -e "${CYAN}╚═══════════════════════════════════════════════════════════════╝${NC}"
    echo ""
    echo -e "${BOLD}USAGE:${NC}  $0 <command> [arguments]"
    echo ""
    echo -e "${BOLD}COMMANDS:${NC}"
    echo "  start <language> [count]       Start worker(s) for a language"
    echo "  start <lang:count> ...         Start multiple languages at once"
    echo "  start --all                    Start 1 worker for each language"
    echo "  stop <language>                Stop all workers for a language"
    echo "  stop --all                     Stop all workers"
    echo "  scale <language> <count>       Scale to exact number of workers"
    echo "  status                         Show status of all workers"
    echo "  logs <language> [instance]     Tail logs for a worker"
    echo ""
    echo -e "${BOLD}LANGUAGES:${NC}  python (8081+), java (8091+), rust (8101+)"
    echo ""
    echo -e "${BOLD}EXAMPLES:${NC}"
    echo "  $0 start python 3              # Start 3 Python workers"
    echo "  $0 start python:3 java:2       # Start 3 Python + 2 Java"
    echo "  $0 scale java 5                # Scale Java to 5 workers"
    echo "  $0 stop rust                   # Stop all Rust workers"
    echo "  $0 status                      # View all workers"
    echo ""
}

# Check worker binary exists
if [ ! -f "$WORKER_BINARY" ]; then
    echo -e "${RED}Error: Worker binary not found at $WORKER_BINARY${NC}"
    echo "Please run 'cargo build --workspace --release' first"
    exit 1
fi

# Main dispatch
case "${1:-}" in
    start)  shift; cmd_start "$@" ;;
    stop)   cmd_stop "${2:---all}" ;;
    scale)  cmd_scale "${2:-}" "${3:-}" ;;
    status) cmd_status ;;
    logs)   cmd_logs "${2:-python}" "${3:-1}" ;;
    --help|-h|"") usage ;;
    *)
        echo -e "${RED}Unknown command: $1${NC}"
        usage
        exit 1
        ;;
esac
