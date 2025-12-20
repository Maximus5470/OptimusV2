#!/bin/bash

API_URL="http://localhost:4001"

# Colors for output
GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

run_test() {
    local lang="$1"
    local test_name="$2"
    local payload="$3"

    echo -e "${BLUE}[$lang] ${YELLOW}$test_name${NC}"

    # Submit job
    response=$(curl -s -X POST "$API_URL/execute" -H "Content-Type: application/json" -d "$payload")
    job_id=$(echo "$response" | jq -r '.job_id')

    if [ "$job_id" == "null" ] || [ -z "$job_id" ]; then
        echo -e "${RED}  Failed to submit job: $response${NC}"
        echo ""
        return
    fi

    echo "  Job ID: $job_id"

    # Poll for result (max 15 seconds)
    result_body=""
    for i in {1..15}; do
        sleep 1
        resp=$(curl -s -w "\n%{http_code}" "$API_URL/job/$job_id")
        http_code="${resp##*$'\n'}"
        body="${resp%$'\n'*}"

        if [ "$http_code" = "202" ]; then
            # Job pending
            echo -n "."
            continue
        elif [ "$http_code" = "200" ]; then
            # Job result available - check overall status
            status=$(echo "$body" | jq -r '.overall_status // .status // "unknown"')
            status_lower=$(echo "$status" | tr '[:upper:]' '[:lower:]')
            if [ "$status_lower" = "completed" ] || [ "$status_lower" = "failed" ] || [ "$status_lower" = "timedout" ] || [ "$status_lower" = "cancelled" ]; then
                result_body="$body"
                break
            else
                echo -n "."
                continue
            fi
        else
            echo ""
            echo -e "${RED}  Error fetching job: HTTP $http_code${NC}"
            echo "  $body"
            result_body="$body"
            break
        fi
    done
    echo ""

    if [ -z "$result_body" ]; then
        echo -e "${YELLOW}  No final result received (timed out polling)${NC}"
        echo ""
        return
    fi

    # Normalize and display result
    echo "$result_body" | jq '{status: (.overall_status // .status // "pending"), score: .score, max_score: .max_score, test_results: (.results // [] | map({id: .test_id, status: .status, stdout: .stdout, stderr: .stderr, time_ms: .execution_time_ms})) }'
    echo ""
}

echo "=========================================="
echo "    OPTIMUS LANGUAGE TEST SUITE"
echo "=========================================="
echo ""

# ==================== PYTHON TESTS ====================
echo -e "${GREEN}========== PYTHON TESTS ==========${NC}"
echo ""

run_test "Python" "✅ Success" '{
  "language": "python",
  "source_code": "print(input())",
  "test_cases": [{"id": 1, "input": "hello", "expected_output": "hello", "weight": 10}],
  "timeout_ms": 5000
}'

run_test "Python" "❌ Syntax Error" '{
  "language": "python",
  "source_code": "print(input(",
  "test_cases": [{"id": 1, "input": "hello", "expected_output": "hello", "weight": 10}],
  "timeout_ms": 5000
}'

run_test "Python" "❌ Runtime Error (Division by Zero)" '{
  "language": "python",
  "source_code": "x = 1/0",
  "test_cases": [{"id": 1, "input": "", "expected_output": "", "weight": 10}],
  "timeout_ms": 5000
}'

run_test "Python" "❌ Wrong Answer" '{
  "language": "python",
  "source_code": "print(\"wrong\")",
  "test_cases": [{"id": 1, "input": "hello", "expected_output": "hello", "weight": 10}],
  "timeout_ms": 5000
}'

run_test "Python" "❌ Timeout (Infinite Loop)" '{
  "language": "python",
  "source_code": "while True: pass",
  "test_cases": [{"id": 1, "input": "", "expected_output": "", "weight": 10}],
  "timeout_ms": 2000
}'

# ==================== JAVA TESTS ====================
echo -e "${GREEN}========== JAVA TESTS ==========${NC}"
echo ""

run_test "Java" "✅ Success" '{
  "language": "java",
  "source_code": "import java.util.Scanner;\npublic class Main {\n  public static void main(String[] args) {\n    Scanner sc = new Scanner(System.in);\n    System.out.println(sc.nextLine());\n  }\n}",
  "test_cases": [{"id": 1, "input": "hello", "expected_output": "hello", "weight": 10}],
  "timeout_ms": 10000
}'

run_test "Java" "❌ Compilation Error" '{
  "language": "java",
  "source_code": "public class Main {\n  public static void main(String[] args) {\n    System.out.println(\"hello\"\n  }\n}",
  "test_cases": [{"id": 1, "input": "", "expected_output": "hello", "weight": 10}],
  "timeout_ms": 10000
}'

run_test "Java" "❌ Runtime Error (NullPointerException)" '{
  "language": "java",
  "source_code": "public class Main {\n  public static void main(String[] args) {\n    String s = null;\n    System.out.println(s.length());\n  }\n}",
  "test_cases": [{"id": 1, "input": "", "expected_output": "", "weight": 10}],
  "timeout_ms": 10000
}'

run_test "Java" "❌ Wrong Answer" '{
  "language": "java",
  "source_code": "public class Main {\n  public static void main(String[] args) {\n    System.out.println(\"wrong\");\n  }\n}",
  "test_cases": [{"id": 1, "input": "hello", "expected_output": "hello", "weight": 10}],
  "timeout_ms": 10000
}'

run_test "Java" "❌ Timeout (Infinite Loop)" '{
  "language": "java",
  "source_code": "public class Main {\n  public static void main(String[] args) {\n    while(true) {}\n  }\n}",
  "test_cases": [{"id": 1, "input": "", "expected_output": "", "weight": 10}],
  "timeout_ms": 3000
}'

# ==================== RUST TESTS ====================
echo -e "${GREEN}========== RUST TESTS ==========${NC}"
echo ""

run_test "Rust" "✅ Success" '{
  "language": "rust",
  "source_code": "use std::io::{self, BufRead};\nfn main() {\n  let stdin = io::stdin();\n  let line = stdin.lock().lines().next().unwrap().unwrap();\n  println!(\"{}\", line);\n}",
  "test_cases": [{"id": 1, "input": "hello", "expected_output": "hello", "weight": 10}],
  "timeout_ms": 10000
}'

run_test "Rust" "❌ Compilation Error" '{
  "language": "rust",
  "source_code": "fn main() {\n  println!(\"hello\"\n}",
  "test_cases": [{"id": 1, "input": "", "expected_output": "hello", "weight": 10}],
  "timeout_ms": 10000
}'

run_test "Rust" "❌ Runtime Error (Panic)" '{
  "language": "rust",
  "source_code": "fn main() {\n  let v: Vec<i32> = vec![];\n  println!(\"{}\", v[0]);\n}",
  "test_cases": [{"id": 1, "input": "", "expected_output": "", "weight": 10}],
  "timeout_ms": 10000
}'

run_test "Rust" "❌ Wrong Answer" '{
  "language": "rust",
  "source_code": "fn main() {\n  println!(\"wrong\");\n}",
  "test_cases": [{"id": 1, "input": "hello", "expected_output": "hello", "weight": 10}],
  "timeout_ms": 10000
}'

run_test "Rust" "❌ Timeout (Infinite Loop)" '{
  "language": "rust",
  "source_code": "fn main() {\n  loop {}\n}",
  "test_cases": [{"id": 1, "input": "", "expected_output": "", "weight": 10}],
  "timeout_ms": 3000
}'

echo "=========================================="
echo "    TEST SUITE COMPLETE"
echo "=========================================="
